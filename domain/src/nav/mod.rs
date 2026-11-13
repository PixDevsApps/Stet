//! Navigation between documents and positions: the Ctrl+Tab most-recently-used switcher, the
//! back/forward history of caret positions, the recent files, the closed tabs and quick open.

mod closed;
mod history;
mod mru;
pub mod quick_open;
mod recent;

pub use closed::{ClosedTab, ClosedTabs};
pub use history::{History, Location};
pub use mru::Mru;
pub use recent::{RECENT_LIMIT, RecentFiles};
