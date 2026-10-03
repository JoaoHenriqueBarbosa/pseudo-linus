//! Custo de contar explicitamente as estruturas do kernel (buffers de pipe, conteúdo de arquivo).
//!
//! O design manda contabilizar essas estruturas à parte, por um contador atômico por processo (e um
//! por sandbox, pra cota). Aqui um pipe e um arquivo de tmpfs mínimos rodam com e sem esse contador,
//! e, no binário do `tracking-allocator`, também com as alocações do kernel dentro do escopo que as
//! tira da conta do processo (`kernel_scope`), que é o custo extra de não contar duas vezes.

use std::collections::VecDeque;
use std::hint::black_box;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Barrier, Condvar, Mutex, RwLock};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::accounting::Accounting;

/// Contador de bytes do kernel de um processo (ou de uma sandbox), numa linha de cache própria.
#[repr(align(64))]
#[derive(Debug, Default)]
pub struct MemCounter {
    bytes: AtomicI64,
}

impl MemCounter {
    pub fn new() -> MemCounter {
        MemCounter::default()
    }

    #[inline]
    pub fn charge(&self, n: i64) {
        self.bytes.fetch_add(n, Ordering::Relaxed);
    }

    #[inline]
    pub fn uncharge(&self, n: i64) {
        self.bytes.fetch_sub(n, Ordering::Relaxed);
    }

    pub fn get(&self) -> i64 {
        self.bytes.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KMode {
    /// Sem contabilidade.
    None,
    /// Um contador atômico do processo dono.
    Process,
    /// Contador do processo e contador da sandbox (cota), dois atômicos.
    ProcessAndSandbox,
}

/// Quem paga pelas estruturas: o processo dono e a sandbox.
#[derive(Debug)]
pub struct Charger<'a> {
    pub mode: KMode,
    pub process: &'a MemCounter,
    pub sandbox: &'a MemCounter,
}

impl Charger<'_> {
    #[inline]
    pub fn charge(&self, n: i64) {
        match self.mode {
            KMode::None => {}
            KMode::Process => self.process.charge(n),
            KMode::ProcessAndSandbox => {
                self.process.charge(n);
                self.sandbox.charge(n);
            }
        }
    }

    #[inline]
    pub fn uncharge(&self, n: i64) {
        match self.mode {
            KMode::None => {}
            KMode::Process => self.process.uncharge(n),
            KMode::ProcessAndSandbox => {
                self.process.uncharge(n);
                self.sandbox.uncharge(n);
            }
        }
    }
}

pub const PAGE: usize = 4096;
/// Capacidade padrão do pipe no Linux: 16 páginas (64 KiB).
pub const PIPE_PAGES: usize = 16;

#[derive(Debug)]
struct PipeState {
    pages: VecDeque<Box<[u8]>>,
    closed: bool,
}

/// Pipe à moda do Linux: cada `write` de até uma página aloca a página na hora, o leitor copia e a
/// libera. A página é cobrada do dono no `write` e devolvida no `read`.
#[derive(Debug)]
pub struct Pipe {
    state: Mutex<PipeState>,
    readable: Condvar,
    writable: Condvar,
}

impl Default for Pipe {
    fn default() -> Pipe {
        Pipe {
            state: Mutex::new(PipeState { pages: VecDeque::with_capacity(PIPE_PAGES), closed: false }),
            readable: Condvar::new(),
            writable: Condvar::new(),
        }
    }
}

impl Pipe {
    pub fn write<A: Accounting>(&self, acct: &A, scope: bool, ch: &Charger<'_>, data: &[u8]) {
        let page: Box<[u8]> = if scope { acct.kernel_scope(|| Box::from(data)) } else { Box::from(data) };
        ch.charge(page.len() as i64);
        let mut st = self.state.lock().expect("pipe");
        while st.pages.len() >= PIPE_PAGES {
            st = self.writable.wait(st).expect("pipe");
        }
        st.pages.push_back(page);
        drop(st);
        self.readable.notify_one();
    }

    pub fn read<A: Accounting>(&self, acct: &A, scope: bool, ch: &Charger<'_>, buf: &mut [u8]) -> Option<usize> {
        let mut st = self.state.lock().expect("pipe");
        let page = loop {
            if let Some(p) = st.pages.pop_front() {
                break p;
            }
            if st.closed {
                return None;
            }
            st = self.readable.wait(st).expect("pipe");
        };
        drop(st);
        self.writable.notify_one();
        let n = page.len();
        buf[..n].copy_from_slice(&page);
        ch.uncharge(n as i64);
        if scope {
            acct.kernel_scope(|| drop(page));
        } else {
            drop(page);
        }
        Some(n)
    }

    pub fn close(&self) {
        self.state.lock().expect("pipe").closed = true;
        self.readable.notify_all();
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransferResult {
    pub elapsed_ns: u64,
    /// Tempo de CPU das threads envolvidas.
    pub cpu_ns: u64,
    pub ops: u64,
    pub bytes: u64,
    /// Os contadores voltaram a zero no fim (tudo cobrado foi devolvido).
    pub balanced: bool,
}

fn cpu() -> u64 {
    crate::sys::thread_cpu_ns().unwrap_or(0)
}

/// Escritor e leitor, cada um num pseudo-processo, passando `total` bytes em writes de 4 KiB. Mede a
/// vazão de ponta a ponta (parede), que depende do escalonador e por isso é ruidosa.
pub fn pipe_transfer<A: Accounting>(acct: &A, mode: KMode, scope: bool, total: u64) -> TransferResult {
    let process = MemCounter::new();
    let sandbox = MemCounter::new();
    let ch = Charger { mode, process: &process, sandbox: &sandbox };
    let pipe = Pipe::default();
    let writes = total / PAGE as u64;
    let bar = Barrier::new(2);
    let t0 = Instant::now();
    let (read, cpu_ns) = std::thread::scope(|s| {
        let writer = s.spawn(|| {
            acct.run_process(None, |_| {
                let data = [0x61u8; PAGE];
                bar.wait();
                let c0 = cpu();
                for _ in 0..writes {
                    pipe.write(acct, scope, &ch, &data);
                }
                pipe.close();
                cpu() - c0
            })
        });
        let reader = s.spawn(|| {
            acct.run_process(None, |_| {
                let mut buf = [0u8; PAGE];
                let mut got = 0u64;
                bar.wait();
                let c0 = cpu();
                while let Some(n) = pipe.read(acct, scope, &ch, &mut buf) {
                    got += n as u64;
                }
                black_box(&buf);
                (got, cpu() - c0)
            })
        });
        let w = writer.join().expect("escritor");
        let (got, r) = reader.join().expect("leitor");
        (got, w + r)
    });
    let elapsed_ns = t0.elapsed().as_nanos() as u64;
    assert_eq!(read, writes * PAGE as u64, "o pipe perdeu bytes");
    TransferResult { elapsed_ns, cpu_ns, ops: writes, bytes: read, balanced: process.get() == 0 && sandbox.get() == 0 }
}

/// O mesmo pipe numa thread só: cada operação escreve uma página e lê de volta (nunca bloqueia). Mede o
/// custo de CPU por operação do caminho inteiro (alocar a página, cobrar, travar, enfileirar, avisar,
/// desenfileirar, copiar, devolver a cobrança, liberar), sem depender do escalonador.
pub fn pipe_single<A: Accounting>(acct: &A, mode: KMode, scope: bool, ops: u64) -> TransferResult {
    let process = MemCounter::new();
    let sandbox = MemCounter::new();
    let ch = Charger { mode, process: &process, sandbox: &sandbox };
    let pipe = Pipe::default();
    let (elapsed_ns, cpu_ns, got) = acct.run_process(None, |_| {
        let data = [0x61u8; PAGE];
        let mut buf = [0u8; PAGE];
        let mut got = 0u64;
        let c0 = cpu();
        let t0 = Instant::now();
        for _ in 0..ops {
            pipe.write(acct, scope, &ch, &data);
            got += pipe.read(acct, scope, &ch, &mut buf).expect("página") as u64;
        }
        black_box(&buf);
        (t0.elapsed().as_nanos() as u64, cpu() - c0, got)
    });
    assert_eq!(got, ops * PAGE as u64, "o pipe perdeu bytes");
    TransferResult { elapsed_ns, cpu_ns, ops, bytes: got, balanced: process.get() == 0 && sandbox.get() == 0 }
}

/// Arquivo de tmpfs mínimo: conteúdo num `Vec` atrás de um `RwLock`.
#[derive(Debug, Default)]
pub struct MemFile {
    data: RwLock<Vec<u8>>,
}

impl MemFile {
    /// Acrescenta `chunk` e cobra o tamanho lógico acrescentado.
    pub fn append<A: Accounting>(&self, acct: &A, scope: bool, ch: &Charger<'_>, chunk: &[u8]) {
        let mut d = self.data.write().expect("arquivo");
        if scope {
            acct.kernel_scope(|| d.extend_from_slice(chunk));
        } else {
            d.extend_from_slice(chunk);
        }
        drop(d);
        ch.charge(chunk.len() as i64);
    }

    /// Trunca pra zero e devolve a cobrança.
    pub fn truncate<A: Accounting>(&self, acct: &A, scope: bool, ch: &Charger<'_>) {
        let mut d = self.data.write().expect("arquivo");
        let len = d.len() as i64;
        let old = std::mem::take(&mut *d);
        drop(d);
        if scope {
            acct.kernel_scope(|| drop(old));
        } else {
            drop(old);
        }
        ch.uncharge(len);
    }
}

/// Um processo escreve `total` bytes num arquivo em appends de 4 KiB e depois trunca.
pub fn file_append<A: Accounting>(acct: &A, mode: KMode, scope: bool, total: u64) -> TransferResult {
    let process = MemCounter::new();
    let sandbox = MemCounter::new();
    let ch = Charger { mode, process: &process, sandbox: &sandbox };
    let file = MemFile::default();
    let writes = total / PAGE as u64;
    let (elapsed_ns, cpu_ns) = acct.run_process(None, |_| {
        let chunk = [0x62u8; PAGE];
        let c0 = cpu();
        let t0 = Instant::now();
        for _ in 0..writes {
            file.append(acct, scope, &ch, &chunk);
        }
        file.truncate(acct, scope, &ch);
        (t0.elapsed().as_nanos() as u64, cpu() - c0)
    });
    TransferResult {
        elapsed_ns,
        cpu_ns,
        ops: writes,
        bytes: writes * PAGE as u64,
        balanced: process.get() == 0 && sandbox.get() == 0,
    }
}

/// ns por par cobrar+devolver, numa thread.
pub fn atomic_pair_single(iters: u64) -> f64 {
    let c = MemCounter::new();
    let t0 = Instant::now();
    for _ in 0..iters {
        black_box(&c).charge(PAGE as i64);
        black_box(&c).uncharge(PAGE as i64);
    }
    t0.elapsed().as_nanos() as f64 / iters as f64
}

/// ns por par em `threads` threads ao mesmo tempo: cada uma no próprio contador (`shared = false`) ou
/// todas no mesmo (o contador da sandbox, `shared = true`). Devolve o tempo de parede por par de uma
/// thread.
pub fn atomic_pair_threads(threads: usize, iters: u64, shared: bool) -> f64 {
    let own: Vec<MemCounter> = (0..threads).map(|_| MemCounter::new()).collect();
    let common = MemCounter::new();
    let bar = Barrier::new(threads + 1);
    let worst = std::thread::scope(|s| {
        let hs: Vec<_> = (0..threads)
            .map(|i| {
                let c = if shared { &common } else { &own[i] };
                let bar = &bar;
                s.spawn(move || {
                    bar.wait();
                    let t0 = Instant::now();
                    for _ in 0..iters {
                        black_box(c).charge(PAGE as i64);
                        black_box(c).uncharge(PAGE as i64);
                    }
                    t0.elapsed().as_nanos() as u64
                })
            })
            .collect();
        bar.wait();
        hs.into_iter().map(|h| h.join().expect("thread")).max().unwrap_or(0)
    });
    worst as f64 / iters as f64
}

/// Contador da sandbox atualizado em lotes: cada thread cobra o próprio contador exato e acumula a parte
/// da sandbox localmente, tocando a linha de cache compartilhada só quando o saldo local passa de
/// `batch` bytes (a cota da sandbox fica atrasada em até `threads x batch`). Cada thread faz blocos de
/// 256 cobranças seguidos de 256 devoluções; devolve ns de parede por par, da thread mais lenta.
pub fn atomic_pair_threads_batched(threads: usize, iters: u64, batch: i64) -> f64 {
    const BLOCK: u64 = 256;
    let common = MemCounter::new();
    let bar = Barrier::new(threads + 1);
    let worst = std::thread::scope(|s| {
        let hs: Vec<_> = (0..threads)
            .map(|_| {
                let (bar, common) = (&bar, &common);
                s.spawn(move || {
                    let own = MemCounter::new();
                    let mut local: i64 = 0;
                    bar.wait();
                    let t0 = Instant::now();
                    for _ in 0..iters / BLOCK {
                        for _ in 0..BLOCK {
                            black_box(&own).charge(PAGE as i64);
                            local += PAGE as i64;
                            if local >= batch {
                                black_box(common).charge(local);
                                local = 0;
                            }
                        }
                        for _ in 0..BLOCK {
                            black_box(&own).uncharge(PAGE as i64);
                            local -= PAGE as i64;
                            if local <= -batch {
                                black_box(common).uncharge(-local);
                                local = 0;
                            }
                        }
                    }
                    common.charge(local);
                    t0.elapsed().as_nanos() as u64
                })
            })
            .collect();
        bar.wait();
        hs.into_iter().map(|h| h.join().expect("thread")).max().unwrap_or(0)
    });
    assert_eq!(common.get(), 0, "o contador em lotes não fechou em zero");
    worst as f64 / iters as f64
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VariantRow {
    pub mode: KMode,
    /// Alocações do kernel dentro do `kernel_scope` do candidato.
    pub kernel_scope: bool,
    /// Mediana da métrica (CPU ou parede, conforme a tabela) por operação, em ns.
    pub ns_per_op: f64,
    pub runs_ns_per_op: Vec<f64>,
    /// Diferença da mediana contra a variante sem contabilidade e sem escopo, em %.
    pub overhead_pct: f64,
    /// Diferença em ns por operação contra a mesma variante de base.
    pub delta_ns_per_op: f64,
    /// O mesmo pelos mínimos (o custo intrínseco, menos sensível aos vizinhos).
    pub min_ns_per_op: f64,
    pub overhead_min_pct: f64,
    pub all_balanced: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KernelSuite {
    pub reps: u32,
    /// Pipe numa thread (escreve e lê de volta), CPU por operação de 4 KiB.
    pub pipe_single_ops: u64,
    pub pipe_single_cpu: Vec<VariantRow>,
    /// Pipe entre dois pseudo-processos, parede por operação (ruidoso: depende do escalonador).
    pub pipe_two_threads_bytes: u64,
    pub pipe_two_threads_wall: Vec<VariantRow>,
    /// Arquivo de tmpfs com appends de 4 KiB, CPU por append.
    pub file_bytes: u64,
    pub file_cpu: Vec<VariantRow>,
    pub atomic_pair_ns_single: f64,
    pub atomic_pair_ns_16_own: f64,
    pub atomic_pair_ns_16_shared: f64,
    pub atomic_pair_ns_16_shared_batched_64k: f64,
}

fn median_f(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    s[s.len() / 2]
}

fn min_f(v: &[f64]) -> f64 {
    v.iter().copied().fold(f64::INFINITY, f64::min)
}

fn rows(variants: &[(KMode, bool)], runs: &[Vec<f64>], balanced: &[bool]) -> Vec<VariantRow> {
    let base = median_f(&runs[0]);
    let base_min = min_f(&runs[0]);
    variants
        .iter()
        .zip(runs)
        .zip(balanced)
        .map(|((&(mode, scope), r), &b)| {
            let m = median_f(r);
            let mn = min_f(r);
            VariantRow {
                mode,
                kernel_scope: scope,
                ns_per_op: m,
                runs_ns_per_op: r.clone(),
                overhead_pct: (m - base) / base * 100.0,
                delta_ns_per_op: m - base,
                min_ns_per_op: mn,
                overhead_min_pct: (mn - base_min) / base_min * 100.0,
                all_balanced: b,
            }
        })
        .collect()
}

/// Mede todas as variantes intercaladas (`reps` rodadas, a ordem gira a cada rodada).
pub fn suite<A: Accounting>(acct: &A, reps: u32, pipe_total: u64, file_total: u64) -> KernelSuite {
    let mut variants = vec![(KMode::None, false), (KMode::Process, false), (KMode::ProcessAndSandbox, false)];
    if acct.caps().kernel_scope {
        variants.push((KMode::ProcessAndSandbox, true));
    }
    let n = variants.len();
    let single_ops = pipe_total / PAGE as u64;
    let mut single = vec![Vec::new(); n];
    let mut two = vec![Vec::new(); n];
    let mut file = vec![Vec::new(); n];
    let mut single_bal = vec![true; n];
    let mut two_bal = vec![true; n];
    let mut file_bal = vec![true; n];
    for rep in 0..reps as usize {
        for k in 0..n {
            let i = (k + rep) % n;
            let (mode, scope) = variants[i];
            let p = pipe_single(acct, mode, scope, single_ops);
            single[i].push(p.cpu_ns as f64 / p.ops as f64);
            single_bal[i] &= p.balanced;
            let p = pipe_transfer(acct, mode, scope, pipe_total / 4);
            two[i].push(p.elapsed_ns as f64 / p.ops as f64);
            two_bal[i] &= p.balanced;
            let f = file_append(acct, mode, scope, file_total);
            file[i].push(f.cpu_ns as f64 / f.ops as f64);
            file_bal[i] &= f.balanced;
        }
    }
    KernelSuite {
        reps,
        pipe_single_ops: single_ops,
        pipe_single_cpu: rows(&variants, &single, &single_bal),
        pipe_two_threads_bytes: pipe_total / 4,
        pipe_two_threads_wall: rows(&variants, &two, &two_bal),
        file_bytes: file_total,
        file_cpu: rows(&variants, &file, &file_bal),
        atomic_pair_ns_single: atomic_pair_single(20_000_000),
        atomic_pair_ns_16_own: atomic_pair_threads(16, 2_000_000, false),
        atomic_pair_ns_16_shared: atomic_pair_threads(16, 2_000_000, true),
        atomic_pair_ns_16_shared_batched_64k: atomic_pair_threads_batched(16, 2_000_000, 64 << 10),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::{Capabilities, Pid};

    struct Plain;
    impl Accounting for Plain {
        fn name(&self) -> &'static str {
            "plain"
        }
        fn caps(&self) -> Capabilities {
            Capabilities::default()
        }
        fn run_process<R>(&self, _limit: Option<i64>, body: impl FnOnce(Pid) -> R) -> R {
            body(Pid(0))
        }
    }

    #[test]
    fn pipe_moves_every_byte_and_balances() {
        for mode in [KMode::None, KMode::Process, KMode::ProcessAndSandbox] {
            let r = pipe_transfer(&Plain, mode, false, 4 << 20);
            assert_eq!(r.bytes, 4 << 20);
            assert!(r.balanced, "{mode:?}");
            let r = pipe_single(&Plain, mode, false, 1000);
            assert_eq!(r.bytes, 1000 * PAGE as u64);
            assert!(r.balanced, "{mode:?}");
        }
    }

    #[test]
    fn batched_sandbox_counter_closes_at_zero() {
        // A própria função confere que o contador compartilhado fecha em zero.
        assert!(atomic_pair_threads_batched(4, 10_240, 64 << 10) > 0.0);
    }

    #[test]
    fn file_charges_and_truncate_returns_everything() {
        let process = MemCounter::new();
        let sandbox = MemCounter::new();
        let ch = Charger { mode: KMode::ProcessAndSandbox, process: &process, sandbox: &sandbox };
        let f = MemFile::default();
        for _ in 0..10 {
            f.append(&Plain, false, &ch, &[1u8; PAGE]);
        }
        assert_eq!(process.get(), 10 * PAGE as i64);
        assert_eq!(sandbox.get(), 10 * PAGE as i64);
        f.truncate(&Plain, false, &ch);
        assert_eq!(process.get(), 0);
        assert_eq!(sandbox.get(), 0);
        assert!(file_append(&Plain, KMode::Process, false, 1 << 20).balanced);
    }
}
