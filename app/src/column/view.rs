//! `StetView`: the editor's GtkSourceView with column mode. The rectangle is an anchor and a
//! cursor in visual columns (virtual space allowed). While it is active the GTK caret sits at
//! the cursor corner (clamped to the text), the GTK selection is empty and GTK's caret is
//! hidden; the view paints the rectangle itself.

use super::{Motion, a11y, clipboard, edit, keys, paint};
use gtk4 as gtk;
use gtk4::glib;
use gtk4::glib::subclass::Signal;
use gtk4::subclass::prelude::*;
use sourceview5::prelude::*;
use sourceview5::subclass::prelude::*;
use std::cell::{Cell, OnceCell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;
use stet_domain::column::{
    ColumnEditor, ColumnError, Rect, VisualPos, backspace, delete_forward, home_column, locate,
    text_width, type_text, visual_column,
};
use stet_domain::marks::Footprint;
use stet_domain::text::TextEdit;

/// How long a column typing burst stays open without a keystroke (ADR-016).
pub const BURST_IDLE: Duration = Duration::from_millis(1000);

/// The furthest column a cursor may reach, past the end of its line.
const MAX_COLUMN: usize = 1 << 16;

/// Keeps a document's bookmarks and marks across a large column edit (M7): called with the
/// edit's lines, its edits in the buffer's offsets before it, and what applies it, which
/// returns the footprint of the replacements it made.
pub type ColumnTracker = Rc<dyn Fn(Range<usize>, &[TextEdit], &mut dyn FnMut() -> Footprint)>;

/// Blank space a comparison puts between lines so that equal lines face each other (M7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pad {
    /// The view's top margin, above the first line.
    Above(i32),
    /// Space below `line`, from a tag's `pixels-below-lines`.
    Below { line: usize, pixels: i32 },
    /// The view's bottom margin, after the last line.
    End(i32),
}

/// How column mode ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collapse {
    /// The caret at the cursor corner, clamped to the text.
    Cursor,
    /// A stream selection from the anchor corner to the cursor corner.
    Stream,
    /// The caret and the selection as they are: something else just moved them.
    Keep,
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct StetView {
        pub rect: Cell<Option<Rect>>,
        /// Above zero while column mode edits the buffer or moves the caret itself, so the
        /// buffer hooks let those changes through.
        pub busy: Cell<u32>,
        /// A typing burst holds a user action open (ADR-016).
        pub burst: Cell<bool>,
        pub burst_timer: RefCell<Option<glib::SourceId>>,
        /// Begin/End Select: the line (a mark, so edits above move it) and the column.
        pub begin: RefCell<Option<(gtk::TextMark, usize)>>,
        /// An Alt+drag is in progress.
        pub dragging: Cell<bool>,
        pub keys: OnceCell<gtk::EventControllerKey>,
        pub painted_rows: Cell<usize>,
        pub last_op: Cell<Option<edit::OpStats>>,
        /// What this view shares with the other views of its buffer (M7).
        pub shared: OnceCell<Rc<edit::BufferShared>>,
        /// The document's bookmarks and marks, for large column edits (M7).
        pub tracker: RefCell<Option<ColumnTracker>>,
        /// The padding a comparison put between lines, painted over (M7).
        pub pads: RefCell<Vec<Pad>>,
        /// The empty lines a comparison colours, in order, with the scheme style of their
        /// background, painted here because GTK paints no paragraph background on a line
        /// without characters (M7).
        pub empty_lines: RefCell<Vec<(usize, &'static str)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for StetView {
        const NAME: &'static str = "StetView";
        type Type = super::StetView;
        type ParentType = sourceview5::View;

        fn type_init(_type_: &mut glib::subclass::InitializingType<Self>) {
            // SAFETY: `type_init` runs once, after the type is registered and before its class
            // is initialized, which is when an interface may be added.
            unsafe {
                let type_ = Self::type_data().as_ref().type_();
                a11y::override_accessible_text(type_);
            }
        }
    }

    impl ObjectImpl for StetView {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| vec![Signal::builder("column-changed").build()])
        }

        fn constructed(&self) {
            self.parent_constructed();
            keys::connect(&self.obj());
        }

        fn dispose(&self) {
            self.obj().close_burst();
        }
    }

    impl WidgetImpl for StetView {}

    impl TextViewImpl for StetView {
        fn snapshot_layer(&self, layer: gtk::TextViewLayer, snapshot: gtk::Snapshot) {
            self.parent_snapshot_layer(layer, snapshot.clone());
            if layer == gtk::TextViewLayer::BelowText && !self.empty_lines.borrow().is_empty() {
                paint::empty_lines(&self.obj(), &self.empty_lines.borrow(), &snapshot);
            }
            if layer == gtk::TextViewLayer::BelowText
                && let Some(rect) = self.rect.get()
            {
                self.painted_rows
                    .set(paint::paint(&self.obj(), rect, &snapshot));
            }
            // Above the text, so a line's own background does not cover the padding.
            if layer == gtk::TextViewLayer::AboveText && !self.pads.borrow().is_empty() {
                paint::pads(&self.obj(), &self.pads.borrow(), &snapshot);
            }
        }

        fn backspace(&self) {
            if !self.obj().column_backspace() {
                self.parent_backspace();
            }
        }

        fn delete_from_cursor(&self, type_: gtk::DeleteType, count: i32) {
            let view = self.obj();
            if view.rect().is_some() && type_ == gtk::DeleteType::Chars && count == 1 {
                view.delete_forward_or_block(true);
                return;
            }
            view.end_column_mode(Collapse::Cursor);
            self.parent_delete_from_cursor(type_, count);
        }

        fn copy_clipboard(&self) {
            if !clipboard::copy(&self.obj(), false) {
                self.parent_copy_clipboard();
            }
        }

        fn cut_clipboard(&self) {
            if !clipboard::copy(&self.obj(), true) {
                self.parent_cut_clipboard();
            }
        }

        fn paste_clipboard(&self) {
            if !clipboard::paste(&self.obj()) {
                self.parent_paste_clipboard();
            }
        }

        /// A movement that is not column-aware leaves column mode from the cursor corner.
        fn move_cursor(&self, step: gtk::MovementStep, count: i32, extend: bool) {
            self.obj().end_column_mode(Collapse::Cursor);
            self.parent_move_cursor(step, count, extend);
        }
    }

    impl ViewImpl for StetView {}
}

glib::wrapper! {
    pub struct StetView(ObjectSubclass<imp::StetView>)
        @extends sourceview5::View, gtk::TextView, gtk::Widget,
        @implements gtk::Accessible, gtk::AccessibleText, gtk::Buildable, gtk::ConstraintTarget,
            gtk::Scrollable;
}

/// Builds a [`StetView`] like `sourceview5::View::builder()`. The buffer is set last, after
/// column mode has connected its buffer hooks, so they run before GtkTextView's and
/// GtkSourceView's own handlers.
pub struct StetViewBuilder {
    buffer: Option<(sourceview5::Buffer, Rc<edit::BufferShared>)>,
    builder: glib::object::ObjectBuilder<'static, StetView>,
}

impl StetViewBuilder {
    /// The buffer, and what its views share ([`edit::connect_history`]).
    pub fn buffer(mut self, buffer: &sourceview5::Buffer, shared: &Rc<edit::BufferShared>) -> Self {
        self.buffer = Some((buffer.clone(), shared.clone()));
        self
    }

    fn property(mut self, name: &'static str, value: impl Into<glib::Value>) -> Self {
        self.builder = self.builder.property(name, value);
        self
    }

    pub fn monospace(self, monospace: bool) -> Self {
        self.property("monospace", monospace)
    }

    pub fn show_line_numbers(self, show: bool) -> Self {
        self.property("show-line-numbers", show)
    }

    pub fn highlight_current_line(self, highlight: bool) -> Self {
        self.property("highlight-current-line", highlight)
    }

    pub fn auto_indent(self, auto_indent: bool) -> Self {
        self.property("auto-indent", auto_indent)
    }

    pub fn tab_width(self, width: u32) -> Self {
        self.property("tab-width", width)
    }

    pub fn indent_width(self, width: i32) -> Self {
        self.property("indent-width", width)
    }

    pub fn insert_spaces_instead_of_tabs(self, spaces: bool) -> Self {
        self.property("insert-spaces-instead-of-tabs", spaces)
    }

    pub fn smart_home_end(self, smart: sourceview5::SmartHomeEndType) -> Self {
        self.property("smart-home-end", smart)
    }

    pub fn left_margin(self, margin: i32) -> Self {
        self.property("left-margin", margin)
    }

    pub fn hexpand(self, expand: bool) -> Self {
        self.property("hexpand", expand)
    }

    pub fn vexpand(self, expand: bool) -> Self {
        self.property("vexpand", expand)
    }

    pub fn build(self) -> StetView {
        let view = self.builder.build();
        if let Some((buffer, shared)) = self.buffer {
            let _ = view.imp().shared.set(shared);
            edit::connect_buffer(&view, &buffer);
            view.set_buffer(Some(&buffer));
        }
        view
    }
}

impl StetView {
    pub fn builder() -> StetViewBuilder {
        StetViewBuilder {
            buffer: None,
            builder: glib::Object::builder(),
        }
    }

    pub fn source_buffer(&self) -> sourceview5::Buffer {
        self.buffer().downcast().expect("a GtkSourceBuffer")
    }

    /// The rectangle, while column mode is on.
    pub fn rect(&self) -> Option<Rect> {
        self.imp().rect.get()
    }

    /// Runs when the rectangle changes or column mode ends.
    pub fn connect_column_changed<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_local("column-changed", false, move |values| {
            let view = values[0].get::<Self>().expect("the emitter");
            f(&view);
            None
        })
    }

    /// Rows painted in the last frame, for the self-test.
    pub fn painted_rows(&self) -> usize {
        self.imp().painted_rows.get()
    }

    /// The last column operation's size and cost, for the self-test.
    pub fn last_op(&self) -> Option<edit::OpStats> {
        self.imp().last_op.get()
    }

    pub(super) fn record_op(&self, stats: edit::OpStats) {
        self.imp().last_op.set(Some(stats));
    }

    /// The key controller column mode uses, for the self-test.
    pub fn column_keys(&self) -> gtk::EventControllerKey {
        self.imp().keys.get().expect("constructed").clone()
    }

    pub(super) fn set_column_keys(&self, controller: gtk::EventControllerKey) {
        let _ = self.imp().keys.set(controller);
    }

    pub(super) fn tab(&self) -> usize {
        self.tab_width().max(1) as usize
    }

    /// The width of one cell (a space) in pixels.
    pub(super) fn advance(&self) -> f64 {
        f64::from(crate::tabstops::space_advance(self)) / f64::from(gtk::pango::SCALE)
    }

    pub(super) fn last_line(&self) -> usize {
        (self.buffer().line_count() - 1).max(0) as usize
    }

    /// Runs `f` with the buffer hooks letting its edits and caret moves through.
    pub(super) fn busy<T>(&self, f: impl FnOnce() -> T) -> T {
        let busy = &self.imp().busy;
        busy.set(busy.get() + 1);
        let result = f();
        busy.set(busy.get() - 1);
        result
    }

    pub(super) fn is_busy(&self) -> bool {
        self.imp().busy.get() > 0
    }

    /// The padding to paint over, after a comparison set it (M7).
    pub fn set_pads(&self, pads: Vec<Pad>) {
        self.imp().pads.replace(pads);
        self.queue_draw();
    }

    pub fn pads(&self) -> Vec<Pad> {
        self.imp().pads.borrow().clone()
    }

    /// The empty lines to paint in their scheme style's background, in order, after a
    /// comparison set them (M7).
    pub fn set_empty_lines(&self, lines: Vec<(usize, &'static str)>) {
        self.imp().empty_lines.replace(lines);
        self.queue_draw();
    }

    pub fn set_column_tracker(&self, tracker: ColumnTracker) {
        self.imp().tracker.replace(Some(tracker));
    }

    pub(super) fn column_tracker(&self) -> Option<ColumnTracker> {
        self.imp().tracker.borrow().clone()
    }

    /// Runs `f` with this view's buffer hooks letting caret moves and edits through, as
    /// column mode's own do: another view of the document putting back its caret (M7).
    pub fn quietly<T>(&self, f: impl FnOnce() -> T) -> T {
        self.busy(f)
    }

    /// The text of `line`, without its line break.
    pub fn line_text(&self, line: usize) -> String {
        let buffer = self.buffer();
        let Some(start) = buffer.iter_at_line(line as i32) else {
            return String::new();
        };
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        buffer.text(&start, &end, true).to_string()
    }

    /// The line and visual column of `iter`.
    pub fn visual_pos(&self, iter: &gtk::TextIter) -> VisualPos {
        VisualPos::new(
            iter.line().max(0) as usize,
            ViewExt::visual_column(self, iter) as usize,
        )
    }

    /// The character at or containing `pos`, or its line's end when it is in virtual space.
    pub fn iter_at_pos(&self, pos: VisualPos) -> gtk::TextIter {
        let buffer = self.buffer();
        let line = pos.line.min(self.last_line());
        let at = locate(&self.line_text(line), pos.column, self.tab());
        buffer
            .iter_at_line_offset(line as i32, at.char_offset as i32)
            .unwrap_or_else(|| buffer.end_iter())
    }

    fn caret_pos(&self) -> VisualPos {
        let buffer = self.buffer();
        self.visual_pos(&buffer.iter_at_mark(&buffer.get_insert()))
    }

    /// The rectangle a column-select key starts from: the GTK selection's bound to its caret.
    fn rect_from_selection(&self) -> Rect {
        let buffer = self.buffer();
        let bound = self.visual_pos(&buffer.iter_at_mark(&buffer.selection_bound()));
        Rect::new(bound, self.caret_pos())
    }

    /// Shows `rect` and puts the GTK caret at its cursor corner.
    pub fn set_rect(&self, rect: Rect) {
        let last = self.last_line();
        let clamp = |pos: VisualPos| VisualPos::new(pos.line.min(last), pos.column.min(MAX_COLUMN));
        let rect = Rect::new(clamp(rect.anchor), clamp(rect.cursor));
        if self.imp().rect.replace(Some(rect)).is_none() {
            self.set_cursor_visible(false);
            if let Some(shared) = self.imp().shared.get() {
                shared.take_column(self);
            }
        }
        self.place_caret(rect.cursor);
        self.queue_draw();
        self.emit_by_name::<()>("column-changed", &[]);
    }

    pub(super) fn place_caret(&self, pos: VisualPos) {
        let iter = self.iter_at_pos(pos);
        self.busy(|| self.buffer().place_cursor(&iter));
    }

    /// Leaves column mode; nothing happens outside it.
    pub fn end_column_mode(&self, how: Collapse) {
        let Some(rect) = self.imp().rect.take() else {
            return;
        };
        self.close_burst();
        match how {
            Collapse::Cursor => self.place_caret(rect.cursor),
            Collapse::Stream => {
                let anchor = self.iter_at_pos(rect.anchor);
                let cursor = self.iter_at_pos(rect.cursor);
                self.busy(|| self.buffer().select_range(&cursor, &anchor));
            }
            Collapse::Keep => {}
        }
        self.set_cursor_visible(true);
        self.queue_draw();
        self.emit_by_name::<()>("column-changed", &[]);
    }

    /// Opens a typing burst, or keeps it open for another [`BURST_IDLE`].
    pub(super) fn open_burst(&self) {
        let imp = self.imp();
        if !imp.burst.replace(true) {
            self.buffer().begin_user_action();
            keys::set_open_burst(self);
        }
        if let Some(source) = imp.burst_timer.take() {
            source.remove();
        }
        let weak = self.downgrade();
        let source = glib::timeout_add_local_once(BURST_IDLE, move || {
            if let Some(view) = weak.upgrade() {
                view.imp().burst_timer.take();
                view.close_burst();
            }
        });
        imp.burst_timer.replace(Some(source));
    }

    /// Closes the typing burst, so that it is one undo step and Undo works again.
    pub fn close_burst(&self) {
        let imp = self.imp();
        if let Some(source) = imp.burst_timer.take() {
            source.remove();
        }
        if imp.burst.replace(false) {
            keys::clear_open_burst(self);
            self.buffer().end_user_action();
        }
    }

    pub fn burst_open(&self) -> bool {
        self.imp().burst.get()
    }

    /// Alt+Shift+arrow and friends: moves the cursor corner, starting from the GTK selection
    /// when column mode is off.
    pub fn extend(&self, motion: Motion) {
        self.close_burst();
        let rect = self.rect().unwrap_or_else(|| self.rect_from_selection());
        let last = self.last_line();
        let tab = self.tab();
        let mut cursor = rect.cursor;
        match motion {
            Motion::Left => cursor.column = cursor.column.saturating_sub(1),
            Motion::Right => cursor.column = (cursor.column + 1).min(MAX_COLUMN),
            Motion::Up => cursor.line = cursor.line.saturating_sub(1),
            Motion::Down => cursor.line = (cursor.line + 1).min(last),
            Motion::LineStart => {
                cursor.column = home_column(&self.line_text(cursor.line), cursor.column, tab);
            }
            Motion::LineEnd => cursor.column = text_width(&self.line_text(cursor.line), 0, tab),
            Motion::PageUp => cursor.line = cursor.line.saturating_sub(self.page_lines()),
            Motion::PageDown => cursor.line = (cursor.line + self.page_lines()).min(last),
        }
        self.set_rect(Rect::new(rect.anchor, cursor));
        self.scroll_to_pos(cursor);
    }

    /// Lines in one page of the view.
    fn page_lines(&self) -> usize {
        let buffer = self.buffer();
        let line_height = buffer
            .iter_at_line(0)
            .map_or(1, |iter| self.line_yrange(&iter).1.max(1));
        (self.visible_rect().height() / line_height).max(1) as usize
    }

    /// Scrolls so that `pos` is on screen, virtual space included.
    pub(super) fn scroll_to_pos(&self, pos: VisualPos) {
        self.scroll_mark_onscreen(&self.buffer().get_insert());
        let Some(hadjustment) = self.hadjustment() else {
            return;
        };
        let x = paint::x_of(self, pos);
        let visible = self.visible_rect();
        let margin = 2.0 * self.advance();
        let (left, right) = (
            f64::from(visible.x()),
            f64::from(visible.x() + visible.width()),
        );
        if x + margin > right {
            hadjustment.set_value(x + margin - f64::from(visible.width()));
        } else if x < left {
            hadjustment.set_value((x - margin).max(0.0));
        }
    }

    /// Begin/End Select in Column Mode: the first use marks a corner, the second makes the
    /// rectangle from it to the caret.
    pub fn begin_end_select(&self) {
        self.close_burst();
        let buffer = self.buffer();
        if let Some((mark, column)) = self.imp().begin.take() {
            let line = buffer.iter_at_mark(&mark).line().max(0) as usize;
            buffer.delete_mark(&mark);
            let cursor = self
                .rect()
                .map_or_else(|| self.caret_pos(), |rect| rect.cursor);
            self.set_rect(Rect::new(VisualPos::new(line, column), cursor));
            return;
        }
        let begin = self
            .rect()
            .map_or_else(|| self.caret_pos(), |rect| rect.cursor);
        self.end_column_mode(Collapse::Cursor);
        let Some(line_start) = buffer.iter_at_line(begin.line as i32) else {
            return;
        };
        let mark = buffer.create_mark(None, &line_start, true);
        self.imp().begin.replace(Some((mark, begin.column)));
    }

    /// Whether Begin/End Select has marked its first corner.
    pub fn begun(&self) -> bool {
        self.imp().begin.borrow().is_some()
    }

    fn refuse_edit(&self) -> bool {
        if self.is_editable() {
            return false;
        }
        self.error_bell();
        true
    }

    /// Typed text at the column caret, or in place of the block; in overwrite mode a caret
    /// types over as many cells as the text takes. Returns the GTK caret afterwards.
    pub(super) fn column_type(&self, typed: &str) -> gtk::TextIter {
        let rect = self.rect().expect("column mode is on");
        let tab = self.tab();
        let target = if self.overwrites() && rect.is_empty() {
            let left = rect.left();
            let width = text_width(typed, left, tab);
            Rect::new((rect.anchor.line, left), (rect.cursor.line, left + width))
        } else {
            rect
        };
        self.open_burst();
        if let Err(error) = edit::apply(self, target, target.lines(), |text, index, rect| {
            type_text(text, index, rect, typed, tab)
        }) {
            tracing::warn!(%error, "column typing refused");
        }
        let buffer = self.buffer();
        buffer.iter_at_mark(&buffer.get_insert())
    }

    /// Tab in column mode: a tab, or spaces to the next indentation stop.
    pub(super) fn column_tab(&self) {
        let Some(rect) = self.rect() else {
            return;
        };
        if self.refuse_edit() {
            return;
        }
        let typed = if self.is_insert_spaces_instead_of_tabs() {
            let width = match self.indent_width() {
                width if width > 0 => width as usize,
                _ => self.tab(),
            };
            " ".repeat(width - rect.left() % width)
        } else {
            "\t".to_owned()
        };
        self.column_type(&typed);
    }

    /// Backspace in column mode; false outside it.
    pub(super) fn column_backspace(&self) -> bool {
        let Some(rect) = self.rect() else {
            return false;
        };
        if self.refuse_edit() {
            return true;
        }
        let tab = self.tab();
        self.open_burst();
        let _ = edit::apply(self, rect, rect.lines(), |text, index, rect| {
            Ok(backspace(text, index, rect, tab))
        });
        true
    }

    /// Delete in column mode: the block, or the character after the caret on every line.
    /// From the keyboard it joins the typing burst; from the menu it is a step of its own.
    pub fn delete_forward_or_block(&self, burst: bool) {
        let Some(rect) = self.rect() else {
            return;
        };
        if self.refuse_edit() {
            return;
        }
        let tab = self.tab();
        if burst {
            self.open_burst();
        } else {
            self.close_burst();
        }
        let _ = edit::apply(self, rect, rect.lines(), |text, index, rect| {
            Ok(delete_forward(text, index, rect, tab))
        });
    }

    /// The Column Editor's insert: on the rectangle's lines, or, with only a caret, from the
    /// caret line to the end of the text at the caret's column. In a rectangle the written
    /// column stays selected; with a caret, the caret stays where the text begins.
    pub fn column_editor(&self, editor: &ColumnEditor) -> Result<(), ColumnError> {
        self.close_burst();
        let tab = self.tab();
        match self.rect() {
            Some(rect) => {
                edit::apply(self, rect, rect.lines(), |text, index, rect| {
                    editor.apply(text, index, rect, tab)
                })?;
            }
            None => {
                let caret = self.caret_pos();
                let rect = Rect::new((self.last_line(), caret.column), caret);
                edit::apply(self, rect, rect.lines(), |text, index, rect| {
                    editor.apply(text, index, rect, tab)
                })?;
                self.end_column_mode(Collapse::Keep);
                self.place_caret(caret);
            }
        }
        Ok(())
    }

    /// Where a point of the widget falls, as a line and visual column (virtual space past the
    /// end of the line, cells inside a tab).
    pub fn pos_at(&self, x: f64, y: f64) -> VisualPos {
        let (bx, by) =
            self.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
        let (line_start, _) = self.line_at_y(by);
        let line = line_start.line().max(0) as usize;
        let text = self.line_text(line);
        let tab = self.tab();
        let mut end = line_start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        let end_x = f64::from(self.iter_location(&end).x());
        let width = text_width(&text, 0, tab);
        let bx = f64::from(bx);
        if bx >= end_x {
            let extra = ((bx - end_x) / self.advance()).round().max(0.0) as usize;
            return VisualPos::new(line, (width + extra).min(MAX_COLUMN));
        }
        // The character (grapheme) that contains the point; `iter_at_location` would round to
        // the nearest boundary instead, past a tab from its right half.
        let Some((iter, _)) = self.iter_at_position(bx as i32, by) else {
            return VisualPos::new(line, width);
        };
        let offset = |iter: &gtk::TextIter| iter.line_offset().max(0) as usize;
        let start = visual_column(&text, offset(&iter), tab);
        if iter.line().max(0) as usize != line {
            return VisualPos::new(line, start);
        }
        let mut next = iter;
        if !next.forward_cursor_position() || next.line() != iter.line() {
            next = end;
        }
        let cells = visual_column(&text, offset(&next), tab) - start;
        let left = f64::from(self.iter_location(&iter).x());
        let right = f64::from(self.iter_location(&next).x());
        let fraction = (bx - left) / (right - left).max(1.0);
        let into = (fraction * cells as f64).round().max(0.0) as usize;
        VisualPos::new(line, start + into.min(cells))
    }

    /// A point of the widget inside the cell at `pos`, for the self-test's drags.
    pub fn point_of(&self, pos: VisualPos) -> (f64, f64) {
        let iter = self.iter_at_pos(pos);
        let location = self.iter_location(&iter);
        let at = locate(
            &self.line_text(iter.line().max(0) as usize),
            pos.column,
            self.tab(),
        );
        let x = f64::from(location.x()) + (at.offset_cells() as f64 + 0.25) * self.advance();
        let y = f64::from(location.y()) + f64::from(location.height()) / 2.0;
        let (wx, wy) = self.buffer_to_window_coords(
            gtk::TextWindowType::Widget,
            x.round() as i32,
            y.round() as i32,
        );
        (f64::from(wx), f64::from(wy))
    }

    /// A button press at a point of the widget. Alt starts a rectangle there (Alt+Shift
    /// stretches the current one, or one from the caret); a plain press ends column mode and
    /// is GTK's. Returns whether column mode takes the press.
    pub fn pointer_begin(&self, x: f64, y: f64, alt: bool, shift: bool) -> bool {
        keys::close_open_burst();
        if !alt {
            self.end_column_mode(Collapse::Keep);
            return false;
        }
        let pos = self.pos_at(x, y);
        let anchor = match (shift, self.rect()) {
            (true, Some(rect)) => rect.anchor,
            (true, None) => self.caret_pos(),
            (false, _) => pos,
        };
        self.imp().dragging.set(true);
        self.grab_focus();
        self.set_rect(Rect::new(anchor, pos));
        true
    }

    /// The pointer moved during an Alt+drag.
    pub fn pointer_update(&self, x: f64, y: f64) {
        if !self.imp().dragging.get() {
            return;
        }
        let Some(rect) = self.rect() else {
            return;
        };
        let pos = self.pos_at(x, y);
        if pos != rect.cursor {
            self.set_rect(Rect::new(rect.anchor, pos));
            self.scroll_to_pos(pos);
        }
    }

    pub fn pointer_end(&self) {
        self.imp().dragging.set(false);
    }
}
