//! The modules `import` serves without a plugin: the ones that *are* the
//! host, or that a wasm round trip on a script's hottest path would only slow
//! down. They live under `internal.` so a published plugin can never shadow
//! one, and they are the Lua plugin's four with the same semantics — less
//! `json.to_array`, which exists only because a Lua table cannot say whether
//! it is empty as an array or as a map.

use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use happyview_plugin_sdk::host::Level;
use happyview_plugin_sdk::tid::{
    tid_from_parts, tid_from_unix_microseconds, tid_to_unix_microseconds,
};
use happyview_plugin_sdk::ScriptLogRequest;
use rquickjs::function::{Opt, Rest};
use rquickjs::prelude::Coerced;
use rquickjs::{Ctx, Exception, FromJs, Function, Object, Result, Value};

use crate::backend::Backend;
use crate::convert;

pub const PREFIX: &str = "internal.";

/// Every built-in name, for lookup and for naming them in an unknown-module
/// error. One list, so a new built-in cannot be added to the match below and
/// forgotten here.
pub const MODULES: [&str; 4] = [
    "internal.logging",
    "internal.time",
    "internal.tids",
    "internal.json",
];

/// The module's exports as one object, or `None` for a name that is not a
/// built-in.
pub fn module<'js>(
    ctx: &Ctx<'js>,
    name: &str,
    backend: &Rc<dyn Backend>,
) -> Result<Option<Object<'js>>> {
    if !MODULES.contains(&name) {
        return Ok(None);
    }
    let suffix = name
        .strip_prefix(PREFIX)
        .expect("every entry in MODULES carries the prefix");
    Ok(Some(match suffix {
        "logging" => logging(ctx, backend)?,
        "time" => time(ctx)?,
        "tids" => tids(ctx)?,
        "json" => json(ctx)?,
        _ => unreachable!("MODULES and this match must name the same suffixes"),
    }))
}

/// Microseconds since the epoch, from the one clock in this VM: `Date`,
/// `performance` and this all read WASI's.
///
/// Microseconds rather than milliseconds because a TID is
/// `(microseconds << 10) | clock_id`: at millisecond resolution every TID
/// minted in the same millisecond differs only in the ten random bits, so
/// they collide about once in a thousand pairs and sort in random order —
/// and a TID is a record key.
fn now_micros() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_micros() as i64,
        Err(before) => -(before.duration().as_micros() as i64),
    }
}

/// A plain `Error` carrying `message` whole. rquickjs's typed throws cut a
/// message at 255 bytes, which would cut a library's own explanation short.
pub fn throw_error(ctx: &Ctx<'_>, message: &str) -> rquickjs::Error {
    Exception::throw_message(ctx, message)
}

/// The message of whatever was just thrown, for re-raising it under a label.
fn caught_message(ctx: &Ctx<'_>, error: rquickjs::Error) -> String {
    match error {
        rquickjs::Error::Exception => {
            let thrown = ctx.catch();
            thrown
                .as_object()
                .and_then(|object| object.get::<_, Coerced<String>>("message").ok())
                .map(|message| message.0)
                .unwrap_or_else(|| "uncaught exception".to_string())
        }
        other => other.to_string(),
    }
}

fn send_log(
    backend: &Rc<dyn Backend>,
    level: Level,
    message: String,
    fields: Option<serde_json::Value>,
) {
    // A log line that cannot be written must not end the run: the host's own
    // write is best-effort for the same reason, and a script has nothing
    // useful to do about it.
    let _ = backend.script_log(&ScriptLogRequest {
        level,
        message,
        fields,
    });
}

const LEVELS: [(&str, Level); 4] = [
    ("debug", Level::Debug),
    ("info", Level::Info),
    ("warn", Level::Warn),
    ("error", Level::Error),
];

fn logging<'js>(ctx: &Ctx<'js>, backend: &Rc<dyn Backend>) -> Result<Object<'js>> {
    let module = Object::new(ctx.clone())?;
    for (name, level) in LEVELS {
        let backend = backend.clone();
        module.set(
            name,
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'js>,
                      message: Coerced<String>,
                      fields: Opt<Value<'js>>|
                      -> Result<()> {
                    let fields = match fields.0 {
                        Some(fields) if !fields.is_undefined() && !fields.is_null() => {
                            Some(convert::to_json(&ctx, fields)?)
                        }
                        _ => None,
                    };
                    send_log(&backend, level, message.0, fields);
                    Ok(())
                },
            )?
            .with_name(name)?,
        )?;
    }
    Ok(module)
}

/// `console`, onto `internal.logging` at the matching level, `log` as
/// `info`. The arguments are one line, as a console prints them: strings as
/// they are, anything else as its JSON, or as `String(value)` when it has
/// none. They carry no fields, since a console call has no place to say
/// which argument was meant as one.
pub fn console<'js>(ctx: &Ctx<'js>, backend: Rc<dyn Backend>) -> Result<Object<'js>> {
    let console = Object::new(ctx.clone())?;
    for (name, level) in [("log", Level::Info)].into_iter().chain(LEVELS) {
        let backend = backend.clone();
        console.set(
            name,
            Function::new(
                ctx.clone(),
                move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> Result<()> {
                    let line = args
                        .0
                        .into_iter()
                        .map(|value| console_text(&ctx, value))
                        .collect::<Result<Vec<_>>>()?
                        .join(" ");
                    send_log(&backend, level, line, None);
                    Ok(())
                },
            )?
            .with_name(name)?,
        )?;
    }
    Ok(console)
}

fn console_text<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> Result<String> {
    if let Some(text) = value.as_string() {
        return text.to_string();
    }
    if value.is_undefined() {
        return Ok("undefined".to_string());
    }
    match convert::to_json(ctx, value.clone()) {
        Ok(json) => Ok(json.to_string()),
        Err(rquickjs::Error::Exception) => {
            ctx.catch();
            Ok(Coerced::<String>::from_js(ctx, value)?.0)
        }
        Err(other) => Err(other),
    }
}

fn time<'js>(ctx: &Ctx<'js>) -> Result<Object<'js>> {
    let module = Object::new(ctx.clone())?;
    module.set(
        "now",
        Function::new(ctx.clone(), || (now_micros() / 1000) as f64)?.with_name("now")?,
    )?;
    module.set(
        "to_iso8601",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, ms: f64| -> Result<String> {
            if ms.is_finite() {
                if let Some(at) = chrono::DateTime::from_timestamp_millis(ms as i64) {
                    return Ok(at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
                }
            }
            Err(throw_error(&ctx, &format!("timestamp out of range: {ms}")))
        })?
        .with_name("to_iso8601")?,
    )?;
    module.set(
        "from_iso8601",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, text: Coerced<String>| -> Value<'js> {
                // `null` rather than `undefined`, which is what an unparseable
                // date is everywhere else in JavaScript's own library.
                match chrono::DateTime::parse_from_rfc3339(&text.0) {
                    Ok(at) => Value::new_number(ctx, at.timestamp_millis() as f64),
                    Err(_) => Value::new_null(ctx),
                }
            },
        )?
        .with_name("from_iso8601")?,
    )?;
    Ok(module)
}

fn tids<'js>(ctx: &Ctx<'js>) -> Result<Object<'js>> {
    let module = Object::new(ctx.clone())?;
    module.set(
        "create",
        Function::new(ctx.clone(), || {
            tid_from_parts(now_micros() as u64, clock_id())
        })?
        .with_name("create")?,
    )?;
    module.set(
        "to_tid",
        Function::new(ctx.clone(), |ms: f64| {
            tid_from_unix_microseconds((ms as i64).saturating_mul(1000))
        })?
        .with_name("to_tid")?,
    )?;
    module.set(
        "from_tid",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, tid: Coerced<String>| -> Result<f64> {
                tid_to_unix_microseconds(&tid.0)
                    .map(|us| (us / 1000) as f64)
                    .ok_or_else(|| throw_error(&ctx, &format!("invalid TID: {}", tid.0)))
            },
        )?
        .with_name("from_tid")?,
    )?;
    Ok(module)
}

/// The clock id distinguishes two TIDs minted in the same microsecond, so it
/// needs entropy rather than a counter — two instances of this plugin would
/// count identically.
fn clock_id() -> u16 {
    let mut bytes = [0u8; 2];
    // An unreadable entropy source leaves the timestamp to separate TIDs,
    // which is what a zero clock id already means elsewhere in the codec.
    let _ = getrandom::fill(&mut bytes);
    u16::from_le_bytes(bytes)
}

/// `JSON` is already a global; this is the one the Lua plugin's scripts
/// know, with this plugin's conversion rules, so an encoded value is exactly
/// what the same value would carry to a library.
fn json<'js>(ctx: &Ctx<'js>) -> Result<Object<'js>> {
    let module = Object::new(ctx.clone())?;
    module.set(
        "encode",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, value: Value<'js>| -> Result<String> {
                match convert::to_json(&ctx, value) {
                    Ok(json) => Ok(json.to_string()),
                    Err(e) => {
                        let message = caught_message(&ctx, e);
                        Err(throw_error(&ctx, &format!("json.encode: {message}")))
                    }
                }
            },
        )?
        .with_name("encode")?,
    )?;
    module.set(
        "decode",
        Function::new(
            ctx.clone(),
            |ctx: Ctx<'js>, text: Coerced<String>| -> Result<Value<'js>> {
                let json: serde_json::Value = serde_json::from_str(&text.0)
                    .map_err(|e| throw_error(&ctx, &format!("json.decode: {e}")))?;
                convert::to_js(&ctx, &json)
            },
        )?
        .with_name("decode")?,
    )?;
    Ok(module)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::Fake;
    use crate::sandbox;
    use rquickjs::{Context, Runtime};
    use serde_json::json;

    /// `name`'s exports bound as the global `m`, on a fake that records logs.
    fn with_module<R>(name: &str, f: impl for<'js> FnOnce(Ctx<'js>, &Fake) -> R) -> R {
        let fake = Rc::new(Fake::new());
        let backend: Rc<dyn Backend> = fake.clone();
        let runtime = Runtime::new().unwrap();
        let context: Context = sandbox::create(&runtime, backend.clone()).unwrap();
        context.with(|ctx| {
            let exports = module(&ctx, name, &backend).unwrap().unwrap();
            ctx.globals().set("m", exports).unwrap();
            f(ctx, &fake)
        })
    }

    fn eval<T: for<'js> FromJs<'js>>(ctx: &Ctx<'_>, source: &str) -> T {
        ctx.eval(source).unwrap_or_else(|e| {
            let thrown = ctx.catch();
            panic!("{source}: {e}: {thrown:?}")
        })
    }

    fn error_of(ctx: &Ctx<'_>, source: &str) -> String {
        ctx.eval::<Value, _>(source).expect_err(source);
        caught_message(ctx, rquickjs::Error::Exception)
    }

    #[test]
    fn an_unknown_name_is_not_a_builtin() {
        sandbox::tests::with_context(|ctx| {
            let backend: Rc<dyn Backend> = Rc::new(Fake::new());
            assert!(module(&ctx, "happyview.db", &backend).unwrap().is_none());
            assert!(module(&ctx, "internal.nope", &backend).unwrap().is_none());
        });
    }

    #[test]
    fn time_round_trips_iso8601() {
        with_module("internal.time", |ctx, _| {
            assert!(eval::<f64>(&ctx, "m.now()") > 1_700_000_000_000.0);
            assert_eq!(
                eval::<String>(&ctx, "m.to_iso8601(1757775845000)"),
                "2025-09-13T15:04:05.000Z"
            );
            assert_eq!(
                eval::<f64>(&ctx, "m.from_iso8601('2025-09-13T15:04:05.000Z')"),
                1_757_775_845_000.0
            );
            assert!(eval::<bool>(&ctx, "m.from_iso8601('soon') === null"));
            assert!(error_of(&ctx, "m.to_iso8601(Infinity)").contains("timestamp out of range"));
        });
    }

    #[test]
    fn tids_are_minted_and_converted() {
        with_module("internal.tids", |ctx, _| {
            let tid: String = eval(&ctx, "m.create()");
            assert_eq!(tid.len(), 13);
            for ch in tid.chars() {
                assert!("234567abcdefghijklmnopqrstuvwxyz".contains(ch), "{tid}");
            }
            // The Lua plugin's test explains the bound: what matters is that
            // the clock, at microsecond resolution, orders a run of TIDs.
            let minted: Vec<String> = eval(
                &ctx,
                "const out = []; for (let i = 0; i < 500; i++) out.push(m.create()); out",
            );
            let stamps: Vec<&str> = minted.iter().map(|tid| &tid[..11]).collect();
            let distinct: std::collections::BTreeSet<&&str> = stamps.iter().collect();
            assert!(distinct.len() > 50, "{} distinct", distinct.len());
            for pair in stamps.windows(2) {
                assert!(pair[0] <= pair[1], "{} then {}", pair[0], pair[1]);
            }

            assert_eq!(
                eval::<f64>(&ctx, "m.from_tid(m.to_tid(1757775845000))"),
                1_757_775_845_000.0
            );
            assert_eq!(error_of(&ctx, "m.from_tid('nope')"), "invalid TID: nope");
        });
    }

    #[test]
    fn json_round_trips_and_names_what_it_refuses() {
        with_module("internal.json", |ctx, _| {
            assert_eq!(
                eval::<String>(&ctx, r#"m.encode(m.decode('{"a":1,"b":[]}'))"#),
                r#"{"a":1,"b":[]}"#
            );
            assert_eq!(eval::<String>(&ctx, "m.encode([])"), "[]");
            assert_eq!(eval::<String>(&ctx, "m.encode({})"), "{}");
            assert!(eval::<bool>(&ctx, "m.to_array === undefined"));
            assert_eq!(
                error_of(&ctx, "m.encode({ f() {} })"),
                "json.encode: cannot convert a function to JSON (at 'f')"
            );
            assert!(error_of(&ctx, "m.decode('not valid json')").starts_with("json.decode:"));
        });
    }

    #[test]
    fn a_log_line_reaches_the_host_with_its_level_and_fields() {
        with_module("internal.logging", |ctx, fake| {
            eval::<()>(
                &ctx,
                "m.info('hi', { n: 2 }); m.debug('d'); m.warn('w', null); m.error(42)",
            );
            let logs = fake.logs.borrow();
            let seen: Vec<(Level, &str, Option<&serde_json::Value>)> = logs
                .iter()
                .map(|log| (log.level, log.message.as_str(), log.fields.as_ref()))
                .collect();
            assert_eq!(
                seen,
                vec![
                    (Level::Info, "hi", Some(&json!({ "n": 2 }))),
                    (Level::Debug, "d", None),
                    (Level::Warn, "w", None),
                    (Level::Error, "42", None),
                ]
            );
        });
    }

    #[test]
    fn a_log_lines_fields_must_be_encodable() {
        with_module("internal.logging", |ctx, fake| {
            let error = error_of(&ctx, "const t = {}; t.self = t; m.info('x', t)");
            assert!(error.contains("circular"), "{error}");
            assert!(fake.logs.borrow().is_empty());
        });
    }

    #[test]
    fn console_writes_one_line_at_the_matching_level() {
        with_module("internal.logging", |ctx, fake| {
            eval::<()>(
                &ctx,
                "console.log('a', 1, { b: [true] }, undefined, () => 1); \
                 console.info('i'); console.debug('d'); console.warn('w'); console.error('e')",
            );
            let logs = fake.logs.borrow();
            let seen: Vec<(Level, &str)> = logs
                .iter()
                .map(|log| (log.level, log.message.as_str()))
                .collect();
            assert_eq!(
                seen,
                vec![
                    (Level::Info, r#"a 1 {"b":[true]} undefined () => 1"#),
                    (Level::Info, "i"),
                    (Level::Debug, "d"),
                    (Level::Warn, "w"),
                    (Level::Error, "e"),
                ]
            );
        });
    }
}
