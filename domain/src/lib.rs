//! Toolkit-independent editor logic. No I/O, no GTK, no threads.

pub mod actions;
pub mod column;
pub mod diff;
pub mod document;
pub mod fuzzy;
pub mod indent;
pub mod language;
pub mod location;
pub mod marks;
pub mod nav;
pub mod ops;
pub mod palette;
pub mod quick_open;
pub mod recent;
pub mod replace_files;
pub mod search;
pub mod session;
pub mod settings;
pub mod text;
pub mod theme;
pub mod untitled;
pub mod view;
