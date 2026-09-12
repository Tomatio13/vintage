//! A single-consumer, coalescing wakeup channel. It carries no terminal bytes.
use std::{
    future::poll_fn,
    sync::{Arc, Mutex},
    task::{Poll, Waker},
};

#[derive(Default)]
struct State {
    pending: bool,
    closed: bool,
    waker: Option<Waker>,
}

pub(crate) struct UpdateSender(Arc<Mutex<State>>);

/// One UI consumer waits for changes without occupying a worker or polling.
pub struct Updates(Arc<Mutex<State>>);

pub(crate) fn channel() -> (UpdateSender, Updates) {
    let state = Arc::new(Mutex::new(State::default()));
    (UpdateSender(state.clone()), Updates(state))
}

impl UpdateSender {
    pub(crate) fn notify(&self) {
        let waker = {
            let mut state = self.0.lock().expect("update mutex poisoned");
            if state.closed {
                return;
            }
            state.pending = true;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    pub(crate) fn close(&self) {
        let waker = {
            let mut state = self.0.lock().expect("update mutex poisoned");
            state.closed = true;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl Drop for UpdateSender {
    fn drop(&mut self) {
        self.close();
    }
}

impl Updates {
    /// True means inspect the latest state. False means the service closed.
    /// Registering the waker and inspecting the flag share a lock, preventing
    /// a notification from being lost between the two operations.
    pub async fn next(&mut self) -> bool {
        poll_fn(|cx| {
            let mut state = self.0.lock().expect("update mutex poisoned");
            if std::mem::take(&mut state.pending) {
                Poll::Ready(true)
            } else if state.closed {
                Poll::Ready(false)
            } else {
                state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}

impl Drop for Updates {
    fn drop(&mut self) {
        let mut state = self.0.lock().expect("update mutex poisoned");
        state.closed = true;
        state.waker = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        pin::pin,
        sync::atomic::{AtomicUsize, Ordering},
        task::{Context, Wake},
    };

    #[derive(Default)]
    struct Counter(AtomicUsize);
    impl Wake for Counter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn idle_wait_does_not_wake_and_burst_coalesces() {
        let (sender, mut receiver) = channel();
        let counter = Arc::new(Counter::default());
        let waker = Waker::from(counter.clone());
        let mut cx = Context::from_waker(&waker);
        let mut wait = pin!(receiver.next());
        assert!(wait.as_mut().poll(&mut cx).is_pending());
        assert_eq!(counter.0.load(Ordering::Relaxed), 0);
        for _ in 0..10_000 {
            sender.notify();
        }
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);
        assert_eq!(wait.as_mut().poll(&mut cx), Poll::Ready(true));
    }

    #[test]
    fn notification_before_wait_and_close_preserve_final_refresh() {
        let (sender, mut receiver) = channel();
        sender.notify();
        drop(sender);
        let mut cx = Context::from_waker(Waker::noop());
        assert_eq!(pin!(receiver.next()).poll(&mut cx), Poll::Ready(true));
        assert_eq!(pin!(receiver.next()).poll(&mut cx), Poll::Ready(false));
    }

    #[test]
    fn close_wakes_waiter_and_receiver_drop_releases_waker() {
        let (sender, mut receiver) = channel();
        let counter = Arc::new(Counter::default());
        let waker = Waker::from(counter.clone());
        let mut cx = Context::from_waker(&waker);
        assert!(pin!(receiver.next()).poll(&mut cx).is_pending());
        sender.close();
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);
        assert_eq!(pin!(receiver.next()).poll(&mut cx), Poll::Ready(false));
        let (sender, mut receiver) = channel();
        assert!(pin!(receiver.next()).poll(&mut cx).is_pending());
        drop(receiver);
        assert_eq!(Arc::strong_count(&counter), 2);
        sender.notify(); // Closed consumer is a no-op, never a blocking send.
    }

    #[test]
    fn concurrent_registration_and_notification_cannot_lose_wakeup() {
        for _ in 0..100 {
            let (sender, mut receiver) = channel();
            let counter = Arc::new(Counter::default());
            let waker = Waker::from(counter.clone());
            let mut cx = Context::from_waker(&waker);
            let worker = std::thread::spawn(move || {
                sender.notify();
                sender
            });
            let mut wait = pin!(receiver.next());
            let first = wait.as_mut().poll(&mut cx);
            let _sender = worker.join().unwrap();
            if first.is_pending() {
                assert_eq!(counter.0.load(Ordering::Relaxed), 1);
                assert_eq!(wait.as_mut().poll(&mut cx), Poll::Ready(true));
            } else {
                assert_eq!(first, Poll::Ready(true));
            }
        }
    }
}
