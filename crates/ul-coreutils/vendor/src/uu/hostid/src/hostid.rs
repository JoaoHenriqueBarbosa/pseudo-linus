// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (ToDO) gethostid

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
use clap::Command;
use core::ffi::c_long;
use sysio::io::{Write, stdout};
use uucore::{error::UResult, format_usage, translate};

// support targets missing libc::gethostid
fn gethostid() -> c_long {
    // /etc/hostid is still useful when
    // - Windows native hostid is called on MSYS shell
    // - wasi binary is called on unix host
    if let Ok(data) = sysio::fs::read("/etc/hostid")
        && let Some(bytes) = data.get(..4).and_then(|s| <[u8; 4]>::try_from(s).ok())
    {
        return u32::from_ne_bytes(bytes) as c_long;
    }
    // Porte pseudo-linus: o `gethostbyname` da glibc com o nsswitch do Debian (`files dns`) olha
    // primeiro o /etc/hosts; aqui só ele, o do pseudo-FS (sem DNS do host).
    #[cfg(unix)]
    {
        let hostname = sysio::unistd::gethostname();
        if let Ok(hosts) = sysio::fs::read("/etc/hosts") {
            for line in hosts.split(|b| *b == b'\n') {
                let line = line.split(|b| *b == b'#').next().unwrap_or(&[]);
                let mut fields = line.split(u8::is_ascii_whitespace).filter(|f| !f.is_empty());
                let Some(addr) = fields.next() else { continue };
                if !fields.any(|name| name == hostname.as_slice()) {
                    continue;
                }
                if let Ok(ipv4) = String::from_utf8_lossy(addr).parse::<std::net::Ipv4Addr>() {
                    return u32::from_ne_bytes(ipv4.octets()).rotate_left(16) as c_long;
                }
            }
        }
    }
    // todo: remove to_string_lossy and use this for unix
    // todo: use std::net::hostname when is was stabilized
    #[cfg(windows)]
    {
        use std::net::{IpAddr, ToSocketAddrs as _};
        if let Ok(mut hostname) = hostname::get() {
            hostname.push(":0");
            if let Ok(mut addrs) = hostname.to_string_lossy().to_socket_addrs()
                && let Some(addr) = addrs.find(|a| a.ip().is_ipv4())
                && let IpAddr::V4(ipv4) = addr.ip()
            {
                return u32::from_ne_bytes(ipv4.octets()).rotate_left(16) as c_long;
            }
        }
    }
    0
}

#[uucore::main(no_signals)]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    uucore::clap_localization::handle_clap_result(uu_app(), args)?;
    // The identifier `gethostid` reports is 32 bits wide, but it arrives in a
    // `c_long`, and nothing promises how the unused upper bits are filled. Mask
    // them off so a value with the high bit set prints as its own eight digits
    // rather than as a sign-extended one; where `c_long` is itself 32 bits the
    // mask changes nothing.

    let mut result = gethostid();

    #[allow(overflowing_literals)]
    let mask = 0xffff_ffff;

    result &= mask;
    writeln!(stdout(), "{result:0>8x}")?;
    Ok(())
}

pub fn uu_app() -> Command {
    Command::new("hostid")
        .version(uucore::crate_version!())
        .help_template(uucore::localized_help_template("hostid"))
        .about(translate!("hostid-about"))
        .override_usage(format_usage(&translate!("hostid-usage")))
        .infer_long_args(true)
}
