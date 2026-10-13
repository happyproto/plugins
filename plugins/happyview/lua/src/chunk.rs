//! Naming a chunk, loading one, and reading a position back out of an error.
//!
//! A project's chunk is loaded as `@<path>`, which Lua renders as
//! `<path>:12:` — the form a terminal makes clickable. A dashboard script
//! keeps the bare name `script`, rendered `[string "script"]:12:`, which is
//! what every stored event log and the host's differential comparison read.

use happyview_plugin_sdk::PreparedArtifact;
use mlua::chunk::ChunkMode;
use mlua::{Function, Lua, Result as LuaResult};

/// The chunk name of a script that no file names: one saved in the dashboard.
pub const DEFAULT: &str = happyview_plugin_sdk::DEFAULT_CHUNK_NAME;

pub fn lua_name(name: &str) -> String {
    if name == DEFAULT {
        DEFAULT.to_string()
    } else {
        format!("@{name}")
    }
}

/// The chunk name a function's source records, when it is a script's or a
/// module's. The plugin's own chunks are `=`-named, and C functions are
/// `=[C]`, so neither is anybody's file.
pub fn name_of_source(source: &str) -> Option<&str> {
    if source == DEFAULT {
        return Some(DEFAULT);
    }
    source.strip_prefix('@')
}

/// Source text, and never a binary chunk. PUC Lua does not verify bytecode,
/// so the only bytecode a VM loads is an artifact the host supplied.
pub fn load_source(lua: &Lua, name: &str, source: &str) -> LuaResult<Function> {
    lua.load(source)
        .set_mode(ChunkMode::Text)
        .set_name(lua_name(name))
        .into_function()
}

/// The chunk a run executes: the host's bytecode when this build wrote it,
/// the source otherwise. The outer `Err` is an artifact this build cannot
/// load with no source to fall back to, which is the host's to fix rather
/// than the script's.
pub fn load(
    lua: &Lua,
    name: &str,
    source: &str,
    artifact: Option<&PreparedArtifact>,
) -> Result<LuaResult<Function>, String> {
    if let Some(artifact) = artifact {
        match load_artifact(lua, name, artifact) {
            Ok(main) => return Ok(Ok(main)),
            Err(why) if source.is_empty() => return Err(why),
            Err(_) => {}
        }
    }
    Ok(load_source(lua, name, source))
}

/// `artifact` as a function, when it is bytecode this build wrote for chunk
/// `name`. PUC Lua does not verify bytecode, so the stamp is checked before
/// a byte of it reaches `lua_load`.
///
/// A dump carries the name it was compiled under and ignores the one it is
/// loaded with, so an artifact prepared for another chunk would report its
/// positions, and send its referrer, as that chunk. A dump that will not
/// load is the host's artifact being damaged rather than the script being
/// wrong, so it is refused the same way.
fn load_artifact(lua: &Lua, name: &str, artifact: &PreparedArtifact) -> Result<Function, String> {
    crate::artifact::usable(artifact)?;
    let main = lua
        .load(artifact.bytes.as_slice())
        .set_mode(ChunkMode::Binary)
        // A dump ignores this for its positions; it only labels a load error,
        // which would otherwise name this file.
        .set_name(lua_name(name))
        .into_function()
        .map_err(|e| format!("the prepared artifact for {name} did not load: {e}"))?;
    let expected = lua_name(name);
    let embedded = main.info().source.unwrap_or_default();
    if embedded != expected {
        let embedded = embedded.strip_prefix('@').unwrap_or(&embedded);
        return Err(format!(
            "the prepared artifact was compiled as {embedded}, and this run is {name}"
        ));
    }
    Ok(main)
}

/// Where an error was raised, and what it said once the position is off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub chunk: Option<String>,
    pub line: Option<u32>,
    pub message: String,
}

/// The first `<chunk>:<line>: ` in `raw` whose chunk is one this run loaded.
/// A project chunk is `@`-named rather than left as a bare string because Lua
/// shortens a long `@` name from the front, keeping the file name, where it
/// shortens a bare one from the end, which can leave two files in one deep
/// directory reading the same.
///
/// Matching only `known` names is what keeps `error("status:404: x", 0)` from
/// being read as line 404 of a file called `status`. The traceback goes
/// first: it names only chunk lines and library frames, which is noise to a
/// caller, and a position inside it would otherwise be read as the error's
/// own. An error with no position keeps its whole text, which is why
/// `error("x", 0)` reads as it was written.
pub fn locate(raw: &str, known: &[String]) -> Located {
    let head = raw.split("\nstack traceback:").next().unwrap_or(raw);
    for (at, _) in head.match_indices(':') {
        let rest = &head[at + 1..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || !rest[digits..].starts_with(": ") {
            continue;
        }
        let Ok(line) = rest[..digits].parse::<u32>() else {
            continue;
        };
        let before = &head[..at];
        if let Some(name) = known.iter().find(|name| shown_as(name, before)) {
            return Located {
                // The dashboard's chunk names no file anyone could open, and
                // the host's reference runner reports it as none too.
                chunk: (name != DEFAULT).then(|| name.clone()),
                line: Some(line),
                message: rest[digits + 2..].to_string(),
            };
        }
    }
    Located {
        chunk: None,
        line: None,
        message: head.to_string(),
    }
}

/// How Lua renders chunk `name` at the start of a message, unshortened. An
/// error this plugin raises itself on a script's behalf is written the same
/// way, so `locate` reads both alike.
pub fn shown(name: &str) -> String {
    if name == DEFAULT {
        format!(r#"[string "{DEFAULT}"]"#)
    } else {
        name.to_string()
    }
}

/// Whether `before` ends with how Lua renders chunk `name` in a message.
/// Lua shortens a name longer than its chunk-id buffer from the front,
/// behind `...`, keeping the end — the part that names the file.
fn shown_as(name: &str, before: &str) -> bool {
    if name == DEFAULT {
        return before.ends_with(&shown(DEFAULT));
    }
    let whole = before
        .strip_suffix(name)
        .is_some_and(|lead| lead.is_empty() || lead.ends_with(' '));
    if whole {
        return true;
    }
    // `luaO_chunkid` copies an `@` name whole when it fits `LUA_IDSIZE` (60)
    // with its terminator, and otherwise writes `...` and the last
    // `LUA_IDSIZE - 4` bytes. Only a name over the first limit is ever
    // shortened, and only to exactly that tail, which keeps an unrelated
    // `...` in a message from reading as one.
    if name.len() < LUA_IDSIZE {
        return false;
    }
    let kept = &name.as_bytes()[name.len() - (LUA_IDSIZE - 4)..];
    let mut rendered = b"...".to_vec();
    rendered.extend_from_slice(kept);
    before
        .as_bytes()
        .strip_suffix(rendered.as_slice())
        .is_some_and(|lead| lead.is_empty() || lead.ends_with(b" "))
}

/// Lua's chunk-id buffer size, terminator included (`luaconf.h`).
const LUA_IDSIZE: usize = 60;

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_dashboard_chunk_keeps_its_bare_name_and_a_project_chunk_is_a_file() {
        assert_eq!(lua_name("script"), "script");
        assert_eq!(lua_name("lib/format.lua"), "@lib/format.lua");
        assert_eq!(name_of_source("script"), Some("script"));
        assert_eq!(name_of_source("@lib/format.lua"), Some("lib/format.lua"));
        // The plugin's own chunks are `=`-named and are nobody's file.
        assert_eq!(name_of_source("=catch_guards"), None);
        assert_eq!(name_of_source("=[C]"), None);
    }

    #[test]
    fn a_position_in_a_known_chunk_is_read_out_whatever_mlua_put_in_front() {
        let known = names(&["script"]);
        assert_eq!(
            locate(r#"runtime error: [string "script"]:3: boom"#, &known),
            Located {
                chunk: None,
                line: Some(3),
                message: "boom".into()
            }
        );
        let known = names(&["src/handler.lua", "lib/format.lua"]);
        assert_eq!(
            locate(
                "runtime error: lib/format.lua:12: attempt to index a nil value\nstack traceback:\n\t[C]: in ?",
                &known
            ),
            Located {
                chunk: Some("lib/format.lua".into()),
                line: Some(12),
                message: "attempt to index a nil value".into()
            }
        );
    }

    #[test]
    fn a_long_path_shortened_from_the_front_still_names_its_file() {
        let long = format!(
            "src/lexicons/{}/getArtifact.lua",
            "a/very/long/path/segment".repeat(3)
        );
        let lua = mlua::Lua::new();
        let error = lua
            .load("error('x')")
            .set_name(lua_name(&long))
            .exec()
            .unwrap_err()
            .to_string();
        assert!(error.contains("..."), "Lua shortened nothing: {error}");
        let located = locate(&error, std::slice::from_ref(&long));
        assert_eq!(located.chunk.as_deref(), Some(long.as_str()), "{error}");
        assert_eq!(located.line, Some(1));
        assert_eq!(located.message, "x");
    }

    /// The error Lua raises in a chunk named `name`, by name alone.
    fn raised_in(name: &str) -> String {
        mlua::Lua::new()
            .load("error('x')")
            .set_name(lua_name(name))
            .exec()
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn the_shortening_limit_is_pinned_at_the_byte() {
        // `luaO_chunkid` keeps an `@` name whole while it is under 60 bytes.
        let whole = "a".repeat(59);
        let error = raised_in(&whole);
        assert!(error.contains(&format!("{whole}:1:")), "{error}");
        assert!(!error.contains("..."), "{error}");
        let located = locate(&error, std::slice::from_ref(&whole));
        assert_eq!(located.chunk.as_deref(), Some(whole.as_str()), "{error}");

        let shortened = "a".repeat(60);
        let error = raised_in(&shortened);
        assert!(
            error.contains(&format!("...{}:1:", "a".repeat(56))),
            "{error}"
        );
        let located = locate(&error, std::slice::from_ref(&shortened));
        assert_eq!(
            located.chunk.as_deref(),
            Some(shortened.as_str()),
            "{error}"
        );
        assert_eq!(located.line, Some(1));
    }

    #[test]
    fn a_damaged_artifact_reports_the_chunk_and_not_the_plugins_own_file() {
        let lua = mlua::Lua::new();
        let main = load_source(&lua, "lib/format.lua", "return 1").unwrap();
        let mut artifact = crate::artifact::prepare(&main);
        artifact.bytes.truncate(artifact.bytes.len() / 2);
        let error = load(&lua, "lib/format.lua", "", Some(&artifact)).unwrap_err();
        assert!(error.contains("lib/format.lua"), "{error}");
        assert!(!error.contains("src/chunk.rs"), "{error}");
    }

    #[test]
    fn a_message_that_only_looks_like_a_position_is_left_alone() {
        let known = names(&["script", "lib/format.lua"]);
        let located = locate("runtime error: status:404: not found", &known);
        assert_eq!(located.chunk, None);
        assert_eq!(located.line, None);
        assert_eq!(located.message, "runtime error: status:404: not found");
        // Neither is an unrelated `...` in front of a short known name.
        let located = locate(
            r#"runtime error: loading...lua:5: oops"#,
            &names(&["lib/format.lua"]),
        );
        assert_eq!(located.line, None);
        // A short name is never shortened, so a leading `...` is not Lua's.
        let located = locate(
            "runtime error: x ...lib/format.lua:5: oops",
            &names(&["lib/format.lua"]),
        );
        assert_eq!(located.line, None);
        // A name merely ending in a known one is not that file.
        let located = locate(
            "runtime error: mylib/format.lua:3: x",
            &names(&["format.lua"]),
        );
        assert_eq!(located.line, None);
    }

    #[test]
    fn source_text_never_loads_as_bytecode() {
        let lua = mlua::Lua::new();
        let bytecode = lua.load("return 1").into_function().unwrap().dump(false);
        let source = String::from_utf8_lossy(&bytecode).into_owned();
        // `from_utf8_lossy` keeps the leading "\x1bLua" signature.
        assert!(source.starts_with("\u{1b}Lua"));
        let error = load_source(&lua, "script", &source)
            .unwrap_err()
            .to_string();
        assert!(error.contains("attempt to load a binary chunk"), "{error}");
    }
}
