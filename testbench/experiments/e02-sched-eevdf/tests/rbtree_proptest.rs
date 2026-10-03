//! H12: 100 mil sequências de operações na `rbtree`, com invariantes, modelo `BTreeMap` e força bruta
//! checados depois de cada operação (ver `rbprop`). Falha aqui é bug da árvore.

use e02_sched_eevdf::rbprop;

#[test]
fn rbtree_matches_model_on_100k_sequences() {
    let report = rbprop::run(100_000);
    assert!(report.passed, "{}", report.failure.unwrap_or_default());
    assert!(report.max_len >= 250, "as sequências precisam chegar a árvores grandes: {}", report.max_len);
}

/// Casos dirigidos que o gerador pode demorar a achar: só chaves iguais, e esvaziar e reencher.
#[test]
fn directed_sequences() {
    use rbprop::Op;
    let mut ops: Vec<Op> = (0..200).map(|i| Op::Insert { key: 7, value: i }).collect();
    ops.extend((0..100).map(|i| Op::RemoveNth(i * 13)));
    ops.extend((0..120).map(|_| Op::PopFirst));
    ops.extend((0..300).map(|i| Op::Insert { key: (i % 17) as u16, value: -i }));
    ops.extend((0..50).map(|i| Op::UpdateNth(i * 7, i32::MIN + i as i32)));
    ops.push(Op::Navigate(9));
    rbprop::check_sequence(&ops).expect("sequência dirigida");
}
