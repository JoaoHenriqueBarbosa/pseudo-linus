//! Bytes aleatórios do `getrandom(2)` do pseudo-kernel (nunca do host).

use crate::proc;

/// Enche `buf` de bytes aleatórios.
pub fn fill_bytes(buf: &mut [u8]) {
    let sys = proc::sys();
    let mut done = 0;
    while done < buf.len() {
        match sys.getrandom(&mut buf[done..]) {
            Ok(0) => break,
            Ok(n) => done += n,
            Err(sysabi::Errno::EINTR) => {}
            Err(_) => break,
        }
    }
}

/// Um `u64` aleatório.
pub fn next_u64() -> u64 {
    let mut b = [0u8; 8];
    fill_bytes(&mut b);
    u64::from_le_bytes(b)
}
