//! Relógio do processo. `SystemTime::now()` leria o relógio do host; o pseudo-kernel decide o "agora"
//! (é assim que o faketime dos casos vira determinístico sem LD_PRELOAD).

use std::time::SystemTime;

pub fn now() -> SystemTime {
    crate::proc::current().now
}
