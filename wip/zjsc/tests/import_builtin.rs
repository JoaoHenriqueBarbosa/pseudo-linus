//! Módulos embutidos pelo `import` a partir do `/app/main.js`, contra o bun 1.4.2: o `import()` do principal CommonJS
//! e o `import` estático e dinâmico do principal módulo ES resolvem `node:path` e `path` para o mesmo namespace, com o
//! `default` igual ao `require`, e `node:` desconhecido rejeita com `ERR_UNKNOWN_BUILTIN_MODULE`. Os esperados vêm de
//! `bun /app/main.js` na imagem do oráculo.
mod main_golden;

#[test]
fn cjs_dynamic_import_of_builtins_matches_bun() {
    main_golden::assert_main_script_matches(
        include_str!("golden/cjs_dynamic_import_builtin_main.js"),
        include_str!("golden/cjs_dynamic_import_builtin_main.expected.txt"),
    );
}

#[test]
fn esm_import_of_builtins_matches_bun() {
    main_golden::assert_main_script_matches(include_str!("golden/esm_import_builtin_main.js"), include_str!("golden/esm_import_builtin_main.expected.txt"));
}
