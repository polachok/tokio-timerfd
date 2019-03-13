use futures::stream::poll_fn;
use futures::{try_ready, Async, Stream};
use mio::unix::EventedFd;
use mio::{Evented, Poll, PollOpt, Ready, Token};
use std::io::Result;
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use timerfd::{SetTimeFlags, TimerFd as InnerTimerFd, TimerState};
use tokio_reactor::{PollEvented, Handle, Registration};
use tokio_timer::{timer::TimerImpl, Resolution};
use tokio_executor::park::Park;

pub use timerfd::ClockId;

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
    pub fn new_with_handle(clock: ClockId, handle: &Handle) -> std::io::Result<Self> {
        let inner = PollEvented::new_with_handle(Inner(InnerTimerFd::new_custom(clock, true, true)?), handle)?;
        Ok(TimerFd(inner))
    }

    pub fn new(clock: ClockId) -> std::io::Result<Self> {
        Self::new_with_handle(clock, &Handle::default())
    }

    pub fn periodic(mut self, dur: Duration) -> impl Stream<Item = (), Error = std::io::Error> {
        (self.0).get_mut().0.set_state(
            TimerState::Periodic {
                current: dur,
                interval: dur,
            },
            SetTimeFlags::Default,
        );
        poll_fn(move || {
            let ready = try_ready!(self.0.poll_read_ready(Ready::readable()));
            self.0.get_mut().0.read();
            self.0.clear_read_ready(ready)?;
            Ok(Async::Ready(Some(())))
        })
    }
}

pub struct TimerFdImpl<T> {
    timerfd: Inner,
    registration: Registration,
    park: T,
}

impl<T> TimerFdImpl<T> where T: Park {
    pub fn new(clock: ClockId, park: T) -> Result<Self> {
        Self::new_with_handle(clock, park, &Handle::default())
    }

    pub fn new_with_handle(clock: ClockId, park: T, handle: &Handle) -> Result<Self> {
        let timerfd = Inner(InnerTimerFd::new_custom(clock, true, true)?);
        let registration = Registration::new();
        registration.register_with(&timerfd, handle)?;
        Ok(TimerFdImpl { timerfd, registration, park })
    }
}

impl<T> Park for TimerFdImpl<T> where T: Park {
    type Unpark = T::Unpark;
    type Error = T::Error;

    fn unpark(&self) -> Self::Unpark {
        self.park.unpark()
    }

    fn park(&mut self) -> std::result::Result<(), Self::Error> {
        self.park.park()
    }

    fn park_timeout(&mut self, duration: Duration) -> std::result::Result<(), Self::Error> {
        if duration == Duration::from_millis(0) {
            return self.park.park_timeout(duration);
        }
        if let Ok(Some(_ready)) = self.registration.take_read_ready() {
            self.timerfd.0.read();
        }
        //println!("parking for {:?}", duration);
        self.timerfd.0.set_state(TimerState::Oneshot(duration), SetTimeFlags::Default);
        //self.timerfd.get_mut().0.read();
        //self.timerfd.clear_read_ready(Ready::readable()).unwrap();
        //let _ = self.timerfd.poll_read_ready(Ready::readable()).unwrap();
        self.park.park() //_timeout(Duration::from_millis(1))
    }
}

impl<T> TimerImpl<T> for TimerFdImpl<T> where T: Park {
    fn resolution(&self) -> Resolution {
        Resolution {
            nanos_per_unit: 1_000,
            units_per_sec: 1_000_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;
    use super::*;

    #[test]
    fn periodic_works() {
        let timer = TimerFd::new(ClockId::Monotonic).unwrap();
        let now = Instant::now();
        let fut = timer
            .periodic(Duration::from_micros(10))
            .take(10)
            .map_err(|err| println!("{:?}", err))
            .for_each(|_| {
                //println!("hello");
                Ok(())
            });
        tokio::run(fut);
        println!("{:?}", now.elapsed());
    }
}
