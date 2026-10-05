// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore NPROCESSORS SCHED ONLN getaffinity getcpu getscheduler sched sysconf

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
use clap::{Arg, ArgAction, Command};
use sysio::env;
use sysio::io::{Write, stdout};
use uucore::{
    error::{UResult, USimpleError, strip_errno},
    format_usage, translate,
};

static OPT_ALL: &str = "all";
static OPT_IGNORE: &str = "ignore";

#[uucore::main(no_signals)]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let matches = uucore::clap_localization::handle_clap_result(uu_app(), args)?;
    // Porte pseudo-linus: o N é validado aqui, com a mensagem do GNU (`invalid number: ‘abc’`);
    // um valor que não cabe satura, como o `xstrtoul` do GNU faz.
    #[allow(clippy::unwrap_used, reason = "clap provides 0 by default")]
    let ignore_text = matches.get_one::<String>(OPT_IGNORE).unwrap();
    let ignore = match ignore_text.trim_start().parse::<usize>() {
        Ok(n) => n,
        Err(e) if *e.kind() == std::num::IntErrorKind::PosOverflow => usize::MAX,
        Err(_) => {
            return Err(USimpleError::new(
                1,
                translate!(
                    "nproc-error-invalid-number",
                    "value" => uucore::display::locale_quote(ignore_text)
                ),
            ));
        }
    };
    // Uses the OpenMP variable to limit the number of threads
    // Non OMP_THREAD_LIMIT>0 cases are rejected
    let limit = env::var("OMP_THREAD_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(usize::MAX);

    let mut cores = if matches.get_flag(OPT_ALL) {
        num_cpus_all()
    } else {
        // OMP_NUM_THREADS doesn't have an impact on --all
        // Uses the OpenMP variable to force the number of threads
        // If the parsing fails, returns the number of CPU
        // Non OMP_NUM_THREADS>0 cases are rejected
        omp_num_threads().unwrap_or_else(available_parallelism)
    };

    cores = cores.saturating_sub(ignore).clamp(1, limit);
    //discard error about stdout flush
    stdout()
        .write_all(format!("{cores}\n").as_bytes())
        .map_err(|e| USimpleError::new(1, strip_errno(&e)))
}

fn omp_num_threads() -> Option<usize> {
    let threads = env::var("OMP_NUM_THREADS").ok()?;
    let s = threads.split_terminator(',').next()?;
    // In some cases, OMP_NUM_THREADS can be "x,y,z"
    // In this case, only take the first one (like GNU)
    match s.trim().parse::<usize>() {
        Ok(n @ 1..) => Some(n),
        Err(e) if *e.kind() == std::num::IntErrorKind::PosOverflow => Some(usize::MAX),
        _ => None,
    }
}

pub fn uu_app() -> Command {
    Command::new("nproc")
        .version(uucore::crate_version!())
        .help_template(uucore::localized_help_template("nproc"))
        .about(translate!("nproc-about"))
        .override_usage(format_usage(&translate!("nproc-usage")))
        .infer_long_args(true)
        .arg(
            Arg::new(OPT_ALL)
                .long(OPT_ALL)
                .help(translate!("nproc-help-all"))
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(OPT_IGNORE)
                .long(OPT_IGNORE)
                .value_name("N")
                .default_value("0")
                .value_parser(clap::value_parser!(String))
                .help(translate!("nproc-help-ignore")),
        )
}

fn num_cpus_all() -> usize {
    // sysconf returns (hardcoded?) 2 if /proc and /sys are masked, and sched_getaffinity syscall was blocked by strace.
    // So fallback to available_parallelism at here is not useful
    // Porte pseudo-linus: as CPUs configuradas do sandbox são as vCPUs que o pseudo-kernel dá a
    // ele (não há /sys/devices/system/cpu do host pra glibc contar).
    #[cfg(unix)]
    return sysio::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    // not sure what we can do for non-unix...
    #[cfg(not(unix))]
    available_parallelism()
}

// We cannot use sysio::thread::available_parallelism to mimic GNU's rounding...
#[cfg(any(target_os = "linux", target_os = "android"))]
fn cgroups2_quota() -> Option<usize> {
    use sysio::fs::read_to_string;
    let cgroups = read_to_string("/proc/self/cgroup").ok()?;
    let path = cgroups.lines().next()?.split(':').nth(2)?;
    let pair = read_to_string(format!("/sys/fs/cgroup{path}/cpu.max")).ok()?;
    let mut pair = pair.split_whitespace();
    // map the string "max" to None as we unwrap_or(usize::MAX) later
    let quota = pair.next()?.parse::<usize>().ok()?;
    // kernel does not provide 0 period. But it seems GNU cares about it
    let period = pair.next()?.parse::<usize>().ok().filter(|&p| p > 0)?;
    // mimic GNU's rounding
    Some(quota.saturating_add(period / 2) / period)
}

fn available_parallelism() -> usize {
    // return all online cores if sched_getaffinity syscall failed as same as GNU
    // Porte pseudo-linus: sched_getaffinity(2) do pseudo-kernel (vCPUs do sandbox). Todo processo
    // é SCHED_OTHER (o EEVDF do pseudo-kernel), então a cota do cgroup sempre vale.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let affinity = sysio::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    return affinity.min(cgroups2_quota().unwrap_or(usize::MAX));
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    sysio::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}
