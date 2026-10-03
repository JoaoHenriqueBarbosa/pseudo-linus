//! E06: refaz tudo e grava `results/e06-isolation.json`.
//!
//! `e06-isolation` roda H19, H20 e H21 (medição completa); `--quick` encurta a medição.
//! `e06-isolation child <modo>` é o subprocesso onde Landlock e seccomp são aplicados.

use std::time::Instant;

use anyhow::Result;
use harness::{CandidateResult, ExperimentResult, Fit};
use serde_json::{Value, json};

use e06_isolation::child::BenchConfig;
use e06_isolation::probe::Role;
use e06_isolation::{child, h19, h20, h21, layout};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("child") {
        return child::main(&args[1..]);
    }
    let bench_scale = if args.iter().any(|a| a == "--quick") { "quick" } else { "full" };
    let exe = std::env::current_exe()?;
    let start = Instant::now();

    eprintln!("[e06] H19: cargo clippy nas sondas de userland");
    let h19 = h19::run()?;
    eprintln!("[e06] H19 {:?}: {}", h19.verdict, h19.summary);
    eprintln!("[e06] H20: cargo build do consumidor com forbid(unsafe_code), um caso por vez");
    let h20 = h20::run()?;
    eprintln!("[e06] H20 {:?}: {}", h20.verdict, h20.summary);
    eprintln!("[e06] H21: Landlock e seccomp por thread num subprocesso (medição {bench_scale})");
    let h21 = h21::run(&exe, bench_scale)?;
    eprintln!("[e06] H21 {:?}: {}", h21.verdict, h21.summary);

    let mut result = ExperimentResult::new(
        "e06-isolation",
        "Isolamento em três camadas: lint de userland, forbid(unsafe_code), Landlock e seccomp por thread",
    );
    result.hypothesis("H19", h19.verdict, h19.summary.clone(), h19_evidence(&h19));
    result.hypothesis("H20", h20.verdict, h20.summary.clone(), h20_evidence(&h20));
    result.hypothesis("H21", h21.verdict, h21.summary.clone(), h21_evidence(&h21));
    result.candidates = candidates(&h21)?;
    result.metrics = json!({
        "bench_scale": bench_scale,
        "bench": h21.bench,
        "overheads_vs_none": h21.overheads,
        "host_security": host_security(),
        "elapsed_s": start.elapsed().as_secs_f64(),
    });
    result.notes = notes(&h19, &h20, &h21);
    let path = result.write()?;
    println!("{}", path.display());
    for h in &result.hypotheses {
        println!("{} {}: {}", h.id, h.verdict.label_pt(), h.summary);
    }
    Ok(())
}

fn h19_evidence(h: &h19::H19Outcome) -> Value {
    json!({
        "clippy_indirect": {
            "command": h.indirect.command,
            "exit_code": h.indirect.exit_code,
            "disallowed_diagnostics": h.indirect_disallowed,
            "diagnostics": h.indirect.diagnostics,
        },
        "clippy_direct": {
            "command": h.direct.command,
            "exit_code": h.direct.exit_code,
            "modules": h.direct_modules,
            "diagnostics": h.direct.diagnostics,
        },
        "depscan": { "userland": h.depscan_indirect, "userland_direct": h.depscan_direct },
    })
}

fn h20_evidence(h: &h20::H20Outcome) -> Value {
    json!({
        "baseline": h.baseline,
        "cases": h.cases,
        "depscan_consumer_root_unsafe": h.depscan_root_unsafe,
        "depscan_unsafe_by_dependency": h.depscan_unsafe,
    })
}

fn h21_evidence(h: &h21::H21Outcome) -> Value {
    json!({
        "landlock": {
            "mounted_dir": h.landlock.mounted_dir,
            "applied": h.landlock.applied,
            "restrict_error": h.landlock.restrict_error,
            "tsync_hard_requirement": h.landlock.tsync_hard_requirement,
            "probes": h.landlock.probes,
            "ok": h.landlock_ok,
        },
        "seccomp": {
            "denied": h.seccomp.denied,
            "bpf_instructions": { "deny": h.seccomp.deny_instructions, "clone3": h.seccomp.clone3_instructions },
            "apply_error": h.seccomp.apply_error,
            "probes": h.seccomp.probes,
            "ok": h.seccomp_ok,
        },
        "strace_spawn": h.spawn_trace,
        "bench_sanity_ok": h.bench_sanity_ok,
        "overheads_vs_none": h.overheads,
    })
}

fn candidates(h: &h21::H21Outcome) -> Result<Vec<CandidateResult>> {
    let manifest = layout::experiment_dir().join("Cargo.toml");
    let over = |c: BenchConfig| h.overheads.iter().find(|o| o.config == c).cloned();
    let mut out = Vec::new();
    for (name, role, ok, notes, metrics) in [
        (
            "landlock",
            "isolamento em runtime: FS do host por thread",
            h.landlock_ok,
            "API segura; restrict_self vale pra thread atual e pras filhas criadas depois; ABI efetiva lida do RestrictionStatus; all_threads (TSYNC) só na ABI v8.",
            json!({ "applied": h.landlock.applied, "overhead": over(BenchConfig::Landlock) }),
        ),
        (
            "seccompiler",
            "isolamento em runtime: syscalls por thread",
            h.seccomp_ok,
            "API segura; apply_filter só na thread atual; uma ação por filtro, então clone3 (ENOSYS) vai num segundo filtro; não cobre x32 sozinho.",
            json!({
                "bpf_instructions": { "deny": h.seccomp.deny_instructions, "clone3": h.seccomp.clone3_instructions },
                "overhead": over(BenchConfig::Seccomp),
                "overhead_arg_checked": over(BenchConfig::SeccompArgChecked),
            }),
        ),
    ] {
        let scan = depscan::scan(&manifest, name)?;
        let mut metrics = metrics;
        metrics["depscan"] = json!({
            "host_touch": scan.root.counts.host_touch(),
            "unsafe": scan.root.counts.unsafe_total(),
            "tree_category": scan.tree_category.letter(),
        });
        out.push(CandidateResult {
            name: name.to_string(),
            version: scan.root.version.clone(),
            role: role.to_string(),
            category: Some(scan.root.category.letter().to_string()),
            conformance: None,
            fit: if ok { Fit::Fits } else { Fit::DoesNotFit },
            notes: notes.to_string(),
            metrics,
        });
    }
    Ok(out)
}

fn read_trimmed(path: &str) -> String {
    std::fs::read_to_string(path).map(|s| s.trim().to_string()).unwrap_or_else(|e| format!("erro: {e}"))
}

fn host_security() -> Value {
    json!({
        "lsm": read_trimmed("/sys/kernel/security/lsm"),
        "io_uring_disabled": read_trimmed("/proc/sys/kernel/io_uring_disabled"),
        "spec_store_bypass": read_trimmed("/sys/devices/system/cpu/vulnerabilities/spec_store_bypass"),
        "yama_ptrace_scope": read_trimmed("/proc/sys/kernel/yama/ptrace_scope"),
    })
}

/// Achados pro design, cada um amarrado a uma sonda medida.
fn notes(h19: &h19::H19Outcome, h20: &h20::H20Outcome, h21: &h21::H21Outcome) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(m) = h19.direct_modules.iter().find(|m| m.module == "handle_from_dep") {
        notes.push(format!(
            "H19: um std::fs::File que chega por dependência e é lido pelo trait Read no crate de userland {} pelo clippy; \
             disallowed-types só enxerga o tipo quando ele é escrito no código.",
            if m.caught { "foi pego" } else { "não é pego" }
        ));
    }
    if let Some(m) = h19.direct_modules.iter().find(|m| m.module == "direct_print_macro") {
        notes.push(format!(
            "H19: println! {} (com disallowed-macros configurado); sem essa lista, disallowed-methods de std::io::stdout não enxerga a macro.",
            if m.caught { "foi pego" } else { "não foi pego" }
        ));
    }
    if let Some(c) = h20.cases.iter().find(|c| c.feature == "intrusive_adapter") {
        notes.push(format!(
            "H20: intrusive_adapter! terminou em {:?}; com deny no lugar de forbid o allow(unsafe_code) da macro passaria.",
            c.outcome
        ));
    }
    notes.push(format!(
        "H20: o depscan conta {} unsafe no próprio consumidor e acha o unsafe nas crates de macro ({}); a camada que pega \
         unsafe gerado por macro é o scanner de dependências, não o lint.",
        h20.depscan_root_unsafe,
        h20.depscan_unsafe.keys().cloned().collect::<Vec<_>>().join(", ")
    ));
    let find = |probes: &[e06_isolation::probe::Probe], thread: &str, action: &str| {
        probes.iter().find(|p| p.thread == thread && p.action == action && p.role != Role::Control).cloned()
    };
    if let Some(p) = find(&h21.landlock.probes, "pool_worker_created_before", "read_file_for_restricted_thread") {
        notes.push(format!(
            "H21: thread que já existia antes da restrição (modelo de pool global, rayon/tokio) fez I/O de host a pedido da \
             thread restrita: {}. Pseudo-processo não pode despachar trabalho pra pool compartilhado com o kernel.",
            if p.result.ok { "funcionou" } else { "falhou" }
        ));
    }
    if let Some(p) = find(&h21.landlock.probes, "restricted", "read_preopened_fd") {
        notes.push(format!(
            "H21: fd aberto antes do restrict_self continua legível na thread restrita ({}); reabrir pelo /proc/self/fd \
             passa pelo Landlock de novo ({}).",
            if p.result.ok { "ok" } else { "negado" },
            find(&h21.landlock.probes, "restricted", "reopen_via_proc_fd")
                .and_then(|q| q.result.errno_name.clone())
                .unwrap_or_else(|| "permitido".to_string())
        ));
    }
    notes.push(format!(
        "H21: TSYNC (all_threads) como requisito duro: {}.",
        match (&h21.landlock.tsync_hard_requirement.accepted, &h21.landlock.tsync_hard_requirement.error) {
            (true, _) => "aceito pelo kernel".to_string(),
            (false, Some(e)) => format!("recusado ({e})"),
            (false, None) => "recusado".to_string(),
        }
    ));
    let t = &h21.spawn_trace;
    if t.available {
        notes.push(format!(
            "H21 (strace): na thread filtrada clone3 voltou ENOSYS: {}, o fork caiu no clone e levou EPERM: {}, a thread \
             nasceu por clone com CLONE_THREAD: {}. Efeito colateral global da glibc: depois disso a thread principal, sem \
             filtro, {} thread por clone3 e {} processo por clone3 (o ENOSYS fica num estático do processo).",
            t.restricted_clone3_enosys,
            t.restricted_fork_clone_eperm,
            t.restricted_thread_clone_ok,
            if t.after_thread_via_clone3 { "ainda cria" } else { "não cria mais" },
            if t.after_process_via_clone3 { "ainda cria" } else { "não cria mais" },
        ));
    } else {
        notes.push(format!("H21: evidência por strace indisponível: {}", t.error.clone().unwrap_or_default()));
    }
    notes.push(
        "H21: socket e connect ficam negados na thread do pseudo-processo, então a rede (ureq do F12) tem que rodar numa \
         thread do kernel fora do filtro, ou trocar a negação por regras de rede do Landlock (AccessNet, ABI v4+)."
            .to_string(),
    );
    notes
}
