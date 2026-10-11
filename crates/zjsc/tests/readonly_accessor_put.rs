//! Atribuição a acessor sem setter (getter nativo de protótipo, `CustomAccessor|ReadOnly` ainda não reificado, ou
//! `GetterSetter` sem setter): `TypeError: Attempted to assign to readonly property.` em modo estrito, silêncio no
//! sloppy. Valores medidos no bun 1.4.2. O `Structure` raiz liga os bits de "somente leitura ou getter/setter" a partir
//! da tabela estática do `ClassInfo` (`hasStaticSetterOrReadonlyProperties`), senão o `put` toma o caminho rápido.
use zjsc::api::eval::evaluate_script;
use zjsc::wtf::text::conversion_mode::ConversionMode;

fn run(program: &str) -> String {
    let value = evaluate_script(program).unwrap_or_else(|_| panic!("o programa lançou exceção"));
    assert!(value.is_string(), "o programa não devolveu string");
    String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned()
}

const CASES: [(&str, &str); 14] = [
    ("DataView.byteOffset", "new DataView(new ArrayBuffer(4)).byteOffset = 1"),
    ("DataView.buffer", "new DataView(new ArrayBuffer(4)).buffer = 1"),
    ("DataView.byteLength", "new DataView(new ArrayBuffer(4)).byteLength = 1"),
    ("TypedArray.length", "new Uint8Array(2).length = 1"),
    ("TypedArray.byteLength", "new Uint8Array(2).byteLength = 1"),
    ("ArrayBuffer.byteLength", "new ArrayBuffer(2).byteLength = 1"),
    ("Map.size", "new Map().size = 1"),
    ("Set.size", "new Set().size = 1"),
    ("RegExp.flags", "/a/g.flags = 'i'"),
    ("RegExp.source", "/a/g.source = 'i'"),
    ("Symbol.description", "Symbol('x').description = 1"),
    ("class getter", "new (class { get x() { return 1; } })().x = 1"),
    ("literal getter", "({ get x() { return 1; } }).x = 2"),
    ("Function.length", "(function () {}).length = 3"),
];

fn report(strict: bool) -> String {
    let directive = if strict { "'use strict';" } else { "" };
    let mut program = String::from("var out = [];\n");
    for (name, statement) in CASES {
        program.push_str(&format!(
            "try {{ (function () {{ {directive} {statement}; }})(); out.push('{name}=ok'); }} \
             catch (e) {{ out.push('{name}=' + e.name + ': ' + e.message); }}\n"
        ));
    }
    program.push_str("out.join('|')");
    run(&program)
}

#[test]
fn strict_mode_throws() {
    let expected: Vec<String> =
        CASES.iter().map(|(name, _)| format!("{name}=TypeError: Attempted to assign to readonly property.")).collect();
    assert_eq!(report(true), expected.join("|"));
}

#[test]
fn sloppy_mode_is_silent() {
    let expected: Vec<String> = CASES.iter().map(|(name, _)| format!("{name}=ok")).collect();
    assert_eq!(report(false), expected.join("|"));
}
