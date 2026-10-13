//! Where an error happened, in the author's own source and file.
//!
//! QuickJS places an error in the module it compiled, under the name it was
//! compiled as: the script's chunk name, or a local module's id. A module's
//! code is its front end's output rather than what the author wrote whenever
//! the front end rewrote anything, so each project module is recorded here
//! with its [`SourceMap`], and every position the engine reports — an
//! error's `line`, each frame in its `raw` stack — passes through the map of
//! the module it is in. A frame in any other module, a library's or a
//! built-in's, is not the author's and is never reported as a position.

use std::collections::BTreeMap;

/// A position in a module, as QuickJS writes one: both counts start at 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

/// One point a front end knows the origin of: where something it emitted
/// came from in the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub generated: Position,
    pub original: Position,
}

/// What a front end knows about where its output came from, kept sorted by
/// generated position so a lookup is a binary search.
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    mappings: Vec<Mapping>,
}

impl SourceMap {
    pub fn new(mut mappings: Vec<Mapping>) -> Self {
        mappings.sort_by_key(|mapping| mapping.generated);
        Self { mappings }
    }

    /// Where `at` came from: the nearest mapping at or before it on the same
    /// generated line, or, when the line's first mapping lies after it, that
    /// one — a front end emits a line for one statement, so any mapping on
    /// the line names the statement. A line with no mapping at all has no
    /// origin to report, and `None` says so.
    fn original(&self, at: Position) -> Option<Position> {
        let start = self
            .mappings
            .partition_point(|mapping| mapping.generated.line < at.line);
        let end = self
            .mappings
            .partition_point(|mapping| mapping.generated.line <= at.line);
        let line = &self.mappings[start..end];
        let before = line.partition_point(|mapping| mapping.generated <= at);
        line.get(before.saturating_sub(1))
            .map(|mapping| mapping.original)
    }
}

/// The one place a generated position becomes the author's. A position no
/// mapping covers keeps the generated one rather than inventing one.
pub fn remap(map: Option<&SourceMap>, at: Position) -> Position {
    map.and_then(|map| map.original(at)).unwrap_or(at)
}

/// Every module a run compiled from project code, by the name QuickJS knows
/// it as, with the map back to its author's source when it has one.
#[derive(Debug, Default)]
pub struct Chunks {
    maps: BTreeMap<String, Option<SourceMap>>,
}

impl Chunks {
    pub fn insert(&mut self, name: &str, map: Option<SourceMap>) {
        self.maps.insert(name.to_string(), map);
    }

    pub fn contains(&self, name: &str) -> bool {
        self.maps.contains_key(name)
    }

    /// A `name:L:C` frame location at the start of `text` in a project
    /// module, its position, and how many bytes it takes. The location
    /// starts exactly at the name, so `lib/a.js:3:1` is never read as
    /// `a.js`.
    fn frame_at(&self, text: &str) -> Option<(&str, Position, usize)> {
        self.maps.keys().find_map(|name| {
            let rest = text.strip_prefix(name.as_str())?.strip_prefix(':')?;
            let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
            let line_len = digits(rest);
            let after_line = rest[line_len..].strip_prefix(':')?;
            let column_len = digits(after_line);
            if line_len == 0 || column_len == 0 {
                return None;
            }
            let line = rest[..line_len].parse().ok()?;
            let column = after_line[..column_len].parse().ok()?;
            let len = name.len() + 1 + line_len + 1 + column_len;
            Some((name.as_str(), Position { line, column }, len))
        })
    }

    /// `stack` with every frame in a project module moved to its author's
    /// position. A frame is recognised only where QuickJS writes one, after
    /// `(` or `at `, so a library's module whose name merely ends in a
    /// project module's is left alone.
    pub fn remap_frames(&self, stack: &str) -> String {
        let mut out = String::with_capacity(stack.len());
        let mut i = 0;
        while i < stack.len() {
            let framed = stack[..i].ends_with('(') || stack[..i].ends_with("at ");
            if framed {
                if let Some((name, at, len)) = self.frame_at(&stack[i..]) {
                    let position = remap(self.maps.get(name).and_then(Option::as_ref), at);
                    out.push_str(&format!("{name}:{}:{}", position.line, position.column));
                    i += len;
                    continue;
                }
            }
            let ch = stack[i..].chars().next().expect("i is on a char boundary");
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    /// The file and line of the innermost frame in a project module, from a
    /// stack [`remap_frames`](Self::remap_frames) has already rewritten. A
    /// frame in a library's module or a native function is skipped, so an
    /// error thrown by the bridge is placed at the line that called it.
    pub fn line_of(&self, stack: &str) -> Option<(String, u32)> {
        stack.lines().find_map(|frame| {
            let frame = frame.trim();
            let location = match frame.rfind('(') {
                Some(at) => &frame[at + 1..],
                None => frame.strip_prefix("at ")?,
            };
            self.frame_at(location)
                .map(|(name, position, _)| (name.to_string(), position.line))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: u32, column: u32) -> Position {
        Position { line, column }
    }

    /// `(generated, original)`, each as `(line, column)`.
    type Pair = ((u32, u32), (u32, u32));

    fn map(pairs: &[Pair]) -> SourceMap {
        SourceMap::new(
            pairs
                .iter()
                .map(|&((gl, gc), (ol, oc))| Mapping {
                    generated: at(gl, gc),
                    original: at(ol, oc),
                })
                .collect(),
        )
    }

    #[test]
    fn a_position_takes_the_nearest_mapping_before_it_on_its_line() {
        let map = map(&[((3, 5), (1, 1)), ((3, 1), (9, 1)), ((5, 3), (12, 7))]);
        assert_eq!(remap(Some(&map), at(3, 1)), at(9, 1));
        assert_eq!(remap(Some(&map), at(3, 4)), at(9, 1));
        assert_eq!(remap(Some(&map), at(3, 40)), at(1, 1));
        assert_eq!(remap(Some(&map), at(5, 1)), at(12, 7));
    }

    #[test]
    fn a_position_no_mapping_covers_keeps_the_generated_one() {
        let map = map(&[((3, 1), (9, 1))]);
        assert_eq!(remap(Some(&map), at(4, 2)), at(4, 2));
        assert_eq!(remap(None, at(4, 2)), at(4, 2));
    }

    #[test]
    fn every_frame_in_a_project_module_is_rewritten_by_its_own_map() {
        let mut chunks = Chunks::default();
        chunks.insert("script", Some(map(&[((7, 1), (2, 1)), ((9, 3), (4, 3))])));
        chunks.insert("lib/format.ts", Some(map(&[((5, 1), (40, 1))])));
        chunks.insert("lib/plain.js", None);
        let stack = "Error: boom\n    at inner (script:7:10)\n    at happyview.db:7:1\n    \
                     at myscript:7:1\n    at fmt (lib/format.ts:5:3)\n    at lib/plain.js:3:1\n    \
                     at handle (script:20:1)";
        assert_eq!(
            chunks.remap_frames(stack),
            "Error: boom\n    at inner (script:2:1)\n    at happyview.db:7:1\n    \
             at myscript:7:1\n    at fmt (lib/format.ts:40:1)\n    at lib/plain.js:3:1\n    \
             at handle (script:20:1)"
        );
    }

    #[test]
    fn the_innermost_project_frame_names_its_file() {
        let mut chunks = Chunks::default();
        chunks.insert("script", None);
        chunks.insert("lib/format.js", None);
        let stack =
            "    at <native>\n    at happyview.db:3:1\n    at f (lib/format.js:12:5)\n    at script:2:1";
        assert_eq!(
            chunks.line_of(stack),
            Some(("lib/format.js".to_string(), 12))
        );
        assert_eq!(
            chunks.line_of("    at happyview.db:3:1\n    at <native>"),
            None
        );
    }

    #[test]
    fn a_name_matches_only_where_the_frame_location_starts() {
        let mut chunks = Chunks::default();
        chunks.insert("a.js", None);
        assert_eq!(chunks.line_of("    at f (lib/a.js:3:1)"), None);
        assert_eq!(
            chunks.line_of("    at a.js:2:1"),
            Some(("a.js".to_string(), 2))
        );
    }
}
