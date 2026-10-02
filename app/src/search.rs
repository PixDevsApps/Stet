//! Asynchronous Find Next/Previous over a GtkSourceView search context.
//!
//! sourceview5 0.11.2's `forward_async`/`backward_async` (and their futures) ignore the
//! `gboolean` that `gtk_source_search_context_forward_finish` returns, so when nothing matches
//! they hand back uninitialised iterators as a match; selecting them aborts in GTK
//! (docs/upstream/sourceview5-replace-all.md). This wrapper calls the C API itself and honours
//! that return value. The synchronous `forward`/`backward` are correct but block on large
//! buffers.

use gtk4 as gtk;
use gtk4::glib;
use gtk4::glib::translate::{ToGlibPtr, ToGlibPtrMut};
use sourceview5::prelude::*;

/// A match: its bounds and whether the search wrapped around the end of the buffer.
pub type Match = (gtk::TextIter, gtk::TextIter, bool);

struct Pending {
    forward: bool,
    buffer: sourceview5::Buffer,
    sender: async_channel::Sender<Option<Match>>,
}

/// Finds the next (`forward`) or previous match from `from`, or `None`.
pub async fn find(
    search: &sourceview5::SearchContext,
    from: &gtk::TextIter,
    forward: bool,
) -> Option<Match> {
    let (sender, receiver) = async_channel::bounded(1);
    let pending = Box::new(Pending {
        forward,
        buffer: search.buffer(),
        sender,
    });
    unsafe extern "C" fn done(
        source: *mut glib::gobject_ffi::GObject,
        result: *mut gtk4::gio::ffi::GAsyncResult,
        data: glib::ffi::gpointer,
    ) {
        // SAFETY: `data` is the `Pending` boxed below, handed to exactly one callback; the
        // iterators are initialised from the buffer before the finish function overwrites them.
        unsafe {
            let pending = Box::from_raw(data as *mut Pending);
            let mut start = pending.buffer.start_iter();
            let mut end = pending.buffer.start_iter();
            let mut wrapped = 0;
            let mut error = std::ptr::null_mut();
            let finish = if pending.forward {
                sourceview5::ffi::gtk_source_search_context_forward_finish
            } else {
                sourceview5::ffi::gtk_source_search_context_backward_finish
            };
            let found = finish(
                source as *mut _,
                result,
                start.to_glib_none_mut().0,
                end.to_glib_none_mut().0,
                &mut wrapped,
                &mut error,
            ) != 0;
            let failed = !error.is_null();
            if failed {
                glib::ffi::g_error_free(error);
            }
            let found = (found && !failed).then_some((start, end, wrapped != 0));
            let _ = pending.sender.try_send(found);
        }
    }
    // SAFETY: the context and the iterator outlive the call (GIO copies the iterator), and
    // `done` takes back ownership of `pending`.
    unsafe {
        let start = if forward {
            sourceview5::ffi::gtk_source_search_context_forward_async
        } else {
            sourceview5::ffi::gtk_source_search_context_backward_async
        };
        start(
            search.to_glib_none().0,
            from.to_glib_none().0,
            std::ptr::null_mut(),
            Some(done),
            Box::into_raw(pending) as glib::ffi::gpointer,
        );
    }
    receiver.recv().await.ok().flatten()
}
