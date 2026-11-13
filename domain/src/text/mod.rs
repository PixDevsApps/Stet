pub mod chars;
pub mod edit;
pub mod eol;
pub mod index;
pub mod lines;
pub mod selection;
pub mod words;

pub use chars::{NUL_PLACEHOLDER, fold_char, is_word_char};
pub use edit::{
    BULK_EDIT_THRESHOLD, Bias, EditError, TextEdit, apply, map_offset, minimal_edit, normalize,
};
pub use eol::{Eol, EolStats, normalize_to_lf};
pub use index::{LineIndex, Position};
pub use lines::{
    DISPLAY_BREAK_CHARS, LONG_LINE_CHARS, LongestLine, break_long_lines, line_count, longest_line,
};
pub use selection::Selection;
pub use words::count_words;
