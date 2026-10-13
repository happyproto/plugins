//! A chunk's prepared form: PUC Lua bytecode, dumped with its debug
//! information kept, so a run loads it without lexing or parsing and an
//! error in it still names its chunk and line.

use happyview_plugin_sdk::PreparedArtifact;

pub const FORMAT: &str = "lua-bytecode";

/// The release the bytecode was compiled by. A dump records its own format
/// version but not the release, and a release can change what the same
/// source compiles to.
pub const ENGINE: &str = concat!("puc-lua-", env!("HV_LUA_RELEASE"));

/// `function` as an artifact. Dumped before it ever runs, so what is kept is
/// the compiled chunk and nothing its file scope did.
pub fn prepare(function: &mlua::Function) -> PreparedArtifact {
    PreparedArtifact {
        // The host stamps both: a plugin cannot know the id and version it
        // was installed under.
        interpreter: String::new(),
        version: String::new(),
        engine: ENGINE.to_string(),
        format: FORMAT.to_string(),
        bytes: function.dump(false),
        source_map: None,
    }
}

/// Whether this build can load `artifact`. Lua checks a dump's header, but
/// not the release that wrote it, and loading another release's bytecode
/// is undefined rather than an error.
pub fn usable(artifact: &PreparedArtifact) -> Result<(), String> {
    if artifact.format == FORMAT && artifact.engine == ENGINE {
        return Ok(());
    }
    Err(format!(
        "the prepared artifact is {} from {}, and this interpreter runs {FORMAT} from {ENGINE}",
        artifact.format, artifact.engine
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk;

    #[test]
    fn the_engine_is_the_vendored_lua_release() {
        // Moves only when `lua-src` does, deliberately: a new release can
        // compile the same source differently, and every artifact stamped
        // with the old one has to be rebuilt.
        assert_eq!(ENGINE, "puc-lua-5.4.8");
        assert_eq!(FORMAT, "lua-bytecode");
    }

    #[test]
    fn an_artifact_keeps_debug_information_and_the_chunk_name() {
        let lua = mlua::Lua::new();
        let main =
            chunk::load_source(&lua, "lib/format.lua", "local x = 1\n\nerror('boom')").unwrap();
        let artifact = prepare(&main);
        assert_eq!(artifact.engine, ENGINE);
        assert_eq!(artifact.format, FORMAT);
        assert!(artifact.interpreter.is_empty() && artifact.version.is_empty());
        assert_eq!(artifact.source_map, None);
        assert!(artifact.bytes.starts_with(b"\x1bLua"));

        let loaded = lua
            .load(artifact.bytes.as_slice())
            .set_mode(mlua::chunk::ChunkMode::Binary)
            .into_function()
            .unwrap();
        let error = loaded.call::<()>(()).unwrap_err().to_string();
        assert!(error.contains("lib/format.lua:3: boom"), "{error}");
    }

    #[test]
    fn an_artifact_from_another_engine_or_format_is_not_usable() {
        let lua = mlua::Lua::new();
        let mut artifact = prepare(&lua.load("return 1").into_function().unwrap());
        assert_eq!(usable(&artifact), Ok(()));
        artifact.engine = "puc-lua-5.4.6".to_string();
        let why = usable(&artifact).unwrap_err();
        assert!(
            why.contains("puc-lua-5.4.6") && why.contains(ENGINE),
            "{why}"
        );
        artifact.engine = ENGINE.to_string();
        artifact.format = "quickjs-bytecode".to_string();
        assert!(usable(&artifact).unwrap_err().contains("quickjs-bytecode"));
    }
}
