//! Self-test commands for column mode: rectangles, keys dispatched in GTK's order, typing
//! the way GtkTextView commits text, Alt+drag through the gesture's handlers, the rectangular
//! clipboard, the Column Editor, undo steps and timings, and the accessibility override.

use super::registry::{Command, commands};
use super::script::boolean;
use super::{Harness, StepResult, WAIT_TIMEOUT, build, equal, number};
use crate::convert::{BufferPositions, IterPosition, to_tab_width};
use crate::editor::column::{StetView, dialog, keys};
use crate::sys::accessible_text;
use gtk4 as gtk;
use gtk4::glib::translate::IntoGlib;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use libadwaita::prelude::AdwDialogExt;
use sourceview5::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use stet_domain::actions::ActionId;
use stet_domain::column::{Rect, VisualPos};
use stet_domain::text::{LineNumber, VisualColumn};

pub(super) const COMMANDS: &[Command] = commands![
    "column-select" (4, 4) => column_select,
    "column-key" (1, 1) => async column_key,
    "column-type" (1, 1) => column_type,
    "column-commit" (1, 1) => column_commit,
    "column-drag" (4, 5) => column_drag,
    "column-click" (2, 2) => column_click,
    "column-checkpoint" (1, 1) => column_checkpoint,
    "clipboard-text" (1, 1) => clipboard_text,
    "column-editor" (1, 7) => async column_editor,
    "bench-undo-redo" (2, 2) => async bench_undo_redo,
    "add-source-mark" (1, 1) => add_source_mark,
    "write-column-lines" (2, 2) => write_column_lines,
    "assert-column" (1, 4) => assert_column,
    "assert-burst" (1, 1) => assert_burst,
    "assert-undo-steps" (2, 2) => assert_undo_steps,
    "assert-clipboard-mime" (2, 2) => async assert_clipboard_mime,
    "assert-column-op" (1, 2) => assert_column_op,
    "assert-column-painted" (1, 1) => async assert_column_painted,
    "assert-column-editor" (1, 1) => assert_column_editor,
    "assert-a11y-selection" (0, 0) => assert_a11y_selection,
    "assert-key-route" (2, 2) => assert_key_route,
    "assert-controller-order" (0, 0) => assert_controller_order,
    "assert-visible-line" (1, 1) => assert_visible_line,
    "assert-hit-test" (1, 1) => assert_hit_test,
    "assert-source-marks" (1, 1) => assert_source_marks,
];

/// The category of the source marks the self-test makes.
const MARK_CATEGORY: &str = "stet-selftest";

thread_local! {
    static CHECKPOINTS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn ms(duration: Duration) -> String {
    format!("{:.1} ms", duration.as_secs_f64() * 1e3)
}

/// A line and visual column from the script, both counted from 1.
fn pos(line: &str, column: &str) -> Result<VisualPos, String> {
    let (line, column) = (number(line)?, number(column)?);
    match (LineNumber::new(line), column.checked_sub(1)) {
        (Some(line), Some(column)) => Ok(VisualPos::new(line.index(), VisualColumn::new(column))),
        _ => Err("lines and columns count from 1".to_owned()),
    }
}

/// `pos` as the script writes it: line and visual column counted from 1.
fn shown(pos: VisualPos) -> String {
    format!("{}:{}", pos.line.number(), pos.column.get() + 1)
}

/// What GtkTextView does with committed text (`gtk_text_view_commit_text`): the selection
/// goes, overwrite mode deletes the character after the caret, and the text goes in at the
/// caret, in one user action.
fn commit(view: &sourceview5::View, text: &str) {
    let buffer = view.buffer();
    let editable = view.is_editable();
    buffer.begin_user_action();
    let had_selection = buffer.has_selection();
    buffer.delete_selection(true, editable);
    if text != "\n" && !had_selection && view.overwrites() {
        let mut start = buffer.iter_at_mark(&buffer.get_insert());
        if !start.ends_line() {
            let mut end = start;
            end.forward_cursor_position();
            buffer.delete_interactive(&mut start, &mut end, editable);
        }
    }
    buffer.insert_interactive_at_cursor(text, editable);
    buffer.end_user_action();
    view.scroll_mark_onscreen(&buffer.get_insert());
}

/// The registry action an application accelerator runs for a key, if any.
fn app_action(key: gdk::Key, mods: gdk::ModifierType) -> Option<ActionId> {
    ActionId::ALL.into_iter().find(|id| {
        id.default_accels()
            .iter()
            .filter_map(|accel| gtk::accelerator_parse(*accel))
            .any(|(keyval, modifiers)| keyval == key && modifiers == mods)
    })
}

/// The widget's own shortcut (a class binding) for `accel`, if any.
fn binding(view: &StetView, accel: &str) -> Option<gtk::Shortcut> {
    fn matches(trigger: gtk::ShortcutTrigger, wanted: &gtk::ShortcutTrigger) -> bool {
        match trigger.downcast::<gtk::AlternativeTrigger>() {
            Ok(alternative) => {
                matches(alternative.first(), wanted) || matches(alternative.second(), wanted)
            }
            Err(trigger) => trigger.equal(wanted),
        }
    }
    let wanted = gtk::ShortcutTrigger::parse_string(accel)?;
    let controllers = view.observe_controllers();
    (0..controllers.n_items())
        .filter_map(|index| {
            controllers
                .item(index)
                .and_downcast::<gtk::ShortcutController>()
        })
        .flat_map(|controller| {
            (0..controller.n_items())
                .filter_map(|position| controller.item(position).and_downcast::<gtk::Shortcut>())
                .collect::<Vec<_>>()
        })
        .find(|shortcut| {
            shortcut
                .trigger()
                .is_some_and(|trigger| matches(trigger, &wanted))
        })
}

fn describe(shortcut: &gtk::Shortcut) -> String {
    shortcut.action().map_or_else(
        || "nothing".to_owned(),
        |action| action.to_str().to_string(),
    )
}

impl Harness {
    fn column_view(&self) -> Result<StetView, String> {
        Ok(self.page()?.column_view().clone())
    }

    /// `column-select <line> <column> <line> <column>`: column mode with that rectangle,
    /// anchor first, cursor corner second (1-based; columns past a line's end are virtual
    /// space), with the focus in the editor.
    fn column_select(&self, args: &[String]) -> StepResult {
        let anchor = pos(&args[0], &args[1])?;
        let cursor = pos(&args[2], &args[3])?;
        let view = self.column_view()?;
        view.grab_focus();
        view.set_rect(Rect::new(anchor, cursor));
        Ok(())
    }

    /// `column-type <text>`: commits the text one character at a time, as typing does.
    fn column_type(&self, args: &[String]) -> StepResult {
        let view = self.column_view()?;
        view.grab_focus();
        for ch in args[0].chars() {
            commit(view.upcast_ref(), &ch.to_string());
        }
        Ok(())
    }

    /// `column-commit <text>`: commits the text all at once, as an input method does.
    fn column_commit(&self, args: &[String]) -> StepResult {
        let view = self.column_view()?;
        view.grab_focus();
        commit(view.upcast_ref(), &args[0]);
        Ok(())
    }

    /// `column-checkpoint <name>`: remembers the current tab's text, for `assert-undo-steps`
    /// and `bench-undo-redo`.
    fn column_checkpoint(&self, args: &[String]) -> StepResult {
        let text = self.page()?.text();
        CHECKPOINTS.with(|checkpoints| {
            checkpoints.borrow_mut().insert(args[0].clone(), text);
        });
        Ok(())
    }

    /// `clipboard-text <text>`: puts plain text on the clipboard.
    fn clipboard_text(&self, args: &[String]) {
        self.window.window().clipboard().set_text(&args[0]);
    }

    /// `assert-clipboard-mime <mime type> <text>`: the clipboard's content in that type.
    async fn assert_clipboard_mime(&self, args: &[String]) -> StepResult {
        let clipboard = self.window.window().clipboard();
        match crate::editor::column::clipboard::read(&clipboard, &args[0]).await {
            Ok(text) => equal(&format!("clipboard as {}", args[0]), text, args[1].clone()),
            Err(error) => Err(format!("the clipboard has no {}: {error}", args[0])),
        }
    }

    /// `write-column-lines <path> <count>`: writes that many lines of spike S5's shapes.
    fn write_column_lines(&self, args: &[String]) -> StepResult {
        let count = number(&args[1])?;
        super::write_file(std::path::Path::new(&args[0]), s5_lines(count).as_bytes())
    }

    /// `add-source-mark <line>`: adds a source mark of the self-test's category on the line.
    fn add_source_mark(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        let buffer = page.buffer();
        let line = number(&args[0])?;
        let iter = LineNumber::new(line)
            .and_then(|number| buffer.iter_at_line_start(number.index()))
            .ok_or_else(|| format!("there is no line {line}"))?;
        buffer.create_source_mark(None, MARK_CATEGORY, &iter);
        Ok(())
    }

    /// `assert-column-painted <n>|<<n>`: column mode painted n rows, or fewer than n but some
    /// (only the visible rows of a tall rectangle are painted), once two frames were painted.
    async fn assert_column_painted(&self, args: &[String]) -> StepResult {
        self.wait_frames(2).await?;
        let painted = self.column_view()?.painted_rows();
        match args[0].strip_prefix('<') {
            Some(limit) if painted > 0 && painted < number(limit)? => {
                println!("  # {painted} rows painted");
                Ok(())
            }
            Some(limit) => Err(format!("{painted} rows painted, not 1 to {limit}")),
            None => equal("painted rows", painted, number(&args[0])?),
        }
    }

    /// `assert-burst true|false`: whether a typing burst is open.
    fn assert_burst(&self, args: &[String]) -> StepResult {
        let view = self.column_view()?;
        equal("typing burst open", view.burst_open(), boolean(&args[0])?)
    }

    /// `assert-hit-test <line>`: every cell of the line, and a few past its end, maps to a
    /// point of the widget that maps back to the same cell (what Alt+drag relies on).
    fn assert_hit_test(&self, args: &[String]) -> StepResult {
        let view = self.column_view()?;
        let line = LineNumber::new(number(&args[0])?).ok_or("lines count from 1")?;
        let width = stet_domain::column::text_width(
            &view.line_text(line.index()),
            VisualColumn::ZERO,
            to_tab_width(view.tab_width()),
        );
        let wrong: Vec<String> = (0..width + 4)
            .filter_map(|column| {
                let pos = VisualPos::new(line.index(), VisualColumn::new(column));
                let (x, y) = view.point_of(pos);
                let back = view.pos_at(x, y);
                (back != pos).then(|| format!("{column}→{}", back.column.get()))
            })
            .collect();
        if wrong.is_empty() {
            println!("  # line {line}: {} cells map back", width + 4);
            Ok(())
        } else {
            Err(format!(
                "cells that map back elsewhere: {}",
                wrong.join(", ")
            ))
        }
    }

    /// `assert-visible-line <line>`: the line is on screen in the current view.
    fn assert_visible_line(&self, args: &[String]) -> StepResult {
        let view = self.column_view()?;
        let line = number(&args[0])?;
        let visible = view.visible_rect();
        let top = view.line_at_y(visible.y()).0.display_line();
        let bottom = view
            .line_at_y(visible.y() + visible.height())
            .0
            .display_line();
        if LineNumber::new(line).is_some_and(|line| (top..=bottom).contains(&line)) {
            Ok(())
        } else {
            Err(format!("lines {top} to {bottom} are visible, not {line}"))
        }
    }

    /// `assert-source-marks <lines>`: the lines with the self-test's source marks, joined by
    /// commas.
    fn assert_source_marks(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        let buffer = page.buffer();
        let lines: Vec<String> = (0..buffer.line_count())
            .filter(|line| {
                !buffer
                    .source_marks_at_line(*line, Some(MARK_CATEGORY))
                    .is_empty()
            })
            .map(|line| (line + 1).to_string())
            .collect();
        equal("lines with marks", lines.join(","), args[0].clone())
    }

    /// `column-key <key>`: sends the key the way GTK dispatches it to the editor: the window's
    /// capture-phase controller (which closes a typing burst), the application accelerators,
    /// column mode's key controller on the view, then the view's own bindings; Enter and text
    /// keys are committed.
    async fn column_key(&self, args: &[String]) -> StepResult {
        let accel = args[0].as_str();
        let (key, mods) =
            gtk::accelerator_parse(accel).ok_or_else(|| format!("bad key {accel}"))?;
        let view = self.column_view()?;
        if keys::closes_burst(key, mods) {
            keys::close_open_burst();
        }
        if let Some(id) = app_action(key, mods) {
            self.activate(id.name(), None)?;
            self.wait_until_settled().await;
            return Ok(());
        }
        let values: [&dyn ToValue; 3] = [&key.into_glib(), &0u32, &mods];
        if view
            .column_keys()
            .emit_by_name::<bool>("key-pressed", &values)
        {
            return Ok(());
        }
        if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter) {
            commit(view.upcast_ref(), "\n");
            return Ok(());
        }
        if let Some(shortcut) = binding(&view, accel) {
            let action = shortcut
                .action()
                .ok_or_else(|| format!("{accel}'s binding has no action"))?;
            action.activate(
                gtk::ShortcutActionFlags::EXCLUSIVE,
                &view,
                shortcut.arguments().as_ref(),
            );
            return Ok(());
        }
        // GtkTextView has no use for Escape outside column mode.
        if key == gdk::Key::Escape {
            return Ok(());
        }
        let typed = key.to_unicode().filter(|ch| !ch.is_control());
        match typed {
            Some(ch)
                if !mods
                    .intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK) =>
            {
                commit(view.upcast_ref(), &ch.to_string());
                Ok(())
            }
            _ => Err(format!("{accel} reaches nothing in the editor")),
        }
    }

    /// `column-drag <line> <column> <line> <column> [shift]`: an Alt+drag (Alt+Shift with
    /// `shift`; the same cell twice is an Alt+click) from one cell to another, through the drag
    /// gesture's handlers.
    fn column_drag(&self, args: &[String]) -> StepResult {
        let view = self.column_view()?;
        let from = pos(&args[0], &args[1])?;
        let to = pos(&args[2], &args[3])?;
        let shift = match args.get(4).map(String::as_str) {
            None => false,
            Some("shift") => true,
            Some(other) => return Err(format!("{other}: use shift or nothing")),
        };
        let (x, y) = view.point_of(from);
        if !view.pointer_begin(x, y, true, shift) {
            return Err("the drag was not taken".to_owned());
        }
        if from != to {
            let (x, y) = view.point_of(to);
            view.pointer_update(x, y);
        }
        view.pointer_end();
        Ok(())
    }

    /// `column-click <line> <column>`: a plain click on a cell: column mode lets it go, then
    /// GTK puts the caret there.
    fn column_click(&self, args: &[String]) -> StepResult {
        let view = self.column_view()?;
        let at = pos(&args[0], &args[1])?;
        let (x, y) = view.point_of(at);
        if view.pointer_begin(x, y, false, false) {
            return Err("a plain click was taken by column mode".to_owned());
        }
        let iter = view.iter_at_pos(at);
        view.buffer().place_cursor(&iter);
        view.grab_focus();
        Ok(())
    }

    /// `assert-column none` or `assert-column <line> <column> <line> <column>`: column mode is
    /// off, or on with that rectangle (anchor, then cursor corner); GTK's caret shows only
    /// when it is off.
    fn assert_column(&self, args: &[String]) -> StepResult {
        let view = self.column_view()?;
        let rect = view.rect();
        let cursor_shown = view.is_cursor_visible();
        if rect.is_some() == cursor_shown {
            return Err(format!(
                "GTK's caret is {} while column mode is {}",
                if cursor_shown { "shown" } else { "hidden" },
                if rect.is_some() { "on" } else { "off" }
            ));
        }
        match args {
            [none] if none == "none" => match rect {
                None => Ok(()),
                Some(rect) => Err(format!("column mode is on: {rect:?}")),
            },
            [l1, c1, l2, c2] => {
                let wanted = Rect::new(pos(l1, c1)?, pos(l2, c2)?);
                match rect {
                    Some(rect) if rect == wanted => Ok(()),
                    Some(rect) => Err(format!(
                        "the rectangle is {} to {}",
                        shown(rect.anchor),
                        shown(rect.cursor)
                    )),
                    None => Err("column mode is off".to_owned()),
                }
            }
            _ => Err(
                "use assert-column none, or anchor line and column, cursor line and column"
                    .to_owned(),
            ),
        }
    }

    /// `assert-undo-steps <checkpoint> <n>`: undoes until the text is the checkpoint's,
    /// expecting exactly n steps, then redoes them, expecting the text from before.
    fn assert_undo_steps(&self, args: &[String]) -> StepResult {
        let (checkpoint, steps) = (args[0].as_str(), args[1].as_str());
        let page = self.page()?;
        let buffer = page.buffer();
        let wanted = number(steps)?;
        let target = CHECKPOINTS
            .with(|checkpoints| checkpoints.borrow().get(checkpoint).cloned())
            .ok_or_else(|| format!("no checkpoint {checkpoint}"))?;
        let before = page.text();
        let mut taken = 0;
        while page.text() != target && taken < wanted + 5 && buffer.can_undo() {
            buffer.undo();
            taken += 1;
        }
        let reached = page.text() == target;
        for _ in 0..taken {
            if buffer.can_redo() {
                buffer.redo();
            }
        }
        let restored = page.text() == before;
        if !reached {
            return Err(format!(
                "{taken} undo steps did not reach {checkpoint} (can undo: {})",
                buffer.can_undo()
            ));
        }
        if !restored {
            return Err(format!("redoing {taken} steps did not restore the text"));
        }
        equal("undo steps", taken, wanted)
    }

    /// `assert-column-op <limit ms> [bulk|single]`: the last column operation took no longer,
    /// and was one bulk edit or not.
    fn assert_column_op(&self, args: &[String]) -> StepResult {
        let view = self.column_view()?;
        let stats = view
            .last_op()
            .ok_or_else(|| "no column operation ran".to_owned())?;
        println!(
            "  # column operation: {} lines, {} edits{}, {} in all, {} changing the buffer ({} build)",
            stats.lines,
            stats.edits,
            if stats.bulk { " as one bulk edit" } else { "" },
            ms(stats.took),
            ms(stats.applying),
            build()
        );
        if let Some(kind) = args.get(1) {
            let bulk = match kind.as_str() {
                "bulk" => true,
                "single" => false,
                other => return Err(format!("{other}: use bulk or single")),
            };
            equal("bulk edit", stats.bulk, bulk)?;
        }
        let limit = Duration::from_millis(number(&args[0])? as u64);
        if stats.took <= limit {
            Ok(())
        } else {
            Err(format!("it took {}, over {}", ms(stats.took), ms(limit)))
        }
    }

    /// `bench-undo-redo <checkpoint> <limit ms>`: one undo and one redo, each timed and held to
    /// the limit; the undo must give the checkpoint's text and the redo the text from before.
    async fn bench_undo_redo(&self, args: &[String]) -> StepResult {
        let (checkpoint, limit) = (args[0].as_str(), args[1].as_str());
        let page = self.page()?;
        let buffer = page.buffer();
        let limit = Duration::from_millis(number(limit)? as u64);
        let target = CHECKPOINTS
            .with(|checkpoints| checkpoints.borrow().get(checkpoint).cloned())
            .ok_or_else(|| format!("no checkpoint {checkpoint}"))?;
        if !buffer.can_undo() {
            return Err("nothing to undo (is a typing burst open?)".to_owned());
        }
        let before = page.text();
        let started = Instant::now();
        buffer.undo();
        let undo = started.elapsed();
        let undone = page.text() == target;
        let idle = idle_after(started).await;
        let started = Instant::now();
        buffer.redo();
        let redo = started.elapsed();
        let redone = page.text() == before;
        let redo_idle = idle_after(started).await;
        println!(
            "  # undo {} (idle after {}), redo {} (idle after {}) ({} build)",
            ms(undo),
            ms(idle),
            ms(redo),
            ms(redo_idle),
            build()
        );
        if !undone {
            return Err(format!("one undo did not give {checkpoint}"));
        }
        if !redone {
            return Err("redo did not give the text back".to_owned());
        }
        if undo > limit || redo > limit {
            return Err(format!("over {}", ms(limit)));
        }
        Ok(())
    }

    /// `column-editor open`, `text <text>`, `numbers <initial> <step> <repeat> <leading>
    /// <format> [<uppercase>]`, `ok`, `enter` or `cancel`: opens, fills and answers the Column
    /// Editor (`enter` activates the focused field).
    async fn column_editor(&self, args: &[String]) -> StepResult {
        let verb = args[0].as_str();
        if verb == "open" {
            self.activate(ActionId::ColumnEditor.name(), None)?;
            let started = Instant::now();
            while dialog::current().is_none() {
                if started.elapsed() > WAIT_TIMEOUT {
                    return Err("the Column Editor did not open".to_owned());
                }
                glib::timeout_future(Duration::from_millis(5)).await;
            }
            self.wait_until_settled().await;
            return Ok(());
        }
        let editor = dialog::current().ok_or_else(|| "the Column Editor is not open".to_owned())?;
        // A user acts on the dialog once it is on screen; its buttons ignore activation before.
        let started = Instant::now();
        while !editor.ok.is_mapped() {
            if started.elapsed() > WAIT_TIMEOUT {
                return Err("the Column Editor never appeared".to_owned());
            }
            glib::timeout_future(Duration::from_millis(5)).await;
        }
        match (verb, &args[1..]) {
            ("text", [text]) => {
                editor.text_mode.set_active(true);
                editor.text.set_text(text);
            }
            ("numbers", [initial, step, repeat, leading, format, rest @ ..]) if rest.len() <= 1 => {
                editor.number_mode.set_active(true);
                editor.initial.set_text(initial);
                editor.step.set_text(step);
                editor.repeat.set_text(repeat);
                if !editor.choose_leading(leading) {
                    return Err(format!("no leading {leading}"));
                }
                if !editor.choose_base(format) {
                    return Err(format!("no format {format}"));
                }
                if let Some(uppercase) = rest.first() {
                    editor.uppercase.set_active(boolean(uppercase)?);
                }
            }
            ("ok", []) => {
                editor.ok.emit_clicked();
                self.wait_until_settled().await;
            }
            // Enter in the focused field: its text widget activates the dialog's default
            // button, OK, which clicks itself 250 ms later (GtkButton's activation).
            ("enter", []) => {
                let field = if editor.number_mode.is_active() {
                    &editor.initial
                } else {
                    &editor.text
                };
                let text = field
                    .delegate()
                    .and_downcast::<gtk::Text>()
                    .ok_or("the field has no text widget")?;
                text.emit_activate();
                self.wait_until_settled().await;
            }
            ("cancel", []) => {
                editor.dialog.close();
                self.wait_until_settled().await;
            }
            _ => {
                return Err(
                    "use column-editor open, text <text>, numbers <initial> <step> <repeat> \
                     <leading> <format> [uppercase], ok, enter or cancel"
                        .to_owned(),
                );
            }
        }
        Ok(())
    }

    /// `assert-a11y-selection`: the editor's GtkAccessibleText `get_selection` with a
    /// selection present: called with NULL ranges as GTK's AT-SPI AddSelection does (which
    /// crashes GTK 4.22's own), then with storage, which must give the selection.
    fn assert_a11y_selection(&self, _: &[String]) -> StepResult {
        let view = self.column_view()?;
        let buffer = view.buffer();
        let (start, end) = buffer
            .selection_bounds()
            .ok_or_else(|| "select something first".to_owned())?;
        let (selected, count, _, ours) = accessible_text::probe_selection(&view, false);
        equal("our get_selection in the vtable", ours, true)?;
        equal(
            "selection found (NULL ranges)",
            (selected, count),
            (true, 1),
        )?;
        let (selected, count, ranges, _) = accessible_text::probe_selection(&view, true);
        let (start, end) = (start.char_offset(), end.char_offset());
        let wanted = vec![(start.get(), end.max(start) - start)];
        equal(
            "selection (with ranges)",
            (selected, count, ranges),
            (true, 1, wanted),
        )?;
        println!("  # get_selection with NULL ranges: TRUE, 1 range, no crash");
        Ok(())
    }

    /// `assert-key-route <key> <route>`: which handler the key reaches first: `app:<action>`
    /// for an application accelerator, else `binding:<action>` for the editor's own binding,
    /// or `none`.
    fn assert_key_route(&self, args: &[String]) -> StepResult {
        let (accel, wanted) = (args[0].as_str(), args[1].as_str());
        let (key, mods) =
            gtk::accelerator_parse(accel).ok_or_else(|| format!("bad key {accel}"))?;
        let view = self.column_view()?;
        let own = binding(&view, accel);
        let route = match app_action(key, mods) {
            Some(id) => format!("app:{}", id.name()),
            None => own.as_ref().map_or_else(
                || "none".to_owned(),
                |shortcut| format!("binding:{}", describe(shortcut)),
            ),
        };
        if let Some(shortcut) = &own
            && route.starts_with("app:")
        {
            println!(
                "  # {accel}: {route} runs in the window's capture phase, before the editor's \
                 own binding {}",
                describe(shortcut)
            );
        }
        equal("route", route, wanted.to_owned())
    }
}

impl Harness {
    /// `assert-controller-order`: column mode's controllers run first where they must: on the
    /// window, the burst closer before the application accelerators; on the view, the column
    /// keys before GtkSourceView's own capture-phase key handler (which feeds the input
    /// method). GTK runs a widget's controllers of one phase in list order, newest first.
    fn assert_controller_order(&self, _: &[String]) -> StepResult {
        fn listed(widget: &gtk::Widget) -> Vec<gtk::EventController> {
            let controllers = widget.observe_controllers();
            (0..controllers.n_items())
                .filter_map(|index| {
                    controllers
                        .item(index)
                        .and_downcast::<gtk::EventController>()
                })
                .collect()
        }
        let named = |list: &[gtk::EventController], name: &str| {
            list.iter()
                .position(|controller| controller.name().as_deref() == Some(name))
        };
        let window = listed(self.window.window().upcast_ref());
        let closer = named(&window, keys::WINDOW_EVENTS)
            .ok_or("the window has no burst-closing controller")?;
        let accels = named(&window, "gtk-application-shortcuts")
            .ok_or("the window has no application accelerators")?;
        if closer > accels {
            return Err(format!(
                "the burst closer is controller {closer}, after the accelerators at {accels}"
            ));
        }
        let view = self.column_view()?;
        let own = listed(view.upcast_ref());
        let keys_at = named(&own, keys::VIEW_KEYS).ok_or("the view has no column keys")?;
        let others: Vec<usize> = own
            .iter()
            .enumerate()
            .filter(|(index, controller)| {
                *index != keys_at
                    && controller.is::<gtk::EventControllerKey>()
                    && controller.propagation_phase() == gtk::PropagationPhase::Capture
            })
            .map(|(index, _)| index)
            .collect();
        if others.is_empty() {
            return Err("GtkSourceView's capture-phase key controller is missing".to_owned());
        }
        if others.iter().any(|other| *other < keys_at) {
            return Err(format!(
                "column keys at {keys_at}, another capture-phase key controller before it: {others:?}"
            ));
        }
        println!(
            "  # window: burst closer {closer}, accelerators {accels}; view: column keys {keys_at}, \
             GtkSourceView's key handler {others:?}"
        );
        Ok(())
    }
}

/// `count` lines of spike S5's seven shapes (empty, a leading tab, short, a tab across column
/// 10, accented letters and a dash, 107 characters, indented code), each ending with a line
/// break.
fn s5_lines(count: usize) -> String {
    let mut text = String::new();
    for i in 0..count {
        let line = match i % 7 {
            0 => String::new(),
            1 => format!("\tfn item_{i}() {{}}"),
            2 => "short".to_owned(),
            3 => format!("abcdefgh\t= {i};"),
            4 => format!("héllo wörld — ünïcode {i}"),
            5 => format!("{i:>6} {}", "x".repeat(100)),
            _ => format!("    let value_{i} = {i};"),
        };
        text.push_str(&line);
        text.push('\n');
    }
    text
}

/// Waits until the main loop is idle at low priority; returns the time since `started`.
async fn idle_after(started: Instant) -> Duration {
    let (sender, receiver) = async_channel::bounded(1);
    glib::idle_add_local_full(glib::Priority::LOW, move || {
        let _ = sender.try_send(());
        glib::ControlFlow::Break
    });
    let _ = receiver.recv().await;
    started.elapsed()
}

impl Harness {
    /// `assert-column-editor <state>`: the Column Editor is `open`, `closed` or `focused` (the
    /// keyboard focus is in it), shows `error:none` or `error:<text>`, or has
    /// `<field>=<value>` for `text`, `initial`, `step` or `repeat`.
    fn assert_column_editor(&self, args: &[String]) -> StepResult {
        let wanted = args[0].as_str();
        let editor = dialog::current();
        match wanted {
            "open" => return equal("Column Editor open", editor.is_some(), true),
            "closed" => return equal("Column Editor open", editor.is_some(), false),
            _ => {}
        }
        let editor = editor.ok_or_else(|| "the Column Editor is not open".to_owned())?;
        if wanted == "focused" {
            let focus = RootExt::focus(self.window.window());
            let inside = focus
                .as_ref()
                .is_some_and(|focus| focus.is_ancestor(&editor.dialog) && focus.is_mapped());
            return if inside {
                Ok(())
            } else {
                Err(format!(
                    "the focus is on {:?}",
                    focus.map(|focus| focus.type_())
                ))
            };
        }
        if let Some(text) = wanted.strip_prefix("error:") {
            let shown = editor.error_text();
            return match (text, shown) {
                ("none", None) => Ok(()),
                ("none", Some(shown)) => Err(format!("the Column Editor says {shown:?}")),
                (text, Some(shown)) if shown.contains(text) => Ok(()),
                (text, shown) => Err(format!("the Column Editor says {shown:?}, not {text:?}")),
            };
        }
        let (field, value) = wanted
            .split_once('=')
            .ok_or_else(|| "use open, closed, focused, error:… or field=value".to_owned())?;
        let entry = match field {
            "text" => &editor.text,
            "initial" => &editor.initial,
            "step" => &editor.step,
            "repeat" => &editor.repeat,
            other => return Err(format!("no field {other}")),
        };
        equal(field, entry.text().to_string(), value.to_owned())
    }
}
