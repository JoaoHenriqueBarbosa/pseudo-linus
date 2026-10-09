//! Golden de `console.log` com funções, classes, `Map`, `Set`, iteradores e caixas de primitivo contra o bun 1.4.2:
//! `tests/golden/console_object_more_bun.tsv` sai de `bun scripts/gen-console-object-more-golden.js`. Colunas, todas em
//! JSON: fonte, bytes do stdout em hex, bytes do stderr em hex e o valor de `R`. O console do host é um [`MemoryConsole`];
//! o formatador está em `src/runtime/console_format.rs` e o modelo de referência em `scripts/console-object-model.js`.
//!
//! O arquivo cobre mais do que o formatador já faz (`Date`, `RegExp`, `Error`, `Promise`, typed arrays, `Symbol` e
//! `BigInt` encaixotados...). As linhas em escopo são as listadas em [`IN_SCOPE`] (índice da linha no TSV, base 0, faixas
//! inclusivas) e têm de bater byte a byte; as demais são contadas como xfail e só entram no relatório do teste.
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

use zjsc::api::eval::evaluate_named_script_with_console;
use zjsc::runtime::console_host::MemoryConsole;

const GOLDEN: &str = include_str!("golden/console_object_more_bun.tsv");

/// Os casos de `console.log(Error)`, medidos como programa principal (`/app/main.js`): fonte, stderr, código de saída e
/// stdout (ver `common::MainScriptRow`). As linhas 407 e 947 a 955 do TSV acima (via `eval`) ficam como xfail.
const MAIN_GOLDEN: &str = include_str!("golden/console_error_main_bun.tsv");

/// Faixas inclusivas de linhas do TSV que o formatador cobre nesta fatia.
const IN_SCOPE: &[(usize, usize)] = &[
    // Funções e classes: `[Function: f]`, `[AsyncFunction]`, `[class A extends B]`, sem propriedades próprias, e os
    // especificadores `%s`, `%o`, `%O`, `%j` com função e classe (82 a 84). O nome vem do executável
    // (`calculated_display_name`), por isso 28, 29 e 39 a 43 (`static name`, `defineProperty(f, 'name', ...)`) saem do
    // `function`/`class` declarado. Ficam de fora 35 (`Function.prototype`) e 58 (`arguments`).
    (0, 34),
    (36, 57),
    (59, 151),
    // `Date` (`print_json`), exceto `Date.prototype` (174, só dá para imprimir com as chaves não enumeráveis do objeto).
    (152, 173),
    (175, 182),
    // `RegExp` (`Tag::String`), exceto `RegExp.prototype` (196, getters), o iterador de `matchAll` (214) e os arrays do
    // resultado de `match` (215 a 219).
    (183, 195),
    (197, 213),
    // Caixas de primitivo: `Number`, `String`, `Boolean`, subclasses (`[Number (N): 5]`), `Number.prototype`, `Boolean.prototype`,
    // as classes `Number`, `String`, `Boolean`, `Symbol` e `BigInt` encaixotados, protótipo nulo (346, 347) e `%s` (367).
    // Ficam de fora `String.prototype` (354), `Symbol.prototype` e `BigInt.prototype` (356, 357).
    (313, 353),
    (355, 355),
    (358, 367),
    // `@@toStringTag` e protótipo nulo: o nome antes do `{`, o acessor `[Getter]` (370) e as chaves enumeráveis do
    // protótipo imediato quando o objeto não tem propriedade própria (381, 411), e o typed array e o `ArrayBuffer` de
    // protótipo nulo (404, 406: `print_typed_array` não olha o protótipo). Fica de fora 407 (`Error`, e 947 em diante, os
    // `Error` em array/objeto/subclasse): o formatador já imprime o relato do erro (`append_error_report`), mas o golden
    // vem de um `eval` dentro de `case.js` (frames `eval (file:///case.js:L:C)` e `/case.js:2:5`) e o trecho de contexto do
    // bun pula a segunda linha do fonte (numera a primeira como 2), o que o harness desta tabela, que roda o fonte como
    // programa, ainda não reproduz.
    (368, 400),
    (401, 406),
    (408, 432),
    // `Promise` (`<pending>`, `<resolved>`, `<rejected>`), e `%s`, `%o`, `%O`, `%j` com `Promise` (514 a 516). Ficam de fora
    // `Promise.prototype` (482), `Promise` (483), `Promise.withResolvers` (485) e os geradores (494 em diante).
    (433, 481),
    (484, 484),
    (486, 493),
    (514, 516),
    // Membros herdados do protótipo (`forEachPropertyImpl`): métodos de classe, getters, dois níveis, símbolo, limite de cinco.
    (909, 946),
    // `Symbol.for('nodejs.util.inspect.custom')` do usuário: retorno string crua, objeto e primitivo formatados de novo,
    // `depth` e `options`, método que lança, getter, herdado, `%o`/`%O`/`%s`/`%j` e `console.dir` (956 a 989, 996 a 1005).
    // Incluem 990 a 995: `TextEncoderStream` e `ReadableStream` (inspect custom nativo) e `Headers` (o `toJSON` com chaves
    // entre aspas, `format_web_pairs`).
    (956, 1005),
    // `Blob`, `File`, `Request` e `Response` sem cores (`format_web_body`): o tamanho do cabeçalho (`format_byte_size`) e os campos.
    (819, 820),
    (824, 825),
    // `AbortController`, `AbortSignal`, `Event`, `EventTarget` (826 a 829, 1053 a 1061) e `DOMException` (1065 a 1067): o
    // nome da classe, as chaves próprias e as do protótipo, os atributos nativos como valor (`is_custom_accessor_function`).
    // `MessageEvent` e `ErrorEvent` (1062, 1063: o bun imprime só `type` e os campos, `event_target::console_pairs`) e o
    // `console.dir` com cores de `Blob`, `File`, `Request`, `Response`, `AbortController`, `Event` e `DOMException` (1068 a
    // 1074). O `CloseEvent` (1064) sai do caminho comum: `isTrusted` próprio, `wasClean`, `code` e `reason` do protótipo e
    // depois os membros de `Event`.
    (826, 829),
    (1053, 1074),
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn in_scope(line: usize) -> bool {
    IN_SCOPE.iter().any(|&(first, last)| (first..=last).contains(&line))
}

#[test]
fn console_error_main_matches_bun() {
    common::run_with_stack(256 * 1024 * 1024, || {
        zjsc::runtime::vm::VM::set_thread_stack_budget(240 * 1024 * 1024);
        let failures: Vec<String> = MAIN_GOLDEN.lines().filter(|line| !line.is_empty()).flat_map(|line| common::MainScriptRow::parse(line).check("console-error-main")).collect();
        assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
    });
}

#[test]
fn console_object_more_match_bun() {
    let mut failures = Vec::new();
    let (mut total, mut checked, mut xfail, mut xfail_passing) = (0, 0, 0, 0);
    for (number, line) in GOLDEN.lines().enumerate() {
        let columns: Vec<&str> = line.split('\t').collect();
        assert_eq!(columns.len(), 4, "linha malformada: {line}");
        let source = common::json_string(columns[0]);
        let expected_stdout = common::json_string(columns[1]);
        let expected_stderr = common::json_string(columns[2]);
        let expected_result = common::json_string(columns[3]);
        total += 1;
        let console = Rc::new(MemoryConsole::new(b""));
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            evaluate_named_script_with_console(&source, "console_object_more_case.js", "R", Some(console.clone()))
                .map(|read| read.ok().filter(|value| !value.is_undefined()).map(|value| String::from_utf16_lossy(&value.to_wtf_string().characters_without_null_termination().expect("unidades UTF-16"))))
        }));
        let result = match outcome {
            Ok(Ok(Some(text))) => text,
            Ok(Ok(None)) => "<undefined>".to_string(),
            Ok(Err(uncaught)) => uncaught,
            Err(_) => "<pânico>".to_string(),
        };
        let (stdout, stderr) = (hex(&console.stdout_bytes()), hex(&console.stderr_bytes()));
        let matches = stdout == expected_stdout && stderr == expected_stderr && result == expected_result;
        if !in_scope(number) {
            xfail += 1;
            xfail_passing += usize::from(matches);
            continue;
        }
        checked += 1;
        if !matches {
            let shown = source.split_once('\n').and_then(|(_, rest)| rest.split_once('\n')).map_or(source.as_str(), |(_, program)| program);
            failures.push(format!("linha {number}: {shown}\n  stdout {stdout:?} esperado {expected_stdout:?}\n  stderr {stderr:?} esperado {expected_stderr:?}\n  R {result:?} esperado {expected_result:?}"));
        }
    }
    eprintln!("console_object_more: {checked} em escopo, {xfail} xfail ({xfail_passing} já batem), {total} no total");
    assert!(total >= 900, "poucos casos: {total}");
    assert_eq!(checked + xfail, total);
    assert!(failures.is_empty(), "{} de {checked} em escopo divergem:\n{}", failures.len(), failures.iter().take(10).cloned().collect::<Vec<_>>().join("\n"));
}
