use crate::{ClockId, TimerFd};
use futures::{try_ready, Async, Stream};
use std::io::Error as IoError;
use std::time::{Duration, Instant};
use timerfd::{SetTimeFlags, TimerState};

/// A stream representing notifications at fixed interval
pub struct Interval {
    timerfd: TimerFd,
    at: Instant,
    duration: Duration,
    initialized: bool,
}

impl Interval {
    /// Create a new `Interval` that starts at `at` and yields every `duration`
    /// interval after that.
    /// The `duration` argument must be a non-zero duration.
    ///
    /// # Panics
    ///
    /// This function panics if `duration` is zero.
    pub fn new(at: Instant, duration: Duration) -> Result<Interval, IoError> {
        let timerfd = TimerFd::new(ClockId::Monotonic)?;
        assert!(
            duration > Duration::new(0, 0),
            "`duration` must be non-zero."
        );
        Ok(Interval {
            timerfd,
            at,
            duration,
            initialized: false,
        })
    }

    /// Creates new `Interval` that yields with interval of `duration`.
    ///
    /// The function is shortcut for `Interval::new(Instant::now() + duration, duration)`.
    ///
    /// The `duration` argument must be a non-zero duration.
    ///
    /// # Panics
    ///
    /// This function panics if `duration` is zero.
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
            let mut first_duration = if self.at > now {
                self.at - now
            } else {
                self.duration
            };
            if first_duration == Duration::from_millis(0) {
                first_duration = self.duration
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use tokio::prelude::*;

    #[test]
    fn interval_works_zero() {
        tokio::run(future::lazy(|| {
            let now = Instant::now();
            let interval = Interval::new(Instant::now(), Duration::from_micros(0)).unwrap();
            interval
                .take(2)
                .map_err(|err| panic!("{:?}", err))
                .for_each(move |_| Ok(()))
                .and_then(move |_| {
                    let elapsed = now.elapsed();
                    println!("{:?}", elapsed);
                    assert!(elapsed < Duration::from_millis(1));
                    Ok(())
                })
        }));
    }

    #[test]
    fn interval_works() {
        tokio::run(future::lazy(|| {
            let now = Instant::now();
            let interval = Interval::new_interval(Duration::from_micros(1)).unwrap();
            interval
                .take(2)
                .map_err(|err| panic!("{:?}", err))
                .for_each(move |_| Ok(()))
                .and_then(move |_| {
                    let elapsed = now.elapsed();
                    println!("{:?}", elapsed);
                    assert!(elapsed < Duration::from_millis(1));
                    Ok(())
                })
        }));
    }
}
