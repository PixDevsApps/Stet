//! The tabs, the views and moving between them: the tab strip's menu and the commands behind it
//! ([`tabs`]), pinned tabs ([`pins`]), the split view ([`split`]), the View menu's window-wide
//! commands ([`view`]), the Ctrl+Tab switcher and the jumps back and forward ([`nav`]), and
//! quick open ([`quick_open`]).

pub(super) mod nav;
pub(super) mod pins;
pub(super) mod quick_open;
pub(super) mod split;
pub(super) mod tabs;
pub(super) mod view;
