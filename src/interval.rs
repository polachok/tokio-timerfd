use crate::{ClockId, TimerFd};
use futures::{try_ready, Async, Stream};
use std::io::Error as IoError;
use std::time::{Duration, Instant};
use timerfd::{SetTimeFlags, TimerState};

pub struct Interval {
    timerfd: TimerFd,
    at: Instant,
    duration: Duration,
    initialized: bool,
}

impl Interval {
    pub fn new(at: Instant, duration: Duration) -> Result<Interval, IoError> {
        let timerfd = TimerFd::new(ClockId::Monotonic)?;
        /*
         */
        Ok(Interval {
            timerfd,
            at,
            duration,
            initialized: false,
        })
    }

    pub fn new_interval(duration: Duration) -> Result<Interval, IoError> {
        Self::new(Instant::now() + duration, duration)
    }
}

impl Stream for Interval {
    type Item = ();
    type Error = IoError;

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        if !self.initialized {
            let now = Instant::now();
            let first_duration = if self.at > now {
                self.at - now
            } else {
                Duration::from_millis(0)
            };
            self.timerfd.set_state(
                TimerState::Periodic {
                    current: first_duration,
                    interval: self.duration,
                },
                SetTimeFlags::Default,
            );
            self.initialized = true;
        }
        try_ready!(self.timerfd.poll_read());
        Ok(Async::Ready(Some(())))
    }
}
