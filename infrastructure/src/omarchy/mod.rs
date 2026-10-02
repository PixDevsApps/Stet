pub mod colors;
pub mod defaults;
pub mod font;
pub mod paths;

pub use colors::{parse_colors, resolve_palette};
pub use font::{fontconfig_dir, monospace_family};
pub use paths::{current_dir, current_theme_dir, omarchy_dir, theme_color_script};
