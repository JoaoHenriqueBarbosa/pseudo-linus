//! Regressões de comportamento de módulos em tempo de execução, medidas no bun 1.4.2. Cada caso fixa uma
//! divergência dos goldens `module_*` que já fechou.
use zjsc::api::module::evaluate_module_map;

fn run(files: &[(&str, &str)]) -> (String, Option<String>) {
    let files: Vec<(String, String)> = files.iter().map(|(name, text)| ((*name).to_owned(), (*text).to_owned())).collect();
    let outcome = evaluate_module_map(&files, "main.mjs");
    (outcome.log_json, outcome.error)
}

/// O namespace é `OverridesPut`: a escrita num objeto que herda dele cai no `[[Set]]` do namespace, que recusa.
#[test]
fn write_through_namespace_prototype_is_readonly() {
    let (log, error) = run(&[
        ("main.mjs", "import * as ns from './a.mjs'; try { Object.create(ns).x = 1; L('wrote'); } catch (e) { L(e.name + ': ' + e.message); }"),
        ("a.mjs", "export let l = 1;"),
    ]);
    assert_eq!(error, None);
    assert_eq!(log, r#"["TypeError: Attempted to assign to readonly property."]"#);
}
