//! Text without a toolkit: character and byte offsets as types of their own, edits in character offsets (ADR-003),
//! line endings (ADR-008), lines, selections, word counts and how counts are shown. Every item
//! is used from here, as `text::Item`; the submodules only group the code.

mod chars;
mod edit;
mod eol;
mod index;
mod lines;
mod offset;
mod selection;
mod units;
mod wording;
mod words;

pub use chars::{NUL_PLACEHOLDER, fold_char, is_word_char};
pub use edit::{
    BULK_EDIT_THRESHOLD, Bias, EditError, TextEdit, apply, map_offset, minimal_edit, normalize,
};
pub use eol::{Eol, EolStats, LfNormalizer, normalize_to_lf};
pub use index::{LineIndex, Position};
pub use lines::{
    DISPLAY_BREAK_CHARS, LONG_LINE_CHARS, LongestLine, break_long_lines, line_count, longest_line,
};
pub use offset::{ByteCursor, CharCursor, LineCursor, char_to_byte, count_chars, next_boundary};
pub use selection::Selection;
pub use units::{Anchor, ByteOffset, CharOffset, OffsetRange};
pub use wording::{counted, grouped, plural};
pub use words::count_words;
