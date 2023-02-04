use crate::{get_clock, TimerFd};
use futures::{task, try_ready, Async, Stream};
use slab::Slab;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::io::Error as IoError;
use std::time::{Duration, Instant};
use timerfd::{SetTimeFlags, TimerState};

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
    task: Option<task::Task>,
}

impl<T> DelayQueue<T> {
    /// Create a new, empty, `DelayQueue`
    ///
    /// The queue will not allocate storage until items are inserted into it.
    pub fn new() -> Result<DelayQueue<T>, IoError> {
        let timerfd = TimerFd::new(get_clock())?;
        Ok(DelayQueue {
            timerfd,
            heap: BinaryHeap::new(),
            slab: Slab::new(),
            task: None,
        })
    }

    fn poll_next(&mut self) -> Result<Async<Option<Expired<T>>>, IoError> {
        let now = Instant::now();
        if let Some(item) = self.heap.peek() {
            if item.0.expiration > now {
                let duration = item.0.expiration - now;
                self.timerfd
                    .set_state(TimerState::Oneshot(duration), SetTimeFlags::Default);
            } else {
                let item = self.heap.pop().unwrap();
                let data = self.slab.remove(item.0.index);

                if let Some(task) = &self.task {
                    task.notify();
                }

                return Ok(Async::Ready(Some(Expired {
                    data,
                    deadline: item.0.expiration,
                    key: Key(item.0.index),
                })));
            };
        }
        Ok(Async::NotReady)
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
        if let Some(task) = &self.task {
            task.notify();
        }
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

impl<T> Stream for DelayQueue<T> {
    type Item = Expired<T>;
    type Error = IoError;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        self.timerfd.poll_read()?;
        self.task = Some(task::current());
        let expired = try_ready!(self.poll_next());
        Ok(Async::Ready(expired))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use tokio::prelude::*;

    #[test]
    fn delay_queue_insert() {
        tokio::run(future::lazy(|| {
            let mut queue = DelayQueue::new().unwrap();
            queue.insert(3u32, Duration::from_micros(300));
            queue.insert(1u32, Duration::from_micros(100));
            queue.insert(4u32, Duration::from_micros(400));
            queue.insert(2u32, Duration::from_micros(200));
            let mut ctr = 1;
            queue
                .take(4)
                .for_each(move |item| {
                    assert_eq!(item.into_inner(), ctr);
                    ctr += 1;
                    Ok(())
                })
                .map_err(|_| ())
        }))
    }

    #[test]
    fn delay_queue_insert_at() {
        tokio::run(future::lazy(|| {
            let mut queue = DelayQueue::new().unwrap();
            let now = Instant::now();
            queue.insert_at(5u32, now + Duration::from_micros(402));
            queue.insert_at(4u32, now + Duration::from_micros(401));
            queue.insert_at(2u32, now + Duration::from_micros(200));
            queue.insert_at(1u32, now + Duration::from_micros(100));
            queue.insert_at(3u32, now + Duration::from_micros(300));
            let mut ctr = 0;
            queue
                .take(5)
                .for_each(move |item| {
                    ctr += 1;
                    assert_eq!(item.into_inner(), ctr);
                    Ok(())
                })
                .map_err(|_| ())
        }))
    }
}
