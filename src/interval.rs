use std::io::Error as IoError;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use crate::{get_clock, TimerFd};
use futures_core::{ready, Stream};
use timerfd::{SetTimeFlags, TimerState};
use tokio::io::{AsyncRead, ReadBuf};

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
        let timerfd = TimerFd::new(get_clock())?;
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

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if !self.initialized {
            let now = Instant::now();
            let first_duration = if self.at > now {
                self.at - now
            } else {
                /* can't set it to zero as it disables timer */
                Duration::from_nanos(1)
            };
            let duration = self.duration;
            self.as_mut().timerfd.set_state(
                TimerState::Periodic {
                    current: first_duration,
                    interval: duration,
                },
                SetTimeFlags::Default,
            );
            self.initialized = true;
        }
        let mut buf = [0u8; 8];
        let mut buf = ReadBuf::new(&mut buf);
        ready!(Pin::new(&mut self.as_mut().timerfd).poll_read(cx, &mut buf)?);
        Poll::Ready(Some(Ok(())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream::StreamExt;
    use std::time::Instant;

    #[tokio::test]
    async fn interval_works() {
        let mut interval = Interval::new_interval(Duration::from_micros(1)).unwrap();

        let start = Instant::now();
        for _ in 0..5 {
            interval.next().await.unwrap().unwrap();
        }
        let elapsed = start.elapsed();
        println!("{:?}", elapsed);
        assert!(elapsed < Duration::from_millis(1));
    }

    #[tokio::test]
    async fn long_interval_works() {
        let mut interval = Interval::new_interval(Duration::from_secs(1)).unwrap();

        let start = Instant::now();
        for _ in 0..5 {
            let before = Instant::now();
            interval.next().await.unwrap().unwrap();
            let elapsed = before.elapsed();
            assert!(elapsed.as_secs_f64() > 0.99 && elapsed.as_secs_f64() < 1.1);
        }
        let elapsed = start.elapsed();
        println!("long interval elapsed: {:?}", elapsed);
        assert!(elapsed < Duration::from_secs(6));
    }
}
