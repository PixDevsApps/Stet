//! A NULL-safe `GtkAccessibleText.get_selection` for the editor.
//!
//! GTK 4.22's AT-SPI `Text.AddSelection` handler (`gtkatspitext.c`) calls
//! `gtk_accessible_text_get_selection (text, &n_ranges, NULL)`, and GtkTextView's
//! implementation writes the ranges through that NULL whenever a selection exists, which
//! crashes the application (fixed upstream on main, not in 4.22;
//! docs/upstream/gtk-accessible-text-add-selection.md). `StetView` implements the interface
//! again: GObject starts its vtable as a copy of GtkTextView's, and only `get_selection` is
//! replaced, by a function that calls GtkTextView's with storage of its own and hands the
//! ranges out only when the caller asked for them.

use super::view::StetView;
use gtk4::ffi;
use gtk4::glib;
use gtk4::glib::translate::IntoGlib;
use gtk4::glib::{ffi as glib_ffi, gobject_ffi};
use gtk4::prelude::*;
use std::sync::OnceLock;

type GetSelection = unsafe extern "C" fn(
    *mut ffi::GtkAccessibleText,
    *mut usize,
    *mut *mut ffi::GtkAccessibleTextRange,
) -> glib_ffi::gboolean;

/// GtkTextView's `get_selection`, which ours chains to.
static PARENT: OnceLock<GetSelection> = OnceLock::new();

/// Adds GtkAccessibleText to `type_` again, with [`interface_init`].
///
/// # Safety
///
/// `type_` must be a registered subclass of GtkTextView whose class is not initialized yet.
pub(super) unsafe fn override_accessible_text(type_: glib::Type) {
    let info = gobject_ffi::GInterfaceInfo {
        interface_init: Some(interface_init),
        interface_finalize: None,
        interface_data: std::ptr::null_mut(),
    };
    // SAFETY: GObject copies `info`; the interface may be added again to a subclass whose
    // class is not initialized yet.
    unsafe {
        gobject_ffi::g_type_add_interface_static(
            type_.into_glib(),
            ffi::gtk_accessible_text_get_type(),
            &info,
        );
    }
}

unsafe extern "C" fn interface_init(iface: glib_ffi::gpointer, _data: glib_ffi::gpointer) {
    // SAFETY: GObject passes this type's GtkAccessibleTextInterface vtable, already filled
    // with a copy of the parent's.
    unsafe {
        let parent = gobject_ffi::g_type_interface_peek_parent(iface)
            as *const ffi::GtkAccessibleTextInterface;
        let iface = &mut *(iface as *mut ffi::GtkAccessibleTextInterface);
        let chained = if parent.is_null() {
            iface.get_selection
        } else {
            (*parent).get_selection
        };
        if let Some(chained) = chained {
            let _ = PARENT.set(chained);
        }
        iface.get_selection = Some(get_selection);
    }
}

unsafe extern "C" fn get_selection(
    text: *mut ffi::GtkAccessibleText,
    n_ranges: *mut usize,
    ranges: *mut *mut ffi::GtkAccessibleTextRange,
) -> glib_ffi::gboolean {
    // SAFETY: `text` is a StetView; `n_ranges` and `ranges` may each be NULL. The parent
    // allocates the ranges with g_new, so g_free releases them when nobody wants them.
    unsafe {
        let mut count = 0usize;
        let mut found: *mut ffi::GtkAccessibleTextRange = std::ptr::null_mut();
        let selected = match PARENT.get() {
            Some(parent) => parent(text, &mut count, &mut found),
            None => glib_ffi::GFALSE,
        };
        if selected == glib_ffi::GFALSE {
            count = 0;
        }
        if !n_ranges.is_null() {
            *n_ranges = count;
        }
        if ranges.is_null() {
            glib_ffi::g_free(found.cast());
        } else {
            *ranges = found;
        }
        selected
    }
}

/// What `gtk_accessible_text_get_selection` returns for `view`, called through the interface
/// the way GTK's AT-SPI `AddSelection` calls it: with `ranges` NULL when `with_ranges` is
/// false. Returns the result, the range count, the ranges when asked for, and whether the
/// vtable's function is this module's.
pub fn probe_selection(
    view: &StetView,
    with_ranges: bool,
) -> (bool, usize, Vec<(usize, usize)>, bool) {
    // SAFETY: the view implements GtkAccessibleText; the vtable comes from its class, as
    // GTK_ACCESSIBLE_TEXT_GET_IFACE does; the ranges are freed after copying.
    unsafe {
        let instance: *mut gobject_ffi::GTypeInstance = view.as_ptr().cast();
        let iface = gobject_ffi::g_type_interface_peek(
            (*instance).g_class.cast(),
            ffi::gtk_accessible_text_get_type(),
        ) as *const ffi::GtkAccessibleTextInterface;
        let function = (*iface).get_selection.expect("a get_selection");
        let ours = function as usize == get_selection as GetSelection as usize;
        let text: *mut ffi::GtkAccessibleText = view.as_ptr().cast();
        let mut count = usize::MAX;
        let mut found: *mut ffi::GtkAccessibleTextRange = std::ptr::null_mut();
        let selected = if with_ranges {
            function(text, &mut count, &mut found)
        } else {
            function(text, &mut count, std::ptr::null_mut())
        };
        let mut ranges = Vec::new();
        if !found.is_null() {
            for index in 0..count {
                let range = &*found.add(index);
                ranges.push((range.start, range.length));
            }
            glib_ffi::g_free(found.cast());
        }
        (selected != glib_ffi::GFALSE, count, ranges, ours)
    }
}
