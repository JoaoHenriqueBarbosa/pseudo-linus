//! Golden dos streams web contra o JavaScriptCore do bun: `tests/golden/streams_bun.tsv` sai do gerador de golden
//! rodado no bun 1.4.2. Cada linha é um programa (JSON) e o texto da variável global `R` que ele grava. Só os casos de
//! [`IN_SCOPE`] (índice da linha no TSV, base 0, faixas inclusivas) rodam; o resto é comportamento e entra com as
//! fatias seguintes de `wip-notes/streams-plan.md`.
//! Ficam de fora, por LACUNA (linhas 1-based do TSV): `TextEncoderStream`/`TextDecoderStream`/`CompressionStream`/
//! `DecompressionStream` (40 a 51), `getReader()`/`values()` em instância (70, 71, 73 a 75), o fluxo de bytes, `tee`,
//! `values`, `from`, `pipeTo`/`pipeThrough` e o resto do comportamento (a partir da linha 188), exceto o
//! `WritableStream` sem timer e o `TransformStream` (ver `IN_SCOPE`).
mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};

use zjsc::api::eval::evaluate_named_script_running_timers_reporting_uncaught;

const GOLDEN: &str = include_str!("golden/streams_bun.tsv");

const IN_SCOPE: &[(usize, usize)] = &[
    // Descritor do global, `length`, `name`, chaves do construtor e do protótipo (inclui as duas estratégias de fila).
    (0, 38),
    // Descritores de cada membro do protótipo, símbolos, `values === [Symbol.asyncIterator]`, `ReadableStream.from`,
    // `toStringTag` das instâncias que já se criam.
    (51, 68),
    (71, 71),
    // Construtor sem `new`, argumento de leitor/escritor, construtor ilegal, `locked` e estratégias.
    (75, 91),
    // `this` alheio em métodos (lança ou rejeita), em acessores e nos getters de promessa.
    (133, 154),
    // `new ReadableStream(...)`: validação de fonte e de estratégia, `type: 'bytes'`, `start` que lança.
    (92, 106),
    // Fatia 3: start/pull/cancel, `highWaterMark`, `size`, controlador padrão (`enqueue`/`close`/`error`/`desiredSize`),
    // leitor padrão (`read`/`releaseLock`/`closed`/`cancel`) e a ordem de microtasks (os índices 168 e 172 usam
    // `setTimeout`, e o corredor esvazia o laço de eventos virtual). Ficam fora (por LACUNA): `getReader({ mode })`
    // com BYOB ou `options` inválido (187 a 189).
    (155, 186),
    (190, 191),
    // `tee` (199 a 205): ordem de leituras, cancel dos dois ramos com a razão composta, erro propagado, cópia do chunk.
    (199, 205),
    // `length` de `respond`/`respondWithNewView`, descritor do `inspect.custom`, controladores sem `new`, texto do
    // `inspect.custom` do `ReadableStream`, do `WritableStream` novo e do controlador (o do leitor e o do
    // `TransformStream` passam por `util.inspect`, índices 332 e 334, fora).
    (314, 331),
    (333, 333),
    (335, 338),
    // `WritableStream`: validação do construtor (107 a 111) e o comportamento de escritor/controlador/sink (226 a 240;
    // o corredor esvazia o laço de eventos virtual, então os turnos de timer rodam). Fica fora o `pipeTo`/`pipeThrough`.
    (107, 111),
    (226, 240),
    // `TransformStream`: validação do construtor (112 a 117) e o comportamento de transform/flush/cancel, controlador
    // (`enqueue`/`error`/`terminate`/`desiredSize`) e contrapressão (241 a 254). Fica fora o `pipeTo`/`pipeThrough`.
    (112, 117),
    (241, 254),
    // Reentrada no `TransformStream` por dentro do `size()` e do `transform` (339 a 346, medida no bun).
    (339, 346),
    // `pipeTo`/`pipeThrough` (255 a 274): ordem de escritas, preventClose/Abort/Cancel, signal, erros de argumento e de
    // stream travado, propagação de erro nos dois sentidos.
    (255, 274),
    // Grade medida de `pipeTo`/`pipeThrough` (347 a 369): preventClose/Abort/Cancel contra término, erro nos dois lados
    // e `signal`, destino e origem já fechados, `pipeThrough` com `TransformStream`.
    (347, 369),
    // Fluxo de bytes (370 a 510): `getReader({ mode })`, leitor BYOB (`cancel` e `releaseLock` com leitura pendente, view
    // vazia com `done: true` num stream cancelado), controlador de bytes, `byobRequest` (`view`, `respond`,
    // `respondWithNewView`, invalidação), `autoAllocateChunkSize`, `pull` que lança, `read(view, { min })` (o `close()`
    // com descritor parcial alinhado não erra e a leitura fica pendente) e os `this` alheios do
    // `ReadableStreamBYOBRequest` e do `byobRequest`.
    (370, 510),
];

fn in_scope(line: usize) -> bool {
    IN_SCOPE.iter().any(|&(first, last)| (first..=last).contains(&line))
}

#[test]
fn streams_match_bun() {
    let scoped: String = GOLDEN.lines().enumerate().filter(|&(number, _)| in_scope(number)).map(|(_, line)| format!("{line}\n")).collect();
    let expected: usize = IN_SCOPE.iter().map(|&(first, last)| last - first + 1).sum();
    assert_eq!(scoped.lines().count(), expected, "faixa de IN_SCOPE fora do golden");
    common::check(&scoped, common::NO_PRELUDES, expected, |source| {
        match catch_unwind(AssertUnwindSafe(|| evaluate_named_script_running_timers_reporting_uncaught(source, "streams_case.js", "R"))) {
            Ok(Err(uncaught)) => Ok(common::Units::from(uncaught)),
            Ok(Ok(read)) => common::guarded_units(|| read),
            Err(panic) => {
                let reason = panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string())).unwrap_or_default();
                Err(format!("pânico: {reason}"))
            }
        }
    });
}
