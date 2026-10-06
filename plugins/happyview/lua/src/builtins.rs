//! The modules `require` serves without a plugin: the ones that *are* the
//! host, or that a wasm round trip on a script's hottest path would only slow
//! down. They live under `internal.` so a published plugin can never shadow
//! one.

use std::time::{SystemTime, UNIX_EPOCH};

use happyview_plugin_sdk::host::{self, Level};
use happyview_plugin_sdk::tid::{
    tid_from_parts, tid_from_unix_microseconds, tid_to_unix_microseconds,
};
use happyview_plugin_sdk::ScriptLogRequest;
use mlua::{Lua, LuaSerdeExt, Result as LuaResult, Table, Value};

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

pub fn module(lua: &Lua, name: &str) -> LuaResult<Option<Table>> {
    if !MODULES.contains(&name) {
        return Ok(None);
    }
    let suffix = name
        .strip_prefix(PREFIX)
        .expect("every entry in MODULES carries the prefix");
    Ok(Some(match suffix {
        "logging" => logging(lua)?,
        "time" => time(lua)?,
        "tids" => tids(lua)?,
        "json" => json(lua)?,
        _ => unreachable!("MODULES and this match must name the same suffixes"),
    }))
}

/// Microseconds since the epoch, from the one clock in this VM: `os.time`,
/// `os.date` and this have to agree, and a second clock could not.
///
/// Microseconds rather than milliseconds because a TID is
/// `(microseconds << 10) | clock_id`: at millisecond resolution every TID
/// minted in the same millisecond differs only in the ten random bits, so
/// they collide about once in a thousand pairs and sort in random order —
/// and a TID is a record key. WASI's clock answers nanoseconds.
fn now_micros() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_micros() as i64,
        Err(before) => -(before.duration().as_micros() as i64),
    }
}

fn now_millis() -> i64 {
    now_micros() / 1000
}

fn logging(lua: &Lua) -> LuaResult<Table> {
    let t = lua.create_table()?;
    for (name, level) in [
        ("debug", Level::Debug),
        ("info", Level::Info),
        ("warn", Level::Warn),
        ("error", Level::Error),
    ] {
        t.set(
            name,
            lua.create_function(
                move |lua, (message, fields): (String, Option<Table>)| -> LuaResult<()> {
                    let fields = match fields {
                        Some(fields) => Some(convert::to_json(lua, Value::Table(fields))?),
                        None => None,
                    };
                    // A log line that cannot be written must not end the run:
                    // the host's own write is best-effort for the same reason,
                    // and a script has nothing useful to do about it.
                    let _ = host::script_log(&ScriptLogRequest {
                        level,
                        message,
                        fields,
                    });
                    Ok(())
                },
            )?,
        )?;
    }
    Ok(t)
}

fn time(lua: &Lua) -> LuaResult<Table> {
    let t = lua.create_table()?;
    t.set("now", lua.create_function(|_, ()| Ok(now_millis()))?)?;
    t.set(
        "to_iso8601",
        lua.create_function(|_, ms: i64| {
            chrono::DateTime::from_timestamp_millis(ms)
                .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
                .ok_or_else(|| mlua::Error::runtime(format!("timestamp out of range: {ms}")))
        })?,
    )?;
    t.set(
        "from_iso8601",
        lua.create_function(|_, s: String| {
            Ok(chrono::DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.timestamp_millis()))
        })?,
    )?;
    Ok(t)
}

fn tids(lua: &Lua) -> LuaResult<Table> {
    let t = lua.create_table()?;
    t.set(
        "create",
        lua.create_function(|_, ()| Ok(tid_from_parts(now_micros() as u64, clock_id())))?,
    )?;
    t.set(
        "to_tid",
        lua.create_function(|_, ms: i64| Ok(tid_from_unix_microseconds(ms * 1000)))?,
    )?;
    t.set(
        "from_tid",
        lua.create_function(|_, tid: String| {
            tid_to_unix_microseconds(&tid)
                .map(|us| us / 1000)
                .ok_or_else(|| mlua::Error::runtime(format!("invalid TID: {tid}")))
        })?,
    )?;
    Ok(t)
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

fn json(lua: &Lua) -> LuaResult<Table> {
    let t = lua.create_table()?;
    t.set(
        "encode",
        lua.create_function(|lua, value: Value| {
            let value = convert::to_json(lua, value)
                .map_err(|e| mlua::Error::runtime(format!("json.encode: {e}")))?;
            serde_json::to_string(&value)
                .map_err(|e| mlua::Error::runtime(format!("json.encode: {e}")))
        })?,
    )?;
    t.set(
        "decode",
        lua.create_function(|lua, s: String| {
            let value: serde_json::Value = serde_json::from_str(&s)
                .map_err(|e| mlua::Error::runtime(format!("json.decode: {e}")))?;
            convert::to_lua(lua, &value)
                .map_err(|e| mlua::Error::runtime(format!("json.decode: {e}")))
        })?,
    )?;
    t.set(
        "to_array",
        lua.create_function(|lua, table: Table| {
            // Lua has one table type, so an empty sequence and an empty map
            // are the same value until something says which was meant.
            let values: Vec<Value> = table.sequence_values().collect::<LuaResult<_>>()?;
            let sequence = lua.create_sequence_from(values)?;
            sequence.set_metatable(Some(lua.array_metatable()))?;
            Ok(sequence)
        })?,
    )?;
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::tests::sandbox;

    fn with(name: &str, global: &str) -> Lua {
        let lua = sandbox();
        let module = module(&lua, name).unwrap().unwrap();
        lua.globals().set(global, module).unwrap();
        lua
    }

    fn eval<T: mlua::FromLuaMulti>(lua: &Lua, source: &str) -> T {
        lua.load(source)
            .eval()
            .unwrap_or_else(|e| panic!("{source}: {e}"))
    }

    #[test]
    fn an_unknown_name_is_not_a_builtin() {
        let lua = sandbox();
        assert!(module(&lua, "happyview.db").unwrap().is_none());
        assert!(module(&lua, "internal.nope").unwrap().is_none());
    }

    #[test]
    fn time_round_trips_iso8601() {
        let lua = with("internal.time", "time");
        assert!(eval::<i64>(&lua, "return time.now()") > 1_700_000_000_000);
        assert_eq!(
            eval::<String>(&lua, "return time.to_iso8601(1757775845000)"),
            "2025-09-13T15:04:05.000Z"
        );
        assert_eq!(
            eval::<i64>(
                &lua,
                r#"return time.from_iso8601("2025-09-13T15:04:05.000Z")"#
            ),
            1_757_775_845_000
        );
        let unparseable: Value = eval(&lua, r#"return time.from_iso8601("soon")"#);
        assert!(unparseable.is_nil());
    }

    #[test]
    fn tids_are_minted_and_converted() {
        let lua = with("internal.tids", "tids");
        let tid: String = eval(&lua, "return tids.create()");
        assert_eq!(tid.len(), 13);
        for ch in tid.chars() {
            assert!("234567abcdefghijklmnopqrstuvwxyz".contains(ch), "{tid}");
        }
        // A TID is a record key, so what matters is that the clock — not the
        // ten random bits — is what orders a run of them. The last two
        // characters are exactly those ten bits, so the first eleven are the
        // timestamp alone.
        //
        // The assertion counts *distinct* timestamps rather than requiring
        // each to exceed the last, because strict increase holds only while a
        // mint costs more than a microsecond: true on a slow host, a coin
        // flip on a fast one.
        //
        // The margin, so nobody tightens the bound without knowing what it
        // rests on: fifty distinct microseconds across five hundred mints
        // needs the run to span fifty microseconds, so about a hundred
        // nanoseconds a mint. A mint is a trampoline into Rust, a clock read
        // and a string intern, which is two to five hundred — so the bound
        // has two to five times the margin it needs, in one direction only.
        // At millisecond resolution the count is one or two, whatever the
        // host.
        let minted: Vec<String> = eval(
            &lua,
            "local out = {} for i = 1, 500 do out[i] = tids.create() end return out",
        );
        assert_eq!(minted.len(), 500);

        let stamps: Vec<&str> = minted.iter().map(|tid| &tid[..11]).collect();
        let distinct: std::collections::BTreeSet<&&str> = stamps.iter().collect();
        assert!(
            distinct.len() > 50,
            "only {} distinct timestamps across 500 TIDs, so they are not being \
             minted at microsecond resolution",
            distinct.len()
        );
        // What makes this safe is the window, not the clock: `SystemTime` is
        // wall time and can step backwards, where `Instant` could not. The
        // five hundred mints span a hundred microseconds or so, and nothing
        // adjusts a wall clock inside one.
        for pair in stamps.windows(2) {
            assert!(pair[0] <= pair[1], "{} then {}", pair[0], pair[1]);
        }

        let from: String = eval(&lua, "return tids.to_tid(1757775845000)");
        assert_eq!(
            eval::<i64>(&lua, &format!(r#"return tids.from_tid("{from}")"#)),
            1_757_775_845_000
        );
        let error = lua
            .load(r#"return tids.from_tid("nope")"#)
            .eval::<Value>()
            .expect_err("13 characters of the alphabet or nothing")
            .to_string();
        assert!(error.contains("invalid TID: nope"), "{error}");
    }

    #[test]
    fn json_round_trips_and_marks_arrays() {
        let lua = with("internal.json", "json");
        assert_eq!(
            eval::<String>(&lua, r#"return json.encode(json.decode('{"a":1}'))"#),
            r#"{"a":1}"#
        );
        assert_eq!(
            eval::<String>(&lua, "return json.encode(json.to_array({}))"),
            "[]"
        );
        assert_eq!(eval::<String>(&lua, "return json.encode({})"), "{}");
        // The marker survives the table being emptied, which is the whole
        // reason it exists.
        assert_eq!(
            eval::<String>(
                &lua,
                "local a = json.to_array({1, 2}) a[1] = nil a[2] = nil return json.encode(a)"
            ),
            "[]"
        );
        for (source, expected) in [
            ("return json.encode(json)", "json.encode:"),
            (r#"return json.decode("not valid json")"#, "json.decode:"),
        ] {
            let error = lua
                .load(source)
                .eval::<Value>()
                .expect_err(source)
                .to_string();
            assert!(error.contains(expected), "{source}: {error}");
        }
    }

    /// A log line reaches the host and nothing else; off wasm the wrapper
    /// reports `NotWasm`, and the run must not end because of it.
    #[test]
    fn a_log_line_never_fails_the_run() {
        let lua = with("internal.logging", "log");
        lua.load(r#"log.info("hi", { n = 2 }) log.debug("d") log.warn("w") log.error("e")"#)
            .exec()
            .expect("a log line that cannot be written is not a script error");
    }

    #[test]
    fn a_log_lines_fields_must_be_encodable() {
        let lua = with("internal.logging", "log");
        let error = lua
            .load("local t = {} t.self = t log.info('x', t)")
            .exec()
            .expect_err("a cycle cannot be sent")
            .to_string();
        assert!(error.contains("recursive table"), "{error}");
    }
}
