//! Contagem de células vivas do registro (`cell_registry::live_cell_count`), a medida do futuro coletor.
//! Hoje não há coleta: toda célula vive até o fim da thread (`wip-notes/gc-audit.md`).
use zjsc::api::eval::evaluate_script;
use zjsc::runtime::cell_registry::live_cell_count;

#[test]
fn live_cell_count_grows_with_allocated_objects() {
    let before = live_cell_count();
    let result = evaluate_script("var keep = []; for (var i = 0; i < 10000; i++) { keep.push({ i: i }); } keep.length");
    assert!(result.is_ok(), "o script deve rodar");
    let after = live_cell_count();
    assert!(after >= before + 10000, "esperava ao menos 10000 células novas, antes={before} depois={after}");
}

/// Quando houver coletor, lixo cíclico inalcançável tem de sair do registro depois de `collect_garbage`.
#[test]
#[ignore = "sem coletor: o registro é o único dono das células e nunca solta nenhuma (gc-audit.md, seção 5)"]
fn registry_does_not_grow_with_cyclic_garbage() {
    let vm = zjsc::runtime::vm::VM::new();
    let before = live_cell_count();
    let result = evaluate_script("for (var i = 0; i < 10000; i++) { var o = {}; o.self = o; }");
    assert!(result.is_ok(), "o script deve rodar");
    vm.collect_garbage();
    let after = live_cell_count();
    assert!(after < before + 100, "lixo cíclico não foi coletado, antes={before} depois={after}");
}
