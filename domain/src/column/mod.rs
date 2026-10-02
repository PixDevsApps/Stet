//! Column (rectangular) mode for M6: visual-column math, the rectangle, and the column edits,
//! as pure functions over a text snapshot that return [`TextEdit`](crate::text::TextEdit)s in
//! character offsets (ADR-003, ADR-016).
//!
//! # Cells
//!
//! A line is a row of cells. A tab advances to the next multiple of the tab width, and every
//! other character takes one cell, wide East Asian characters and combining marks included.
//! That is how `gtk_source_view_get_visual_column` counts, so a column here is the column that
//! GtkSourceView and the status bar report. A font draws a wide character wider than one cell,
//! so on its line the painted rectangle, which follows the glyphs, has a ragged edge; it still
//! covers exactly the characters an edit touches.
//!
//! Columns past the end of a line are virtual space: inserting there pads the line with spaces,
//! and deleting there does nothing. No edit adds or removes a line break, except that [`paste`]
//! appends lines when the block runs past the end of the text.
//!
//! # A tab across the column
//!
//! When an edit's column falls strictly inside a tab, the tab is first replaced by the spaces it
//! is drawn as, so the edit lands exactly on the column and nothing before it moves. Only that
//! tab changes, and only on the lines the edit touches. A tab that starts or ends at the column
//! stays a tab, and tabs after the column still align to their stops.
//!
//! Snapping to the tab's start, as the S5 prototype did, puts typed text left of the painted
//! caret, splits a typed word around the tab (`ab\tcde`), deletes more or fewer cells than the
//! block shows, and copies a tab whose width changes wherever it is pasted. Scintilla never
//! changes a tab: it snaps each line's edge to the nearest character boundary by pixel x,
//! paints those ragged edges, and re-snaps every line from the anchor's x after each keystroke.
//! Vim's blockwise operators and Emacs's rectangle commands (`move-to-column` with FORCE) split
//! the tab into spaces, as here.
//!
//! # Backspace and Delete
//!
//! Both delete a block. At a column caret, Backspace deletes the character before the caret on
//! every line when those characters are equally wide, so a tab typed in column mode, or tabs
//! that indent every line up to the caret, go in one keystroke; otherwise it deletes one cell,
//! splitting a tab as above. Delete does the same after the caret. Neither joins lines, and
//! lines that end before the deleted cells keep their text: Backspace at column 0 does nothing,
//! and in virtual space it only moves the caret.
//!
//! # Clipboard, large edits and snapshots
//!
//! [`copy`] returns each line's cells as a [`Block`] row, whose [`Block::to_text`] goes on the
//! clipboard under [`MIME_TYPE`] and as `text/plain`. [`paste`] writes row *i* on line top + *i*
//! at the rectangle's left column.
//!
//! A [`ColumnEdit`] of more than [`BULK_THRESHOLD`] edits goes to the buffer as one
//! [`BulkEdit`] (ADR-003 amendment).
//!
//! An operation reads only the lines it edits (for [`paste`], the lines its rows land on, and
//! whether the text ends there), so a caller can pass a snapshot of just those lines: move the
//! rectangle into it with [`Rect::shift_lines`] and add the snapshot's first character offset
//! to the edits.

mod block;
mod bulk;
mod editing;
mod editor;
mod rect;
mod visual;

#[cfg(test)]
mod tests;

pub use block::{Block, MIME_TYPE, copy, paste};
pub use bulk::{BULK_THRESHOLD, BulkEdit};
pub use editing::{
    ColumnEdit, ColumnError, backspace, delete_block, delete_forward, insert, type_text,
};
pub use editor::{Base, ColumnEditor, Leading, NumberSequence};
pub use rect::{LineSpan, Rect, VisualPos, spans};
pub use visual::{Located, TabCut, home_column, locate, next_column, text_width, visual_column};
