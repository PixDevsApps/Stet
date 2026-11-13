//! The View menu's window-wide commands: word wrap, whitespace and the document map, which
//! every editor of the window shares; zoom, which every window shares; and full screen.

use crate::editor::ViewSettings;
use crate::window::Window;
use gtk4::glib;
use gtk4::prelude::*;
use stet_domain::actions::ActionId;
use stet_domain::view::Zoom;

/// The View menu's commands on the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::window) enum ViewCommand {
    WordWrap,
    ShowWhitespace,
    DocumentMap,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    FullScreen,
}

/// Full Screen's check mark follows the window, also when the compositor changes it.
pub(in crate::window) fn connect(window: &Window) {
    window
        .inner
        .window
        .connect_fullscreened_notify(glib::clone!(
            #[weak]
            window,
            move |toplevel| {
                if let Some(action) = window.action(ActionId::FullScreen) {
                    action.set_state(&toplevel.is_fullscreen().to_variant());
                }
            }
        ));
}

impl Window {
    /// Runs a View-menu command.
    pub(in crate::window) fn run_view(&self, command: ViewCommand) {
        match command {
            ViewCommand::WordWrap => {
                self.change_settings(|settings| settings.wrap = !settings.wrap);
            }
            ViewCommand::ShowWhitespace => {
                self.change_settings(|settings| settings.whitespace = !settings.whitespace);
            }
            ViewCommand::DocumentMap => {
                self.change_settings(|settings| settings.map = !settings.map);
            }
            ViewCommand::ZoomIn => self.set_zoom(Zoom::zoom_in),
            ViewCommand::ZoomOut => self.set_zoom(Zoom::zoom_out),
            ViewCommand::ZoomReset => self.set_zoom(|_| Zoom::default()),
            ViewCommand::FullScreen => {
                let window = &self.inner.window;
                if window.is_fullscreen() {
                    window.unfullscreen();
                } else {
                    window.fullscreen();
                }
            }
        }
    }

    fn change_settings(&self, change: impl FnOnce(&mut ViewSettings)) {
        let mut settings = self.inner.settings.get();
        change(&mut settings);
        self.set_view_settings(settings);
    }

    /// Applies the window's view settings to every editor and the View menu's check marks.
    pub(in crate::window) fn set_view_settings(&self, settings: ViewSettings) {
        self.inner.settings.set(settings);
        for page in self.pages() {
            page.apply_settings(settings);
        }
        for (id, on) in [
            (ActionId::WordWrap, settings.wrap),
            (ActionId::ShowWhitespace, settings.whitespace),
            (ActionId::DocumentMap, settings.map),
        ] {
            if let Some(action) = self.action(id) {
                action.set_state(&on.to_variant());
            }
        }
    }

    fn set_zoom(&self, next: impl FnOnce(Zoom) -> Zoom) {
        let appearance = &self.inner.shared.appearance;
        appearance.set_zoom(next(appearance.zoom()));
    }
}
