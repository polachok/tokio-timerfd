use mio::unix::EventedFd;
use mio::{Evented, Poll, PollOpt, Ready, Token};
use std::io::Result;
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use timerfd::{SetTimeFlags, TimerFd as InnerTimerFd, TimerState};
use tokio_reactor::{Reactor, Handle, Registration};
use tokio_timer::{Timer, clock::Clock, timer::TimerImpl, Resolution};
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

pub struct TimerFd<T> {
    timerfd: Inner,
    registration: Registration,
    park: T,
}

impl<T> TimerFd<T> where T: Park {
    pub fn new(clock: ClockId, park: T) -> Result<Self> {
        Self::new_with_handle(clock, park, &Handle::default())
    }

    pub fn new_with_handle(clock: ClockId, park: T, handle: &Handle) -> Result<Self> {
        let timerfd = Inner(InnerTimerFd::new_custom(clock, true, true)?);
        let registration = Registration::new();
        registration.register_with(&timerfd, handle)?;
        Ok(TimerFd { timerfd, registration, park })
    }
}

impl<T> Park for TimerFd<T> where T: Park {
    type Unpark = T::Unpark;
    type Error = T::Error;

    fn unpark(&self) -> Self::Unpark {
        self.park.unpark()
    }

    fn park(&mut self) -> std::result::Result<(), Self::Error> {
        self.park.park()
    }

    fn park_timeout(&mut self, duration: Duration) -> std::result::Result<(), Self::Error> {
        /* for timerfd 0 duration means disable */
        if duration == Duration::from_millis(0) {
            return self.park.park_timeout(duration);
        }
        if let Ok(Some(_ready)) = self.registration.take_read_ready() {
            self.timerfd.0.read();
        }
        self.timerfd.0.set_state(TimerState::Oneshot(duration), SetTimeFlags::Default);
        self.park.park()
    }
}

impl<T> TimerImpl<T> for TimerFd<T> where T: Park {
    fn resolution(&self) -> Resolution {
        Resolution {
            nanos_per_unit: 10_000,
            units_per_sec: 100_000,
        }
    }
}


pub fn new_timer(clock: Clock, reactor: Reactor) -> Result<Timer<Reactor, Clock, TimerFd<Reactor>>> {
    let handle = reactor.handle();
    let timerfd = TimerFd::new_with_handle(ClockId::Monotonic, reactor, &handle)?;
    Ok(Timer::new_with_now_and_impl(clock, timerfd))
}
