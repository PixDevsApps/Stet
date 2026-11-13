//! Signal handlers that run once.

use gtk4::glib;
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Connects `handler` to the first emission of a signal of `object` only: it disconnects
/// itself, then runs. `connect` makes the connection with the closure it is given, for any
/// signal whose handler takes just the emitter:
/// `connect_once(&clock, |clock, run| clock.connect_after_paint(run), |clock| …)`.
pub fn connect_once<O: IsA<glib::Object>>(
    object: &O,
    connect: impl FnOnce(&O, Box<dyn Fn(&O)>) -> glib::SignalHandlerId,
    handler: impl FnOnce(&O) + 'static,
) {
    let id: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::default();
    let handler = Cell::new(Some(handler));
    let connected = connect(
        object,
        Box::new(glib::clone!(
            #[strong]
            id,
            move |object: &O| {
                if let Some(id) = id.take() {
                    object.disconnect(id);
                }
                if let Some(handler) = handler.take() {
                    handler(object);
                }
            }
        )),
    );
    id.replace(Some(connected));
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk4::gio;

    #[test]
    fn runs_on_the_first_emission_only() {
        let action = gio::SimpleAction::new("test", None);
        let runs = Rc::new(Cell::new(0));
        let counted = runs.clone();
        connect_once(
            &action,
            |action, run| action.connect_enabled_notify(run),
            move |action| counted.set(counted.get() + usize::from(!action.is_enabled())),
        );
        action.set_enabled(false);
        action.set_enabled(true);
        action.set_enabled(false);
        assert_eq!(runs.get(), 1);
    }
}
