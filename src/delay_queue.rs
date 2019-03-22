use crate::{ClockId, TimerFd};
use futures::{try_ready, Async, Stream, task};
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

#[derive(Debug)]
pub struct Key(usize);

pub struct DelayQueue<T> {
    timerfd: TimerFd,
    slab: Slab<T>,
    heap: BinaryHeap<Reverse<Entry>>,
    task: Option<task::Task>,
}

impl<T> DelayQueue<T> {
    pub fn new() -> Result<DelayQueue<T>, IoError> {
        let timerfd = TimerFd::new(ClockId::Monotonic)?;
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
                })))
            };
        }
        Ok(Async::NotReady)
    }

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

    pub fn insert(&mut self, value: T, timeout: Duration) -> Key {
        self.insert_at(value, Instant::now() + timeout)
    }

    pub fn clear(&mut self) {
        self.heap.clear();
        self.slab.clear();
    }
}

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
    use std::time::{Instant, Duration};
    use tokio::prelude::*;

    #[test]
    fn delay_queue_works() {
        tokio::run(future::lazy(|| {
            let now = Instant::now();
            let mut queue = DelayQueue::new().unwrap();
            queue.insert_at(3u32, now + Duration::from_micros(30));
            queue.insert_at(1u32, now + Duration::from_micros(10));
            queue.insert_at(4u32, now + Duration::from_micros(40));
            queue.insert_at(2u32, now + Duration::from_micros(20));
            let mut ctr = 1;
            queue.take(4).for_each(move |item| {
                assert_eq!(item.into_inner(), ctr);
                ctr += 1;
                Ok(())
            }).map_err(|_| ())
        }))
    }
}
