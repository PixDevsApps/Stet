//! Only the newest run applies its result: each run takes a [`Ticket`], and a result whose
//! ticket is no longer current is dropped. [`SharedLatest`] does the same for runs whose ticket
//! travels to another thread.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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

/// Like [`Latest`], for runs on other threads: its [`Stamp`]s are plain numbers that can be
/// sent anywhere, and clones share one counter, so another thread can tell whether a stamp is
/// still current. Stamps are ordered, so a caller can also show any result newer than the one
/// it shows.
#[derive(Debug, Clone, Default)]
pub struct SharedLatest {
    newest: Arc<AtomicU64>,
}

impl SharedLatest {
    /// Starts a run: its stamp is current until the next run starts.
    pub fn next(&self) -> Stamp {
        Stamp(self.newest.fetch_add(1, Ordering::AcqRel) + 1)
    }

    /// The stamp of the newest run started so far.
    pub fn newest(&self) -> Stamp {
        Stamp(self.newest.load(Ordering::Acquire))
    }

    /// No newer run started since `stamp` was handed out.
    pub fn is_current(&self, stamp: Stamp) -> bool {
        self.newest() == stamp
    }
}

/// One run of a [`SharedLatest`]; a later run's stamp compares greater.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp(u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_stamps_are_ordered_and_only_the_newest_is_current() {
        let latest = SharedLatest::default();
        let before = latest.newest();
        let first = latest.next();
        let other = latest.clone();
        let second = std::thread::spawn(move || other.next()).join().unwrap();
        assert!(before < first && first < second);
        assert!(!latest.is_current(first));
        assert!(latest.is_current(second));
        assert_eq!(latest.newest(), second);
    }

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
