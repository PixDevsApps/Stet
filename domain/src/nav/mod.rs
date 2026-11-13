//! Navigation between documents and positions: the Ctrl+Tab most-recently-used switcher, the
//! back/forward history of caret positions, the recent files and quick open.

mod history;
mod mru;
pub mod quick_open;
mod recent;

pub use history::{History, Location};
pub use mru::Mru;
pub use recent::{RECENT_LIMIT, RecentFiles};
