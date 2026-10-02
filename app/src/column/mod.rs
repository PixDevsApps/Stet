//! Column (rectangular) mode, M6 (ADR-016): the editor's view, [`StetView`], keeps a rectangle
//! of visual columns from an anchor to a cursor, paints it, and turns typing, Backspace, Delete,
//! Tab and the clipboard into `domain::column` edits on every line of it. The Column Editor
//! (Alt+C) writes text or numbers down a column.
//!
//! Column mode is entered with Alt+Shift+arrows (registry actions whose application
//! accelerators run before GtkSourceView's `move-viewport` bindings), Alt+drag, Alt+click and
//! Begin/End Select, and left with Escape, a plain arrow, a click, or anything that edits or
//! moves the caret without knowing about columns.

pub mod a11y;
pub mod clipboard;
pub mod dialog;
pub mod edit;
pub mod keys;
mod paint;
mod view;

pub use view::{Collapse, Pad, StetView};

use stet_domain::actions::ActionId;

/// What a column-select action does to the cursor corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    PageUp,
    PageDown,
}

impl Motion {
    fn of(id: ActionId) -> Option<Self> {
        Some(match id {
            ActionId::ColumnSelectLeft => Self::Left,
            ActionId::ColumnSelectRight => Self::Right,
            ActionId::ColumnSelectUp => Self::Up,
            ActionId::ColumnSelectDown => Self::Down,
            ActionId::ColumnSelectLineStart => Self::LineStart,
            ActionId::ColumnSelectLineEnd => Self::LineEnd,
            ActionId::ColumnSelectPageUp => Self::PageUp,
            ActionId::ColumnSelectPageDown => Self::PageDown,
            _ => return None,
        })
    }
}

/// Runs a column-mode registry action on `view`.
pub fn run_action(view: &StetView, id: ActionId) {
    if let Some(motion) = Motion::of(id) {
        view.extend(motion);
    } else if id == ActionId::ColumnBeginEndSelect {
        view.begin_end_select();
    }
}

/// Called before every registry action on the current editor: the typing burst closes first
/// (ADR-016), and an action that does not know about columns ends column mode, leaving the
/// rectangle's corners as a stream selection, so a find or a line operation works on the
/// rectangle's lines. Returns true when column mode carried the action out itself.
pub fn before_action(view: &StetView, id: ActionId) -> bool {
    view.close_burst();
    if view.rect().is_none() {
        return false;
    }
    match id {
        ActionId::Delete => {
            view.delete_forward_or_block(false);
            true
        }
        ActionId::Undo | ActionId::Redo => {
            view.end_column_mode(Collapse::Cursor);
            false
        }
        id if keeps_column_mode(id) => false,
        _ => {
            view.end_column_mode(Collapse::Stream);
            false
        }
    }
}

/// Actions that leave the rectangle alone: column mode's own, the clipboard (the view handles
/// it), and actions that neither edit nor move the caret.
fn keeps_column_mode(id: ActionId) -> bool {
    Motion::of(id).is_some()
        || matches!(
            id,
            ActionId::ColumnBeginEndSelect
                | ActionId::ColumnEditor
                | ActionId::Cut
                | ActionId::Copy
                | ActionId::Paste
                | ActionId::Save
                | ActionId::SaveAs
                | ActionId::SaveAll
                | ActionId::CommandPalette
                | ActionId::ZoomIn
                | ActionId::ZoomOut
                | ActionId::ZoomReset
                | ActionId::FullScreen
                | ActionId::WordWrap
                | ActionId::ShowWhitespace
                | ActionId::ChooseLanguage
                | ActionId::SetLanguage
                | ActionId::NewTab
                | ActionId::Open
                | ActionId::OpenRecent
                | ActionId::NextTab
                | ActionId::PreviousTab
                | ActionId::MoveTabForward
                | ActionId::MoveTabBackward
                | ActionId::About
                | ActionId::ToggleBookmark
                | ActionId::ClearBookmarks
                | ActionId::CopyBookmarkedLines
                | ActionId::InverseBookmarks
                | ActionId::ClearMarks
                | ActionId::CopyMarkedText
                | ActionId::ClearStyle1
                | ActionId::ClearStyle2
                | ActionId::ClearStyle3
                | ActionId::ClearStyle4
                | ActionId::ClearStyle5
                | ActionId::ClearAllStyles
                | ActionId::SwitchView
                | ActionId::CloneToOtherView
                | ActionId::SyncVerticalScrolling
                | ActionId::SyncHorizontalScrolling
                | ActionId::CompareIgnoreWhitespace
                | ActionId::CompareIgnoreCase
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_column_select_action_has_a_motion() {
        let selects = ActionId::ALL
            .into_iter()
            .filter(|id| id.name().starts_with("column-select-"))
            .collect::<Vec<_>>();
        assert_eq!(selects.len(), 8);
        for id in selects {
            assert!(Motion::of(id).is_some(), "{id:?}");
            assert!(keeps_column_mode(id), "{id:?}");
        }
        assert!(!keeps_column_mode(ActionId::Find));
        assert!(!keeps_column_mode(ActionId::SelectAll));
        assert!(!keeps_column_mode(ActionId::GoToLine));
    }
}
