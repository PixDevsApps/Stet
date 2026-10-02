//! Exact tab stops (ADR-015): the tab width times the font's exact space advance, in Pango
//! units, instead of GtkSourceView's whole-pixel stops, which drift off the character grid
//! when the advance is fractional.
//!
//! GtkSourceView writes its own pixel stops back on every CSS change of the view (font, focus,
//! hover, theme) once a tab width was set, so the window checks the visible view in every
//! layout phase and re-applies ours when they were replaced or the font or tab width changed.

use gtk4 as gtk;
use gtk4::pango;
use gtk4::prelude::*;
use sourceview5::prelude::*;
use std::cell::Cell;

/// The stop last applied, and the font and tab width it was computed for.
#[derive(Debug, Default)]
pub struct TabStops {
    serial: Cell<Option<u32>>,
    width: Cell<u32>,
    stop: Cell<Option<i32>>,
}

impl TabStops {
    /// Re-applies the stops when GtkSourceView replaced them or the font or the tab width
    /// changed. Returns whether it did.
    pub fn ensure(&self, view: &sourceview5::View) -> bool {
        let serial = view.pango_context().serial();
        let width = view.tab_width();
        let current = current_stop(view);
        if current.is_some()
            && current == self.stop.get()
            && self.serial.get() == Some(serial)
            && self.width.get() == width
        {
            return false;
        }
        let stop = stop_for(view);
        if current != Some(stop) {
            let mut tabs = pango::TabArray::new(1, false);
            tabs.set_tab(0, pango::TabAlign::Left, stop);
            view.set_tabs(&tabs);
        }
        self.serial.set(Some(serial));
        self.width.set(width);
        self.stop.set(Some(stop));
        true
    }

    pub fn invalidate(&self) {
        self.serial.set(None);
    }
}

/// The advance of a space in the view's current font, in Pango units.
pub fn space_advance(view: &impl IsA<gtk::Widget>) -> i32 {
    view.create_pango_layout(Some(" ")).size().0
}

/// The first tab stop, in Pango units: the tab width in spaces.
pub fn stop_for(view: &sourceview5::View) -> i32 {
    space_advance(view) * view.tab_width() as i32
}

/// The view's current first stop, in Pango units, if it is set in Pango units.
pub fn current_stop(view: &sourceview5::View) -> Option<i32> {
    let tabs = view.tabs()?;
    (!tabs.is_positions_in_pixels() && tabs.size() > 0).then(|| tabs.tab(0).1)
}
