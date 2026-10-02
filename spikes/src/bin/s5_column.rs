//! S5 spike: column (rectangular) mode on GtkSourceView 5 through a `sourceview5::View` subclass.
//! Run: `tools/headless.sh <display> cargo run --release -p stet-spikes --bin s5_column [-- <out.md>]`.

use anyhow::{Context, Result, ensure};
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{gdk, glib, graphene, gsk};
use serde_json::json;
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use stet_spikes::{Report, Row, run_main_loop_until, wait_frames, write_markdown};

const LINES: usize = 10_000;
const LAST: i32 = LINES as i32 - 1;
const TAB_WIDTH: u32 = 4;
const REPS: usize = 5;
const TIMEOUT: Duration = Duration::from_secs(30);
const WATCHDOG: Duration = Duration::from_secs(420);
const INSERT_BAR_MS: f64 = 200.0;
const STALL_WINDOW: Duration = Duration::from_millis(1500);
const COLUMN_MIME: &str = "application/x-stet-column-block";
const FILL: gdk::RGBA = gdk::RGBA::new(0.21, 0.52, 0.89, 0.35);
const CARET: gdk::RGBA = gdk::RGBA::new(0.21, 0.52, 0.89, 0.9);
const CARET_WIDTH: f32 = 2.0;

static WARNINGS: AtomicUsize = AtomicUsize::new(0);

const CAVEATS: [&str; 5] = [
    "Rendered by GTK's Broadway backend and read back through a Cairo renderer, not Wayland/GL.",
    "Keystrokes are simulated through the buffer and view APIs that the IM commit and key bindings call; \
     fcitx5 preedit and commit on the live session are MANUAL.",
    "Visual columns count one cell per character (tabs expand); East Asian wide characters are not \
     double-width, matching gtk_source_view_get_visual_column.",
    "Timings are wall-clock inside the process on a release build; each is the median of 5 repetitions.",
    "The rectangle lives in the spike's ColumnView; Alt+drag and Alt+Shift+arrow gestures are not prototyped.",
];

/// A rectangular selection in visual columns; the cursor line holds the primary (GTK) caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    anchor_line: i32,
    anchor_col: u32,
    cursor_line: i32,
    cursor_col: u32,
}

impl Rect {
    fn new(anchor: (i32, u32), cursor: (i32, u32)) -> Self {
        Self {
            anchor_line: anchor.0,
            anchor_col: anchor.1,
            cursor_line: cursor.0,
            cursor_col: cursor.1,
        }
    }

    fn top(self) -> i32 {
        self.anchor_line.min(self.cursor_line)
    }

    fn bottom(self) -> i32 {
        self.anchor_line.max(self.cursor_line)
    }

    fn left(self) -> u32 {
        self.anchor_col.min(self.cursor_col)
    }

    fn right(self) -> u32 {
        self.anchor_col.max(self.cursor_col)
    }

    fn with_col(self, col: u32) -> Self {
        Self {
            anchor_col: col,
            cursor_col: col,
            ..self
        }
    }
}

fn char_width(ch: char, col: u32, tab: u32) -> u32 {
    if ch == '\t' { tab - col % tab } else { 1 }
}

fn text_width(text: &str, start_col: u32, tab: u32) -> u32 {
    text.chars()
        .fold(start_col, |col, ch| col + char_width(ch, col, tab))
        - start_col
}

/// Byte index where visual column `col` starts in `line`, snapping to the start of a tab that
/// spans it, and the visual column reached there (less than `col` past the end of the line).
fn locate(line: &str, col: u32, tab: u32) -> (usize, u32) {
    let mut reached = 0;
    for (index, ch) in line.char_indices() {
        let next = reached + char_width(ch, reached, tab);
        if next > col {
            return (index, reached);
        }
        reached = next;
    }
    (line.len(), reached)
}

fn insert_in_line(line: &str, col: u32, text: &str, tab: u32) -> String {
    let (index, reached) = locate(line, col, tab);
    let pad = if index == line.len() {
        col - reached
    } else {
        0
    };
    format!(
        "{}{}{text}{}",
        &line[..index],
        " ".repeat(pad as usize),
        &line[index..]
    )
}

fn delete_in_line(line: &str, from: u32, to: u32, tab: u32) -> String {
    let (start, _) = locate(line, from, tab);
    let (end, _) = locate(line, to, tab);
    format!("{}{}", &line[..start], &line[end..])
}

fn block_in_line(line: &str, from: u32, to: u32, tab: u32) -> &str {
    let (start, _) = locate(line, from, tab);
    let (end, _) = locate(line, to, tab);
    &line[start..end]
}

fn iter_at_col(
    buffer: &gtk::TextBuffer,
    line: i32,
    col: u32,
    tab: u32,
) -> Option<(gtk::TextIter, u32)> {
    let mut iter = buffer.iter_at_line(line)?;
    let mut reached = 0;
    while !iter.ends_line() {
        let next = reached + char_width(iter.char(), reached, tab);
        if next > col {
            break;
        }
        reached = next;
        iter.forward_char();
    }
    Some((iter, reached))
}

fn insert_at_col(buffer: &gtk::TextBuffer, line: i32, col: u32, text: &str, tab: u32) {
    let Some((mut iter, reached)) = iter_at_col(buffer, line, col, tab) else {
        return;
    };
    if reached < col && iter.ends_line() {
        let padded = format!("{}{text}", " ".repeat((col - reached) as usize));
        buffer.insert(&mut iter, &padded);
    } else {
        buffer.insert(&mut iter, text);
    }
}

fn delete_cols(buffer: &gtk::TextBuffer, line: i32, from: u32, to: u32, tab: u32) {
    let (Some((mut start, _)), Some((mut end, _))) = (
        iter_at_col(buffer, line, from, tab),
        iter_at_col(buffer, line, to, tab),
    ) else {
        return;
    };
    if start.offset() < end.offset() {
        buffer.delete(&mut start, &mut end);
    }
}

/// Inserts `text` at the rectangle's left column on every line as one user action.
fn insert_column(buffer: &gtk::TextBuffer, rect: Rect, text: &str, tab: u32) {
    buffer.begin_user_action();
    for line in rect.top()..=rect.bottom() {
        insert_at_col(buffer, line, rect.left(), text, tab);
    }
    buffer.end_user_action();
}

enum Pending {
    Insert {
        line: i32,
        offset: i32,
        col: u32,
        text: String,
    },
    Delete {
        line: i32,
        from: u32,
        to: u32,
    },
}

mod imp {
    use super::*;
    use sourceview5::subclass::prelude::*;

    #[derive(Default)]
    pub struct ColumnView {
        pub(super) rect: Cell<Option<Rect>>,
        pub(super) replicating: Cell<bool>,
        pub(super) pending: RefCell<Option<Pending>>,
        pub(super) layer_calls: Cell<u64>,
        pub(super) rect_rows_painted: Cell<u64>,
        pub(super) rect_paint_samples: RefCell<Vec<Duration>>,
        pub(super) guarded_history_edits: Cell<u64>,
        pub(super) pastes: Cell<u64>,
        pub(super) group_typing: Cell<bool>,
        pub(super) session_open: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ColumnView {
        const NAME: &'static str = "StetS5ColumnView";
        type Type = super::ColumnView;
        type ParentType = sourceview5::View;
    }

    impl ObjectImpl for ColumnView {}
    impl WidgetImpl for ColumnView {}

    impl TextViewImpl for ColumnView {
        fn snapshot_layer(&self, layer: gtk::TextViewLayer, snapshot: gtk::Snapshot) {
            self.parent_snapshot_layer(layer, snapshot.clone());
            self.layer_calls.set(self.layer_calls.get() + 1);
            if layer != gtk::TextViewLayer::BelowText {
                return;
            }
            if let Some(rect) = self.rect.get() {
                let started = Instant::now();
                let rows = self.obj().paint_rect(rect, &snapshot);
                self.rect_paint_samples.borrow_mut().push(started.elapsed());
                self.rect_rows_painted
                    .set(self.rect_rows_painted.get() + rows);
            }
        }

        fn paste_clipboard(&self) {
            let view = self.obj().clone();
            let clipboard = view.clipboard();
            if !clipboard.formats().contain_mime_type(COLUMN_MIME) {
                self.parent_paste_clipboard();
                return;
            }
            glib::spawn_future_local(glib::clone!(
                #[weak]
                view,
                async move {
                    match read_clipboard(&clipboard, COLUMN_MIME).await {
                        Ok(bytes) => view.paste_block(&String::from_utf8_lossy(&bytes)),
                        Err(error) => eprintln!("s5: rectangular paste failed: {error}"),
                    }
                }
            ));
        }
    }

    impl ViewImpl for ColumnView {}
}

glib::wrapper! {
    pub struct ColumnView(ObjectSubclass<imp::ColumnView>)
        @extends sourceview5::View, gtk::TextView, gtk::Widget,
        @implements gtk::Accessible, gtk::AccessibleText, gtk::Buildable, gtk::ConstraintTarget,
            gtk::Scrollable;
}

impl ColumnView {
    fn new(buffer: &sourceview5::Buffer) -> Self {
        let view: Self = glib::Object::builder()
            .property("buffer", buffer)
            .property("monospace", true)
            .property("tab-width", TAB_WIDTH)
            .build();
        view.connect_buffer(buffer);
        view
    }

    fn connect_buffer(&self, buffer: &sourceview5::Buffer) {
        buffer.connect_insert_text(glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |_, location, text| view.before_insert(location, text)
        ));
        buffer.connect_local(
            "insert-text",
            true,
            glib::clone!(
                #[weak(rename_to = view)]
                self,
                #[upgrade_or]
                None,
                move |_| {
                    view.after_insert();
                    None
                }
            ),
        );
        buffer.connect_delete_range(glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |_, start, end| view.before_delete(start, end)
        ));
        buffer.connect_local(
            "delete-range",
            true,
            glib::clone!(
                #[weak(rename_to = view)]
                self,
                #[upgrade_or]
                None,
                move |_| {
                    view.after_delete();
                    None
                }
            ),
        );
        for signal in ["undo", "redo"] {
            buffer.connect_local(
                signal,
                false,
                glib::clone!(
                    #[weak(rename_to = view)]
                    self,
                    #[upgrade_or]
                    None,
                    move |_| {
                        view.set_rect(None);
                        view.imp().replicating.set(true);
                        None
                    }
                ),
            );
            buffer.connect_local(
                signal,
                true,
                glib::clone!(
                    #[weak(rename_to = view)]
                    self,
                    #[upgrade_or]
                    None,
                    move |_| {
                        view.imp().replicating.set(false);
                        None
                    }
                ),
            );
        }
    }

    fn rect(&self) -> Option<Rect> {
        self.imp().rect.get()
    }

    fn set_rect(&self, rect: Option<Rect>) {
        if rect.is_none() {
            self.close_session();
        }
        self.imp().rect.set(rect);
        self.queue_draw();
    }

    fn before_insert(&self, location: &gtk::TextIter, text: &str) {
        let imp = self.imp();
        if imp.replicating.get() {
            imp.guarded_history_edits
                .set(imp.guarded_history_edits.get() + 1);
            return;
        }
        let Some(rect) = imp.rect.get() else {
            return;
        };
        let col = self.visual_column(location);
        let at_column = col == rect.left() || (col < rect.left() && location.ends_line());
        if text.contains(['\n', '\r'])
            || location.line() != rect.cursor_line
            || rect.left() != rect.right()
            || !at_column
        {
            self.set_rect(None);
            return;
        }
        imp.pending.replace(Some(Pending::Insert {
            line: location.line(),
            offset: location.line_offset(),
            col,
            text: text.to_owned(),
        }));
    }

    fn after_insert(&self) {
        let imp = self.imp();
        let Some(Pending::Insert {
            line,
            offset,
            col,
            text,
        }) = imp.pending.take()
        else {
            return;
        };
        let Some(rect) = imp.rect.get() else {
            return;
        };
        let buffer = self.buffer();
        let tab = self.tab_width();
        let target = rect.left();
        imp.replicating.set(true);
        if col < target
            && let Some(mut start) = buffer.iter_at_line_offset(line, offset)
        {
            buffer.insert(&mut start, &" ".repeat((target - col) as usize));
        }
        for other in rect.top()..=rect.bottom() {
            if other != line {
                insert_at_col(&buffer, other, target, &text, tab);
            }
        }
        imp.replicating.set(false);
        self.set_rect(Some(rect.with_col(target + text_width(&text, target, tab))));
        self.open_session();
    }

    /// With grouping on, the first replicated keystroke opens a user action that stays open until
    /// column mode ends (or an undo/redo arrives), so a typing burst is one undo step.
    fn set_group_typing(&self, enabled: bool) {
        self.imp().group_typing.set(enabled);
        if !enabled {
            self.close_session();
        }
    }

    fn open_session(&self) {
        let imp = self.imp();
        if imp.group_typing.get() && !imp.session_open.replace(true) {
            self.buffer().begin_user_action();
        }
    }

    fn close_session(&self) {
        if self.imp().session_open.replace(false) {
            self.buffer().end_user_action();
        }
    }

    fn before_delete(&self, start: &gtk::TextIter, end: &gtk::TextIter) {
        let imp = self.imp();
        if imp.replicating.get() {
            imp.guarded_history_edits
                .set(imp.guarded_history_edits.get() + 1);
            return;
        }
        let Some(rect) = imp.rect.get() else {
            return;
        };
        let (from, to) = (self.visual_column(start), self.visual_column(end));
        if start.line() != rect.cursor_line
            || end.line() != start.line()
            || rect.left() != rect.right()
            || (from != rect.left() && to != rect.left())
        {
            self.set_rect(None);
            return;
        }
        imp.pending.replace(Some(Pending::Delete {
            line: start.line(),
            from,
            to,
        }));
    }

    fn after_delete(&self) {
        let imp = self.imp();
        let Some(Pending::Delete { line, from, to }) = imp.pending.take() else {
            return;
        };
        let Some(rect) = imp.rect.get() else {
            return;
        };
        let buffer = self.buffer();
        let tab = self.tab_width();
        imp.replicating.set(true);
        for other in rect.top()..=rect.bottom() {
            if other != line {
                delete_cols(&buffer, other, from, to, tab);
            }
        }
        imp.replicating.set(false);
        self.set_rect(Some(rect.with_col(from)));
        self.open_session();
    }

    fn insert_column(&self, text: &str) {
        let Some(rect) = self.rect() else {
            return;
        };
        let tab = self.tab_width();
        self.imp().replicating.set(true);
        insert_column(&self.buffer(), rect, text, tab);
        self.imp().replicating.set(false);
        self.set_rect(Some(
            rect.with_col(rect.left() + text_width(text, rect.left(), tab)),
        ));
    }

    /// The alternative to per-line inserts: rewrite all of the rectangle's lines with one delete
    /// and one insert.
    fn replace_column(&self, rect: Rect, text: &str) {
        let buffer = self.buffer();
        let (Some(mut start), Some(mut end)) = (
            buffer.iter_at_line(rect.top()),
            buffer.iter_at_line(rect.bottom()),
        ) else {
            return;
        };
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        let tab = self.tab_width();
        let rewritten = buffer
            .text(&start, &end, true)
            .split('\n')
            .map(|line| insert_in_line(line, rect.left(), text, tab))
            .collect::<Vec<_>>()
            .join("\n");
        self.set_rect(None);
        self.imp().replicating.set(true);
        buffer.begin_user_action();
        buffer.delete(&mut start, &mut end);
        buffer.insert(&mut start, &rewritten);
        buffer.end_user_action();
        self.imp().replicating.set(false);
    }

    /// Replaces GtkSourceView's whole-pixel tab stops with `tab_width` space advances in Pango
    /// units, so tab stops stay on the character grid when the advance is fractional.
    fn apply_exact_tabs(&self) {
        let (space, _) = self.create_pango_layout(Some(" ")).size();
        let mut tabs = gtk::pango::TabArray::new(1, false);
        tabs.set_tab(
            0,
            gtk::pango::TabAlign::Left,
            space * self.tab_width() as i32,
        );
        self.set_tabs(&tabs);
    }

    fn tab_stops(&self) -> serde_json::Value {
        self.tabs().map_or(json!(null), |tabs| {
            json!({ "first_stop": tabs.tab(0).1, "in_pixels": tabs.is_positions_in_pixels() })
        })
    }

    fn delete_block(&self) {
        let Some(rect) = self.rect() else {
            return;
        };
        let buffer = self.buffer();
        let tab = self.tab_width();
        self.imp().replicating.set(true);
        buffer.begin_user_action();
        for line in rect.top()..=rect.bottom() {
            delete_cols(&buffer, line, rect.left(), rect.right(), tab);
        }
        buffer.end_user_action();
        self.imp().replicating.set(false);
        self.set_rect(Some(rect.with_col(rect.left())));
    }

    fn block_lines(&self, rect: Rect) -> Vec<String> {
        let buffer = self.buffer();
        let tab = self.tab_width();
        (rect.top()..=rect.bottom())
            .filter_map(|line| {
                let (start, _) = iter_at_col(&buffer, line, rect.left(), tab)?;
                let (end, _) = iter_at_col(&buffer, line, rect.right(), tab)?;
                Some(buffer.text(&start, &end, true).to_string())
            })
            .collect()
    }

    /// Puts the block on the clipboard twice: under the column MIME type for rectangular paste,
    /// and as plain text for every other application.
    fn copy_block(&self) -> Result<()> {
        let rect = self.rect().context("no rectangle to copy")?;
        let bytes = glib::Bytes::from_owned(self.block_lines(rect).join("\n").into_bytes());
        let provider = gdk::ContentProvider::new_union(&[
            gdk::ContentProvider::for_bytes(COLUMN_MIME, &bytes),
            gdk::ContentProvider::for_bytes("text/plain;charset=utf-8", &bytes),
        ]);
        self.clipboard()
            .set_content(Some(&provider))
            .context("setting the clipboard content")
    }

    fn paste_block(&self, block: &str) {
        let buffer = self.buffer();
        let tab = self.tab_width();
        let (top, col) = match self.rect() {
            Some(rect) => (rect.top(), rect.left()),
            None => {
                let cursor = buffer.iter_at_mark(&buffer.get_insert());
                (cursor.line(), self.visual_column(&cursor))
            }
        };
        let imp = self.imp();
        imp.replicating.set(true);
        buffer.begin_user_action();
        for (index, row) in block.split('\n').enumerate() {
            let line = top + index as i32;
            if line >= buffer.line_count() {
                buffer.insert(&mut buffer.end_iter(), "\n");
            }
            insert_at_col(&buffer, line, col, row, tab);
        }
        buffer.end_user_action();
        imp.replicating.set(false);
        self.set_rect(None);
        imp.pastes.set(imp.pastes.get() + 1);
    }

    fn space_width(&self) -> f32 {
        let (width, _) = self.create_pango_layout(Some(" ")).size();
        width as f32 / gtk::pango::SCALE as f32
    }

    fn x_at_col(&self, buffer: &gtk::TextBuffer, line: i32, col: u32, space: f32) -> f32 {
        let Some((iter, reached)) = iter_at_col(buffer, line, col, self.tab_width()) else {
            return 0.0;
        };
        self.iter_location(&iter).x() as f32 + (col - reached) as f32 * space
    }

    /// Paints the visible rows of the rectangle in buffer coordinates; returns the row count.
    fn paint_rect(&self, rect: Rect, snapshot: &gtk::Snapshot) -> u64 {
        let buffer = self.buffer();
        let visible = self.visible_rect();
        let first = rect.top().max(self.line_at_y(visible.y()).0.line());
        let last = rect
            .bottom()
            .min(self.line_at_y(visible.y() + visible.height()).0.line());
        let space = self.space_width();
        let caret = rect.left() == rect.right();
        let mut rows = 0;
        for line in first..=last {
            let Some(iter) = buffer.iter_at_line(line) else {
                break;
            };
            let (y, height) = self.line_yrange(&iter);
            let left = self.x_at_col(&buffer, line, rect.left(), space);
            let (width, color) = if caret {
                (CARET_WIDTH, &CARET)
            } else {
                (
                    self.x_at_col(&buffer, line, rect.right(), space) - left,
                    &FILL,
                )
            };
            snapshot.append_color(
                color,
                &graphene::Rect::new(left, y as f32, width, height as f32),
            );
            rows += 1;
        }
        rows
    }

    fn layer_calls(&self) -> u64 {
        self.imp().layer_calls.get()
    }

    fn rect_rows_painted(&self) -> u64 {
        self.imp().rect_rows_painted.get()
    }

    fn take_paint_samples(&self) -> Vec<Duration> {
        self.imp().rect_paint_samples.take()
    }

    fn pastes(&self) -> u64 {
        self.imp().pastes.get()
    }
}

async fn read_clipboard(clipboard: &gdk::Clipboard, mime: &str) -> Result<Vec<u8>, glib::Error> {
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
    Ok(data)
}

fn block_on<T: 'static>(future: impl Future<Output = T> + 'static) -> Result<T> {
    let slot = Rc::new(RefCell::new(None));
    glib::spawn_future_local(glib::clone!(
        #[strong]
        slot,
        async move {
            slot.replace(Some(future.await));
        }
    ));
    run_main_loop_until(TIMEOUT, || slot.borrow().is_some())?;
    slot.take().context("future finished without a value")
}

#[derive(Clone, Copy)]
struct Summary {
    median: f64,
    min: f64,
    max: f64,
}

impl Summary {
    fn of(samples: &[f64]) -> Self {
        let mut sorted = samples.to_vec();
        sorted.sort_by(f64::total_cmp);
        let mid = sorted.len() / 2;
        let median = if sorted.len().is_multiple_of(2) {
            (sorted[mid - 1] + sorted[mid]) / 2.0
        } else {
            sorted[mid]
        };
        Self {
            median,
            min: sorted[0],
            max: sorted[sorted.len() - 1],
        }
    }

    fn text(self, unit: &str) -> String {
        format!(
            "median {:.2} {unit} (min {:.2}, max {:.2})",
            self.median, self.min, self.max
        )
    }

    fn json(self) -> serde_json::Value {
        json!({ "median": self.median, "min": self.min, "max": self.max })
    }
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn hash(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn text(buffer: &impl IsA<gtk::TextBuffer>) -> String {
    let (start, end) = buffer.bounds();
    buffer.text(&start, &end, true).to_string()
}

fn reset(buffer: &impl IsA<gtk::TextBuffer>, content: &str) {
    buffer.begin_irreversible_action();
    buffer.set_text(content);
    buffer.end_irreversible_action();
}

fn undo_until(buffer: &impl IsA<gtk::TextBuffer>, target: &str, limit: usize) -> (usize, bool) {
    let mut steps = 0;
    while text(buffer) != target && steps < limit && buffer.can_undo() {
        buffer.undo();
        steps += 1;
    }
    (steps, text(buffer) == target)
}

fn redo_until(buffer: &impl IsA<gtk::TextBuffer>, target: &str, limit: usize) -> (usize, bool) {
    let mut steps = 0;
    while text(buffer) != target && steps < limit && buffer.can_redo() {
        buffer.redo();
        steps += 1;
    }
    (steps, text(buffer) == target)
}

fn first_diff(actual: &str, expected: &str) -> serde_json::Value {
    let mismatch = actual
        .split('\n')
        .zip(expected.split('\n'))
        .enumerate()
        .find(|(_, (got, want))| got != want);
    match mismatch {
        Some((line, (got, want))) => json!({ "line": line, "actual": got, "expected": want }),
        None if actual == expected => json!(null),
        None => {
            json!({ "line_counts": [actual.split('\n').count(), expected.split('\n').count()] })
        }
    }
}

fn make_lines(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| match i % 7 {
            0 => String::new(),
            1 => format!("\tfn item_{i}() {{}}"),
            2 => "short".to_owned(),
            3 => format!("abcdefgh\t= {i};"),
            4 => format!("héllo wörld — ünïcode {i}"),
            5 => format!("{i:>6} {}", "x".repeat(100)),
            _ => format!("    let value_{i} = {i};"),
        })
        .collect()
}

fn map_lines(lines: &[String], f: impl Fn(&str) -> String) -> Vec<String> {
    lines.iter().map(|line| f(line)).collect()
}

fn pass(passed: bool) -> Option<bool> {
    Some(passed)
}

struct Fixture {
    view: ColumnView,
    buffer: sourceview5::Buffer,
    lines: Vec<String>,
    original: String,
    local: PathBuf,
}

impl Fixture {
    fn text_buffer(&self) -> &gtk::TextBuffer {
        self.buffer.upcast_ref()
    }

    fn line_end(&self, line: i32) -> Result<gtk::TextIter> {
        let mut iter = self
            .buffer
            .iter_at_line(line)
            .context("line out of range")?;
        if !iter.ends_line() {
            iter.forward_to_line_end();
        }
        Ok(iter)
    }

    fn reset_to(&self, content: &str) -> Result<()> {
        self.view.set_rect(None);
        reset(&self.buffer, content);
        wait_frames(&self.view, 1, TIMEOUT)?;
        Ok(())
    }

    fn scroll_to_line(&self, line: i32) -> Result<()> {
        let iter = self
            .buffer
            .iter_at_line(line)
            .context("line out of range")?;
        let mark = self.buffer.create_mark(None, &iter, true);
        self.view.scroll_to_mark(&mark, 0.0, true, 0.0, 0.0);
        let view = &self.view;
        run_main_loop_until(TIMEOUT, || {
            let visible = view.visible_rect();
            let (y, _) = view.line_yrange(&iter);
            visible.y() <= y && y < visible.y() + visible.height()
        })?;
        wait_frames(view, 3, TIMEOUT)?;
        self.buffer.delete_mark(&mark);
        Ok(())
    }
}

struct Image {
    texture: gdk::Texture,
    bytes: glib::Bytes,
    stride: usize,
}

impl Image {
    fn pixel(&self, x: i32, y: i32) -> Result<[u8; 4]> {
        ensure!(
            x >= 0 && y >= 0 && x < self.texture.width() && y < self.texture.height(),
            "pixel ({x}, {y}) outside the rendered view"
        );
        let index = y as usize * self.stride + x as usize * 4;
        let pixel = self.bytes.get(index..index + 4).context("short texture")?;
        Ok([pixel[0], pixel[1], pixel[2], pixel[3]])
    }
}

fn render(view: &ColumnView) -> Result<Image> {
    let (width, height) = (view.width(), view.height());
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(view)).snapshot(&snapshot, f64::from(width), f64::from(height));
    let node = snapshot
        .to_node()
        .context("the view produced no render node")?;
    let renderer = gsk::CairoRenderer::new();
    renderer
        .realize_for_display(&view.display())
        .context("realizing the Cairo renderer")?;
    let viewport = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
    let texture = renderer.render_texture(&node, Some(&viewport));
    renderer.unrealize();
    let mut downloader = gdk::TextureDownloader::new(&texture);
    downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = downloader.download_bytes();
    Ok(Image {
        texture,
        bytes,
        stride,
    })
}

fn blend(background: [u8; 4], color: &gdk::RGBA) -> [u8; 4] {
    let mix = |bg: u8, fg: f32| {
        (f32::from(bg) * (1.0 - color.alpha()) + fg * 255.0 * color.alpha()).round() as u8
    };
    [
        mix(background[0], color.red()),
        mix(background[1], color.green()),
        mix(background[2], color.blue()),
        255,
    ]
}

/// Samples an empty line inside the rectangle (column 15) and outside it (column 30), and checks
/// the inside pixel is the fill blended over the outside (background) pixel.
fn pixel_check(fx: &Fixture, line: i32, png: &Path) -> Result<(bool, serde_json::Value)> {
    let view = &fx.view;
    let buffer = fx.text_buffer();
    let iter = buffer.iter_at_line(line).context("line out of range")?;
    let (y, height) = view.line_yrange(&iter);
    let space = view.space_width();
    let inside_x = view.x_at_col(buffer, line, 15, space);
    let outside_x = view.x_at_col(buffer, line, 30, space);
    let (inside_wx, wy) =
        view.buffer_to_window_coords(gtk::TextWindowType::Widget, inside_x as i32, y + height / 2);
    let (outside_wx, _) = view.buffer_to_window_coords(
        gtk::TextWindowType::Widget,
        outside_x as i32,
        y + height / 2,
    );
    let image = render(view)?;
    image
        .texture
        .save_to_png(png)
        .with_context(|| format!("saving {}", png.display()))?;
    let inside = image.pixel(inside_wx, wy)?;
    let outside = image.pixel(outside_wx, wy)?;
    let expected = blend(outside, &FILL);
    let close = inside
        .iter()
        .zip(expected)
        .take(3)
        .all(|(&got, want)| got.abs_diff(want) <= 6);
    let ok = close && inside != outside;
    let detail = json!({
        "line": line,
        "widget_xy_inside": [inside_wx, wy],
        "widget_xy_outside": [outside_wx, wy],
        "inside_rgba": inside,
        "outside_rgba": outside,
        "expected_rgba": expected,
        "png": png.display().to_string(),
        "ok": ok,
    });
    Ok((ok, detail))
}

/// Columns 10, 15 and 20 on lines 1–6 whose x differs from the same column on empty line 7.
fn misaligned_columns(fx: &Fixture) -> Vec<serde_json::Value> {
    let view = &fx.view;
    let buffer = fx.text_buffer();
    let space = view.space_width();
    let mut misaligned = Vec::new();
    for col in [10, 15, 20] {
        let reference = view.x_at_col(buffer, 7, col, space);
        for line in 1..=6 {
            let x = view.x_at_col(buffer, line, col, space);
            if (x - reference).abs() > 0.5 {
                misaligned
                    .push(json!({ "line": line, "col": col, "x": x, "empty_line_x": reference }));
            }
        }
    }
    misaligned
}

fn check_painting(fx: &Fixture, report: &Report, rows: &mut Vec<Row>) -> Result<()> {
    let view = &fx.view;
    let buffer = fx.text_buffer();
    let is_view = view.is::<sourceview5::View>();
    report.record("subclass_type", view.type_().name());
    rows.push(Row::new(
        "Subclass sourceview5::View (ViewImpl) and override TextViewImpl::snapshot_layer",
        format!(
            "{} builds and runs; is_a GtkSourceView: {is_view}",
            view.type_().name()
        ),
        "possible from gtk4-rs",
        pass(is_view),
    ));

    let mut checked = 0u64;
    let mut mismatches = 0u64;
    for line in 0..=LAST {
        for col in 0..=24 {
            let (iter, reached) =
                iter_at_col(buffer, line, col, TAB_WIDTH).context("line out of range")?;
            checked += 1;
            if view.visual_column(&iter) != reached {
                mismatches += 1;
            }
        }
    }
    report.record(
        "visual_column_agreement",
        json!({ "checked": checked, "mismatches": mismatches }),
    );
    rows.push(Row::new(
        "Tab-aware visual column walker vs gtk_source_view_get_visual_column (10k lines × cols 0–24)",
        format!("{mismatches} mismatches in {checked} positions"),
        "0 mismatches",
        pass(mismatches == 0),
    ));

    let calls_before = view.layer_calls();
    let rows_before = view.rect_rows_painted();
    view.set_rect(Some(Rect::new((2, 10), (40, 20))));
    wait_frames(view, 3, TIMEOUT)?;
    let layer_calls = view.layer_calls() - calls_before;
    let rect_rows = view.rect_rows_painted() - rows_before;
    report.record(
        "snapshot_layer_calls",
        json!({ "layer_calls": layer_calls, "rect_rows": rect_rows, "realized": view.is_realized() }),
    );
    rows.push(Row::new(
        "snapshot_layer invoked on a realized view under Broadway (3 frames)",
        format!("{layer_calls} layer calls, {rect_rows} rectangle rows painted"),
        "> 0",
        pass(layer_calls > 0 && rect_rows > 0),
    ));

    let (top_ok, top) = pixel_check(fx, 7, &fx.local.join("s5-column-top.png"))?;
    report.record("pixels_top", top.clone());

    let default_tabs = view.tab_stops();
    let misaligned = misaligned_columns(fx);
    view.apply_exact_tabs();
    wait_frames(view, 2, TIMEOUT)?;
    let exact_tabs = view.tab_stops();
    let misaligned_exact = misaligned_columns(fx);
    let (exact_ok, exact_pixels) =
        pixel_check(fx, 7, &fx.local.join("s5-column-top-exact-tabs.png"))?;
    report.record(
        "column_x_alignment",
        json!({
            "space_width_px": view.space_width(),
            "font": view.pango_context().font_description().map(|font| font.to_str().to_string()),
            "gtksourceview_tabs": default_tabs,
            "misaligned_with_gtksourceview_tabs": misaligned,
            "exact_tabs": exact_tabs,
            "misaligned_with_exact_tabs": misaligned_exact,
            "pixels_with_exact_tabs": exact_pixels,
        }),
    );

    view.set_rect(Some(Rect::new((0, 10), (LAST, 20))));
    wait_frames(view, 2, TIMEOUT)?;
    view.take_paint_samples();
    let rows_before = view.rect_rows_painted();
    let tick = view.add_tick_callback(|view, _| {
        view.queue_draw();
        glib::ControlFlow::Continue
    });
    wait_frames(view, 30, TIMEOUT)?;
    tick.remove();
    let samples: Vec<f64> = view
        .take_paint_samples()
        .into_iter()
        .map(|sample| sample.as_secs_f64() * 1e6)
        .collect();
    ensure!(!samples.is_empty(), "the rectangle was never painted");
    let per_paint = (view.rect_rows_painted() - rows_before) / samples.len() as u64;
    let paint = Summary::of(&samples);
    report.record(
        "rect_paint_us",
        json!({ "paints": samples.len(), "rows_per_paint": per_paint, "us": paint.json() }),
    );

    view.set_rect(Some(Rect::new((5000, 10), (5040, 20))));
    fx.scroll_to_line(5000)?;
    let (scrolled_ok, scrolled) = pixel_check(fx, 5005, &fx.local.join("s5-column-scrolled.png"))?;
    report.record("pixels_scrolled", scrolled.clone());
    rows.push(Row::new(
        "Rectangle pixels: fill blended over background inside, background outside",
        format!(
            "top (line 7): inside {} vs expected {}; scrolled (line 5005): inside {} vs expected {}",
            top["inside_rgba"], top["expected_rgba"], scrolled["inside_rgba"], scrolled["expected_rgba"]
        ),
        "match within 6/255 at both scroll positions",
        pass(top_ok && scrolled_ok),
    ));
    rows.push(Row::new(
        "Column x on lines 1–6 (tabs, text, virtual space) vs an empty line, GtkSourceView's own tab stops",
        format!(
            "{} of 18 positions off; tab stop {} vs 4 × {:.2} px space advance",
            misaligned.len(),
            default_tabs,
            view.space_width()
        ),
        "aligned within 0.5 px",
        pass(misaligned.is_empty()),
    ));
    rows.push(Row::new(
        "Same, after set_tabs with 4 space advances in Pango units",
        format!(
            "{} of 18 positions off; tab stop {exact_tabs}; pixel check {}",
            misaligned_exact.len(),
            if exact_ok { "ok" } else { "failed" }
        ),
        "aligned within 0.5 px",
        pass(misaligned_exact.is_empty() && exact_ok),
    ));
    rows.push(Row::new(
        "Rectangle paint cost per frame (10k-line rectangle, only visible rows painted)",
        format!(
            "{} over {} paints, {per_paint} rows each",
            paint.text("µs"),
            samples.len()
        ),
        "not set by the plan",
        None,
    ));
    fx.view.set_rect(None);
    fx.scroll_to_line(0)
}

/// What one edit costs the UI: the synchronous edit, the work of the next frame, and the longest
/// main-loop stall within `STALL_WINDOW` afterwards (idle relayout and that frame included).
#[derive(Clone, Copy)]
struct EditCost {
    edit_ms: f64,
    frame_delay_ms: f64,
    frame_work_ms: f64,
    max_stall_ms: f64,
}

fn edit_cost(view: &ColumnView, edit: impl FnOnce()) -> Result<EditCost> {
    let clock = view.frame_clock().context("no frame clock")?;
    let starts: Rc<RefCell<Vec<Instant>>> = Rc::default();
    let ends: Rc<RefCell<Vec<Instant>>> = Rc::default();
    let before = clock.connect_before_paint(glib::clone!(
        #[strong]
        starts,
        move |_| starts.borrow_mut().push(Instant::now())
    ));
    let after = clock.connect_after_paint(glib::clone!(
        #[strong]
        ends,
        move |_| ends.borrow_mut().push(Instant::now())
    ));
    let started = Instant::now();
    edit();
    let edited = Instant::now();
    view.queue_draw();
    let last_tick = Rc::new(Cell::new(edited));
    let max_gap = Rc::new(Cell::new(Duration::ZERO));
    let ticker = glib::timeout_add_local(
        Duration::from_millis(1),
        glib::clone!(
            #[strong]
            last_tick,
            #[strong]
            max_gap,
            move || {
                let now = Instant::now();
                max_gap.set(max_gap.get().max(now - last_tick.get()));
                last_tick.set(now);
                glib::ControlFlow::Continue
            }
        ),
    );
    let waited = run_main_loop_until(TIMEOUT, || {
        !ends.borrow().is_empty() && edited.elapsed() >= STALL_WINDOW
    });
    ticker.remove();
    clock.disconnect(before);
    clock.disconnect(after);
    waited?;
    let frame_start = *starts.borrow().first().context("no frame started")?;
    let frame_end = *ends
        .borrow()
        .iter()
        .find(|end| **end >= frame_start)
        .context("no frame finished")?;
    Ok(EditCost {
        edit_ms: ms(edited - started),
        frame_delay_ms: ms(frame_start - edited),
        frame_work_ms: ms(frame_end - frame_start),
        max_stall_ms: ms(max_gap.get()),
    })
}

struct Costs {
    edit: Summary,
    frame_delay: Summary,
    frame_work: Summary,
    max_stall: Summary,
}

impl Costs {
    fn of(costs: &[EditCost]) -> Self {
        let pick = |f: fn(&EditCost) -> f64| Summary::of(&costs.iter().map(f).collect::<Vec<_>>());
        Self {
            edit: pick(|cost| cost.edit_ms),
            frame_delay: pick(|cost| cost.frame_delay_ms),
            frame_work: pick(|cost| cost.frame_work_ms),
            max_stall: pick(|cost| cost.max_stall_ms),
        }
    }

    fn json(&self) -> serde_json::Value {
        json!({
            "edit_ms": self.edit.json(),
            "frame_delay_ms": self.frame_delay.json(),
            "frame_work_ms": self.frame_work.json(),
            "max_stall_ms": self.max_stall.json(),
        })
    }

    fn after_edit_text(&self) -> String {
        format!(
            "next frame's work {}; longest main-loop stall in the {} ms after the edit {}",
            self.frame_work.text("ms"),
            STALL_WINDOW.as_millis(),
            self.max_stall.text("ms")
        )
    }
}

fn check_baseline(fx: &Fixture, report: &Report, rows: &mut Vec<Row>) -> Result<()> {
    fx.reset_to(&fx.original)?;
    let mut idle = Vec::new();
    let mut one_line = Vec::new();
    for _ in 0..REPS {
        idle.push(edit_cost(&fx.view, || {})?);
        one_line.push(edit_cost(&fx.view, || {
            fx.buffer.insert(&mut fx.buffer.start_iter(), "x");
        })?);
    }
    let (idle, one_line) = (Costs::of(&idle), Costs::of(&one_line));
    report.record(
        "baseline_costs",
        json!({ "idle_redraw": idle.json(), "one_char_insert": one_line.json() }),
    );
    rows.push(Row::new(
        "Baseline: a redraw with no edit / a one-character insert",
        format!(
            "frame work {} / {}; longest stall {} / {}; Broadway starts the frame {} after the request",
            idle.frame_work.text("ms"),
            one_line.frame_work.text("ms"),
            idle.max_stall.text("ms"),
            one_line.max_stall.text("ms"),
            idle.frame_delay.text("ms")
        ),
        "reference",
        None,
    ));
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Strategy {
    PerLine,
    Replace,
}

fn check_column_insert(fx: &Fixture, report: &Report, rows: &mut Vec<Row>) -> Result<()> {
    for (label, col, attached, strategy) in [
        (
            "col 0, per-line inserts, view attached",
            0,
            true,
            Strategy::PerLine,
        ),
        (
            "col 10, padding and tabs, per-line inserts, view attached",
            10,
            true,
            Strategy::PerLine,
        ),
        (
            "col 0, per-line inserts, buffer only",
            0,
            false,
            Strategy::PerLine,
        ),
        (
            "col 0, one delete + one insert of all lines, view attached",
            0,
            true,
            Strategy::Replace,
        ),
    ] {
        let expected = map_lines(&fx.lines, |line| {
            insert_in_line(line, col, "// ", TAB_WIDTH)
        })
        .join("\n");
        let detached = sourceview5::Buffer::new(None);
        let buffer: &gtk::TextBuffer = if attached {
            fx.text_buffer()
        } else {
            detached.upcast_ref()
        };
        let (mut costs, mut insert, mut undo, mut redo) = (vec![], vec![], vec![], vec![]);
        let (mut correct, mut undo_ok, mut redo_ok) = (true, true, true);
        let (mut hashes, mut mark_lines) = (json!(null), json!(null));
        let mark_sets = Rc::new(Cell::new(0u64));
        let counter = buffer.connect_mark_set(glib::clone!(
            #[strong]
            mark_sets,
            move |_, _, _| mark_sets.set(mark_sets.get() + 1)
        ));
        let (mut mark_sets_edit, mut mark_sets_undo) = (0, 0);
        for rep in 0..REPS {
            if attached {
                fx.reset_to(&fx.original)?;
            } else {
                reset(buffer, &fx.original);
            }
            let mark =
                buffer.create_mark(None, &buffer.iter_at_line(5000).context("line 5000")?, true);
            let rect = Rect::new((LAST, col), (0, col));
            let apply = || match (attached, strategy) {
                (true, Strategy::PerLine) => {
                    fx.view.set_rect(Some(rect));
                    fx.view.insert_column("// ");
                }
                (true, Strategy::Replace) => fx.view.replace_column(rect, "// "),
                (false, _) => insert_column(buffer, rect, "// ", TAB_WIDTH),
            };
            let sets_before = mark_sets.get();
            if attached {
                let cost = edit_cost(&fx.view, apply)?;
                insert.push(cost.edit_ms);
                costs.push(cost);
            } else {
                let started = Instant::now();
                apply();
                insert.push(ms(started.elapsed()));
            }
            let after = text(buffer);
            let mark_after_edit = buffer.iter_at_mark(&mark).line();
            correct &= after == expected;
            let sets_after_edit = mark_sets.get();
            let started = Instant::now();
            buffer.undo();
            undo.push(ms(started.elapsed()));
            if rep == 0 {
                mark_sets_edit = sets_after_edit - sets_before;
                mark_sets_undo = mark_sets.get() - sets_after_edit;
            }
            let undone = text(buffer);
            let mark_after_undo = buffer.iter_at_mark(&mark).line();
            undo_ok &= undone == fx.original && !buffer.can_undo();
            let started = Instant::now();
            buffer.redo();
            redo.push(ms(started.elapsed()));
            let redone = text(buffer);
            redo_ok &= redone == expected;
            buffer.delete_mark(&mark);
            if rep == 0 {
                hashes = json!({
                    "original": hash(&fx.original),
                    "expected": hash(&expected),
                    "after_insert": hash(&after),
                    "after_undo": hash(&undone),
                    "after_redo": hash(&redone),
                });
                mark_lines = json!({ "before": 5000, "after_edit": mark_after_edit, "after_undo": mark_after_undo });
            }
        }
        buffer.disconnect(counter);
        let insert = Summary::of(&insert);
        let (undo, redo) = (Summary::of(&undo), Summary::of(&redo));
        let costs = (!costs.is_empty()).then(|| Costs::of(&costs));
        report.record(
            &format!("column_insert[{label}]"),
            json!({
                "insert_ms": insert.json(),
                "after_edit": costs.as_ref().map(Costs::json),
                "undo_ms": undo.json(),
                "redo_ms": redo.json(),
                "correct": correct, "undo_ok": undo_ok, "redo_ok": redo_ok,
                "hashes": hashes,
                "mark_on_line_5000": mark_lines,
                "mark_set_emissions": { "edit": mark_sets_edit, "undo": mark_sets_undo },
            }),
        );
        rows.push(Row::new(
            format!(
                "Insert \"// \" at a visual column across 10k lines as one user action ({label})"
            ),
            format!("{}; text matches model: {correct}", insert.text("ms")),
            "< 200 ms",
            pass(correct && insert.max < INSERT_BAR_MS),
        ));
        rows.push(Row::new(
            format!("One undo restores the original (hash), redo re-applies ({label})"),
            format!(
                "undo ok: {undo_ok}, {}; redo ok: {redo_ok}, {}; a mark on line 5000 is on line {} after the edit and {} after undo",
                undo.text("ms"),
                redo.text("ms"),
                mark_lines["after_edit"],
                mark_lines["after_undo"]
            ),
            "1 step, hash equal",
            pass(undo_ok && redo_ok),
        ));
        if let Some(costs) = costs {
            rows.push(Row::new(
                format!("UI cost after the edit ({label})"),
                costs.after_edit_text(),
                "not set by the plan",
                None,
            ));
        }
        if attached && col == 10 {
            let (mut aligned, mut snapped) = (0, 0);
            for (index, line) in fx.lines.iter().enumerate() {
                let (byte, reached) = locate(line, col, TAB_WIDTH);
                let pad = if byte == line.len() { col - reached } else { 0 };
                let offset = line[..byte].chars().count() as i32 + pad as i32;
                let iter = buffer
                    .iter_at_line_offset(index as i32, offset)
                    .context("offset out of range")?;
                if fx.view.visual_column(&iter) == col {
                    aligned += 1;
                } else {
                    snapped += 1;
                }
            }
            report.record(
                "inserted_text_columns",
                json!({ "at_col_10": aligned, "snapped_to_tab_start": snapped }),
            );
            rows.push(Row::new(
                "GtkSourceView's visual column of the inserted text (col 10)",
                format!("{aligned} lines at column 10, {snapped} snapped to the start of a tab spanning it"),
                "only lines with a tab across the column snap",
                pass(aligned + snapped == LINES && snapped == (0..LINES).filter(|i| i % 7 == 3).count()),
            ));
        }
    }
    Ok(())
}

/// Every line after typing `typed` at `col` and then pressing Backspace `backspaces` times.
fn typed_model(lines: &[String], col: u32, typed: &str, backspaces: u32) -> String {
    map_lines(lines, |line| {
        let mut line = line.to_owned();
        let mut at = col;
        for ch in typed.chars() {
            let key = ch.to_string();
            line = insert_in_line(&line, at, &key, TAB_WIDTH);
            at += text_width(&key, at, TAB_WIDTH);
        }
        for _ in 0..backspaces {
            line = delete_in_line(&line, at - 1, at, TAB_WIDTH);
            at -= 1;
        }
        line
    })
    .join("\n")
}

fn check_typing(fx: &Fixture, report: &Report, rows: &mut Vec<Row>) -> Result<()> {
    let view = &fx.view;
    let buffer = &fx.buffer;
    fx.reset_to(&fx.original)?;
    view.set_rect(Some(Rect::new((LAST, 10), (0, 10))));
    buffer.place_cursor(&fx.line_end(0)?);
    let mut costs = Vec::new();
    let mut refused = 0;
    for key in ["a", "b", "c", "d", "e"] {
        costs.push(edit_cost(view, || {
            if !buffer.insert_interactive_at_cursor(key, true) {
                refused += 1;
            }
        })?);
    }
    let typed = typed_model(&fx.lines, 10, "abcde", 0);
    let typed_ok = refused == 0 && text(buffer) == typed;
    let typed_diff = first_diff(&text(buffer), &typed);
    let rect_after = view.rect();
    let guarded_before = view.imp().guarded_history_edits.get();
    let (undo_steps, undo_ok) = undo_until(buffer, &fx.original, 50);
    let guarded = view.imp().guarded_history_edits.get() - guarded_before;
    let (redo_steps, redo_ok) = redo_until(buffer, &typed, 50);

    view.set_rect(Some(Rect::new((LAST, 15), (0, 15))));
    buffer.place_cursor(&fx.line_end(0)?);
    let started = Instant::now();
    view.emit_insert_at_cursor("f");
    let view_insert_ms = ms(started.elapsed());
    let view_insert_ok = text(buffer) == typed_model(&fx.lines, 10, "abcdef", 0);
    let mut cursor = buffer.iter_at_mark(&buffer.get_insert());
    let started = Instant::now();
    let deleted = buffer.backspace(&mut cursor, true, true);
    let buffer_backspace_ms = ms(started.elapsed());
    let buffer_backspace_ok = deleted && text(buffer) == typed_model(&fx.lines, 10, "abcdef", 1);
    let started = Instant::now();
    view.emit_backspace();
    let view_backspace_ms = ms(started.elapsed());
    let expected_final = typed_model(&fx.lines, 10, "abcdef", 2);
    let view_backspace_ok = text(buffer) == expected_final;
    let backspace_diff = first_diff(&text(buffer), &expected_final);
    let (final_undo_steps, final_undo_ok) = undo_until(buffer, &fx.original, 50);

    fx.reset_to(&fx.original)?;
    buffer.place_cursor(&fx.line_end(1)?);
    for key in ["a", "b", "c", "d", "e"] {
        buffer.insert_interactive_at_cursor(key, true);
    }
    let (plain_steps, plain_ok) = undo_until(buffer, &fx.original, 50);

    fx.reset_to(&fx.original)?;
    view.set_group_typing(true);
    view.set_rect(Some(Rect::new((LAST, 10), (0, 10))));
    buffer.place_cursor(&fx.line_end(0)?);
    for key in ["a", "b", "c", "d", "e"] {
        buffer.insert_interactive_at_cursor(key, true);
    }
    view.emit_backspace();
    let grouped = typed_model(&fx.lines, 10, "abcde", 1);
    let grouped_typed_ok = text(buffer) == grouped;
    let can_undo_while_open = buffer.can_undo();
    view.set_rect(None);
    let (grouped_steps, grouped_undo_ok) = undo_until(buffer, &fx.original, 50);
    let (grouped_redo_steps, grouped_redo_ok) = redo_until(buffer, &grouped, 50);

    fx.reset_to(&fx.original)?;
    buffer.place_cursor(&fx.line_end(1)?);
    buffer.insert_interactive_at_cursor("x", true);
    let before_burst = text(buffer);
    view.set_rect(Some(Rect::new((LAST, 10), (0, 10))));
    buffer.place_cursor(&fx.line_end(0)?);
    for key in ["a", "b", "c"] {
        buffer.insert_interactive_at_cursor(key, true);
    }
    let burst = text(buffer);
    let can_undo_while_open_with_history = buffer.can_undo();
    buffer.undo();
    let undo_while_open_is_noop = text(buffer) == burst;
    view.close_session();
    buffer.undo();
    let close_then_undo_ok = text(buffer) == before_burst;
    view.set_group_typing(false);
    view.set_rect(None);

    let costs = Costs::of(&costs);
    report.record(
        "typing",
        json!({
            "keystrokes": costs.json(),
            "typed_ok": typed_ok,
            "typed_first_diff": typed_diff,
            "rect_after": format!("{rect_after:?}"),
            "undo_steps": undo_steps, "undo_ok": undo_ok,
            "history_edits_seen_under_guard": guarded,
            "redo_steps": redo_steps, "redo_ok": redo_ok,
            "view_insert_at_cursor": { "ms": view_insert_ms, "ok": view_insert_ok },
            "buffer_backspace": { "ms": buffer_backspace_ms, "ok": buffer_backspace_ok },
            "view_backspace": { "ms": view_backspace_ms, "ok": view_backspace_ok, "first_diff": backspace_diff },
            "undo_steps_after_6_inserts_2_backspaces": final_undo_steps, "final_undo_ok": final_undo_ok,
            "max_undo_levels": buffer.max_undo_levels(),
            "plain_typing_undo_steps": plain_steps, "plain_typing_undo_ok": plain_ok,
            "grouped": {
                "typed_ok": grouped_typed_ok,
                "can_undo_while_open_without_prior_history": can_undo_while_open,
                "undo_steps": grouped_steps, "undo_ok": grouped_undo_ok,
                "redo_steps": grouped_redo_steps, "redo_ok": grouped_redo_ok,
                "can_undo_while_open_with_prior_history": can_undo_while_open_with_history,
                "undo_while_open_is_noop": undo_while_open_is_noop,
                "close_then_undo_removes_exactly_the_burst": close_then_undo_ok,
            },
        }),
    );
    rows.push(Row::new(
        "Typed text replicated via insert-text (5 × insert_interactive_at_cursor, the IM commit call)",
        format!(
            "all 10k lines match: {typed_ok}; rectangle advanced to column {:?}",
            rect_after.map(|rect| rect.left())
        ),
        "works",
        pass(typed_ok),
    ));
    rows.push(Row::new(
        "Per-keystroke replication across 10k lines",
        format!("{}; {}", costs.edit.text("ms"), costs.after_edit_text()),
        "not set by the plan",
        None,
    ));
    rows.push(Row::new(
        "Undo steps for 5 replicated keystrokes",
        format!(
            "{undo_steps} steps back to the original (ok: {undo_ok}); redo {redo_steps} steps (ok: {redo_ok}); \
             {guarded} history edits passed the undo guard"
        ),
        "1 step",
        pass(undo_ok && redo_ok && undo_steps == 1),
    ));
    rows.push(Row::new(
        "Reference: the same 5 keystrokes typed on one line without a rectangle",
        format!("{plain_steps} undo step(s) (ok: {plain_ok})"),
        "reference",
        None,
    ));
    rows.push(Row::new(
        "Grouped typing: one user action held open from the first replicated keystroke until column mode ends",
        format!(
            "5 keys + Backspace replicated: {grouped_typed_ok}; {grouped_steps} undo step(s) (ok: {grouped_undo_ok}); \
             redo {grouped_redo_steps} (ok: {grouped_redo_ok}); while the group is open can_undo is \
             {can_undo_while_open} (no earlier history) / {can_undo_while_open_with_history} (with earlier history) and \
             undo is a no-op: {undo_while_open_is_noop}; closing the group, then undo, removes exactly the burst: {close_then_undo_ok}"
        ),
        "1 step",
        pass(
            grouped_typed_ok
                && grouped_undo_ok
                && grouped_redo_ok
                && grouped_steps == 1
                && close_then_undo_ok,
        ),
    ));
    rows.push(Row::new(
        "Key-binding path: the view's insert-at-cursor signal",
        format!("replicated: {view_insert_ok} ({view_insert_ms:.2} ms)"),
        "works",
        pass(view_insert_ok),
    ));
    rows.push(Row::new(
        "Backspace replicated via delete-range (buffer.backspace, then the view's backspace signal)",
        format!(
            "buffer.backspace: {buffer_backspace_ok} ({buffer_backspace_ms:.2} ms); \
             view backspace: {view_backspace_ok} ({view_backspace_ms:.2} ms); \
             {final_undo_steps} undo steps back to the original (ok: {final_undo_ok})"
        ),
        "works",
        pass(buffer_backspace_ok && view_backspace_ok && final_undo_ok),
    ));
    Ok(())
}

fn check_block_delete(fx: &Fixture, report: &Report, rows: &mut Vec<Row>) -> Result<()> {
    let inserted = map_lines(&fx.lines, |line| {
        insert_in_line(line, 10, "abcde", TAB_WIDTH)
    });
    let inserted_text = inserted.join("\n");
    let expected = inserted
        .iter()
        .map(|line| delete_in_line(line, 10, 15, TAB_WIDTH))
        .collect::<Vec<_>>()
        .join("\n");
    fx.reset_to(&inserted_text)?;
    let (mut times, mut correct, mut undo_ok) = (vec![], true, true);
    for _ in 0..REPS {
        fx.view.set_rect(Some(Rect::new((0, 10), (LAST, 15))));
        let started = Instant::now();
        fx.view.delete_block();
        times.push(ms(started.elapsed()));
        correct &= text(&fx.buffer) == expected;
        fx.buffer.undo();
        undo_ok &= text(&fx.buffer) == inserted_text && !fx.buffer.can_undo();
    }
    let times = Summary::of(&times);
    report.record(
        "block_delete",
        json!({ "ms": times.json(), "correct": correct, "undo_ok": undo_ok }),
    );
    rows.push(Row::new(
        "Delete a 5-column block across 10k lines, one user action",
        format!(
            "{}; matches model: {correct}; one undo restores: {undo_ok}",
            times.text("ms")
        ),
        "< 200 ms, 1 undo step",
        pass(correct && undo_ok && times.max < INSERT_BAR_MS),
    ));
    Ok(())
}

fn check_clipboard(fx: &Fixture, report: &Report, rows: &mut Vec<Row>) -> Result<()> {
    let view = &fx.view;
    let buffer = &fx.buffer;
    let inserted = map_lines(&fx.lines, |line| {
        insert_in_line(line, 10, "abcde", TAB_WIDTH)
    });
    let inserted_text = inserted.join("\n");
    let block: Vec<&str> = inserted
        .iter()
        .map(|line| block_in_line(line, 10, 15, TAB_WIDTH))
        .collect();
    fx.reset_to(&inserted_text)?;
    view.set_rect(Some(Rect::new((0, 10), (LAST, 15))));
    view.copy_block()?;
    let clipboard = view.clipboard();
    let formats = clipboard.formats();
    let has_mime = formats.contain_mime_type(COLUMN_MIME);
    let plain = block_on(glib::clone!(
        #[strong]
        clipboard,
        async move { clipboard.read_text_future().await }
    ))?
    .context("reading text/plain")?
    .map(|text| text.to_string());
    let plain_ok = plain.as_deref() == Some(block.join("\n").as_str());

    let pasted = inserted
        .iter()
        .zip(&block)
        .map(|(line, row)| insert_in_line(line, 40, row, TAB_WIDTH))
        .collect::<Vec<_>>()
        .join("\n");
    let (mut times, mut correct, mut undo_ok) = (vec![], true, true);
    for _ in 0..REPS {
        view.set_rect(Some(Rect::new((0, 40), (0, 40))));
        let before = view.pastes();
        let started = Instant::now();
        view.emit_paste_clipboard();
        run_main_loop_until(TIMEOUT, || view.pastes() > before)?;
        times.push(ms(started.elapsed()));
        correct &= text(buffer) == pasted;
        buffer.undo();
        undo_ok &= text(buffer) == inserted_text && !buffer.can_undo();
    }

    clipboard.set_text("plain paste");
    view.set_rect(None);
    buffer.place_cursor(&buffer.iter_at_line(1).context("line 1")?);
    view.emit_paste_clipboard();
    let fallback_expected = {
        let mut lines = inserted.clone();
        lines[1] = format!("plain paste{}", lines[1]);
        lines.join("\n")
    };
    let fallback_ok = run_main_loop_until(TIMEOUT, || text(buffer) == fallback_expected).is_ok();
    buffer.undo();

    let times = Summary::of(&times);
    report.record(
        "clipboard",
        json!({
            "formats": formats.to_str().to_string(),
            "has_column_mime": has_mime,
            "plain_text_ok": plain_ok,
            "paste_ms": times.json(),
            "paste_correct": correct,
            "paste_undo_ok": undo_ok,
            "plain_fallback_ok": fallback_ok,
        }),
    );
    rows.push(Row::new(
        "Rectangular clipboard: union ContentProvider with a custom MIME type plus text/plain",
        format!(
            "formats {}; text/plain reads back the joined block: {plain_ok}",
            formats.to_str()
        ),
        "both formats offered",
        pass(has_mime && plain_ok),
    ));
    rows.push(Row::new(
        "Paste a 10k-row block at column 40 through the paste_clipboard override (async read included)",
        format!(
            "{}; matches model: {correct}; one undo restores: {undo_ok}; plain-text paste falls back to GTK: {fallback_ok}",
            times.text("ms")
        ),
        "one undo step",
        pass(correct && undo_ok && fallback_ok),
    ));
    Ok(())
}

fn run(report: &Report) -> Result<Vec<Row>> {
    let lines = make_lines(LINES);
    let original = lines.join("\n");
    let buffer = sourceview5::Buffer::new(None);
    reset(&buffer, &original);
    let view = ColumnView::new(&buffer);
    let scrolled = gtk::ScrolledWindow::builder().child(&view).build();
    let window = gtk::Window::builder()
        .title("S5 column")
        .default_width(900)
        .default_height(700)
        .child(&scrolled)
        .build();
    window.present();
    run_main_loop_until(TIMEOUT, || view.is_mapped())?;
    wait_frames(&view, 3, TIMEOUT)?;
    report.record(
        "environment",
        json!({
            "gtk": format!("{}.{}.{}", gtk::major_version(), gtk::minor_version(), gtk::micro_version()),
            "view_size": [view.width(), view.height()],
            "tab_width": view.tab_width(),
        }),
    );

    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.local");
    std::fs::create_dir_all(&local).context("creating .local")?;
    let fx = Fixture {
        view,
        buffer,
        lines,
        original,
        local,
    };
    let mut rows = Vec::new();
    check_painting(&fx, report, &mut rows)?;
    check_baseline(&fx, report, &mut rows)?;
    check_column_insert(&fx, report, &mut rows)?;
    check_typing(&fx, report, &mut rows)?;
    check_block_delete(&fx, report, &mut rows)?;
    check_clipboard(&fx, report, &mut rows)?;
    let tabs_at_end = fx.view.tab_stops();
    fx.view.set_tab_width(8);
    fx.view.set_tab_width(TAB_WIDTH);
    wait_frames(&fx.view, 1, TIMEOUT)?;
    let tabs_after_change = fx.view.tab_stops();
    report.record(
        "tab_stops_persistence",
        json!({ "end_of_run": tabs_at_end, "after_tab_width_change": tabs_after_change }),
    );
    rows.push(Row::new(
        "Exact tab stops at the end of the run / after a tab-width change",
        format!("{tabs_at_end} / {tabs_after_change}"),
        "diagnostic",
        None,
    ));
    let warnings = WARNINGS.load(Ordering::Relaxed);
    report.record("glib_warnings", warnings);
    rows.push(Row::new(
        "GLib/GTK warnings and criticals during the run",
        warnings.to_string(),
        "0",
        pass(warnings == 0),
    ));
    rows.push(Row::new(
        "fcitx5 preedit and commit in column mode",
        "MANUAL: needs the live Hyprland session",
        "works",
        None,
    ));
    window.destroy();
    Ok(rows)
}

fn main() -> Result<()> {
    stet_spikes::require_headless()?;
    glib::log_set_writer_func(|level, fields| {
        if matches!(
            level,
            glib::LogLevel::Warning | glib::LogLevel::Critical | glib::LogLevel::Error
        ) {
            WARNINGS.fetch_add(1, Ordering::Relaxed);
        }
        glib::log_writer_default(level, fields)
    });
    std::thread::spawn(|| {
        std::thread::sleep(WATCHDOG);
        println!(r#"{{"spike":"s5_column","key":"watchdog","value":"timeout"}}"#);
        std::process::exit(124);
    });
    gtk::init().context("initializing GTK")?;
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.local/s5-column.md"));
    let report = Report::new("s5_column");
    let rows = run(&report)?;
    write_markdown(&out, "S5 column mode (generated)", &rows, &CAVEATS)?;
    let failed = rows.iter().filter(|row| row.passed == Some(false)).count();
    report.record("failed_rows", failed);
    Ok(())
}
