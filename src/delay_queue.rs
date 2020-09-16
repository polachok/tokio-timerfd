use crate::{ClockId, TimerFd};
use futures_util::{ready, stream::Stream};
use slab::Slab;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::future::Future;
use std::io::Error as IoError;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::{Duration, Instant};
use timerfd::{SetTimeFlags, TimerState};
use tokio::sync::Notify;

struct Entry {
    expiration: Instant,
    index: usize,
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Entry) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Entry {
    fn cmp(&self, other: &Entry) -> std::cmp::Ordering {
        self.expiration.cmp(&other.expiration)
    }
}
impl PartialEq for Entry {
    fn eq(&self, other: &Entry) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}
impl Eq for Entry {}

/// Token to a value stored in a `DelayQueue`.
#[derive(Debug)]
pub struct Key(usize);

/// A queue of delayed elements.
///
/// Once an element is inserted into the `DelayQueue`, it is yielded once the
/// specified deadline has been reached.
pub struct DelayQueue<T> {
    timerfd: TimerFd,
    slab: Slab<T>,
    heap: BinaryHeap<Reverse<Entry>>,
    task: Notify,
}

impl<T> DelayQueue<T> {
    /// Create a new, empty, `DelayQueue`
    ///
    /// The queue will not allocate storage until items are inserted into it.
    pub fn new() -> Result<DelayQueue<T>, IoError> {
        let timerfd = TimerFd::new(ClockId::Monotonic)?;
        Ok(DelayQueue {
            timerfd,
            heap: BinaryHeap::new(),
            slab: Slab::new(),
            task: Notify::new(),
        })
    }

    pub fn len(&self) -> usize {
        self.slab.len()
    }

    /// Insert `value` into the queue set to expire at a specific instant in
    /// time.
    ///
    /// This function is identical to `insert`, but takes an `Instant` instead
    /// of a `Duration`.
    ///
    /// `value` is stored in the queue until `when` is reached. At which point,
    /// `value` will be returned from [`poll`]. If `when` has already been
    /// reached, then `value` is immediately made available to poll.
    pub fn insert_at(&mut self, value: T, when: Instant) -> Key {
        let idx = self.slab.insert(value);
        self.heap.push(Reverse(Entry {
            expiration: when,
            index: idx,
        }));
        self.task.notify();
        Key(idx)
    }

    /// Insert `value` into the queue set to expire after the requested duration
    /// elapses.
    ///
    /// This function is identical to `insert_at`, but takes a `Duration`
    /// instead of an `Instant`.
    pub fn insert(&mut self, value: T, timeout: Duration) -> Key {
        self.insert_at(value, Instant::now() + timeout)
    }

    /// Clears the queue, removing all items.
    pub fn clear(&mut self) {
        // TODO: should return None
        self.heap.clear();
        self.slab.clear();
    }
}

/// An entry in `DelayQueue` that has expired and removed.
#[derive(Debug)]
pub struct Expired<T> {
    data: T,
    deadline: Instant,
    key: Key,
}

impl<T> Expired<T> {
    pub fn into_inner(self) -> T {
        self.data
    }
}

impl<T> Stream for DelayQueue<T>
where
    T: Unpin,
{
    type Item = Result<Expired<T>, IoError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        {
            let notified = self.task.notified();
            tokio::pin!(notified);
            let _ = notified.poll(cx);
        }

        loop {
            let now = Instant::now();
            if let Some(item) = self.heap.peek() {
                if item.0.expiration > now {
                    let duration = item.0.expiration - now;
                    self.timerfd
                        .set_state(TimerState::Oneshot(duration), SetTimeFlags::Default);
                } else {
                    let item = self.heap.pop().unwrap();
                    let data = self.slab.remove(item.0.index);

                    self.task.notify();

                    return Poll::Ready(Some(Ok(Expired {
                        data,
                        deadline: item.0.expiration,
                        key: Key(item.0.index),
                    })));
                };
            }
            if let Err(err) = ready!(self.timerfd.poll_read(cx)) {
                return Poll::Ready(Some(Err(err)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use tokio::stream::StreamExt;

    #[tokio::test]
    async fn delay_queue_insert() {
        let mut queue = DelayQueue::new().unwrap();
        queue.insert(3u32, Duration::from_micros(300));
        queue.insert(1u32, Duration::from_micros(100));
        queue.insert(4u32, Duration::from_micros(400));
        queue.insert(2u32, Duration::from_micros(200));
        tokio::pin!(queue);
        for ctr in 1..5u32 {
            let item = queue.next().await.unwrap().unwrap();
            assert_eq!(item.into_inner(), ctr);
        }
    }

    #[tokio::test]
    async fn delay_queue_insert_at() {
        let mut queue = DelayQueue::new().unwrap();
        let now = Instant::now();
        queue.insert_at(5u32, now + Duration::from_micros(402));
        queue.insert_at(4u32, now + Duration::from_micros(401));
        queue.insert_at(2u32, now + Duration::from_micros(200));
        queue.insert_at(1u32, now + Duration::from_micros(100));
        queue.insert_at(3u32, now + Duration::from_micros(300));
        tokio::pin!(queue);
        for ctr in 1..6u32 {
            let item = queue.next().await.unwrap().unwrap();
            assert_eq!(item.into_inner(), ctr);
        }
    }
}
