//! `gtk_source_buffer_forward_iter_to_source_mark`, which sourceview5 0.11 leaves unbound.

use gtk4 as gtk;
use gtk4::glib;
use gtk4::glib::translate::{ToGlibPtr, ToGlibPtrMut};
use std::ffi::CString;

/// Moves `iter` to the next place after it with a source mark of `category`, or of any
/// category when it is `None`; false when there is none.
pub fn forward_to_source_mark(
    buffer: &sourceview5::Buffer,
    iter: &mut gtk::TextIter,
    category: Option<&str>,
) -> bool {
    let category = category.map(|category| CString::new(category).expect("no NUL in a category"));
    let category = category
        .as_ref()
        .map_or(std::ptr::null(), |category| category.as_ptr());
    // SAFETY: a valid buffer and iterator, and a NUL-terminated category that outlives the call,
    // or NULL for every category.
    unsafe {
        sourceview5::ffi::gtk_source_buffer_forward_iter_to_source_mark(
            buffer.to_glib_none().0,
            iter.to_glib_none_mut().0,
            category,
        ) != glib::ffi::GFALSE
    }
}
