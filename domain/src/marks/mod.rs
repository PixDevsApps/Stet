//! Bookmarks, Mark results and style tokens, and the bookmarked-line operations of the
//! Search > Bookmark menu.
//!
//! The GtkSourceView buffer keeps bookmarks as marks while the user types. These models hold
//! them by line for the operations here and for bulk edits, where buffer marks collapse and undo
//! does not restore them (ADR-003 amendment).

mod bookmarks;
mod history;
mod lines;
mod styles;

pub use bookmarks::Bookmarks;
pub use history::{BookmarkHistory, Footprint, Step};
pub use lines::{
    Cut, LineEdit, copy_bookmarked_lines, cut_bookmarked_lines, delete_bookmarked_lines,
    delete_unbookmarked_lines, paste_to_bookmarked_lines,
};
pub use styles::{MarkStyle, Marks, bookmark_lines_of};
