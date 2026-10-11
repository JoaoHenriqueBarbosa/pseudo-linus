//! Golden de Error e stack traces de runtime contra o JavaScriptCore do bun: `tests/golden/error_bun.tsv` sai de
//! `scripts/gen-error-golden.js`, rodado no bun 1.4.2. Cada linha é um programa (JSON, várias linhas) e o texto da
//! variável global `R` que ele grava, depois de esvaziadas as microtarefas. Cobre o construtor (`message`,
//! `options.cause` e a ordem de leitura), `AggregateError`, `SuppressedError`, `Error.captureStackTrace`,
//! `Error.stackTraceLimit`, `Error.prepareStackTrace` com `CallSite`, a propriedade `stack`, `toString`,
//! `instanceof`, as mensagens `(evaluating '...')` dos erros nativos e os tipos de erro. O arquivo se chama
//! `error_case.js` dos dois lados, então nome, linha e coluna de cada frame têm de ser idênticos.
mod common;

const GOLDEN: &str = include_str!("golden/error_bun.tsv");

const PRELUDES: &str = include_str!("golden/error.preludes.json");

/// Linhas do golden (1-based) que divergem do bun só pelo transpilador dele, que reimprime o arquivo antes de entregá-lo ao motor:
/// linha e coluna dos frames de `stack` (o texto do transpilador junta statements e muda a coluna do chamador), o nome do frame
/// (`<anonymous>`), e os programas que o transpilador recusa no parse de arquivo (`'str'.length = 1`, `const c = 1; c = 2`: o bun
/// não roda o programa, `R` fica indefinido, enquanto por `eval` o bun lança o mesmo TypeError do motor, medido no bun 1.4.2).
/// Fecham quando o printer do zbun alimentar o zjsc (backlog no `PLAN.md`).
const CAUSE: &str = "reimpressão do transpilador: posição e nome de frame, ou programa recusado no parse do arquivo";
const KNOWN: &[common::KnownCase] = &[
    common::KnownCase { label: "329", cause: CAUSE },
    common::KnownCase { label: "340", cause: CAUSE },
    common::KnownCase { label: "384", cause: CAUSE },
    common::KnownCase { label: "385", cause: CAUSE },
    common::KnownCase { label: "541", cause: CAUSE },
    common::KnownCase { label: "542", cause: CAUSE },
    common::KnownCase { label: "551", cause: CAUSE },
    common::KnownCase { label: "584", cause: CAUSE },
    common::KnownCase { label: "593", cause: CAUSE },
    common::KnownCase { label: "689", cause: CAUSE },
    common::KnownCase { label: "698", cause: CAUSE },
    common::KnownCase { label: "793", cause: CAUSE },
    common::KnownCase { label: "794", cause: CAUSE },
    common::KnownCase { label: "803", cause: CAUSE },
    common::KnownCase { label: "877", cause: CAUSE },
    common::KnownCase { label: "878", cause: CAUSE },
    common::KnownCase { label: "887", cause: CAUSE },
    common::KnownCase { label: "940", cause: CAUSE },
    common::KnownCase { label: "941", cause: CAUSE },
    common::KnownCase { label: "950", cause: CAUSE },
    common::KnownCase { label: "1129", cause: CAUSE },
    common::KnownCase { label: "1130", cause: CAUSE },
    common::KnownCase { label: "1139", cause: CAUSE },
    common::KnownCase { label: "1256", cause: CAUSE },
    common::KnownCase { label: "1265", cause: CAUSE },
    common::KnownCase { label: "1305", cause: CAUSE },
    common::KnownCase { label: "1309", cause: CAUSE },
    common::KnownCase { label: "1310", cause: CAUSE },
    common::KnownCase { label: "1330", cause: CAUSE },
    common::KnownCase { label: "1331", cause: CAUSE },
    common::KnownCase { label: "1348", cause: CAUSE },
    common::KnownCase { label: "1431", cause: CAUSE },
    common::KnownCase { label: "1432", cause: CAUSE },
    common::KnownCase { label: "1433", cause: CAUSE },
    common::KnownCase { label: "1450", cause: CAUSE },
    common::KnownCase { label: "1452", cause: CAUSE },
    common::KnownCase { label: "1460", cause: CAUSE },
    common::KnownCase { label: "1468", cause: CAUSE },
    common::KnownCase { label: "1469", cause: CAUSE },
    common::KnownCase { label: "1470", cause: CAUSE },
    common::KnownCase { label: "1476", cause: CAUSE },
    common::KnownCase { label: "1500", cause: CAUSE },
    common::KnownCase { label: "1501", cause: CAUSE },
    common::KnownCase { label: "1506", cause: CAUSE },
    common::KnownCase { label: "1508", cause: CAUSE },
    common::KnownCase { label: "2014", cause: CAUSE },
    common::KnownCase { label: "2024", cause: CAUSE },
    common::KnownCase { label: "2025", cause: CAUSE },
    common::KnownCase { label: "2606", cause: CAUSE },
    common::KnownCase { label: "2607", cause: CAUSE },
    common::KnownCase { label: "2608", cause: CAUSE },
    common::KnownCase { label: "2609", cause: CAUSE },
    common::KnownCase { label: "2610", cause: CAUSE },
    common::KnownCase { label: "2611", cause: CAUSE },
    common::KnownCase { label: "2612", cause: CAUSE },
];

#[test]
fn error_matches_bun() {
    common::run_mapped_golden_big_stack_known_lines(GOLDEN, PRELUDES, 1500, "error_case.js", KNOWN);
}
