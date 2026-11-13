//! The window's widget tree (DESIGN.md "Window layout"): the two views' tab strips side by side
//! on top, over their tabs; the find bar right below the views, the results panel below both
//! and the status bar at the bottom. Outside Hyprland the window controls sit at the ends of the
//! strips, which also move the window.

use crate::window::{FindBar, ResultsPanel, StatusBar};
use gtk4 as gtk;
use gtk4::gdk;
use libadwaita as adw;
use libadwaita::prelude::*;

/// One of the window's two views: a tab strip over its tabs. The second view shows only
/// while the window is split, and either view hides when its last tab goes.
pub(in crate::window) struct ViewPane {
    pub(in crate::window) tabs: adw::TabView,
    pub(in crate::window) bar: adw::TabBar,
    /// Opens a new tab in this view, right after the strip's last tab.
    pub(in crate::window) new_tab: gtk::Button,
    /// The strip's end: the + and, on the last strip on screen outside Hyprland, the window
    /// controls at the far end.
    pub(in crate::window) trail: gtk::Box,
    /// The strip's tabs, as wide as they are, so that the + follows them. `None` if
    /// libadwaita's tab strip is built differently: the + then sits at the strip's far end.
    #[cfg(feature = "selftest")]
    pub(in crate::window) tab_scroller: Option<gtk::ScrolledWindow>,
    /// Takes a tab dropped on the + or the empty part of the strip after it, which libadwaita's
    /// strip doesn't cover since the tabs are only as wide as they are.
    pub(in crate::window) tab_drop: gtk::DropTarget,
}

impl ViewPane {
    pub(in crate::window) fn new() -> Self {
        let tabs = adw::TabView::new();
        tabs.set_shortcuts(adw::TabViewShortcuts::empty());
        let bar = adw::TabBar::builder()
            .view(&tabs)
            .autohide(false)
            .expand_tabs(false)
            .build();
        bar.add_css_class("stet-mono");
        let new_tab = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("New Tab")
            .focus_on_click(false)
            .valign(gtk::Align::Center)
            .build();
        new_tab.add_css_class("flat");
        new_tab.update_property(&[gtk::accessible::Property::Label("New Tab")]);
        let trail = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        trail.append(&new_tab);
        // libadwaita gives the strip's tabs all the room the ends leave. Sized to the tabs
        // instead, they leave it to a filler after the +, which keeps the + right after the
        // last tab, and at the strip's end once the tabs scroll.
        let tab_scroller = tab_scroller(&bar);
        if let Some(scroller) = &tab_scroller {
            scroller.set_hexpand(false);
            scroller.set_propagate_natural_width(true);
            trail.append(&gtk::Box::builder().hexpand(true).build());
        } else {
            tracing::warn!("the tab strip has no tab scroller: the + sits at its end");
        }
        bar.set_end_action_widget(Some(&trail));
        let tab_drop = gtk::DropTarget::new(adw::TabPage::static_type(), gdk::DragAction::MOVE);
        trail.add_controller(tab_drop.clone());
        Self {
            tabs,
            bar,
            new_tab,
            trail,
            #[cfg(feature = "selftest")]
            tab_scroller,
            tab_drop,
        }
    }
}

/// The scrolled window in a tab strip that holds its unpinned tabs: the one of its two that
/// expands (the pinned tabs' doesn't).
fn tab_scroller(bar: &adw::TabBar) -> Option<gtk::ScrolledWindow> {
    let mut widgets = vec![bar.clone().upcast::<gtk::Widget>()];
    while let Some(widget) = widgets.pop() {
        if let Some(scroller) = widget.downcast_ref::<gtk::ScrolledWindow>()
            && scroller.hexpands()
        {
            return Some(scroller.clone());
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            child = next.next_sibling();
            widgets.push(next);
        }
    }
    None
}

/// The parts of the widget tree that features change after the window is built.
pub(in crate::window) struct Layout {
    /// The views side by side, divided where the tab strips above them are.
    pub(in crate::window) views_paned: gtk::Paned,
    /// The hamburger menu and, outside Hyprland, the window controls the decoration layout
    /// puts first, at the start of the first strip on screen.
    pub(in crate::window) start: gtk::Box,
    /// The window controls outside Hyprland, at the end of the last strip on screen, after its
    /// + and filler.
    pub(in crate::window) end: gtk::Box,
    pub(in crate::window) menu_button: gtk::MenuButton,
    /// The views and the find bar: what the command palette and quick open drop down from.
    pub(in crate::window) content: gtk::Box,
    /// The editor (and find bar) above, the results panel below.
    pub(in crate::window) paned: gtk::Paned,
}

impl Layout {
    /// Builds the tree around `views` and the bars. Returns it with its root, which the
    /// Ctrl+Tab `switcher` floats over.
    pub(in crate::window) fn build(
        views: &[ViewPane; 2],
        find: &FindBar,
        results: &ResultsPanel,
        status: &StatusBar,
        switcher: &impl IsA<gtk::Widget>,
    ) -> (Self, gtk::Overlay) {
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Menu")
            .build();
        menu_button.add_css_class("flat");
        let start = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let end = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let tiling = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
            || std::env::var("XDG_CURRENT_DESKTOP")
                .is_ok_and(|desktop| desktop.contains("Hyprland"));
        if !tiling {
            start.append(&gtk::WindowControls::new(gtk::PackType::Start));
            end.append(&gtk::WindowControls::new(gtk::PackType::End));
        }
        start.append(&menu_button);
        views[0].bar.set_start_action_widget(Some(&start));
        views[0].trail.append(&end);
        // The strips sit side by side over the views, divided where the views are.
        let split_paned = |start: &gtk::Widget, end: &gtk::Widget| {
            gtk::Paned::builder()
                .orientation(gtk::Orientation::Horizontal)
                .start_child(start)
                .end_child(end)
                .resize_start_child(true)
                .resize_end_child(true)
                .shrink_start_child(false)
                .shrink_end_child(false)
                .wide_handle(false)
                .build()
        };
        let top_paned = split_paned(views[0].bar.upcast_ref(), views[1].bar.upcast_ref());
        let views_paned = split_paned(views[0].tabs.upcast_ref(), views[1].tabs.upcast_ref());
        views_paned.set_vexpand(true);
        views[1].bar.set_visible(false);
        views[1].tabs.set_visible(false);
        top_paned
            .bind_property("position", &views_paned, "position")
            .bidirectional()
            .sync_create()
            .build();
        let top: gtk::Widget = if tiling {
            top_paned.upcast()
        } else {
            gtk::WindowHandle::builder()
                .child(&top_paned)
                .build()
                .upcast()
        };

        // The find bar stays right below the editor; the results panel opens below both.
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&views_paned);
        content.append(find.root());
        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Vertical)
            .start_child(&content)
            .end_child(results.root())
            .resize_start_child(true)
            .resize_end_child(false)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .wide_handle(false)
            .vexpand(true)
            .build();
        let toolbar = adw::ToolbarView::builder()
            .content(&paned)
            .top_bar_style(adw::ToolbarStyle::RaisedBorder)
            .bottom_bar_style(adw::ToolbarStyle::RaisedBorder)
            .build();
        toolbar.add_top_bar(&top);
        toolbar.add_bottom_bar(&status.root);
        // The Ctrl+Tab switcher floats over everything but the toasts.
        let overlay = gtk::Overlay::builder().child(&toolbar).build();
        overlay.add_overlay(switcher);
        let layout = Self {
            views_paned,
            start,
            end,
            menu_button,
            content,
            paned,
        };
        (layout, overlay)
    }
}
