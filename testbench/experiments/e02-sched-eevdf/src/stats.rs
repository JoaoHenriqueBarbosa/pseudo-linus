//! Estatística descritiva simples pros relatórios.

use serde::Serialize;

/// Mediana de uma amostra (média dos dois do meio quando o tamanho é par).
pub fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

/// Percentil pelo vizinho mais próximo (0 a 100).
pub fn percentile_u64(values: &[u64], p: f64) -> u64 {
    sched::percentile(values, p).unwrap_or(0)
}

/// Resumo de uma amostra: média, desvio padrão, mínimo e máximo.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Summary {
    pub n: usize,
    pub mean: f64,
    pub stdev: f64,
    pub min: f64,
    pub max: f64,
}

/// Resume uma amostra.
pub fn summarize(values: &[f64]) -> Summary {
    let n = values.len();
    if n == 0 {
        return Summary { n, mean: f64::NAN, stdev: f64::NAN, min: f64::NAN, max: f64::NAN };
    }
    let mean = values.iter().sum::<f64>() / n as f64;
    let var = if n > 1 { values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64 } else { 0.0 };
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Summary { n, mean, stdev: var.sqrt(), min, max }
}

/// Quantis de latência em µs.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct LatencyQuantiles {
    pub n: usize,
    pub p50_us: f64,
    pub p90_us: f64,
    pub p99_us: f64,
    pub max_us: f64,
    pub mean_us: f64,
}

/// Quantis de uma amostra de latências em ns.
pub fn latency_quantiles(ns: &[u64]) -> LatencyQuantiles {
    let us = |x: u64| x as f64 / 1000.0;
    let mean = if ns.is_empty() { 0.0 } else { ns.iter().sum::<u64>() as f64 / ns.len() as f64 / 1000.0 };
    LatencyQuantiles {
        n: ns.len(),
        p50_us: us(percentile_u64(ns, 50.0)),
        p90_us: us(percentile_u64(ns, 90.0)),
        p99_us: us(percentile_u64(ns, 99.0)),
        max_us: us(ns.iter().copied().max().unwrap_or(0)),
        mean_us: mean,
    }
}
