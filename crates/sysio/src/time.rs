//! Relógio do pseudo-processo. `SystemTime::now()` e `Instant::now()` leriam o relógio do host; aqui
//! o "agora" vem do `clock_gettime` do kernel do pseudo-linus (é assim que um caso com relógio fixo
//! fica determinístico sem LD_PRELOAD).

use std::ops::{Add, AddAssign, Sub, SubAssign};
use std::time::Duration;

pub use std::time::{SystemTime, SystemTimeError, TryFromFloatSecsError, UNIX_EPOCH};

use sysabi::{Clock, TimeSpec};

use crate::proc;

/// `clock_gettime(CLOCK_REALTIME)`.
pub fn now() -> SystemTime {
    match proc::sys().clock_gettime(Clock::Realtime) {
        Ok(t) => crate::fs::timespec_to_system(t),
        Err(_) => UNIX_EPOCH,
    }
}

/// `clock_gettime` de qualquer relógio, como `TimeSpec`.
pub fn clock(clock: Clock) -> TimeSpec {
    proc::sys().clock_gettime(clock).unwrap_or_default()
}

/// `std::time::Instant` sobre `CLOCK_MONOTONIC` do pseudo-kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(Duration);

impl Instant {
    pub fn now() -> Instant {
        let t = clock(Clock::Monotonic);
        Instant(Duration::new(t.sec.max(0) as u64, t.nsec))
    }
    pub fn duration_since(&self, earlier: Instant) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
    pub fn checked_duration_since(&self, earlier: Instant) -> Option<Duration> {
        self.0.checked_sub(earlier.0)
    }
    pub fn saturating_duration_since(&self, earlier: Instant) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
    pub fn elapsed(&self) -> Duration {
        Instant::now().duration_since(*self)
    }
    pub fn checked_add(&self, d: Duration) -> Option<Instant> {
        self.0.checked_add(d).map(Instant)
    }
    pub fn checked_sub(&self, d: Duration) -> Option<Instant> {
        self.0.checked_sub(d).map(Instant)
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;
    fn add(self, d: Duration) -> Instant {
        Instant(self.0 + d)
    }
}

impl AddAssign<Duration> for Instant {
    fn add_assign(&mut self, d: Duration) {
        self.0 += d;
    }
}

impl Sub<Duration> for Instant {
    type Output = Instant;
    fn sub(self, d: Duration) -> Instant {
        Instant(self.0.saturating_sub(d))
    }
}

impl SubAssign<Duration> for Instant {
    fn sub_assign(&mut self, d: Duration) {
        self.0 = self.0.saturating_sub(d);
    }
}

impl Sub<Instant> for Instant {
    type Output = Duration;
    fn sub(self, other: Instant) -> Duration {
        self.duration_since(other)
    }
}

/// `nanosleep(2)` até completar `d` (EINTR de sinal capturado dorme o resto, como o
/// `std::thread::sleep`).
pub fn sleep(d: Duration) {
    let _ = sleep_interruptible(d, false);
}

/// Dorme `d`; com `stop_on_signal`, volta `Err(EINTR)` quando um sinal capturado interrompe.
pub fn sleep_interruptible(d: Duration, stop_on_signal: bool) -> std::io::Result<()> {
    let sys = proc::sys();
    let deadline = Instant::now() + d;
    let mut left = d;
    loop {
        match sys.nanosleep(left) {
            Ok(()) => return Ok(()),
            Err(sysabi::Errno::EINTR) if stop_on_signal => return Err(crate::errno::err(crate::errno::EINTR)),
            Err(sysabi::Errno::EINTR) => {
                let now = Instant::now();
                if now >= deadline {
                    return Ok(());
                }
                left = deadline - now;
            }
            Err(e) => return Err(crate::errno::from_errno(e)),
        }
    }
}
