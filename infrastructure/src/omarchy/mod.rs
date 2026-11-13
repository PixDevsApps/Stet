//! Omarchy's side of the desktop: the current theme and its palette, the monospace font, the
//! default editor setting and where Omarchy keeps its state. Everything here also works
//! outside Omarchy, where the files are simply missing and the fallbacks apply.

pub mod colors;
pub mod defaults;
pub mod font;
pub mod paths;
mod theme;

pub use theme::{CurrentTheme, read_current_theme};
