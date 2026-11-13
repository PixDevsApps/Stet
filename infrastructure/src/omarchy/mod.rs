pub mod colors;
pub mod defaults;
pub mod font;
pub mod paths;
mod theme;

pub use font::fontconfig_dir;
pub use paths::{omarchy_dir, theme_color_script};
pub use theme::{CurrentTheme, read_current_theme};
