use futures::stream::poll_fn;
use futures::{try_ready, Async, Stream};
use mio::unix::EventedFd;
use mio::{Evented, Poll, PollOpt, Ready, Token};
use std::io::Result;
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use timerfd::{SetTimeFlags, TimerFd as InnerTimerFd, TimerState};
use tokio_reactor::PollEvented;

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
    pub fn new(clock: ClockId) -> std::io::Result<Self> {
        let inner = PollEvented::new(Inner(InnerTimerFd::new_custom(clock, true, true)?));
        Ok(TimerFd(inner))
    }

    fn set_state(&mut self, state: TimerState, flags: SetTimeFlags) {
        (self.0).get_mut().0.set_state(state, flags);
    }

    fn poll_read(&mut self) -> Result<Async<()>> {
        let ready = try_ready!(self.0.poll_read_ready(Ready::readable()));
        self.0.get_mut().0.read();
        self.0.clear_read_ready(ready)?;
        Ok(Async::Ready(()))
    }

    #[deprecated(note = "please use Interval")]
    pub fn periodic(mut self, dur: Duration) -> impl Stream<Item = (), Error = std::io::Error> {
        self.set_state(
            TimerState::Periodic {
                current: dur,
                interval: dur,
            },
            SetTimeFlags::Default,
        );
        poll_fn(move || {
            try_ready!(self.poll_read());
            Ok(Async::Ready(Some(())))
        })
    }
}

/// Create a Future that completes in `duration` from now.
pub fn sleep(duration: Duration) -> Delay {
    Delay::new(Instant::now() + duration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use tokio::prelude::*;

    #[test]
    fn periodic_works() {
        let timer = TimerFd::new(ClockId::Monotonic).unwrap();
        tokio::run(future::lazy(|| {
            let now = Instant::now();
            timer
                .periodic(Duration::from_micros(1))
                .take(2)
                .map_err(|err| println!("{:?}", err))
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
