//! The two limits the interpreter owns: Lua instructions, and mlua's own
//! allocator ceiling. Wall-clock time is the host's, as an epoch deadline
//! armed before this module is ever entered, so there is no clock here.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// How often the hook counts, at most. Lua tells a hook only that `n`
/// instructions have passed, never how many since the last call, so a count
/// is a number of firings and is known to within one step. Smaller is more
/// exact and costs a call into the host's side of mlua per step.
const STEP: u32 = 1_000;

/// Below this the hook would fire so often that counting costs more than it
/// tells, so the budget is enforced with one firing and nothing is counted.
const MIN_STEP: u32 = 100;

/// The largest step that divides `budget` evenly, so the firing that spends it
/// lands on its last instruction exactly as a single firing at `budget` would,
/// or `None` when that step would be under [`MIN_STEP`]. Every budget a
/// multiple of 1,000, the default among them, counts in steps of 1,000.
fn step_for(budget: u32) -> Option<u32> {
    (MIN_STEP..=STEP.min(budget))
        .rev()
        .find(|step| budget.is_multiple_of(*step))
}

/// Whether this run's instruction budget has been spent, and how much of it
/// the run used. Shared with the hook, and read once the run ends to tell a
/// spent budget from any other failure — the error a script finally raises
/// may say something else entirely.
#[derive(Clone)]
pub struct Budget {
    spent: Arc<AtomicBool>,
    /// The budget and the step it is counted in, when it is counted.
    counted: Option<(u32, u32)>,
    fired: Arc<AtomicU64>,
}

impl Budget {
    pub fn is_spent(&self) -> bool {
        self.spent.load(Ordering::Relaxed)
    }

    /// The instructions the run executed: the whole budget once it is spent,
    /// otherwise the middle of the step the run ended in, so a mean over many
    /// runs comes out right. `None` for a run without a budget (a job), or one
    /// whose budget is not counted.
    pub fn used(&self) -> Option<u64> {
        let (budget, step) = self.counted?;
        if self.is_spent() {
            return Some(u64::from(budget));
        }
        let fired = self.fired.load(Ordering::Relaxed);
        Some(fired * u64::from(step) + u64::from(step / 2))
    }
}

/// Arm both limits. `instructions` of `None` installs no hook, which is the
/// job path's exemption; the catch guards go in either way, so `pcall` and
/// its neighbours behave identically whether or not a budget is armed.
pub fn install(lua: &Lua, limits: &ExecuteLimits) -> LuaResult<Budget> {
    let counted = limits
        .instructions
        .and_then(|budget| step_for(budget).map(|step| (budget, step)));
    install_counted(lua, limits, counted)
}

/// [`install`] with the counting chosen, so a test can arm one budget both
/// ways.
fn install_counted(
    lua: &Lua,
    limits: &ExecuteLimits,
    counted: Option<(u32, u32)>,
) -> LuaResult<Budget> {
    let budget = Budget {
        spent: Arc::new(AtomicBool::new(false)),
        counted,
        fired: Arc::new(AtomicU64::new(0)),
    };

    if let Some(limit) = limits.instructions {
        let spent_flag = Arc::clone(&budget.spent);
        let fired = Arc::clone(&budget.fired);
        let (every, firings) = match counted {
            Some((_, step)) => (step, u64::from(limit / step)),
            None => (limit, 1),
        };
        // Global rather than per-thread: a script's own coroutines inherit
        // only a global hook. Once tripped it raises at every trigger, so a
        // `pcall` that swallows the first raise buys one more step and no
        // more; the guards below turn even that into an immediate re-raise.
        lua.set_global_hook(
            mlua::HookTriggers::new().every_nth_instruction(every),
            move |_, _| {
                if fired.fetch_add(1, Ordering::Relaxed) + 1 < firings
                    && !spent_flag.load(Ordering::Relaxed)
                {
                    return Ok(mlua::VmState::Continue);
                }
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
        .set_mode(mlua::chunk::ChunkMode::Text)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(instructions: u32) -> ExecuteLimits {
        ExecuteLimits {
            instructions: Some(instructions),
            memory_bytes: 0,
        }
    }

    /// Runs `source` under `budget`, counted in `step` or with one firing, and
    /// answers the global `n` it reached and the budget.
    fn run(source: &str, budget: u32, step: Option<u32>) -> (i64, Budget) {
        let lua = Lua::new();
        let armed = install_counted(&lua, &limits(budget), step.map(|s| (budget, s))).unwrap();
        let _ = lua.load(source).exec();
        (lua.globals().get::<i64>("n").unwrap_or(0), armed)
    }

    /// The instructions `source` executes, counted one at a time.
    fn exact(source: &str) -> u64 {
        let lua = Lua::new();
        let count = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&count);
        lua.set_global_hook(
            mlua::HookTriggers::new().every_nth_instruction(1),
            move |_, _| {
                counter.fetch_add(1, Ordering::Relaxed);
                Ok(mlua::VmState::Continue)
            },
        )
        .unwrap();
        lua.load(source).exec().unwrap();
        count.load(Ordering::Relaxed)
    }

    #[test]
    fn a_step_divides_its_budget_or_nothing_is_counted() {
        assert_eq!(step_for(1_000_000), Some(1_000));
        assert_eq!(step_for(10_000), Some(1_000));
        assert_eq!(step_for(1_500), Some(750));
        assert_eq!(step_for(150), Some(150));
        assert_eq!(step_for(1_234_567), Some(127));
        // A prime has no step worth the calls.
        assert_eq!(step_for(1_000_003), None);
        assert_eq!(step_for(99), None);
    }

    /// Counting must not move where a budget ends: the firing that spends it
    /// lands on the same instruction a single firing at the budget does.
    #[test]
    fn a_counted_budget_stops_on_the_same_instruction_as_a_single_firing() {
        let source = "n = 0 while true do n = n + 1 end";
        for budget in [1_000, 10_000, 1_234_567] {
            let step = step_for(budget).unwrap();
            let (counted, counted_budget) = run(source, budget, Some(step));
            let (single, single_budget) = run(source, budget, None);
            assert_eq!(counted, single, "budget {budget}");
            assert!(counted > 0);
            assert!(counted_budget.is_spent() && single_budget.is_spent());
            assert_eq!(counted_budget.used(), Some(u64::from(budget)));
            assert_eq!(single_budget.used(), None);
        }
    }

    #[test]
    fn a_run_that_finishes_reports_within_half_a_step() {
        for iterations in [0, 10, 333, 5_000, 120_000] {
            let source = format!("n = 0 for i = 1, {iterations} do n = n + i end");
            let truth = exact(&source);
            let (_, armed) = run(&source, 1_000_000, Some(1_000));
            assert!(!armed.is_spent());
            let used = armed.used().unwrap();
            assert!(
                used.abs_diff(truth) <= 500,
                "{iterations} iterations: {used} against {truth}"
            );
        }
    }

    #[test]
    fn a_run_without_a_budget_counts_nothing() {
        let lua = Lua::new();
        let armed = install(
            &lua,
            &ExecuteLimits {
                instructions: None,
                memory_bytes: 0,
            },
        )
        .unwrap();
        lua.load("for i = 1, 10000 do end").exec().unwrap();
        assert_eq!(armed.used(), None);
    }

    /// What counting costs against a single firing, printed rather than
    /// asserted, since it is a property of the machine. `--ignored --nocapture`
    /// under `--release`.
    #[test]
    #[ignore]
    fn counting_overhead() {
        let source = "n = 0 for i = 1, 20000000 do n = n + i end";
        let time = |step: Option<u32>| {
            let started = std::time::Instant::now();
            let (_, armed) = run(source, 1_000_000_000, step);
            assert!(!armed.is_spent());
            started.elapsed()
        };
        for _ in 0..3 {
            let single = time(None);
            let counted = time(Some(1_000));
            println!(
                "single {single:?}, counted {counted:?}: {:+.2}%",
                (counted.as_secs_f64() / single.as_secs_f64() - 1.0) * 100.0
            );
        }
    }
}
