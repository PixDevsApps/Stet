//! Editor view settings: font size and zoom, tab width, and the undo cap.

use std::fmt;
use std::num::NonZeroU8;

use crate::text::VisualColumn;

/// ADR-004: 11 pt by default.
pub const DEFAULT_FONT_SIZE_PT: i32 = 11;

/// How many cells a tab advances to: the next multiple of the width. Never 0; `config.toml`
/// allows 1 to 16, GtkSourceView up to 32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabWidth(NonZeroU8);

impl TabWidth {
    /// The default: four cells.
    pub const DEFAULT: Self = Self(NonZeroU8::new(4).unwrap());

    /// A width of `cells` cells, or `None` for 0.
    #[must_use]
    pub const fn new(cells: u8) -> Option<Self> {
        match NonZeroU8::new(cells) {
            Some(cells) => Some(Self(cells)),
            None => None,
        }
    }

    /// The width in cells.
    #[must_use]
    pub fn get(self) -> usize {
        usize::from(self.0.get())
    }

    /// The column a tab that starts at `column` advances to.
    #[must_use]
    pub fn next_stop(self, column: VisualColumn) -> VisualColumn {
        column + (self.get() - column.get() % self.get())
    }
}

impl Default for TabWidth {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// GtkSourceView's `tab-width` and the number `config.toml` writes.
impl From<TabWidth> for u32 {
    fn from(width: TabWidth) -> Self {
        Self::from(width.0.get())
    }
}

/// A width read from `config.toml`, or from GtkSourceView's `tab-width`: 1 to 255.
impl TryFrom<u32> for TabWidth {
    type Error = std::num::TryFromIntError;

    fn try_from(cells: u32) -> Result<Self, Self::Error> {
        NonZeroU8::try_from(u8::try_from(cells)?).map(Self)
    }
}

impl TryFrom<i64> for TabWidth {
    type Error = std::num::TryFromIntError;

    fn try_from(cells: i64) -> Result<Self, Self::Error> {
        NonZeroU8::try_from(u8::try_from(cells)?).map(Self)
    }
}

impl fmt::Display for TabWidth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The undo cap set on every buffer (ADR-003 amendment, 2026-10-01). It limits undo
/// steps, not edits: typing merges into word-sized steps, and bulk operations are one step.
pub const MAX_UNDO_LEVELS: u32 = 1000;

/// Syntax highlighting is on by default up to this size (ADR-014 amendment, 2026-10-01):
/// above it, GtkSourceView's first pass over the file held off frames for 0.5–1.5 s while
/// typing right after opening.
pub const HIGHLIGHT_MAX_BYTES: usize = 2 * 1024 * 1024;

/// ... and up to this many lines: typing right after opening got sluggish with more lines
/// at the same size.
pub const HIGHLIGHT_MAX_LINES: usize = 100_000;

/// Zoom in points relative to the base size, as in Scintilla (which allows -10 to +20).
/// Below -5 an 11 pt font gets unreadable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Zoom(i32);

impl Zoom {
    /// The smallest zoom, in points.
    pub const MIN: i32 = -5;
    /// The largest zoom, in points.
    pub const MAX: i32 = 20;

    /// A zoom of `points`, clamped to [`MIN`](Self::MIN)..=[`MAX`](Self::MAX).
    #[must_use]
    pub const fn new(points: i32) -> Self {
        Self(if points < Self::MIN {
            Self::MIN
        } else if points > Self::MAX {
            Self::MAX
        } else {
            points
        })
    }

    /// The zoom in points over the base font size.
    #[must_use]
    pub const fn points(self) -> i32 {
        self.0
    }

    /// One point larger, up to [`MAX`](Self::MAX).
    #[must_use]
    pub const fn zoom_in(self) -> Self {
        Self::new(self.0 + 1)
    }

    /// One point smaller, down to [`MIN`](Self::MIN).
    #[must_use]
    pub const fn zoom_out(self) -> Self {
        Self::new(self.0 - 1)
    }

    /// The editor font size for a base size, never below 1 pt.
    #[must_use]
    pub const fn font_size(self, base_pt: i32) -> i32 {
        let size = base_pt + self.0;
        if size < 1 { 1 } else { size }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_advance_to_the_next_multiple_of_the_width() {
        let width = TabWidth::DEFAULT;
        assert_eq!(width.get(), 4);
        assert_eq!(width.next_stop(VisualColumn::ZERO), VisualColumn::new(4));
        assert_eq!(width.next_stop(VisualColumn::new(3)), VisualColumn::new(4));
        assert_eq!(width.next_stop(VisualColumn::new(4)), VisualColumn::new(8));
        assert_eq!(TabWidth::new(0), None);
        assert_eq!(TabWidth::try_from(0_u32).ok(), None);
        assert_eq!(TabWidth::try_from(256_i64).ok(), None);
        assert_eq!(TabWidth::try_from(16_i64).ok(), TabWidth::new(16));
        assert_eq!(u32::from(TabWidth::new(9).unwrap()), 9);
        assert_eq!(TabWidth::DEFAULT.to_string(), "4");
    }

    #[test]
    fn zoom_steps_and_clamps() {
        let zoom = Zoom::default();
        assert_eq!(zoom.zoom_in().points(), 1);
        assert_eq!(zoom.zoom_out().points(), -1);
        assert_eq!(Zoom::new(100).points(), Zoom::MAX);
        assert_eq!(Zoom::new(-100).points(), Zoom::MIN);
        assert_eq!(Zoom::new(Zoom::MAX).zoom_in().points(), Zoom::MAX);
        assert_eq!(Zoom::new(3).font_size(DEFAULT_FONT_SIZE_PT), 14);
        assert_eq!(Zoom::new(-5).font_size(3), 1);
    }
}
