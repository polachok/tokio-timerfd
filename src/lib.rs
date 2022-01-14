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

use std::io::{Error, Result};
use std::os::unix::io::{AsRawFd, RawFd};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures_core::ready;
use timerfd::{SetTimeFlags, TimerFd as InnerTimerFd, TimerState};
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncRead, Interest, ReadBuf};

pub use timerfd::ClockId;

mod delay;
/*
mod delay_queue;
*/
mod interval;

pub use delay::Delay;
pub use interval::Interval;
/*
pub use delay_queue::DelayQueue;
*/

pub struct TimerFd(AsyncFd<InnerTimerFd>);

impl TimerFd {
    pub fn new(clock: ClockId) -> std::io::Result<Self> {
        let fd = InnerTimerFd::new_custom(clock, true, true)?;
        let inner = AsyncFd::with_interest(fd, Interest::READABLE)?;
        Ok(TimerFd(inner))
    }

    fn set_state(&mut self, state: TimerState, flags: SetTimeFlags) {
        (self.0).get_mut().set_state(state, flags);
    }
}

fn read_u64(fd: RawFd) -> Result<u64> {
    let mut buf = [0u8; 8];
    let rv = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut _, 8) };
    match rv {
        len if len >= 0 => Ok(u64::from_ne_bytes(buf)),
        _err => Err(Error::last_os_error()),
    }
}

impl AsyncRead for TimerFd {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<Result<()>> {
        let inner = self.as_mut();
        let fd = inner.0.as_raw_fd();

        loop {
            let mut guard = ready!(inner.0.poll_read_ready(cx))?;
            match guard.try_io(|_| read_u64(fd)) {
                Ok(res) => {
                    let num = res?;
                    buf.put_slice(&num.to_ne_bytes());
                    break;
                }
                Err(_) => continue,
            }
        }
        Poll::Ready(Ok(()))
    }
}

/// Create a Future that completes in `duration` from now.
pub fn sleep(duration: Duration) -> Delay {
    Delay::new(Instant::now() + duration).expect("can't create delay")
}
