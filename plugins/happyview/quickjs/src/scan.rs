//! The modules a script imports by a literal relative path — static
//! `import`, `export … from` and `import("…")` — so a missing one is
//! reported when the file is saved rather than when it first runs.
//!
//! A scanner rather than a parser: QuickJS keeps its syntax tree to itself,
//! and these three forms need tokens, not a tree. The one place tokens are
//! ambiguous in JavaScript is `/`, which starts a regular expression or
//! divides depending on what came before; it is read the way module lexers
//! read it, from the previous token. It runs only on code QuickJS compiled.

use crate::positions::Position;

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    /// A string literal's text, `None` when it has an escape: decoding one is
    /// not worth a second implementation of JavaScript's escapes, and a
    /// skipped literal is still resolved when it runs.
    Str(Option<String>, Position),
    Punct(u8),
    /// The end of a value that is not a name or a string: a number, a
    /// regular expression or a template. A `/` after one divides.
    Value,
}

/// Keywords after which an expression, and so a regular expression, starts.
const BEFORE_EXPRESSION: [&str; 14] = [
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "case",
    "do",
    "else",
    "yield",
    "await",
];

/// Words that start a statement, so an `import`/`export` clause has ended
/// without a `from` by the time one appears.
const STATEMENT_START: [&str; 9] = [
    "import", "export", "const", "let", "var", "function", "class", "return", "if",
];

pub fn relative_imports(code: &str) -> Vec<(String, Position)> {
    let toks = tokens(code);
    let mut found = Vec::new();
    for (i, tok) in toks.iter().enumerate() {
        let Tok::Ident(word) = tok else { continue };
        // `x.import(…)` is someone's method.
        if matches!(i.checked_sub(1).map(|p| &toks[p]), Some(Tok::Punct(b'.'))) {
            continue;
        }
        let literal = match word.as_str() {
            "import" => match (toks.get(i + 1), toks.get(i + 2), toks.get(i + 3)) {
                (Some(Tok::Punct(b'.')), ..) => None,
                (Some(Tok::Str(text, at)), ..) => Some((text, *at)),
                (
                    Some(Tok::Punct(b'(')),
                    Some(Tok::Str(text, at)),
                    Some(Tok::Punct(b')' | b',')),
                ) => Some((text, *at)),
                (Some(Tok::Punct(b'(')), ..) => None,
                _ => from_clause(&toks, i + 1),
            },
            "export" => match toks.get(i + 1) {
                Some(Tok::Punct(b'*' | b'{')) => from_clause(&toks, i + 1),
                _ => None,
            },
            _ => None,
        };
        if let Some((Some(text), at)) = literal {
            if text.starts_with("./") || text.starts_with("../") {
                found.push((text.clone(), at));
            }
        }
    }
    found
}

/// The string after a clause's `from`, the clause ending at a `;`, a value,
/// or a word that starts a statement.
fn from_clause(toks: &[Tok], start: usize) -> Option<(&Option<String>, Position)> {
    for (j, tok) in toks.iter().enumerate().skip(start) {
        match tok {
            Tok::Ident(word) if word == "from" => {
                if let Some(Tok::Str(text, at)) = toks.get(j + 1) {
                    return Some((text, *at));
                }
            }
            Tok::Ident(word) if STATEMENT_START.contains(&word.as_str()) => return None,
            Tok::Punct(b';') | Tok::Str(..) | Tok::Value => return None,
            _ => {}
        }
    }
    None
}

fn regex_may_start(previous: Option<&Tok>) -> bool {
    match previous {
        None => true,
        Some(Tok::Value | Tok::Str(..)) => false,
        Some(Tok::Punct(b')' | b']')) => false,
        // `}` included: a block's end, where a statement may begin.
        Some(Tok::Punct(_)) => true,
        Some(Tok::Ident(word)) => BEFORE_EXPRESSION.contains(&word.as_str()),
    }
}

/// A cursor that counts lines and columns as it moves.
struct Cursor<'a> {
    bytes: &'a [u8],
    i: usize,
    line: u32,
    line_start: usize,
}

impl Cursor<'_> {
    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.i + ahead).copied()
    }

    fn bump(&mut self) {
        let Some(&b) = self.bytes.get(self.i) else {
            return;
        };
        if b == b'\n' {
            self.line += 1;
            self.line_start = self.i + 1;
        }
        self.i += 1;
    }

    fn position(&self) -> Position {
        Position {
            line: self.line,
            column: (self.i - self.line_start + 1) as u32,
        }
    }
}

fn tokens(code: &str) -> Vec<Tok> {
    let mut c = Cursor {
        bytes: code.as_bytes(),
        i: 0,
        line: 1,
        line_start: 0,
    };
    let mut out: Vec<Tok> = Vec::new();
    // Brace depth, and the depth each open template substitution began at: a
    // `}` at one of those resumes its template rather than closing a block.
    let mut depth = 0u32;
    let mut substitutions: Vec<u32> = Vec::new();
    while let Some(b) = c.peek(0) {
        match b {
            b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c => c.bump(),
            b'/' if c.peek(1) == Some(b'/') => {
                while c.peek(0).is_some_and(|b| b != b'\n') {
                    c.bump();
                }
            }
            b'/' if c.peek(1) == Some(b'*') => {
                c.bump();
                c.bump();
                while c.peek(0).is_some() && !(c.peek(0) == Some(b'*') && c.peek(1) == Some(b'/')) {
                    c.bump();
                }
                c.bump();
                c.bump();
            }
            b'/' if regex_may_start(out.last()) => {
                skip_regex(&mut c);
                out.push(Tok::Value);
            }
            b'"' | b'\'' => {
                let at = c.position();
                let text = string(code, &mut c);
                out.push(Tok::Str(text, at));
            }
            b'`' => {
                c.bump();
                out.push(template(&mut c, &mut depth, &mut substitutions));
            }
            b'{' => {
                depth += 1;
                out.push(Tok::Punct(b));
                c.bump();
            }
            b'}' if substitutions.last() == Some(&depth) => {
                substitutions.pop();
                depth -= 1;
                c.bump();
                out.push(template(&mut c, &mut depth, &mut substitutions));
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                out.push(Tok::Punct(b));
                c.bump();
            }
            b if b.is_ascii_alphabetic() || b == b'_' || b == b'$' || b >= 0x80 => {
                let start = c.i;
                while c.peek(0).is_some_and(|b| {
                    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
                }) {
                    c.bump();
                }
                out.push(Tok::Ident(code[start..c.i].to_string()));
            }
            b if b.is_ascii_digit() => {
                while c
                    .peek(0)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
                {
                    c.bump();
                }
                out.push(Tok::Value);
            }
            _ => {
                out.push(Tok::Punct(b));
                c.bump();
            }
        }
    }
    out
}

/// A quoted string from its opening quote, leaving the cursor past its end.
fn string(code: &str, c: &mut Cursor<'_>) -> Option<String> {
    let quote = c.peek(0).expect("called on a quote");
    c.bump();
    let start = c.i;
    let mut escaped = false;
    while let Some(b) = c.peek(0) {
        match b {
            _ if b == quote => break,
            b'\\' => {
                escaped = true;
                c.bump();
                if c.peek(0).is_some() {
                    c.bump();
                }
            }
            b'\n' => break,
            _ => c.bump(),
        }
    }
    let text = (!escaped).then(|| code[start..c.i].to_string());
    if c.peek(0) == Some(quote) {
        c.bump();
    }
    text
}

/// A template from just inside its backtick or a substitution's `}`, up to
/// its end or its next `${`. Ending at `${` reads as an open parenthesis,
/// since an expression starts there.
fn template(c: &mut Cursor<'_>, depth: &mut u32, substitutions: &mut Vec<u32>) -> Tok {
    while let Some(b) = c.peek(0) {
        match b {
            b'\\' => {
                c.bump();
                if c.peek(0).is_some() {
                    c.bump();
                }
            }
            b'`' => {
                c.bump();
                return Tok::Value;
            }
            b'$' if c.peek(1) == Some(b'{') => {
                c.bump();
                c.bump();
                *depth += 1;
                substitutions.push(*depth);
                return Tok::Punct(b'(');
            }
            _ => c.bump(),
        }
    }
    Tok::Value
}

/// A regular expression from its opening `/`, flags included. A `/` inside
/// a character class does not end one.
fn skip_regex(c: &mut Cursor<'_>) {
    c.bump();
    let mut class = false;
    while let Some(b) = c.peek(0) {
        match b {
            b'\\' => {
                c.bump();
                if c.peek(0).is_some() {
                    c.bump();
                }
            }
            b'[' => {
                class = true;
                c.bump();
            }
            b']' => {
                class = false;
                c.bump();
            }
            b'/' if !class => {
                c.bump();
                while c.peek(0).is_some_and(|b| b.is_ascii_alphanumeric()) {
                    c.bump();
                }
                return;
            }
            // A regular expression never spans a line; this `/` divided.
            b'\n' => return,
            _ => c.bump(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(code: &str) -> Vec<(String, u32)> {
        relative_imports(code)
            .into_iter()
            .map(|(specifier, at)| (specifier, at.line))
            .collect()
    }

    #[test]
    fn every_import_form_with_a_literal_is_found_on_its_line() {
        let code = r#"import a from "./a.js";
import { b } from './b.js';
import * as c from "./c.js";
import "./d.js";
export { e } from "./e.js";
export * from "./f.js";
export * as g from "./g.js";
const h = await import("./h.js");
import {
  i,
} from "../i.js";
"#;
        assert_eq!(
            found(code),
            vec![
                ("./a.js".into(), 1),
                ("./b.js".into(), 2),
                ("./c.js".into(), 3),
                ("./d.js".into(), 4),
                ("./e.js".into(), 5),
                ("./f.js".into(), 6),
                ("./g.js".into(), 7),
                ("./h.js".into(), 8),
                ("../i.js".into(), 11),
            ]
        );
    }

    #[test]
    fn what_is_not_a_literal_relative_import_is_left_out() {
        let code = r#"// import x from "./comment.js"
/* import "./block.js" */
const s = "import('./string.js')";
const t = `import("./template.js")`;
x.import("./method.js");
const m = import.meta.url;
const d = import(name);
const e = import("./a.js" + x);
import db from "happyview.db";
const r = /import("\.\/regex.js")/;
const esc = import("./esc\u0061.js");
export const n = 1;
export default "./not-from.js";
import kept from "./kept.js";
"#;
        assert!(!code.contains("./esca.js"));
        assert!(code.contains("\\u0061"));
        assert_eq!(found(code), vec![("./kept.js".into(), 14)]);
    }

    #[test]
    fn an_import_inside_a_template_substitution_is_found() {
        let code = "const t = `a ${ { x: `b ${ await import(\"./deep.js\") }` }.x } c`;\nimport \"./after.js\";\n";
        assert_eq!(
            found(code),
            vec![("./deep.js".into(), 1), ("./after.js".into(), 2)]
        );
    }

    #[test]
    fn a_slash_is_read_as_division_or_a_regex_from_what_came_before() {
        let code = "const half = total / 2; import \"./after-division.js\";\n\
                    const q = /\"/; import \"./after-regex.js\";\n\
                    function f() { return /'/; }\nimport \"./after-return.js\";\n";
        assert_eq!(
            found(code),
            vec![
                ("./after-division.js".into(), 1),
                ("./after-regex.js".into(), 2),
                ("./after-return.js".into(), 4),
            ]
        );
    }

    #[test]
    fn the_position_is_the_strings_own() {
        let code = "import x\n  from './x.js';";
        assert_eq!(
            relative_imports(code),
            vec![("./x.js".to_string(), Position { line: 2, column: 8 })]
        );
    }
}
