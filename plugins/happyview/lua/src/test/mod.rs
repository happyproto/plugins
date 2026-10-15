//! `internal.test` and the test-file globals, for a run whose kind is `test`.

mod host;

use mlua::Lua;

#[cfg(test)]
pub use host::Recording;
pub use host::{Host, TestHost, Validating};

/// The innermost frame in a script or module: the line in the test file, or
/// in a module it imported, that called the assertion that failed. The
/// dashboard's chunk names no file, so it is not reported as one, and a stack
/// with no script frame on it places nothing.
pub(crate) fn location(lua: &Lua) -> (Option<String>, Option<u32>) {
    match crate::modules::caller(lua) {
        Ok(caller) => (
            Some(caller.name).filter(|name| name != crate::chunk::DEFAULT),
            caller.line.and_then(|line| u32::try_from(line).ok()),
        ),
        Err(_) => (None, None),
    }
}

/// A raised message's place and its text, read the way a run's own failure
/// is, against the chunks this run loaded.
pub(crate) fn split(lua: &Lua, text: &str) -> (Option<String>, Option<u32>, String) {
    let located = crate::chunk::locate(text, &crate::modules::known(lua));
    (located.chunk, located.line, located.message)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "src/lib/a.test.lua";

    fn vm() -> Lua {
        let lua = crate::sandbox::tests::sandbox();
        crate::modules::install(&lua, FILE, Box::new(crate::modules::tests::Files::new(&[])))
            .unwrap();
        lua
    }

    #[test]
    fn location_is_the_innermost_project_frame() {
        let lua = vm();
        lua.globals()
            .set(
                "where_am_i",
                lua.create_function(|lua, ()| Ok(location(lua))).unwrap(),
            )
            .unwrap();
        let (file, line): (Option<String>, Option<u32>) =
            crate::chunk::load_source(&lua, FILE, "local x = 1\nreturn where_am_i()")
                .unwrap()
                .call(())
                .unwrap();
        assert_eq!((file.as_deref(), line), (Some(FILE), Some(2)));
    }

    #[test]
    fn an_error_splits_into_its_place_and_message() {
        let lua = vm();
        let err = crate::chunk::load_source(&lua, FILE, "local x = 1\nerror('boom')")
            .unwrap()
            .call::<()>(())
            .unwrap_err();
        assert_eq!(
            split(&lua, &err.to_string()),
            (Some(FILE.to_string()), Some(2), "boom".to_string())
        );
        for text in ["boom", "AUTH_ERROR: no session", "status:404: x"] {
            assert_eq!(split(&lua, text), (None, None, text.to_string()), "{text}");
        }
    }
}
