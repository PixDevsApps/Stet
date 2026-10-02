//! Navigation between documents and positions: the Ctrl+Tab most-recently-used switcher and the
//! back/forward history of caret positions.

mod history;
mod mru;

pub use history::{History, Location};
pub use mru::Mru;
