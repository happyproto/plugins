//! The relative modules a chunk `require`s by a literal name, so a missing
//! one is reported when the file is saved rather than at its first run.
//!
//! Tokens are enough to find `require` followed by a string, so this is a
//! lexer for Lua 5.4's lexical grammar and nothing more. It runs only on a
//! chunk Lua has already compiled.

use happyview_plugin_sdk::ImportRef;

#[derive(Debug)]
enum Token {
    Name(String),
    /// A string literal's text, `None` when it has an escape: decoding one
    /// is not worth a second implementation of Lua's escapes, and a skipped
    /// literal is still resolved when it runs.
    Str(Option<String>, u32),
    Punct(u8),
}

pub fn relative_requires(source: &str) -> Vec<ImportRef> {
    let tokens = tokens(source);
    let mut found = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let Token::Name(name) = token else { continue };
        if name != "require" {
            continue;
        }
        // `t.require(...)` and `t:require(...)` are someone's method.
        // A `..` before it is concatenation, and `require` is still the global.
        let previous = |back: usize| i.checked_sub(back).map(|p| &tokens[p]);
        if matches!(previous(1), Some(Token::Punct(b'.' | b':')))
            && !matches!(previous(2), Some(Token::Punct(b'.')))
        {
            continue;
        }
        let literal = match (tokens.get(i + 1), tokens.get(i + 2), tokens.get(i + 3)) {
            (Some(Token::Str(text, line)), ..) => Some((text, *line)),
            (Some(Token::Punct(b'(')), Some(Token::Str(text, line)), Some(Token::Punct(b')'))) => {
                Some((text, *line))
            }
            _ => None,
        };
        if let Some((Some(text), line)) = literal {
            if crate::modules::is_relative(text) {
                found.push(ImportRef {
                    specifier: text.clone(),
                    line,
                });
            }
        }
    }
    found
}

/// `[` followed by `level` `=` and another `[`, as a long bracket opens.
fn long_open(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'[') {
        return None;
    }
    let level = bytes[at + 1..].iter().take_while(|&&b| b == b'=').count();
    (bytes.get(at + 1 + level) == Some(&b'[')).then_some(level)
}

/// The end of a long bracket of `level` opened at `at`, counting the lines
/// it spans; the source's end when it never closes.
fn long_close(bytes: &[u8], at: usize, level: usize, line: &mut u32) -> (usize, usize) {
    let body = at + level + 2;
    let mut i = body;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            *line += 1;
        } else if bytes[i] == b']'
            && bytes[i + 1..].iter().take(level).all(|&b| b == b'=')
            && bytes.get(i + 1 + level) == Some(&b']')
        {
            return (body, i + level + 2);
        }
        i += 1;
    }
    (body, bytes.len())
}

fn tokens(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut line = 1u32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b' ' | b'\t' | b'\r' | 0x0b | 0x0c => i += 1,
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                i += 2;
                if let Some(level) = long_open(bytes, i) {
                    i = long_close(bytes, i, level, &mut line).1;
                } else {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                }
            }
            b'[' if long_open(bytes, i).is_some() => {
                let level = long_open(bytes, i).expect("checked by the guard");
                let start = line;
                let (body, end) = long_close(bytes, i, level, &mut line);
                let text = &source[body..end.saturating_sub(level + 2).max(body)];
                // Lua drops a newline straight after the opening bracket.
                let text = text
                    .strip_prefix("\r\n")
                    .or_else(|| text.strip_prefix('\n'))
                    .unwrap_or(text);
                out.push(Token::Str(Some(text.to_string()), start));
                i = end;
            }
            quote @ (b'"' | b'\'') => {
                let start = line;
                let mut escaped = false;
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] != quote {
                    match bytes[j] {
                        b'\\' => {
                            escaped = true;
                            if bytes.get(j + 1) == Some(&b'\n') {
                                line += 1;
                            }
                            j += 2;
                        }
                        b'\n' => {
                            line += 1;
                            j += 1;
                        }
                        _ => j += 1,
                    }
                }
                let end = j.min(bytes.len());
                let text = (!escaped).then(|| source[i + 1..end].to_string());
                out.push(Token::Str(text, start));
                i = end + 1;
            }
            b if b.is_ascii_alphabetic() || b == b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push(Token::Name(source[start..i].to_string()));
            }
            b if b.is_ascii_digit() => {
                // A number never holds a quote or a bracket, so all that
                // matters is not ending it early at an exponent's sign.
                while i < bytes.len() {
                    let b = bytes[i];
                    let signed = matches!(b, b'+' | b'-')
                        && matches!(bytes[i - 1], b'e' | b'E' | b'p' | b'P');
                    if b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || signed {
                        i += 1;
                    } else {
                        break;
                    }
                }
            }
            other => {
                out.push(Token::Punct(other));
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(source: &str) -> Vec<(String, u32)> {
        relative_requires(source)
            .into_iter()
            .map(|import| (import.specifier, import.line))
            .collect()
    }

    #[test]
    fn every_call_form_with_a_literal_is_found_on_its_line() {
        let source = r#"local a = require("./a")
local b = require './b'
local c = require "../up/c"
local d = require [[./d]]
local e = require(
  "./e"
)
local f = require [==[./f]==]
local g = "x" .. require('./g')"#;
        assert_eq!(
            found(source),
            vec![
                ("./a".into(), 1),
                ("./b".into(), 2),
                ("../up/c".into(), 3),
                ("./d".into(), 4),
                ("./e".into(), 6),
                ("./f".into(), 8),
                ("./g".into(), 9),
            ]
        );
    }

    #[test]
    fn what_is_not_a_literal_relative_require_of_the_global_is_left_out() {
        let source = r#"-- require("./comment")
--[[ require("./block")
]]
--[==[ ]] require("./still-comment") ]==]
local s = "require('./string')"
local l = [[ require("./long-string") ]]
local t = x.require("./field")
local u = x:require("./method")
local v = require(name)
local w = require("./a" .. suffix)
local y = require("internal.json")
local z = require("./esc\097ped")
local ok = require("./kept")"#;
        assert_eq!(found(source), vec![("./kept".into(), 13)]);
    }

    #[test]
    fn lines_count_through_strings_and_comments_that_span_them() {
        let source =
            "local s = \"a\\\nb\"\n--[[\n\n]]\nlocal x = [[\n\n]]\nlocal m = require('./m')";
        assert_eq!(found(source), vec![("./m".into(), 9)]);
    }

    #[test]
    fn a_number_or_an_index_is_not_mistaken_for_anything() {
        let source = "local n = 0x1p4 + 3e-2 + t[ [[k]] ]\nreturn require('./n')";
        assert_eq!(found(source), vec![("./n".into(), 2)]);
    }
}
