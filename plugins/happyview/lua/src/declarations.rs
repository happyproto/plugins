//! `get_declarations`: what a script can see, as LuaLS `---@meta` files.
//!
//! - `happyview/<ns>.lua` per library, so `require("happyview.db")` resolves
//!   to its module table.
//! - `lexicons/<nsid as a path>.lua` per lexicon: `lexicons.<nsid>.Params`,
//!   `Input`, `Output`, `Record` and `Event` for its main def, and
//!   `lexicons.<nsid>.<def>` for every other.
//! - `happyview.lua`, which no `require` names: `happyview.QueryCtx` and the
//!   other `ctx` classes, the event classes, and the shapes of blobs, CID
//!   links and bytes.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use happyview_plugin_sdk::declarations::{
    DeclarationFile, Declarations, DeclarationsInput, LexiconDeclaration, ObjectType, Property,
    TypeExpr, TypedExport, TypedSurface,
};
use happyview_plugin_sdk::PluginError;

pub fn declarations(input: &DeclarationsInput) -> Result<Declarations, PluginError> {
    Ok(render(input))
}

pub fn render(input: &DeclarationsInput) -> Declarations {
    let lexicons: BTreeMap<&str, &LexiconDeclaration> = input
        .lexicons
        .iter()
        .map(|l| (l.nsid.as_str(), l))
        .collect();
    let mut files = vec![DeclarationFile {
        path: "happyview.lua".into(),
        contents: host_file(input, &lexicons),
    }];
    for library in &input.libraries {
        files.push(DeclarationFile {
            path: format!("{}.lua", library.namespace.replace('.', "/")),
            contents: library_file(&library.namespace, &library.surface),
        });
    }
    for lexicon in &input.lexicons {
        files.push(DeclarationFile {
            path: format!("lexicons/{}.lua", lexicon.nsid.replace('.', "/")),
            contents: lexicon_file(lexicon, &input.events.record, &lexicons),
        });
    }
    Declarations { files }
}

/// How a name in a type is spelled: a surface's named types are its own
/// classes, a lexicon ref is that lexicon's class.
enum Names<'a> {
    Library(&'a str),
    Lexicons(&'a BTreeMap<&'a str, &'a LexiconDeclaration>),
}

impl Names<'_> {
    fn reference(&self, name: &str) -> String {
        match self {
            Names::Library(namespace) => format!("{namespace}.{name}"),
            Names::Lexicons(lexicons) => lexicon_ref(lexicons, name),
        }
    }
}

/// `nsid#def` as the class that file declares, or `any` for a lexicon the
/// project does not have.
fn lexicon_ref(lexicons: &BTreeMap<&str, &LexiconDeclaration>, name: &str) -> String {
    let (nsid, def) = name.split_once('#').unwrap_or((name, "main"));
    let Some(lexicon) = lexicons.get(nsid) else {
        return "any".into();
    };
    if def == "main" {
        match lexicon.kind.as_str() {
            "record" => format!("lexicons.{nsid}.Record"),
            _ if lexicon.defs.contains_key("main") => format!("lexicons.{nsid}.main"),
            _ => "any".into(),
        }
    } else if lexicon.defs.contains_key(def) {
        format!("lexicons.{nsid}.{def}")
    } else {
        "any".into()
    }
}

fn ty(t: &TypeExpr, names: &Names) -> String {
    match t {
        TypeExpr::String { known_values, .. } if !known_values.is_empty() => {
            let mut parts: Vec<String> = known_values.iter().map(|v| quote(v)).collect();
            parts.push("string".into());
            parts.join("|")
        }
        TypeExpr::String { .. } => "string".into(),
        TypeExpr::Integer => "integer".into(),
        TypeExpr::Number => "number".into(),
        TypeExpr::Boolean => "boolean".into(),
        TypeExpr::Bytes => "happyview.Bytes".into(),
        TypeExpr::Blob => "happyview.Blob".into(),
        TypeExpr::CidLink => "happyview.CidLink".into(),
        TypeExpr::Null => "nil".into(),
        TypeExpr::Unknown | TypeExpr::Any | TypeExpr::Var { .. } => "any".into(),
        TypeExpr::Literal { value } => match value {
            serde_json::Value::String(s) => quote(s),
            other => other.to_string(),
        },
        TypeExpr::Array { items } => format!("{}[]", grouped(items, names)),
        TypeExpr::Map { values } => format!("table<string, {}>", ty(values, names)),
        TypeExpr::Object(object) if object.properties.is_empty() => "table".into(),
        TypeExpr::Object(object) => {
            let fields: Vec<String> = object
                .properties
                .iter()
                .map(|p| {
                    let optional = if p.required { "" } else { "?" };
                    format!("{}{optional}: {}", field_name(&p.name), ty(&p.ty, names))
                })
                .collect();
            format!("{{ {} }}", fields.join(", "))
        }
        TypeExpr::Union { members, open } => {
            let mut parts: Vec<String> = Vec::new();
            for member in members {
                let rendered = ty(member, names);
                if rendered == "any" {
                    return rendered;
                }
                if !parts.contains(&rendered) {
                    parts.push(rendered);
                }
            }
            if *open {
                parts.push("table".into());
            }
            parts.join("|")
        }
        TypeExpr::Ref { name } => names.reference(name),
    }
}

/// A member of `T[]`, parenthesised when it is a union.
fn grouped(t: &TypeExpr, names: &Names) -> String {
    let rendered = ty(t, names);
    if rendered.contains('|') {
        format!("({rendered})")
    } else {
        rendered
    }
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A field LuaLS reads as a name, or `["odd-name"]` when it is not one.
fn field_name(name: &str) -> String {
    if is_identifier(name) {
        name.to_string()
    } else {
        format!("[{}]", quote(name))
    }
}

/// A parameter name a Lua function can declare.
fn param_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if KEYWORDS.contains(&out.as_str()) {
        out.push('_');
    }
    out
}

fn is_identifier(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !KEYWORDS.contains(&name)
}

const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn doc(out: &mut String, description: Option<&str>) {
    if let Some(description) = description.filter(|d| !d.trim().is_empty()) {
        for line in description.lines() {
            let _ = writeln!(out, "--- {}", line.trim_end());
        }
    }
}

/// `---@class` with a `---@field` per property, or `---@alias` for anything
/// that is not an object.
fn declare(out: &mut String, name: &str, description: Option<&str>, t: &TypeExpr, names: &Names) {
    doc(out, description);
    match t {
        TypeExpr::Object(object) => class(out, name, object, names),
        other => {
            let _ = writeln!(out, "---@alias {name} {}", ty(other, names));
        }
    }
    out.push('\n');
}

fn class(out: &mut String, name: &str, object: &ObjectType, names: &Names) {
    let _ = writeln!(out, "---@class {name}");
    for property in &object.properties {
        fields(out, property, names);
    }
}

fn fields(out: &mut String, p: &Property, names: &Names) {
    let optional = if p.required { "" } else { "?" };
    let description = p
        .description
        .as_deref()
        .map(|d| format!(" {}", one_line(d)))
        .unwrap_or_default();
    let _ = writeln!(
        out,
        "---@field {}{optional} {}{description}",
        field_name(&p.name),
        ty(&p.ty, names)
    );
}

fn params(out: &mut String, params: &[Property], names: &Names) -> Vec<String> {
    params
        .iter()
        .map(|p| {
            let name = param_name(&p.name);
            let optional = if p.required { "" } else { "?" };
            let description = p
                .description
                .as_deref()
                .map(|d| format!(" {}", one_line(d)))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "---@param {name}{optional} {}{description}",
                ty(&p.ty, names)
            );
            name
        })
        .collect()
}

fn library_file(namespace: &str, surface: &TypedSurface) -> String {
    let names = Names::Library(namespace);
    let mut out = format!("---@meta {namespace}\n\n");
    for named in &surface.types {
        declare(
            &mut out,
            &format!("{namespace}.{}", named.name),
            named.description.as_deref(),
            &named.ty,
            &names,
        );
    }
    doc(&mut out, surface.description.as_deref());
    let _ = writeln!(out, "---@class {namespace}\nlocal M = {{}}\n");
    for export in &surface.exports {
        match export.kind.as_str() {
            "function" => function(&mut out, export, &names),
            "constructor" => constructor(&mut out, namespace, export, &names),
            // `require` does not render a constant, so neither does this.
            _ => {}
        }
    }
    out.push_str("return M\n");
    out
}

fn function(out: &mut String, export: &TypedExport, names: &Names) {
    doc(out, export.description.as_deref());
    let args = params(out, &export.params, names);
    if let Some(returns) = &export.returns {
        let _ = writeln!(out, "---@return {}", ty(returns, names));
    }
    let _ = writeln!(out, "{}\n", define("M", &export.name, ".", &args));
}

/// `function M.name(args) end`, or `M["end"] = function(args) end` for a
/// name Lua cannot write after a dot. `separator` is `:` for a method.
fn define(table: &str, name: &str, separator: &str, args: &[String]) -> String {
    if is_identifier(name) {
        format!("function {table}{separator}{name}({}) end", args.join(", "))
    } else {
        let mut args = args.to_vec();
        if separator == ":" {
            args.insert(0, "self".into());
        }
        format!(
            "{table}[{}] = function({}) end",
            quote(name),
            args.join(", ")
        )
    }
}

/// The object a constructor returns, as a class whose methods are called
/// with `:`.
fn constructor(out: &mut String, namespace: &str, export: &TypedExport, names: &Names) {
    let object = format!("{namespace}.{}.Object", export.name);
    let _ = writeln!(
        out,
        "---@class {object}\nlocal {} = {{}}\n",
        local(&export.name)
    );
    for method in &export.methods {
        doc(out, method.description.as_deref());
        let args = params(out, &method.params, names);
        let returns = match (&method.returns, method.mode.as_str()) {
            (Some(returns), _) => Some(ty(returns, names)),
            (None, "lazy") => Some(object.clone()),
            (None, _) => None,
        };
        if let Some(returns) = returns {
            let _ = writeln!(out, "---@return {returns}");
        }
        let _ = writeln!(
            out,
            "{}\n",
            define(&local(&export.name), &method.name, ":", &args)
        );
    }
    doc(out, export.description.as_deref());
    let args = params(out, &export.params, names);
    let _ = writeln!(out, "---@return {object}");
    let _ = writeln!(out, "{}\n", define("M", &export.name, ".", &args));
}

fn local(name: &str) -> String {
    format!("{}_object", param_name(name))
}

fn lexicon_file(
    lexicon: &LexiconDeclaration,
    event: &ObjectType,
    lexicons: &BTreeMap<&str, &LexiconDeclaration>,
) -> String {
    let names = Names::Lexicons(lexicons);
    let nsid = &lexicon.nsid;
    let mut out = String::from("---@meta\n\n");
    if let Some(main) = &lexicon.main {
        let description = lexicon.description.as_deref();
        for (suffix, t) in [
            ("Params", &main.params),
            ("Input", &main.input),
            ("Output", &main.output),
            ("Record", &main.record),
        ] {
            if let Some(t) = t {
                declare(
                    &mut out,
                    &format!("lexicons.{nsid}.{suffix}"),
                    description,
                    t,
                    &names,
                );
            }
        }
        if main.record.is_some() {
            let record = TypeExpr::Ref {
                name: format!("{nsid}#main"),
            };
            let shape = substitute(event, &record);
            declare(
                &mut out,
                &format!("lexicons.{nsid}.Event"),
                Some("What a record-event script for this collection receives"),
                &TypeExpr::Object(shape),
                &names,
            );
        }
    }
    for (name, t) in &lexicon.defs {
        declare(
            &mut out,
            &format!("lexicons.{nsid}.{name}"),
            None,
            t,
            &names,
        );
    }
    out
}

/// The event shape with its record type filled in.
fn substitute(event: &ObjectType, record: &TypeExpr) -> ObjectType {
    fn walk(t: &TypeExpr, record: &TypeExpr) -> TypeExpr {
        match t {
            TypeExpr::Var { .. } => record.clone(),
            TypeExpr::Union { members, open } => TypeExpr::Union {
                members: members.iter().map(|m| walk(m, record)).collect(),
                open: *open,
            },
            other => other.clone(),
        }
    }
    ObjectType {
        properties: event
            .properties
            .iter()
            .map(|p| Property {
                ty: walk(&p.ty, record),
                ..p.clone()
            })
            .collect(),
    }
}

fn host_file(input: &DeclarationsInput, lexicons: &BTreeMap<&str, &LexiconDeclaration>) -> String {
    let names = Names::Lexicons(lexicons);
    let mut out = String::from("---@meta\n\n");
    let ctx = &input.ctx;
    for (name, shape, description) in [
        ("QueryCtx", &ctx.query, "ctx for an XRPC query"),
        ("ProcedureCtx", &ctx.procedure, "ctx for an XRPC procedure"),
        ("RecordCtx", &ctx.record, "ctx for a record-event script"),
        ("LabelCtx", &ctx.label, "ctx for a label script"),
        ("JobCtx", &ctx.job, "ctx for a job"),
        ("TestCtx", &ctx.test, "ctx for a test file"),
    ] {
        let class_name = format!("happyview.{name}");
        doc(&mut out, Some(description));
        let _ = writeln!(out, "---@class {class_name}");
        for property in &shape.properties {
            if name == "JobCtx" && property.name == "job" {
                let _ = writeln!(out, "---@field job happyview.Job The job this run is");
            } else {
                fields(&mut out, property, &names);
            }
        }
        out.push('\n');
    }
    out.push_str(JOB);
    declare(
        &mut out,
        "happyview.RecordEvent",
        Some("What a record-event script receives"),
        &TypeExpr::Object(input.events.record.clone()),
        &names,
    );
    declare(
        &mut out,
        "happyview.LabelEvent",
        Some("What a label script receives"),
        &TypeExpr::Object(input.events.label.clone()),
        &names,
    );
    out.push_str(VALUES);
    out
}

/// `ctx.job`, whose three functions the interpreter adds itself.
const JOB: &str = "\
--- The job this run is, and its controls
---@class happyview.Job
---@field id string The job's id
local Job = {}

--- Save progress, which the dashboard shows
---@param data table
function Job.progress(data) end

--- Whether the job has been paused or cancelled; stop when it has
---@return boolean
function Job.should_stop() end

--- Sleep, from 0 to 3600 seconds
---@param seconds number
function Job.wait(seconds) end

";

const VALUES: &str = "\
--- A blob, as a record holds one
---@class happyview.Blob
---@field [\"$type\"] \"blob\"
---@field ref happyview.CidLink
---@field mimeType string
---@field size integer

--- A CID link, as a record holds one
---@class happyview.CidLink
---@field [\"$link\"] string

--- Bytes, as a record holds them
---@class happyview.Bytes
---@field [\"$bytes\"] string The bytes, base64
";

#[cfg(test)]
mod tests {
    use super::*;
    use happyview_plugin_sdk::declarations::{
        parse_surface, CtxShapes, EventShapes, LibraryDeclaration, MainDef,
    };
    use happyview_plugin_sdk::{json, ApiExport, ApiMethod, ApiSurface};

    fn file<'a>(declarations: &'a Declarations, path: &str) -> &'a str {
        &declarations
            .files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("no {path}"))
            .contents
    }

    fn input() -> DeclarationsInput {
        let surface = ApiSurface::new("happyview.db")
            .describe("Read the index")
            .named_type(
                "Envelope",
                "An indexed record",
                json!({"type": "object", "properties": [
                    {"name": "uri", "type": "string"},
                    {"name": "cid", "type": "string?"}
                ]}),
            )
            .export(
                ApiExport::function("get")
                    .describe("One record by URI")
                    .param("uri", "string", "AT URI")
                    .returns(json!("Envelope?")),
            )
            .export(
                ApiExport::constructor("records")
                    .param("collection", "string", "NSID")
                    .method(ApiMethod::lazy("where").param("field", "string", ""))
                    .method(ApiMethod::immediate("run").returns(json!("Envelope[]"))),
            )
            .export(ApiExport::constant("VERSION"));
        DeclarationsInput {
            libraries: vec![LibraryDeclaration {
                namespace: "happyview.db".into(),
                surface: parse_surface(&surface).unwrap(),
            }],
            lexicons: vec![
                LexiconDeclaration {
                    nsid: "com.example.thing".into(),
                    kind: "record".into(),
                    description: Some("A thing".into()),
                    main: Some(MainDef {
                        record: Some(TypeExpr::Object(ObjectType {
                            properties: vec![
                                Property::new("title", TypeExpr::string()),
                                Property::new(
                                    "embed",
                                    TypeExpr::Ref {
                                        name: "com.example.thing#image".into(),
                                    },
                                )
                                .optional(),
                                Property::new(
                                    "other",
                                    TypeExpr::Ref {
                                        name: "app.bsky.feed.post#main".into(),
                                    },
                                )
                                .optional(),
                            ],
                        })),
                        ..MainDef::default()
                    }),
                    defs: BTreeMap::from([(
                        "image".into(),
                        TypeExpr::Object(ObjectType {
                            properties: vec![Property::new("blob", TypeExpr::Blob)],
                        }),
                    )]),
                },
                LexiconDeclaration {
                    nsid: "com.example.getThing".into(),
                    kind: "query".into(),
                    description: None,
                    main: Some(MainDef {
                        params: Some(TypeExpr::Object(ObjectType {
                            properties: vec![Property::new("end", TypeExpr::Integer).optional()],
                        })),
                        output: Some(TypeExpr::Ref {
                            name: "com.example.thing".into(),
                        }),
                        ..MainDef::default()
                    }),
                    defs: BTreeMap::new(),
                },
            ],
            ctx: CtxShapes {
                job: ObjectType {
                    properties: vec![
                        Property::new("trigger", TypeExpr::string()),
                        Property::new("job", TypeExpr::Object(ObjectType::default())),
                    ],
                },
                ..CtxShapes::default()
            },
            events: EventShapes {
                record: ObjectType {
                    properties: vec![Property::new(
                        "record",
                        TypeExpr::Var {
                            name: "record".into(),
                        }
                        .nullable(),
                    )],
                },
                label: ObjectType::default(),
            },
        }
    }

    #[test]
    fn a_library_is_a_module_lua_can_require() {
        let out = render(&input());
        let db = file(&out, "happyview/db.lua");
        assert_eq!(
            db,
            "\
---@meta happyview.db

--- An indexed record
---@class happyview.db.Envelope
---@field uri string
---@field cid? string|nil

--- Read the index
---@class happyview.db
local M = {}

--- One record by URI
---@param uri string AT URI
---@return happyview.db.Envelope|nil
function M.get(uri) end

---@class happyview.db.records.Object
local records_object = {}

---@param field string
---@return happyview.db.records.Object
function records_object:where(field) end

---@return happyview.db.Envelope[]
function records_object:run() end

---@param collection string NSID
---@return happyview.db.records.Object
function M.records(collection) end

return M
"
        );
    }

    #[test]
    fn a_lexicon_names_its_main_def_by_role_and_resolves_refs() {
        let out = render(&input());
        let thing = file(&out, "lexicons/com/example/thing.lua");
        assert!(
            thing.contains("---@class lexicons.com.example.thing.Record\n"),
            "{thing}"
        );
        assert!(
            thing.contains("---@field embed? lexicons.com.example.thing.image\n"),
            "{thing}"
        );
        // A lexicon the project does not have is `any`, not a dangling name.
        assert!(thing.contains("---@field other? any\n"), "{thing}");
        assert!(
            thing.contains(
                "---@class lexicons.com.example.thing.image\n---@field blob happyview.Blob\n"
            ),
            "{thing}"
        );
        assert!(
            thing.contains("---@class lexicons.com.example.thing.Event\n---@field record lexicons.com.example.thing.Record|nil\n"),
            "{thing}"
        );

        let get = file(&out, "lexicons/com/example/getThing.lua");
        assert!(get.contains("---@field [\"end\"]? integer\n"), "{get}");
        assert!(
            get.contains("---@alias lexicons.com.example.getThing.Output lexicons.com.example.thing.Record\n"),
            "{get}"
        );
    }

    #[test]
    fn ctx_gets_the_job_controls_the_interpreter_adds() {
        let out = render(&input());
        let host = file(&out, "happyview.lua");
        assert!(
            host.contains(
                "---@class happyview.JobCtx\n---@field trigger string\n---@field job happyview.Job"
            ),
            "{host}"
        );
        assert!(host.contains("function Job.should_stop() end"), "{host}");
        assert!(host.contains("---@class happyview.QueryCtx\n"), "{host}");
        assert!(
            host.contains("---@class happyview.RecordEvent\n---@field record any\n"),
            "{host}"
        );
    }

    #[test]
    fn every_path_is_relative_and_uses_slashes() {
        for f in render(&input()).files {
            assert!(
                !f.path.starts_with('/') && !f.path.contains(".."),
                "{}",
                f.path
            );
        }
    }

    #[test]
    fn a_keyword_export_is_defined_by_index() {
        assert_eq!(
            define("M", "get", ".", &["uri".into()]),
            "function M.get(uri) end"
        );
        assert_eq!(define("M", "end", ".", &[]), "M[\"end\"] = function() end");
        assert_eq!(
            define("q", "or", ":", &["x".into()]),
            "q[\"or\"] = function(self, x) end"
        );
    }

    #[test]
    fn names_that_are_not_lua_identifiers_are_rewritten() {
        assert_eq!(param_name("end"), "end_");
        assert_eq!(param_name("sort-by"), "sort_by");
        assert_eq!(param_name("2x"), "_2x");
        assert_eq!(field_name("$type"), "[\"$type\"]");
        assert_eq!(field_name("ok"), "ok");
    }

    fn shared_fixture() -> DeclarationsInput {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tests/declarations/input.json"
        );
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    /// The fixture is what `happyview types` handed the echo interpreter for
    /// a sample project, so it is the wire format as the host writes it.
    #[test]
    fn the_shared_fixture_renders() {
        let out = render(&shared_fixture());
        assert!(out.files.len() > 10, "{}", out.files.len());
    }

    /// Writes the fixture's declarations into `$DECLARATIONS_OUT`, for the
    /// job that hands them to the language's own type checker.
    #[test]
    #[ignore = "writes into $DECLARATIONS_OUT for the type-check job"]
    fn render_the_shared_fixture() {
        let out = std::path::PathBuf::from(
            std::env::var("DECLARATIONS_OUT").expect("set DECLARATIONS_OUT"),
        );
        for file in render(&shared_fixture()).files {
            let path = out.join(&file.path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, file.contents).unwrap();
        }
    }
}
