tokio-timerfd
=============

Linux [timerfd](http://man7.org/linux/man-pages/man2/timerfd_create.2.html) for Tokio.

This crates provides [tokio-timer](https://docs.rs/tokio-timer)-like API
on top of timerfd. `timerfd` is a Linux-specific API providing timer notifications as
file descriptor read events.

The advantage of `timerfd` is that it has more granularity than epoll_wait(),
which only provides 1 millisecond timeouts. `timerfd` API allows for nanosecond
precision, but precise timing of the wakeup is not guaranteed on a normal
multitasking system.
