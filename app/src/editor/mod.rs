//! One tab and the document it shows. The buffer is the source of truth for the text
//! (ADR-003) and holds LF line endings only (ADR-008). A document can be shown by two tabs,
//! one in each view of a split window: a clone shares the document and its buffer, and has a
//! view, a caret and banners of its own.
//!
//! - `document`: the document, its buffer and the signal plumbing behind it.
//! - `state`: what the document is backed by and how it is written back.
//! - `page`: the tab widget, its names and the document's state transitions.
//! - `load`: streaming a text into the buffer.
//! - `banners`, `caret`, `marks`, `search`, `appearance`: the rest of what a page shows.

mod appearance;
mod banners;
mod caret;
mod document;
mod load;
mod marks;
mod page;
mod search;
mod state;

pub use appearance::ViewSettings;
pub use document::IndentSource;
pub use load::{CHUNK_INTERVAL, JUMP_LOAD_WAIT, LOAD_WAIT, LoadStats};
pub use page::EditorPage;
pub use state::DocState;
