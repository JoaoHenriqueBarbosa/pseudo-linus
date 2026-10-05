//! `faketime` do libfaketime 0.9.10 (pacote faketime do Debian 13): o wrapper.
//!
//! No sandbox não há `LD_PRELOAD`, então o wrapper faz o que dá: interpreta as opções, converte a
//! especificação de tempo com o `date -d` (como o original), define `FAKETIME` (e as variáveis das
//! opções) no ambiente e troca o processo pelo comando. Mensagens de uso e erros são as do wrapper.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};
use sysio::process::{Command, Stdio};

use crate::util::io;

const VERSION: &str = "0.9.10";

fn usage(name: &str) -> String {
    format!(
        "
Usage: {name} [switches] <timestamp|-> <program with arguments>

This will run the specified 'program' with the given 'timestamp'.
The program will be tricked into seeing the given 'timestamp' as its starting date and time.
The clock continues to run from this timestamp. (Please see the manpage for more advanced options.)
The timestamp must be parsable by the 'date' command. Use '-' to disable fake time.

Options:
  -m                        : Use the multi-threaded version of libfaketime
  -f                        : Use the advanced timestamp specification format (see manpage)
  --exclude-monotonic       : Don't fake the monotonic clock (Only applicable with -f)
  --no-cache                : Disable all caching of fake time
  --date-prog PROG          : Use specified GNU-compatible implementation of 'date' program

Examples:
{name} 'last Friday 5 pm' /bin/date
{name} '2008-12-24 08:15:42' /bin/date
{name} -f '+2,5y x10,0' /bin/bash -c 'date; while true; do echo $SECONDS ; sleep 1 ; done'
{name} -f '+2,5y x0,50' /bin/bash -c 'date; while true; do echo $SECONDS ; sleep 1 ; done'
{name} -f '+2,5y i2,0' /bin/bash -c 'date; while true; do echo $SECONDS ; sleep 1 ; done'
In this single case all spawned processes will use the same global clock without restarting it at the start of each process.

(Please note that it depends on your locale settings whether . or , has to be used for fractions of seconds)

"
    )
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn setenv(name: &str, value: &[u8]) {
    let _ = sys::current().setenv(name.as_bytes(), value);
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let name = io::lossy(argv.first().map(Vec::as_slice).unwrap_or(b"faketime"));

    let mut advanced = false;
    let mut exclude_monotonic = false;
    let mut no_cache = false;
    let mut date_prog: Vec<u8> = b"date".to_vec();
    let mut cur = 1;
    while cur < argv.len() {
        match argv[cur].as_slice() {
            b"-m" => {}
            b"-f" => advanced = true,
            b"--exclude-monotonic" => exclude_monotonic = true,
            b"--no-cache" => no_cache = true,
            b"--date-prog" => {
                cur += 1;
                match argv.get(cur) {
                    Some(p) => date_prog = p.clone(),
                    None => break,
                }
            }
            b"-h" | b"--help" => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&name).as_bytes());
                return 0;
            }
            b"-v" | b"--version" => {
                let mut out = io::stdout();
                let _ = out.write_all(format!("faketime: Version {VERSION}\n").as_bytes());
                return 0;
            }
            _ => break,
        }
        cur += 1;
    }

    if argv.len().saturating_sub(cur) < 2 {
        let mut out = io::stdout();
        let _ = out.write_all(usage(&name).as_bytes());
        return 1;
    }

    let spec = argv[cur].clone();
    if spec != b"-" {
        if advanced {
            setenv("FAKETIME", &spec);
        } else {
            // O original chama `date -d <spec> +%Y-%m-%d %T` e usa a saída como instante absoluto.
            let date = String::from_utf8_lossy(&date_prog).into_owned();
            let spec_s = String::from_utf8_lossy(&spec).into_owned();
            let _ = io::flush_stdout();
            let res = Command::new(&date)
                .arg("-d")
                .arg(&spec_s)
                .arg("+%Y-%m-%d %T")
                .stderr(Stdio::inherit())
                .output();
            let stamp = match res {
                Ok(o) if o.status.success() => {
                    let text = String::from_utf8_lossy(&o.stdout).into_owned();
                    text.lines().next().unwrap_or("").to_string()
                }
                _ => String::new(),
            };
            if stamp.is_empty() {
                io::eprint(String::from(
                    "Error: Timestamp to fake not recognized, please re-try with a different timestamp.\n",
                ));
                return 1;
            }
            let mut v = b"@".to_vec();
            v.extend_from_slice(stamp.as_bytes());
            setenv("FAKETIME", &v);
        }
    }
    if no_cache {
        setenv("FAKETIME_NO_CACHE", b"1");
    }
    if exclude_monotonic {
        setenv("DONT_FAKE_MONOTONIC", b"1");
    }

    let cmd = &argv[cur + 1..];
    let _ = io::flush_stdout();
    let e = crate::setsid::execvp(&cmd[0], cmd);
    io::eprint(format!("Running specified command failed: {}\n", e.message()));
    1
}
