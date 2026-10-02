pub mod edit;
pub mod eol;
pub mod index;
pub mod lines;

pub use edit::{Bias, EditError, TextEdit, apply, map_offset, minimal_edit, normalize};
pub use eol::{Eol, EolStats, convert_from_lf, normalize_to_lf};
pub use index::{LineIndex, Position};
pub use lines::{
    DISPLAY_BREAK_CHARS, LONG_LINE_CHARS, LongestLine, break_long_lines, line_count, longest_line,
};
