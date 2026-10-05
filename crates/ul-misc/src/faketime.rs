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

fn usage(_name: &str) -> String {
    r#"
Usage: faketime [switches] <timestamp> <program with arguments>

This will run the specified 'program' with the given 'arguments'.
The program will be tricked into seeing the given 'timestamp' as its starting date and time.
The clock will continue to run from this timestamp. Please see the manpage (man faketime)
for advanced options, such as stopping the wall clock and make it run faster or slower.

The optional switches are:
  -m                  : Use the multi-threaded version of libfaketime
  -f                  : Use the advanced timestamp specification format (see manpage)
  --exclude-monotonic : Prevent monotonic clock from drifting (not the raw monotonic one)
  -p PID              : Pretend that the program's process ID is PID
  --disable-shm       : Disable use of shared memory by libfaketime.
  --date-prog PROG    : Use specified GNU-compatible implementation of 'date' program

Examples:
faketime 'last friday 5 pm' /bin/date
faketime '2008-12-24 08:15:42' /bin/date
faketime -f '+2,5y x10,0' /bin/bash -c 'date; while true; do echo $SECONDS ; sleep 1 ; done'
faketime -f '+2,5y x0,50' /bin/bash -c 'date; while true; do echo $SECONDS ; sleep 1 ; done'
faketime -f '+2,5y i2,0' /bin/bash -c 'date; while true; do date; sleep 1 ; done'
In this single case all spawned processes will use the same global clock
without restarting it at the start of each process.

(Please note that it depends on your locale settings whether . or , has to be used for fractions)

"#.to_string()
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
                let _ = out.write_all(format!("\nfaketime: Version {VERSION}\nFor usage information please use 'faketime --help'.\n").as_bytes());
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
                .arg("+%s")
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
                let _ = io::stdout().write_all(
                    b"Error: Timestamp to fake not recognized, please re-try with a different timestamp.\n",
                );
                return 1;
            }
            // O wrapper original grava a diferença, em segundos, entre o instante pedido e o atual.
            let target: i64 = stamp.trim().parse().unwrap_or(0);
            let now = sys::current().clock_gettime(sysabi::Clock::Realtime).map_or(0, |t| t.sec);
            setenv("FAKETIME", format!("{:+}", target - now).as_bytes());
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
    io::eprint(format!("faketime: Running specified command failed: {}\n", e.message()));
    1
}
