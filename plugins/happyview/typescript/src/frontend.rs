//! TypeScript to JavaScript, through SWC's TypeScript transform.
//!
//! A full transform rather than type stripping alone, so the TypeScript that
//! has run-time meaning works: `enum` (with its reverse mapping), `namespace`
//! with values, and parameter properties. The output is reprinted rather than
//! edited in place, so it carries a source map, and the engine reads every
//! error's position back through it.
//!
//! The transform runs in the same module as the script, on the same thread,
//! before the engine builds a runtime. Its allocations come from Rust's
//! allocator rather than QuickJS's, so the engine's memory ceiling does not
//! count them: what bounds them is the host's own ceiling on the module's
//! memory, as for any guest. Nor can the instruction budget reach inside it;
//! the host's wall clock is what bounds its time. Both grow with the size of
//! the script.

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use bytes_str::BytesStr;
use happyview_plugin_sdk::{ScriptErrorKind, ValidateError};
use happyview_quickjs::{Frontend, Mapping, Position, Prepared, SourceMap};
use swc_common::errors::{DiagnosticBuilder, Emitter, Handler, HANDLER};
use swc_common::source_map::{FileLoader, FilePathMapping};
use swc_common::sync::Lrc;
use swc_common::{BytePos, FileName, Globals, Span, Spanned, GLOBALS};
use swc_ecma_ast::{
    AutoAccessor, Decorator, EsVersion, JSXElement, JSXFragment, Module, ModuleDecl, ModuleItem,
    TsExportAssignment, TsImportEqualsDecl, TsModuleRef,
};
use swc_ecma_parser::{lexer::Lexer, Parser, StringInput, Syntax, TsSyntax};
use swc_ecma_transforms_typescript::{typescript, TsImportExportAssignConfig};
use swc_ecma_visit::{Visit, VisitWith};
use swc_ts_fast_strip::{operate, Mode, Options, TransformConfig};

pub struct TypeScript;

impl Frontend for TypeScript {
    fn prepare(&self, source: &str) -> Result<Prepared, Vec<ValidateError>> {
        // SWC keeps its interned names and hygiene marks in a scoped global
        // of its own, which has to be set for anything it does.
        GLOBALS.set(&Globals::new(), || {
            refuse(source)?;
            transform(source)
        })
    }
}

/// The syntax every script is read with: TypeScript, never TSX, with
/// decorators read so that one can be refused by name rather than as a stray
/// `@`.
fn syntax() -> TsSyntax {
    TsSyntax {
        tsx: false,
        decorators: true,
        ..Default::default()
    }
}

/// A source map that never reads a file. SWC's default loader reads them
/// through `std::fs`, which on WASI links the filesystem imports the host
/// refuses; nothing here names a file, so nothing here needs one.
struct NoFiles;

impl FileLoader for NoFiles {
    fn file_exists(&self, _: &Path) -> bool {
        false
    }

    fn abs_path(&self, _: &Path) -> Option<PathBuf> {
        None
    }

    fn read_file(&self, path: &Path) -> io::Result<BytesStr> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("a script reads no files, not {}", path.display()),
        ))
    }
}

fn source_map() -> Lrc<swc_common::SourceMap> {
    Lrc::new(swc_common::SourceMap::with_file_loader(
        Box::new(NoFiles),
        FilePathMapping::empty(),
    ))
}

/// The 1-based line `pos` is on in `cm`, which the host and the editor both
/// count from.
fn line(cm: &swc_common::SourceMap, pos: BytePos) -> Option<u32> {
    u32::try_from(cm.lookup_char_pos(pos).line).ok()
}

fn syntax_error(line: Option<u32>, message: impl Into<String>) -> ValidateError {
    ValidateError {
        kind: ScriptErrorKind::Syntax,
        line,
        message: message.into(),
    }
}

/// Parse the script on its own and refuse what the transform would pass
/// through to an engine that cannot run it, or would turn into CommonJS a
/// module has no way to load. Parsing here also reports a syntax error at
/// its own line, which the transform's entry point would only print.
fn refuse(source: &str) -> Result<(), Vec<ValidateError>> {
    let cm = source_map();
    let (parsed, mut errors) = parse(&cm, source, syntax());
    match parsed {
        Some(module) if errors.is_empty() => {
            let mut refusals = Refusals {
                cm: &cm,
                errors: &mut errors,
            };
            module.visit_with(&mut refusals);
        }
        _ => {
            if let Some(jsx) = jsx(source) {
                return Err(vec![jsx]);
            }
        }
    }
    if errors.is_empty() {
        return Ok(());
    }
    // Recovery can report one mistake more than once.
    errors.sort_by_key(|error| error.line);
    errors.dedup_by(|a, b| a.line == b.line && a.message == b.message);
    Err(errors)
}

/// `source` parsed as a module with `syntax`, and every error the parse
/// reported, each on its line.
fn parse(
    cm: &swc_common::SourceMap,
    source: &str,
    syntax: TsSyntax,
) -> (Option<Module>, Vec<ValidateError>) {
    let file = cm.new_source_file(Lrc::new(FileName::Anon), source.to_string());
    let lexer = Lexer::new(
        Syntax::Typescript(syntax),
        EsVersion::latest(),
        StringInput::from(&*file),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    let parsed = parser.parse_module();
    let errors = parsed
        .as_ref()
        .err()
        .into_iter()
        .cloned()
        .chain(parser.take_errors())
        .map(|error| syntax_error(line(cm, error.span().lo), error.kind().msg()))
        .collect();
    (parsed.ok(), errors)
}

const JSX: &str = "JSX is not supported: a script is TypeScript, not TSX";

/// When a script that will not parse as TypeScript parses as TSX and holds
/// JSX, that is what the author wrote, and the refusal says so at the first
/// element rather than reporting the stray `<` TypeScript sees there.
fn jsx(source: &str) -> Option<ValidateError> {
    let cm = source_map();
    let (module, errors) = parse(
        &cm,
        source,
        TsSyntax {
            tsx: true,
            ..syntax()
        },
    );
    let module = module.filter(|_| errors.is_empty())?;
    let mut first = FirstJsx(None);
    module.visit_with(&mut first);
    first.0.map(|span| syntax_error(line(&cm, span.lo), JSX))
}

/// The first JSX element or fragment in a module.
struct FirstJsx(Option<Span>);

impl Visit for FirstJsx {
    fn visit_jsx_element(&mut self, element: &JSXElement) {
        self.0.get_or_insert(element.span);
    }

    fn visit_jsx_fragment(&mut self, fragment: &JSXFragment) {
        self.0.get_or_insert(fragment.span);
    }
}

/// What this front end refuses, each with the line it is on.
struct Refusals<'a> {
    cm: &'a swc_common::SourceMap,
    errors: &'a mut Vec<ValidateError>,
}

impl Refusals<'_> {
    fn refuse(&mut self, span: Span, message: &str) {
        self.errors
            .push(syntax_error(line(self.cm, span.lo), message));
    }
}

const DECORATORS: &str = "decorators are not supported: QuickJS-ng, which runs the script, \
                          does not implement them yet";
const ACCESSORS: &str = "`accessor` fields are not supported: QuickJS-ng, which runs the \
                         script, does not implement them yet";
const IMPORT_REQUIRE: &str = "`import x = require(...)` is CommonJS, which a script cannot \
                              load: write `import x from \"...\"`";
const EXPORT_ASSIGNMENT: &str =
    "`export =` is CommonJS, which a script cannot use: write `export default`";

impl Visit for Refusals<'_> {
    fn visit_decorator(&mut self, decorator: &Decorator) {
        self.refuse(decorator.span, DECORATORS);
    }

    fn visit_auto_accessor(&mut self, accessor: &AutoAccessor) {
        self.refuse(accessor.span, ACCESSORS);
        accessor.visit_children_with(self);
    }

    fn visit_module_item(&mut self, item: &ModuleItem) {
        match item {
            // `import x = require()` declares a value; `import type x =
            // require()` is a type and is elided, so it is no reason to
            // refuse anything. `import x = A.B`, an alias, is plain TypeScript.
            ModuleItem::ModuleDecl(ModuleDecl::TsImportEquals(decl)) => {
                let TsImportEqualsDecl {
                    span,
                    is_type_only,
                    module_ref,
                    ..
                } = &**decl;
                if !is_type_only && matches!(module_ref, TsModuleRef::TsExternalModuleRef(_)) {
                    self.refuse(*span, IMPORT_REQUIRE);
                }
            }
            ModuleItem::ModuleDecl(ModuleDecl::TsExportAssignment(TsExportAssignment {
                span,
                ..
            })) => self.refuse(*span, EXPORT_ASSIGNMENT),
            _ => {}
        }
        item.visit_children_with(self);
    }
}

/// Each diagnostic SWC reported, with where it points when it points
/// anywhere.
type Reported = Rc<RefCell<Vec<(Option<BytePos>, String)>>>;

/// Collects what SWC reports through its handler, since its entry point
/// answers only "syntax error" and prints the rest.
struct Collected(Reported);

impl Emitter for Collected {
    fn emit(&mut self, diagnostic: &mut DiagnosticBuilder<'_>) {
        let at = diagnostic.span.primary_span().map(|span| span.lo);
        self.0.borrow_mut().push((at, diagnostic.message()));
    }
}

/// The transform itself, and the map back to the source.
fn transform(source: &str) -> Result<Prepared, Vec<ValidateError>> {
    let cm = source_map();
    let collected: Reported = Rc::default();
    let handler = Handler::with_emitter(true, false, Box::new(Collected(collected.clone())));
    let options = Options {
        module: Some(true),
        parser: syntax(),
        mode: Mode::Transform,
        source_map: true,
        transform: Some(TransformConfig {
            typescript: typescript::Config {
                // Class fields keep their own semantics, as TypeScript's
                // `useDefineForClassFields` does for any target that has
                // them, rather than becoming assignments in the constructor.
                native_class_properties: true,
                // Both CommonJS forms are refused before this runs; this
                // keeps the transform from ever emitting `require` should one
                // reach it.
                import_export_assign_config: TsImportExportAssignConfig::EsNext,
                ..Default::default()
            },
        }),
        ..Default::default()
    };
    let output = HANDLER.set(&handler, || {
        operate(&cm, &handler, source.to_string(), options)
    });

    let reported: Vec<ValidateError> = collected
        .borrow()
        .iter()
        .map(|(at, message)| syntax_error(at.and_then(|at| line(&cm, at)), message.clone()))
        .collect();
    if !reported.is_empty() {
        return Err(reported);
    }
    let output = output.map_err(|error| vec![syntax_error(None, error.message)])?;
    let map = match output.map {
        Some(map) => Some(decode(&map).map_err(|message| vec![syntax_error(None, message)])?),
        None => None,
    };
    Ok(Prepared {
        code: output.code,
        map,
    })
}

/// SWC's source map, in the engine's terms. Both count from 0 and QuickJS
/// counts from 1; a token SWC emitted with no source is generated code, a
/// helper or a wrapper, and has no position of its own to report.
fn decode(json: &str) -> Result<SourceMap, String> {
    let map = swc_sourcemap::SourceMap::from_slice(json.as_bytes())
        .map_err(|e| format!("the TypeScript compiler's source map did not read back: {e}"))?;
    Ok(SourceMap::new(
        map.tokens()
            .filter(|token| token.has_source())
            .map(|token| Mapping {
                generated: Position {
                    line: token.get_dst_line() + 1,
                    column: token.get_dst_col() + 1,
                },
                original: Position {
                    line: token.get_src_line() + 1,
                    column: token.get_src_col() + 1,
                },
            })
            .collect(),
    ))
}
