//! H09: panic num pseudo-processo fica isolado e não envenena o kernel.
//!
//! Um objeto compartilhado do "kernel" (contador) existe em duas versões: atrás de `std::sync::Mutex`
//! (com poisoning) e de `parking_lot::Mutex` (sem). Em cada modelo e cada variante, um processo entra em
//! panic (dentro do lock ou fora dele), o host confere que o `wait` devolve término anormal, e depois 20
//! processos usam o mesmo objeto: conta quantos terminam normalmente. Por fim um pipeline pequeno passa
//! pelo mesmo kernel pra provar que ele continua atendendo.
//!
//! Variantes: `std_unwrap` (o idioma comum `lock().unwrap()`, que propaga o poison), `std_into_inner`
//! (`unwrap_or_else(PoisonError::into_inner)`, que trata o poison), `parking_lot` e `outside_lock`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError};

use harness::Verdict;
use serde::Serialize;
use serde_json::json;

use super::{HypOut, is_sync, progress, status_str};
use crate::ModelSpec;
use crate::kernel::{ExitStatus, File, new_pipe};
use crate::sys::Sys;
use crate::workloads::{WcOut, asyncs, sync};

const OTHERS: usize = 20;
const VARIANTS: [&str; 4] = ["std_unwrap", "std_into_inner", "parking_lot", "outside_lock"];

struct KObj {
    std_m: std::sync::Mutex<u64>,
    pl_m: parking_lot::Mutex<u64>,
}

fn faulty(obj: &KObj, variant: &str) -> i32 {
    match variant {
        "std_unwrap" | "std_into_inner" => {
            let mut g = obj.std_m.lock().unwrap_or_else(PoisonError::into_inner);
            *g += 1;
            panic!("bug simulado no kernel com std::sync::Mutex travado");
        }
        "parking_lot" => {
            let mut g = obj.pl_m.lock();
            *g += 1;
            panic!("bug simulado no kernel com parking_lot::Mutex travado");
        }
        _ => panic!("bug simulado num builtin, fora de lock"),
    }
}

fn normal(obj: &KObj, variant: &str) -> i32 {
    match variant {
        "std_unwrap" => *obj.std_m.lock().unwrap() += 1,
        "std_into_inner" => *obj.std_m.lock().unwrap_or_else(PoisonError::into_inner) += 1,
        _ => *obj.pl_m.lock() += 1,
    }
    0
}

#[derive(Clone, Debug, Serialize)]
pub struct PanicRow {
    pub model: String,
    pub variant: String,
    pub faulty_status: String,
    pub faulty_abnormal: bool,
    pub others_ok: usize,
    pub others: usize,
    pub std_mutex_poisoned: bool,
    pub kernel_alive: bool,
}

fn alive_check_sync(k: &dyn crate::SyncKernel) -> bool {
    let (r, w) = new_pipe();
    let out = Arc::new(WcOut::default());
    let o2 = out.clone();
    let g = k.spawn(vec![(1, w)], Box::new(|s: &dyn Sys| sync::gen_text(s, 1 << 20)));
    let c = k.spawn(
        vec![(0, r), (1, File::Sink(Arc::new(AtomicU64::new(0))))],
        Box::new(move |s: &dyn Sys| sync::wc(s, &o2, 16 * 1024)),
    );
    k.wait(g) == ExitStatus::Exited(0) && k.wait(c) == ExitStatus::Exited(0) && out.bytes.load(Ordering::Relaxed) == 1 << 20
}

fn alive_check_c(k: &crate::model_c::KernelC) -> bool {
    let (r, w) = new_pipe();
    let out = Arc::new(WcOut::default());
    let o2 = out.clone();
    let g = k.spawn(vec![(1, w)], |c| asyncs::gen_text(c, 1 << 20));
    let c = k.spawn(vec![(0, r), (1, File::Sink(Arc::new(AtomicU64::new(0))))], move |c| asyncs::wc(c, o2, 16 * 1024));
    k.wait(g) == ExitStatus::Exited(0) && k.wait(c) == ExitStatus::Exited(0) && out.bytes.load(Ordering::Relaxed) == 1 << 20
}

fn run_variant(spec: ModelSpec, variant: &'static str) -> PanicRow {
    let obj = Arc::new(KObj { std_m: std::sync::Mutex::new(0), pl_m: parking_lot::Mutex::new(0) });
    let (faulty_status, statuses, alive) = if is_sync(spec) {
        let k = spec.sync_kernel(2, true).expect("sync");
        let o = obj.clone();
        let f = k.spawn(Vec::new(), Box::new(move |_s: &dyn Sys| faulty(&o, variant)));
        let fs = k.wait(f);
        let pids: Vec<_> = (0..OTHERS)
            .map(|_| {
                let o = obj.clone();
                k.spawn(Vec::new(), Box::new(move |_s: &dyn Sys| normal(&o, variant)))
            })
            .collect();
        let sts: Vec<_> = pids.into_iter().map(|p| k.wait(p)).collect();
        let alive = alive_check_sync(k.as_ref());
        (fs, sts, alive)
    } else {
        let k = spec.c_kernel(2, true);
        let o = obj.clone();
        let f = k.spawn(Vec::new(), move |_c| async move { faulty(&o, variant) });
        let fs = k.wait(f);
        let pids: Vec<_> = (0..OTHERS)
            .map(|_| {
                let o = obj.clone();
                k.spawn(Vec::new(), move |_c| async move { normal(&o, variant) })
            })
            .collect();
        let sts: Vec<_> = pids.into_iter().map(|p| k.wait(p)).collect();
        let alive = alive_check_c(&k);
        (fs, sts, alive)
    };
    PanicRow {
        model: spec.label(),
        variant: variant.to_string(),
        faulty_abnormal: matches!(faulty_status, ExitStatus::Panicked(_)),
        faulty_status: status_str(&faulty_status),
        others_ok: statuses.iter().filter(|s| **s == ExitStatus::Exited(0)).count(),
        others: OTHERS,
        std_mutex_poisoned: obj.std_m.is_poisoned(),
        kernel_alive: alive,
    }
}

pub fn measure() -> Vec<PanicRow> {
    // Os panics aqui são esperados: o hook padrão imprimiria dezenas de mensagens no stderr.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut rows = Vec::new();
    for spec in ModelSpec::MAIN {
        progress(format!("H09: panic em {}", spec.label()));
        for v in VARIANTS {
            rows.push(run_variant(spec, v));
        }
    }
    std::panic::set_hook(prev);
    rows
}

pub fn verdict(rows: &[PanicRow]) -> HypOut {
    let isolated = rows.iter().all(|r| r.faulty_abnormal && r.kernel_alive);
    let continue_ok = rows.iter().filter(|r| r.variant != "std_unwrap").all(|r| r.others_ok == r.others);
    let poisoned: Vec<&PanicRow> = rows.iter().filter(|r| r.variant == "std_unwrap").collect();
    let poison_ok: usize = poisoned.iter().map(|r| r.others_ok).sum();
    let poison_total: usize = poisoned.iter().map(|r| r.others).sum();
    let verdict = if isolated && continue_ok { Verdict::Confirmed } else { Verdict::Refuted };
    HypOut {
        id: "H09",
        verdict,
        summary: format!(
            "Nos 4 modelos o panic (dentro e fora de lock) vira término anormal pro pai e o kernel continua \
             atendendo (pipeline de verificação passou em {}/{} casos). Com parking_lot, com std::Mutex tratando o \
             poison (into_inner) e com panic fora de lock, {}/{} processos seguintes terminaram normalmente. Com o \
             idioma lock().unwrap() num std::Mutex envenenado, só {poison_ok}/{poison_total}: é o envenenamento que o \
             kernel precisa evitar (usar parking_lot ou tratar PoisonError).",
            rows.iter().filter(|r| r.kernel_alive).count(),
            rows.len(),
            rows.iter().filter(|r| r.variant != "std_unwrap").map(|r| r.others_ok).sum::<usize>(),
            rows.iter().filter(|r| r.variant != "std_unwrap").map(|r| r.others).sum::<usize>(),
        ),
        evidence: json!({ "rows": rows }),
    }
}
