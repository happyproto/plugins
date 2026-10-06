//! Pure functions over a raw lexicon document — no host calls, so every rule
//! here is unit-testable outside wasm. Mirrors what Lua's `Record:save()` has
//! always done: inject `$type` when absent, fill in schema defaults, and
//! check required fields before a write leaves the plugin.
//!
//! A lexicon document looks like
//! `{ lexicon, id, defs: { main: { type: "record", key, record: { required,
//! properties } } } }`. `None`/`Value::Null` stands for "no lexicon is
//! registered for this collection", which every function here treats as "no
//! constraints" rather than an error — the PDS is the real validator.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use happyview_plugin_sdk::{PluginError, Value};

/// `defs.main.record` of a lexicon document, or `None` when it carries no
/// record schema (including when `lexicon` is `Value::Null`).
fn record_def(lexicon: &Value) -> Option<&Value> {
    lexicon.get("defs")?.get("main")?.get("record")
}

/// The lexicon's `required` field list, or empty when there is none.
pub fn required_fields(lexicon: &Value) -> Vec<String> {
    record_def(lexicon)
        .and_then(|record| record.get("required"))
        .and_then(Value::as_array)
        .map(|fields| {
            fields
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Inject `$type: collection` when the record doesn't already carry one.
/// Non-object records (a script passing something malformed) pass through
/// unchanged — that is `check_required`'s problem, not this function's.
pub fn with_type(collection: &str, mut record: Value) -> Value {
    if let Value::Object(map) = &mut record {
        map.entry("$type")
            .or_insert_with(|| Value::String(collection.to_string()));
    }
    record
}

/// Fill in any property the record is missing with the lexicon's declared
/// default for it.
pub fn populate_defaults(lexicon: &Value, mut record: Value) -> Value {
    let Some(properties) = record_def(lexicon)
        .and_then(|record| record.get("properties"))
        .and_then(Value::as_object)
    else {
        return record;
    };
    if let Value::Object(map) = &mut record {
        for (name, property) in properties {
            if !map.contains_key(name) {
                if let Some(default) = property.get("default") {
                    map.insert(name.clone(), default.clone());
                }
            }
        }
    }
    record
}

/// Every required field the record is missing, or present but `null`. `Ok`
/// when nothing is missing.
pub fn check_required(lexicon: &Value, record: &Value) -> Result<(), Vec<String>> {
    let missing: Vec<String> = required_fields(lexicon)
        .into_iter()
        .filter(|field| {
            record
                .get(field.as_str())
                .map(Value::is_null)
                .unwrap_or(true)
        })
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

/// `with_type` then `populate_defaults`, always; `check_required` unless
/// `skip_check`. What `create`, `put`, `save_local` and `validate` all funnel
/// through, so the four can't drift from each other.
pub fn normalize(
    lexicon: &Value,
    collection: &str,
    record: Value,
    skip_check: bool,
) -> Result<Value, PluginError> {
    let record = with_type(collection, record);
    let record = populate_defaults(lexicon, record);
    if !skip_check {
        if let Err(missing) = check_required(lexicon, &record) {
            return Err(PluginError::new(
                "INVALID_RECORD",
                format!("missing required field(s): {}", missing.join(", ")),
            ));
        }
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use happyview_plugin_sdk::json;

    fn lexicon_with(required: Value, properties: Value) -> Value {
        json!({
            "lexicon": 1,
            "id": "app.test.thing",
            "defs": {
                "main": {
                    "type": "record",
                    "key": "tid",
                    "record": {
                        "type": "object",
                        "required": required,
                        "properties": properties
                    }
                }
            }
        })
    }

    #[test]
    fn required_fields_reads_the_lexicon_list() {
        let lexicon = lexicon_with(json!(["name", "count"]), json!({}));
        assert_eq!(required_fields(&lexicon), alloc::vec!["name", "count"]);
    }

    #[test]
    fn required_fields_is_empty_for_no_lexicon() {
        assert_eq!(required_fields(&Value::Null), Vec::<String>::new());
    }

    #[test]
    fn required_fields_is_empty_when_the_lexicon_declares_none() {
        let lexicon = lexicon_with(json!([]), json!({}));
        assert_eq!(required_fields(&lexicon), Vec::<String>::new());
    }

    #[test]
    fn with_type_injects_the_collection_when_absent() {
        let record = with_type("app.test.thing", json!({"name": "a"}));
        assert_eq!(record, json!({"name": "a", "$type": "app.test.thing"}));
    }

    #[test]
    fn with_type_leaves_an_existing_type_alone() {
        let record = with_type("app.test.thing", json!({"$type": "app.other.thing"}));
        assert_eq!(record["$type"], "app.other.thing");
    }

    #[test]
    fn with_type_ignores_a_non_object_record() {
        let record = with_type("app.test.thing", json!("not an object"));
        assert_eq!(record, json!("not an object"));
    }

    #[test]
    fn populate_defaults_fills_missing_fields() {
        let lexicon = lexicon_with(json!([]), json!({"status": {"default": "draft"}}));
        let record = populate_defaults(&lexicon, json!({"name": "a"}));
        assert_eq!(record, json!({"name": "a", "status": "draft"}));
    }

    #[test]
    fn populate_defaults_never_overwrites_a_present_field() {
        let lexicon = lexicon_with(json!([]), json!({"status": {"default": "draft"}}));
        let record = populate_defaults(&lexicon, json!({"status": "final"}));
        assert_eq!(record["status"], "final");
    }

    #[test]
    fn populate_defaults_is_a_noop_for_no_lexicon() {
        let record = populate_defaults(&Value::Null, json!({"name": "a"}));
        assert_eq!(record, json!({"name": "a"}));
    }

    #[test]
    fn check_required_passes_when_present() {
        let lexicon = lexicon_with(json!(["name", "count"]), json!({}));
        let record = json!({"name": "a", "count": 1});
        assert!(check_required(&lexicon, &record).is_ok());
    }

    #[test]
    fn check_required_fails_when_missing() {
        let lexicon = lexicon_with(json!(["name", "count"]), json!({}));
        let record = json!({"name": "a"});
        assert_eq!(
            check_required(&lexicon, &record).unwrap_err(),
            alloc::vec!["count"]
        );
    }

    #[test]
    fn check_required_treats_null_as_missing() {
        let lexicon = lexicon_with(json!(["name"]), json!({}));
        let record = json!({"name": Value::Null});
        assert_eq!(
            check_required(&lexicon, &record).unwrap_err(),
            alloc::vec!["name"]
        );
    }

    #[test]
    fn check_required_passes_for_no_lexicon() {
        assert!(check_required(&Value::Null, &json!({})).is_ok());
    }

    #[test]
    fn normalize_injects_type_and_defaults_then_checks() {
        let lexicon = lexicon_with(json!(["name"]), json!({"status": {"default": "draft"}}));
        let record = normalize(&lexicon, "app.test.thing", json!({"name": "a"}), false).unwrap();
        assert_eq!(
            record,
            json!({"name": "a", "status": "draft", "$type": "app.test.thing"})
        );
    }

    #[test]
    fn normalize_raises_invalid_record_naming_the_missing_fields() {
        let lexicon = lexicon_with(json!(["name", "count"]), json!({}));
        let err = normalize(&lexicon, "app.test.thing", json!({}), false).unwrap_err();
        assert_eq!(err.code, "INVALID_RECORD");
        assert!(err.message.contains("name"));
        assert!(err.message.contains("count"));
    }

    #[test]
    fn normalize_skips_the_check_when_asked() {
        let lexicon = lexicon_with(json!(["name"]), json!({}));
        let record = normalize(&lexicon, "app.test.thing", json!({}), true).unwrap();
        assert_eq!(record["$type"], "app.test.thing");
    }
}
