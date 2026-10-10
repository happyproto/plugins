//! The limits the interpreter owns: an instruction budget, a memory ceiling
//! and a stack ceiling. Wall-clock time is the host's, as an epoch deadline
//! armed before this module is ever entered, so there is no clock here.

use std::cell::Cell;
use std::rc::Rc;

use happyview_plugin_sdk::ExecuteLimits;
use rquickjs::allocator::{Allocator, RustAllocator};
use rquickjs::Runtime;

/// The message a spent budget fails with. The host writes the sentence a
/// caller sees; this is what reaches the event log.
pub const SPENT: &str = "script exceeded execution limit";

/// The message a refused allocation fails with, whatever QuickJS managed to
/// throw — at the ceiling it often cannot allocate the error it means to.
pub const OUT_OF_MEMORY: &str = "out of memory";

/// QuickJS calls its interrupt handler once every this many interrupt
/// checks, and it checks on every loop back-edge and every call — so a check
/// is what this plugin counts as an instruction, and the budget is spent in
/// steps of this size. Lua's hook counts VM instructions instead; the two are
/// the same order of magnitude per line of script, which is all an operator's
/// number can mean across two engines.
const POLL_INTERVAL: u64 = 10_000;

/// QuickJS's own stack ceiling, so deep recursion is a `RangeError` the script
/// can catch rather than a trap.
///
/// What QuickJS measures is the shadow stack in linear memory, the 1 MiB
/// rustc gives a wasm module; the rest of that is what the Rust frames around
/// the engine and a host call's encoding need. But the stack that actually
/// runs out is the host's native one, which QuickJS cannot see and which
/// every level of recursion costs far more of — over twenty times as much in
/// QuickJS-ng's parser. So this is sized against the host's native stack
/// (`MAX_WASM_STACK`, 16 MiB in HappyView) as much as against its own: at 384
/// KiB every shape measured on the real host, nested source and JSON
/// included, ends in this error first, and ordinary recursion a thousand deep
/// still runs. A host still on wasmtime's 512 KiB default runs out before
/// this ceiling is reached, so there runaway recursion traps as it did
/// without one.
///
/// Upstream QuickJS-ng compiles this check out under WASI; the workspace
/// patches `rquickjs-sys` to a fork that keeps it (see the root
/// `Cargo.toml`).
const STACK_BYTES: usize = 384 * 1024;

/// What this run has spent, shared with the interrupt handler and the
/// allocator, and read once the run ends to tell a spent budget or a refused
/// allocation from any other failure — the error a script finally fails with
/// may say something else entirely.
#[derive(Clone)]
pub struct Budget {
    spent: Rc<Cell<bool>>,
    memory: Rc<Memory>,
}

impl Budget {
    pub fn is_spent(&self) -> bool {
        self.spent.get()
    }

    /// Whether the ceiling has refused an allocation at any point in the run.
    pub fn refused_memory(&self) -> bool {
        self.memory.refused.get()
    }
}

#[derive(Default)]
struct Memory {
    /// Zero, which is no ceiling, until [`install`] arms it.
    limit: Cell<usize>,
    used: Cell<usize>,
    refused: Cell<bool>,
}

impl Memory {
    /// Whether `more` bytes fit, recording the refusal when they do not.
    fn admits(&self, more: usize) -> bool {
        let limit = self.limit.get();
        if limit > 0 && self.used.get().saturating_add(more) > limit {
            self.refused.set(true);
            return false;
        }
        true
    }
}

/// QuickJS's allocations, counted against the ceiling. QuickJS can enforce a
/// limit of its own, but what it raises at that limit is an `InternalError`
/// when it can allocate one and a bare `null` when it cannot, which a script
/// can also throw; counting here is what lets a refusal be read from the run.
pub struct Ceiling {
    inner: RustAllocator,
    memory: Rc<Memory>,
}

// SAFETY: every pointer handed out is `RustAllocator`'s own, unchanged, so
// its guarantees are this allocator's; the counting only declines to call it.
unsafe impl Allocator for Ceiling {
    fn alloc(&mut self, size: usize) -> *mut u8 {
        if !self.memory.admits(size) {
            return std::ptr::null_mut();
        }
        let ptr = self.inner.alloc(size);
        if !ptr.is_null() {
            // SAFETY: `ptr` was just returned by `inner`.
            let size = unsafe { RustAllocator::usable_size(ptr) };
            self.memory.used.set(self.memory.used.get() + size);
        }
        ptr
    }

    fn calloc(&mut self, count: usize, size: usize) -> *mut u8 {
        if !self.memory.admits(count.saturating_mul(size)) {
            return std::ptr::null_mut();
        }
        let ptr = self.inner.calloc(count, size);
        if !ptr.is_null() {
            // SAFETY: `ptr` was just returned by `inner`.
            let size = unsafe { RustAllocator::usable_size(ptr) };
            self.memory.used.set(self.memory.used.get() + size);
        }
        ptr
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8) {
        let size = RustAllocator::usable_size(ptr);
        self.memory
            .used
            .set(self.memory.used.get().saturating_sub(size));
        self.inner.dealloc(ptr);
    }

    unsafe fn realloc(&mut self, ptr: *mut u8, new_size: usize) -> *mut u8 {
        let old = RustAllocator::usable_size(ptr);
        if !self.memory.admits(new_size.saturating_sub(old)) {
            return std::ptr::null_mut();
        }
        let moved = self.inner.realloc(ptr, new_size);
        if !moved.is_null() {
            let new = RustAllocator::usable_size(moved);
            self.memory
                .used
                .set(self.memory.used.get().saturating_sub(old) + new);
        }
        moved
    }

    unsafe fn usable_size(ptr: *mut u8) -> usize {
        RustAllocator::usable_size(ptr)
    }
}

/// A runtime whose allocations are counted, and the budget that will read
/// them. Nothing is limited until [`install`].
pub fn runtime() -> rquickjs::Result<(Runtime, Budget)> {
    let memory = Rc::new(Memory::default());
    let runtime = Runtime::new_with_alloc(Ceiling {
        inner: RustAllocator,
        memory: memory.clone(),
    })?;
    let budget = Budget {
        spent: Rc::new(Cell::new(false)),
        memory,
    };
    Ok((runtime, budget))
}

/// Arm every limit. `instructions` of `None` installs no handler, which is
/// the job path's exemption.
///
/// The interrupt QuickJS raises when the handler answers `true` is
/// uncatchable: `catch` and `finally` are both skipped, and an async function
/// interrupted mid-run leaves its promise pending rather than rejecting it.
/// Once spent the handler keeps answering `true`, so any code that does run
/// afterwards is interrupted at its next check; the event loop stops at the
/// first sign of a spent budget, and the run fails as a timeout whatever the
/// script went on to do.
pub fn install(runtime: &Runtime, budget: &Budget, limits: &ExecuteLimits) {
    if let Some(instructions) = limits.instructions {
        let allowed = (u64::from(instructions) / POLL_INTERVAL).max(1);
        let spent = Rc::clone(&budget.spent);
        let mut polls = 0u64;
        runtime.set_interrupt_handler(Some(Box::new(move || {
            polls += 1;
            if polls >= allowed {
                spent.set(true);
            }
            spent.get()
        })));
    }

    // Armed after the context is built, so a ceiling too small for the
    // standard library fails the script as a memory error rather than
    // failing to build the interpreter. The host sets a wasm-level ceiling
    // above it, so the clean error is the one a script sees.
    if limits.memory_bytes > 0 {
        let limit = usize::try_from(limits.memory_bytes).unwrap_or(usize::MAX);
        budget.memory.limit.set(limit);
    }
    runtime.set_max_stack_size(STACK_BYTES);
}
