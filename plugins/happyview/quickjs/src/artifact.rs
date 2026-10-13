//! A module's prepared form: QuickJS bytecode, written with its debug
//! information and source text kept, so a run loads it without parsing or
//! compiling, its stack frames still carry lines, and `Function.prototype.
//! toString` still answers the author's text.

use std::ffi::CStr;

use happyview_plugin_sdk::PreparedArtifact;

pub const FORMAT: &str = "quickjs-bytecode";

/// The QuickJS-ng release that wrote the bytecode. Its reader checks a
/// bytecode version, but a release can change what that version means, and
/// bytes from another build are undefined rather than refused.
pub fn engine() -> String {
    // SAFETY: `JS_GetVersion` returns a pointer to a static NUL-terminated
    // string compiled into QuickJS-ng.
    let version = unsafe { CStr::from_ptr(rquickjs::qjs::JS_GetVersion()) };
    format!("quickjs-ng-{}", version.to_string_lossy())
}

pub fn prepared(bytes: Vec<u8>, source_map: Option<String>) -> PreparedArtifact {
    PreparedArtifact {
        // The host stamps both: a plugin cannot know the id and version it
        // was installed under.
        interpreter: String::new(),
        version: String::new(),
        engine: engine(),
        format: FORMAT.to_string(),
        bytes,
        source_map,
    }
}

/// Whether this build can load `artifact`. QuickJS does not verify bytecode,
/// so an artifact another build wrote is never handed to its reader.
pub fn usable(artifact: &PreparedArtifact) -> Result<(), String> {
    let engine = engine();
    if artifact.format == FORMAT && artifact.engine == engine {
        return Ok(());
    }
    Err(format!(
        "the prepared artifact is {} from {}, and this interpreter runs {FORMAT} from {engine}",
        artifact.format, artifact.engine
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_is_the_linked_quickjs_ng() {
        // Moves only when the pinned `rquickjs-sys` does, deliberately: a new
        // QuickJS-ng can change its bytecode, and every artifact stamped with
        // the old one has to be rebuilt.
        assert_eq!(engine(), "quickjs-ng-0.16.2");
        assert_eq!(FORMAT, "quickjs-bytecode");
    }

    #[test]
    fn a_prepared_artifact_carries_the_stamp_and_leaves_the_hosts_fields_empty() {
        let artifact = prepared(vec![1, 2, 3], Some("{}".to_string()));
        assert_eq!(artifact.engine, engine());
        assert_eq!(artifact.format, FORMAT);
        assert!(artifact.interpreter.is_empty() && artifact.version.is_empty());
        assert_eq!(artifact.bytes, vec![1, 2, 3]);
        assert_eq!(artifact.source_map.as_deref(), Some("{}"));
        assert_eq!(usable(&artifact), Ok(()));
    }

    #[test]
    fn an_artifact_from_another_engine_or_format_is_not_usable() {
        let mut artifact = prepared(Vec::new(), None);
        artifact.engine = "quickjs-ng-0.10.0".to_string();
        let why = usable(&artifact).unwrap_err();
        assert!(
            why.contains("quickjs-ng-0.10.0") && why.contains(&engine()),
            "{why}"
        );
        artifact.engine = engine();
        artifact.format = "lua-bytecode".to_string();
        assert!(usable(&artifact).unwrap_err().contains("lua-bytecode"));
    }
}
