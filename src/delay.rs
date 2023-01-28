use std::future::Future;
use std::io::Error as IoError;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;

use crate::{get_clock, TimerFd};
use futures_core::ready;
use timerfd::{SetTimeFlags, TimerState};
use tokio::io::{AsyncRead, ReadBuf};

/// A future that completes at a specified instant in time.
/// Instances of Delay perform no work and complete with () once the specified deadline has been reached.
/// Delay is powered by `timerfd` and has a resolution of 1 nanosecond.
pub struct Delay {
    timerfd: TimerFd,
    deadline: Instant,
    initialized: bool,
}

impl Delay {
    /// Create a new `Delay` instance that elapses at `deadline`.
    pub fn new(deadline: Instant) -> Result<Self, IoError> {
        let timerfd = TimerFd::new(get_clock())?;
        Ok(Delay {
            timerfd,
            deadline,
            initialized: false,
        })
    }

    /// Returns the instant at which the future will complete.
    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Returns true if the `Delay` has elapsed
    pub fn is_elapsed(&self) -> bool {
        self.deadline > Instant::now()
    }

    /// Reset the `Delay` instance to a new deadline.
    pub fn reset(&mut self, deadline: Instant) {
        self.deadline = deadline;
        self.initialized = false;
    }
}

impl Future for Delay {
    type Output = Result<(), IoError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.initialized {
            let now = Instant::now();
            let duration = if self.deadline > now {
                self.deadline - now
            } else {
                return Poll::Ready(Ok(()));
            };
            self.timerfd
                .set_state(TimerState::Oneshot(duration), SetTimeFlags::Default);
            self.initialized = true;
        }
        let mut buf = [0u8; 8];
        let mut buf = ReadBuf::new(&mut buf);
        ready!(Pin::new(&mut self.as_mut().timerfd).poll_read(cx, &mut buf)?);
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn delay_zero_duration() -> Result<(), std::io::Error> {
        let now = Instant::now();
        let delay = Delay::new(Instant::now())?;
        delay.await?;
        let elapsed = now.elapsed();
        println!("{:?}", elapsed);
        assert!(elapsed < Duration::from_millis(1));
        Ok(())
    }

    #[tokio::test]
    async fn delay_works() {
        let now = Instant::now();
        let delay = Delay::new(now + Duration::from_micros(10)).unwrap();
        delay.await.unwrap();
        let elapsed = now.elapsed();
        println!("{:?}", elapsed);
        assert!(elapsed < Duration::from_millis(1));
    }
}
