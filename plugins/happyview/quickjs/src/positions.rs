//! Where an error happened, in the author's own source.
//!
//! QuickJS places an error in the module it compiled, which is the front
//! end's output rather than what the author wrote whenever the front end
//! rewrote anything. A front end that does hands back a [`SourceMap`] with
//! its output, and every position the engine reports — an error's `line`,
//! each `script:L:C` frame in its `raw` stack, a compile error, a validate
//! error — passes through [`remap`] on its way out. With no map, every
//! position is already the author's and passes through untouched.

use crate::sandbox;

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
#[derive(Debug, Default)]
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

/// A `script:L:C` frame location at the start of `text`, and how many bytes
/// it takes.
fn frame_at(text: &str) -> Option<(Position, usize)> {
    let marker = format!("{}:", sandbox::MODULE_NAME);
    let rest = text.strip_prefix(&marker)?;
    let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let line_len = digits(rest);
    let after_line = rest[line_len..].strip_prefix(':')?;
    let column_len = digits(after_line);
    if line_len == 0 || column_len == 0 {
        return None;
    }
    let position = Position {
        line: rest[..line_len].parse().ok()?,
        column: after_line[..column_len].parse().ok()?,
    };
    Some((position, marker.len() + line_len + 1 + column_len))
}

/// `stack` with every frame in the script's own module moved to the
/// author's position. A frame is recognised only where QuickJS writes one,
/// after `(` or `at `, so a library's module whose name merely ends in the
/// script's is left alone.
pub fn remap_frames(stack: &str, map: Option<&SourceMap>) -> String {
    if map.is_none() {
        return stack.to_string();
    }
    let mut out = String::with_capacity(stack.len());
    let mut rest = stack;
    while let Some(at) = rest.find(sandbox::MODULE_NAME) {
        let (before, candidate) = rest.split_at(at);
        out.push_str(before);
        let framed = before.ends_with('(') || before.ends_with("at ");
        match frame_at(candidate).filter(|_| framed) {
            Some((position, len)) => {
                let position = remap(map, position);
                out.push_str(&format!(
                    "{}:{}:{}",
                    sandbox::MODULE_NAME,
                    position.line,
                    position.column
                ));
                rest = &candidate[len..];
            }
            None => {
                out.push_str(&candidate[..sandbox::MODULE_NAME.len()]);
                rest = &candidate[sandbox::MODULE_NAME.len()..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The author's line of the innermost frame in the script's own module, from
/// a stack QuickJS writes as `    at handle (script:12:5)` or
/// `    at script:12:5`. A frame in a library's module or in a native
/// function is skipped, so an error thrown by the bridge is placed at the
/// line that called it.
pub fn line_of(stack: &str, map: Option<&SourceMap>) -> Option<u32> {
    let marker = format!("{}:", sandbox::MODULE_NAME);
    stack.lines().find_map(|frame| {
        let frame = frame.trim();
        let location = match frame.rfind(&format!("({marker}")) {
            Some(at) => &frame[at + 1..],
            None => frame
                .strip_prefix("at ")
                .filter(|rest| rest.starts_with(&marker))?,
        };
        frame_at(location).map(|(position, _)| remap(map, position).line)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: u32, column: u32) -> Position {
        Position { line, column }
    }

    fn map(pairs: &[((u32, u32), (u32, u32))]) -> SourceMap {
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
        // Before the line's first mapping: that mapping, the line's
        // statement.
        assert_eq!(remap(Some(&map), at(5, 1)), at(12, 7));
    }

    #[test]
    fn a_position_no_mapping_covers_keeps_the_generated_one() {
        let map = map(&[((3, 1), (9, 1))]);
        // Line 4 has no mapping; the one on line 3 is not borrowed.
        assert_eq!(remap(Some(&map), at(4, 2)), at(4, 2));
        assert_eq!(remap(None, at(4, 2)), at(4, 2));
    }

    #[test]
    fn every_frame_in_the_script_is_rewritten_and_nothing_else() {
        let map = map(&[((7, 1), (2, 1)), ((9, 3), (4, 3))]);
        let stack = "Error: boom\n    at inner (script:7:10)\n    at happyview.db:7:1\n    \
                     at myscript:7:1\n    at script:9:3\n    at handle (script:20:1)";
        assert_eq!(
            remap_frames(stack, Some(&map)),
            "Error: boom\n    at inner (script:2:1)\n    at happyview.db:7:1\n    \
             at myscript:7:1\n    at script:4:3\n    at handle (script:20:1)"
        );
        assert_eq!(line_of(stack, Some(&map)), Some(2));
        assert_eq!(remap_frames(stack, None), stack);
        assert_eq!(line_of(stack, None), Some(7));
    }

    #[test]
    fn a_stack_with_no_frame_in_the_script_has_no_line() {
        assert_eq!(
            line_of("    at happyview.db:3:1\n    at <native>", None),
            None
        );
    }
}
