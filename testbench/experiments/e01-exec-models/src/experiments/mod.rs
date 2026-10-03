//! Medições das hipóteses H01 a H10. Cada submódulo devolve um [`HypOut`] com veredito, resumo com o
//! número que decidiu, e a evidência completa (que vai pro JSON).

pub mod h01;
pub mod h02;
pub mod h03_h04;
pub mod h05;
pub mod h06;
pub mod h07;
pub mod h08;
pub mod h09;
pub mod h10;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use harness::Verdict;
use serde_json::Value;

use crate::kernel::{ExitStatus, File, new_pipe};
use crate::model_c::{BoxFut, CtxC};
use crate::stats::process_cpu_ns;
use crate::sys::Sys;
use crate::{ModelKind, ModelSpec};

pub struct HypOut {
    pub id: &'static str,
    pub verdict: Verdict,
    pub summary: String,
    pub evidence: Value,
}

/// Estágio de pipeline síncrono.
pub type SyncStage = Box<dyn FnOnce(&dyn Sys) -> i32 + Send + 'static>;
/// Estágio de pipeline assíncrono.
pub type AsyncStage = Box<dyn FnOnce(CtxC) -> BoxFut + Send + 'static>;

/// Resultado de um pipeline: tempo de parede, status de cada estágio e bytes que chegaram no escoadouro
/// do último estágio.
pub struct PipelineRun {
    pub elapsed: Duration,
    /// CPU do processo host (todas as threads) gasta durante o pipeline.
    pub cpu: Duration,
    pub statuses: Vec<ExitStatus>,
    pub sink_bytes: u64,
}

/// Monta a tabela de descritores de cada estágio: stdin do pipe anterior, stdout pro próximo; o último
/// escreve num escoadouro, cujo contador é devolvido.
fn pipeline_files(n: usize) -> (Vec<Vec<(usize, File)>>, Arc<AtomicU64>) {
    let sink = Arc::new(AtomicU64::new(0));
    let mut files: Vec<Vec<(usize, File)>> = (0..n).map(|_| Vec::new()).collect();
    let mut prev_read: Option<File> = None;
    for (i, stage_files) in files.iter_mut().enumerate() {
        if let Some(r) = prev_read.take() {
            stage_files.push((0, r));
        }
        if i + 1 < n {
            let (r, w) = new_pipe();
            stage_files.push((1, w));
            prev_read = Some(r);
        } else {
            stage_files.push((1, File::Sink(sink.clone())));
        }
    }
    (files, sink)
}

/// Roda um pipeline num kernel síncrono (timer de fatia ligado).
pub fn run_pipeline_sync(spec: ModelSpec, ncpus: usize, stages: Vec<SyncStage>) -> PipelineRun {
    let k = spec.sync_kernel(ncpus, true).expect("modelo síncrono");
    let (files, sink) = pipeline_files(stages.len());
    let cpu0 = process_cpu_ns();
    let t0 = Instant::now();
    let pids: Vec<_> = stages.into_iter().zip(files).map(|(main, f)| k.spawn(f, main)).collect();
    let statuses: Vec<_> = pids.iter().map(|&p| k.wait(p)).collect();
    let elapsed = t0.elapsed();
    let cpu = Duration::from_nanos(process_cpu_ns() - cpu0);
    PipelineRun { elapsed, cpu, statuses, sink_bytes: sink.load(Ordering::Relaxed) }
}

/// Roda um pipeline no modelo C (timer de fatia ligado).
pub fn run_pipeline_c(ncpus: usize, stages: Vec<AsyncStage>) -> PipelineRun {
    let k = ModelSpec::C.c_kernel(ncpus, true);
    let (files, sink) = pipeline_files(stages.len());
    let cpu0 = process_cpu_ns();
    let t0 = Instant::now();
    let pids: Vec<_> = stages.into_iter().zip(files).map(|(main, f)| k.spawn(f, main)).collect();
    let statuses: Vec<_> = pids.iter().map(|&p| k.wait(p)).collect();
    let elapsed = t0.elapsed();
    let cpu = Duration::from_nanos(process_cpu_ns() - cpu0);
    PipelineRun { elapsed, cpu, statuses, sink_bytes: sink.load(Ordering::Relaxed) }
}

pub fn is_sync(spec: ModelSpec) -> bool {
    spec.kind != ModelKind::C
}

/// Status em texto curto pro JSON.
pub fn status_str(s: &ExitStatus) -> String {
    match s {
        ExitStatus::Exited(c) => format!("exit {c}"),
        ExitStatus::Signaled(sig) => format!("signal {sig}"),
        ExitStatus::Panicked(m) => format!("panic: {m}"),
    }
}

/// Progresso no stderr (o stdout fica livre pra subcomandos que devolvem JSON).
pub fn progress(msg: impl AsRef<str>) {
    eprintln!("[e01] {}", msg.as_ref());
}
