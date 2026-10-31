//! Bounded worker-result queues whose consumer never waits on a producer.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, TryLockError};

#[cfg(any(test, feature = "publication-test-support"))]
use crossbeam_channel::TrySendError;
use crossbeam_channel::{Receiver, SendError, Sender, TryRecvError, bounded};

struct Queue<T> {
    items: VecDeque<T>,
    /// Senders waiting in [`MailboxSender::send`] for space.
    blocked_senders: usize,
    /// Set when the last receiver drops, so blocked and later sends return their result.
    closed: bool,
}

struct Shared<T> {
    queue: Mutex<Queue<T>>,
    space: Condvar,
    capacity: usize,
    /// Published results, readable without the lock.
    len: AtomicUsize,
    receivers: AtomicUsize,
    /// Holds a token after each publication, so a sleeping consumer can select on it.
    doorbell: (Sender<()>, Receiver<()>),
}

impl<T> Shared<T> {
    /// Locks producer state, retaining the queue if a previous owner panicked.
    fn lock(&self) -> MutexGuard<'_, Queue<T>> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Publishes one reserved result before notifying an idle consumer.
    fn publish(&self, mut queue: MutexGuard<'_, Queue<T>>, result: T) {
        queue.items.push_back(result);
        self.len.store(queue.items.len(), Ordering::Release);
        drop(queue);
        let _ = self.doorbell.0.try_send(());
    }

    /// Pops the oldest result, waking a sender blocked on capacity after unlocking.
    fn take(&self, mut queue: MutexGuard<'_, Queue<T>>) -> Option<T> {
        let result = queue.items.pop_front()?;
        self.len.store(queue.items.len(), Ordering::Release);
        let wake = queue.blocked_senders != 0;
        drop(queue);
        if wake {
            self.space.notify_one();
        }
        Some(result)
    }
}

/// Producer side of a [`Mailbox`].
pub(in crate::stream) struct MailboxSender<T> {
    shared: Arc<Shared<T>>,
}

/// Consumer side of a bounded result queue. A result is visible only once fully published, and
/// [`try_recv`](Self::try_recv) skips a publication in progress instead of waiting for it.
pub(in crate::stream) struct Mailbox<T> {
    shared: Arc<Shared<T>>,
}

/// Creates a mailbox holding at most `capacity` results.
pub(in crate::stream) fn result_mailbox<T>(capacity: usize) -> (MailboxSender<T>, Mailbox<T>) {
    assert!(capacity > 0, "a result mailbox needs capacity");
    let shared = Arc::new(Shared {
        queue: Mutex::new(Queue {
            items: VecDeque::with_capacity(capacity),
            blocked_senders: 0,
            closed: false,
        }),
        space: Condvar::new(),
        capacity,
        len: AtomicUsize::new(0),
        receivers: AtomicUsize::new(1),
        doorbell: bounded(1),
    });
    (
        MailboxSender {
            shared: Arc::clone(&shared),
        },
        Mailbox { shared },
    )
}

impl<T> MailboxSender<T> {
    /// Publishes `result` in FIFO order, waiting while the mailbox is full; returns it unsent
    /// once every receiver has dropped.
    pub(in crate::stream) fn send(&self, result: T) -> Result<(), SendError<T>> {
        let shared = &*self.shared;
        let mut queue = shared.lock();
        while queue.items.len() >= shared.capacity && !queue.closed {
            queue.blocked_senders += 1;
            queue = shared
                .space
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner);
            queue.blocked_senders -= 1;
        }
        if queue.closed {
            return Err(SendError(result));
        }
        shared.publish(queue, result);
        Ok(())
    }

    /// Publishes `result` unless the mailbox is full or closed.
    #[cfg(any(test, feature = "publication-test-support"))]
    pub(in crate::stream) fn try_send(&self, result: T) -> Result<(), TrySendError<T>> {
        let shared = &*self.shared;
        let queue = shared.lock();
        if queue.closed {
            return Err(TrySendError::Disconnected(result));
        }
        if queue.items.len() >= shared.capacity {
            return Err(TrySendError::Full(result));
        }
        shared.publish(queue, result);
        Ok(())
    }

    /// Reserves space in an available test mailbox until the returned publisher is called.
    #[cfg(test)]
    pub(in crate::stream) fn reserve_publication(&self) -> impl FnOnce(T) + '_ {
        let queue = self.shared.lock();
        assert!(!queue.closed && queue.items.len() < self.shared.capacity);
        move |result| self.shared.publish(queue, result)
    }
}

impl<T> Clone for MailboxSender<T> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> Mailbox<T> {
    /// Takes the oldest published result. Reports `Empty` while a producer holds the queue, so the
    /// caller leaves that result for a later call. Never reports `Disconnected`.
    pub(in crate::stream) fn try_recv(&self) -> Result<T, TryRecvError> {
        let queue = match self.shared.queue.try_lock() {
            Ok(queue) => queue,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return Err(TryRecvError::Empty),
        };
        self.shared.take(queue).ok_or(TryRecvError::Empty)
    }

    /// Reports whether any fully published results remain, without locking the queue.
    #[must_use]
    pub(in crate::stream) fn is_empty(&self) -> bool {
        self.shared.len.load(Ordering::Acquire) == 0
    }

    /// Ready after any publication since its last receive. Drain it, check the mailbox, then
    /// select on it to sleep without missing a later result.
    pub(in crate::stream) fn doorbell(&self) -> &Receiver<()> {
        &self.shared.doorbell.1
    }
}

#[cfg(test)]
impl<T> Mailbox<T> {
    /// Counts published results without waiting on a producer.
    pub(in crate::stream) fn len(&self) -> usize {
        self.shared.len.load(Ordering::Acquire)
    }

    /// Waits up to `timeout` for a result, blocking on producers as a test may.
    pub(in crate::stream) fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<T, crossbeam_channel::RecvTimeoutError> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(result) = self.shared.take(self.shared.lock()) {
                return Ok(result);
            }
            self.doorbell()
                .recv_deadline(deadline)
                .map_err(|_| crossbeam_channel::RecvTimeoutError::Timeout)?;
        }
    }

    /// Takes published results until the mailbox is empty or busy.
    pub(in crate::stream) fn try_iter(&self) -> impl Iterator<Item = T> + '_ {
        std::iter::from_fn(|| self.try_recv().ok())
    }
}

impl<T> Clone for Mailbox<T> {
    fn clone(&self) -> Self {
        self.shared.receivers.fetch_add(1, Ordering::Relaxed);
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> Drop for Mailbox<T> {
    fn drop(&mut self) {
        if self.shared.receivers.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        let mut queue = self.shared.lock();
        queue.closed = true;
        let items = std::mem::take(&mut queue.items);
        self.shared.len.store(0, Ordering::Release);
        let wake = queue.blocked_senders != 0;
        drop(queue);
        if wake {
            self.shared.space.notify_all();
        }
        drop(items);
    }
}

#[cfg(test)]
mod tests;
