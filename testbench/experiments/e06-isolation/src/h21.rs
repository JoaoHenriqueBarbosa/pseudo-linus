//! H21: Landlock e seccomp por thread isolam o pseudo-processo sem afetar o resto do host?
//!
//! Lado do processo principal: dispara os modos de [`crate::child`] num subprocesso e decide o
//! veredito. Nenhuma restrição é aplicada neste processo.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use harness::Verdict;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::child::{BenchConfig, BenchReport, LandlockReport, SeccompReport, SpawnReport, Stat};
use crate::layout;
use crate::probe::{Probe, Role, mismatches};

/// Roda `<exe> child <args>` e lê o JSON do stdout.
pub fn run_child<T: DeserializeOwned>(exe: &Path, args: &[&str]) -> Result<T> {
    let out = Command::new(exe)
        .arg("child")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("subprocesso {} child {}", exe.display(), args.join(" ")))?;
    if !out.status.success() {
        bail!("subprocesso child {} falhou ({}): {}", args.join(" "), out.status, String::from_utf8_lossy(&out.stderr));
    }
    serde_json::from_slice(&out.stdout).with_context(|| format!("JSON do child {}", args.join(" ")))
}

/// Uma chamada clone/clone3 vista pelo strace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TracedCall {
    pub syscall: String,
    pub clone_thread: bool,
    pub result: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpawnTrace {
    pub available: bool,
    pub error: Option<String>,
    pub report: Option<SpawnReport>,
    /// Chamadas por fase (as fases são marcadas por `statx` em caminhos inexistentes).
    pub phases: BTreeMap<String, Vec<TracedCall>>,
    /// Na fase restrita: clone3 voltou ENOSYS, o fork caiu no clone e levou EPERM, a thread nasceu por clone.
    pub restricted_clone3_enosys: bool,
    pub restricted_fork_clone_eperm: bool,
    pub restricted_thread_clone_ok: bool,
    /// Depois da fase restrita, a thread principal (sem filtro) ainda cria thread por clone3. A glibc
    /// guarda o ENOSYS num estático do processo e para de tentar clone3 pra threads.
    pub after_thread_via_clone3: bool,
    /// Depois da fase restrita, a thread principal ainda cria processo (posix_spawn) por clone3.
    pub after_process_via_clone3: bool,
}

/// Junta as linhas `<unfinished ...>` / `<... resumed>` do `strace -f` por pid.
fn merge_strace_lines(text: &str) -> Vec<String> {
    let mut pending: BTreeMap<String, String> = BTreeMap::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((pid, rest)) = line.split_once(' ') else { continue };
        let rest = rest.trim_start();
        if let Some(head) = rest.strip_suffix("<unfinished ...>") {
            pending.insert(pid.to_string(), head.trim_end().to_string());
        } else if rest.starts_with("<... ") {
            let tail = rest.split_once("resumed>").map(|(_, t)| t).unwrap_or(rest);
            let head = pending.remove(pid).unwrap_or_default();
            out.push(format!("{head}{tail}"));
        } else {
            out.push(rest.to_string());
        }
    }
    out
}

pub fn parse_strace(text: &str) -> BTreeMap<String, Vec<TracedCall>> {
    let mut phases: BTreeMap<String, Vec<TracedCall>> = BTreeMap::new();
    let mut phase = "start".to_string();
    for line in merge_strace_lines(text) {
        if let Some(idx) = line.find("/nonexistent-e06-marker/") {
            let name = &line[idx + "/nonexistent-e06-marker/".len()..];
            phase = name.split('"').next().unwrap_or(name).to_string();
            continue;
        }
        let syscall = if line.starts_with("clone3(") {
            "clone3"
        } else if line.starts_with("clone(") {
            "clone"
        } else {
            continue;
        };
        let result = line.rsplit_once(" = ").map(|(_, r)| r.trim().to_string()).unwrap_or_default();
        phases.entry(phase.clone()).or_default().push(TracedCall {
            syscall: syscall.to_string(),
            clone_thread: line.contains("CLONE_THREAD"),
            result,
        });
    }
    phases
}

fn spawn_trace(exe: &Path) -> SpawnTrace {
    let mut trace = SpawnTrace {
        available: false,
        error: None,
        report: None,
        phases: BTreeMap::new(),
        restricted_clone3_enosys: false,
        restricted_fork_clone_eperm: false,
        restricted_thread_clone_ok: false,
        after_thread_via_clone3: false,
        after_process_via_clone3: false,
    };
    let log = layout::scratch().join("strace-seccomp-spawn.txt");
    let out = Command::new("strace")
        .args(["-f", "-qq", "-e", "trace=clone,clone3,statx", "-o"])
        .arg(&log)
        .arg(exe)
        .args(["child", "seccomp-spawn"])
        .stdin(Stdio::null())
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => {
            trace.error = Some(format!("strace indisponível: {e}"));
            return trace;
        }
    };
    if !out.status.success() {
        trace.error = Some(format!("strace saiu com {}: {}", out.status, String::from_utf8_lossy(&out.stderr)));
        return trace;
    }
    trace.report = serde_json::from_slice(&out.stdout).ok();
    let text = match std::fs::read_to_string(&log) {
        Ok(t) => t,
        Err(e) => {
            trace.error = Some(format!("ler {}: {e}", log.display()));
            return trace;
        }
    };
    trace.available = true;
    trace.phases = parse_strace(&text);
    if let Some(calls) = trace.phases.get("restricted") {
        trace.restricted_clone3_enosys = calls.iter().any(|c| c.syscall == "clone3" && c.result.contains("ENOSYS"));
        trace.restricted_fork_clone_eperm =
            calls.iter().any(|c| c.syscall == "clone" && !c.clone_thread && c.result.contains("EPERM"));
        trace.restricted_thread_clone_ok =
            calls.iter().any(|c| c.syscall == "clone" && c.clone_thread && c.result.parse::<i64>().is_ok_and(|p| p > 0));
    }
    if let Some(calls) = trace.phases.get("main_after") {
        trace.after_thread_via_clone3 = calls.iter().any(|c| c.syscall == "clone3" && c.clone_thread);
        trace.after_process_via_clone3 = calls.iter().any(|c| c.syscall == "clone3" && !c.clone_thread);
    }
    trace
}

/// Diferença de uma métrica em relação a `none`: pelo mínimo (melhor rodada, o estimador menos
/// sensível a carga de outros processos na máquina) e pela mediana.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Delta {
    pub min: f64,
    pub median: f64,
    /// Diferença pelo mínimo em porcentagem do mínimo de `none`.
    pub min_pct: f64,
}

impl Delta {
    fn of(c: &Stat, base: &Stat) -> Delta {
        Delta { min: c.min - base.min, median: c.median - base.median, min_pct: 100.0 * (c.min - base.min) / base.min }
    }
}

/// Overhead de uma configuração em relação a `none`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Overhead {
    pub config: BenchConfig,
    pub getppid_ns: Delta,
    pub pread_1b_ns: Delta,
    pub open_close_ns: Delta,
    pub spawn_us: Delta,
}

pub fn overheads(bench: &BenchReport) -> Vec<Overhead> {
    let Some(base) = bench.configs.iter().find(|c| c.config == BenchConfig::None) else { return Vec::new() };
    bench
        .configs
        .iter()
        .filter(|c| c.config != BenchConfig::None)
        .map(|c| Overhead {
            config: c.config,
            getppid_ns: Delta::of(&c.getppid_ns, &base.getppid_ns),
            pread_1b_ns: Delta::of(&c.pread_1b_ns, &base.pread_1b_ns),
            open_close_ns: Delta::of(&c.open_close_ns, &base.open_close_ns),
            spawn_us: Delta::of(&c.spawn_restrict_join_us, &base.spawn_restrict_join_us),
        })
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct H21Outcome {
    pub landlock: LandlockReport,
    pub seccomp: SeccompReport,
    pub spawn_trace: SpawnTrace,
    pub bench: BenchReport,
    pub overheads: Vec<Overhead>,
    pub landlock_ok: bool,
    pub seccomp_ok: bool,
    pub bench_sanity_ok: bool,
    pub verdict: Verdict,
    pub summary: String,
}

fn describe_mismatches(probes: &[Probe]) -> String {
    mismatches(probes, Role::Criterion)
        .iter()
        .map(|p| format!("{}:{}({}) esperava {:?}, veio {:?}", p.thread, p.action, p.target, p.expect, p.result))
        .collect::<Vec<_>>()
        .join("; ")
}

/// `bench_scale` é `full` ou `quick` (ver [`crate::child::BenchScale`]).
pub fn run(exe: &Path, bench_scale: &str) -> Result<H21Outcome> {
    let landlock: LandlockReport = run_child(exe, &["landlock"])?;
    let seccomp: SeccompReport = run_child(exe, &["seccomp"])?;
    let spawn_trace = spawn_trace(exe);
    let bench: BenchReport = run_child(exe, &["bench", bench_scale])?;
    let overheads = overheads(&bench);

    let landlock_ok = landlock.applied.as_ref().is_some_and(|a| a.ruleset == "fully_enforced")
        && mismatches(&landlock.probes, Role::Criterion).is_empty();
    let seccomp_ok = seccomp.apply_error.is_none() && mismatches(&seccomp.probes, Role::Criterion).is_empty();
    let bench_sanity_ok =
        mismatches(&bench.sanity, Role::Control).is_empty() && mismatches(&bench.inherited_sanity, Role::Control).is_empty();

    let abi = landlock.applied.as_ref().and_then(|a| a.effective_abi).map_or("?".to_string(), |v| v.to_string());
    let get = |c: BenchConfig| overheads.iter().find(|o| o.config == c);
    let fmt_over = |c: BenchConfig| {
        get(c).map_or("sem dado".to_string(), |o| {
            format!(
                "open+close {:+.0} ns ({:+.1}%), pread 1 B {:+.0} ns, getppid {:+.0} ns, criar thread e aplicar {:+.0} µs",
                o.open_close_ns.min, o.open_close_ns.min_pct, o.pread_1b_ns.min, o.getppid_ns.min, o.spawn_us.min,
            )
        })
    };
    let base = bench.configs.iter().find(|c| c.config == BenchConfig::None);
    let base_open = base.map_or(f64::NAN, |c| c.open_close_ns.min);
    let numbers = format!(
        "Overhead pela melhor de {} rodadas (sobre {base_open:.0} ns de open+close sem filtro; medianas no JSON, \
         ruidosas na máquina compartilhada): Landlock {}; seccomp {}; os dois {}; seccomp com BPF forçado nas \
         syscalls do laço {}. Thread criada por uma thread já restrita herda os dois domínios em {:.1} µs (thread \
         sem restrição: {:.1} µs).",
        bench.scale.rounds,
        fmt_over(BenchConfig::Landlock),
        fmt_over(BenchConfig::Seccomp),
        fmt_over(BenchConfig::LandlockSeccomp),
        fmt_over(BenchConfig::SeccompArgChecked),
        bench.inherited_spawn_us.min,
        base.map_or(f64::NAN, |c| c.spawn_restrict_join_us.min),
    );

    let (verdict, summary) = match (landlock_ok, seccomp_ok, bench_sanity_ok) {
        (true, true, true) => (
            Verdict::Confirmed,
            format!(
                "Landlock ABI v{abi} (fully_enforced): a thread restrita leva EACCES fora do diretório montado e lê, \
                 cria e lista dentro dele; a filha herda; a principal, a vizinha criada depois e a que já existia \
                 seguem livres. seccomp por thread: socket, connect, execve, execveat, fork e io_uring_setup voltam \
                 EPERM só na thread filtrada e nas filhas, e criar thread continua funcionando. {numbers}"
            ),
        ),
        (false, false, _) => (
            Verdict::Refuted,
            format!(
                "Landlock: {}. seccomp: {}.",
                describe_mismatches(&landlock.probes),
                describe_mismatches(&seccomp.probes)
            ),
        ),
        _ => (
            Verdict::Partial,
            format!(
                "Landlock {} ({}); seccomp {} ({}); conferência da medição {}. {numbers}",
                if landlock_ok { "ok" } else { "falhou" },
                describe_mismatches(&landlock.probes),
                if seccomp_ok { "ok" } else { "falhou" },
                describe_mismatches(&seccomp.probes),
                if bench_sanity_ok { "ok" } else { "falhou" },
            ),
        ),
    };

    Ok(H21Outcome {
        landlock,
        seccomp,
        spawn_trace,
        bench,
        overheads,
        landlock_ok,
        seccomp_ok,
        bench_sanity_ok,
        verdict,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strace_phases_and_resumed_lines() {
        let text = "\
100 statx(AT_FDCWD, \"/nonexistent-e06-marker/restricted\", AT_STATX_SYNC_AS_STAT, STATX_ALL, 0x7ffd) = -1 ENOENT (No such file or directory)
100 clone3({flags=CLONE_VM|CLONE_VFORK|CLONE_CLEAR_SIGHAND, exit_signal=SIGCHLD, stack=0x7f, stack_size=0x9000}, 88) = -1 ENOSYS (Function not implemented)
100 clone(child_stack=0x7f, flags=CLONE_VM|CLONE_VFORK|SIGCHLD) = -1 EPERM (Operation not permitted)
100 clone(child_stack=0x7f, flags=CLONE_VM|CLONE_FS|CLONE_FILES|CLONE_SIGHAND|CLONE_THREAD|CLONE_SYSVSEM <unfinished ...>
101 statx(AT_FDCWD, \"/x\", 0, 0, 0x1) = -1 ENOENT (No such file or directory)
100 <... clone resumed>, parent_tid=[102], tls=0x7f, child_tidptr=0x7f) = 102
100 statx(AT_FDCWD, \"/nonexistent-e06-marker/main_after\", AT_STATX_SYNC_AS_STAT, STATX_ALL, 0x7ffd) = -1 ENOENT (No such file or directory)
100 clone(child_stack=0x7f, flags=CLONE_VM|CLONE_FS|CLONE_THREAD, parent_tid=[103]) = 103
";
        let phases = parse_strace(text);
        let r = &phases["restricted"];
        assert_eq!(r.len(), 3, "{r:?}");
        assert_eq!(r[0].syscall, "clone3");
        assert!(r[0].result.contains("ENOSYS"));
        assert!(!r[1].clone_thread && r[1].result.contains("EPERM"));
        assert!(r[2].clone_thread);
        assert_eq!(r[2].result, "102");
        assert_eq!(phases["main_after"].len(), 1);
    }
}
