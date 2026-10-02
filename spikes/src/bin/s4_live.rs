//! Spike S4: checks that need the live Hyprland session. Typing through fcitx5, also in a
//! five-line column caret; which shortcuts reach the app; drag-and-drop from Nautilus; the file
//! dialog; Omarchy's universal clipboard keys; and latency from real key presses to the painted
//! frame.
//!
//! Interactive: `STET_SPIKE_ALLOW_LIVE=1 cargo run --release -p stet-spikes --bin s4_live`.
//! `-- --auto` runs the unattended subset: the file dialog check, then selects the first line and
//! prints `READY_CLIPBOARD` so a driver can send SUPER+C and SUPER+V. Results are printed as JSON
//! and written to `target/s4-live.json` (or `$STET_S4_OUT`) on Finish or close.

use anyhow::Result;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use serde_json::{Value, json};
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};
use stet_spikes::{Report, require_headless};

const APP_ID: &str = "io.github.pixdevsapps.Stet.S4";
const DIALOG_TITLE: &str = "Stet S4 file dialog check";
const COLUMN_LINES: i32 = 5;
const KEY_PAINT_WINDOW: Duration = Duration::from_millis(150);
const TARGET_KEYS: [&str; 13] = [
    "Ctrl+Shift+U",
    "Ctrl+Alt+Shift+U",
    "Ctrl+Space",
    "Ctrl+Enter",
    "Alt+F3",
    "Alt+Shift+F3",
    "F1",
    "Ctrl+Q",
    "Ctrl+D",
    "Ctrl+Shift+Up",
    "Alt+Shift+Down",
    "Ctrl+Tab",
    "F11",
];
const SAMPLE: &str = "SUPER+C test line: copy me\n\
fn main() {\n\
    let greeting = \"hello\";\n\
    let target = \"world\";\n\
    let count = 42;\n\
    let ratio = 0.5;\n\
    println!(\"{greeting}, {target}: {count} {ratio}\");\n\
}\n\
\n\
Column caret: put the caret at the start of a line above, click \"Column caret\" and type.\n";
const INSTRUCTIONS: &str = "1. Type in the editor, including a compose or dead-key character if you use one.\n\
2. Press each shortcut in the list once; it gets a tick when it reaches the app.\n\
3. Put the caret at the start of a line, click Column caret, type a word: it should appear on 5 lines.\n\
4. Select text, press SUPER+C; click elsewhere in the editor, press SUPER+V.\n\
5. Drag a file from Nautilus onto the drop box.\n\
6. Click Open file dialog, note whether it floats, then cancel it.\n\
7. Click Finish.";
const FOLLOW_UP: &str = "Follow-up (about a minute):\n\
1. Press F1, F3, Alt+F3 and F11 once each; every key you press is logged by name below.\n\
2. Click Column caret: the caret moves to line 3; type a word, it should appear on lines 3-7.\n\
3. Click at the end of a line and press SUPER+V (pastes what you copied last time).\n\
4. Drag a file from Nautilus onto the drop box.\n\
5. Click Finish.";

struct State {
    auto: bool,
    report: Report,
    log: gtk::TextBuffer,
    status: gtk::Label,
    key_labels: Vec<(&'static str, gtk::Label)>,
    keys_seen: RefCell<BTreeSet<String>>,
    key_pressed_at: Cell<Option<Instant>>,
    changed_at: Cell<Option<Instant>>,
    key_to_paint: RefCell<Vec<f64>>,
    change_to_paint: RefCell<Vec<f64>>,
    inserts: Cell<u32>,
    inserts_after_key: Cell<u32>,
    non_ascii: RefCell<Vec<String>>,
    preedits: RefCell<Vec<String>>,
    column_first_line: Cell<Option<i32>>,
    replicating: Cell<bool>,
    replications: Cell<u32>,
    clipboard: RefCell<Vec<String>>,
    drops: RefCell<Vec<String>>,
    dialog: RefCell<Value>,
    finished: Cell<bool>,
    all_keys: RefCell<Vec<String>>,
    multi_char_inserts: RefCell<Vec<String>>,
    drag_enters: RefCell<Vec<String>>,
}

impl State {
    fn log(&self, message: &str) {
        let mut end = self.log.end_iter();
        self.log.insert(&mut end, &format!("{message}\n"));
        self.report.record("log", message);
    }

    fn refresh_status(&self) {
        self.status.set_text(&format!(
            "Text inserts: {} ({} right after a key event, {} non-ASCII) · preedit updates: {} · \
             column replications: {} · clipboard changes: {} · drops: {} · dialog: {}",
            self.inserts.get(),
            self.inserts_after_key.get(),
            self.non_ascii.borrow().len(),
            self.preedits.borrow().len(),
            self.replications.get(),
            self.clipboard.borrow().len(),
            self.drops.borrow().len(),
            dialog_summary(&self.dialog.borrow()),
        ));
    }

    fn on_key(&self, keyval: gdk::Key, modifiers: gdk::ModifierType) {
        let Some(combo) = combo(keyval, modifiers) else {
            return;
        };
        let command = modifiers.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK,
        );
        if !command {
            self.key_pressed_at.set(Some(Instant::now()));
        }
        if self.all_keys.borrow().len() < 300 {
            self.all_keys.borrow_mut().push(combo.clone());
        }
        if command || keyval.to_unicode().is_none_or(char::is_control) {
            self.log(&format!("key: {combo}"));
        }
        if let Some((name, label)) = self.key_labels.iter().find(|(name, _)| *name == combo)
            && self.keys_seen.borrow_mut().insert(combo.clone())
        {
            label.set_text(&format!("✓ {name}"));
            self.log(&format!("key reached the app: {combo}"));
        }
    }

    fn on_inserted(&self, buffer: &sourceview5::Buffer, end: &gtk::TextIter, text: &str) {
        if self.replicating.get() {
            return;
        }
        self.inserts.set(self.inserts.get() + 1);
        if self
            .key_pressed_at
            .get()
            .is_some_and(|pressed| pressed.elapsed() < KEY_PAINT_WINDOW)
        {
            self.inserts_after_key.set(self.inserts_after_key.get() + 1);
        }
        let chars = text.chars().count();
        if chars > 1 && self.multi_char_inserts.borrow().len() < 20 {
            let sample: String = text.chars().take(40).collect();
            self.multi_char_inserts.borrow_mut().push(sample.clone());
            self.log(&format!(
                "multi-character insert ({chars} chars, paste or IME commit): {sample:?}"
            ));
        }
        if !text.is_ascii() && self.non_ascii.borrow().len() < 20 {
            self.non_ascii.borrow_mut().push(text.to_owned());
            self.log(&format!("non-ASCII text committed: {text:?}"));
        }
        if let Some(first) = self.column_first_line.get() {
            self.replicate(buffer, end, text, first);
        }
        self.refresh_status();
    }

    fn replicate(&self, buffer: &sourceview5::Buffer, end: &gtk::TextIter, text: &str, first: i32) {
        let start = buffer.iter_at_offset(end.offset() - text.chars().count() as i32);
        let (line, column) = (start.line(), start.line_offset());
        if text.contains('\n') || !(first..first + COLUMN_LINES).contains(&line) {
            return;
        }
        self.replicating.set(true);
        for other in (first..first + COLUMN_LINES).filter(|&other| other != line) {
            let Some(mut line_end) = buffer.iter_at_line(other) else {
                continue;
            };
            if !line_end.ends_line() {
                line_end.forward_to_line_end();
            }
            let length = line_end.line_offset();
            if length < column {
                buffer.insert(&mut line_end, &" ".repeat((column - length) as usize));
            }
            if let Some(mut at) = buffer.iter_at_line_offset(other, column) {
                buffer.insert(&mut at, text);
            }
        }
        self.replicating.set(false);
        self.replications.set(self.replications.get() + 1);
    }

    fn on_changed(&self) {
        if self.changed_at.get().is_none() {
            self.changed_at.set(Some(Instant::now()));
        }
    }

    fn on_after_paint(&self) {
        let now = Instant::now();
        if let Some(changed) = self.changed_at.take() {
            self.change_to_paint.borrow_mut().push(ms(now - changed));
            if let Some(pressed) = self.key_pressed_at.take()
                && changed >= pressed
            {
                self.key_to_paint.borrow_mut().push(ms(now - pressed));
            }
        } else if self
            .key_pressed_at
            .get()
            .is_some_and(|pressed| now - pressed > KEY_PAINT_WINDOW)
        {
            self.key_pressed_at.set(None);
        }
    }

    fn on_preedit(&self, preedit: &str) {
        if preedit.is_empty() {
            return;
        }
        let mut preedits = self.preedits.borrow_mut();
        if preedits.len() < 20 {
            preedits.push(preedit.to_owned());
        }
        drop(preedits);
        self.log(&format!("preedit: {preedit:?}"));
        self.refresh_status();
    }

    fn on_clipboard(&self, text: &str) {
        let sample: String = text.chars().take(60).collect();
        self.clipboard.borrow_mut().push(sample.clone());
        self.log(&format!("clipboard changed: {sample:?}"));
        self.refresh_status();
    }

    fn on_drop(&self, paths: Vec<String>) {
        self.log(&format!("drop received: {paths:?}"));
        self.drops.borrow_mut().extend(paths);
        self.refresh_status();
    }

    fn results(&self) -> Value {
        let seen = self.keys_seen.borrow();
        let missing: Vec<&str> = TARGET_KEYS
            .iter()
            .copied()
            .filter(|key| !seen.contains(*key))
            .collect();
        json!({
            "mode": if self.auto { "auto" } else { "interactive" },
            "environment": environment(),
            "keys_reached": seen.iter().collect::<Vec<_>>(),
            "keys_not_seen": missing,
            "text_inserts": self.inserts.get(),
            "text_inserts_right_after_a_key_event": self.inserts_after_key.get(),
            "non_ascii_commits": *self.non_ascii.borrow(),
            "all_keys": *self.all_keys.borrow(),
            "multi_char_inserts": *self.multi_char_inserts.borrow(),
            "drag_enters": *self.drag_enters.borrow(),
            "preedit_samples": *self.preedits.borrow(),
            "column_replications": self.replications.get(),
            "clipboard_changes": *self.clipboard.borrow(),
            "drops": *self.drops.borrow(),
            "file_dialog": *self.dialog.borrow(),
            "key_to_paint_ms": stats(&self.key_to_paint.borrow()),
            "change_to_paint_ms": stats(&self.change_to_paint.borrow()),
        })
    }

    fn finish(&self, app: &gtk::Application) {
        if self.finished.replace(true) {
            return;
        }
        let results = self.results();
        let text = serde_json::to_string_pretty(&results).unwrap_or_default();
        let path = std::env::var_os("STET_S4_OUT").map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/s4-live.json"),
            PathBuf::from,
        );
        match std::fs::write(&path, &text) {
            Ok(()) => println!("S4_RESULTS {}", path.display()),
            Err(error) => eprintln!("could not write {}: {error}", path.display()),
        }
        println!("{text}");
        let _ = std::io::stdout().flush();
        app.quit();
    }
}

fn combo(keyval: gdk::Key, modifiers: gdk::ModifierType) -> Option<String> {
    let name = keyval.to_upper().name()?;
    let name = match name.as_str() {
        "ISO_Left_Tab" => "Tab",
        "Return" | "KP_Enter" => "Enter",
        "space" => "Space",
        "Control_L" | "Control_R" | "Shift_L" | "Shift_R" | "Alt_L" | "Alt_R" | "Super_L"
        | "Super_R" | "Meta_L" | "Meta_R" | "ISO_Level3_Shift" | "Multi_key" => return None,
        other => other,
    };
    let mut parts = Vec::new();
    for (mask, label) in [
        (gdk::ModifierType::CONTROL_MASK, "Ctrl"),
        (gdk::ModifierType::ALT_MASK, "Alt"),
        (gdk::ModifierType::SHIFT_MASK, "Shift"),
        (gdk::ModifierType::SUPER_MASK, "Super"),
    ] {
        if modifiers.contains(mask) {
            parts.push(label);
        }
    }
    parts.push(name);
    Some(parts.join("+"))
}

fn ms(duration: Duration) -> f64 {
    (duration.as_secs_f64() * 100_000.0).round() / 100.0
}

fn stats(samples: &[f64]) -> Value {
    if samples.is_empty() {
        return json!({ "count": 0 });
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
    json!({
        "count": sorted.len(),
        "median": at(0.5),
        "p95": at(0.95),
        "max": sorted[sorted.len() - 1],
    })
}

fn hyprctl(what: &str) -> Value {
    Command::new("hyprctl")
        .args(["-j", what])
        .output()
        .ok()
        .and_then(|output| serde_json::from_slice(&output.stdout).ok())
        .unwrap_or(Value::Null)
}

fn environment() -> Value {
    let monitors: Vec<Value> = hyprctl("monitors")
        .as_array()
        .into_iter()
        .flatten()
        .map(|monitor| {
            json!({
                "name": monitor["name"],
                "size": [monitor["width"], monitor["height"]],
                "refresh": monitor["refreshRate"],
                "scale": monitor["scale"],
            })
        })
        .collect();
    let fcitx5 = Command::new("fcitx5-remote")
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    json!({
        "monitors": monitors,
        "fcitx5_remote_state": fcitx5,
        "gtk": format!("{}.{}.{}", gtk::major_version(), gtk::minor_version(), gtk::micro_version()),
        "backend": std::env::var("GDK_BACKEND").unwrap_or_else(|_| "default (wayland)".into()),
    })
}

fn dialog_windows() -> Value {
    let windows: Vec<Value> = hyprctl("clients")
        .as_array()
        .into_iter()
        .flatten()
        .filter(|client| {
            client["title"]
                .as_str()
                .is_some_and(|t| t.contains(DIALOG_TITLE))
                || client["class"]
                    .as_str()
                    .is_some_and(|c| c.contains("portal"))
        })
        .map(|client| {
            json!({
                "class": client["class"],
                "title": client["title"],
                "floating": client["floating"],
                "size": client["size"],
            })
        })
        .collect();
    json!({ "windows_found": windows.len(), "windows": windows })
}

fn dialog_summary(dialog: &Value) -> String {
    match dialog["windows"].as_array() {
        None => "not checked".into(),
        Some(windows) if windows.is_empty() => "no dialog window found".into(),
        Some(windows) => windows
            .iter()
            .map(|w| format!("{} floating={}", w["class"], w["floating"]))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn open_dialog(state: &Rc<State>, window: &gtk::ApplicationWindow, cancel_after: Option<Duration>) {
    let dialog = gtk::FileDialog::builder()
        .title(DIALOG_TITLE)
        .modal(true)
        .build();
    let cancellable = gio::Cancellable::new();
    state.log("file dialog: opening");
    dialog.open(
        Some(window),
        Some(&cancellable),
        glib::clone!(
            #[strong]
            state,
            move |result| match result {
                Ok(file) => state.log(&format!("file dialog: picked {}", file.uri())),
                Err(error) => state.log(&format!("file dialog: closed ({error})")),
            }
        ),
    );
    glib::timeout_add_local_once(
        Duration::from_millis(1500),
        glib::clone!(
            #[strong]
            state,
            move || {
                let found = dialog_windows();
                state.log(&format!("file dialog windows: {found}"));
                state.dialog.replace(found);
                state.refresh_status();
                if let Some(delay) = cancel_after {
                    glib::timeout_add_local_once(delay, move || cancellable.cancel());
                }
            }
        ),
    );
}

fn build(app: &gtk::Application, auto: bool, follow_up: bool) {
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_cursor_blink(false);
    }
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Stet S4 live checks")
        .default_width(1400)
        .default_height(900)
        .build();

    let buffer = sourceview5::Buffer::new(None);
    buffer.set_text(SAMPLE);
    if let Some(rust) = sourceview5::LanguageManager::default().language("rust") {
        buffer.set_language(Some(&rust));
    }
    let view = sourceview5::View::with_buffer(&buffer);
    view.set_monospace(true);
    view.set_show_line_numbers(true);
    view.set_highlight_current_line(true);
    view.set_vexpand(true);
    view.set_hexpand(true);
    let editor = gtk::ScrolledWindow::builder().child(&view).build();

    let log = gtk::TextBuffer::new(None);
    let log_view = gtk::TextView::builder()
        .buffer(&log)
        .editable(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    let log_scroll = gtk::ScrolledWindow::builder()
        .child(&log_view)
        .vexpand(true)
        .build();

    let keys_box = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(3)
        .build();
    let key_labels: Vec<(&'static str, gtk::Label)> = TARGET_KEYS
        .iter()
        .map(|&name| {
            let label = gtk::Label::builder()
                .label(format!("· {name}"))
                .xalign(0.0)
                .build();
            keys_box.insert(&label, -1);
            (name, label)
        })
        .collect();

    let status = gtk::Label::builder().wrap(true).xalign(0.0).build();
    let state = Rc::new(State {
        auto,
        report: Report::new("s4_live"),
        log,
        status: status.clone(),
        key_labels,
        keys_seen: RefCell::default(),
        key_pressed_at: Cell::default(),
        changed_at: Cell::default(),
        key_to_paint: RefCell::default(),
        change_to_paint: RefCell::default(),
        inserts: Cell::default(),
        inserts_after_key: Cell::default(),
        non_ascii: RefCell::default(),
        preedits: RefCell::default(),
        column_first_line: Cell::default(),
        replicating: Cell::default(),
        replications: Cell::default(),
        clipboard: RefCell::default(),
        drops: RefCell::default(),
        dialog: RefCell::new(Value::Null),
        finished: Cell::default(),
        all_keys: RefCell::default(),
        multi_char_inserts: RefCell::default(),
        drag_enters: RefCell::default(),
    });
    state.refresh_status();

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(glib::clone!(
        #[strong]
        state,
        move |_, keyval, _, modifiers| {
            state.on_key(keyval, modifiers);
            glib::Propagation::Proceed
        }
    ));
    window.add_controller(keys);

    let inserted = state.clone();
    buffer.connect_closure(
        "insert-text",
        true,
        glib::closure_local!(move |buffer: sourceview5::Buffer,
                                   end: gtk::TextIter,
                                   text: String,
                                   _length: i32| {
            inserted.on_inserted(&buffer, &end, &text);
        }),
    );
    buffer.connect_changed(glib::clone!(
        #[strong]
        state,
        move |_| state.on_changed()
    ));
    view.connect_preedit_changed(glib::clone!(
        #[strong]
        state,
        move |_, preedit| state.on_preedit(preedit)
    ));
    window.connect_realize(glib::clone!(
        #[strong]
        state,
        move |window| {
            if let Some(clock) = window.frame_clock() {
                clock.connect_after_paint(glib::clone!(
                    #[strong]
                    state,
                    move |_| state.on_after_paint()
                ));
            }
        }
    ));
    WidgetExt::display(&window)
        .clipboard()
        .connect_changed(glib::clone!(
            #[strong]
            state,
            move |clipboard| {
                let clipboard = clipboard.clone();
                let state = state.clone();
                glib::spawn_future_local(async move {
                    match clipboard.read_text_future().await {
                        Ok(Some(text)) => state.on_clipboard(&text),
                        Ok(None) => state.on_clipboard("<no text>"),
                        Err(error) => state.log(&format!("clipboard read failed: {error}")),
                    }
                });
            }
        ));

    let drop_label = gtk::Label::new(Some("Drop a file from Nautilus here"));
    let drop_zone = gtk::Frame::builder()
        .child(&drop_label)
        .height_request(90)
        .build();
    let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    drop.connect_drop(glib::clone!(
        #[strong]
        state,
        move |_, value, _, _| match value.get::<gdk::FileList>() {
            Ok(list) => {
                let paths = list
                    .files()
                    .iter()
                    .map(|file| {
                        file.path()
                            .map_or_else(|| file.uri().to_string(), |p| p.display().to_string())
                    })
                    .collect::<Vec<_>>();
                drop_label.set_text(&format!("Received: {}", paths.join(", ")));
                state.on_drop(paths);
                true
            }
            Err(_) => false,
        }
    ));
    drop.connect_enter(glib::clone!(
        #[strong]
        state,
        move |target, _, _| {
            let formats = target
                .current_drop()
                .map(|drop| drop.formats().to_str().to_string())
                .unwrap_or_default();
            state.log(&format!("drag entered the drop box, offering: {formats}"));
            state.drag_enters.borrow_mut().push(formats);
            gdk::DragAction::COPY
        }
    ));
    drop_zone.add_controller(drop);

    let column = gtk::ToggleButton::with_label("Column caret (5 lines)");
    column.connect_toggled(glib::clone!(
        #[strong]
        state,
        #[weak]
        buffer,
        #[weak]
        view,
        move |button| {
            if button.is_active() {
                let line = 2;
                if let Some(start) = buffer.iter_at_line(line) {
                    buffer.place_cursor(&start);
                }
                state.column_first_line.set(Some(line));
                state.log(&format!(
                    "column caret on: lines {}–{}",
                    line + 1,
                    line + COLUMN_LINES
                ));
            } else {
                state.column_first_line.set(None);
                state.log("column caret off");
            }
            view.grab_focus();
        }
    ));
    let dialog_button = gtk::Button::with_label("Open file dialog…");
    dialog_button.connect_clicked(glib::clone!(
        #[strong]
        state,
        #[weak]
        window,
        move |_| open_dialog(&state, &window, None)
    ));
    let finish = gtk::Button::with_label("Finish");
    finish.add_css_class("suggested-action");
    finish.connect_clicked(glib::clone!(
        #[strong]
        state,
        #[weak]
        app,
        move |_| state.finish(&app)
    ));
    window.connect_close_request(glib::clone!(
        #[strong]
        state,
        #[weak]
        app,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_| {
            state.finish(&app);
            glib::Propagation::Proceed
        }
    ));

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.append(&column);
    buttons.append(&dialog_button);
    buttons.append(&finish);
    let side = gtk::Box::new(gtk::Orientation::Vertical, 8);
    side.set_margin_start(8);
    side.set_margin_end(8);
    side.set_margin_top(8);
    side.set_margin_bottom(8);
    let instructions = if follow_up { FOLLOW_UP } else { INSTRUCTIONS };
    side.append(
        &gtk::Label::builder()
            .label(instructions)
            .wrap(true)
            .xalign(0.0)
            .build(),
    );
    side.append(&buttons);
    side.append(&keys_box);
    side.append(&drop_zone);
    side.append(&status);
    side.append(&log_scroll);
    let paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&editor)
        .end_child(&side)
        .position(760)
        .build();
    window.set_child(Some(&paned));
    window.present();
    view.grab_focus();

    if auto {
        let app = app.clone();
        glib::timeout_add_local_once(
            Duration::from_millis(800),
            glib::clone!(
                #[strong]
                state,
                move || {
                    open_dialog(&state, &window, Some(Duration::from_millis(500)));
                    glib::timeout_add_local_once(Duration::from_millis(3500), move || {
                        let start = buffer.start_iter();
                        let mut end = start;
                        end.forward_to_line_end();
                        buffer.select_range(&start, &end);
                        window.present();
                        view.grab_focus();
                        println!("READY_CLIPBOARD");
                        let _ = std::io::stdout().flush();
                        glib::timeout_add_local_once(Duration::from_secs(8), move || {
                            state.finish(&app)
                        });
                    });
                }
            ),
        );
    }
}

fn main() -> Result<()> {
    require_headless()?;
    let auto = std::env::args().skip(1).any(|arg| arg == "--auto");
    let follow_up = std::env::args().skip(1).any(|arg| arg == "--follow-up");
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| build(app, auto, follow_up));
    app.run_with_args::<&str>(&[]);
    Ok(())
}
