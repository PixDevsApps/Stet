//! Callbacks to run when something changes, safe to add to while they run.

use std::cell::RefCell;
use std::rc::Rc;

/// One subscribed callback.
type Listener<T> = Rc<dyn Fn(&T)>;

/// The callbacks subscribed to one kind of change, each called with a `&T`.
pub struct Listeners<T: ?Sized> {
    list: RefCell<Vec<Listener<T>>>,
}

impl<T: ?Sized> Default for Listeners<T> {
    fn default() -> Self {
        Self {
            list: RefCell::new(Vec::new()),
        }
    }
}

impl<T: ?Sized> Listeners<T> {
    /// Subscribes `listener` to every later [`notify`](Self::notify).
    pub fn connect(&self, listener: impl Fn(&T) + 'static) {
        self.list.borrow_mut().push(Rc::new(listener));
    }

    /// Calls the listeners subscribed now, in the order they subscribed. The list is not
    /// borrowed while they run, so a listener may subscribe another one; that one hears the
    /// next change, not this one.
    pub fn notify(&self, value: &T) {
        let listeners = self.list.borrow().clone();
        for listener in listeners {
            listener(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn listeners_hear_each_change_in_order() {
        let listeners = Listeners::<u32>::default();
        let heard = Rc::new(RefCell::new(Vec::new()));
        for name in ["a", "b"] {
            let heard = Rc::clone(&heard);
            listeners.connect(move |value| heard.borrow_mut().push((name, *value)));
        }
        listeners.notify(&1);
        listeners.notify(&2);
        assert_eq!(*heard.borrow(), [("a", 1), ("b", 1), ("a", 2), ("b", 2)]);
    }

    #[test]
    fn a_listener_may_subscribe_while_notified() {
        let listeners = Rc::new(Listeners::<()>::default());
        let late = Rc::new(Cell::new(0));
        let weak = Rc::downgrade(&listeners);
        let counter = Rc::clone(&late);
        listeners.connect(move |()| {
            if let Some(listeners) = weak.upgrade() {
                let counter = Rc::clone(&counter);
                listeners.connect(move |()| counter.set(counter.get() + 1));
            }
        });
        listeners.notify(&());
        assert_eq!(late.get(), 0);
        listeners.notify(&());
        assert_eq!(late.get(), 1);
    }
}
