//! Editor view settings: font size and zoom, tab width, and the undo cap.

/// ADR-004: 11 pt by default.
pub const DEFAULT_FONT_SIZE_PT: i32 = 11;

/// The default tab size.
pub const DEFAULT_TAB_WIDTH: u32 = 4;

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
    pub const MIN: i32 = -5;
    pub const MAX: i32 = 20;

    pub const fn new(points: i32) -> Self {
        Self(if points < Self::MIN {
            Self::MIN
        } else if points > Self::MAX {
            Self::MAX
        } else {
            points
        })
    }

    pub const fn points(self) -> i32 {
        self.0
    }

    #[must_use]
    pub const fn zoom_in(self) -> Self {
        Self::new(self.0 + 1)
    }

    #[must_use]
    pub const fn zoom_out(self) -> Self {
        Self::new(self.0 - 1)
    }

    /// The editor font size for a base size, never below 1 pt.
    pub const fn font_size(self, base_pt: i32) -> i32 {
        let size = base_pt + self.0;
        if size < 1 { 1 } else { size }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
