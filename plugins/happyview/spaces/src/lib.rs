//! `happyview.spaces`: read and write permissioned spaces — records,
//! membership and invites. A thin translator over fifteen SDK host
//! wrappers; this crate neither looks up spaces nor enforces access itself.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use happyview_plugin_sdk::host;
use happyview_plugin_sdk::{
    json, library_plugin, ApiExport, ApiSurface, CallContext, Map, ObjectCall, PluginError,
    PluginInfo, RecordRef, SpaceDelete, SpaceInfo, SpaceInviteCreate, SpaceInviteInfo,
    SpaceMemberAdd, SpaceMemberInfo, SpaceMemberRemove, SpaceRecordDelete, SpaceRecordInfo,
    SpaceRecordPut, SpaceRecordWrite, SpaceRecordsPage, SpaceUpdate, SpacesAcceptInvite,
    SpacesAccess, SpacesCreate, SpacesInfo, SpacesMembers, SpacesQuery, Value,
};

library_plugin! {
    info: PluginInfo::new("happyview-spaces", "Spaces", "0.1.0"),
    surface: surface,
    call: dispatch,
}

fn surface() -> ApiSurface {
    ApiSurface::new("happyview.spaces")
        .describe("Read and write permissioned spaces: records, membership and invites")
        .export(
            ApiExport::function("create")
                .describe("Create a space")
                .param_json(json!({"name": "opts", "type": "object", "description": "spaceType, skey and optional fields", "properties": [
                    {"name": "spaceType", "type": "string"},
                    {"name": "skey", "type": "string"},
                    {"name": "display_name", "type": "string?"},
                    {"name": "description", "type": "string?"},
                    {"name": "read_policy", "type": "object?"},
                    {"name": "write_policy", "type": "object?"},
                    {"name": "app_access", "type": "object?"},
                    {"name": "config", "type": "object?"}
                ]}))
                .returns(space_shape()),
        )
        .export(
            ApiExport::function("accept_invite")
                .describe("Redeem an invite token, joining the space it names")
                .param_json(json!({"name": "opts", "type": "object", "properties": [
                    {"name": "token", "type": "string"}
                ]}))
                .returns(space_shape()),
        )
        .export(
            ApiExport::function("info")
                .describe("A space's fields by URI, or nil")
                .param("uri", "string", "Space AT URI")
                .returns(json!({"anyOf": [space_shape(), {"type": "null"}]})),
        )
        .export(
            ApiExport::function("query")
                .describe("Page through a space's records")
                .param_json(json!({"name": "opts", "type": "object", "properties": [
                    {"name": "uri", "type": "string"},
                    {"name": "collection", "type": "string?"},
                    {"name": "limit", "type": "number?"},
                    {"name": "cursor", "type": "string?"}
                ]}))
                .returns(records_page_shape()),
        )
        .export(
            ApiExport::constructor("get")
                .describe("Act on one space, by URI")
                .param("uri", "string", "Space AT URI")
                .immediate("write_record")
                .immediate("put_record")
                .immediate("delete_record")
                .immediate("add_member")
                .immediate("set_member")
                .immediate("put_member")
                .immediate("remove_member")
                .immediate("update")
                .immediate("delete")
                .immediate("create_invite")
                .immediate("members")
                .immediate("is_member")
                .immediate("access")
                .immediate("records"),
        )
}

fn space_shape() -> Value {
    json!({"type": "object", "properties": [
        {"name": "uri", "type": "string"},
        {"name": "id", "type": "string"},
        {"name": "did", "type": "string"},
        {"name": "authority_did", "type": "string"},
        {"name": "creator_did", "type": "string"},
        {"name": "spaceType", "type": "string"},
        {"name": "skey", "type": "string"},
        {"name": "display_name", "type": "string?"},
        {"name": "description", "type": "string?"},
        {"name": "read_policy", "type": "object"},
        {"name": "write_policy", "type": "object"},
        {"name": "app_access", "type": "object"},
        {"name": "config", "type": "object"},
        {"name": "revision", "type": "string?"},
        {"name": "created_at", "type": "string"},
        {"name": "updated_at", "type": "string"}
    ]})
}

fn record_shape() -> Value {
    json!({"type": "object", "properties": [
        {"name": "uri", "type": "string"},
        {"name": "collection", "type": "string"},
        {"name": "rkey", "type": "string"},
        {"name": "record", "type": "object"},
        {"name": "cid", "type": "string"},
        {"name": "author_did", "type": "string"}
    ]})
}

fn records_page_shape() -> Value {
    json!({"type": "object", "properties": [
        {"name": "records", "type": "array", "items": record_shape()},
        {"name": "cursor", "type": "string?"}
    ]})
}

fn dispatch(function: &str, args: &[Value], _ctx: &CallContext) -> Result<Value, PluginError> {
    match function {
        "create" => create(args),
        "accept_invite" => accept_invite(args),
        "info" => info(args),
        "query" => query(args),
        "get" => get(&ObjectCall::from_args(args)?),
        other => Err(PluginError::unknown_function(other)),
    }
}

fn create(args: &[Value]) -> Result<Value, PluginError> {
    let table = table_arg(args, 0)?;
    let type_nsid = required_str_field(table, "spaceType")?;
    let skey = required_str_field(table, "skey")?;
    let display_name = optional_str_field(table, "display_name");
    let description = optional_str_field(table, "description");
    let read_policy = optional_value_field(table, "read_policy");
    let write_policy = optional_value_field(table, "write_policy");
    let app_access = optional_value_field(table, "app_access");
    let config = optional_value_field(table, "config");

    let result = host::spaces_create(&SpacesCreate {
        type_nsid,
        skey,
        display_name,
        description,
        read_policy,
        write_policy,
        app_access,
        config,
    })?;
    Ok(space_value(result))
}

fn accept_invite(args: &[Value]) -> Result<Value, PluginError> {
    let table = table_arg(args, 0)?;
    let token = required_str_field(table, "token")?;
    let result = host::spaces_accept_invite(&SpacesAcceptInvite { token })?;
    Ok(space_value(result))
}

fn info(args: &[Value]) -> Result<Value, PluginError> {
    let uri = str_arg(args, 0, "uri")?;
    let result = host::spaces_info(&SpacesInfo { uri })?;
    Ok(result.map(space_value).unwrap_or(Value::Null))
}

fn query(args: &[Value]) -> Result<Value, PluginError> {
    let table = table_arg(args, 0)?;
    let uri = required_str_field(table, "uri")?;
    let collection = optional_str_field(table, "collection");
    let limit = optional_i64_field(table, "limit");
    let cursor = optional_str_field(table, "cursor");

    let result = host::spaces_query(&SpacesQuery {
        uri,
        collection,
        limit,
        cursor,
    })?;
    Ok(records_page_value(result))
}

/// `get(uri)` returns no object of its own — every method re-resolves the
/// space, so there is nothing here to cache between calls. `get` itself
/// never fails; an unusable or missing URI only ever surfaces as
/// `NOT_FOUND` on the first method call against it.
fn get(call: &ObjectCall) -> Result<Value, PluginError> {
    if !call.steps.is_empty() {
        return Err(PluginError::bad_input(
            "get() takes no lazy steps; call a method directly",
        ));
    }
    let uri = call
        .args
        .first()
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| PluginError::bad_input("uri is required"))?;

    let method_args = call.call.args.as_slice();
    match call.call.name.as_str() {
        "write_record" => write_record(&uri, method_args),
        "put_record" => put_record(&uri, method_args),
        "delete_record" => delete_record(&uri, method_args),
        "add_member" => add_member(&uri, method_args),
        // `put_member` is what the upsert is called on the HTTP surface and
        // in scripts written against it, and the codemod renames no handle
        // method but `query`.
        "set_member" | "put_member" => set_member(&uri, method_args),
        "remove_member" => remove_member(&uri, method_args),
        "update" => update(&uri, method_args),
        "delete" => delete_space(&uri),
        "create_invite" => create_invite(&uri, method_args),
        "members" => members(&uri),
        "is_member" => is_member(&uri, method_args),
        "access" => access(&uri, method_args),
        "records" => records(&uri, method_args),
        other => Err(PluginError::unknown_function(other)),
    }
}

fn write_record(uri: &str, args: &[Value]) -> Result<Value, PluginError> {
    let table = table_arg(args, 0)?;
    let collection = required_str_field(table, "collection")?;
    let record = required_object_field(table, "record")?;

    let result = host::spaces_write_record(&SpaceRecordWrite {
        uri: uri.to_string(),
        collection,
        record,
    })?;
    Ok(record_ref_value(result))
}

fn put_record(uri: &str, args: &[Value]) -> Result<Value, PluginError> {
    let table = table_arg(args, 0)?;
    let collection = required_str_field(table, "collection")?;
    let rkey = required_str_field(table, "rkey")?;
    let record = required_object_field(table, "record")?;
    let swap_cid = optional_str_field(table, "swap_cid");

    let result = host::spaces_put_record(&SpaceRecordPut {
        uri: uri.to_string(),
        collection,
        rkey,
        record,
        swap_cid,
    })?;
    Ok(record_ref_value(result))
}

fn delete_record(uri: &str, args: &[Value]) -> Result<Value, PluginError> {
    let table = table_arg(args, 0)?;
    let collection = required_str_field(table, "collection")?;
    let rkey = required_str_field(table, "rkey")?;
    let swap_cid = optional_str_field(table, "swap_cid");

    host::spaces_delete_record(&SpaceRecordDelete {
        uri: uri.to_string(),
        collection,
        rkey,
        swap_cid,
    })?;
    Ok(Value::Bool(true))
}

fn add_member(uri: &str, args: &[Value]) -> Result<Value, PluginError> {
    let spec = member_add_spec(uri, args)?;
    let result = host::spaces_add_member(&spec)?;
    Ok(member_value(result))
}

fn set_member(uri: &str, args: &[Value]) -> Result<Value, PluginError> {
    let spec = member_add_spec(uri, args)?;
    let result = host::spaces_set_member(&spec)?;
    Ok(member_value(result))
}

/// Access is named either as `read`/`write` or as the `access` word; the
/// host refuses half a pair and refuses a pair beside a word, so neither
/// rule is restated here.
fn member_add_spec(uri: &str, args: &[Value]) -> Result<SpaceMemberAdd, PluginError> {
    let table = table_arg(args, 0)?;
    let did = required_str_field(table, "did")?;
    let access = optional_str_field(table, "access");
    let read = optional_bool_field(table, "read");
    let write = optional_bool_field(table, "write");
    let is_delegation = optional_bool_field(table, "is_delegation");
    Ok(SpaceMemberAdd {
        uri: uri.to_string(),
        did,
        access,
        read,
        write,
        is_delegation,
    })
}

fn remove_member(uri: &str, args: &[Value]) -> Result<Value, PluginError> {
    let table = table_arg(args, 0)?;
    let did = required_str_field(table, "did")?;
    host::spaces_remove_member(&SpaceMemberRemove {
        uri: uri.to_string(),
        did,
    })?;
    Ok(Value::Bool(true))
}

fn update(uri: &str, args: &[Value]) -> Result<Value, PluginError> {
    let spec = update_spec(uri, args)?;
    let result = host::spaces_update(&spec)?;
    Ok(space_value(result))
}

/// Builds `SpaceUpdate` by deserialising the method's table straight into it
/// after inserting `uri`, so `display_name`/`description`'s three-way patch
/// rule (absent = unchanged, `false`/`null` = clear, string = set) applies
/// exactly as the wire type defines it, with no restating of that rule here.
/// `uri` is inserted last and always wins: a table can't carry its own `uri`
/// to redirect the update at a different space than the handle names.
fn update_spec(uri: &str, args: &[Value]) -> Result<SpaceUpdate, PluginError> {
    let mut map = object_or_empty(args, 0)?;
    map.insert("uri".to_string(), Value::String(uri.to_string()));
    Ok(serde_json::from_value(Value::Object(map))?)
}

fn delete_space(uri: &str) -> Result<Value, PluginError> {
    host::spaces_delete(&SpaceDelete {
        uri: uri.to_string(),
    })?;
    Ok(Value::Bool(true))
}

/// Access follows `member_add_spec`'s rule.
fn create_invite(uri: &str, args: &[Value]) -> Result<Value, PluginError> {
    let spec = invite_create_spec(uri, args)?;
    let result = host::spaces_create_invite(&spec)?;
    Ok(invite_value(result))
}

fn invite_create_spec(uri: &str, args: &[Value]) -> Result<SpaceInviteCreate, PluginError> {
    let map = object_or_empty(args, 0)?;
    let table = Value::Object(map);
    Ok(SpaceInviteCreate {
        uri: uri.to_string(),
        access: optional_str_field(&table, "access"),
        read: optional_bool_field(&table, "read"),
        write: optional_bool_field(&table, "write"),
        max_uses: optional_i64_field(&table, "max_uses"),
        expires_at: optional_str_field(&table, "expires_at"),
    })
}

fn members(uri: &str) -> Result<Value, PluginError> {
    let result = host::spaces_members(&SpacesMembers {
        uri: uri.to_string(),
    })?;
    Ok(Value::Array(result.into_iter().map(member_value).collect()))
}

/// Derived from `access`, the same host lookup `access()` uses: a DID is a
/// member exactly when it has an access level at all.
fn is_member(uri: &str, args: &[Value]) -> Result<Value, PluginError> {
    let did = str_arg(args, 0, "did")?;
    let result = host::spaces_access(&SpacesAccess {
        uri: uri.to_string(),
        did,
    })?;
    Ok(Value::Bool(result.is_some()))
}

fn access(uri: &str, args: &[Value]) -> Result<Value, PluginError> {
    let did = str_arg(args, 0, "did")?;
    let result = host::spaces_access(&SpacesAccess {
        uri: uri.to_string(),
        did,
    })?;
    Ok(result.map(Value::String).unwrap_or(Value::Null))
}

fn records(uri: &str, args: &[Value]) -> Result<Value, PluginError> {
    let map = object_or_empty(args, 0)?;
    let table = Value::Object(map);
    let collection = optional_str_field(&table, "collection");
    let limit = optional_i64_field(&table, "limit");
    let cursor = optional_str_field(&table, "cursor");

    let result = host::spaces_query(&SpacesQuery {
        uri: uri.to_string(),
        collection,
        limit,
        cursor,
    })?;
    Ok(records_page_value(result))
}

fn space_value(space: SpaceInfo) -> Value {
    json!({
        "uri": space.uri,
        "id": space.id,
        "did": space.did,
        "authority_did": space.authority_did,
        "creator_did": space.creator_did,
        "spaceType": space.type_nsid,
        "skey": space.skey,
        "display_name": space.display_name,
        "description": space.description,
        "read_policy": space.read_policy,
        "write_policy": space.write_policy,
        "app_access": space.app_access,
        "config": space.config,
        "revision": space.revision,
        "created_at": space.created_at,
        "updated_at": space.updated_at,
    })
}

/// `read` and `write` are the member's actual pair; `access` is the nearest
/// word for it, which cannot distinguish a write-only member from a
/// read/write one.
fn member_value(member: SpaceMemberInfo) -> Value {
    json!({
        "did": member.did,
        "access": member.access,
        "read": member.read,
        "write": member.write,
    })
}

fn invite_value(invite: SpaceInviteInfo) -> Value {
    json!({
        "invite_id": invite.invite_id,
        "token": invite.token,
        "access": invite.access,
        "read": invite.read,
        "write": invite.write,
        "max_uses": invite.max_uses,
        "expires_at": invite.expires_at,
    })
}

fn space_record_value(record: SpaceRecordInfo) -> Value {
    json!({
        "uri": record.uri,
        "collection": record.collection,
        "rkey": record.rkey,
        "record": record.record,
        "cid": record.cid,
        "author_did": record.author_did,
    })
}

fn records_page_value(page: SpaceRecordsPage) -> Value {
    let records: Vec<Value> = page.records.into_iter().map(space_record_value).collect();
    json!({"records": records, "cursor": page.cursor})
}

fn record_ref_value(record_ref: RecordRef) -> Value {
    json!({"uri": record_ref.uri, "cid": record_ref.cid})
}

fn str_arg(args: &[Value], index: usize, name: &str) -> Result<String, PluginError> {
    args.get(index)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| PluginError::bad_input(format!("{name} is required")))
}

/// A method's one table argument (`write_record`, `put_record`,
/// `add_member`, `query`, ...). Not optional — every one of these methods
/// needs at least the fields it requires from it.
fn table_arg(args: &[Value], index: usize) -> Result<&Value, PluginError> {
    match args.get(index) {
        Some(value @ Value::Object(_)) => Ok(value),
        _ => Err(PluginError::bad_input(
            "expected a table argument with the method's fields",
        )),
    }
}

/// An optional table argument (`update`, `create_invite`, `records`) whose
/// fields are all themselves optional, so calling the method with no
/// argument is a valid no-op-shaped request rather than a `BAD_INPUT`.
fn object_or_empty(args: &[Value], index: usize) -> Result<Map<String, Value>, PluginError> {
    match args.get(index) {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(map)) => Ok(map.clone()),
        _ => Err(PluginError::bad_input("opts must be an object")),
    }
}

fn required_str_field(table: &Value, key: &str) -> Result<String, PluginError> {
    table
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| PluginError::bad_input(format!("{key} is required")))
}

fn optional_str_field(table: &Value, key: &str) -> Option<String> {
    table
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn optional_i64_field(table: &Value, key: &str) -> Option<i64> {
    table.get(key).and_then(Value::as_i64)
}

fn optional_bool_field(table: &Value, key: &str) -> Option<bool> {
    table.get(key).and_then(Value::as_bool)
}

fn required_object_field(table: &Value, key: &str) -> Result<Value, PluginError> {
    match table.get(key) {
        Some(value @ Value::Object(_)) => Ok(value.clone()),
        _ => Err(PluginError::bad_input(format!(
            "{key} is required and must be an object"
        ))),
    }
}

/// A field that is either absent or `null` reads the same as not having been
/// passed at all — the same rule every optional field in this crate follows,
/// `update`'s `Patch` fields excepted (see `update`'s doc comment).
fn optional_value_field(table: &Value, key: &str) -> Option<Value> {
    match table.get(key) {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.clone()),
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    #[test]
    fn str_arg_reads_the_indexed_string() {
        let args = [json!("at://did:plc:abc/space/x/y")];
        assert_eq!(
            str_arg(&args, 0, "uri").unwrap(),
            "at://did:plc:abc/space/x/y"
        );
    }

    #[test]
    fn str_arg_rejects_a_missing_value() {
        let args: [Value; 0] = [];
        let err = str_arg(&args, 0, "uri").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn table_arg_rejects_a_missing_value() {
        let args: [Value; 0] = [];
        let err = table_arg(&args, 0).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn table_arg_rejects_a_non_object() {
        let args = [json!("not a table")];
        let err = table_arg(&args, 0).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn table_arg_passes_through_an_object() {
        let args = [json!({"collection": "app.test.thing"})];
        assert_eq!(
            table_arg(&args, 0).unwrap(),
            &json!({"collection": "app.test.thing"})
        );
    }

    #[test]
    fn object_or_empty_defaults_to_empty_when_absent() {
        let args: [Value; 0] = [];
        assert_eq!(object_or_empty(&args, 0).unwrap(), Map::new());
    }

    #[test]
    fn object_or_empty_defaults_to_empty_for_null() {
        let args = [Value::Null];
        assert_eq!(object_or_empty(&args, 0).unwrap(), Map::new());
    }

    #[test]
    fn object_or_empty_passes_through_an_object() {
        let args = [json!({"access": "read"})];
        let map = object_or_empty(&args, 0).unwrap();
        assert_eq!(map.get("access"), Some(&json!("read")));
    }

    #[test]
    fn object_or_empty_rejects_a_non_object() {
        let args = [json!("not an object")];
        let err = object_or_empty(&args, 0).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn required_str_field_reads_a_present_key() {
        let table = json!({"did": "did:plc:abc"});
        assert_eq!(required_str_field(&table, "did").unwrap(), "did:plc:abc");
    }

    #[test]
    fn required_str_field_names_a_missing_key() {
        let table = json!({});
        let err = required_str_field(&table, "did").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
        assert!(err.message.contains("did"));
    }

    #[test]
    fn optional_str_field_reads_a_present_key() {
        let table = json!({"cursor": "abc"});
        assert_eq!(
            optional_str_field(&table, "cursor"),
            Some("abc".to_string())
        );
    }

    #[test]
    fn optional_str_field_is_none_when_absent() {
        let table = json!({});
        assert_eq!(optional_str_field(&table, "cursor"), None);
    }

    #[test]
    fn optional_i64_field_reads_a_present_key() {
        let table = json!({"limit": 50});
        assert_eq!(optional_i64_field(&table, "limit"), Some(50));
    }

    #[test]
    fn optional_i64_field_is_none_when_absent() {
        let table = json!({});
        assert_eq!(optional_i64_field(&table, "limit"), None);
    }

    #[test]
    fn optional_bool_field_reads_a_present_key() {
        let table = json!({"is_delegation": true});
        assert_eq!(optional_bool_field(&table, "is_delegation"), Some(true));
    }

    #[test]
    fn optional_bool_field_is_none_when_absent() {
        let table = json!({});
        assert_eq!(optional_bool_field(&table, "is_delegation"), None);
    }

    #[test]
    fn required_object_field_reads_a_present_object() {
        let table = json!({"record": {"text": "hi"}});
        assert_eq!(
            required_object_field(&table, "record").unwrap(),
            json!({"text": "hi"})
        );
    }

    #[test]
    fn required_object_field_names_a_missing_key() {
        let table = json!({});
        let err = required_object_field(&table, "record").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
        assert!(err.message.contains("record"));
    }

    #[test]
    fn required_object_field_rejects_a_non_object() {
        let table = json!({"record": "not an object"});
        let err = required_object_field(&table, "record").unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn optional_value_field_reads_a_present_key() {
        let table = json!({"read_policy": {"kind": "open"}});
        assert_eq!(
            optional_value_field(&table, "read_policy"),
            Some(json!({"kind": "open"}))
        );
    }

    #[test]
    fn optional_value_field_is_none_when_absent() {
        let table = json!({});
        assert_eq!(optional_value_field(&table, "read_policy"), None);
    }

    #[test]
    fn optional_value_field_is_none_for_null() {
        let table = json!({"read_policy": null});
        assert_eq!(optional_value_field(&table, "read_policy"), None);
    }

    #[test]
    fn get_rejects_lazy_steps() {
        let call = ObjectCall {
            args: vec![json!("at://did:plc:abc/space/x/y")],
            steps: vec![happyview_plugin_sdk::Step {
                name: "where".to_string(),
                args: vec![],
            }],
            call: happyview_plugin_sdk::MethodCall {
                name: "members".to_string(),
                args: vec![],
            },
        };
        let err = get(&call).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn get_requires_a_uri() {
        let call = ObjectCall {
            args: vec![],
            steps: vec![],
            call: happyview_plugin_sdk::MethodCall {
                name: "members".to_string(),
                args: vec![],
            },
        };
        let err = get(&call).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
    }

    #[test]
    fn update_spec_ignores_a_uri_in_the_table_and_uses_the_handles() {
        use happyview_plugin_sdk::Patch;

        // The table carries a *different* uri than the handle's; the
        // handle's must win, not the table's, or a script could redirect
        // an update at a space other than the one it called `get` on.
        let args = [json!({
            "uri": "at://did:plc:other/space/x/y",
            "display_name": "New name",
        })];
        let spec = update_spec("at://did:plc:abc/space/x/y", &args).unwrap();
        assert_eq!(spec.uri, "at://did:plc:abc/space/x/y");
        assert_eq!(spec.display_name, Patch::Set("New name".to_string()));
        assert_eq!(spec.description, Patch::Unchanged);
    }

    #[test]
    fn update_spec_reads_the_patch_states() {
        use happyview_plugin_sdk::Patch;

        let cleared = update_spec(
            "at://did:plc:abc/space/x/y",
            &[json!({"display_name": false})],
        )
        .unwrap();
        assert_eq!(cleared.display_name, Patch::Clear);

        let unchanged = update_spec("at://did:plc:abc/space/x/y", &[json!({})]).unwrap();
        assert_eq!(unchanged.display_name, Patch::Unchanged);
    }

    /// A space's type is `spaceType` everywhere a script meets it: the
    /// parameter `create` reads, the field a space carries back, and the
    /// surface that documents both.
    #[test]
    fn a_space_type_is_named_spacetype_on_every_surface() {
        let space = space_value(SpaceInfo {
            uri: "at://did:plc:abc/space/app.example.thing/s1".to_string(),
            id: "space-1".to_string(),
            did: "did:plc:abc".to_string(),
            authority_did: "did:plc:abc".to_string(),
            creator_did: "did:plc:abc".to_string(),
            type_nsid: "app.example.thing".to_string(),
            skey: "s1".to_string(),
            display_name: None,
            description: None,
            read_policy: json!({}),
            write_policy: json!({}),
            app_access: json!({}),
            config: json!({}),
            revision: None,
            created_at: "2026-10-02T00:00:00Z".to_string(),
            updated_at: "2026-10-02T00:00:00Z".to_string(),
        });
        assert_eq!(space["spaceType"], json!("app.example.thing"));
        assert_eq!(space.get("type"), None);

        let create = surface()
            .exports
            .into_iter()
            .find(|export| export.name == "create")
            .expect("create export");
        let properties = create.params[0]["properties"]
            .as_array()
            .expect("create's opts properties")
            .clone();
        let names: Vec<&str> = properties
            .iter()
            .filter_map(|property| property["name"].as_str())
            .collect();
        assert!(names.contains(&"spaceType"), "{names:?}");
        assert!(!names.contains(&"type"), "{names:?}");
    }

    #[test]
    fn member_add_spec_reads_required_and_optional_fields() {
        let args = [json!({"did": "did:plc:abc", "access": "write"})];
        let spec = member_add_spec("at://did:plc:xyz/space/t/s", &args).unwrap();
        assert_eq!(spec.uri, "at://did:plc:xyz/space/t/s");
        assert_eq!(spec.did, "did:plc:abc");
        assert_eq!(spec.access, Some("write".to_string()));
        assert_eq!(spec.read, None);
        assert_eq!(spec.write, None);
        assert_eq!(spec.is_delegation, None);
    }

    /// The pair the `access` word cannot name, which is why it is carried
    /// separately at all.
    #[test]
    fn member_add_spec_carries_a_write_only_pair() {
        let args = [json!({"did": "did:plc:abc", "read": false, "write": true})];
        let spec = member_add_spec("at://did:plc:xyz/space/t/s", &args).unwrap();
        assert_eq!(spec.access, None);
        assert_eq!(spec.read, Some(false));
        assert_eq!(spec.write, Some(true));
    }

    #[test]
    fn invite_create_spec_carries_a_write_only_pair() {
        let args = [json!({"read": false, "write": true, "max_uses": 3})];
        let spec = invite_create_spec("at://did:plc:xyz/space/t/s", &args).unwrap();
        assert_eq!(spec.uri, "at://did:plc:xyz/space/t/s");
        assert_eq!(spec.access, None);
        assert_eq!(spec.read, Some(false));
        assert_eq!(spec.write, Some(true));
        assert_eq!(spec.max_uses, Some(3));
    }

    #[test]
    fn invite_create_spec_reads_the_access_word() {
        let args = [json!({"access": "read"})];
        let spec = invite_create_spec("at://did:plc:xyz/space/t/s", &args).unwrap();
        assert_eq!(spec.access, Some("read".to_string()));
        assert_eq!(spec.read, None);
        assert_eq!(spec.write, None);
    }

    #[test]
    fn member_value_carries_the_pair_beside_the_word() {
        let value = member_value(SpaceMemberInfo {
            did: "did:plc:abc".to_string(),
            access: "write".to_string(),
            read: false,
            write: true,
        });
        assert_eq!(value["read"], json!(false));
        assert_eq!(value["write"], json!(true));
        assert_eq!(value["access"], json!("write"));
    }

    #[test]
    fn invite_value_carries_the_pair_beside_the_word() {
        let value = invite_value(SpaceInviteInfo {
            invite_id: "inv-1".to_string(),
            token: "tok".to_string(),
            access: "write".to_string(),
            read: false,
            write: true,
            max_uses: None,
            expires_at: None,
        });
        assert_eq!(value["read"], json!(false));
        assert_eq!(value["write"], json!(true));
    }

    /// `put_member` and `set_member` are the same method under two names, so
    /// neither may be the one that reaches `unknown_function`. Off wasm there
    /// is no host to call, so both stop at the same `HOST_ERROR` — which an
    /// unrouted name never reaches.
    #[test]
    fn put_member_and_set_member_dispatch_alike() {
        let call_named = |name: &str| {
            get(&ObjectCall {
                args: vec![json!("at://did:plc:abc/space/x/y")],
                steps: vec![],
                call: happyview_plugin_sdk::MethodCall {
                    name: name.to_string(),
                    args: vec![json!({"did": "did:plc:member", "read": false, "write": true})],
                },
            })
        };

        let set = call_named("set_member").unwrap_err();
        let put = call_named("put_member").unwrap_err();
        assert_eq!(put.code, set.code);
        assert_eq!(put.message, set.message);
        assert_ne!(put.code, "UNKNOWN_FUNCTION");

        let unknown = call_named("put_memberr").unwrap_err();
        assert_eq!(unknown.code, "UNKNOWN_FUNCTION");
    }

    #[test]
    fn member_add_spec_names_a_missing_did() {
        let args = [json!({})];
        let err = member_add_spec("at://did:plc:xyz/space/t/s", &args).unwrap_err();
        assert_eq!(err.code, "BAD_INPUT");
        assert!(err.message.contains("did"));
    }
}
