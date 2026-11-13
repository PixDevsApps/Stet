//! The View menu's window-wide commands: word wrap, whitespace and the document map, which
//! every editor of the window shares; zoom, which every window shares; and full screen.

use crate::editor::ViewSettings;
use crate::window::Window;
use gtk4::prelude::*;
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

    fn set_zoom(&self, next: impl FnOnce(Zoom) -> Zoom) {
        let appearance = &self.inner.shared.appearance;
        appearance.set_zoom(next(appearance.zoom()));
    }
}
