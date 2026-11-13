//! Keyboard focus: the current tab's editor has it, unless the user is working somewhere else
//! on purpose (the find bar, the results panel, an open popover or dialog).
//!
//! GTK does not leave a focus behind when its widget is hidden or removed: after the next frame
//! it hands the focus to an ancestor that takes it or, failing that, to the next focusable
//! widget in the window. That put the focus on the status bar's language button after a file
//! opened before the window was first shown (the tab switch hid the untitled tab's editor while
//! the window was not yet visible), and on the paned or a status-bar button after a popover
//! closed. Every way back to the editor ends here.

use super::Window;
use gtk4 as gtk;
use gtk4::glib;
use libadwaita::prelude::*;

/// The focus goes back to the editor when the window shows, and when a popover that the user
/// opened from a button closes.
pub(in crate::window) fn connect(window: &Window) {
    let inner = &window.inner;
    // Taking the focus again once the window is visible cancels a move GTK queued while it
    // was not (a tab switch before the first frame, when the command line opened files).
    inner.window.connect_map(glib::clone!(
        #[weak]
        window,
        move |_| {
            if window.dialog_open() {
                if let Some(focus) = RootExt::focus(&window.inner.window) {
                    focus.grab_focus();
                }
            } else {
                window.focus_editor();
            }
        }
    ));
    inner.palette.popover.connect_closed(glib::clone!(
        #[weak]
        window,
        move |_| window.restore_focus_later()
    ));
    // The status bar's language, line-ending and encoding buttons and the hamburger menu:
    // when their popover closes, GTK would leave the focus on the button.
    let status = &inner.status;
    for button in [
        &status.language,
        &status.eol,
        &status.encoding,
        &status.indentation,
        &inner.layout.menu_button,
    ] {
        button.connect_active_notify(glib::clone!(
            #[weak]
            window,
            move |button| {
                if !button.is_active() {
                    window.restore_focus_later();
                }
            }
        ));
    }
}

impl Window {
    /// Puts the focus in the current tab's editor.
    pub fn focus_editor(&self) {
        if let Some(page) = self.current_page() {
            page.view().grab_focus();
        }
    }

    /// Whether a dialog is open in the window.
    pub(super) fn dialog_open(&self) -> bool {
        self.inner.messages.dialog.borrow().is_some()
            || self.inner.window.visible_dialog().is_some()
    }

    /// Whether the focus is where the user put it: the current editor, the find bar, the
    /// results panel, an open popover or a dialog. A focus on a widget that is not on screen,
    /// on the status bar, the tab strip or a banner button is not.
    pub(super) fn focus_claimed(&self) -> bool {
        if self.dialog_open() {
            return true;
        }
        let inner = &self.inner;
        let Some(focus) = RootExt::focus(&inner.window) else {
            return false;
        };
        if !focus.is_mapped() {
            return false;
        }
        let within = |widget: &gtk::Widget| focus == *widget || focus.is_ancestor(widget);
        self.current_page()
            .is_some_and(|page| focus == *page.view().upcast_ref::<gtk::Widget>())
            || within(inner.find.root().upcast_ref())
            || within(inner.results.root().upcast_ref())
            || focus.ancestor(gtk::Popover::static_type()).is_some()
    }

    /// Gives the editor the focus back unless the user has put it somewhere on purpose.
    pub fn restore_focus(&self) {
        if !self.focus_claimed() {
            self.focus_editor();
        }
    }

    /// [`Self::restore_focus`] once the current event, and what a popover or dialog does as it
    /// closes, are done; before the next frame, whose end is when GTK would move the focus.
    pub fn restore_focus_later(&self) {
        let weak = self.downgrade();
        glib::idle_add_local_full(glib::Priority::HIGH_IDLE, move || {
            if let Some(window) = weak.upgrade() {
                window.restore_focus();
            }
            glib::ControlFlow::Break
        });
    }
}
