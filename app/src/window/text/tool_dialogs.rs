//! The toolbox's two dialogs: Add Prefix/Suffix and Insert Numbers, simpler forms of
//! the Column Editor. Compact alert dialogs: Tab moves between the fields, Enter applies,
//! Escape cancels, and the fields remember their last values.

use crate::convert::{BufferPositions, to_u32, to_usize};
use crate::editor::EditorPage;
use crate::editor::column::dialog::NUMBERS_OVERFLOW;
use crate::editor::edits;
use crate::window::Window;
use crate::window::dialogs::click_response;
use crate::window::text::toolbox::Reach;
use gtk4 as gtk;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::fmt::Display;
use std::ops::Range;
use std::str::FromStr;
use stet_domain::ops::numbers::{Base, NumberOverflow, NumberSequence, Padding};
use stet_domain::ops::{Change, SyntaxError, Target, prefix};
use stet_domain::text::LineIndex;

/// The response that applies a tool dialog.
pub const APPLY: &str = "apply";

const BASES: [(&str, Base); 4] = [
    ("Decimal", Base::Decimal),
    ("Hexadecimal", Base::Hex),
    ("Octal", Base::Octal),
    ("Binary", Base::Binary),
];

const PADDINGS: [(&str, Padding); 3] = [
    ("None (left-aligned)", Padding::None),
    ("Leading zeros", Padding::Zeros),
    ("Leading spaces", Padding::Spaces),
];

fn dialog(heading: &str, body: &str, apply: &str, rows: &[&gtk::Widget]) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    for row in rows {
        list.append(*row);
    }
    dialog.set_extra_child(Some(&list));
    dialog.add_responses(&[("cancel", "_Cancel"), (APPLY, apply)]);
    dialog.set_response_appearance(APPLY, adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some(APPLY));
    dialog.set_close_response("cancel");
    if let Some(first) = rows.first() {
        dialog.set_focus(Some(*first));
    }
    for row in rows {
        if let Some(entry) = row.downcast_ref::<adw::EntryRow>() {
            enter_applies(&dialog, entry, APPLY);
        }
    }
    dialog
}

/// Enter in `entry` answers `dialog` with `response`, as a click on that button does.
pub fn enter_applies(dialog: &adw::AlertDialog, entry: &adw::EntryRow, response: &'static str) {
    let dialog = dialog.downgrade();
    entry.connect_entry_activated(move |_| {
        if let Some(dialog) = dialog.upgrade() {
            click_response(&dialog, response);
        }
    });
}

fn entry_row(title: &str, text: &str) -> adw::EntryRow {
    adw::EntryRow::builder().title(title).text(text).build()
}

/// A whole number typed into `row`, between `min` and `max`.
fn number<T>(row: &adw::EntryRow, min: T, max: T) -> Result<T, String>
where
    T: FromStr + PartialOrd + Display + Copy,
{
    let title = row.title();
    let text = row.text();
    match text.trim().parse::<T>() {
        Ok(value) if (min..=max).contains(&value) => Ok(value),
        _ => Err(format!(
            "{title} must be a whole number from {min} to {max}, not “{text}”"
        )),
    }
}

/// How many lines Insert Numbers numbers: from the caret's line to the end, or every line the
/// selection touches except a last one it reaches only at column 0 (as `Target::Selection`).
fn numbered_lines(buffer: &sourceview5::Buffer, selected: &Range<usize>) -> usize {
    let first = buffer.iter_at_char_offset(selected.start);
    let last = if selected.is_empty() {
        buffer.line_count() - 1
    } else {
        let end = buffer.iter_at_char_offset(selected.end);
        if end.line() > first.line() && end.starts_line() {
            end.line() - 1
        } else {
            end.line()
        }
    };
    to_usize(last - first.line() + 1)
}

fn combo_row(title: &str, choices: &[&str], selected: usize) -> adw::ComboRow {
    let row = adw::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(choices))
        .build();
    row.set_selected(to_u32(selected));
    row
}

impl Window {
    /// Add Prefix/Suffix: text at the start and the end of every line of the selection, or
    /// of the whole document.
    pub(in crate::window) async fn ask_prefix_suffix(&self, page: &EditorPage) {
        if !self.tool_allowed(page) {
            return;
        }
        let tools = &self.inner.tools;
        let prefix_row = entry_row("Prefix", &tools.prefix.borrow());
        let suffix_row = entry_row("Suffix", &tools.suffix.borrow());
        let dialog = dialog(
            "Add Prefix/Suffix",
            "Adds the text at the start and the end of every line of the selection, or of the \
             whole document.",
            "_Add",
            &[prefix_row.upcast_ref(), suffix_row.upcast_ref()],
        );
        if self.ask(&dialog).await != APPLY {
            return;
        }
        let (prefix, suffix) = (prefix_row.text().to_string(), suffix_row.text().to_string());
        tools.prefix.replace(prefix.clone());
        tools.suffix.replace(suffix.clone());
        if prefix.is_empty() && suffix.is_empty() {
            return;
        }
        self.run_tool(
            page,
            Reach::Document,
            Box::new(move |text, target| Ok(prefix::add(text, &target, &prefix, &suffix))),
            "Add Prefix/Suffix",
        );
    }

    /// Insert Numbers: one number per line at the caret's column, from the caret's line to
    /// the end (as the Column Editor does with a caret), or on the lines of the selection.
    pub(in crate::window) async fn ask_numbers(&self, page: &EditorPage) {
        if !self.tool_allowed(page) {
            return;
        }
        let tools = &self.inner.tools;
        let last = *tools.numbering.borrow();
        let start = entry_row("Start", &last.start.to_string());
        let step = entry_row("Step", &last.step.to_string());
        let repeat = entry_row("Repeat each number", &last.repeat.max(1).to_string());
        let base = combo_row(
            "Base",
            &BASES.map(|(label, _)| label),
            BASES
                .iter()
                .position(|(_, base)| *base == last.base)
                .unwrap_or(0),
        );
        let uppercase = adw::SwitchRow::builder()
            .title("Uppercase hex digits")
            .active(last.uppercase)
            .build();
        let padding = combo_row(
            "Fill to the widest",
            &PADDINGS.map(|(label, _)| label),
            PADDINGS
                .iter()
                .position(|(_, padding)| *padding == last.padding)
                .unwrap_or(0),
        );
        let width = entry_row("Minimum width", &last.width.to_string());
        let dialog = dialog(
            "Insert Numbers",
            "Inserts a number on every line at the caret's column, from the caret's line to the \
             end, or on the lines of the selection.",
            "_Insert",
            &[
                start.upcast_ref(),
                step.upcast_ref(),
                repeat.upcast_ref(),
                base.upcast_ref(),
                uppercase.upcast_ref(),
                padding.upcast_ref(),
                width.upcast_ref(),
            ],
        );
        if self.ask(&dialog).await != APPLY {
            return;
        }
        let read = || -> Result<NumberSequence, String> {
            Ok(NumberSequence {
                start: number(&start, -(1 << 53), 1 << 53)?,
                step: number(&step, -(1 << 40), 1 << 40)?,
                repeat: number(&repeat, 1, 1 << 30)?,
                base: BASES[(base.selected() as usize).min(BASES.len() - 1)].1,
                uppercase: uppercase.is_active(),
                padding: PADDINGS[(padding.selected() as usize).min(PADDINGS.len() - 1)].1,
                width: number(&width, 0, 256)?,
            })
        };
        let numbering = match read() {
            Ok(numbering) => numbering,
            Err(message) => {
                self.show_error("Can't insert numbers", &message).await;
                return;
            }
        };
        tools.numbering.replace(numbering);
        let buffer = page.buffer();
        let (selected, _) = edits::selection(&buffer);
        if numbering.check(numbered_lines(&buffer, &selected)).is_err() {
            self.show_error("Can't insert numbers", NUMBERS_OVERFLOW)
                .await;
            return;
        }
        self.run_on_target(
            page,
            Target::Selection(selected),
            Box::new(move |text, target| {
                let index = LineIndex::new(text);
                let Target::Selection(range) = target else {
                    return Ok(Change::default());
                };
                let (offset, line) = (range.start, index.line_of_char(range.start));
                let column = offset - index.line_chars(line).start;
                let target = if range.is_empty() {
                    Target::Lines(line..index.line_count())
                } else {
                    Target::Selection(range)
                };
                prefix::insert_numbers(text, &target, column, &numbering).map_err(
                    |overflow: NumberOverflow| SyntaxError {
                        message: overflow.to_string(),
                        line: line + 1,
                        column: column + 1,
                        offset,
                    },
                )
            }),
            "Insert Numbers",
        );
    }
}
