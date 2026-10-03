//! Utilitários de medição de tempo.

use std::hint::black_box;
use std::time::Instant;

use serde::Serialize;

/// Resumo de uma amostra de tempos em nanossegundos.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Summary {
    pub n: usize,
    pub median_ns: f64,
    pub p10_ns: f64,
    pub p90_ns: f64,
    pub mean_ns: f64,
}

pub fn summarize(samples: &mut [u64]) -> Summary {
    if samples.is_empty() {
        return Summary::default();
    }
    samples.sort_unstable();
    let pick = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize] as f64;
    let mean = samples.iter().map(|&s| s as f64).sum::<f64>() / samples.len() as f64;
    Summary { n: samples.len(), median_ns: pick(0.5), p10_ns: pick(0.1), p90_ns: pick(0.9), mean_ns: mean }
}

/// Tempo de `f` em nanossegundos.
pub fn time_ns<R>(f: impl FnOnce() -> R) -> (u64, R) {
    let t = Instant::now();
    let r = black_box(f());
    (t.elapsed().as_nanos() as u64, r)
}

/// Tempo médio por iteração de `f`, em lotes, repetido `rounds` vezes; devolve o resumo dos lotes
/// (cada amostra é ns por iteração do lote). Bom pra operações de dezenas de ns.
pub fn per_iter(rounds: usize, iters: usize, mut f: impl FnMut()) -> Summary {
    let mut samples = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let t = Instant::now();
        for _ in 0..iters {
            f();
        }
        samples.push((t.elapsed().as_nanos() / iters as u128) as u64);
    }
    summarize(&mut samples)
}
