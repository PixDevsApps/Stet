//! Commands that change the current document's text: undo, redo, the clipboard and column
//! mode's commands ([`editing`]), and the text toolbox ([`toolbox`]) with its dialogs
//! ([`tool_dialogs`]).

pub(super) mod editing;
pub(super) mod tool_dialogs;
pub(super) mod toolbox;
