//! Golden de exceção não capturada contra o bun 1.4.2: `tests/golden/uncaught_bun.tsv` sai de
//! `scripts/gen-uncaught-golden.js`. Cada linha é `JSON(fonte)<TAB>hex(stderr)<TAB>código de saída`. Esta fatia cobre só o
//! `throw` síncrono no topo de `Error`, das subclasses nativas e de primitivos, o trecho de contexto, `cause`,
//! `AggregateError`, erros de timers/microtarefas, primitivos lançados em timer (o trecho e o frame do `throw`), `cause`
//! circular e rejeições sem tratador de `Error`/primitivos, propriedades próprias do erro (`code` e as demais) e objeto
//! ou array lançado; o rótulo de cada caso
//! coberto está em [`IN_SCOPE`]. Todo outro caso é contado como esperado-falhando (não roda) e listado na saída, de modo
//! que nenhuma linha do golden fica fora da conta.
mod common;

const GOLDEN: &str = include_str!("golden/uncaught_bun.tsv");

/// Quantas linhas iniciais do golden são os casos de trecho de fonte (`contextCases` em
/// `scripts/gen-uncaught-golden.js`): arquivos longos, linhas em branco, tabs, CRLF, unicode, linha longa, sem `\n`
/// final. Todos entram no escopo, sem repetir os fontes aqui.
const CONTEXT_CASES: usize = 55;

/// Os casos desta fatia: rótulo e fonte exato (como está no gerador).
const IN_SCOPE: &[(&str, &str)] = &[
    ("error", "throw new Error(\"boom\");\n"),
    ("native-type-error", "throw new TypeError(\"bad type\");\n"),
    ("native-range-error", "throw new RangeError(\"range\");\n"),
    ("primitive-string", "throw \"uma string\";\n"),
    ("primitive-empty-string", "throw \"\";\n"),
    ("primitive-number", "throw 42;\n"),
    ("primitive-negative-zero", "throw -0;\n"),
    ("primitive-fraction", "throw 1.5;\n"),
    ("context-previous-line", "console.log(\"antes\");\nthrow new Error(\"depois do log\");\n"),
    ("context-pending-timer", "setTimeout(() => { console.log(\"nunca\"); }, 50);\nthrow new Error(\"antes do timer\");\n"),
    ("context-four-lines", "\n\n\nthrow new Error(\"linha 4\");\n"),
    ("context-class", "class MyErr extends Error {}\nthrow new MyErr(\"custom\");\n"),
    ("cause-error", "const e = new Error(\"outer\", { cause: new TypeError(\"inner\") });\nthrow e;\n"),
    ("cause-string", "throw new Error(\"outer\", { cause: \"texto\" });\n"),
    ("cause-object", "throw new Error(\"outer\", { cause: { x: 1 } });\n"),
    ("source-line-secrets-not-redacted", "const token = \"npm_abcdefghijklmnopqrstuvwxyz0123456789abcd\"; const password = \"hunter2\"; throw new Error(\"x \" + token);\n"),
    ("error-secret-named-properties", "const e = new Error(\"p\"); e.password = \"hunter2\"; e.token = \"abc\"; e.code = \"E_X\"; throw e;\n"),
    ("cause-chain", "throw new Error(\"a\", { cause: new Error(\"b\", { cause: new Error(\"c\") }) });\n"),
    ("cause-rethrow", "try { throw new Error(\"a\"); } catch (e) { throw new Error(\"b\", { cause: e }); }\n"),
    ("aggregate", "throw new AggregateError([new Error(\"a\"), new TypeError(\"b\")], \"agg\");\n"),
    ("aggregate-empty", "throw new AggregateError([], \"vazio\");\n"),
    ("rejection-error", "Promise.reject(new Error(\"rejeitada\"));\n"),
    ("rejection-string", "Promise.reject(\"string rejeitada\");\n"),
    ("rejection-number", "Promise.reject(7);\n"),
    ("rejection-then-throw", "Promise.resolve().then(() => { throw new Error(\"then\"); });\n"),
    ("rejection-two", "Promise.reject(new Error(\"r1\"));\nPromise.reject(new Error(\"r2\"));\n"),
    ("rejection-before-timer", "const p = Promise.reject(new Error(\"a\")); setTimeout(() => p.catch(() => {}), 10);\n"),
    ("timeout-throw", "setTimeout(() => { throw new Error(\"timer\"); }, 0);\n"),
    ("immediate-throw", "setImmediate(() => { throw new Error(\"immediate\"); });\n"),
    ("interval-throw", "setInterval(() => { throw new Error(\"interval\"); }, 1);\n"),
    ("microtask-throw", "queueMicrotask(() => { throw new Error(\"microtask\"); });\n"),
    ("late-handled-then-timer", "Promise.reject(new Error(\"tratada tarde\")).catch(() => {});\nsetTimeout(() => { throw new Error(\"depois\"); }, 0);\n"),
    ("timeout-throw-string", "setTimeout(() => { throw \"timer string\"; }, 0);\n"),
    ("timeout-throw-number", "setTimeout(() => { throw 5; }, 0);\n"),
    ("timeout-throw-object", "setTimeout(() => { throw { z: 1 }; }, 0);\n"),
    ("cause-circular", "const e = new Error(\"a\"); e.cause = e; throw e;\n"),
    ("cause-cycle-two", "const a = new Error(\"a\"); const b = new Error(\"b\"); a.cause = b; b.cause = a; throw a;\n"),
    ("cause-cycle-three", "const a = new Error(\"a\"); const b = new Error(\"b\"); const c = new Error(\"c\"); a.cause = b; b.cause = c; c.cause = a; throw a;\n"),
    ("cause-aggregate-self", "const g = new AggregateError([new Error(\"x\")], \"agg\"); g.cause = g; throw g;\n"),
    ("cause-tail-self", "const a = new Error(\"a\"); const b = new Error(\"b\"); a.cause = b; b.cause = b; throw a;\n"),
    ("error-extra-properties", "const e = new Error(\"msg\"); e.extra = 1; e.arr = [1, 2]; throw e;\n"),
    ("error-name-and-code-number", "class MyErr extends Error { constructor(m) { super(m); this.name = \"MyErr\"; this.code = 7; } }\nthrow new MyErr(\"custom\");\n"),
    ("error-code-string", "throw Object.assign(new Error(\"props\"), { code: \"E_X\", errno: -2 });\n"),
    ("throw-object", "throw { a: 1, b: \"x\" };\n"),
    ("throw-array", "throw [1, 2, 3];\n"),
    ("throw-object-name-message", "throw { message: \"m\", name: \"N\" };\n"),
    ("throw-null-prototype", "throw Object.create(null);\n"),
    ("rejection-object", "Promise.reject({ k: 1 });\n"),
    ("throw-function", "throw function f() {};\n"),
    ("error-empty-message", "const e = new Error(\"\"); throw e;\n"),
    ("dom-exception", "throw new DOMException(\"dom\", \"AbortError\");\n"),
    ("dom-exception-no-name", "throw new DOMException(\"sem nome\");\n"),
    ("dom-exception-no-args", "throw new DOMException();\n"),
    ("dom-exception-not-found", "throw new DOMException(\"m\", \"NotFoundError\");\n"),
    ("dom-exception-quota", "throw new DOMException(\"m\", \"QuotaExceededError\");\n"),
    ("dom-exception-unknown-name", "throw new DOMException(\"m\", \"Qualquer\");\n"),
    ("dom-exception-empty-message", "throw new DOMException(\"\", \"AbortError\");\n"),
    ("dom-exception-native-atob", "try { atob(\"*\"); } catch (e) { throw e; }\n"),
    ("dom-exception-native-structured-clone", "try { structuredClone(() => {}); } catch (e) { throw e; }\n"),
    ("dom-exception-subclass", "class X extends DOMException {}\nthrow new X(\"sub\", \"AbortError\");\n"),
    ("dom-exception-subclass-extra", "class X extends DOMException { constructor() { super(\"sub\", \"DataCloneError\"); this.extra = 1; } }\nthrow new X();\n"),
    ("dom-exception-prototype-only", "throw Object.create(DOMException.prototype);\n"),
    ("dom-exception-extra-property", "const e = new DOMException(\"m\", \"AbortError\"); e.extra = 1; throw e;\n"),
    ("dom-exception-assign-name", "const e = new DOMException(\"m\", \"AbortError\"); e.name = \"Outro\"; throw e;\n"),
    ("error-code-prefix-match", "const e = new Error(\"ERR_X: texto\"); e.code = \"ERR_X\"; throw e;\n"),
    ("error-code-no-prefix", "const e = new Error(\"texto\"); e.code = \"ERR_X\"; throw e;\n"),
    ("error-code-prefix-mismatch", "const e = new Error(\"ERR_X: texto\"); e.code = \"ERR_Y\"; throw e;\n"),
    ("error-code-number-prefix", "const e = new Error(\"ERR_X: texto\"); e.code = 5; throw e;\n"),
    ("error-prefix-without-code", "const e = new Error(\"ERR_X: texto\"); throw e;\n"),
    ("promise-builtin-frame", "function a() { return new Promise((_, rej) => rej(new Error(\"x\"))); }\nfunction b() { return a(); }\nb();\n"),
    ("promise-builtin-frame-deep", "function a() { return new Promise((_, rej) => { rej(new Error(\"x\")); }); }\nfunction b() { a(); }\nfunction c() { b(); }\nc();\n"),
    ("async-after-await", "async function f() { await null; throw new Error(\"apos await\"); }\nf();\n"),
    ("async-caller-awaiting", "async function g() { await null; throw new Error(\"apos await\"); }\nasync function f() { await g(); }\nf();\n"),
    ("async-without-await", "async function f() { throw new Error(\"sem await\"); }\nasync function g() { await f(); }\ng();\n"),
    ("then-chained", "function f() { throw new Error(\"then encadeado\"); }\nPromise.resolve(1).then(() => 2).then(() => f());\n"),
    ("then-first-rejects", "Promise.resolve().then(() => { throw new Error(\"t1\"); }).then(() => 1);\n"),
    ("then-named-function", "Promise.resolve().then(function inner() { throw new Error(\"nomeada\"); });\n"),
    ("rejection-method-caller", "const o = { m() { return Promise.reject(new Error(\"metodo\")); } };\nfunction run() { o.m(); }\nrun();\n"),
    ("async-static-method", "class K { static async s() { await 0; throw new Error(\"classe\"); } }\nK.s();\n"),
    ("async-throws-string", "async function f() { await null; throw \"string apos await\"; }\nf();\n"),
    ("rejection-direct-callers", "function a() { Promise.reject(new Error(\"direto\")); }\nfunction b() { a(); }\nb();\n"),
    ("rejection-in-timer", "new Promise((_, rej) => setTimeout(() => rej(new Error(\"timer rej\")), 1));\n"),
    ("async-cause-two-blocks", "async function f() { try { await Promise.reject(new Error(\"a\")); } catch (e) { throw new Error(\"b\", { cause: e }); } }\nf();\n"),
    ("object-frame-line", "throw { sourceURL: \"x.js\", line: 7, column: 3, name: \"N\", message: \"m\" };\n"),
    ("object-frame-no-line", "throw { sourceURL: \"x.js\", name: \"N\", message: \"m\" };\n"),
    ("object-frame-empty-url", "throw { sourceURL: \"\", line: 7, name: \"N\", message: \"m\" };\n"),
    ("object-frame-line-zero", "throw { sourceURL: \"x.js\", line: 0, name: \"N\", message: \"m\" };\n"),
    ("object-frame-string-line", "throw { sourceURL: \"x.js\", line: \"7\", name: \"N\", message: \"m\" };\n"),
    ("object-frame-number-url", "throw { sourceURL: 5, line: 7, name: \"N\", message: \"m\" };\n"),
    ("object-frame-original-line", "throw { sourceURL: \"x.js\", line: 7, originalLine: 12, name: \"N\", message: \"m\" };\n"),
    ("object-frame-original-line-zero", "throw { sourceURL: \"x.js\", line: 7, originalLine: 0, name: \"N\", message: \"m\" };\n"),
    ("object-frame-original-line-string", "throw { sourceURL: \"x.js\", line: 7, originalLine: \"12\", name: \"N\", message: \"m\" };\n"),
    ("object-frame-original-line-only", "throw { sourceURL: \"x.js\", originalLine: 12, name: \"N\", message: \"m\" };\n"),
    ("object-frame-string-line-original", "throw { sourceURL: \"x.js\", line: \"7\", originalLine: 12, name: \"N\", message: \"m\" };\n"),
    ("object-frame-original-fraction", "throw { sourceURL: \"x.js\", line: 7, originalLine: 12.9, originalColumn: 4, name: \"N\", message: \"m\" };\n"),
];

#[test]
fn uncaught_matches_bun() {
    common::run_with_stack(256 * 1024 * 1024, || {
        zjsc::runtime::vm::VM::set_thread_stack_budget(240 * 1024 * 1024);
        let mut covered = 0;
        let mut expected_failing = Vec::new();
        let mut failures = Vec::new();
        let mut context_covered = 0;
        for (index, line) in GOLDEN.lines().filter(|line| !line.is_empty()).enumerate() {
            let row = common::MainScriptRow::parse(line);
            let label = if index < CONTEXT_CASES {
                context_covered += 1;
                "context-excerpt"
            } else if let Some((label, _)) = IN_SCOPE.iter().find(|(_, scoped)| *scoped == row.source) {
                *label
            } else {
                expected_failing.push(row.source);
                continue;
            };
            covered += 1;
            failures.extend(row.check(label));
        }
        assert_eq!(context_covered, CONTEXT_CASES, "os casos de trecho precisam existir no golden");
        assert_eq!(covered, IN_SCOPE.len() + CONTEXT_CASES, "cada caso do escopo precisa existir no golden");
        println!("uncaught: {covered} casos cobertos, {} esperados-falhando (ainda não portados):", expected_failing.len());
        for source in &expected_failing {
            println!("  xfail {source:?}");
        }
        assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
    });
}
