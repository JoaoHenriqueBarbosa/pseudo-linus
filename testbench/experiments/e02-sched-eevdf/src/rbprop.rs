//! Teste de propriedade da `rbtree` (H12).
//!
//! Cada caso é uma sequência de até 511 operações (inserir, remover o n-ésimo, remover o primeiro,
//! trocar valor, navegar) numa árvore com augmentação de tamanho, mínimo e máximo. Depois de **cada**
//! operação:
//!
//! 1. `check_invariants` (raiz preta, sem vermelho-vermelho, altura preta igual, ligações, ordem,
//!    resumo de cada nó igual ao recalculado dos filhos, cache do mais à esquerda, lista de livres);
//! 2. a ordem simétrica (chave, valor, handle) é igual à de um `BTreeMap<(chave, seq), _>`, que modela
//!    chaves repetidas na ordem de inserção;
//! 3. o resumo de cada nó é igual ao agregado calculado por força bruta a partir dos valores crus da
//!    subárvore (sem usar resumo guardado);
//! 4. `first` é o primeiro do modelo e `find` acha o primeiro igual.
//!
//! As chaves vêm de faixas pequena (3), média (40) e grande (65535), pra forçar muitas repetições
//! num caso e árvores com chaves distintas no outro.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::time::Instant;

use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestError, TestRng, TestRunner};
use rbtree::{Augment, NodeId, RbTree};
use serde::Serialize;

/// Operação de um caso.
#[derive(Clone, Debug)]
pub enum Op {
    Insert { key: u16, value: i32 },
    RemoveNth(usize),
    PopFirst,
    UpdateNth(usize, i32),
    Navigate(u16),
}

/// Augmentação de teste: tamanho, menor e maior valor da subárvore.
#[derive(Clone, Copy, Debug)]
pub struct SizeMinMax;

/// Resumo de [`SizeMinMax`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Agg {
    pub size: u32,
    pub min: i32,
    pub max: i32,
}

impl Augment<u16, i32> for SizeMinMax {
    type Summary = Agg;

    fn summarize(_key: &u16, value: &i32, left: Option<&Agg>, right: Option<&Agg>) -> Agg {
        let mut a = Agg { size: 1, min: *value, max: *value };
        for c in [left, right].into_iter().flatten() {
            a.size += c.size;
            a.min = a.min.min(c.min);
            a.max = a.max.max(c.max);
        }
        a
    }
}

type Tree = RbTree<u16, i32, SizeMinMax>;
type Model = BTreeMap<(u16, u64), (i32, NodeId)>;

fn op_strategy(key_max: u16, insert_weight: u32) -> impl Strategy<Value = Op> {
    prop_oneof![
        insert_weight => (0..=key_max, any::<i32>()).prop_map(|(key, value)| Op::Insert { key, value }),
        3 => any::<usize>().prop_map(Op::RemoveNth),
        1 => Just(Op::PopFirst),
        2 => (any::<usize>(), any::<i32>()).prop_map(|(i, v)| Op::UpdateNth(i, v)),
        1 => (0..=key_max).prop_map(Op::Navigate),
    ]
}

/// Estratégia de um caso. Nove em dez casos escolhem a faixa de chaves e geram de 1 a 255 operações
/// equilibradas (a árvore sobe e desce perto de dezenas de nós, com muitas remoções e
/// rebalanceamentos); um em dez gera de 256 a 511 operações com inserção dominante, pra chegar a árvores
/// de centenas de nós e alturas maiores.
pub fn case_strategy() -> impl Strategy<Value = Vec<Op>> {
    let balanced = prop_oneof![Just(3u16), Just(40u16), Just(u16::MAX)]
        .prop_flat_map(|key_max| proptest::collection::vec(op_strategy(key_max, 6), 1..256));
    let growing =
        prop_oneof![Just(40u16), Just(u16::MAX)].prop_flat_map(|key_max| proptest::collection::vec(op_strategy(key_max, 24), 256..512));
    prop_oneof![9 => balanced, 1 => growing]
}

/// Agregado da subárvore calculado só com os valores crus, comparado com o resumo de cada nó.
fn brute_force_check(tree: &Tree, node: NodeId) -> Result<Agg, String> {
    let mut a = Agg { size: 1, min: *tree.value(node), max: *tree.value(node) };
    for child in [tree.left(node), tree.right(node)].into_iter().flatten() {
        let c = brute_force_check(tree, child)?;
        a.size += c.size;
        a.min = a.min.min(c.min);
        a.max = a.max.max(c.max);
    }
    if *tree.summary(node) != a {
        return Err(format!("resumo do nó {node:?} é {:?}, força bruta dá {a:?}", tree.summary(node)));
    }
    Ok(a)
}

fn verify(tree: &Tree, model: &Model) -> Result<(), String> {
    tree.check_invariants().map_err(|e| format!("invariante: {e}"))?;
    if tree.len() != model.len() {
        return Err(format!("len {} contra {} no modelo", tree.len(), model.len()));
    }
    for ((id, k, v), ((mk, _), (mv, mid))) in tree.iter().zip(model.iter()) {
        if *k != *mk || *v != *mv || id != *mid {
            return Err(format!("ordem simétrica diverge: árvore ({k}, {v}, {id:?}), modelo ({mk}, {mv}, {mid:?})"));
        }
    }
    if tree.first() != model.values().next().map(|v| v.1) {
        return Err("first diverge do modelo".to_string());
    }
    if let Some(root) = tree.root() {
        brute_force_check(tree, root)?;
    }
    Ok(())
}

/// Roda uma sequência com todas as checagens. Devolve o maior tamanho atingido.
pub fn check_sequence(ops: &[Op]) -> Result<usize, String> {
    let mut tree: Tree = RbTree::new();
    let mut model: Model = BTreeMap::new();
    let mut seq = 0u64;
    let mut max_len = 0;
    for op in ops {
        match *op {
            Op::Insert { key, value } => {
                let id = tree.insert(key, value);
                model.insert((key, seq), (value, id));
                seq += 1;
            }
            Op::RemoveNth(i) if !model.is_empty() => {
                let k = *model.keys().nth(i % model.len()).expect("índice válido");
                let (v, id) = model.remove(&k).expect("existe");
                let got = tree.remove(id);
                if got != (k.0, v) {
                    return Err(format!("remove devolveu {got:?}, esperado {:?}", (k.0, v)));
                }
            }
            Op::PopFirst if !model.is_empty() => {
                let (k, (v, id)) = model.pop_first().expect("não vazio");
                if tree.first() != Some(id) {
                    return Err("first não é o menor".to_string());
                }
                let got = tree.remove(id);
                if got != (k.0, v) {
                    return Err(format!("remove do primeiro devolveu {got:?}"));
                }
            }
            Op::UpdateNth(i, nv) if !model.is_empty() => {
                let k = *model.keys().nth(i % model.len()).expect("índice válido");
                let entry = model.get_mut(&k).expect("existe");
                entry.0 = nv;
                tree.update_value(entry.1, |v| *v = nv);
            }
            Op::Navigate(probe) => {
                let expected_lb = model.range((probe, 0)..).next().map(|(_, v)| v.1);
                if tree.lower_bound(&probe) != expected_lb {
                    return Err(format!("lower_bound({probe}) diverge"));
                }
                let expected_find = model.range((probe, 0)..(probe.saturating_add(1), 0)).next().map(|(_, v)| v.1);
                if probe != u16::MAX && tree.find(&probe) != expected_find {
                    return Err(format!("find({probe}) diverge"));
                }
                let ids: Vec<NodeId> = model.values().map(|v| v.1).collect();
                for w in ids.windows(2) {
                    if tree.next(w[0]) != Some(w[1]) || tree.prev(w[1]) != Some(w[0]) {
                        return Err("next/prev divergem do modelo".to_string());
                    }
                }
                if tree.last() != ids.last().copied() {
                    return Err("last diverge".to_string());
                }
            }
            _ => {}
        }
        max_len = max_len.max(tree.len());
        verify(&tree, &model)?;
    }
    Ok(max_len)
}

/// Resultado do teste de propriedade.
#[derive(Clone, Debug, Serialize)]
pub struct PropReport {
    pub cases: u32,
    pub passed: bool,
    pub failure: Option<String>,
    pub operations: u64,
    pub max_len: usize,
    pub elapsed_s: f64,
}

/// Roda `cases` sequências com semente fixa (resultado reproduzível) e sem gravar arquivos de
/// regressão.
pub fn run(cases: u32) -> PropReport {
    let config = Config { cases, failure_persistence: None, ..Config::default() };
    let rng = TestRng::deterministic_rng(RngAlgorithm::ChaCha);
    let mut runner = TestRunner::new_with_rng(config, rng);
    let ops = Cell::new(0u64);
    let max_len = Cell::new(0usize);
    let t0 = Instant::now();
    let result = runner.run(&case_strategy(), |seq| {
        ops.set(ops.get() + seq.len() as u64);
        match check_sequence(&seq) {
            Ok(m) => {
                max_len.set(max_len.get().max(m));
                Ok(())
            }
            Err(e) => Err(TestCaseError::fail(e)),
        }
    });
    let failure = match result {
        Ok(()) => None,
        Err(TestError::Fail(reason, input)) => Some(format!("{reason}; entrada mínima: {input:?}")),
        Err(TestError::Abort(reason)) => Some(format!("abortado: {reason}")),
    };
    PropReport {
        cases,
        passed: failure.is_none(),
        failure,
        operations: ops.get(),
        max_len: max_len.get(),
        elapsed_s: t0.elapsed().as_secs_f64(),
    }
}
