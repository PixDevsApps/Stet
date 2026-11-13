//! An `insert-text` handler that may take over an insertion. The bindings' `connect_insert_text`
//! hands the handler a copy of the location, so moving it does not reach the caller, as the
//! default handler's move past the inserted text does.

use gtk4 as gtk;
use gtk4::glib;
use gtk4::glib::translate::{FromGlibPtrBorrow, FromGlibPtrNone, ToGlibPtr};
use gtk4::prelude::*;
use std::ffi::{CStr, c_char, c_int};

/// Connects `handler` to `buffer`'s `insert-text`, before the default handler. It gets the
/// buffer, the location and the text (an insertion whose text is not UTF-8 never reaches it).
/// When it returns an iterator, the caller's location is moved there; a handler that carries
/// out the insertion itself stops the emission and returns where the text ends.
pub fn connect_insert_text<F>(buffer: &impl IsA<gtk::TextBuffer>, handler: F)
where
    F: Fn(&gtk::TextBuffer, &gtk::TextIter, &str) -> Option<gtk::TextIter> + 'static,
{
    unsafe extern "C" fn trampoline<F>(
        buffer: *mut gtk::ffi::GtkTextBuffer,
        location: *mut gtk::ffi::GtkTextIter,
        text: *const c_char,
        len: c_int,
        data: glib::ffi::gpointer,
    ) where
        F: Fn(&gtk::TextBuffer, &gtk::TextIter, &str) -> Option<gtk::TextIter> + 'static,
    {
        // SAFETY: GTK passes a valid buffer, iterator and text of `len` bytes (or NUL-terminated
        // when `len` is negative); `data` is the handler boxed in `connect_insert_text`.
        unsafe {
            let handler = &*(data as *const F);
            let bytes = match usize::try_from(len) {
                Ok(len) => std::slice::from_raw_parts(text.cast::<u8>(), len),
                Err(_) => CStr::from_ptr(text).to_bytes(),
            };
            let Ok(text) = std::str::from_utf8(bytes) else {
                return;
            };
            let buffer = gtk::TextBuffer::from_glib_borrow(buffer);
            let iter = gtk::TextIter::from_glib_none(location.cast_const());
            if let Some(moved) = handler(&buffer, &iter, text) {
                *location = *moved.to_glib_none().0;
            }
        }
    }
    let trampoline: unsafe extern "C" fn(
        *mut gtk::ffi::GtkTextBuffer,
        *mut gtk::ffi::GtkTextIter,
        *const c_char,
        c_int,
        glib::ffi::gpointer,
    ) = trampoline::<F>;
    // SAFETY: the trampoline matches `insert-text`'s C signature, and the boxed handler is freed
    // by `connect_raw`'s destroy notify when the handler goes.
    unsafe {
        glib::signal::connect_raw(
            buffer.as_ptr().cast(),
            c"insert-text".as_ptr(),
            Some(std::mem::transmute::<*const (), unsafe extern "C" fn()>(
                trampoline as *const (),
            )),
            Box::into_raw(Box::new(handler)),
        );
    }
}
