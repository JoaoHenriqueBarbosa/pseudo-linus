// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore hashset Addrs addrs

#![cfg(any(all(unix, not(any(target_os = "aix", target_os = "redox"))), windows))]

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
use sysio::io::{Write, stdout};

use std::str;
use std::{collections::hash_set::HashSet, ffi::OsString};

use clap::builder::ValueParser;
use clap::{Arg, ArgAction, ArgMatches, Command};

use uucore::translate;

use uucore::{
    error::{FromIo, UResult},
    format_usage,
};

static OPT_DOMAIN: &str = "domain";
static OPT_IP_ADDRESS: &str = "ip-address";
static OPT_FQDN: &str = "fqdn";
static OPT_SHORT: &str = "short";
static OPT_HOST: &str = "host";

// Porte pseudo-linus: o crate `hostname` (gethostname/sethostname do host) vira o uts do
// pseudo-kernel, e a resolução do `-i` usa só o /etc/hosts do pseudo-FS (sem DNS do host).
mod hostname {
    use std::ffi::{OsStr, OsString};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    pub fn get() -> OsString {
        OsString::from_vec(sysio::unistd::gethostname())
    }

    pub fn set(name: &OsStr) -> sysio::io::Result<()> {
        sysio::unistd::sethostname(name.as_bytes())
    }

    /// Endereços do nome no /etc/hosts, na ordem do arquivo.
    pub fn lookup(name: &str) -> sysio::io::Result<Vec<std::net::IpAddr>> {
        let hosts = sysio::fs::read("/etc/hosts").unwrap_or_default();
        let mut out = Vec::new();
        for line in String::from_utf8_lossy(&hosts).lines() {
            let line = line.split('#').next().unwrap_or("");
            let mut fields = line.split_whitespace();
            let Some(addr) = fields.next() else { continue };
            if fields.any(|n| n == name)
                && let Ok(ip) = addr.parse()
            {
                out.push(ip);
            }
        }
        if out.is_empty() {
            return Err(sysio::io::Error::other("Name or service not known"));
        }
        Ok(out)
    }
}

#[uucore::main(no_signals)]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let matches = uucore::clap_localization::handle_clap_result(uu_app(), args)?;

    match matches.get_one::<OsString>(OPT_HOST) {
        None => display_hostname(&matches),
        Some(host) => {
            hostname::set(host).map_err_context(|| translate!("hostname-error-set-hostname"))
        }
    }
}

pub fn uu_app() -> Command {
    Command::new("hostname")
        .version(uucore::crate_version!())
        .help_template(uucore::localized_help_template(uucore::util_name()))
        .about(translate!("hostname-about"))
        .override_usage(format_usage(&translate!("hostname-usage")))
        .infer_long_args(true)
        .arg(
            Arg::new(OPT_DOMAIN)
                .short('d')
                .long("domain")
                .overrides_with_all([OPT_DOMAIN, OPT_IP_ADDRESS, OPT_FQDN, OPT_SHORT])
                .help(translate!("hostname-help-domain"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(OPT_IP_ADDRESS)
                .short('i')
                .long("ip-address")
                .overrides_with_all([OPT_DOMAIN, OPT_IP_ADDRESS, OPT_FQDN, OPT_SHORT])
                .help(translate!("hostname-help-ip-address"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(OPT_FQDN)
                .short('f')
                .long("fqdn")
                .overrides_with_all([OPT_DOMAIN, OPT_IP_ADDRESS, OPT_FQDN, OPT_SHORT])
                .help(translate!("hostname-help-fqdn"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(OPT_SHORT)
                .short('s')
                .long("short")
                .overrides_with_all([OPT_DOMAIN, OPT_IP_ADDRESS, OPT_FQDN, OPT_SHORT])
                .help(translate!("hostname-help-short"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(OPT_HOST)
                .value_parser(ValueParser::os_string())
                .value_hint(clap::ValueHint::Hostname),
        )
}

fn display_hostname(matches: &ArgMatches) -> UResult<()> {
    // Porte pseudo-linus: o gethostname do pseudo-kernel não falha.
    let hostname = hostname::get().to_string_lossy().into_owned();

    if matches.get_flag(OPT_IP_ADDRESS) {
        let addresses = hostname::lookup(&hostname)
            .map_err_context(|| "failed to resolve socket addresses".to_owned())?
            .into_iter()
            .map(|ip| std::net::SocketAddr::new(ip, 1));

        let mut hashset = HashSet::new();
        let mut output = String::new();
        for addr in addresses {
            // XXX: not sure why this is necessary...
            if !hashset.contains(&addr) {
                let mut ip = addr.to_string();
                if ip.ends_with(":1") {
                    let len = ip.len();
                    ip.truncate(len - 2);
                }
                output.push_str(&ip);
                output.push(' ');
                hashset.insert(addr);
            }
        }
        let len = output.len();
        if len > 0 {
            writeln!(stdout(), "{}", &output[0..len - 1])?;
        }

        Ok(())
    } else {
        if matches.get_flag(OPT_SHORT) || matches.get_flag(OPT_DOMAIN) {
            let mut it = hostname.char_indices().filter(|&ci| ci.1 == '.');
            if let Some(ci) = it.next() {
                if matches.get_flag(OPT_SHORT) {
                    writeln!(stdout(), "{}", &hostname[0..ci.0])?;
                } else {
                    writeln!(stdout(), "{}", &hostname[ci.0 + 1..])?;
                }
            } else if matches.get_flag(OPT_SHORT) {
                writeln!(stdout(), "{hostname}")?;
            }
            return Ok(());
        }

        writeln!(stdout(), "{hostname}")?;

        Ok(())
    }
}
