//! Testes de unidade da árvore. O teste de propriedade com 100 mil sequências fica no experimento E02
//! (`testbench/experiments/e02-sched-eevdf`); aqui vão casos dirigidos e um modelo aleatório menor,
//! sem dependências.

use std::collections::BTreeMap;

use super::*;

/// Gerador SplitMix64: determinístico e sem dependência.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Augmentação de teste: tamanho da subárvore e menor valor nela.
struct SizeMin;

#[derive(Clone, Debug, PartialEq)]
struct SizeMinSummary {
    size: usize,
    min_value: i64,
}

impl Augment<u32, i64> for SizeMin {
    type Summary = SizeMinSummary;

    fn summarize(_key: &u32, value: &i64, left: Option<&SizeMinSummary>, right: Option<&SizeMinSummary>) -> SizeMinSummary {
        let mut s = SizeMinSummary { size: 1, min_value: *value };
        for child in [left, right].into_iter().flatten() {
            s.size += child.size;
            s.min_value = s.min_value.min(child.min_value);
        }
        s
    }
}

fn assert_ok<K: Ord, V, A: Augment<K, V>>(tree: &RbTree<K, V, A>) -> InvariantReport {
    match tree.check_invariants() {
        Ok(r) => r,
        Err(e) => panic!("invariante violada: {e}"),
    }
}

#[test]
fn empty_tree_is_valid() {
    let tree: RbTree<u32, ()> = RbTree::new();
    let r = assert_ok(&tree);
    assert_eq!(r.len, 0);
    assert_eq!(tree.first(), None);
    assert_eq!(tree.root(), None);
    assert_eq!(tree.last(), None);
    assert_eq!(tree.iter().count(), 0);
}

#[test]
fn ascending_and_descending_inserts_stay_balanced() {
    for descending in [false, true] {
        let mut tree: RbTree<u32, u32> = RbTree::new();
        for i in 0..2000u32 {
            let k = if descending { 2000 - i } else { i };
            tree.insert(k, i);
            let r = assert_ok(&tree);
            // Altura de uma rubro-negra com n nós é no máximo 2*log2(n+1).
            let bound = 2.0 * ((r.len + 1) as f64).log2();
            assert!(r.height as f64 <= bound, "altura {} acima de {bound}", r.height);
        }
        let keys: Vec<u32> = tree.iter().map(|(_, k, _)| *k).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
    }
}

#[test]
fn equal_keys_keep_insertion_order() {
    let mut tree: RbTree<u32, u32> = RbTree::new();
    let mut ids = Vec::new();
    for seq in 0..50u32 {
        ids.push(tree.insert(7, seq));
        tree.insert(3, 1000 + seq);
        tree.insert(9, 2000 + seq);
        assert_ok(&tree);
    }
    let sevens: Vec<u32> = tree.iter().filter(|(_, k, _)| **k == 7).map(|(_, _, v)| *v).collect();
    assert_eq!(sevens, (0..50).collect::<Vec<_>>());
    for id in ids.iter().step_by(3) {
        tree.remove(*id);
        assert_ok(&tree);
    }
    let sevens: Vec<u32> = tree.iter().filter(|(_, k, _)| **k == 7).map(|(_, _, v)| *v).collect();
    let expected: Vec<u32> = (0..50).filter(|s| s % 3 != 0).collect();
    assert_eq!(sevens, expected);
    // find devolve o primeiro igual na ordem simétrica.
    let f = tree.find(&7).expect("existe 7");
    assert_eq!(*tree.value(f), 1);
}

#[test]
fn leftmost_cache_follows_removals() {
    let mut tree: RbTree<u32, ()> = RbTree::new();
    let ids: Vec<NodeId> = (0..100u32).map(|k| tree.insert(k, ())).collect();
    for (k, id) in ids.iter().enumerate() {
        assert_eq!(tree.first(), Some(*id));
        assert_eq!(*tree.key(tree.first().expect("não vazia")), k as u32);
        tree.remove(*id);
        assert_ok(&tree);
    }
    assert!(tree.is_empty());
}

#[test]
fn free_slots_are_reused() {
    let mut tree: RbTree<u32, u32> = RbTree::new();
    let ids: Vec<NodeId> = (0..64u32).map(|k| tree.insert(k, k)).collect();
    assert_eq!(tree.nodes.len(), 64);
    for id in &ids[..32] {
        tree.remove(*id);
    }
    for k in 100..132u32 {
        tree.insert(k, k);
    }
    assert_eq!(tree.nodes.len(), 64);
    assert_eq!(tree.len(), 64);
    assert_ok(&tree);
}

#[test]
fn update_value_propagates_summary() {
    let mut tree: RbTree<u32, i64, SizeMin> = RbTree::new();
    let ids: Vec<NodeId> = (0..300u32).map(|k| tree.insert(k, 1000 + i64::from(k))).collect();
    let root = tree.root().expect("não vazia");
    assert_eq!(tree.summary(root), &SizeMinSummary { size: 300, min_value: 1000 });
    tree.update_value(ids[217], |v| *v = -5);
    assert_ok(&tree);
    assert_eq!(tree.summary(tree.root().expect("não vazia")).min_value, -5);
    tree.update_value(ids[217], |v| *v = 5000);
    assert_ok(&tree);
    assert_eq!(tree.summary(tree.root().expect("não vazia")).min_value, 1000);
}

#[test]
fn navigation_is_consistent() {
    let mut tree: RbTree<u32, ()> = RbTree::new();
    let mut rng = SplitMix(7);
    for _ in 0..500 {
        tree.insert(rng.below(200) as u32, ());
    }
    let forward: Vec<NodeId> = tree.iter().map(|(id, _, _)| id).collect();
    let mut backward = Vec::new();
    let mut cur = tree.last();
    while let Some(id) = cur {
        backward.push(id);
        cur = tree.prev(id);
    }
    backward.reverse();
    assert_eq!(forward, backward);
    for w in forward.windows(2) {
        assert_eq!(tree.next(w[0]), Some(w[1]));
        assert_eq!(tree.prev(w[1]), Some(w[0]));
    }
    for probe in 0..205u32 {
        let lb = tree.lower_bound(&probe);
        let expected = forward.iter().copied().find(|&id| *tree.key(id) >= probe);
        assert_eq!(lb, expected);
    }
    // Filhos e pais conversam.
    for &id in &forward {
        if let Some(l) = tree.left(id) {
            assert_eq!(tree.parent(l), Some(id));
        }
        if let Some(r) = tree.right(id) {
            assert_eq!(tree.parent(r), Some(id));
        }
    }
}

/// Sequências aleatórias contra um `BTreeMap<(chave, seq), valor>`, que modela a ordem simétrica com
/// empates na ordem de inserção. Invariantes checadas depois de cada operação.
#[test]
fn random_operations_match_btreemap_model() {
    let mut rng = SplitMix(0x5eed);
    for round in 0..200 {
        let mut tree: RbTree<u32, i64, SizeMin> = RbTree::new();
        let mut model: BTreeMap<(u32, u64), (i64, NodeId)> = BTreeMap::new();
        let mut seq = 0u64;
        let key_range = [4u64, 32, 1000][round % 3];
        for _ in 0..300 {
            match rng.below(10) {
                0..=4 => {
                    let k = rng.below(key_range) as u32;
                    let v = rng.below(10_000) as i64 - 5000;
                    let id = tree.insert(k, v);
                    model.insert((k, seq), (v, id));
                    seq += 1;
                }
                5..=7 if !model.is_empty() => {
                    let nth = rng.below(model.len() as u64) as usize;
                    let key = *model.keys().nth(nth).expect("índice válido");
                    let (v, id) = model.remove(&key).expect("existe");
                    assert_eq!(tree.remove(id), (key.0, v));
                }
                8 if !model.is_empty() => {
                    let key = *model.keys().next().expect("não vazio");
                    let (v, id) = model.remove(&key).expect("existe");
                    assert_eq!(tree.first(), Some(id));
                    assert_eq!(tree.remove(id), (key.0, v));
                }
                _ if !model.is_empty() => {
                    let nth = rng.below(model.len() as u64) as usize;
                    let key = *model.keys().nth(nth).expect("índice válido");
                    let nv = rng.below(10_000) as i64 - 5000;
                    let entry = model.get_mut(&key).expect("existe");
                    entry.0 = nv;
                    tree.update_value(entry.1, |v| *v = nv);
                }
                _ => {}
            }
            assert_ok(&tree);
            let got: Vec<(u32, i64)> = tree.iter().map(|(_, k, v)| (*k, *v)).collect();
            let want: Vec<(u32, i64)> = model.iter().map(|(k, v)| (k.0, v.0)).collect();
            assert_eq!(got, want);
            match tree.root() {
                None => assert!(model.is_empty()),
                Some(r) => {
                    let brute = model.values().map(|v| v.0).min().expect("não vazio");
                    assert_eq!(tree.summary(r).min_value, brute);
                    assert_eq!(tree.summary(r).size, model.len());
                }
            }
        }
    }
}

#[test]
fn removing_everything_in_random_order() {
    let mut rng = SplitMix(99);
    let mut tree: RbTree<u32, (), SizeMinUnit> = RbTree::new();
    let mut ids: Vec<NodeId> = (0..3000).map(|_| tree.insert(rng.below(500) as u32, ())).collect();
    while !ids.is_empty() {
        let i = rng.below(ids.len() as u64) as usize;
        let id = ids.swap_remove(i);
        tree.remove(id);
        if ids.len().is_multiple_of(97) {
            assert_ok(&tree);
        }
    }
    assert_ok(&tree);
    assert!(tree.is_empty());
    assert_eq!(tree.first(), None);
}

/// Augmentação de contagem, pra exercitar rotações com resumo sem valor.
struct SizeMinUnit;

impl Augment<u32, ()> for SizeMinUnit {
    type Summary = usize;

    fn summarize(_key: &u32, _value: &(), left: Option<&usize>, right: Option<&usize>) -> usize {
        1 + left.copied().unwrap_or(0) + right.copied().unwrap_or(0)
    }
}

/// O verificador acusa cada tipo de corrupção (mexendo nos campos internos de uma árvore válida).
#[test]
fn checker_detects_corruption() {
    fn build() -> (RbTree<u32, i64, SizeMin>, Vec<NodeId>) {
        let mut t: RbTree<u32, i64, SizeMin> = RbTree::new();
        let ids = (0..64u32).map(|k| t.insert(k, i64::from(k))).collect();
        (t, ids)
    }

    let (mut t, _) = build();
    let root = t.root;
    t.node_mut(root).red = true;
    assert_eq!(t.check_invariants(), Err(InvariantViolation::RootNotBlack));

    let (mut t, ids) = build();
    let red = ids.iter().map(|id| id.0).find(|&i| t.node(i).red && t.node(i).left != NIL).expect("há vermelho com filho");
    let child = t.node(red).left;
    t.node_mut(child).red = true;
    assert!(matches!(t.check_invariants(), Err(InvariantViolation::RedRed { .. } | InvariantViolation::BlackHeightMismatch { .. })));

    let (mut t, ids) = build();
    t.entry_mut(ids[10].0).summary.min_value = -1;
    assert!(matches!(t.check_invariants(), Err(InvariantViolation::StaleSummary { .. })));

    let (mut t, ids) = build();
    t.entry_mut(ids[20].0).key = 1000;
    assert!(matches!(t.check_invariants(), Err(InvariantViolation::OrderViolation { .. })));

    let (mut t, ids) = build();
    let leaf = ids.iter().map(|id| id.0).find(|&i| t.node(i).left == NIL && t.node(i).right == NIL).expect("folha");
    t.node_mut(leaf).parent = t.root;
    assert!(matches!(t.check_invariants(), Err(InvariantViolation::BrokenParentLink { .. })));

    let (mut t, ids) = build();
    t.leftmost = ids[1].0;
    assert!(matches!(t.check_invariants(), Err(InvariantViolation::WrongLeftmost { .. })));

    let (mut t, _) = build();
    t.len += 1;
    assert!(matches!(t.check_invariants(), Err(InvariantViolation::LenMismatch { .. })));

    let (mut t, ids) = build();
    t.remove(ids[5]);
    t.free_head = NIL;
    assert_eq!(t.check_invariants(), Err(InvariantViolation::FreeListCorrupt));
}

#[test]
#[should_panic(expected = "não aponta pra nó ocupado")]
fn removing_twice_panics() {
    let mut tree: RbTree<u32, ()> = RbTree::new();
    let id = tree.insert(1, ());
    tree.remove(id);
    tree.remove(id);
}
