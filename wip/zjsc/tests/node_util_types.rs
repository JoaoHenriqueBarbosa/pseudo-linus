//! `require("util/types")` contra o bun 1.4.2: `tests/golden/node_util_types_main.js` roda como `/app/main.js` e a
//! saída tem de ser a que o bun do oráculo deu (`node_util_types_main.expected.txt`, gerada com `bun /app/main.js` na
//! imagem do oráculo): os 44 predicados, a superfície e cada um sobre uma bateria de 72 valores.
mod main_golden;

#[test]
fn node_util_types_matches_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/node_util_types_main.js"), include_str!("golden/node_util_types_main.expected.txt"));
}

/// O programa principal ES: `isModuleNamespaceObject` de um namespace estático e de um `import()`.
#[test]
fn node_util_types_namespace_matches_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/node_util_types_esm_main.js"), include_str!("golden/node_util_types_esm_main.expected.txt"));
}
