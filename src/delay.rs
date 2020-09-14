use crate::{ClockId, TimerFd};
use futures_core::ready;
use std::future::Future;
use std::io::Error as IoError;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;
use timerfd::{SetTimeFlags, TimerState};
use tokio::sync::Notify;

/// A future that completes at a specified instant in time.
/// Instances of Delay perform no work and complete with () once the specified deadline has been reached.
/// Delay is powered by `timerfd` and has a resolution of 1 nanosecond.
pub struct Delay {
    timerfd: TimerFd,
    deadline: Instant,
    initialized: bool,
    task: Notify,
}

impl Delay {
    /// Create a new `Delay` instance that elapses at `deadline`.
    pub fn new(deadline: Instant) -> Result<Self, IoError> {
        let timerfd = TimerFd::new(ClockId::Monotonic)?;
        Ok(Delay {
            timerfd,
            deadline,
            initialized: false,
            task: Notify::new(),
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
        self.task.notify()
    }
}

impl Future for Delay {
    type Output = Result<(), IoError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        {
            let notified = self.task.notified();
            tokio::pin!(notified);
            let _ = notified.poll(cx);
        }
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

        let res = ready!(self.timerfd.poll_read(cx));
        Poll::Ready(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn delay_zero_duration() {
        let now = Instant::now();
        Delay::new(Instant::now()).unwrap().await.unwrap();
        let elapsed = now.elapsed();
        println!("{:?}", elapsed);
        assert!(elapsed < Duration::from_millis(1));
    }

    #[tokio::test]
    async fn dropped_delay_doesnt_fire() {
        use futures_util::TryFutureExt;
        let now = Instant::now();
        let delay = Delay::new(now + Duration::from_millis(500))
            .unwrap()
            .and_then(|()| async {
                panic!();
                Ok(1u32)
            });
        tokio::pin!(delay);
        let ready = async { 1i32 };
        tokio::pin!(ready);
        futures_util::future::select(ready, delay).await;
    }

    #[tokio::test]
    async fn delay_works() {
        let now = Instant::now();
        let delay = Delay::new(now + Duration::from_micros(90)).unwrap();
        delay.await.unwrap();
        let elapsed = now.elapsed();
        println!("{:?}", elapsed);
        assert!(elapsed < Duration::from_millis(1));
    }
}
