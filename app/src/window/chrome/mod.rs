//! The window's chrome around the editor: the status bar ([`status`]) and the pickers it opens
//! (the language in [`language`], the encoding in [`encoding_picker`], the indentation in
//! [`indentation`]), the hamburger menu ([`menu`]), the command palette ([`palette`]), and
//! accessible names for the items of the popover menus ([`a11y`]).

pub(super) mod a11y;
pub(super) mod encoding_picker;
pub(super) mod indentation;
pub(super) mod language;
pub(super) mod menu;
pub(super) mod palette;
pub(super) mod status;
