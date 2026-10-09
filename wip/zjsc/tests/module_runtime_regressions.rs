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

/// O módulo JSON do Bun exporta as chaves do objeto de topo; o que não é objeto exporta `__esModule`.
#[test]
fn json_module_exports_top_level_keys() {
    let (log, error) = run(&[
        (
            "main.mjs",
            "import * as d from './d.json'; import * as e from './e.json'; import { a } from './d.json'; \
             L(Object.keys(d)); L(JSON.stringify(d.default)); L(a); L(Object.keys(e)); L(e.__esModule);",
        ),
        ("d.json", r#"{"b":1,"a":2,"default":3}"#),
        ("e.json", "[1,2]"),
    ]);
    assert_eq!(error, None);
    assert_eq!(log, r#"["a,b,default","{\"b\":1,\"a\":2,\"default\":3}","2","__esModule,default","true"]"#);
}

/// `import.meta` do Bun: nenhuma propriedade própria, os valores vêm de um protótipo escondido que lê pelo `this`
/// (`@@toStringTag` `ImportMeta`); `getPrototypeOf` dá `null`, mas o `[[Prototype]]` de verdade continua lá.
#[test]
fn import_meta_is_the_bun_import_meta_object() {
    let (log, error) = run(&[(
        "main.mjs",
        "const m = import.meta; L(Reflect.ownKeys(m).length); L('hasOwnProperty' in m); L('url' in m); \
         L(Object.prototype.toString.call(m)); L(Object.getPrototypeOf(m)); L(m instanceof Object); \
         const d = Object.create(m); L(d.url); L('url' in d); L(Object.prototype.toString.call(d)); \
         L(typeof m.main); m.x = 1; m.url = 'z'; L(JSON.stringify(m)); \
         try { m(); } catch (e) { L(e.message); } \
         Reflect.setPrototypeOf(m, null); L('resolve' in m);",
    )]);
    assert_eq!(error, None);
    assert_eq!(
        log,
        concat!(
            r#"["0","true","true","[object ImportMeta]","null","false","undefined","true","[object ImportMeta]","boolean","#,
            r#""{\"x\":1,\"url\":\"z\"}","m is not a function. (In 'm()', 'm' is an instance of ImportMeta)","false"]"#
        )
    );
}
