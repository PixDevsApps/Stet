//! The rectangular clipboard (ADR-016): a copied block goes on the clipboard under
//! `application/x-stet-column-block` and as plain text; a block pastes as a rectangle, in
//! column mode and at a normal caret; plain text pastes into a rectangle by its line count: one
//! line is typed on every row, several lines are pasted as a block.

use super::edit;
use super::view::{Collapse, StetView};
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use stet_domain::column::{
    Block, MIME_TYPE, Rect, copy as copy_block, delete_block, paste as paste_block, type_text,
};

/// Copy (or cut) the rectangle; false outside column mode, where GTK copies its selection.
pub(super) fn copy(view: &StetView, cut: bool) -> bool {
    let Some(rect) = view.rect() else {
        return false;
    };
    view.close_burst();
    if rect.is_empty() {
        return true;
    }
    let (first, text, _) = edit::snapshot(view, rect.lines());
    let index = stet_domain::text::LineIndex::new(&text);
    let block = copy_block(
        &text,
        &index,
        rect.shift_lines(-(first as isize)),
        view.tab(),
    );
    set_block(&view.clipboard(), &block);
    if cut {
        if view.is_editable() {
            let tab = view.tab();
            let _ = edit::apply(view, rect, rect.lines(), |text, index, rect| {
                Ok(delete_block(text, index, rect, tab))
            });
        } else {
            view.error_bell();
        }
    }
    true
}

/// Puts `block` on `clipboard` as a block and as plain text for every other application.
pub fn set_block(clipboard: &gdk::Clipboard, block: &Block) {
    let text = block.to_text();
    let bytes = glib::Bytes::from_owned(text.clone().into_bytes());
    let provider = gdk::ContentProvider::new_union(&[
        gdk::ContentProvider::for_bytes(MIME_TYPE, &bytes),
        gdk::ContentProvider::for_value(&text.to_value()),
    ]);
    if let Err(error) = clipboard.set_content(Some(&provider)) {
        tracing::warn!(%error, "could not set the clipboard");
    }
}

/// What a paste brings.
enum Pasted {
    Block(Block),
    Text(String),
}

/// Paste; false when GTK pastes (plain text outside column mode).
pub(super) fn paste(view: &StetView) -> bool {
    let clipboard = view.clipboard();
    let block = clipboard.formats().contain_mime_type(MIME_TYPE);
    if !block && view.rect().is_none() {
        return false;
    }
    view.close_burst();
    if !view.is_editable() {
        view.error_bell();
        return true;
    }
    let weak = view.downgrade();
    glib::spawn_future_local(async move {
        let pasted = if block {
            read(&clipboard, MIME_TYPE)
                .await
                .map(|text| Pasted::Block(Block::from_text(&text)))
        } else {
            clipboard
                .read_text_future()
                .await
                .map(|text| Pasted::Text(text.map(|text| text.to_string()).unwrap_or_default()))
        };
        let Some(view) = weak.upgrade() else {
            return;
        };
        match pasted {
            Ok(pasted) => paste_into(&view, pasted),
            Err(error) => tracing::warn!(%error, "could not read the clipboard"),
        }
    });
    true
}

fn paste_into(view: &StetView, pasted: Pasted) {
    if !view.is_editable() {
        return;
    }
    let tab = view.tab();
    match (view.rect(), pasted) {
        (Some(rect), Pasted::Block(block)) => paste_rows(view, rect, &block),
        (Some(rect), Pasted::Text(text)) => {
            if text.is_empty() {
                return;
            }
            let block = Block::from_plain_text(&text);
            if let [row] = block.rows.as_slice() {
                let _ = edit::apply(view, rect, rect.lines(), |text, index, rect| {
                    type_text(text, index, rect, row, tab)
                });
            } else {
                paste_rows(view, rect, &block);
            }
        }
        // A block at a normal caret: the selection goes, the rows go in from the caret's
        // column, and the caret ends after the first row, outside column mode. One undo step.
        (None, Pasted::Block(block)) => {
            let buffer = view.buffer();
            buffer.begin_user_action();
            buffer.delete_selection(true, true);
            let caret = view.visual_pos(&buffer.iter_at_mark(&buffer.get_insert()));
            paste_rows(view, Rect::new(caret, caret), &block);
            buffer.end_user_action();
            if let Some(rect) = view.rect() {
                view.end_column_mode(Collapse::Keep);
                view.place_caret(rect.cursor);
            }
        }
        (None, Pasted::Text(_)) => {}
    }
}

fn paste_rows(view: &StetView, rect: Rect, block: &Block) {
    let tab = view.tab();
    let lines = rect.top()..rect.lines().end.max(rect.top() + block.rows.len());
    let _ = edit::apply(view, rect, lines, |text, index, rect| {
        Ok(paste_block(text, index, rect, block, tab))
    });
    view.scroll_mark_onscreen(&view.buffer().get_insert());
}

/// Reads `mime` from `clipboard` as UTF-8 text.
pub async fn read(clipboard: &gdk::Clipboard, mime: &str) -> Result<String, glib::Error> {
    let (stream, _) = clipboard
        .read_future(&[mime], glib::Priority::DEFAULT)
        .await?;
    let mut data = Vec::new();
    loop {
        let chunk = stream
            .read_bytes_future(1 << 16, glib::Priority::DEFAULT)
            .await?;
        if chunk.is_empty() {
            break;
        }
        data.extend_from_slice(&chunk);
    }
    let _ = stream.close_future(glib::Priority::DEFAULT).await;
    Ok(String::from_utf8_lossy(&data).into_owned())
}
