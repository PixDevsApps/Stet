//! The hamburger menu at the start of the first tab strip: built from the action registry
//! when it opens, after the keys or the recent files changed; its submenus open beside it and
//! scroll rather than hang below the window.

use crate::actions;
use crate::convert::saturating_i32;
use crate::window::Window;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gio, glib};

/// Between the menu button and the menu's items: the popover's arrow, border and padding, and
/// a little room above the window's bottom edge.
const MENU_MARGIN: i32 = 40;

const MENU_MIN_HEIGHT: i32 = 160;

/// The recent files are in the menu: it is built again when they change.
pub(in crate::window) fn connect(window: &Window) {
    window.inner.shared.connect_recent_changed(glib::clone!(
        #[weak]
        window,
        move || {
            window.rebuild_menu();
            window.update_actions();
        }
    ));
}

impl Window {
    /// The menu is out of date (keys, recent files): it is built again when it next opens.
    /// Building it takes about 20 ms, which used to run twice at startup and once for every
    /// opened file.
    pub fn rebuild_menu(&self) {
        let button = &self.inner.layout.menu_button;
        button.set_menu_model(None::<&gio::MenuModel>);
        if button.is_active() {
            self.ensure_menu();
        }
    }

    /// Builds the menu if it is out of date; the button's popup function runs it before the
    /// menu opens.
    pub(in crate::window) fn ensure_menu(&self) {
        let button = &self.inner.layout.menu_button;
        if button.menu_model().is_none() {
            let recent = self.inner.shared.recent();
            button.set_menu_model(Some(&actions::menu_model(recent.paths(), &self.keymap())));
            if let Some(popover) = button
                .popover()
                .and_then(|popover| popover.downcast::<gtk::PopoverMenu>().ok())
            {
                // From the button's left edge into the window: centred on the button at the
                // window's left end, half the menu would hang outside the window.
                popover.set_halign(gtk::Align::Start);
                // Submenus open beside the menu when the pointer rests on their item,
                // each as wide as its own items, rather than in its place on a click.
                popover.set_flags(gtk::PopoverMenuFlags::NESTED);
                for menu in menus_in(popover.upcast_ref()) {
                    menu.connect_show(glib::clone!(
                        #[weak(rename_to = window)]
                        self,
                        move |menu| window.fit_menu(menu.upcast_ref())
                    ));
                }
            }
        }
    }

    /// Keeps the menu and its submenus above the window's bottom edge: one taller than
    /// the room below its button or item scrolls.
    fn fit_menu(&self, popover: &gtk::Popover) {
        let window = self.inner.window.upcast_ref::<gtk::Widget>();
        let anchor = popover
            .parent()
            .unwrap_or_else(|| self.inner.layout.menu_button.clone().upcast());
        let (Some(scroller), Some(bounds)) = (
            menu_part::<gtk::ScrolledWindow>(popover),
            anchor.compute_bounds(window),
        ) else {
            return;
        };
        let top = if anchor.is::<gtk::MenuButton>() {
            bounds.y() + bounds.height()
        } else {
            bounds.y()
        };
        let below = window.height() - saturating_i32(f64::from(top.ceil()));
        scroller.set_max_content_height((below - MENU_MARGIN).max(MENU_MIN_HEIGHT));
    }
}

/// The first widget of type `T` in a popover menu, such as its scrolled window, which GTK
/// builds inside the popover with no API to reach it.
fn menu_part<T: IsA<gtk::Widget>>(popover: &gtk::Popover) -> Option<T> {
    let mut widgets = vec![popover.clone().upcast::<gtk::Widget>()];
    while let Some(widget) = widgets.pop() {
        if let Ok(part) = widget.clone().downcast::<T>() {
            return Some(part);
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            child = next.next_sibling();
            widgets.push(next);
        }
    }
    None
}

/// A nested popover menu and its submenus' popovers, which GTK makes children of their items.
pub fn menus_in(menu: &gtk::Widget) -> Vec<gtk::PopoverMenu> {
    let mut menus = Vec::new();
    let mut widgets = vec![menu.clone()];
    while let Some(widget) = widgets.pop() {
        if let Some(menu) = widget.downcast_ref::<gtk::PopoverMenu>() {
            menus.push(menu.clone());
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            child = next.next_sibling();
            widgets.push(next);
        }
    }
    menus
}
