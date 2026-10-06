//! JSON across the boundary: three calls into mlua's serde bridge and no
//! conversion of its own. The host makes the same three calls, so a rule
//! written out a second time here could drift from it without either side
//! failing to compile.

use mlua::{Lua, LuaSerdeExt, Result as LuaResult, Table, Value};
use serde_json::Value as Json;

/// A library result's `null` becomes `nil`, so `if grant.handle then` and
/// `== nil` both behave. mlua's default is a light-userdata sentinel, which
/// is truthy and concatenates as an error.
pub fn library_result(lua: &Lua, value: &Json) -> LuaResult<Value> {
    lua.to_value_with(
        value,
        mlua::serde::SerializeOptions::new()
            .serialize_none_to_null(false)
            .serialize_unit_to_null(false),
    )
}

/// Everywhere else — `input`, `context`, `json.decode` — a `null` is that
/// sentinel, which is truthy and encodes back to `null`.
pub fn to_lua(lua: &Lua, value: &Json) -> LuaResult<Value> {
    lua.to_value(value)
}

pub fn to_json(lua: &Lua, value: Value) -> LuaResult<Json> {
    lua.from_value(value)
}

/// The arguments of a call or of one chained step. `sequence_values` stops at
/// the first `nil`, so `f(1, nil, 3)` carries one argument — which is where
/// mlua cuts it, and therefore where the native bridge cuts it too.
pub fn sequence_to_json(lua: &Lua, table: &Table) -> LuaResult<Vec<Json>> {
    table
        .sequence_values::<Value>()
        .map(|value| to_json(lua, value?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::tests::sandbox;
    use serde_json::json;

    fn eval<T: mlua::FromLuaMulti>(lua: &Lua, source: &str) -> T {
        lua.load(source)
            .eval()
            .unwrap_or_else(|e| panic!("{source}: {e}"))
    }

    #[test]
    fn a_library_results_null_is_nil_and_every_other_null_is_the_sentinel() {
        let lua = sandbox();
        let document = json!({ "handle": null });

        lua.globals()
            .set("result", library_result(&lua, &document).unwrap())
            .unwrap();
        lua.globals()
            .set("decoded", to_lua(&lua, &document).unwrap())
            .unwrap();

        assert!(eval::<bool>(&lua, "return result.handle == nil"));
        assert!(eval::<bool>(&lua, "return decoded.handle ~= nil"));
        assert!(eval::<bool>(
            &lua,
            "return decoded.handle and true or false"
        ));
        // The sentinel encodes back to null, so a decode-encode round trip
        // keeps a field a script never touched.
        let back = to_json(&lua, lua.globals().get("decoded").unwrap()).unwrap();
        assert_eq!(back, document);
    }

    #[test]
    fn an_array_keeps_its_metatable_so_an_emptied_one_is_still_an_array() {
        let lua = sandbox();
        lua.globals()
            .set("list", to_lua(&lua, &json!([1, 2, 3])).unwrap())
            .unwrap();
        lua.load("for i = #list, 1, -1 do list[i] = nil end")
            .exec()
            .unwrap();
        assert_eq!(
            to_json(&lua, lua.globals().get("list").unwrap()).unwrap(),
            json!([])
        );
    }

    #[test]
    fn an_unmarked_empty_table_is_an_object() {
        let lua = sandbox();
        let value: Value = eval(&lua, "return {}");
        assert_eq!(to_json(&lua, value).unwrap(), json!({}));
    }

    #[test]
    fn integers_and_floats_stay_distinct_in_both_directions() {
        let lua = sandbox();
        lua.globals()
            .set("nums", to_lua(&lua, &json!({ "i": 3, "f": 3.5 })).unwrap())
            .unwrap();
        let kinds: Vec<String> = eval(&lua, "return { math.type(nums.i), math.type(nums.f) }");
        assert_eq!(kinds, vec!["integer", "float"]);

        let value: Value = eval(&lua, "return { i = 3, f = 3.5 }");
        let back = to_json(&lua, value).unwrap();
        assert!(back["i"].is_i64(), "{back}");
        assert!(back["f"].is_f64(), "{back}");
    }

    #[test]
    fn a_non_string_key_and_a_recursive_table_are_refused_with_serdes_wording() {
        let lua = sandbox();
        let value: Value = eval(&lua, "return { [5] = 'x' }");
        let error = to_json(&lua, value)
            .expect_err("a number key is not a JSON key")
            .to_string();
        assert!(error.contains("invalid type: integer `5`"), "{error}");

        let value: Value = eval(&lua, "local t = {}; t.self = t; return t");
        let error = to_json(&lua, value)
            .expect_err("a cycle cannot be encoded")
            .to_string();
        assert!(error.contains("recursive table"), "{error}");
    }

    #[test]
    fn a_nil_cuts_an_argument_list_where_mlua_cuts_it() {
        let lua = sandbox();
        let table: Table = eval(&lua, "return { 1, nil, 3 }");
        assert_eq!(sequence_to_json(&lua, &table).unwrap(), vec![json!(1)]);
        let table: Table = eval(&lua, "return { 1, 2, 3 }");
        assert_eq!(
            sequence_to_json(&lua, &table).unwrap(),
            vec![json!(1), json!(2), json!(3)]
        );
    }
}
