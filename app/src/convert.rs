//! Where GTK's `i32` and `u32` character offsets, line numbers, list positions and pixel sizes
//! meet Rust's `usize`. A valid iterator's offset, line and column are never negative, and no
//! buffer or list holds `i32::MAX` items, so the conversions clamp instead of failing: a
//! negative count becomes 0, and an index past GTK's range becomes its largest value, which is
//! past the end of any buffer or list.

use gtk4 as gtk;
use gtk4::prelude::*;

/// A count or position GTK never makes negative; a negative one counts as 0.
pub(crate) fn to_usize(value: i32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

/// An index or count as GTK's `i32`, saturated at `i32::MAX`.
pub(crate) fn to_i32(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// An index or count as GTK's `u32`, saturated at `u32::MAX` (`GTK_INVALID_LIST_POSITION` for
/// a list position).
pub(crate) fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// The integer part of `value` as a pixel position or size, saturated to `i32`'s range; NaN
/// gives 0.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a float-to-integer `as` saturates, which is the conversion wanted"
)]
pub(crate) fn saturating_i32(value: f64) -> i32 {
    value as i32
}

/// The integer part of `value` as a count, saturated to `usize`'s range; negative values and
/// NaN give 0.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a float-to-integer `as` saturates, which is the conversion wanted"
)]
pub(crate) fn saturating_usize(value: f64) -> usize {
    value as usize
}

/// `value` as the nearest `f32`, for the graphics APIs that take one.
#[expect(
    clippy::cast_possible_truncation,
    reason = "rounding to the nearest f32 is the conversion wanted"
)]
pub(crate) fn to_f32(value: f64) -> f32 {
    value as f32
}

/// A colour channel from 0 to 1 as 0 to 255, rounded.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=255 first"
)]
pub(crate) fn unit_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A colour channel from 0 to 1 as 0 to 65535, truncated.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=65535 first"
)]
pub(crate) fn unit_to_u16(value: f32) -> u16 {
    (value.clamp(0.0, 1.0) * 65535.0) as u16
}

/// A buffer's text by character offsets, line numbers and columns, all zero-based.
pub(crate) trait BufferPositions {
    /// The iterator at character `offset`, or at the end when `offset` is past it.
    fn iter_at_char_offset(&self, offset: usize) -> gtk::TextIter;
    /// The start of `line`, if the buffer has it.
    fn iter_at_line_number(&self, line: usize) -> Option<gtk::TextIter>;
    /// `column` characters into `line`, if the buffer has the line.
    fn iter_at_line_column(&self, line: usize, column: usize) -> Option<gtk::TextIter>;
    /// How many characters the buffer holds.
    fn total_chars(&self) -> usize;
    /// How many lines the buffer holds: at least 1.
    fn total_lines(&self) -> usize;
}

impl<T: IsA<gtk::TextBuffer>> BufferPositions for T {
    fn iter_at_char_offset(&self, offset: usize) -> gtk::TextIter {
        self.iter_at_offset(to_i32(offset))
    }

    fn iter_at_line_number(&self, line: usize) -> Option<gtk::TextIter> {
        self.iter_at_line(to_i32(line))
    }

    fn iter_at_line_column(&self, line: usize, column: usize) -> Option<gtk::TextIter> {
        self.iter_at_line_offset(to_i32(line), to_i32(column))
    }

    fn total_chars(&self) -> usize {
        to_usize(self.char_count())
    }

    fn total_lines(&self) -> usize {
        to_usize(self.line_count()).max(1)
    }
}

/// An iterator's position as a character offset, line number and column, all zero-based.
pub(crate) trait IterPosition {
    fn char_offset(&self) -> usize;
    fn line_number(&self) -> usize;
    /// Characters from the start of the line.
    fn column(&self) -> usize;
}

impl IterPosition for gtk::TextIter {
    fn char_offset(&self) -> usize {
        to_usize(self.offset())
    }

    fn line_number(&self) -> usize {
        to_usize(self.line())
    }

    fn column(&self) -> usize {
        to_usize(self.line_offset())
    }
}
