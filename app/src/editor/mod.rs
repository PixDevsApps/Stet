//! One tab and the document it shows. The buffer is the source of truth for the text
//! (ADR-003) and holds LF line endings only (ADR-008). A document can be shown by two tabs,
//! one in each view of a split window: a clone shares the document and its buffer, and has a
//! view, a caret and banners of its own.
//!
//! - `document`: the document, its buffer and the signal plumbing behind it.
//! - `state`: what the document is backed by and how it is written back.
//! - `page`: the tab widget, its names and the document's state transitions.
//! - [`column`](mod@column): the editor's view, with column mode.
//! - [`load`]: streaming a text into the buffer.
//! - [`edits`]: domain edits applied to the buffer as one undo step.
//! - [`search`], [`marks`]: the page's search state, and the document's bookmarks and marks.
//! - [`comparison`]: a comparison as one document shows it.
//! - `session`: the tab's part in the session.
//! - [`tabstops`]: exact tab stops.
//! - `banners`, `caret`, `appearance`: the rest of what a page shows.

mod appearance;
mod banners;
mod caret;
pub(crate) mod column;
pub(crate) mod comparison;
mod document;
pub(crate) mod edits;
pub(crate) mod load;
pub(crate) mod marks;
mod page;
pub(crate) mod search;
mod session;
mod state;
pub(crate) mod tabstops;

pub use appearance::ViewSettings;
pub use banners::BannerButton;
pub use document::IndentSource;
pub use page::EditorPage;
pub use state::DocState;
