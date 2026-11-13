//! The search results panel at the bottom of the window: each search is a heading with its
//! files, and each file its hit lines with the matches emphasized. Find All (current or open
//! documents), Find in Files and Replace in Files fill it. Double-click or Enter on a hit
//! jumps to it; Next/Previous Search Result step through the newest search's matches. A
//! `GtkListView` over a `GtkTreeListModel` keeps large result sets cheap.
//!
//! [`commands`] holds the window's side: the panel's wiring and the commands on it.

mod commands;

pub(super) use commands::connect;

use crate::convert::{to_u32, unit_to_u16};
use crate::editor::EditorPage;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, pango};
use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::rc::Rc;
use stet_domain::text::{CharOffset, Position, counted};
use stet_infrastructure::search::Hit;

/// Searches kept in the panel; older ones are dropped.
const MAX_SEARCHES: u32 = 10;
/// The panel's height when it first opens.
const INITIAL_HEIGHT: i32 = 220;

/// Where a file's hits are.
#[derive(Clone)]
pub enum FileTarget {
    /// A file on disk (Find in Files).
    Path(PathBuf),
    /// An open document (Find All), which may also have a path to fall back on once closed.
    Page {
        page: glib::WeakRef<EditorPage>,
        path: Option<PathBuf>,
    },
}

/// A place to jump to: the match's position and its length in characters.
#[derive(Clone)]
pub struct Jump {
    pub target: FileTarget,
    pub position: Position,
    pub length: usize,
}

pub struct FileNode {
    pub label: String,
    pub target: FileTarget,
    pub hits: Vec<Hit>,
    /// Shown after the label instead of the hit count (Replace in Files: `3 replacements`,
    /// or why the file was left alone).
    pub summary: Option<String>,
}

impl FileNode {
    fn match_count(&self) -> usize {
        self.hits.iter().map(|hit| hit.match_ranges.len()).sum()
    }

    /// `label (3 hits)`, or the label with its summary.
    fn row_text(&self) -> String {
        match &self.summary {
            Some(summary) => format!("{} ({summary})", self.label),
            None => format!(
                "{} ({})",
                self.label,
                counted(self.match_count(), "hit", "hits")
            ),
        }
    }

    fn jump(&self, hit: usize, range: usize) -> Option<Jump> {
        let hit = self.hits.get(hit)?;
        let found = hit
            .match_ranges
            .get(range)
            .or_else(|| hit.match_ranges.first());
        let (column, length) = found.map_or((hit.line_offset, 0), |found| {
            let before = hit.line_text.get(..found.start).unwrap_or_default();
            let text = hit.line_text.get(found.clone()).unwrap_or_default();
            (
                hit.line_offset + before.chars().count(),
                text.chars().count(),
            )
        });
        Some(Jump {
            target: self.target.clone(),
            position: Position::new(hit.line_number.index(), column - CharOffset::ZERO),
            length,
        })
    }
}

pub struct SearchNode {
    heading: RefCell<String>,
    files: gio::ListStore,
    nodes: RefCell<Vec<Rc<FileNode>>>,
    hits: Cell<usize>,
}

impl SearchNode {
    pub fn hits(&self) -> usize {
        self.hits.get()
    }

    pub fn files(&self) -> usize {
        self.nodes.borrow().len()
    }

    pub fn heading(&self) -> String {
        self.heading.borrow().clone()
    }
}

#[derive(Clone)]
enum Node {
    Search(Rc<SearchNode>),
    File(Rc<FileNode>),
    Hit(Rc<FileNode>, usize),
}

fn node_of(item: &glib::Object) -> Option<Node> {
    let item = match item.downcast_ref::<gtk::TreeListRow>() {
        Some(row) => row.item()?,
        None => item.clone(),
    };
    let boxed = item.downcast_ref::<glib::BoxedAnyObject>()?;
    Some(boxed.borrow::<Node>().clone())
}

/// A row's widgets, kept to restyle bound rows when the theme changes.
struct RowWidgets {
    expander: gtk::TreeExpander,
    prefix: gtk::Label,
    text: gtk::Inscription,
}

type OnJump = Box<dyn Fn(Jump)>;

/// The results panel: its searches, the status line above them and its buttons.
pub struct ResultsPanel {
    root: gtk::Box,
    status: gtk::Label,
    stop: gtk::Button,
    copy: gtk::Button,
    clear: gtk::Button,
    close: gtk::Button,
    list: gtk::ListView,
    keys: gtk::EventControllerKey,
    /// The panel's height, kept while it is closed.
    height: Cell<i32>,
    selection: gtk::SingleSelection,
    tree: gtk::TreeListModel,
    searches: gio::ListStore,
    /// The newest search's current match: file, hit line and match on the line.
    cursor: Cell<Option<(usize, usize, usize)>>,
    match_color: Cell<Option<gdk::RGBA>>,
    bound: RefCell<Vec<(glib::WeakRef<gtk::ListItem>, Rc<RowWidgets>)>>,
    on_jump: RefCell<Option<OnJump>>,
}

impl ResultsPanel {
    pub fn new() -> Rc<Self> {
        let searches = gio::ListStore::new::<glib::BoxedAnyObject>();
        let tree =
            gtk::TreeListModel::new(searches.clone(), false, true, |item| match node_of(item)? {
                Node::Search(search) => Some(search.files.clone().upcast()),
                Node::File(file) => {
                    let hits = gio::ListStore::new::<glib::BoxedAnyObject>();
                    let items: Vec<glib::BoxedAnyObject> = (0..file.hits.len())
                        .map(|index| glib::BoxedAnyObject::new(Node::Hit(file.clone(), index)))
                        .collect();
                    hits.extend_from_slice(&items);
                    Some(hits.upcast())
                }
                Node::Hit(..) => None,
            });
        let selection = gtk::SingleSelection::builder()
            .model(&tree)
            .autoselect(false)
            .can_unselect(true)
            .build();
        let factory = gtk::SignalListItemFactory::new();
        let list = gtk::ListView::builder()
            .model(&selection)
            .factory(&factory)
            .single_click_activate(false)
            .css_classes(["stet-results-list", "stet-mono"])
            .build();
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hexpand(true)
            .build();

        let status = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(pango::EllipsizeMode::End)
            .css_classes(["stet-results-status"])
            .build();
        let button = |label: &str, tooltip: &str| {
            let button = gtk::Button::builder()
                .label(label)
                .tooltip_text(tooltip)
                .build();
            button.add_css_class("flat");
            button
        };
        let stop = button("Stop", "Stop Find in Files");
        stop.set_visible(false);
        let copy = button("Copy", "Copy the search results");
        let clear = button("Clear", "Clear the search results");
        let close = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Close (Escape)")
            .build();
        close.add_css_class("flat");
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        header.add_css_class("stet-results-header");
        for widget in [
            status.upcast_ref::<gtk::Widget>(),
            stop.upcast_ref(),
            copy.upcast_ref(),
            clear.upcast_ref(),
            close.upcast_ref(),
        ] {
            header.append(widget);
        }
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("stet-results");
        root.append(&header);
        root.append(&scrolled);
        root.set_visible(false);
        let keys = gtk::EventControllerKey::new();
        root.add_controller(keys.clone());

        let panel = Rc::new(Self {
            root,
            status,
            stop,
            copy,
            clear,
            close,
            list,
            keys,
            height: Cell::new(INITIAL_HEIGHT),
            selection,
            tree,
            searches,
            cursor: Cell::new(None),
            match_color: Cell::new(None),
            bound: RefCell::new(Vec::new()),
            on_jump: RefCell::new(None),
        });
        panel.connect_factory(&factory);
        let weak = Rc::downgrade(&panel);
        panel.list.connect_activate(move |_, position| {
            if let Some(panel) = weak.upgrade() {
                panel.activate(position);
            }
        });
        panel
    }

    fn connect_factory(self: &Rc<Self>, factory: &gtk::SignalListItemFactory) {
        factory.connect_setup(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let prefix = gtk::Label::builder().xalign(0.0).build();
            let text = gtk::Inscription::builder()
                .hexpand(true)
                .xalign(0.0)
                .min_chars(10)
                .nat_chars(120)
                .text_overflow(gtk::InscriptionOverflow::EllipsizeEnd)
                .build();
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            content.append(&prefix);
            content.append(&text);
            let expander = gtk::TreeExpander::builder()
                .child(&content)
                .indent_for_icon(true)
                .build();
            item.set_child(Some(&expander));
        });
        let weak = Rc::downgrade(self);
        factory.connect_bind(move |_, item| {
            let (Some(panel), Some(item)) = (weak.upgrade(), item.downcast_ref::<gtk::ListItem>())
            else {
                return;
            };
            let Some(widgets) = row_widgets(item) else {
                return;
            };
            let row = item.item().and_downcast::<gtk::TreeListRow>();
            widgets.expander.set_list_row(row.as_ref());
            if let Some(node) = item.item().as_ref().and_then(node_of) {
                panel.fill(&widgets, &node);
            }
            let widgets = Rc::new(widgets);
            panel.bound.borrow_mut().push((item.downgrade(), widgets));
        });
        let weak = Rc::downgrade(self);
        factory.connect_unbind(move |_, item| {
            let (Some(panel), Some(item)) = (weak.upgrade(), item.downcast_ref::<gtk::ListItem>())
            else {
                return;
            };
            panel
                .bound
                .borrow_mut()
                .retain(|(bound, _)| bound.upgrade().is_some_and(|bound| &bound != item));
        });
    }

    fn fill(&self, widgets: &RowWidgets, node: &Node) {
        let (prefix, text, classes): (String, Option<(String, pango::AttrList)>, &[&str]) =
            match node {
                Node::Search(search) => (search.heading(), None, &["stet-results-search"]),
                Node::File(file) => (file.row_text(), None, &["stet-results-file"]),
                Node::Hit(file, index) => {
                    let hit = &file.hits[*index];
                    let attributes = pango::AttrList::new();
                    for range in &hit.match_ranges {
                        let (start, end) = (to_u32(range.start), to_u32(range.end));
                        let mut weight = pango::AttrInt::new_weight(pango::Weight::Bold);
                        weight.set_start_index(start);
                        weight.set_end_index(end);
                        attributes.insert(weight);
                        if let Some(color) = self.match_color.get() {
                            let mut background = pango::AttrColor::new_background(
                                unit_to_u16(color.red()),
                                unit_to_u16(color.green()),
                                unit_to_u16(color.blue()),
                            );
                            background.set_start_index(start);
                            background.set_end_index(end);
                            attributes.insert(background);
                        }
                    }
                    (
                        format!("Line {}:", hit.line_number),
                        Some((hit.line_text.clone(), attributes)),
                        &["stet-results-line"],
                    )
                }
            };
        widgets.prefix.set_label(&prefix);
        widgets.prefix.set_css_classes(classes);
        match text {
            Some((text, attributes)) => {
                widgets.text.set_text(Some(&text));
                widgets.text.set_attributes(Some(&attributes));
                widgets.text.set_visible(true);
            }
            None => {
                widgets.text.set_text(None);
                widgets.text.set_attributes(None);
                widgets.text.set_visible(false);
            }
        }
    }

    /// The panel's widget, which the window places below the editor.
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// The status line above the searches, with `tooltip` for details.
    pub fn set_status(&self, text: &str, tooltip: Option<&str>) {
        self.status.set_label(text);
        self.status.set_tooltip_text(tooltip);
    }

    /// The status line's text; its tooltip stays.
    pub fn set_status_text(&self, text: &str) {
        self.status.set_label(text);
    }

    /// Shows Stop while a files search runs.
    pub fn set_stoppable(&self, stoppable: bool) {
        self.stop.set_visible(stoppable);
    }

    pub fn connect_jump(&self, on_jump: impl Fn(Jump) + 'static) {
        self.on_jump.replace(Some(Box::new(on_jump)));
    }

    /// The colour that emphasizes matches: the scheme's `search-match` background.
    pub fn set_match_color(&self, color: Option<gdk::RGBA>) {
        self.match_color.set(color);
        let bound: Vec<(Node, Rc<RowWidgets>)> = self
            .bound
            .borrow()
            .iter()
            .filter_map(|(item, widgets)| {
                let item = item.upgrade()?;
                Some((node_of(&item.item()?)?, Rc::clone(widgets)))
            })
            .collect();
        for (node, widgets) in bound {
            self.fill(&widgets, &node);
        }
    }

    #[cfg(feature = "selftest")]
    pub fn is_shown(&self) -> bool {
        self.root.is_visible()
    }

    /// The status line's text.
    #[cfg(feature = "selftest")]
    pub fn status_text(&self) -> String {
        self.status.label().to_string()
    }

    /// The panel's key handler (Escape, Ctrl+C), which the self-test drives.
    #[cfg(feature = "selftest")]
    pub fn keys(&self) -> &gtk::EventControllerKey {
        &self.keys
    }

    /// Starts a search at the top of the panel, expanded; older searches are collapsed.
    pub fn begin(&self, heading: String) -> Rc<SearchNode> {
        for position in 0..self.searches.n_items() {
            if let Some(row) = self.tree.child_row(position) {
                row.set_expanded(false);
            }
        }
        let search = Rc::new(SearchNode {
            heading: RefCell::new(heading),
            files: gio::ListStore::new::<glib::BoxedAnyObject>(),
            nodes: RefCell::new(Vec::new()),
            hits: Cell::new(0),
        });
        self.searches
            .insert(0, &glib::BoxedAnyObject::new(Node::Search(search.clone())));
        while self.searches.n_items() > MAX_SEARCHES {
            self.searches.remove(self.searches.n_items() - 1);
        }
        if let Some(row) = self.tree.child_row(0) {
            row.set_expanded(true);
        }
        self.cursor.set(None);
        self.selection.set_selected(gtk::INVALID_LIST_POSITION);
        search
    }

    /// Adds files after the others in `search`, in one change of the list. (Inserting Find in
    /// Files' files one by one to keep them sorted blocked the main loop for 0.43 s per drain
    /// with 13,000 files.)
    pub fn add_files(&self, search: &Rc<SearchNode>, files: Vec<FileNode>) {
        if files.is_empty() {
            return;
        }
        let mut items = Vec::with_capacity(files.len());
        let mut nodes = search.nodes.borrow_mut();
        for file in files {
            search.hits.set(search.hits.get() + file.match_count());
            let file = Rc::new(file);
            items.push(glib::BoxedAnyObject::new(Node::File(file.clone())));
            nodes.push(file);
        }
        drop(nodes);
        search.files.extend_from_slice(&items);
    }

    /// Changes `search`'s heading.
    pub fn set_heading(&self, search: &Rc<SearchNode>, heading: String) {
        search.heading.replace(heading);
        let position = (0..self.searches.n_items()).find(|&position| {
            self.searches
                .item(position)
                .as_ref()
                .and_then(node_of)
                .is_some_and(
                    |node| matches!(node, Node::Search(other) if Rc::ptr_eq(&other, search)),
                )
        });
        if let Some(position) = position
            && let Some(row) = self.tree.child_row(position)
        {
            let row_position = row.position();
            let bound: Vec<Rc<RowWidgets>> = self
                .bound
                .borrow()
                .iter()
                .filter_map(|(item, widgets)| {
                    let item = item.upgrade()?;
                    (item.position() == row_position).then(|| Rc::clone(widgets))
                })
                .collect();
            for widgets in bound {
                self.fill(&widgets, &Node::Search(search.clone()));
            }
        }
    }

    /// The newest search.
    pub fn latest(&self) -> Option<Rc<SearchNode>> {
        match node_of(&self.searches.item(0)?)? {
            Node::Search(search) => Some(search),
            _ => None,
        }
    }

    #[cfg(feature = "selftest")]
    pub fn search_count(&self) -> u32 {
        self.searches.n_items()
    }

    pub fn clear(&self) {
        self.searches.remove_all();
        self.cursor.set(None);
    }

    /// Steps to the newest search's next (or previous) match, wrapping around, selects its
    /// line in the list and returns where it is.
    pub fn step(&self, forward: bool) -> Option<Jump> {
        let search = self.latest()?;
        let nodes = search.nodes.borrow();
        let counts: Vec<Vec<usize>> = nodes
            .iter()
            .map(|file| {
                file.hits
                    .iter()
                    .map(|hit| hit.match_ranges.len().max(1))
                    .collect()
            })
            .collect();
        let total: usize = counts.iter().flatten().sum();
        if total == 0 {
            return None;
        }
        let flat = |(file, hit, range): (usize, usize, usize)| -> usize {
            counts[..file].iter().flatten().sum::<usize>()
                + counts[file][..hit].iter().sum::<usize>()
                + range
        };
        let next = match self.cursor.get() {
            None if forward => 0,
            None => total - 1,
            Some(cursor) if forward => (flat(cursor) + 1) % total,
            Some(cursor) => (flat(cursor) + total - 1) % total,
        };
        let mut remaining = next;
        let mut cursor = None;
        'files: for (file, hits) in counts.iter().enumerate() {
            for (hit, &count) in hits.iter().enumerate() {
                if remaining < count {
                    cursor = Some((file, hit, remaining));
                    break 'files;
                }
                remaining -= count;
            }
        }
        let (file, hit, range) = cursor?;
        self.cursor.set(Some((file, hit, range)));
        let jump = nodes[file].jump(hit, range);
        drop(nodes);
        self.select_line(0, file, hit);
        jump
    }

    /// Selects and scrolls to a hit line of search `search` (0 is the newest).
    fn select_line(&self, search: u32, file: usize, hit: usize) {
        let Some(search_row) = self.tree.child_row(search) else {
            return;
        };
        search_row.set_expanded(true);
        let Some(file_row) = search_row.child_row(to_u32(file)) else {
            return;
        };
        file_row.set_expanded(true);
        let Some(hit_row) = file_row.child_row(to_u32(hit)) else {
            return;
        };
        let position = hit_row.position();
        self.selection.set_selected(position);
        self.list
            .scroll_to(position, gtk::ListScrollFlags::NONE, None);
    }

    /// What double-click or Enter does on the row at `position`.
    fn activate(&self, position: u32) {
        let Some(row) = self.tree.item(position).and_downcast::<gtk::TreeListRow>() else {
            return;
        };
        let Some(node) = row.item().as_ref().and_then(node_of) else {
            return;
        };
        match node {
            Node::Hit(file, hit) => {
                self.remember_cursor(&row, &file, hit);
                if let Some(jump) = file.jump(hit, 0)
                    && let Some(on_jump) = self.on_jump.borrow().as_ref()
                {
                    on_jump(jump);
                }
            }
            Node::Search(_) | Node::File(_) => row.set_expanded(!row.is_expanded()),
        }
    }

    /// Activates the selected row, as Enter does.
    #[cfg(feature = "selftest")]
    pub fn activate_selected(&self) -> bool {
        let position = self.selection.selected();
        if position == gtk::INVALID_LIST_POSITION {
            return false;
        }
        self.activate(position);
        true
    }

    /// Selects the row at `position` of the flattened list, for the self-test.
    #[cfg(feature = "selftest")]
    pub fn select_position(&self, position: u32) -> bool {
        if position >= self.tree.n_items() {
            return false;
        }
        self.selection.set_selected(position);
        true
    }

    /// Makes an activated hit in the newest search the starting point for Next/Previous.
    fn remember_cursor(&self, row: &gtk::TreeListRow, file: &Rc<FileNode>, hit: usize) {
        let Some(search) = self.latest() else {
            return;
        };
        let in_latest = row
            .parent()
            .and_then(|parent| parent.parent())
            .is_some_and(|search_row| search_row.position() == 0);
        if !in_latest {
            return;
        }
        let index = search
            .nodes
            .borrow()
            .iter()
            .position(|node| Rc::ptr_eq(node, file));
        if let Some(index) = index {
            self.cursor.set(Some((index, hit, 0)));
        }
    }

    /// The text of the rows shown, one per line, for the self-test and Copy.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for position in 0..self.searches.n_items() {
            let Some(Node::Search(search)) =
                self.searches.item(position).as_ref().and_then(node_of)
            else {
                continue;
            };
            out.push_str(&search.heading());
            out.push('\n');
            for file in search.nodes.borrow().iter() {
                let _ = writeln!(out, "  {}", file.row_text());
                for hit in &file.hits {
                    let _ = writeln!(out, "    Line {}: {}", hit.line_number, hit.line_text);
                }
            }
        }
        out
    }

    /// The selected row as text: a hit line, a file with its hits, or a whole search.
    pub fn selected_text(&self) -> Option<String> {
        let position = self.selection.selected();
        let row = self
            .tree
            .item(position)
            .and_downcast::<gtk::TreeListRow>()?;
        let node = row.item().as_ref().and_then(node_of)?;
        let file_text = |file: &FileNode| {
            let mut out = format!("{}\n", file.label);
            for hit in &file.hits {
                let _ = writeln!(out, "  Line {}: {}", hit.line_number, hit.line_text);
            }
            out
        };
        Some(match node {
            Node::Hit(file, hit) => file.hits[hit].line_text.clone(),
            Node::File(file) => file_text(&file),
            Node::Search(search) => {
                let mut out = format!("{}\n", search.heading());
                for file in search.nodes.borrow().iter() {
                    out.push_str(&file_text(file));
                }
                out
            }
        })
    }
}

fn row_widgets(item: &gtk::ListItem) -> Option<RowWidgets> {
    let expander = item.child().and_downcast::<gtk::TreeExpander>()?;
    let content = expander.child().and_downcast::<gtk::Box>()?;
    let prefix = content.first_child().and_downcast::<gtk::Label>()?;
    let text = prefix.next_sibling().and_downcast::<gtk::Inscription>()?;
    Some(RowWidgets {
        expander,
        prefix,
        text,
    })
}
