//! JSON across the boundary. Written out rather than routed through
//! `JSON.stringify`, because what `stringify` does with a value JSON cannot
//! hold is to drop it without a word — a function-valued field vanishes from
//! a library's arguments, and a `BigInt` throws a message naming neither the
//! field nor the call. Here each is refused, saying what it was.
//!
//! The rest of the rules are `JSON.stringify`'s, so a script author's
//! expectations carry over: `undefined` leaves an object, becomes `null` in an
//! array, and a `toJSON` method (a `Date`'s, say) answers for its object.

use rquickjs::function::This;
use rquickjs::{Array, Ctx, Exception, Object, Result, String as JsString, Value};
use serde_json::{Map, Number, Value as Json};

/// serde_json's own recursion limit, so anything this produces the host can
/// parse back — and a structure deep enough to exhaust the guest's stack is
/// refused before it does.
const MAX_DEPTH: usize = 128;

/// The largest magnitude at which every integer is a distinct `f64`. A float
/// with no fraction inside it crosses as an integer, which is what
/// `JSON.stringify` then serde_json would have made of it.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

pub fn to_js<'js>(ctx: &Ctx<'js>, value: &Json) -> Result<Value<'js>> {
    Ok(match value {
        Json::Null => Value::new_null(ctx.clone()),
        Json::Bool(b) => Value::new_bool(ctx.clone(), *b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => match i32::try_from(i) {
                Ok(small) => Value::new_int(ctx.clone(), small),
                Err(_) => Value::new_float(ctx.clone(), i as f64),
            },
            None => Value::new_float(ctx.clone(), n.as_f64().unwrap_or(f64::NAN)),
        },
        Json::String(s) => JsString::from_str(ctx.clone(), s)?.into_value(),
        Json::Array(items) => {
            let array = Array::new(ctx.clone())?;
            for (i, item) in items.iter().enumerate() {
                array.set(i, to_js(ctx, item)?)?;
            }
            array.into_value()
        }
        Json::Object(fields) => {
            let object = Object::new(ctx.clone())?;
            for (key, field) in fields {
                object.set(key.as_str(), to_js(ctx, field)?)?;
            }
            object.into_value()
        }
    })
}

/// A value as JSON, or a `TypeError` thrown in the script saying why not.
/// `undefined` at the top is `null`: the caller that cares whether a script
/// returned nothing reads that before converting.
pub fn to_json<'js>(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Json> {
    let mut ancestors = Vec::new();
    Ok(convert(ctx, value, "", &mut ancestors)?.unwrap_or(Json::Null))
}

/// The arguments of a call or of one chained step. The list ends at the first
/// `undefined`, which is where the Lua bridge ends it at a `nil` — so
/// `.limit(input.limit)` with no `limit` sends a step with no argument in
/// either language, and a library reads the two the same way.
pub fn arguments<'js>(ctx: &Ctx<'js>, values: Vec<Value<'js>>) -> Result<Vec<Json>> {
    values
        .into_iter()
        .take_while(|value| !value.is_undefined())
        .map(|value| to_json(ctx, value))
        .collect()
}

/// `None` is a value `JSON.stringify` would leave out of an object.
fn convert<'js>(
    ctx: &Ctx<'js>,
    value: Value<'js>,
    key: &str,
    ancestors: &mut Vec<Value<'js>>,
) -> Result<Option<Json>> {
    if value.is_undefined() {
        return Ok(None);
    }
    if value.is_null() {
        return Ok(Some(Json::Null));
    }
    if let Some(b) = value.as_bool() {
        return Ok(Some(Json::Bool(b)));
    }
    if let Some(i) = value.as_int() {
        return Ok(Some(Json::from(i)));
    }
    if let Some(f) = value.as_float() {
        return Ok(Some(number(f)));
    }
    if let Some(s) = value.as_string() {
        return Ok(Some(Json::String(s.to_string()?)));
    }
    if value.is_function() {
        return Err(refuse(ctx, "a function", key));
    }
    if value.is_symbol() {
        return Err(refuse(ctx, "a symbol", key));
    }
    if value.is_big_int() {
        return Err(refuse(ctx, "a BigInt", key));
    }
    let Some(object) = value.as_object() else {
        return Err(refuse(ctx, value.type_name(), key));
    };

    // `toJSON` replaces its object before anything else reads it, and may
    // answer with anything at all, including `undefined`.
    if let Some(to_json) = object.get::<_, Value>("toJSON")?.into_function() {
        let replaced: Value = to_json.call((This(value.clone()), key))?;
        if replaced != value {
            return convert(ctx, replaced, key, ancestors);
        }
    }

    if ancestors.contains(&value) {
        return Err(Exception::throw_type(
            ctx,
            &format!("cannot convert a circular structure to JSON{}", at(key)),
        ));
    }
    if ancestors.len() >= MAX_DEPTH {
        return Err(Exception::throw_range(
            ctx,
            &format!("cannot convert a structure nested deeper than {MAX_DEPTH} levels to JSON"),
        ));
    }
    ancestors.push(value.clone());

    let converted = if let Some(array) = value.as_array() {
        let mut items = Vec::with_capacity(array.len());
        for (i, item) in array.iter::<Value>().enumerate() {
            let index = i.to_string();
            items.push(convert(ctx, item?, &index, ancestors)?.unwrap_or(Json::Null));
        }
        Json::Array(items)
    } else {
        let mut fields = Map::new();
        for entry in object.props::<String, Value>() {
            let (name, field) = entry?;
            if let Some(field) = convert(ctx, field, &name, ancestors)? {
                fields.insert(name, field);
            }
        }
        Json::Object(fields)
    };

    ancestors.pop();
    Ok(Some(converted))
}

/// JavaScript has one number type, so whether `3` was meant as an integer is
/// read from the value. NaN and the infinities are `null`, as `stringify`
/// makes them.
fn number(f: f64) -> Json {
    if f.is_finite() && f.fract() == 0.0 && f.abs() <= MAX_SAFE_INTEGER {
        return Json::from(f as i64);
    }
    Number::from_f64(f).map_or(Json::Null, Json::Number)
}

fn refuse(ctx: &Ctx<'_>, what: &str, key: &str) -> rquickjs::Error {
    Exception::throw_type(ctx, &format!("cannot convert {what} to JSON{}", at(key)))
}

fn at(key: &str) -> String {
    if key.is_empty() {
        String::new()
    } else {
        format!(" (at '{key}')")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::tests::with_context;
    use serde_json::json;

    fn json_of(source: &str) -> std::result::Result<Json, String> {
        with_context(|ctx| {
            let value: Value = ctx.eval(source).map_err(|e| e.to_string())?;
            to_json(&ctx, value).map_err(|_| {
                let thrown = ctx.catch();
                thrown
                    .as_object()
                    .and_then(|o| o.get::<_, String>("message").ok())
                    .unwrap_or_default()
            })
        })
    }

    #[test]
    fn objects_and_arrays_cross_as_themselves() {
        assert_eq!(
            json_of("({ a: 1, b: [true, null, 'x'], c: { d: 2.5 } })").unwrap(),
            json!({ "a": 1, "b": [true, null, "x"], "c": { "d": 2.5 } })
        );
        // No Lua-style ambiguity: an empty array is an array.
        assert_eq!(json_of("([])").unwrap(), json!([]));
        assert_eq!(json_of("({})").unwrap(), json!({}));
    }

    #[test]
    fn undefined_leaves_an_object_and_is_null_in_an_array() {
        assert_eq!(
            json_of("({ a: undefined, b: 1 })").unwrap(),
            json!({ "b": 1 })
        );
        assert_eq!(json_of("([1, undefined, 3])").unwrap(), json!([1, null, 3]));
        assert_eq!(json_of("[1, , 3]").unwrap(), json!([1, null, 3]));
    }

    #[test]
    fn integers_stay_integers_where_serde_json_would_read_one() {
        let value = json_of("({ i: 3, big: 2 ** 40, f: 3.5, neg: -7, whole: 4.0 })").unwrap();
        assert!(value["i"].is_i64(), "{value}");
        assert!(value["big"].is_i64(), "{value}");
        assert!(value["whole"].is_i64(), "{value}");
        assert!(value["f"].is_f64(), "{value}");
        assert_eq!(value["neg"], json!(-7));
        // Past 2^53 a float no longer names one integer, so it stays a float.
        assert!(json_of("2 ** 60").unwrap().is_f64());
        assert_eq!(json_of("[NaN, Infinity]").unwrap(), json!([null, null]));
    }

    #[test]
    fn what_json_cannot_hold_is_refused_naming_it() {
        for (source, expected) in [
            ("({ f() {} })", "cannot convert a function to JSON (at 'f')"),
            (
                "({ s: Symbol('x') })",
                "cannot convert a symbol to JSON (at 's')",
            ),
            ("({ n: 10n })", "cannot convert a BigInt to JSON (at 'n')"),
            ("[1, () => 2]", "cannot convert a function to JSON (at '1')"),
        ] {
            assert_eq!(json_of(source).unwrap_err(), expected, "{source}");
        }
    }

    #[test]
    fn a_cycle_is_refused_and_a_shared_branch_is_not() {
        assert_eq!(
            json_of("const o = {}; o.self = o; o").unwrap_err(),
            "cannot convert a circular structure to JSON (at 'self')"
        );
        assert_eq!(
            json_of("const s = { n: 1 }; ({ a: s, b: s })").unwrap(),
            json!({ "a": { "n": 1 }, "b": { "n": 1 } })
        );
    }

    #[test]
    fn a_structure_deeper_than_serde_json_reads_is_refused() {
        let error = json_of("let v = 1; for (let i = 0; i < 200; i++) v = [v]; v").unwrap_err();
        assert!(error.contains("deeper than 128"), "{error}");
    }

    #[test]
    fn to_json_methods_answer_for_their_object() {
        assert_eq!(
            json_of("({ at: new Date(0) })").unwrap(),
            json!({ "at": "1970-01-01T00:00:00.000Z" })
        );
        assert_eq!(
            json_of("({ a: { toJSON() { return 'mine' } } })").unwrap(),
            json!({ "a": "mine" })
        );
    }

    #[test]
    fn json_round_trips_through_javascript() {
        let document = json!({
            "s": "x", "i": 3, "f": 1.5, "t": true, "n": null,
            "list": [1, { "deep": [] }], "big": 3_000_000_000_i64,
        });
        let back = with_context(|ctx| {
            let value = to_js(&ctx, &document).unwrap();
            to_json(&ctx, value).unwrap()
        });
        assert_eq!(back, document);
    }

    #[test]
    fn an_argument_list_ends_at_the_first_undefined() {
        let args = with_context(|ctx| {
            let values: Vec<Value> = ctx.eval("[1, null, undefined, 3]").unwrap();
            arguments(&ctx, values).unwrap()
        });
        assert_eq!(args, vec![json!(1), json!(null)]);
    }
}
