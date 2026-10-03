//! H08: stack overflow num pseudo-processo derruba o host; `stacker::maybe_grow` com limite de
//! profundidade mitiga.
//!
//! Cada variante roda num subprocesso (`overflow <variante>`), sem core dump. O pai registra se o
//! subprocesso morreu e com que sinal; se sobreviveu, o subprocesso imprime um JSON com o status do
//! pseudo-processo, a profundidade alcançada e o que o `stacker` achava da pilha.
//!
//! Variantes: `*-plain` (recursão sem limite), `*-stacker` (recursão com `maybe_grow` e limite de
//! profundidade de 100 mil quadros, cerca de 60 MB de pilha) e `*-stacker-interleaved` (dois processos
//! na mesma CPU virtual: X suspende no meio da recursão, dentro de um segmento que o stacker alocou, e Y
//! recursa enquanto isso; no B os dois dividem o thread-local `STACK_LIMIT` do stacker).

use std::hint::black_box;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use harness::Verdict;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{HypOut, progress, status_str};
use crate::kernel::ExitStatus;
use crate::sys::Sys;
use crate::{ModelKind, ModelSpec};

const RED_ZONE: usize = 32 * 1024;
const GROW: usize = 1024 * 1024;
const DEPTH_LIMIT: u64 = 100_000;
/// Código de saída do pseudo-processo quando o limite de profundidade é atingido (como o "nesting too
/// deep" de um interpretador).
const EXIT_TOO_DEEP: i32 = 3;

pub const VARIANTS: &[&str] = &[
    "A-plain",
    "B64-plain",
    "B256-plain",
    "C-plain",
    "A-stacker",
    "B64-stacker",
    "B256-stacker",
    "C-stacker",
    "A-stacker-interleaved",
    // B256 porque é onde o stacker sozinho funciona (vê 0 bytes e cresce na primeira chamada): o que se
    // testa aqui é só o efeito de duas corrotinas dividirem o thread-local dele.
    "B256-stacker-interleaved",
];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChildReport {
    pub variant: String,
    pub statuses: Vec<String>,
    pub depth_reached: u64,
    /// O que `stacker::remaining_stack()` dizia na entrada do pseudo-processo, em bytes.
    pub remaining_stack_at_entry: Option<u64>,
    /// Idem pro processo Y da variante intercalada (que começa com X suspenso num segmento do stacker).
    pub remaining_stack_at_entry_y: Option<u64>,
    /// Tamanho real da pilha do pseudo-processo (0 quando é a pilha do worker).
    pub real_stack_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct OverflowRow {
    pub variant: String,
    pub host_survived: bool,
    pub signal: Option<i32>,
    pub exit_code: Option<i32>,
    pub stderr_head: String,
    pub child: Option<ChildReport>,
    /// `stacker::remaining_stack()` na entrada do processo (X) e do Y, lido do stderr.
    pub remaining_at_entry: Option<u64>,
    pub remaining_at_entry_y: Option<u64>,
    pub elapsed_ms: f64,
}

#[inline(never)]
fn plain(depth: u64) -> u64 {
    let mut pad = [0u8; 512];
    pad[(depth % 512) as usize] = depth as u8;
    black_box(&mut pad);
    if black_box(depth) == u64::MAX {
        return 0;
    }
    plain(depth + 1).wrapping_add(u64::from(pad[7]))
}

#[inline(never)]
fn guarded(depth: u64, limit: u64, deepest: &AtomicU64, pause: &dyn Fn(u64)) -> Result<u64, u64> {
    if depth >= limit {
        return Err(depth);
    }
    deepest.fetch_max(depth, Ordering::Relaxed);
    pause(depth);
    stacker::maybe_grow(RED_ZONE, GROW, || {
        let mut pad = [0u8; 512];
        pad[(depth % 512) as usize] = depth as u8;
        black_box(&mut pad);
        guarded(depth + 1, limit, deepest, pause).map(|v| v.wrapping_add(u64::from(pad[7])))
    })
}

fn remaining() -> u64 {
    stacker::remaining_stack().map(|r| r as u64).unwrap_or(u64::MAX)
}

/// Registra o que o stacker acha da pilha na entrada do processo. Vai também pro stderr, pra que o pai
/// tenha o número mesmo quando o subprocesso morre logo depois.
fn note_entry(slot: &AtomicU64, tag: &str) {
    let r = remaining();
    eprintln!("E01_REMAINING_{tag}={r}");
    slot.store(r, Ordering::Relaxed);
}

/// Subcomando `overflow <variante>`: pode derrubar o processo (é o ponto).
pub fn overflow_child(variant: &str) -> ChildReport {
    let _ = rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable);
    let _ = rustix::process::setrlimit(
        rustix::process::Resource::Core,
        rustix::process::Rlimit { current: Some(0), maximum: None },
    );
    let (model, rest) = variant.split_once('-').expect("variante <modelo>-<tipo>");
    let spec = ModelSpec::parse(model).expect("modelo");
    let real_stack = if spec.kind == ModelKind::C { 0 } else { spec.stack as u64 };
    let deepest = Arc::new(AtomicU64::new(0));
    let entry_remaining = Arc::new(AtomicU64::new(0));
    let entry_remaining_y = Arc::new(AtomicU64::new(0));
    let mut report = ChildReport { variant: variant.to_string(), real_stack_bytes: real_stack, ..Default::default() };

    let statuses: Vec<ExitStatus> = match (spec.kind, rest) {
        (ModelKind::C, "plain") => {
            let k = spec.c_kernel(1, false);
            let pid = k.spawn(Vec::new(), |_ctx| async { plain(0) as i32 });
            vec![k.wait(pid)]
        }
        (ModelKind::C, "stacker") => {
            let k = spec.c_kernel(1, false);
            let (d2, e2) = (deepest.clone(), entry_remaining.clone());
            let pid = k.spawn(Vec::new(), move |_ctx| async move {
                note_entry(&e2, "X");
                match guarded(0, DEPTH_LIMIT, &d2, &|_| {}) {
                    Ok(_) => 0,
                    Err(_) => EXIT_TOO_DEEP,
                }
            });
            vec![k.wait(pid)]
        }
        (_, "plain") => {
            let k = spec.sync_kernel(1, false).expect("sync");
            let pid = k.spawn(Vec::new(), Box::new(|_s: &dyn Sys| plain(0) as i32));
            vec![k.wait(pid)]
        }
        (_, "stacker") => {
            let k = spec.sync_kernel(1, false).expect("sync");
            let (d2, e2) = (deepest.clone(), entry_remaining.clone());
            let pid = k.spawn(
                Vec::new(),
                Box::new(move |_s: &dyn Sys| {
                    note_entry(&e2, "X");
                    match guarded(0, DEPTH_LIMIT, &d2, &|_| {}) {
                        Ok(_) => 0,
                        Err(_) => EXIT_TOO_DEEP,
                    }
                }),
            );
            vec![k.wait(pid)]
        }
        (_, "stacker-interleaved") => {
            let k = spec.sync_kernel(1, false).expect("sync");
            let y_done = Arc::new(AtomicBool::new(false));
            let yd = y_done.clone();
            let e2 = entry_remaining.clone();
            let ey = entry_remaining_y.clone();
            let x = k.spawn(
                Vec::new(),
                Box::new(move |sys: &dyn Sys| {
                    note_entry(&e2, "X");
                    let d = AtomicU64::new(0);
                    // Na profundidade 2000 (já dentro de um segmento do stacker), cede a CPU até Y terminar.
                    let pause = |depth: u64| {
                        if depth == 2000 {
                            while !yd.load(Ordering::Acquire) {
                                sys.yield_now();
                                std::thread::yield_now();
                            }
                        }
                    };
                    match guarded(0, 4000, &d, &pause) {
                        Ok(_) => 0,
                        Err(_) => EXIT_TOO_DEEP,
                    }
                }),
            );
            let d2 = deepest.clone();
            let y = k.spawn(
                Vec::new(),
                Box::new(move |_s: &dyn Sys| {
                    // Y começa com X suspenso dentro de um segmento do stacker.
                    note_entry(&ey, "Y");
                    let r = match guarded(0, DEPTH_LIMIT, &d2, &|_| {}) {
                        Ok(_) => 0,
                        Err(_) => EXIT_TOO_DEEP,
                    };
                    y_done.store(true, Ordering::Release);
                    r
                }),
            );
            let sy = k.wait(y);
            let sx = k.wait(x);
            vec![sy, sx]
        }
        _ => panic!("variante desconhecida: {variant}"),
    };
    report.statuses = statuses.iter().map(status_str).collect();
    report.depth_reached = deepest.load(Ordering::Relaxed);
    report.remaining_stack_at_entry = Some(entry_remaining.load(Ordering::Relaxed));
    if rest == "stacker-interleaved" {
        report.remaining_stack_at_entry_y = Some(entry_remaining_y.load(Ordering::Relaxed));
    }
    report
}

/// Lê do stderr do subprocesso o que o stacker dizia na entrada (vale também quando ele morreu).
fn remaining_from_stderr(stderr: &str, tag: &str) -> Option<u64> {
    let key = format!("E01_REMAINING_{tag}=");
    stderr.lines().find_map(|l| l.strip_prefix(&key)).and_then(|v| v.trim().parse().ok())
}

const CHILD_TIMEOUT: Duration = Duration::from_secs(60);

/// Roda uma variante num subprocesso do binário `exe` (o próprio binário do experimento, ou o
/// `CARGO_BIN_EXE_*` nos testes de integração).
pub fn run_child(exe: &std::path::Path, variant: &str) -> OverflowRow {
    let t = Instant::now();
    let mut child = Command::new(exe)
        .args(["overflow", variant])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("subprocesso");
    // A saída é curta (cabe no buffer do pipe), então dá pra esperar com prazo antes de ler.
    let mut timed_out = false;
    while child.try_wait().ok().flatten().is_none() {
        if t.elapsed() > CHILD_TIMEOUT {
            let _ = child.kill();
            timed_out = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let out = child.wait_with_output().expect("saída do subprocesso");
    let mut stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if timed_out {
        stderr.insert_str(0, "prazo estourado | ");
    }
    let stderr_head: String = stderr
        .lines()
        .filter(|l| !l.starts_with("E01_REMAINING_") && !l.trim().is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join(" | ")
        .chars()
        .take(240)
        .collect();
    let child = String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .and_then(|l| serde_json::from_str::<ChildReport>(l).ok());
    OverflowRow {
        variant: variant.to_string(),
        host_survived: out.status.success() && child.is_some(),
        signal: out.status.signal(),
        exit_code: out.status.code(),
        stderr_head,
        child,
        remaining_at_entry: remaining_from_stderr(&stderr, "X"),
        remaining_at_entry_y: remaining_from_stderr(&stderr, "Y"),
        elapsed_ms: (t.elapsed().as_secs_f64() * 1e3 * 1000.0).round() / 1000.0,
    }
}

/// Execuções por variante: o resultado do stacker numa corrotina depende de onde o mmap colocou cada
/// pilha (ASLR), então as variantes com stacker rodam várias vezes.
fn runs_for(variant: &str) -> usize {
    if variant.contains("-stacker") { 5 } else { 2 }
}

pub fn measure() -> Vec<OverflowRow> {
    let exe = std::env::current_exe().expect("current_exe");
    let mut rows = Vec::new();
    for v in VARIANTS {
        progress(format!("H08: overflow {v} (subprocesso, {} execuções)", runs_for(v)));
        for _ in 0..runs_for(v) {
            rows.push(run_child(&exe, v));
        }
    }
    rows
}

fn survived_with_error(r: &OverflowRow) -> bool {
    r.host_survived
        && r.child
            .as_ref()
            .is_some_and(|c| c.statuses.first().map(String::as_str) == Some(&format!("exit {EXIT_TOO_DEEP}")))
}

#[derive(Serialize)]
struct VariantSummary {
    variant: String,
    runs: usize,
    host_survived: usize,
    signals: Vec<i32>,
    remaining_at_entry: Vec<u64>,
    remaining_at_entry_y: Vec<u64>,
}

pub fn verdict(rows: &[OverflowRow]) -> HypOut {
    let summaries: Vec<VariantSummary> = VARIANTS
        .iter()
        .map(|v| {
            let rs: Vec<&OverflowRow> = rows.iter().filter(|r| r.variant == *v).collect();
            let mut signals: Vec<i32> = rs.iter().filter_map(|r| r.signal).collect();
            signals.sort_unstable();
            signals.dedup();
            let mut rem: Vec<u64> = rs.iter().filter_map(|r| r.remaining_at_entry).collect();
            rem.sort_unstable();
            rem.dedup();
            let mut rem_y: Vec<u64> = rs.iter().filter_map(|r| r.remaining_at_entry_y).collect();
            rem_y.sort_unstable();
            rem_y.dedup();
            VariantSummary {
                variant: v.to_string(),
                runs: rs.len(),
                host_survived: rs.iter().filter(|r| survived_with_error(r)).count(),
                signals,
                remaining_at_entry: rem,
                remaining_at_entry_y: rem_y,
            }
        })
        .collect();
    let get = |v: &str| summaries.iter().find(|s| s.variant == v);
    let plain_crash = summaries
        .iter()
        .filter(|s| s.variant.ends_with("-plain"))
        .all(|s| s.runs > 0 && s.host_survived == 0 && !s.signals.is_empty());
    let stacker: Vec<&VariantSummary> = summaries.iter().filter(|s| s.variant.contains("-stacker")).collect();
    let ok: Vec<&str> =
        stacker.iter().filter(|s| s.runs > 0 && s.host_survived == s.runs).map(|s| s.variant.as_str()).collect();
    let failed: Vec<&VariantSummary> = stacker.iter().copied().filter(|s| s.host_survived < s.runs).collect();
    let verdict = if !plain_crash {
        Verdict::Refuted
    } else if failed.is_empty() {
        Verdict::Confirmed
    } else if !ok.is_empty() {
        Verdict::Partial
    } else {
        Verdict::Refuted
    };
    let sigs = |s: &VariantSummary| s.signals.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("/");
    let plains: Vec<String> = summaries
        .iter()
        .filter(|s| s.variant.ends_with("-plain"))
        .map(|s| format!("{} morreu com sinal {}", s.variant, sigs(s)))
        .collect();
    let failed_desc: Vec<String> = failed
        .iter()
        .map(|s| format!("{} derrubou o host em {} de {} execuções (sinal {})", s.variant, s.runs - s.host_survived, s.runs, sigs(s)))
        .collect();
    let fmt_rem = |v: &[u64]| {
        if v.is_empty() {
            "sem dado".to_string()
        } else {
            v.iter().map(|x| format!("{x}")).collect::<Vec<_>>().join(" ou ") + " bytes"
        }
    };
    let rem_b = format!(
        "B64 {}, B256 {}, e o Y da variante intercalada (X suspenso num segmento do stacker) {}",
        fmt_rem(get("B64-stacker").map(|s| s.remaining_at_entry.as_slice()).unwrap_or(&[])),
        fmt_rem(get("B256-stacker").map(|s| s.remaining_at_entry.as_slice()).unwrap_or(&[])),
        fmt_rem(get("B256-stacker-interleaved").map(|s| s.remaining_at_entry_y.as_slice()).unwrap_or(&[])),
    );
    HypOut {
        id: "H08",
        verdict,
        summary: format!(
            "Sem mitigação: {}. Com stacker::maybe_grow e limite de {DEPTH_LIMIT} quadros o pseudo-processo sai com \
             erro e o host sobrevive em todas as execuções de: {}; falha em: {}. Nas corrotinas o stacker calcula a \
             pilha restante com os limites da thread do worker (thread-local STACK_LIMIT), não com os da corrotina: \
             na entrada ele viu {rem_b}; 0 faz ele crescer já na primeira chamada (funciona por acaso), um valor maior \
             que a pilha real faz ele nunca crescer, e qual dos dois acontece depende de onde o mmap pôs cada pilha.",
            plains.join(", "),
            if ok.is_empty() { "nenhum".to_string() } else { ok.join(", ") },
            if failed_desc.is_empty() { "nenhum".to_string() } else { failed_desc.join("; ") },
        ),
        evidence: json!({
            "per_variant": summaries,
            "rows": rows,
            "depth_limit": DEPTH_LIMIT,
            "red_zone_bytes": RED_ZONE,
            "grow_bytes": GROW,
            "child_timeout_s": CHILD_TIMEOUT.as_secs(),
        }),
    }
}
