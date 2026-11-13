//! Split view: two views side by side, each a tab strip over its tabs, divided at the same
//! place. A view shows only while it has tabs, so a window without a second view looks as it
//! did before; Move to Other View and Clone to Other View fill the second one, and closing its
//! last tab collapses the split.
//!
//! The **active view** is where commands act, new tabs open, and what the status bar and the
//! window title show: the view that last had the keyboard focus or a click on its strip, or the
//! one Switch View (F8) moved to. A clone is a second tab over the same document (and buffer),
//! with a view, caret and banners of its own (`EditorPage::clone_view`).
//!
//! **Synchronised scrolling** keeps the distance between the two views' scroll positions while
//! one scrolls (View › Synchronise Vertical/Horizontal Scrolling); a comparison keeps the views
//! level instead (`window::tools::compare`).

use crate::editor::EditorPage;
use crate::runtime::Flag;
use crate::window::Window;
use gtk4 as gtk;
use gtk4::glib;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::Cell;
use stet_domain::actions::ActionId;

/// The CSS class of the active view's tab strip while the window is split.
const ACTIVE_CLASS: &str = "stet-active-view";

/// The window's split-view state.
#[derive(Debug, Default)]
pub struct SplitState {
    vertical: Cell<bool>,
    horizontal: Cell<bool>,
    /// A synchronised scroll is being applied: the other view's own change is not passed on.
    syncing: Flag,
    /// The scroll positions last seen of the page in front of each view: vertical, horizontal.
    last: [[Cell<f64>; 2]; 2],
    /// The divider position the session restored, used when the window next splits.
    pub(in crate::window) position: Cell<Option<i32>>,
}

/// The commands of View › Split and the tab menu's moves to the other view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::window) enum SplitCommand {
    MoveToOtherView,
    CloneToOtherView,
    /// Moves the focus to the other view.
    SwitchView,
    SyncVerticalScrolling,
    SyncHorizontalScrolling,
}

/// Makes a view the active one when the user turns to it.
pub(in crate::window) fn connect(window: &Window) {
    for (index, view) in window.inner.views.iter().enumerate() {
        // Focus moving into a view makes it the active one.
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(glib::clone!(
            #[weak]
            window,
            move |_| window.set_active_view(index, false)
        ));
        view.tabs.add_controller(focus);
        // So does a click on its tab strip, but not on the menu or the window controls at its
        // ends.
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(glib::clone!(
            #[weak]
            window,
            move |gesture, _, x, y| {
                let layout = &window.inner.layout;
                let ends = [&layout.start, &layout.end];
                let on_ends = gesture
                    .widget()
                    .and_then(|bar| bar.pick(x, y, gtk::PickFlags::DEFAULT))
                    .is_some_and(|target| {
                        ends.iter().any(|end| {
                            target == *end.upcast_ref::<gtk::Widget>() || target.is_ancestor(*end)
                        })
                    });
                if !on_ends {
                    window.set_active_view(index, true);
                }
            }
        ));
        view.bar.add_controller(click);
    }
}

impl Window {
    /// Synchronised scrolling for a new tab's view.
    pub(in crate::window) fn connect_page_split(&self, page: &EditorPage) {
        let view = page.view();
        for (adjustment, vertical) in [(view.vadjustment(), true), (view.hadjustment(), false)] {
            let Some(adjustment) = adjustment else {
                continue;
            };
            adjustment.connect_value_changed(glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[weak]
                page,
                move |adjustment| window.on_scrolled(&page, vertical, adjustment.value())
            ));
        }
    }

    /// The view that shows `page`.
    pub fn view_of(&self, page: &EditorPage) -> Option<usize> {
        self.locate(page).map(|(index, _)| index)
    }

    /// The active view: 0 for the main view, 1 for the second.
    pub fn active_view(&self) -> usize {
        self.inner.active.get()
    }

    /// Whether both views show tabs.
    pub fn is_split(&self) -> bool {
        self.inner.views.iter().all(|view| view.tabs.n_pages() > 0)
    }

    pub(in crate::window) fn active_tabs(&self) -> adw::TabView {
        self.inner.views[self.inner.active.get()].tabs.clone()
    }

    /// The tab in front of each view that has tabs.
    pub(in crate::window) fn shown_pages(&self) -> Vec<EditorPage> {
        self.inner
            .views
            .iter()
            .filter_map(|view| view.tabs.selected_page())
            .map(|tab_page| crate::window::page_of(&tab_page))
            .collect()
    }

    /// The tab in front of view `index`.
    pub fn view_page(&self, index: usize) -> Option<EditorPage> {
        self.inner
            .views
            .get(index)?
            .tabs
            .selected_page()
            .map(|tab_page| crate::window::page_of(&tab_page))
    }

    /// Makes view `index` the active one, if it shows tabs.
    pub(in crate::window) fn set_active_view(&self, index: usize, take_focus: bool) {
        let inner = &self.inner;
        if inner.active.get() == index || inner.views[index].tabs.n_pages() == 0 {
            return;
        }
        inner.active.set(index);
        self.style_views();
        self.current_changed(take_focus);
    }

    /// Shows the views that have tabs, puts the menu at the start of the first strip on screen
    /// and the window controls at the end of the last, and keeps the active view one that
    /// shows tabs.
    pub(in crate::window) fn update_layout(&self) {
        let inner = &self.inner;
        let counts = [inner.views[0].tabs.n_pages(), inner.views[1].tabs.n_pages()];
        let was_split = inner.views.iter().all(|view| view.tabs.get_visible());
        for (view, count) in inner.views.iter().zip(counts) {
            view.tabs.set_visible(count > 0);
            view.bar.set_visible(count > 0);
        }
        let first = usize::from(counts[0] == 0 && counts[1] > 0);
        let last = usize::from(counts[1] > 0);
        let other = &inner.views[1 - first].bar;
        if other.start_action_widget().is_some() {
            other.set_start_action_widget(None::<&gtk::Widget>);
        }
        let (start, end) = (&inner.layout.start, &inner.layout.end);
        let bar = &inner.views[first].bar;
        if bar.start_action_widget().is_none() {
            bar.set_start_action_widget(Some(start));
        }
        let trail = &inner.views[last].trail;
        if end.parent().as_ref() != Some(trail.upcast_ref()) {
            if let Some(other) = end.parent().and_downcast::<gtk::Box>() {
                other.remove(end);
            }
            trail.append(end);
        }
        let split = counts.iter().all(|count| *count > 0);
        if split && !was_split {
            self.place_divider();
        }
        let active = inner.active.get();
        if counts[active] == 0 && counts[1 - active] > 0 {
            inner.active.set(1 - active);
            self.current_changed(true);
        }
        self.style_views();
        self.reset_scroll_sync();
    }

    /// The divider where the session had it, else in the middle.
    fn place_divider(&self) {
        let inner = &self.inner;
        let width = [
            inner.layout.views_paned.width(),
            inner.window.width(),
            inner.window.default_width(),
        ]
        .into_iter()
        .find(|width| *width > 0)
        .unwrap_or(0);
        let restored = inner
            .split
            .position
            .take()
            .filter(|position| *position > 0 && *position < width);
        inner
            .layout
            .views_paned
            .set_position(restored.unwrap_or(width / 2));
    }

    /// Moves the divider to `position`, or keeps it for the next split (for the self-test).
    #[cfg(feature = "selftest")]
    pub fn set_split_position(&self, position: i32) {
        if self.is_split() {
            self.inner.layout.views_paned.set_position(position);
        } else {
            self.inner.split.position.set(Some(position));
        }
    }

    /// The divider's position while the window is split, else 0.
    pub fn split_position(&self) -> i32 {
        if self.is_split() {
            self.inner.layout.views_paned.position()
        } else {
            0
        }
    }

    /// Marks the active view's strip while the window is split.
    pub(in crate::window) fn style_views(&self) {
        let split = self.is_split();
        let active = self.inner.active.get();
        for (index, view) in self.inner.views.iter().enumerate() {
            if split && index == active {
                view.bar.add_css_class(ACTIVE_CLASS);
            } else {
                view.bar.remove_css_class(ACTIVE_CLASS);
            }
        }
    }

    /// Runs a split-view command.
    pub(in crate::window) fn run_split(&self, command: SplitCommand) {
        match command {
            SplitCommand::MoveToOtherView => {
                if let Some(page) = self.context_page() {
                    self.move_to_other_view(&page);
                }
            }
            SplitCommand::CloneToOtherView => {
                if let Some(page) = self.context_page() {
                    self.clone_to_other_view(&page);
                }
            }
            SplitCommand::SwitchView => {
                if self.is_split() {
                    self.set_active_view(1 - self.inner.active.get(), false);
                    self.focus_editor();
                }
            }
            SplitCommand::SyncVerticalScrolling => {
                self.toggle_scroll_sync(
                    &self.inner.split.vertical,
                    ActionId::SyncVerticalScrolling,
                );
            }
            SplitCommand::SyncHorizontalScrolling => self.toggle_scroll_sync(
                &self.inner.split.horizontal,
                ActionId::SyncHorizontalScrolling,
            ),
        }
    }

    /// Turns one of the scroll syncs on or off, with its check mark.
    fn toggle_scroll_sync(&self, cell: &Cell<bool>, id: ActionId) {
        cell.set(!cell.get());
        if let Some(action) = self.action(id) {
            action.set_state(&cell.get().to_variant());
        }
        self.reset_scroll_sync();
    }

    /// Move to Other View: the tab goes to the other view, or closes when the other view shows
    /// its document already, which then comes to the front.
    pub fn move_to_other_view(&self, page: &EditorPage) {
        let Some((from, tab_page)) = self.locate(page) else {
            return;
        };
        let to = 1 - from;
        if let Some(there) = page
            .document_pages()
            .into_iter()
            .find(|other| other != page && self.view_of(other) == Some(to))
        {
            self.select(&there);
            self.close_without_prompt(page);
            return;
        }
        let inner = &self.inner;
        let (source, target) = (&inner.views[from].tabs, &inner.views[to].tabs);
        let pinned = tab_page.is_pinned();
        if pinned {
            source.set_page_pinned(&tab_page, false);
        }
        {
            let _moving = inner.tabs.moving();
            source.transfer_page(&tab_page, target, target.n_pages());
        }
        if pinned {
            target.set_page_pinned(&tab_page, true);
        }
        self.update_layout();
        self.select(page);
        self.refresh_page(page);
        self.update_actions();
    }

    /// Clone to Other View: a second tab over the same document in the other view, or the one
    /// there already.
    pub fn clone_to_other_view(&self, page: &EditorPage) -> Option<EditorPage> {
        let from = self.view_of(page)?;
        let to = 1 - from;
        if let Some(there) = page
            .document_pages()
            .into_iter()
            .find(|other| self.view_of(other) == Some(to))
        {
            self.select(&there);
            return Some(there);
        }
        let scroll = page.scroll_position();
        let clone = page.clone_view(
            self.inner.shared.appearance.scheme().as_ref(),
            self.inner.settings.get(),
        );
        self.attach_page(to, &clone, true);
        self.refresh_banners(&clone);
        // The clone opens where the original is scrolled, once it is laid out.
        let target = clone.clone();
        glib::idle_add_local_once(move || target.set_scroll_position(scroll));
        self.update_actions();
        Some(clone)
    }

    pub(in crate::window) fn update_split_actions(&self) {
        let page = self.current_page();
        let movable = page.is_some() && (self.is_split() || self.pages().len() > 1);
        for (id, enabled) in [
            (ActionId::MoveToOtherView, movable),
            (ActionId::CloneToOtherView, page.is_some()),
            (ActionId::SwitchView, self.is_split()),
        ] {
            if let Some(action) = self.action(id) {
                action.set_enabled(enabled);
            }
        }
    }

    // ----- synchronised scrolling --------------------------------------------------------

    /// Whether scrolling one view scrolls the other: the View menu's toggles, or a
    /// comparison, which keeps the views level (`level`).
    fn scroll_sync(&self, vertical: bool) -> (bool, bool) {
        if self.comparison_shown() {
            return (true, true);
        }
        let split = &self.inner.split;
        let on = if vertical {
            split.vertical.get()
        } else {
            split.horizontal.get()
        };
        (on, false)
    }

    /// The pages in front changed, or the sync toggles did: the positions now are the ones
    /// later scrolls are measured from.
    pub(in crate::window) fn reset_scroll_sync(&self) {
        for (index, [vertical, horizontal]) in self.inner.split.last.iter().enumerate() {
            let Some(page) = self.view_page(index) else {
                continue;
            };
            let view = page.view();
            if let Some(adjustment) = view.vadjustment() {
                vertical.set(adjustment.value());
            }
            if let Some(adjustment) = view.hadjustment() {
                horizontal.set(adjustment.value());
            }
        }
    }

    fn on_scrolled(&self, page: &EditorPage, vertical: bool, value: f64) {
        let split = &self.inner.split;
        if split.syncing.is_raised() {
            return;
        }
        let Some(index) = self.view_of(page) else {
            return;
        };
        if self.view_page(index).as_ref() != Some(page) {
            return;
        }
        let axis = usize::from(!vertical);
        let last = split.last[index][axis].replace(value);
        let (on, level) = self.scroll_sync(vertical);
        if !on || !self.is_split() {
            return;
        }
        let Some(other) = self.view_page(1 - index) else {
            return;
        };
        let view = other.view();
        let adjustment = if vertical {
            view.vadjustment()
        } else {
            view.hadjustment()
        };
        let Some(adjustment) = adjustment else {
            return;
        };
        let target = if level {
            value
        } else {
            adjustment.value() + (value - last)
        };
        let upper = (adjustment.upper() - adjustment.page_size()).max(adjustment.lower());
        {
            let _syncing = split.syncing.raise();
            adjustment.set_value(target.clamp(adjustment.lower(), upper));
        }
        split.last[1 - index][axis].set(adjustment.value());
    }
}
