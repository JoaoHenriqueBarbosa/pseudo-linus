//! Desempenho da `rbtree` contra `BTreeMap` (H12).
//!
//! Pra cada tamanho n, com chaves `u64` aleatórias:
//!
//! - **insert**: monta a estrutura do zero com n inserções;
//! - **remove**: tira os n elementos em ordem aleatória. A `rbtree` remove por handle (é a API que o
//!   escalonador usa, como o `rb_erase` do kernel) e também por chave (`find` + `remove`), pra comparar
//!   com o `BTreeMap::remove` em pé de igualdade. As duas estruturas são montadas por inserções
//!   (o `collect` do `BTreeMap` constrói em lote, com nós compactos, e favoreceria ele);
//! - **leftmost**: consulta o menor elemento (`first`, cache O(1), contra `first_key_value`);
//! - **churn**: o padrão do escalonador, com n elementos fixos: tira o menor e insere de volta com
//!   chave maior (`pop_first` + `insert` no `BTreeMap`).
//!
//! A `rbtree` roda sem augmentação e com a augmentação real do EEVDF (`min_vruntime`/`min_slice`, chave
//! `VDeadline`, valor `QueuedEntity`), pra medir o custo de manter o resumo.
//!
//! Cada repetição mede todas as variantes em seguida (intercaladas), e o resultado de cada variante é o
//! **mínimo** de 7 repetições, em ns por operação: com outros processos disputando CPU e cache, o mínimo
//! é a repetição menos perturbada, e intercalar evita que uma mudança de carga no meio da medição caia
//! só numa das variantes.

use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Instant;

use rbtree::{NodeId, RbTree};
use sched::EntityId;
use sched::timeline::{EevdfAugment, QueuedEntity, VDeadline};
use serde::Serialize;

use crate::rng::SplitMix;

const REPS: usize = 7;

/// Medidas de um tamanho, em ns por operação.
#[derive(Clone, Debug, Serialize)]
pub struct OpBench {
    pub n: usize,
    pub insert_rb: f64,
    pub insert_rb_aug: f64,
    pub insert_bt: f64,
    pub remove_rb_handle: f64,
    pub remove_rb_key: f64,
    pub remove_rb_aug: f64,
    pub remove_bt: f64,
    pub leftmost_rb: f64,
    pub leftmost_bt: f64,
    pub churn_rb: f64,
    pub churn_rb_aug: f64,
    pub churn_bt: f64,
}

impl OpBench {
    /// Razões `rbtree / BTreeMap` (abaixo de 1, a árvore é mais rápida). Remoção usa a versão por chave,
    /// que é a comparação justa.
    pub fn ratios(&self) -> Ratios {
        Ratios {
            n: self.n,
            insert: self.insert_rb / self.insert_bt,
            insert_aug: self.insert_rb_aug / self.insert_bt,
            remove_key: self.remove_rb_key / self.remove_bt,
            remove_handle: self.remove_rb_handle / self.remove_bt,
            remove_aug: self.remove_rb_aug / self.remove_bt,
            leftmost: self.leftmost_rb / self.leftmost_bt,
            churn: self.churn_rb / self.churn_bt,
            churn_aug: self.churn_rb_aug / self.churn_bt,
        }
    }
}

/// Razões de tempo `rbtree / BTreeMap`.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Ratios {
    pub n: usize,
    pub insert: f64,
    pub insert_aug: f64,
    pub remove_key: f64,
    pub remove_handle: f64,
    pub remove_aug: f64,
    pub leftmost: f64,
    pub churn: f64,
    pub churn_aug: f64,
}

fn keys(n: usize, seed: u64) -> Vec<u64> {
    let mut rng = SplitMix::new(seed);
    (0..n).map(|_| rng.next_u64() >> 8).collect()
}

fn shuffled(n: usize, seed: u64) -> Vec<usize> {
    let mut rng = SplitMix::new(seed);
    let mut idx: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        idx.swap(i, j);
    }
    idx
}

fn entity(i: usize, k: u64) -> QueuedEntity {
    QueuedEntity { entity: EntityId::from_index(i as u32), vruntime: k / 2, slice: 2_800_000, weight: 1 << 20 }
}

/// ns por operação de uma execução de `f`, que faz `ops` operações.
fn time_once(ops: usize, f: impl FnOnce()) -> f64 {
    let t0 = Instant::now();
    f();
    t0.elapsed().as_nanos() as f64 / ops as f64
}

fn best(samples: &[f64]) -> f64 {
    samples.iter().copied().fold(f64::INFINITY, f64::min)
}

/// Quantas vezes repetir a carga pra cada medida ficar longe da resolução do relógio.
fn rounds_for(n: usize) -> usize {
    (200_000 / n.max(1)).max(1)
}

/// Amostras de uma variante ao longo das repetições.
#[derive(Default)]
struct Samples(Vec<f64>);

impl Samples {
    fn push(&mut self, x: f64) {
        self.0.push(x);
    }

    fn best(&self) -> f64 {
        best(&self.0)
    }
}

/// Mede um tamanho.
pub fn bench_size(n: usize) -> OpBench {
    let ks = keys(n, 0xabc0 + n as u64);
    let order = shuffled(n, 0x5150 + n as u64);
    let rounds = rounds_for(n);
    let total = n * rounds;

    let mut deltas_rng = SplitMix::new(77);
    let deltas: Vec<u64> = (0..4096).map(|_| 1 + (deltas_rng.next_u64() >> 40)).collect();
    let queries = 1_000_000;
    let churn_ops = 300_000;

    // Estruturas cheias pra leftmost e churn (o churn mantém o tamanho).
    let mut rb: RbTree<u64, u32> = RbTree::with_capacity(n);
    let mut rb_aug: RbTree<VDeadline, QueuedEntity, EevdfAugment> = RbTree::with_capacity(n);
    let mut bt: BTreeMap<u64, u32> = BTreeMap::new();
    for (i, &k) in ks.iter().enumerate() {
        rb.insert(k, i as u32);
        rb_aug.insert(VDeadline(k), entity(i, k));
        bt.insert(k, i as u32);
    }

    let (mut insert_rb, mut insert_rb_aug, mut insert_bt) = (Samples::default(), Samples::default(), Samples::default());
    let (mut remove_rb_handle, mut remove_rb_key, mut remove_rb_aug, mut remove_bt) =
        (Samples::default(), Samples::default(), Samples::default(), Samples::default());
    let (mut leftmost_rb, mut leftmost_bt) = (Samples::default(), Samples::default());
    let (mut churn_rb, mut churn_rb_aug, mut churn_bt) = (Samples::default(), Samples::default(), Samples::default());

    for _ in 0..REPS {
        insert_rb.push(time_once(total, || {
            for _ in 0..rounds {
                let mut t: RbTree<u64, u32> = RbTree::with_capacity(n);
                for (i, &k) in ks.iter().enumerate() {
                    t.insert(k, i as u32);
                }
                black_box(&t);
            }
        }));
        insert_rb_aug.push(time_once(total, || {
            for _ in 0..rounds {
                let mut t: RbTree<VDeadline, QueuedEntity, EevdfAugment> = RbTree::with_capacity(n);
                for (i, &k) in ks.iter().enumerate() {
                    t.insert(VDeadline(k), entity(i, k));
                }
                black_box(&t);
            }
        }));
        insert_bt.push(time_once(total, || {
            for _ in 0..rounds {
                let mut t: BTreeMap<u64, u32> = BTreeMap::new();
                for (i, &k) in ks.iter().enumerate() {
                    t.insert(k, i as u32);
                }
                black_box(&t);
            }
        }));

        // Remoção: a montagem fica fora do tempo medido.
        let mut elapsed = [0u128; 4];
        for _ in 0..rounds {
            let mut t: RbTree<u64, u32> = RbTree::with_capacity(n);
            let ids: Vec<NodeId> = ks.iter().enumerate().map(|(i, &k)| t.insert(k, i as u32)).collect();
            let t0 = Instant::now();
            for &i in &order {
                black_box(t.remove(ids[i]));
            }
            elapsed[0] += t0.elapsed().as_nanos();

            let mut t: RbTree<u64, u32> = RbTree::with_capacity(n);
            for (i, &k) in ks.iter().enumerate() {
                t.insert(k, i as u32);
            }
            let t0 = Instant::now();
            for &i in &order {
                let id = t.find(&ks[i]).expect("chave presente");
                black_box(t.remove(id));
            }
            elapsed[1] += t0.elapsed().as_nanos();

            let mut t: RbTree<VDeadline, QueuedEntity, EevdfAugment> = RbTree::with_capacity(n);
            let ids: Vec<NodeId> = ks.iter().enumerate().map(|(i, &k)| t.insert(VDeadline(k), entity(i, k))).collect();
            let t0 = Instant::now();
            for &i in &order {
                black_box(t.remove(ids[i]));
            }
            elapsed[2] += t0.elapsed().as_nanos();

            let mut t: BTreeMap<u64, u32> = BTreeMap::new();
            for (i, &k) in ks.iter().enumerate() {
                t.insert(k, i as u32);
            }
            let t0 = Instant::now();
            for &i in &order {
                black_box(t.remove(&ks[i]));
            }
            elapsed[3] += t0.elapsed().as_nanos();
        }
        remove_rb_handle.push(elapsed[0] as f64 / total as f64);
        remove_rb_key.push(elapsed[1] as f64 / total as f64);
        remove_rb_aug.push(elapsed[2] as f64 / total as f64);
        remove_bt.push(elapsed[3] as f64 / total as f64);

        leftmost_rb.push(time_once(queries, || {
            let mut acc = 0u64;
            for _ in 0..queries {
                let id = black_box(&rb).first().expect("não vazia");
                acc = acc.wrapping_add(*rb.key(id));
            }
            black_box(acc);
        }));
        leftmost_bt.push(time_once(queries, || {
            let mut acc = 0u64;
            for _ in 0..queries {
                acc = acc.wrapping_add(*black_box(&bt).first_key_value().expect("não vazio").0);
            }
            black_box(acc);
        }));

        churn_rb.push(time_once(churn_ops, || {
            for j in 0..churn_ops {
                let id = rb.first().expect("não vazia");
                let (k, v) = rb.remove(id);
                rb.insert(k + deltas[j & 4095], v);
            }
        }));
        churn_rb_aug.push(time_once(churn_ops, || {
            for j in 0..churn_ops {
                let id = rb_aug.first().expect("não vazia");
                let (k, mut v) = rb_aug.remove(id);
                v.vruntime += deltas[j & 4095] / 2;
                rb_aug.insert(VDeadline(k.0 + deltas[j & 4095]), v);
            }
        }));
        churn_bt.push(time_once(churn_ops, || {
            for j in 0..churn_ops {
                let (k, v) = bt.pop_first().expect("não vazio");
                bt.insert(k + deltas[j & 4095], v);
            }
        }));
    }

    OpBench {
        n,
        insert_rb: insert_rb.best(),
        insert_rb_aug: insert_rb_aug.best(),
        insert_bt: insert_bt.best(),
        remove_rb_handle: remove_rb_handle.best(),
        remove_rb_key: remove_rb_key.best(),
        remove_rb_aug: remove_rb_aug.best(),
        remove_bt: remove_bt.best(),
        leftmost_rb: leftmost_rb.best(),
        leftmost_bt: leftmost_bt.best(),
        churn_rb: churn_rb.best(),
        churn_rb_aug: churn_rb_aug.best(),
        churn_bt: churn_bt.best(),
    }
}
