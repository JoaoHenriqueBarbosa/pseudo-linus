//! Hora local do sandbox nos registros do wget: o fuso vem de [`ul_common::time::zone::local`]
//! (`TZ` do ambiente ou fuso do sandbox, resolvido como o glibc).

use jiff::Timestamp;
use ul_common::time::zone::local;

/// O instante corrente formatado com `strftime` no fuso local.
pub fn now_local(fmt: &str) -> String {
    let (sec, nsec) = crate::net::io::wall();
    let ts = Timestamp::new(sec, nsec as i32).unwrap_or(Timestamp::UNIX_EPOCH);
    ts.to_zoned(local()).strftime(fmt).to_string()
}
