//! Cargas usadas pra medir o overhead de cada allocator contra o System.
//!
//! - `sort_owned`: o `sort` ingênuo de um builtin: quebra a entrada em linhas próprias (`Vec<u8>` por
//!   linha, 1M alocações), ordena (o sort estável aloca um buffer auxiliar) e monta a saída num `Vec`
//!   que cresce por realocação. É a carga do critério da H16.
//! - `sort_borrowed`: o mesmo sort com linhas emprestadas do buffer de entrada, como o uutils faz
//!   (poucas alocações grandes). Mostra o outro extremo.
//! - `small_ring`: muitas alocações pequenas (8 a 512 bytes) com 4096 vivas, padrão de interpretador.
//! - `small_ring_mt`: a mesma carga em N threads, cada uma num pseudo-processo próprio.

use std::sync::Barrier;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::accounting::Accounting;
use crate::rng::{SplitMix64, fnv1a};

const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// Entrada do sort: `lines` linhas aleatórias de 1 a 64 caracteres `[a-z0-9]`, terminadas em `\n`.
pub fn gen_lines(lines: usize, seed: u64) -> Vec<u8> {
    let mut rng = SplitMix64::new(seed);
    let mut out = Vec::with_capacity(lines * 34);
    for _ in 0..lines {
        let len = 1 + rng.below(64) as usize;
        for _ in 0..len {
            out.push(ALPHABET[rng.below(ALPHABET.len() as u64) as usize]);
        }
        out.push(b'\n');
    }
    out
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Timed {
    /// Tempo de parede da região medida.
    pub elapsed_ns: u64,
    /// Tempo de CPU da thread na mesma região (sem a espera na fila do escalonador).
    pub cpu_ns: u64,
    pub checksum: u64,
    pub items: u64,
}

fn ns(d: Duration) -> u64 {
    d.as_nanos() as u64
}

fn cpu() -> u64 {
    crate::sys::thread_cpu_ns().unwrap_or(0)
}

fn body_of(input: &[u8]) -> &[u8] {
    input.strip_suffix(b"\n").unwrap_or(input)
}

/// Sort com uma alocação por linha. O tempo inclui montar as linhas, ordenar, escrever a saída e
/// liberar tudo; o hash da saída fica fora da conta.
pub fn sort_owned(input: &[u8]) -> Timed {
    let c0 = cpu();
    let t0 = Instant::now();
    let mut lines: Vec<Vec<u8>> = body_of(input).split(|&b| b == b'\n').map(<[u8]>::to_vec).collect();
    lines.sort();
    let mut out: Vec<u8> = Vec::new();
    for l in &lines {
        out.extend_from_slice(l);
        out.push(b'\n');
    }
    let build = t0.elapsed();
    let c1 = cpu();
    let checksum = fnv1a(&out);
    let items = lines.len() as u64;
    let c2 = cpu();
    let t1 = Instant::now();
    drop(lines);
    drop(out);
    let free = t1.elapsed();
    let c3 = cpu();
    Timed { elapsed_ns: ns(build + free), cpu_ns: (c1 - c0) + (c3 - c2), checksum, items }
}

/// Sort com linhas emprestadas da entrada.
pub fn sort_borrowed(input: &[u8]) -> Timed {
    let c0 = cpu();
    let t0 = Instant::now();
    let mut lines: Vec<&[u8]> = body_of(input).split(|&b| b == b'\n').collect();
    lines.sort();
    let mut out: Vec<u8> = Vec::new();
    for l in &lines {
        out.extend_from_slice(l);
        out.push(b'\n');
    }
    let build = t0.elapsed();
    let c1 = cpu();
    let checksum = fnv1a(&out);
    let items = lines.len() as u64;
    let c2 = cpu();
    let t1 = Instant::now();
    drop(lines);
    drop(out);
    let free = t1.elapsed();
    let c3 = cpu();
    Timed { elapsed_ns: ns(build + free), cpu_ns: (c1 - c0) + (c3 - c2), checksum, items }
}

/// Anel de 4096 buffers: cada operação libera um buffer aleatório e aloca outro de 8 a 512 bytes.
pub fn small_ring(ops: u64, seed: u64) -> Timed {
    const RING: usize = 4096;
    let mut rng = SplitMix64::new(seed);
    let mut ring: Vec<Vec<u8>> = Vec::with_capacity(RING);
    ring.resize_with(RING, Vec::new);
    let c0 = cpu();
    let t0 = Instant::now();
    for i in 0..ops {
        let idx = rng.below(RING as u64) as usize;
        let size = 8 + rng.below(505) as usize;
        let mut v = Vec::with_capacity(size);
        v.push(i as u8);
        ring[idx] = v;
    }
    let mut checksum = 0u64;
    for v in &ring {
        checksum = checksum.wrapping_mul(31).wrapping_add(v.first().copied().map_or(0, u64::from));
    }
    drop(ring);
    let elapsed_ns = ns(t0.elapsed());
    Timed { elapsed_ns, cpu_ns: cpu() - c0, checksum, items: ops }
}

#[derive(Clone, Debug, Serialize)]
pub struct MtTimed {
    /// Tempo de parede: da largada comum até a última thread terminar.
    pub wall_ns: u64,
    /// Soma dos tempos de parede de cada thread.
    pub thread_wall_ns_sum: u64,
    /// Soma dos tempos de CPU de cada thread (inclui o tempo gasto disputando linhas de cache).
    pub cpu_ns_sum: u64,
    pub threads: u32,
    pub ops_per_thread: u64,
    pub checksum: u64,
}

/// `small_ring` em `threads` pseudo-processos simultâneos.
pub fn small_ring_mt<A: Accounting>(acct: &A, threads: u32, ops: u64, seed: u64) -> MtTimed {
    let barrier = Barrier::new(threads as usize + 1);
    let results: Vec<Timed> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let barrier = &barrier;
                s.spawn(move || {
                    acct.run_process(None, |_| {
                        barrier.wait();
                        small_ring(ops, seed.wrapping_add(u64::from(t)))
                    })
                })
            })
            .collect();
        barrier.wait();
        handles.into_iter().map(|h| h.join().expect("thread de carga")).collect()
    });
    MtTimed {
        wall_ns: results.iter().map(|r| r.elapsed_ns).max().unwrap_or(0),
        thread_wall_ns_sum: results.iter().map(|r| r.elapsed_ns).sum(),
        cpu_ns_sum: results.iter().map(|r| r.cpu_ns).sum(),
        threads,
        ops_per_thread: ops,
        checksum: results.iter().fold(0u64, |a, r| a.wrapping_mul(31).wrapping_add(r.checksum)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_input_has_the_requested_lines() {
        let input = gen_lines(1000, 1);
        assert_eq!(input.iter().filter(|&&b| b == b'\n').count(), 1000);
        assert!(input.iter().all(|b| *b == b'\n' || ALPHABET.contains(b)));
        assert_eq!(input, gen_lines(1000, 1));
    }

    #[test]
    fn both_sorts_produce_the_same_sorted_output() {
        let input = gen_lines(5000, 3);
        let a = sort_owned(&input);
        let b = sort_borrowed(&input);
        assert_eq!(a.checksum, b.checksum);
        assert_eq!(a.items, 5000);
        let mut lines: Vec<&[u8]> = body_of(&input).split(|&b| b == b'\n').collect();
        lines.sort();
        let mut out = Vec::new();
        for l in lines {
            out.extend_from_slice(l);
            out.push(b'\n');
        }
        assert_eq!(fnv1a(&out), a.checksum);
    }

    #[test]
    fn small_ring_is_deterministic() {
        assert_eq!(small_ring(10_000, 5).checksum, small_ring(10_000, 5).checksum);
    }
}
