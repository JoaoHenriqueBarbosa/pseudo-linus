//! Bytes aleatórios do pseudo-processo. Num pseudo-linus de verdade viriam do `/dev/urandom` do
//! pseudo-kernel; aqui, se o VFS tiver `/dev/urandom`, ele é lido, senão um gerador por processo
//! (splitmix64) semeado na criação do processo. O programa nunca chama o `getrandom` do host.

use std::io::Read;
use std::sync::Mutex;

struct Rng(Mutex<u64>);

fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

pub fn fill_bytes(buf: &mut [u8]) {
    if let Ok(mut f) = crate::fs::File::open("/dev/urandom")
        && f.read_exact(buf).is_ok()
    {
        return;
    }
    let p = crate::proc::current();
    let seed = p
        .now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ (std::sync::Arc::as_ptr(&p) as usize as u64);
    let rng = crate::proc::proc_local(|| Rng(Mutex::new(seed)));
    let mut state = crate::proc::lock(&rng.0);
    for chunk in buf.chunks_mut(8) {
        let v = next(&mut state).to_le_bytes();
        chunk.copy_from_slice(&v[..chunk.len()]);
    }
}
