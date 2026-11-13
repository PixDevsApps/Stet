//! Self-test commands for the current editor: typing, the caret, the view's settings, and
//! its text, selection and tab stops.

use super::registry::{Command, commands};
use super::script::boolean;
use super::{Harness, StepResult, equal, number};
use crate::convert::to_u32;
use crate::editor::tabstops;
use gtk4 as gtk;
use gtk4::prelude::*;
use sourceview5::prelude::*;

pub(super) const COMMANDS: &[Command] = commands![
    "type" (1, 1) => type_text,
    "cursor" (2, 2) => cursor,
    "set-tab-width" (1, 1) => set_tab_width,
    "toggle-overwrite" (0, 0) => toggle_overwrite,
    "assert-text" (1, 1) => assert_text,
    "assert-text-contains" (1, 1) => assert_text_contains,
    "assert-selection" (1, 1) => assert_selection,
    "assert-cursor" (2, 2) => assert_cursor,
    "assert-wrap" (1, 1) => assert_wrap,
    "assert-undo-levels" (1, 1) => assert_undo_levels,
    "assert-tab-grid" (0, 0) => assert_tab_grid,
];

impl Harness {
    /// `type <text>`: inserts the text at the caret of the current editor, as typing does
    /// (`insert_interactive_at_cursor`), with the focus in it.
    fn type_text(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        let view = page.view();
        view.grab_focus();
        page.buffer()
            .insert_interactive_at_cursor(&args[0], view.is_editable());
        Ok(())
    }

    /// `cursor <line> <column>`: moves the caret (1-based).
    fn cursor(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        let (line, column) = (number(&args[0])?, number(&args[1])?);
        page.go_to(stet_domain::text::Position::new(line - 1, column - 1));
        Ok(())
    }

    /// `set-tab-width <width>`: sets the current view's tab width.
    fn set_tab_width(&self, args: &[String]) -> StepResult {
        self.page()?.view().set_tab_width(to_u32(number(&args[0])?));
        Ok(())
    }

    /// `toggle-overwrite`: what the Insert key does in GtkTextView.
    fn toggle_overwrite(&self, _: &[String]) -> StepResult {
        self.page()?.view().emit_toggle_overwrite();
        Ok(())
    }

    /// `assert-text <text>`: the current tab's whole text.
    fn assert_text(&self, args: &[String]) -> StepResult {
        equal("text", self.page()?.text(), args[0].clone())
    }

    /// `assert-text-contains <text>`: the current tab's text contains the text.
    fn assert_text_contains(&self, args: &[String]) -> StepResult {
        let text = self.page()?.text();
        if text.contains(args[0].as_str()) {
            Ok(())
        } else {
            Err(format!("{:?} does not contain {:?}", text, args[0]))
        }
    }

    /// `assert-selection <text>`: the selected text of the current tab.
    fn assert_selection(&self, args: &[String]) -> StepResult {
        let buffer = self.page()?.buffer();
        let selected = buffer
            .selection_bounds()
            .map(|(start, end)| buffer.text(&start, &end, true).to_string())
            .unwrap_or_default();
        equal("selection", selected, args[0].clone())
    }

    /// `assert-cursor <line> <column>`: the caret of the current tab (1-based).
    fn assert_cursor(&self, args: &[String]) -> StepResult {
        let cursor = self.page()?.cursor();
        equal(
            "cursor (line, column)",
            (cursor.line + 1, cursor.column + 1),
            (number(&args[0])?, number(&args[1])?),
        )
    }

    /// `assert-wrap true|false`: whether the current view wraps lines.
    fn assert_wrap(&self, args: &[String]) -> StepResult {
        equal(
            "word wrap",
            self.page()?.view().wrap_mode() != gtk::WrapMode::None,
            boolean(&args[0])?,
        )
    }

    /// `assert-undo-levels <n>`: the current buffer's undo limit.
    fn assert_undo_levels(&self, args: &[String]) -> StepResult {
        equal(
            "max undo levels",
            self.page()?.buffer().max_undo_levels() as usize,
            number(&args[0])?,
        )
    }

    /// `assert-tab-grid`: after a tab, the next character's `iter_location` x lies on the
    /// character grid (whole columns of the font's space advance), within rounding to whole
    /// pixels (ADR-015).
    fn assert_tab_grid(&self, _: &[String]) -> StepResult {
        let page = self.page()?;
        let view = page.view();
        let buffer = page.buffer();
        let advance = f64::from(tabstops::space_advance(view)) / f64::from(gtk::pango::SCALE);
        let tab_width = view.tab_width() as usize;
        let expected_stop = tabstops::stop_for(view);
        let stop = tabstops::current_stop(view);
        if stop != Some(expected_stop) {
            return Err(format!(
                "tab stop {stop:?} Pango units, expected {expected_stop} ({tab_width} × {advance:.3} px)"
            ));
        }
        let mut worst = 0.0f64;
        let mut checked = 0;
        for line in 0..buffer.line_count() {
            let Some(mut iter) = buffer.iter_at_line(line) else {
                continue;
            };
            let origin = f64::from(view.iter_location(&iter).x());
            let mut column = 0usize;
            while !iter.ends_line() {
                let character = iter.char();
                if !iter.forward_char() {
                    break;
                }
                column = if character == '\t' {
                    (column / tab_width + 1) * tab_width
                } else {
                    column + 1
                };
                let x = f64::from(view.iter_location(&iter).x());
                let expected = origin + column as f64 * advance;
                worst = worst.max((x - expected).abs());
                checked += 1;
            }
        }
        if checked == 0 {
            return Err("the buffer has no characters to check".to_owned());
        }
        if worst > 0.51 {
            return Err(format!(
                "off the grid by up to {worst:.2} px (advance {advance:.3} px, {checked} positions)"
            ));
        }
        println!(
            "  # tab grid: {checked} positions within {worst:.2} px; advance {advance:.3} px, \
             stop {expected_stop} Pango units, tab width {tab_width}"
        );
        Ok(())
    }
}
