use crate::{ClockId, Delay, TimerFd};
use futures::{try_ready, Async, Stream};
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

pub struct Key(usize);

pub struct DelayQueue<T> {
    timerfd: TimerFd,
    slab: Slab<T>,
    heap: BinaryHeap<Reverse<Entry>>,
}

impl<T> DelayQueue<T> {
    pub fn new() -> Result<DelayQueue<T>, IoError> {
        let timerfd = TimerFd::new(ClockId::Monotonic)?;
        Ok(DelayQueue {
            timerfd,
            heap: BinaryHeap::new(),
            slab: Slab::new(),
        })
    }

    pub fn insert_at(&mut self, value: T, when: Instant) -> Key {
        let idx = self.slab.insert(value);
        self.heap.push(Reverse(Entry {
            expiration: when,
            index: idx,
        }));
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
        match self.timerfd.poll_read() {
            Ok(Async::Ready(_)) => {
                // process everything
                //if let Some(item) = self.heap.peek() {}
            }
            Ok(Async::NotReady) => {
                if let Some(item) = self.heap.peek() {
                    let now = Instant::now();
                    let duration = if item.0.expiration > now {
                        item.0.expiration - now
                    } else {
                        unimplemented!()
                    };
                    self.timerfd
                        .set_state(TimerState::Oneshot(duration), SetTimeFlags::Default);
                }
            }
            Err(err) => return Err(err),
        }
        Ok(Async::NotReady)
    }
}
