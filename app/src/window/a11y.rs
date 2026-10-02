//! Accessible names for the items of GTK's popover menus. GTK 4.22's GtkModelButton, every
//! item of a menu built from a GMenuModel (the main menu, the tab menu, the editor's context
//! menu), reaches AT-SPI with an empty name and nothing readable inside, so a screen reader
//! announced nothing (docs/upstream/gtk-popover-menu-item-names.md). Once such a menu is up,
//! every item gets its own text as its accessible label.

use super::Window;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;

impl Window {
    /// Names the menu items under the window once the menu that is opening has its widgets.
    pub(super) fn name_menu_items_soon(&self) {
        let window = self.inner.window.clone();
        glib::idle_add_local_full(glib::Priority::HIGH_IDLE, move || {
            name_menu_items(window.upcast_ref());
            glib::ControlFlow::Break
        });
    }

    /// The main menu is built when it opens (`Window::rebuild_menu`); it and the editors'
    /// context menus name their items when they open; the tab menu does from its setup
    /// (`tabs.rs`).
    pub(super) fn connect_menu_names(&self) {
        self.inner.menu_button.set_create_popup_func(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |_| Window { inner }.ensure_menu()
        ));
        self.inner.menu_button.connect_active_notify(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |button| {
                if button.is_active() {
                    Window { inner }.name_menu_items_soon();
                }
            }
        ));
    }

    /// A right click on an editor opens GtkTextView's context menu.
    pub(super) fn connect_editor_menu_names(&self, view: &gtk::Widget) {
        let click = gtk::GestureClick::builder()
            .button(gtk::gdk::BUTTON_SECONDARY)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        click.connect_pressed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |_, _, _, _| Window { inner }.name_menu_items_soon()
        ));
        view.add_controller(click);
    }
}

/// Gives every GtkModelButton under `root` its text, without mnemonic underscores, as its
/// accessible label.
pub fn name_menu_items(root: &gtk::Widget) {
    let mut named = 0;
    let mut stack = vec![root.clone()];
    while let Some(widget) = stack.pop() {
        if widget.type_().name() == "GtkModelButton"
            && widget.find_property("text").is_some()
            && let Some(text) = widget.property::<Option<String>>("text")
            && !text.is_empty()
        {
            // GTK points the button's "labelled by" at a label whose name is empty, and an
            // ARIA "labelled by" outranks a label: drop it, then give the text as the label.
            widget.reset_relation(gtk::AccessibleRelation::LabelledBy);
            widget.update_property(&[gtk::accessible::Property::Label(&without_mnemonics(&text))]);
            named += 1;
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            child = next.next_sibling();
            stack.push(next);
        }
    }
    tracing::debug!(named, "menu items named for screen readers");
}

/// `_Save` → `Save`, `a__b` → `a_b`.
fn without_mnemonics(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '_' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::without_mnemonics;

    #[test]
    fn mnemonics_go_and_doubled_underscores_stay_single() {
        assert_eq!(without_mnemonics("_Save"), "Save");
        assert_eq!(without_mnemonics("Save _As…"), "Save As…");
        assert_eq!(without_mnemonics("my__file.txt"), "my_file.txt");
        assert_eq!(without_mnemonics("plain"), "plain");
    }
}
