//! Parte de `wtf/MonotonicTime.h` que o `Parser` usa: `now()` e a diferença entre dois instantes.
//!
//! O C++ lê `CLOCK_MONOTONIC`; Rust seguro só expõe `Instant`, cuja origem é opaca. Como o relógio
//! monotônico só é observável por diferenças, ancora-se um `Instant` no primeiro uso do processo e o
//! valor é o tempo decorrido desde então, em segundos.

use std::ops::Sub;
use std::sync::OnceLock;
use std::time::Instant;

use crate::wtf::seconds::Seconds;

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// `class MonotonicTime`.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct MonotonicTime {
    value: f64,
}

impl MonotonicTime {
    pub fn now() -> MonotonicTime {
        MonotonicTime { value: epoch().elapsed().as_secs_f64() }
    }

    pub fn seconds_since_epoch(self) -> Seconds {
        Seconds::new(self.value)
    }

    /// `explicit operator bool`.
    pub fn is_set(self) -> bool {
        self.value != 0.0
    }
}

impl Sub for MonotonicTime {
    type Output = Seconds;

    fn sub(self, other: MonotonicTime) -> Seconds {
        Seconds::new(self.value - other.value)
    }
}
