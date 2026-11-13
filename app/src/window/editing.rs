//! The registry's commands on the current editor that no other feature owns: undo, redo and
//! the clipboard for the menus and the palette (the same keys reach the focused widget
//! directly), and column mode's selections and the Column Editor.

use super::Window;
use crate::column::Motion;
use gtk4::prelude::*;

/// Undo, redo and the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EditCommand {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    SelectAll,
}

/// Column mode's commands (ADR-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ColumnCommand {
    /// Extends the rectangle: Alt+Shift+arrows and their kin.
    Select(Motion),
    BeginEndSelect,
    /// Opens the Column Editor.
    Editor,
}

impl Window {
    /// Runs an edit command on the current editor, which then has the focus.
    pub(super) fn run_edit(&self, command: EditCommand) {
        let Some(page) = self.current_page() else {
            return;
        };
        let view = page.view();
        let buffer = page.buffer();
        match command {
            EditCommand::Undo => {
                if buffer.can_undo() {
                    buffer.undo();
                }
            }
            EditCommand::Redo => {
                if buffer.can_redo() {
                    buffer.redo();
                }
            }
            EditCommand::Cut => view.emit_cut_clipboard(),
            EditCommand::Copy => view.emit_copy_clipboard(),
            EditCommand::Paste => view.emit_paste_clipboard(),
            EditCommand::Delete => {
                buffer.delete_selection(true, view.is_editable());
            }
            EditCommand::SelectAll => view.emit_select_all(true),
        }
        view.grab_focus();
    }

    /// Runs a column-mode command on the current editor.
    pub(super) fn run_column(&self, command: ColumnCommand) {
        let Some(page) = self.current_page() else {
            return;
        };
        let view = page.column_view();
        match command {
            ColumnCommand::Select(motion) => view.extend(motion),
            ColumnCommand::BeginEndSelect => view.begin_end_select(),
            ColumnCommand::Editor => crate::column::dialog::open(self, view),
        }
    }
}
