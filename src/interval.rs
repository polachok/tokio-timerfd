use crate::{ClockId, TimerFd};
use futures_util::{ready, stream::Stream};
use std::io::Error as IoError;
use std::pin::Pin;
use std::task::{Context, Poll};
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
    type Item = Result<(), IoError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
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
            let duration = self.duration;
            self.timerfd.set_state(
                TimerState::Periodic {
                    current: first_duration,
                    interval: duration,
                },
                SetTimeFlags::Default,
            );
            self.initialized = true;
        }
        let res = ready!(self.timerfd.poll_read(cx));
        Poll::Ready(Some(res))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn interval_works() {
        use tokio::stream::StreamExt;
        let now = Instant::now();
        let interval = Interval::new_interval(Duration::from_micros(1)).unwrap();
        tokio::pin!(interval);
        let _ = interval.next().await;
        let _ = interval.next().await;
        let elapsed = now.elapsed();
        println!("{:?}", elapsed);
        assert!(elapsed < Duration::from_millis(1));
    }
}
