//! `happyview-javascript`: JavaScript as a HappyView interpreter plugin.
//! QuickJS-ng through `rquickjs`, compiled to `wasm32-wasip1`, behind the
//! SDK's `execute` and `validate` exports.

mod backend;
mod bridge;
mod budget;
mod builtins;
mod calls;
mod conformance;
mod convert;
mod ctx;
mod imports;
mod sandbox;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use happyview_plugin_sdk::{
    interpreter_plugin, ExecuteInput, ExecuteLimits, ExecuteOutput, PluginError, PluginInfo,
    ScriptErrorKind, ScriptValueKind, ValidateError, ValidateInput, ValidateOutput,
};
use rquickjs::prelude::Coerced;
use rquickjs::{Ctx, FromJs, Module, Promise, Value};

use backend::Backend;
use budget::Budget;
use calls::{Calls, ClearOnDrop, Unsettled};

const MISSING_HANDLE: &str = "script must export a default function";

/// Validation carries no operator limits, so it works from a fixed pair:
/// large enough for any file-scope work a script does while loading, small
/// enough that a loop at that scope ends rather than hanging the editor.
const VALIDATE_LIMITS: ExecuteLimits = ExecuteLimits {
    instructions: Some(1_000_000),
    memory_bytes: 64 * 1024 * 1024,
};

/// How many named imports validation learns before it gives up. Each costs
/// one more pass, and a script with more than this is not one a person
/// wrote.
const MAX_STUBBED_IMPORTS: usize = 256;

interpreter_plugin! {
    info: PluginInfo::new("happyview-javascript", "JavaScript", "0.1.0"),
    execute: execute_script,
    validate: validate_source,
}

// `removed_globals` is read by neither export. It is the host's list of
// HappyView v2's Lua globals, which the Lua plugin's guard turns into a
// sentence naming the codemod; no JavaScript script predates v3, so there is
// nothing for that sentence to send anyone to, and reading an undeclared
// name in a module is already a `ReferenceError` naming it.

fn execute_script(input: &ExecuteInput) -> Result<ExecuteOutput, PluginError> {
    execute_with(input, Rc::new(backend::Host))
}

fn execute_with(
    input: &ExecuteInput,
    backend: Rc<dyn Backend>,
) -> Result<ExecuteOutput, PluginError> {
    let (runtime, budget) = budget::runtime().map_err(interpreter_error)?;
    let calls = Rc::new(Calls::new(backend.clone()));
    runtime.set_loader(
        imports::Names,
        imports::Libraries::new(backend.clone(), &input.libraries, calls.clone()),
    );
    let context = sandbox::create(&runtime, backend.clone()).map_err(interpreter_error)?;
    budget::install(&runtime, &budget, &input.limits);

    context.with(|ctx| {
        let _clear = ClearOnDrop(&calls);
        run(&ctx, input, &backend, &calls, &budget)
    })
}

fn run<'js>(
    ctx: &Ctx<'js>,
    input: &ExecuteInput,
    backend: &Rc<dyn Backend>,
    calls: &Calls,
    budget: &Budget,
) -> Result<ExecuteOutput, PluginError> {
    let module = match Module::declare(ctx.clone(), sandbox::MODULE_NAME, input.source.as_str()) {
        Ok(module) => module,
        Err(e) => return Ok(script_failure(ctx, ScriptErrorKind::Syntax, e, budget)),
    };
    // File scope, top-level `await` included, settles through the same loop
    // a handler's promise does, before `handle` is ever called.
    let module = match module.eval() {
        Ok((module, evaluated)) => match wait(ctx, calls, &evaluated, budget, "the module")? {
            Ok(_) => module,
            Err(failure) => return Ok(failure),
        },
        Err(e) => return Ok(script_failure(ctx, ScriptErrorKind::Runtime, e, budget)),
    };
    let handle: Value = match module.get("default") {
        Ok(handle) => handle,
        Err(e) => return Ok(script_failure(ctx, ScriptErrorKind::Runtime, e, budget)),
    };
    let Some(handle) = handle.into_function() else {
        return Ok(failure(ScriptErrorKind::MissingHandle, MISSING_HANDLE));
    };

    // A ceiling small enough to refuse the arguments is the script's limit
    // being reached, not the interpreter failing.
    let arguments = convert::to_js(ctx, &input.input)
        .and_then(|argument| Ok((argument, ctx::build(ctx, &input.context, backend)?)));
    let returned = match arguments.and_then(|arguments| handle.call::<_, Value>(arguments)) {
        Ok(returned) => returned,
        Err(e) => return Ok(script_failure(ctx, ScriptErrorKind::Runtime, e, budget)),
    };
    let returned = match returned.clone().into_promise() {
        Some(promise) => match wait(ctx, calls, &promise, budget, "handle's promise")? {
            Ok(value) => value,
            Err(failure) => return Ok(failure),
        },
        None => returned,
    };

    // A run that reached a normal return with its budget spent caught the
    // interrupt's consequences somewhere — an `await` on a call the
    // interrupted code had started, say — and the limit wins regardless.
    if budget.is_spent() {
        return Ok(failure(ScriptErrorKind::Timeout, budget::SPENT));
    }

    let value_kind = value_kind(&returned);
    match convert::to_json(ctx, returned) {
        Ok(value) => Ok(ExecuteOutput::Returned { value, value_kind }),
        Err(e) => Ok(script_failure(ctx, ScriptErrorKind::Runtime, e, budget)),
    }
}

/// `promise`'s value, or the output the run ends with instead. The outer
/// error is the host refusing a wait, which is this plugin failing.
fn wait<'js>(
    ctx: &Ctx<'js>,
    calls: &Calls,
    promise: &Promise<'js>,
    budget: &Budget,
    what: &str,
) -> Result<Result<Value<'js>, ExecuteOutput>, PluginError> {
    match calls.run(ctx, promise, budget) {
        Ok(value) => Ok(Ok(value)),
        Err(Unsettled::Thrown(e)) => Ok(Err(script_failure(
            ctx,
            ScriptErrorKind::Runtime,
            e,
            budget,
        ))),
        Err(Unsettled::Spent) => Ok(Err(failure(ScriptErrorKind::Timeout, budget::SPENT))),
        Err(Unsettled::Stuck) => Ok(Err(failure(ScriptErrorKind::Runtime, never_settled(what)))),
        Err(Unsettled::Host(e)) => Err(e),
    }
}

/// The deadlock: the queue is dry, no call is in flight, and the promise is
/// still pending, so nothing can ever settle it.
fn never_settled(what: &str) -> String {
    format!("{what} never settled: nothing it is waiting on can still happen")
}

type Wanted = Rc<RefCell<BTreeMap<String, BTreeSet<String>>>>;

fn validate_source(input: &ValidateInput) -> Result<ValidateOutput, PluginError> {
    let wanted: Wanted = Rc::new(RefCell::new(BTreeMap::new()));
    for _ in 0..=MAX_STUBBED_IMPORTS {
        match validate_once(input, &wanted)? {
            Attempt::Done(output) => return Ok(output),
            Attempt::Wants(module, export) => {
                // A name already stubbed and still missing is not one more
                // pass can supply.
                if !wanted
                    .borrow_mut()
                    .entry(module.clone())
                    .or_default()
                    .insert(export.clone())
                {
                    return Ok(invalid(
                        ScriptErrorKind::Runtime,
                        None,
                        format!("could not find export '{export}' in module '{module}'"),
                    ));
                }
            }
        }
    }
    Ok(invalid(
        ScriptErrorKind::Runtime,
        None,
        format!("more than {MAX_STUBBED_IMPORTS} named imports"),
    ))
}

enum Attempt {
    Done(ValidateOutput),
    /// Linking failed for want of this export from this stubbed module.
    Wants(String, String),
}

/// Compile and evaluate file scope with every import a stub, then check for
/// a default export that is a function. Evaluating under a budget is the
/// point: a loop at file scope fails rather than hanging the editor.
fn validate_once(input: &ValidateInput, wanted: &Wanted) -> Result<Attempt, PluginError> {
    let (runtime, budget) = budget::runtime().map_err(interpreter_error)?;
    let backend: Rc<dyn Backend> = Rc::new(backend::Validating);
    let calls = Calls::new(backend.clone());
    runtime.set_loader(
        imports::Names,
        imports::Stubs {
            wanted: wanted.clone(),
        },
    );
    let context = sandbox::create(&runtime, backend).map_err(interpreter_error)?;
    budget::install(&runtime, &budget, &VALIDATE_LIMITS);

    context.with(|ctx| {
        let _clear = ClearOnDrop(&calls);
        let refused = |kind, e| -> Attempt {
            let thrown = describe(&ctx, e);
            if let Some((module, export)) = imports::missing_export(&thrown.text) {
                return Attempt::Wants(module, export);
            }
            let kind = if budget.is_spent() {
                ScriptErrorKind::Timeout
            } else {
                compile_kind(kind, &thrown)
            };
            Attempt::Done(invalid(kind, thrown.line, thrown.message))
        };
        let done = |kind, message: &str| Attempt::Done(invalid(kind, None, message));

        let module = match Module::declare(ctx.clone(), sandbox::MODULE_NAME, input.source.as_str())
        {
            Ok(module) => module,
            Err(e) => return Ok(refused(ScriptErrorKind::Syntax, e)),
        };
        let module = match module.eval() {
            Ok((module, evaluated)) => match calls.run(&ctx, &evaluated, &budget) {
                Ok(_) => module,
                Err(Unsettled::Thrown(e)) => return Ok(refused(ScriptErrorKind::Runtime, e)),
                Err(Unsettled::Spent) => {
                    return Ok(done(ScriptErrorKind::Timeout, budget::SPENT));
                }
                Err(Unsettled::Stuck) => {
                    return Ok(done(ScriptErrorKind::Runtime, &never_settled("the module")));
                }
                Err(Unsettled::Host(e)) => return Err(e),
            },
            Err(e) => return Ok(refused(ScriptErrorKind::Runtime, e)),
        };
        Ok(match module.get::<_, Value>("default") {
            Ok(handle) if handle.is_function() => Attempt::Done(ValidateOutput {
                valid: true,
                errors: Vec::new(),
            }),
            Ok(_) => done(ScriptErrorKind::MissingHandle, MISSING_HANDLE),
            Err(e) => refused(ScriptErrorKind::Runtime, e),
        })
    })
}

/// The host branches three ways on what a script returned, and a JSON `null`
/// cannot tell a returned nothing from a returned null — so both of
/// JavaScript's nothings are "nothing", and the host is never asked to tell
/// them apart.
fn value_kind(value: &Value) -> ScriptValueKind {
    if value.is_undefined() || value.is_null() {
        ScriptValueKind::None
    } else if value.is_object() {
        ScriptValueKind::Object
    } else {
        ScriptValueKind::Other
    }
}

fn failure(kind: ScriptErrorKind, message: impl Into<String>) -> ExecuteOutput {
    let message = message.into();
    ExecuteOutput::Error {
        kind,
        raw: message.clone(),
        message,
        line: None,
    }
}

/// A failure the script earned. `kind` is read from the run rather than
/// from the text: a spent budget may finally surface as whatever error the
/// script was in the middle of.
fn script_failure(
    ctx: &Ctx<'_>,
    default: ScriptErrorKind,
    error: rquickjs::Error,
    budget: &Budget,
) -> ExecuteOutput {
    let thrown = describe(ctx, error);
    let default = compile_kind(default, &thrown);
    let (kind, message) = if budget.is_spent() {
        (ScriptErrorKind::Timeout, budget::SPENT.to_string())
    } else if thrown.memory && budget.refused_memory() {
        (ScriptErrorKind::Memory, budget::OUT_OF_MEMORY.to_string())
    } else {
        (default, thrown.message)
    };
    ExecuteOutput::Error {
        kind,
        message,
        line: thrown.line,
        raw: thrown.raw,
    }
}

/// Why compiling failed. QuickJS resolves and loads a module's imports while
/// compiling it, so a compile can fail for a reason that is not the
/// script's syntax — an import nothing serves — and calling that a syntax
/// error would send an author looking for a missing brace. Only a
/// `SyntaxError` is `syntax`; anything else is `runtime`, as anything else
/// raised while a Lua chunk loads is.
fn compile_kind(default: ScriptErrorKind, thrown: &Thrown) -> ScriptErrorKind {
    match default {
        ScriptErrorKind::Syntax if !thrown.syntax => ScriptErrorKind::Runtime,
        other => other,
    }
}

/// What was thrown, read into text while the context still holds it.
struct Thrown {
    /// An error's own `message`, with no name: what QuickJS's link failures
    /// are matched on.
    text: String,
    /// What a caller is shown: the message, prefixed with the error's name
    /// unless that is plain `Error`, so `throw new Error("boom")` reads as
    /// `boom` and a `TypeError` says that it is one.
    message: String,
    line: Option<u32>,
    /// The name, message and stack whole, for the event log.
    raw: String,
    /// Whether this is what QuickJS throws when an allocation is refused:
    /// an `InternalError` saying so when it can allocate one, and a bare
    /// `null` when it cannot. A script can throw either, so it counts only
    /// alongside the ceiling's own record of a refusal.
    memory: bool,
    /// Whether it is a `SyntaxError`.
    syntax: bool,
}

impl Thrown {
    fn plain(text: String, memory: bool) -> Self {
        Self {
            message: text.clone(),
            raw: text.clone(),
            text,
            line: None,
            memory,
            syntax: false,
        }
    }
}

fn describe(ctx: &Ctx<'_>, error: rquickjs::Error) -> Thrown {
    match error {
        rquickjs::Error::Exception => describe_value(ctx, ctx.catch()),
        rquickjs::Error::Allocation => Thrown::plain(budget::OUT_OF_MEMORY.to_string(), true),
        other => Thrown::plain(other.to_string(), false),
    }
}

fn describe_value<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> Thrown {
    let read = |name: &str| -> Option<String> {
        let object = value.as_object()?;
        match object.get::<_, Option<Coerced<String>>>(name) {
            Ok(text) => text.map(|text| text.0),
            Err(_) => {
                // A getter that throws while an error is being read leaves
                // its own exception behind; it is not the one being reported.
                ctx.catch();
                None
            }
        }
    };

    if value.is_error() {
        let name = read("name").unwrap_or_else(|| "Error".to_string());
        let text = read("message").unwrap_or_default();
        let stack = read("stack").unwrap_or_default();
        let message = if name == "Error" {
            text.clone()
        } else {
            format!("{name}: {text}")
        };
        let mut raw = format!("{name}: {text}");
        if !stack.trim().is_empty() {
            raw.push('\n');
            raw.push_str(stack.trim_end());
        }
        return Thrown {
            syntax: name == "SyntaxError",
            line: line_of(&stack),
            memory: name == "InternalError" && text == budget::OUT_OF_MEMORY,
            text,
            message,
            raw,
        };
    }

    // `throw "text"`, or anything else that is not an `Error`: its own
    // string form, and no stack to place it.
    let memory = value.is_null();
    let text = match Coerced::<String>::from_js(ctx, value) {
        Ok(text) => text.0,
        Err(_) => {
            ctx.catch();
            "a value with no string form was thrown".to_string()
        }
    };
    Thrown::plain(text, memory)
}

/// The line of the innermost frame in the script's own module, from a stack
/// QuickJS writes as `    at handle (script:12:5)` or `    at script:12:5`.
/// A frame in a library's module or in a native function is skipped, so an
/// error thrown by the bridge is placed at the line that called it.
fn line_of(stack: &str) -> Option<u32> {
    let marker = format!("{}:", sandbox::MODULE_NAME);
    stack.lines().find_map(|frame| {
        let frame = frame.trim();
        let location = match frame.rfind(&format!("({marker}")) {
            Some(at) => &frame[at + 1..],
            None => frame
                .strip_prefix("at ")
                .filter(|rest| rest.starts_with(&marker))?,
        };
        location[marker.len()..]
            .split(|c: char| !c.is_ascii_digit())
            .next()?
            .parse()
            .ok()
    })
}

fn invalid(kind: ScriptErrorKind, line: Option<u32>, message: impl Into<String>) -> ValidateOutput {
    ValidateOutput {
        valid: false,
        errors: vec![ValidateError {
            kind,
            line,
            message: message.into(),
        }],
    }
}

/// A runtime that cannot be built is the interpreter failing, not the
/// script, so it leaves as an envelope error rather than as a script result.
fn interpreter_error(e: rquickjs::Error) -> PluginError {
    PluginError::new("INTERPRETER_ERROR", e.to_string())
}

#[cfg(test)]
mod tests;
