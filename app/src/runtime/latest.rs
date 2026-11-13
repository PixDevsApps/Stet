//! Only the newest run applies its result: each run takes a [`Ticket`], and a result whose
//! ticket is no longer current is dropped.

use std::cell::Cell;
use std::rc::Rc;

/// Hands out a ticket per run; only the newest ticket is current.
#[derive(Debug, Default)]
pub struct Latest {
    newest: Rc<Cell<u64>>,
}

impl Latest {
    /// Starts a run: its ticket is current until the next run starts or
    /// [`invalidate`](Self::invalidate) is called.
    pub fn next(&self) -> Ticket {
        self.invalidate();
        Ticket {
            newest: Rc::clone(&self.newest),
            id: self.newest.get(),
        }
    }

    /// Makes every ticket handed out so far stale, without starting a run.
    pub fn invalidate(&self) {
        self.newest.set(self.newest.get() + 1);
    }
}

/// One run's claim to apply its result.
#[derive(Debug, Clone)]
pub struct Ticket {
    newest: Rc<Cell<u64>>,
    id: u64,
}

impl Ticket {
    /// No newer run started and nothing invalidated this one since it was handed out.
    pub fn is_current(&self) -> bool {
        self.newest.get() == self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_newest_ticket_is_current() {
        let latest = Latest::default();
        let first = latest.next();
        assert!(first.is_current());
        let second = latest.next();
        assert!(!first.is_current());
        assert!(second.is_current());
        let copy = second.clone();
        drop(second);
        assert!(copy.is_current());
    }

    #[test]
    fn invalidate_makes_every_ticket_stale() {
        let latest = Latest::default();
        let ticket = latest.next();
        latest.invalidate();
        assert!(!ticket.is_current());
        assert!(latest.next().is_current());
    }
}
