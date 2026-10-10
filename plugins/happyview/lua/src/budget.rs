//! The two limits the interpreter owns: Lua instructions, and mlua's own
//! allocator ceiling. Wall-clock time is the host's, as an epoch deadline
//! armed before this module is ever entered, so there is no clock here.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use happyview_plugin_sdk::ExecuteLimits;
use mlua::{Lua, Result as LuaResult};

/// Wraps every Lua function that can hand a script control it should no
/// longer have. `pcall` and `xpcall` catch the hook's raise; `coroutine.resume`
/// catches it when it fires inside a script's own coroutine, and a
/// `coroutine.wrap` function does the same. Each wrapper re-raises after the
/// call it protects returns, whatever it returned, so a spent budget walks out
/// to the top of `handle` through every layer of catching. They are Lua so
/// that the host calls' yields still cross them.
const CATCH_GUARDS: &str = r#"
local tripped, raise = ...
local pack, unpack = table.pack, table.unpack
local function guarded(f)
    return function(...)
        local results = pack(f(...))
        if tripped() then raise() end
        return unpack(results, 1, results.n)
    end
end
pcall = guarded(pcall)
xpcall = guarded(xpcall)
coroutine.resume = guarded(coroutine.resume)
local wrap = coroutine.wrap
coroutine.wrap = function(f) return guarded(wrap(f)) end
return coroutine.resume
"#;

/// Where the guarded `coroutine.resume` is kept for `internal.async`, whose
/// scheduler resumes through it. Kept apart from the global, which a script
/// may reassign.
pub const RESUME_KEY: &str = "happyview.budget.resume";

/// The message a spent budget raises. The host writes the sentence a caller
/// sees; this is what reaches the event log.
pub const SPENT: &str = "script exceeded execution limit";

fn spent() -> mlua::Error {
    mlua::Error::runtime(SPENT)
}

/// Whether this run's instruction budget has been spent. Shared with the
/// hook, and read once the run ends to tell a spent budget from any other
/// failure — the error a script finally raises may say something else
/// entirely.
#[derive(Clone)]
pub struct Budget {
    spent: Arc<AtomicBool>,
}

impl Budget {
    pub fn is_spent(&self) -> bool {
        self.spent.load(Ordering::Relaxed)
    }
}

/// Arm both limits. `instructions` of `None` installs no hook, which is the
/// job path's exemption; the catch guards go in either way, so `pcall` and
/// its neighbours behave identically whether or not a budget is armed.
pub fn install(lua: &Lua, limits: &ExecuteLimits) -> LuaResult<Budget> {
    let budget = Budget {
        spent: Arc::new(AtomicBool::new(false)),
    };

    if let Some(every) = limits.instructions {
        let spent_flag = Arc::clone(&budget.spent);
        // Global rather than per-thread: a script's own coroutines inherit
        // only a global hook. Once tripped it raises at every trigger, so a
        // `pcall` that swallows the first raise buys one more stretch and no
        // more; the guards below turn even that into an immediate re-raise.
        lua.set_global_hook(
            mlua::HookTriggers::new().every_nth_instruction(every),
            move |_, _| {
                spent_flag.store(true, Ordering::Relaxed);
                Err(spent())
            },
        )?;
    }

    let spent_flag = Arc::clone(&budget.spent);
    let tripped = lua.create_function(move |_, ()| Ok(spent_flag.load(Ordering::Relaxed)))?;
    let raise = lua.create_function(|_, ()| -> LuaResult<()> { Err(spent()) })?;
    let resume: mlua::Function = lua
        .load(CATCH_GUARDS)
        .set_name("=catch_guards")
        .call((tripped, raise))?;
    lua.set_named_registry_value(RESUME_KEY, resume)?;

    // mlua's own ceiling, which refuses an allocation as a Lua error rather
    // than as a trap. The host sets a wasm-level one above it, so the clean
    // error is the one a script sees.
    if limits.memory_bytes > 0 {
        lua.set_memory_limit(limits.memory_bytes as usize)?;
    }

    Ok(budget)
}

/// Whether a failure is the allocator's refusal, wherever it was wrapped.
pub fn is_memory_error(error: &mlua::Error) -> bool {
    match error {
        mlua::Error::MemoryError(_) => true,
        mlua::Error::CallbackError { cause, .. } => is_memory_error(cause),
        mlua::Error::WithContext { cause, .. } => is_memory_error(cause),
        _ => false,
    }
}
