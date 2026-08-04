//! One axis (rows or columns) of the grid: a default size with sparse explicit
//! overrides, and the cumulative offset index that maps between line indices and
//! positions. See `docs/42-GRID-LAYOUT-AND-RENDERING-ARCHITECTURE.md`.
//!
//! Positions are in twips (1/1440 inch). Queries are O(overrides) — effectively
//! O(1) for a uniform grid; a prefix-sum can make it O(log n) if a sheet ever
//! carries many overrides.

use std::collections::BTreeMap;

/// A grid axis: a default line size plus per-line overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Axis {
    default_size: i64,
    overrides: BTreeMap<u32, i64>,
}

impl Axis {
    /// A uniform axis with the given default line size (twips).
    pub fn uniform(default_size: i64) -> Self {
        Self {
            default_size,
            overrides: BTreeMap::new(),
        }
    }

    /// Set an explicit size (twips) for one line.
    pub fn set_size(&mut self, line: u32, size: i64) {
        self.overrides.insert(line, size);
    }

    /// The size (twips) of `line`.
    pub fn size(&self, line: u32) -> i64 {
        self.overrides
            .get(&line)
            .copied()
            .unwrap_or(self.default_size)
    }

    /// The twip position of the leading edge of `line` (sum of all sizes before it).
    pub fn offset(&self, line: u32) -> i64 {
        let mut acc = 0i64;
        let mut cursor = 0u32;
        for (&over_line, &size) in self.overrides.range(..line) {
            acc += (over_line - cursor) as i64 * self.default_size;
            acc += size;
            cursor = over_line + 1;
        }
        acc + (line - cursor) as i64 * self.default_size
    }

    /// The line containing twip position `pos` (clamped to `0` for negatives).
    pub fn line_at(&self, pos: i64) -> u32 {
        if pos <= 0 {
            return 0;
        }
        let mut acc = 0i64;
        let mut cursor = 0u32;
        for (&over_line, &size) in &self.overrides {
            // The default-sized run [cursor, over_line).
            let run = (over_line - cursor) as i64 * self.default_size;
            if acc + run > pos {
                return cursor + ((pos - acc) / self.default_size) as u32;
            }
            acc += run;
            cursor = over_line;
            // The overridden line itself.
            if acc + size > pos {
                return cursor;
            }
            acc += size;
            cursor += 1;
        }
        cursor + ((pos - acc) / self.default_size) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::Axis;

    #[test]
    fn uniform_offsets_and_lookup() {
        let axis = Axis::uniform(100);
        assert_eq!(axis.offset(0), 0);
        assert_eq!(axis.offset(3), 300);
        assert_eq!(axis.size(5), 100);
        assert_eq!(axis.line_at(0), 0);
        assert_eq!(axis.line_at(250), 2);
        assert_eq!(axis.line_at(299), 2);
        assert_eq!(axis.line_at(300), 3);
    }

    #[test]
    fn overrides_shift_offsets_and_lookup() {
        let mut axis = Axis::uniform(100);
        axis.set_size(2, 500); // line 2 is 500 wide
        // offsets: 0,100,200, [500], 700,800,...
        assert_eq!(axis.offset(2), 200);
        assert_eq!(axis.offset(3), 700);
        assert_eq!(axis.offset(4), 800);
        assert_eq!(axis.line_at(199), 1);
        assert_eq!(axis.line_at(200), 2);
        assert_eq!(axis.line_at(699), 2); // still within the wide line 2
        assert_eq!(axis.line_at(700), 3);
    }

    #[test]
    fn offset_and_line_at_are_inverse() {
        let mut axis = Axis::uniform(120);
        axis.set_size(10, 40);
        axis.set_size(25, 300);
        for line in [0u32, 1, 9, 10, 11, 24, 25, 26, 100] {
            let off = axis.offset(line);
            assert_eq!(axis.line_at(off), line, "line {line} offset {off}");
        }
    }
}
