//! This crates provides [tokio-timer](https://docs.rs/tokio-timer)-like API
//! on top of timerfd. `timerfd` is a Linux-specific API providing timer notifications as
//! file descriptor read events.
//!
//! The advantage of `timerfd` is that it has more granularity than epoll_wait(),
//! which only provides 1 millisecond timeouts. `timerfd` API allows for nanosecond
//! precision, but precise timing of the wakeup is not guaranteed on a normal
//! multitasking system.
//!
//! Despite the name, this crate is *not* a part of the tokio project.
//!
//! * [`Delay`]: A future that completes at a specified instant in time.
//! * [`Interval`] A stream that yields at fixed time intervals.
//! * [`DelayQueue`]: A queue where items are returned once the requested delay
//!   has expired.
//!
//! [`Delay`]: struct.Delay.html
//! [`DelayQueue`]: struct.DelayQueue.html
//! [`Interval`]: struct.Interval.html

use futures_core::ready;
use mio::unix::EventedFd;
use mio::{Evented, Poll, PollOpt, Ready, Token};
use std::io::Result;
use std::os::unix::io::AsRawFd;
use std::task::{self, Context};
use std::time::{Duration, Instant};
use timerfd::{SetTimeFlags, TimerFd as InnerTimerFd, TimerState};
use tokio::io::PollEvented;

pub use timerfd::ClockId;

mod delay;
mod delay_queue;
mod interval;

pub use delay::Delay;
pub use delay_queue::DelayQueue;
pub use interval::Interval;

struct Inner(InnerTimerFd);

impl Evented for Inner {
    fn register(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt) -> Result<()> {
        poll.register(&EventedFd(&self.0.as_raw_fd()), token, interest, opts)
    }

    fn reregister(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt) -> Result<()> {
        poll.reregister(&EventedFd(&self.0.as_raw_fd()), token, interest, opts)
    }

    fn deregister(&self, poll: &Poll) -> Result<()> {
        poll.deregister(&EventedFd(&self.0.as_raw_fd()))
    }
}

pub struct TimerFd(PollEvented<Inner>);

impl TimerFd {
    pub fn new(clock: ClockId) -> Result<Self> {
        let inner = PollEvented::new(Inner(InnerTimerFd::new_custom(clock, true, true)?))?;
        Ok(TimerFd(inner))
    }

    fn set_state(&mut self, state: TimerState, flags: SetTimeFlags) {
        (self.0).get_mut().0.set_state(state, flags);
    }

    fn poll_read(&mut self, cx: &mut Context) -> task::Poll<Result<()>> {
        let ready = ready!(self.0.poll_read_ready(cx, Ready::readable()))?;
        self.0.get_mut().0.read();
        let res = self.0.clear_read_ready(cx, ready);
        task::Poll::Ready(res)
    }
}

/// Create a Future that completes in `duration` from now.
pub fn sleep(duration: Duration) -> Delay {
    Delay::new(Instant::now() + duration).expect("can't create delay")
}
