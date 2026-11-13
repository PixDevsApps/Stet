//! Toolkit-independent editor logic. No I/O, no GTK, no threads.
//!
//! The modules follow the editor's bounded contexts (ADR-020):
//! - [`text`]: character and byte offsets, edits, line endings and lines.
//! - [`document`]: names and titles, untitled tabs, indentation, language and `file:line`.
//! - [`search`]: queries, the regex dialect, replacement templates and Replace in Files.
//! - [`ops`]: the text tools (case, lines, comments, numbers, JSON and XML).
//! - [`column`](mod@column): column mode and rectangular selections.
//! - [`diff`]: comparing two documents.
//! - [`marks`]: bookmarks, marked lines and the token styles.
//! - [`nav`]: the tab switcher, position history, recent files and quick open.
//! - [`fuzzy`]: the fuzzy matcher that quick open, the palette and the pickers rank with.
//! - [`session`]: the tabs and window state that `session.json` keeps, and the backup policy.
//! - [`settings`]: `config.toml` and the keymap in `keys.toml`.
//! - [`actions`]: the commands, their accelerators and the menus built from them.
//! - [`view`]: the editor view's font size, zoom, tab width and undo cap.
//! - [`theme`]: colours, the Omarchy palette and the editor's colour roles.
//! - [`updates`]: checking for a newer release.

pub mod actions;
pub mod column;
pub mod diff;
pub mod document;
pub mod fuzzy;
pub mod marks;
pub mod nav;
pub mod ops;
pub mod search;
pub mod session;
pub mod settings;
pub mod text;
pub mod theme;
pub mod updates;
pub mod view;
