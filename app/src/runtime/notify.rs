//! Waiting on the GTK thread for something to change, without polling.

use gtk4::glib;
use std::cell::RefCell;
use std::future::Future;
use std::time::{Duration, Instant};

/// Wakes the tasks waiting on it. A task subscribes with [`notified`](Self::notified) or
/// [`any`](Self::any) and is woken by the next [`notify`](Self::notify) after that.
#[derive(Debug, Default)]
pub struct Notify {
    waiters: RefCell<Vec<async_channel::Sender<()>>>,
}

impl Notify {
    /// Wakes every task subscribed now.
    pub fn notify(&self) {
        for waiter in self.waiters.take() {
            let _ = waiter.try_send(());
        }
    }

    /// Resolves at the next [`notify`](Self::notify). It subscribes when called, not when first
    /// polled, so a change right after a check cannot be missed.
    pub fn notified(&self) -> impl Future<Output = ()> + use<> {
        Self::any(&[self])
    }

    /// Resolves at the next [`notify`](Self::notify) of any of `sources`; at once when there
    /// are none. Subscribes when called.
    pub fn any(sources: &[&Self]) -> impl Future<Output = ()> + use<> {
        let (sender, receiver) = async_channel::bounded(1);
        for source in sources {
            let mut waiters = source.waiters.borrow_mut();
            waiters.retain(|waiter| !waiter.is_closed());
            waiters.push(sender.clone());
        }
        drop(sender);
        async move {
            let _ = receiver.recv().await;
        }
    }
}

/// Waits until `ready` holds or `limit` has passed, and returns whether it holds. `ready` is
/// checked at once, and again each time the future `wake` returns resolves; `wake` is called
/// right after each check, so it subscribes to what `ready` reads as it was checked. Without a
/// limit it waits as long as it takes.
pub async fn wait_until<W: Future<Output = ()>>(
    limit: Option<Duration>,
    mut ready: impl FnMut() -> bool,
    mut wake: impl FnMut() -> W,
) -> bool {
    let deadline = limit.map(|limit| Instant::now() + limit);
    loop {
        if ready() {
            return true;
        }
        let woken = wake();
        match deadline {
            None => woken.await,
            Some(deadline) => {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() || glib::future_with_timeout(left, woken).await.is_err() {
                    return ready();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::debounce::tests::{iterate_until, main_context};
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn notify_wakes_who_subscribed_before_it() {
        let (_guard, context) = main_context();
        let _owner = context.acquire().expect("the default main context");
        let notify = Rc::new(Notify::default());
        let woken = Rc::new(Cell::new(0));
        for _ in 0..2 {
            let subscribed = notify.notified();
            let woken = woken.clone();
            context.spawn_local(async move {
                subscribed.await;
                woken.set(woken.get() + 1);
            });
        }
        notify.notify();
        iterate_until(&context, || woken.get() == 2);
        let late = notify.notified();
        let woken_late = Rc::new(Cell::new(false));
        let flag = woken_late.clone();
        context.spawn_local(async move {
            late.await;
            flag.set(true);
        });
        for _ in 0..5 {
            context.iteration(false);
        }
        assert!(!woken_late.get());
        notify.notify();
        iterate_until(&context, || woken_late.get());
    }

    #[test]
    fn any_wakes_on_either_source() {
        let (_guard, context) = main_context();
        let _owner = context.acquire().expect("the default main context");
        let (first, second) = (Notify::default(), Notify::default());
        let woken = Rc::new(Cell::new(false));
        let flag = woken.clone();
        let subscribed = Notify::any(&[&first, &second]);
        context.spawn_local(async move {
            subscribed.await;
            flag.set(true);
        });
        second.notify();
        iterate_until(&context, || woken.get());
        let _again = first.notified();
        assert_eq!(
            first.waiters.borrow().len(),
            1,
            "the woken waiter is pruned"
        );
    }

    #[test]
    fn wait_until_checks_after_each_notification_and_times_out() {
        let (_guard, context) = main_context();
        let _owner = context.acquire().expect("the default main context");
        let notify = Rc::new(Notify::default());
        let value = Rc::new(Cell::new(0));
        let result = Rc::new(Cell::new(None));
        context.spawn_local({
            let (notify, value, result) = (notify.clone(), value.clone(), result.clone());
            async move {
                let ready = wait_until(
                    Some(Duration::from_secs(1)),
                    || value.get() == 2,
                    || notify.notified(),
                )
                .await;
                result.set(Some(ready));
            }
        });
        for step in 1..=2 {
            context.iteration(false);
            value.set(step);
            notify.notify();
        }
        iterate_until(&context, || result.get().is_some());
        assert_eq!(result.get(), Some(true));

        result.set(None);
        context.spawn_local({
            let result = result.clone();
            async move {
                let ready = wait_until(
                    Some(Duration::from_millis(20)),
                    || false,
                    || notify.notified(),
                )
                .await;
                result.set(Some(ready));
            }
        });
        iterate_until(&context, || result.get().is_some());
        assert_eq!(result.get(), Some(false));
    }
}
