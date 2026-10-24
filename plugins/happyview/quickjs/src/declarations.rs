//! `get_declarations`: what a script can see, as `.d.ts` modules, for the
//! JavaScript and TypeScript front ends alike.
//!
//! - `happyview/<ns>.d.ts` per library: `declare module "happyview.db"`.
//!   Functions and immediate methods return promises, because a library call
//!   is asynchronous here; a constructor and a lazy method return the chain
//!   at once.
//! - `lexicons/<nsid as a path>.d.ts` per lexicon: `declare module
//!   "happyview:lexicons/<nsid>"` exporting `Params`, `Input`, `Output`,
//!   `Record` and `Event` for its main def, and every other def by name.
//! - `happyview/ctx.d.ts`: `declare module "happyview:ctx"` with `QueryCtx`
//!   and the other `ctx` types, the event types, and blobs, CID links and
//!   bytes.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use happyview_plugin_sdk::declarations::{
    DeclarationFile, Declarations, DeclarationsInput, LexiconDeclaration, ObjectType, Property,
    TypeExpr, TypedExport, TypedSurface,
};
use happyview_plugin_sdk::PluginError;

const CTX: &str = "happyview:ctx";

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
        path: "happyview/ctx.d.ts".into(),
        contents: ctx_file(input, &lexicons),
    }];
    for library in &input.libraries {
        files.push(DeclarationFile {
            path: format!("{}.d.ts", library.namespace.replace('.', "/")),
            contents: library_file(&library.namespace, &library.surface),
        });
    }
    for lexicon in &input.lexicons {
        files.push(DeclarationFile {
            path: format!("lexicons/{}.d.ts", lexicon.nsid.replace('.', "/")),
            contents: lexicon_file(lexicon, &lexicons),
        });
    }
    Declarations { files }
}

enum Names<'a> {
    /// A surface's own named types, declared in the same module.
    Library,
    /// Lexicon refs, from inside the module of the lexicon named.
    Lexicons {
        lexicons: &'a BTreeMap<&'a str, &'a LexiconDeclaration>,
        current: Option<&'a str>,
    },
}

impl Names<'_> {
    fn reference(&self, name: &str) -> String {
        match self {
            Names::Library => type_name(name),
            Names::Lexicons { lexicons, current } => {
                let (nsid, def) = name.split_once('#').unwrap_or((name, "main"));
                let Some(lexicon) = lexicons.get(nsid) else {
                    return "unknown".into();
                };
                let local = if def == "main" {
                    match lexicon.kind.as_str() {
                        "record" => "Record".to_string(),
                        _ if lexicon.defs.contains_key("main") => "main".into(),
                        _ => return "unknown".into(),
                    }
                } else if lexicon.defs.contains_key(def) {
                    type_name(def)
                } else {
                    return "unknown".into();
                };
                if *current == Some(nsid) {
                    local
                } else {
                    format!("import(\"happyview:lexicons/{nsid}\").{local}")
                }
            }
        }
    }
}

fn ty(t: &TypeExpr, names: &Names) -> String {
    match t {
        TypeExpr::String { known_values, .. } if !known_values.is_empty() => {
            // `(string & {})` keeps the suggestions from collapsing into
            // plain `string`, while still admitting any string.
            let mut parts: Vec<String> = known_values.iter().map(|v| quote(v)).collect();
            parts.push("(string & {})".into());
            parts.join(" | ")
        }
        TypeExpr::String { .. } => "string".into(),
        TypeExpr::Integer | TypeExpr::Number => "number".into(),
        TypeExpr::Boolean => "boolean".into(),
        TypeExpr::Bytes => format!("import(\"{CTX}\").Bytes"),
        TypeExpr::Blob => format!("import(\"{CTX}\").Blob"),
        TypeExpr::CidLink => format!("import(\"{CTX}\").CidLink"),
        TypeExpr::Null => "null".into(),
        TypeExpr::Unknown | TypeExpr::Var { .. } => "unknown".into(),
        TypeExpr::Any => "any".into(),
        TypeExpr::Literal { value } => match value {
            serde_json::Value::String(s) => quote(s),
            other => other.to_string(),
        },
        TypeExpr::Array { items } => format!("{}[]", grouped(items, names)),
        // An index signature rather than `Record<string, T>`: a record
        // lexicon's module exports a `Record` of its own, which would shadow
        // the global one there.
        TypeExpr::Map { values } => format!("{{ [key: string]: {} }}", ty(values, names)),
        TypeExpr::Object(object) if object.properties.is_empty() => {
            "{ [key: string]: unknown }".into()
        }
        TypeExpr::Object(object) => {
            let fields: Vec<String> = object
                .properties
                .iter()
                .map(|p| format!("{}{}: {}", key(&p.name), optional(p), ty(&p.ty, names)))
                .collect();
            format!("{{ {} }}", fields.join("; "))
        }
        TypeExpr::Union { members, open } => {
            let mut parts: Vec<String> = Vec::new();
            for member in members {
                let rendered = ty(member, names);
                if rendered == "unknown" || rendered == "any" {
                    return rendered;
                }
                if !parts.contains(&rendered) {
                    parts.push(rendered);
                }
            }
            if *open {
                parts.push("{ $type: string }".into());
            }
            parts.join(" | ")
        }
        TypeExpr::Ref { name } => names.reference(name),
    }
}

fn grouped(t: &TypeExpr, names: &Names) -> String {
    let rendered = ty(t, names);
    if rendered.contains(' ') && !rendered.starts_with('{') && !rendered.starts_with("import(") {
        format!("({rendered})")
    } else {
        rendered
    }
}

fn optional(p: &Property) -> &'static str {
    if p.required {
        ""
    } else {
        "?"
    }
}

fn quote(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

/// A property key: bare when it is an identifier, quoted otherwise.
fn key(name: &str) -> String {
    if is_identifier(name) {
        name.to_string()
    } else {
        quote(name)
    }
}

fn is_identifier(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// A name a declaration can bind: parameters and type names.
fn binding(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if RESERVED.contains(&out.as_str()) {
        out.push('_');
    }
    out
}

fn type_name(name: &str) -> String {
    binding(name)
}

const RESERVED: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "let",
    "static",
    "yield",
    "await",
    "implements",
    "interface",
    "package",
    "private",
    "protected",
    "public",
    "any",
    "boolean",
    "number",
    "string",
    "symbol",
    "unknown",
    "never",
    "object",
    "undefined",
];

fn doc(out: &mut String, indent: &str, description: Option<&str>) {
    if let Some(description) = description.filter(|d| !d.trim().is_empty()) {
        let text = description.replace("*/", "*\\/");
        let lines: Vec<&str> = text.lines().collect();
        if lines.len() == 1 {
            let _ = writeln!(out, "{indent}/** {} */", lines[0].trim());
        } else {
            let _ = writeln!(out, "{indent}/**");
            for line in lines {
                let _ = writeln!(out, "{indent} * {}", line.trim_end());
            }
            let _ = writeln!(out, "{indent} */");
        }
    }
}

/// An `interface` for an object, a `type` for anything else.
fn declare(out: &mut String, name: &str, description: Option<&str>, t: &TypeExpr, names: &Names) {
    doc(out, "  ", description);
    match t {
        TypeExpr::Object(object) => interface(out, name, object, names),
        other => {
            let _ = writeln!(out, "  export type {name} = {};", ty(other, names));
        }
    }
}

fn interface(out: &mut String, name: &str, object: &ObjectType, names: &Names) {
    let _ = writeln!(out, "  export interface {name} {{");
    for p in &object.properties {
        doc(out, "    ", p.description.as_deref());
        let _ = writeln!(
            out,
            "    {}{}: {};",
            key(&p.name),
            optional(p),
            ty(&p.ty, names)
        );
    }
    let _ = writeln!(out, "  }}");
}

/// A parameter list. TypeScript refuses a required parameter after an
/// optional one, so an optional one that precedes a required one is written
/// as accepting `undefined` instead.
fn params(params: &[Property], names: &Names) -> String {
    let last_required = params.iter().rposition(|p| p.required);
    params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let t = ty(&p.ty, names);
            match (p.required, last_required) {
                (false, Some(last)) if i < last => format!("{}: {t} | undefined", binding(&p.name)),
                (false, _) => format!("{}?: {t}", binding(&p.name)),
                (true, _) => format!("{}: {t}", binding(&p.name)),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn param_docs(
    out: &mut String,
    indent: &str,
    export_description: Option<&str>,
    params: &[Property],
) {
    let described: Vec<&Property> = params.iter().filter(|p| p.description.is_some()).collect();
    if export_description.is_none() && described.is_empty() {
        return;
    }
    let _ = writeln!(out, "{indent}/**");
    if let Some(description) = export_description {
        for line in description.replace("*/", "*\\/").lines() {
            let _ = writeln!(out, "{indent} * {}", line.trim_end());
        }
    }
    for p in described {
        let _ = writeln!(
            out,
            "{indent} * @param {} {}",
            binding(&p.name),
            p.description
                .as_deref()
                .unwrap_or_default()
                .replace("*/", "*\\/")
        );
    }
    let _ = writeln!(out, "{indent} */");
}

fn library_file(namespace: &str, surface: &TypedSurface) -> String {
    let names = Names::Library;
    let mut out = String::new();
    doc(&mut out, "", surface.description.as_deref());
    let _ = writeln!(out, "declare module {} {{", quote(namespace));
    for named in &surface.types {
        declare(
            &mut out,
            &type_name(&named.name),
            named.description.as_deref(),
            &named.ty,
            &names,
        );
    }
    for export in &surface.exports {
        match export.kind.as_str() {
            "function" => function(&mut out, export, &names),
            "constructor" => constructor(&mut out, export, &names),
            _ => {}
        }
    }
    out.push_str("}\n");
    out
}

fn promise(returns: Option<&TypeExpr>, names: &Names) -> String {
    format!(
        "Promise<{}>",
        returns
            .map(|r| ty(r, names))
            .unwrap_or_else(|| "unknown".into())
    )
}

fn function(out: &mut String, export: &TypedExport, names: &Names) {
    param_docs(out, "  ", export.description.as_deref(), &export.params);
    let signature = format!(
        "({}): {}",
        params(&export.params, names),
        promise(export.returns.as_ref(), names)
    );
    export_function(out, &export.name, &signature);
}

/// `export function name…`, or, for a name a declaration cannot bind such
/// as `delete`, a function under a bindable name exported as the real one.
fn export_function(out: &mut String, name: &str, signature: &str) {
    let local = binding(name);
    if local == name {
        let _ = writeln!(out, "  export function {name}{signature};");
    } else {
        let _ = writeln!(out, "  function {local}{signature};");
        let _ = writeln!(out, "  export {{ {local} as {} }};", export_name(name));
    }
}

/// The name an `export { … as name }` gives: any identifier, reserved words
/// included, or a string for anything else.
fn export_name(name: &str) -> String {
    if is_identifier(name) {
        name.to_string()
    } else {
        quote(name)
    }
}

fn constructor(out: &mut String, export: &TypedExport, names: &Names) {
    let chain = format!("{}Chain", binding(&export.name));
    let _ = writeln!(out, "  export interface {chain} {{");
    for method in &export.methods {
        param_docs(out, "    ", method.description.as_deref(), &method.params);
        let returns = match (&method.returns, method.mode.as_str()) {
            (Some(r), "lazy") => ty(r, names),
            (None, "lazy") => chain.clone(),
            (r, _) => promise(r.as_ref(), names),
        };
        let _ = writeln!(
            out,
            "    {}({}): {returns};",
            key(&method.name),
            params(&method.params, names)
        );
    }
    let _ = writeln!(out, "  }}");
    param_docs(out, "  ", export.description.as_deref(), &export.params);
    let signature = format!("({}): {chain}", params(&export.params, names));
    export_function(out, &export.name, &signature);
}

fn lexicon_file(
    lexicon: &LexiconDeclaration,
    lexicons: &BTreeMap<&str, &LexiconDeclaration>,
) -> String {
    let names = Names::Lexicons {
        lexicons,
        current: Some(&lexicon.nsid),
    };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "declare module \"happyview:lexicons/{}\" {{",
        lexicon.nsid
    );
    if let Some(main) = &lexicon.main {
        let description = lexicon.description.as_deref();
        for (name, t) in [
            ("Params", &main.params),
            ("Input", &main.input),
            ("Output", &main.output),
            ("Record", &main.record),
        ] {
            if let Some(t) = t {
                declare(&mut out, name, description, t, &names);
            }
        }
        if main.record.is_some() {
            doc(
                &mut out,
                "  ",
                Some("What a record-event script for this collection receives"),
            );
            let _ = writeln!(
                out,
                "  export type Event = import(\"{CTX}\").RecordEvent<Record>;"
            );
        }
    }
    for (name, t) in &lexicon.defs {
        declare(&mut out, &type_name(name), None, t, &names);
    }
    out.push_str("}\n");
    out
}

fn ctx_file(input: &DeclarationsInput, lexicons: &BTreeMap<&str, &LexiconDeclaration>) -> String {
    let names = Names::Lexicons {
        lexicons,
        current: None,
    };
    let mut out = format!("declare module \"{CTX}\" {{\n");
    let ctx = &input.ctx;
    for (name, shape, description) in [
        ("QueryCtx", &ctx.query, "ctx for an XRPC query"),
        ("ProcedureCtx", &ctx.procedure, "ctx for an XRPC procedure"),
        ("RecordCtx", &ctx.record, "ctx for a record-event script"),
        ("LabelCtx", &ctx.label, "ctx for a label script"),
        ("JobCtx", &ctx.job, "ctx for a job"),
        ("TestCtx", &ctx.test, "ctx for a test file"),
    ] {
        doc(&mut out, "  ", Some(description));
        let _ = writeln!(out, "  export interface {name} {{");
        for p in &shape.properties {
            doc(&mut out, "    ", p.description.as_deref());
            let t = if name == "JobCtx" && p.name == "job" {
                "Job".to_string()
            } else {
                ty(&p.ty, &names)
            };
            let _ = writeln!(out, "    {}{}: {t};", key(&p.name), optional(p));
        }
        let _ = writeln!(out, "  }}");
    }
    out.push_str(JOB);

    // The record event is generic over the record a lexicon declares.
    doc(&mut out, "  ", Some("What a record-event script receives"));
    let _ = writeln!(out, "  export interface RecordEvent<R = unknown> {{");
    for p in &input.events.record.properties {
        doc(&mut out, "    ", p.description.as_deref());
        let t = generic(&p.ty, &names);
        let _ = writeln!(out, "    {}{}: {t};", key(&p.name), optional(p));
    }
    let _ = writeln!(out, "  }}");
    declare(
        &mut out,
        "LabelEvent",
        Some("What a label script receives"),
        &TypeExpr::Object(input.events.label.clone()),
        &names,
    );
    out.push_str(VALUES);
    out.push_str("}\n");
    out
}

/// A type with its record variable spelled `R`.
fn generic(t: &TypeExpr, names: &Names) -> String {
    match t {
        TypeExpr::Var { .. } => "R".into(),
        TypeExpr::Union { members, .. }
            if members.iter().any(|m| matches!(m, TypeExpr::Var { .. })) =>
        {
            members
                .iter()
                .map(|m| generic(m, names))
                .collect::<Vec<_>>()
                .join(" | ")
        }
        other => ty(other, names),
    }
}

const JOB: &str = "  /** The job this run is, and its controls */
  export interface Job {
    /** The job's id */
    id: string;
    /** Save progress, which the dashboard shows */
    progress(data: unknown): void;
    /** Whether the job has been paused or cancelled; stop when it has */
    should_stop(): boolean;
    /** Sleep, from 0 to 3600 seconds */
    wait(seconds: number): Promise<void>;
  }
";

const VALUES: &str = "  /** A blob, as a record holds one */
  export interface Blob {
    $type: \"blob\";
    ref: CidLink;
    mimeType: string;
    size: number;
  }
  /** A CID link, as a record holds one */
  export interface CidLink {
    $link: string;
  }
  /** Bytes, as a record holds them: base64 */
  export interface Bytes {
    $bytes: string;
  }
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
                    .param("opts", "object?", "")
                    .param("default", "string", ""),
            )
            .export(
                ApiExport::constructor("records")
                    .param("collection", "string", "NSID")
                    .method(ApiMethod::lazy("where").param("field", "string", ""))
                    .method(ApiMethod::immediate("run").returns(json!("Envelope[]"))),
            );
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
                                    "sort",
                                    TypeExpr::String {
                                        format: None,
                                        known_values: vec!["new".into()],
                                    },
                                )
                                .optional(),
                                Property::new("cover", TypeExpr::Blob).optional(),
                            ],
                        })),
                        ..MainDef::default()
                    }),
                    defs: BTreeMap::new(),
                },
                LexiconDeclaration {
                    nsid: "com.example.getThing".into(),
                    kind: "query".into(),
                    description: None,
                    main: Some(MainDef {
                        output: Some(TypeExpr::Ref {
                            name: "com.example.thing#main".into(),
                        }),
                        params: Some(TypeExpr::Object(ObjectType {
                            properties: vec![Property::new(
                                "missing",
                                TypeExpr::Ref {
                                    name: "com.example.nope#x".into(),
                                },
                            )],
                        })),
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
    fn a_library_is_an_ambient_module() {
        let out = render(&input());
        assert_eq!(
            file(&out, "happyview/db.d.ts"),
            "\
/** Read the index */
declare module \"happyview.db\" {
  /** An indexed record */
  export interface Envelope {
    uri: string;
    cid?: string | null;
  }
  /**
   * One record by URI
   * @param uri AT URI
   */
  export function get(uri: string, opts: { [key: string]: unknown } | null | undefined, default_: string): Promise<unknown>;
  export interface recordsChain {
    where(field: string): recordsChain;
    run(): Promise<Envelope[]>;
  }
  /**
   * @param collection NSID
   */
  export function records(collection: string): recordsChain;
}
"
        );
    }

    #[test]
    fn lexicons_export_their_roles_and_import_each_other() {
        let out = render(&input());
        let thing = file(&out, "lexicons/com/example/thing.d.ts");
        assert!(
            thing.starts_with("declare module \"happyview:lexicons/com.example.thing\" {\n"),
            "{thing}"
        );
        assert!(
            thing.contains("    sort?: \"new\" | (string & {});\n"),
            "{thing}"
        );
        assert!(
            thing.contains("    cover?: import(\"happyview:ctx\").Blob;\n"),
            "{thing}"
        );
        assert!(
            thing
                .contains("  export type Event = import(\"happyview:ctx\").RecordEvent<Record>;\n"),
            "{thing}"
        );

        let get = file(&out, "lexicons/com/example/getThing.d.ts");
        assert!(
            get.contains(
                "  export type Output = import(\"happyview:lexicons/com.example.thing\").Record;\n"
            ),
            "{get}"
        );
        assert!(get.contains("    missing: unknown;\n"), "{get}");
    }

    #[test]
    fn ctx_declares_the_job_controls_and_a_generic_record_event() {
        let out = render(&input());
        let ctx = file(&out, "happyview/ctx.d.ts");
        assert!(
            ctx.contains("  export interface JobCtx {\n    trigger: string;\n    job: Job;\n  }\n"),
            "{ctx}"
        );
        assert!(
            ctx.contains("    wait(seconds: number): Promise<void>;\n"),
            "{ctx}"
        );
        assert!(
            ctx.contains(
                "  export interface RecordEvent<R = unknown> {\n    record: R | null;\n  }\n"
            ),
            "{ctx}"
        );
    }

    #[test]
    fn a_reserved_export_name_is_exported_as_itself() {
        let mut out = String::new();
        export_function(&mut out, "delete", "(): Promise<unknown>");
        assert_eq!(
            out,
            "  function delete_(): Promise<unknown>;\n  export { delete_ as delete };\n"
        );
    }

    #[test]
    fn names_are_made_bindable() {
        assert_eq!(binding("default"), "default_");
        assert_eq!(binding("sort-by"), "sort_by");
        assert_eq!(key("$type"), "$type");
        assert_eq!(key("a-b"), "\"a-b\"");
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
