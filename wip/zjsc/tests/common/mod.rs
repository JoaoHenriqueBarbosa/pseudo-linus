//! Parte comum dos goldens contra o bun: `tests/golden/<nome>_bun.tsv` com o prelúdio fatorado.
//!
//! Cada linha é `JSON(sufixo)<TAB>JSON(resultado)[<TAB>índice]`. O programa avaliado é `preludes[índice] + sufixo`, com
//! os prelúdios num array JSON de strings em `tests/golden/<nome>.preludes.json` (índice 0, o padrão quando a terceira
//! coluna falta), lido por [`preludes_from_json`]. O formato é escrito por `scripts/golden-prelude.js`.
//!
//! Uma quarta coluna opcional, `JSON([saídas])`, lista saídas alternativas que o bun também produz para o mesmo
//! programa (não determinismo medido por `scripts/golden-alternatives.js`); o caso passa com a segunda coluna ou com
//! qualquer alternativa. Linhas sem a quarta coluna se comportam como sempre.
//!
//! Uma quinta coluna opcional, `JSON([modo, índiceDaCauda, ...corridas])` (ver [`ProgramMeta::parse_factored`]), diz se o bun
//! rodou o programa como ESM ou CJS e leva o mapa de posições do texto gravado (reimpresso pelo bun) para o fonte original.
//! As corridas que caem dentro do prelúdio são quase as mesmas em todas as linhas e ficam no `preludes.json`, cujas
//! entradas viram `{"text":...,"runs":[...],"tails":[[...],...]}` ([`Prelude`]): `runs` é o prefixo comum e `tails[i]` o
//! resto de cada variante; a coluna guarda só o índice da cauda e as corridas do sufixo. Ela exige as colunas 3 e 4
//! (índice do prelúdio e `[]` quando não há alternativas). Teste de golden com essa coluna usa `check_with_meta` e
//! `evaluate_golden_program(source, meta, "<nome>.js", "R")`.
#![allow(dead_code)]

use std::panic::{catch_unwind, AssertUnwindSafe};

use std::borrow::Cow;
use std::ops::Deref;

use zjsc::api::eval::{
    evaluate_cjs_program_units, evaluate_indirect_eval_result_units, evaluate_mapped_script_result_units, evaluate_named_script_result_units,
    evaluate_run_in_this_context_units,
};
use zjsc::parser::position_map::parse_integer_array;
use zjsc::runtime::js_value::JSValue;

/// Texto como unidades de código UTF-16, sem perda: o `JSString` do motor e o literal JSON do golden podem guardar
/// surrogate solitário, que `String` do Rust não guarda. A comparação do golden é feita aqui, unidade a unidade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Units(pub Vec<u16>);

impl Units {
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl From<String> for Units {
    fn from(text: String) -> Units {
        Units(text.encode_utf16().collect())
    }
}

impl From<&str> for Units {
    fn from(text: &str) -> Units {
        Units(text.encode_utf16().collect())
    }
}

/// Texto legível das unidades para a mensagem de divergência: surrogate solitário sai como `\uXXXX` (o resto como no
/// `Debug` de `str`), então `\ud800` e `\udc00` nunca parecem iguais.
pub fn escape_units(units: &[u16]) -> String {
    let mut text = String::from("\"");
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok(character) => text.extend(character.escape_debug()),
            Err(error) => text.push_str(&format!("\\u{:04x}", error.unpaired_surrogate())),
        }
    }
    text.push('"');
    text
}

/// Decodifica um literal de string JSON (o que `JSON.stringify` emite) para unidades UTF-16, preservando surrogates.
pub fn json_units(literal: &str) -> Vec<u16> {
    let inner = literal.strip_prefix('"').and_then(|text| text.strip_suffix('"')).expect("literal JSON entre aspas");
    let mut decoded: Vec<u16> = Vec::new();
    let mut chars = inner.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            let mut buffer = [0u16; 2];
            decoded.extend_from_slice(character.encode_utf16(&mut buffer));
            continue;
        }
        let escape = chars.next().expect("escape completo");
        match escape {
            'n' => decoded.push(0x0A),
            't' => decoded.push(0x09),
            'r' => decoded.push(0x0D),
            'b' => decoded.push(0x08),
            'f' => decoded.push(0x0C),
            'u' => {
                let code: String = chars.by_ref().take(4).collect();
                decoded.push(u16::from_str_radix(&code, 16).expect("hexadecimal"));
            }
            other => {
                let mut buffer = [0u16; 2];
                decoded.extend_from_slice(other.encode_utf16(&mut buffer));
            }
        }
    }
    decoded
}

/// O programa de uma linha do golden: as unidades UTF-16 exatas (surrogate solitário incluído) que o avaliador recebe, e
/// o texto com perda (U+FFFD no lugar do surrogate solitário) só para mensagens e para os testes que o leem como `&str`.
#[derive(Debug, Clone)]
pub struct Program {
    pub units: Vec<u16>,
    text: String,
}

impl Program {
    pub fn new(units: Vec<u16>) -> Program {
        let text = String::from_utf16_lossy(&units);
        Program { units, text }
    }
}

impl Deref for Program {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

impl std::fmt::Display for Program {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.text)
    }
}

/// Fonte de programa que os avaliadores do `common` aceitam: `&str`/`String` (convertidos sem perda) ou [`Program`].
pub trait ProgramSource {
    fn units(&self) -> Cow<'_, [u16]>;
}

impl ProgramSource for str {
    fn units(&self) -> Cow<'_, [u16]> {
        Cow::Owned(self.encode_utf16().collect())
    }
}

impl ProgramSource for String {
    fn units(&self) -> Cow<'_, [u16]> {
        self.as_str().units()
    }
}

impl ProgramSource for Program {
    fn units(&self) -> Cow<'_, [u16]> {
        Cow::Borrowed(&self.units)
    }
}

impl<T: ProgramSource + ?Sized> ProgramSource for &T {
    fn units(&self) -> Cow<'_, [u16]> {
        (**self).units()
    }
}

/// `json_units` como `String`, para os testes que comparam texto sem surrogate solitário: ele vira U+FFFD (com perda).
pub fn json_string(literal: &str) -> String {
    String::from_utf16_lossy(&json_units(literal))
}

/// Decodifica um array JSON de literais de string (`["a","b"]`), como na quarta coluna dos goldens com alternativas.
/// Os literais são emitidos por `JSON.stringify`, então `"` e `\` internos vêm escapados e a divisão respeita isso.
pub fn json_string_array(array: &str) -> Option<Vec<Vec<u16>>> {
    let inner = array.strip_prefix('[')?.strip_suffix(']')?;
    let mut literals = Vec::new();
    let mut start = None;
    let mut escaped = false;
    for (index, character) in inner.char_indices() {
        match (start, character) {
            (None, '"') => start = Some(index),
            (None, ',') => {}
            (None, other) if other.is_whitespace() => {}
            (None, _) => return None,
            (Some(_), _) if escaped => escaped = false,
            (Some(_), '\\') => escaped = true,
            (Some(begin), '"') => {
                literals.push(json_units(&inner[begin..=index]));
                start = None;
            }
            (Some(_), _) => {}
        }
    }
    start.is_none().then_some(literals)
}

/// Roda a avaliação e devolve o texto do valor (`<undefined>` quando não é string), ou o motivo de não ter devolvido.
/// Com perda: surrogate solitário vira U+FFFD. Os goldens usam `guarded_units`.
pub fn guarded(evaluate: impl FnOnce() -> Result<JSValue, JSValue>) -> Result<String, String> {
    guarded_units(evaluate).map(|units| String::from_utf16_lossy(&units.0))
}

/// `guarded` sem perda: o texto do valor como as unidades de código do `JSString`.
pub fn guarded_units(evaluate: impl FnOnce() -> Result<JSValue, JSValue>) -> Result<Units, String> {
    match catch_unwind(AssertUnwindSafe(evaluate)) {
        Ok(Ok(value)) if value.is_undefined() => Ok(Units::from("<undefined>")),
        Ok(Ok(value)) => Ok(Units(value.to_wtf_string().characters_without_null_termination().expect("unidades UTF-16 do resultado"))),
        Ok(Err(_)) => Err("o programa lançou exceção".to_string()),
        Err(panic) => {
            let reason = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
                .unwrap_or_default();
            Err(format!("pânico: {reason}"))
        }
    }
}

/// Pilha da thread dos goldens de recursão funda: o orçamento padrão do VM (1 MiB) estoura em `apply` gigante e em
/// recursão de milhares de níveis, que o bun aceita.
const THREAD_STACK_BYTES: usize = 256 * 1024 * 1024;

/// Como o gerador do golden avaliou o programa no bun; o teste tem de avaliá-lo do mesmo jeito, porque `var`, `function`,
/// `let` e `const` no topo mudam entre script global e eval indireto. O modo é escolhido no fechamento que o teste passa a
/// `run_golden`/`run_factored`/`check` (o prelúdio e o sufixo já chegam juntos em `source`, como o gerador os concatena).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalMode {
    /// `vm.runInThisContext` (script global), arquivo gravado e executado: `Program` do porte, esvazia as microtarefas e
    /// lê a global `result_name`.
    Script,
    /// `(0, eval)(source)` e leitura imediata de `result_name` (o filho que lê o programa do stdin e imprime logo, ou o
    /// próprio gerador).
    IndirectEval,
    /// `(0, eval)(source)`, `setTimeout(0)` do driver (microtarefas esvaziam) e só então a leitura de `result_name`.
    IndirectEvalDrained,
    /// `vm.runInThisContext(fonte, { filename: url })` chamado de um arquivo CJS (`case.js`), como os geradores que gravam
    /// `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(...), { filename }) } catch (e) {}`: a pilha
    /// leva as frames do hospedeiro (`runInThisContext (unknown)`, `<anonymous> (case.js:1:26)`) e as colunas cruas. `url`
    /// vazio é a chamada sem a opção `filename` (o nome é `file:///`).
    RunInThisContext,
}

impl EvalMode {
    /// Avalia o programa no modo e devolve o valor da global `result_name`. `url` só vale no modo `Script`: o eval
    /// indireto não tem nome de arquivo.
    pub fn evaluate(self, source: impl ProgramSource, url: &str, result_name: &str) -> Result<JSValue, JSValue> {
        let source = source.units();
        let source = source.as_ref();
        match self {
            EvalMode::Script => evaluate_named_script_result_units(source, url, result_name),
            EvalMode::IndirectEval => evaluate_indirect_eval_result_units(source, result_name, false),
            EvalMode::IndirectEvalDrained => evaluate_indirect_eval_result_units(source, result_name, true),
            EvalMode::RunInThisContext => evaluate_run_in_this_context_units(source, Some(url).filter(|name| !name.is_empty()), result_name),
        }
    }
}

/// Lista de prelúdios de um golden sem prelúdio: um único, vazio.
pub const NO_PRELUDES: &str = "[\"\"]";

/// Uma entrada do `tests/golden/<nome>.preludes.json`: o texto do prelúdio e, nos goldens mapeados, as corridas do mapa de
/// posições que caem nele (ver [`ProgramMeta::parse_factored`]).
#[derive(Debug, Default, Clone)]
pub struct Prelude {
    pub text: String,
    /// `text` em unidades UTF-16 exatas (o `text` troca o surrogate solitário por U+FFFD).
    pub units: Vec<u16>,
    /// Maior prefixo comum das corridas das linhas do grupo, em coordenadas absolutas do programa completo.
    pub runs: Vec<i64>,
    /// O que falta de cada variante das corridas do prelúdio depois de `runs`; a quinta coluna escolhe uma pelo índice.
    pub tails: Vec<Vec<i64>>,
}

impl Prelude {
    /// Um prelúdio só de texto (sem corridas), a partir das unidades UTF-16 exatas.
    fn from_text_units(units: Vec<u16>) -> Prelude {
        Prelude { text: String::from_utf16_lossy(&units), units, ..Prelude::default() }
    }
}

/// Divide o miolo de um array ou objeto JSON nas vírgulas de nível zero, sem olhar dentro de strings, colchetes e chaves.
fn split_top_level(inner: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start, mut in_string, mut escaped) = (0usize, 0usize, false, false);
    for (index, character) in inner.char_indices() {
        match character {
            _ if escaped => escaped = false,
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '[' | '{' if !in_string => depth += 1,
            ']' | '}' if !in_string => depth -= 1,
            ',' if !in_string && depth == 0 => {
                parts.push(inner[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    if !inner[start..].trim().is_empty() {
        parts.push(inner[start..].trim());
    }
    parts
}

fn strip_brackets<'a>(text: &'a str, open: char, close: char) -> &'a str {
    text.trim().strip_prefix(open).and_then(|rest| rest.strip_suffix(close)).unwrap_or_else(|| panic!("preludes.json: esperava {open}...{close}"))
}

/// Decodifica o `tests/golden/<nome>.preludes.json` na ordem dos índices da terceira coluna. Cada entrada é uma string (só o
/// texto) ou `{"text":...,"runs":[...],"tails":[[...],...]}`.
pub fn parse_preludes(json: &str) -> Vec<Prelude> {
    split_top_level(strip_brackets(json, '[', ']'))
        .into_iter()
        .map(|entry| {
            if entry.starts_with('"') {
                return Prelude::from_text_units(json_units(entry));
            }
            let mut prelude = Prelude::default();
            for member in split_top_level(strip_brackets(entry, '{', '}')) {
                let (key, value) = member.split_once(':').expect("preludes.json: membro chave:valor");
                match key.trim() {
                    "\"text\"" => {
                        let units = json_units(value.trim());
                        prelude.text = String::from_utf16_lossy(&units);
                        prelude.units = units;
                    }
                    "\"runs\"" => prelude.runs = parse_integer_array(value).expect("preludes.json: runs"),
                    "\"tails\"" => {
                        prelude.tails = split_top_level(strip_brackets(value, '[', ']')).into_iter().map(|tail| parse_integer_array(tail).expect("preludes.json: tail")).collect()
                    }
                    other => panic!("preludes.json: chave desconhecida {other}"),
                }
            }
            prelude
        })
        .collect()
}

/// Só os textos dos prelúdios de [`parse_preludes`].
pub fn preludes_from_json(json: &str) -> Vec<String> {
    parse_preludes(json).into_iter().map(|prelude| prelude.text).collect()
}

/// Golden sem prelúdio: cada linha é `JSON(programa)<TAB>JSON(resultado)`; `evaluate` avalia o programa.
pub fn run_golden(tsv: &str, min_total: usize, evaluate: impl Fn(&Program) -> Result<JSValue, JSValue>) {
    run_factored(tsv, NO_PRELUDES, min_total, evaluate);
}

/// Golden com prelúdio fatorado (ver o topo do módulo): `evaluate` avalia o programa completo (prelúdio + sufixo).
pub fn run_factored(tsv: &str, preludes: &str, min_total: usize, evaluate: impl Fn(&Program) -> Result<JSValue, JSValue>) {
    check(tsv, preludes, min_total, |source| guarded_units(|| evaluate(source)));
}

/// Igual a `run_golden`, numa thread de pilha grande com o orçamento do VM ajustado a ela.
pub fn run_golden_big_stack(tsv: &'static str, min_total: usize, evaluate: impl Fn(&Program) -> Result<JSValue, JSValue> + Send + 'static) {
    run_factored_big_stack(tsv, NO_PRELUDES, min_total, evaluate);
}

/// Igual a `run_factored`, numa thread de pilha grande com o orçamento do VM ajustado a ela.
pub fn run_factored_big_stack(tsv: &'static str, preludes: &'static str, min_total: usize, evaluate: impl Fn(&Program) -> Result<JSValue, JSValue> + Send + 'static) {
    on_big_stack(move || run_factored(tsv, preludes, min_total, evaluate));
}

/// Golden com a quinta coluna (modo ESM/CJS e mapa de posições, ver [`ProgramMeta`]): `url` é o nome do arquivo dos dois
/// lados (`__filename` do CJS e o que o `stack` mostra) e o programa grava o resultado em `globalThis.R`.
pub fn run_mapped_golden(tsv: &str, preludes: &str, min_total: usize, url: &str) {
    check_with_meta(tsv, preludes, min_total, |source, meta| guarded_units(|| evaluate_golden_program(source, meta, url, "R")));
}

/// `run_mapped_golden` numa thread de pilha grande.
pub fn run_mapped_golden_big_stack(tsv: &'static str, preludes: &'static str, min_total: usize, url: &'static str) {
    on_big_stack(move || run_mapped_golden(tsv, preludes, min_total, url));
}

fn on_big_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(move || {
            // Folga de 16 MiB para os protetores nativos; sem isto o orçamento seria o padrão de 1 MiB.
            zjsc::runtime::vm::VM::set_thread_stack_budget(THREAD_STACK_BYTES - 16 * 1024 * 1024);
            body();
        })
        .expect("thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}

/// Compara cada programa do golden com o resultado do bun; `run` avalia o programa completo (prelúdio + sufixo).
/// O resultado do motor (`String` ou `Units`) é comparado em UTF-16, sem perda.
pub fn check<R: Into<Units>>(tsv: &str, preludes: &str, min_total: usize, run: impl Fn(&Program) -> Result<R, String>) {
    check_with_meta(tsv, preludes, min_total, |source, _| run(source));
}

/// O que a quinta coluna do tsv diz sobre como o bun rodou o programa: `JSON([modo, linha, coluna, dlinha, dcoluna, ...])`.
/// `modo` 0 é ESM (o programa é o texto do transpilador e roda como script), 1 é CJS e 2 é CJS estrito (o programa é o
/// corpo do wrapper do runtime do bun, com `"use strict";\n` na frente no estrito). O resto é o mapa de posições do texto
/// gravado para o fonte original (`zjsc::parser::position_map`); vazio quando as posições coincidem. Linha sem a quinta
/// coluna (golden antigo): `Default`, ESM sem mapa.
#[derive(Debug, Default, Clone)]
pub struct ProgramMeta {
    pub mode: i64,
    pub position_runs: Vec<i64>,
}

impl ProgramMeta {
    pub fn parse(column: &str) -> ProgramMeta {
        let numbers = parse_integer_array(column).expect("quinta coluna: array JSON de inteiros");
        let (mode, runs) = numbers.split_first().expect("quinta coluna sem o modo");
        ProgramMeta { mode: *mode, position_runs: runs.to_vec() }
    }

    /// A quinta coluna fatorada, `JSON([modo, índiceDaCauda, ...corridasDoSufixo])`: as corridas do programa completo são
    /// `prelude.runs ++ prelude.tails[índiceDaCauda] ++ corridasDoSufixo` (índice -1: a linha não tem corrida nenhuma). A
    /// fatoração é feita por `factorMeta` em `scripts/golden-prelude.js` e o resultado é idêntico à lista original.
    pub fn parse_factored(column: &str, prelude: &Prelude) -> ProgramMeta {
        let numbers = parse_integer_array(column).expect("quinta coluna: array JSON de inteiros");
        let [mode, tail, rest @ ..] = numbers.as_slice() else { panic!("quinta coluna sem o modo e o índice da cauda") };
        let mut position_runs = Vec::new();
        if let Ok(tail) = usize::try_from(*tail) {
            position_runs.extend_from_slice(&prelude.runs);
            position_runs.extend_from_slice(prelude.tails.get(tail).expect("índice da cauda fora do prelúdio"));
        }
        position_runs.extend_from_slice(rest);
        ProgramMeta { mode: *mode, position_runs }
    }

    pub fn is_cjs(&self) -> bool {
        self.mode != 0
    }
}

/// Avalia o programa do golden como o bun o rodou (`meta.mode`) e devolve o valor da global `result_name`, depois das
/// microtarefas: ESM como script com o mapa de posições, CJS no wrapper de `evaluate_cjs_program`.
pub fn evaluate_golden_program(source: impl ProgramSource, meta: &ProgramMeta, url: &str, result_name: &str) -> Result<JSValue, JSValue> {
    let source = source.units();
    if meta.is_cjs() {
        evaluate_cjs_program_units(&source, url, result_name, &meta.position_runs)
    } else {
        evaluate_mapped_script_result_units(&source, url, result_name, &meta.position_runs)
    }
}

/// `check` com a quinta coluna: `run` recebe o programa completo e o `ProgramMeta` da linha.
pub fn check_with_meta<R: Into<Units>>(tsv: &str, preludes: &str, min_total: usize, run: impl Fn(&Program, &ProgramMeta) -> Result<R, String>) {
    let preludes = parse_preludes(preludes);
    let mut failures = Vec::new();
    let mut total = 0;
    // GOLDEN_LINES=a-b restringe a medição às linhas a..=b do tsv (1-based), para isolar caso lento ou que estoura.
    let range = std::env::var("GOLDEN_LINES").ok().map(|spec| {
        let (start, end) = spec.split_once('-').expect("GOLDEN_LINES=a-b");
        (start.parse::<usize>().expect("início"), end.parse::<usize>().expect("fim"))
    });
    let selected = tsv.lines().enumerate().filter(|(index, _)| range.is_none_or(|(start, end)| (start..=end).contains(&(index + 1))));
    for line in selected.map(|(_, line)| line).filter(|line| !line.is_empty()) {
        let mut columns = line.split('\t');
        let suffix = json_units(columns.next().expect("sufixo"));
        let expected = json_units(columns.next().expect("resultado"));
        let prelude = columns.next().map_or(0, |index| index.parse::<usize>().expect("índice do prelúdio"));
        // Quarta coluna: outras saídas que o bun também produz (não determinismo medido pelo gerador).
        let alternatives: Vec<Vec<u16>> = columns.next().map_or_else(Vec::new, |array| json_string_array(array).expect("alternativas JSON"));
        let meta = columns.next().map_or_else(ProgramMeta::default, |column| ProgramMeta::parse_factored(column, &preludes[prelude]));
        let source = Program::new([preludes[prelude].units.as_slice(), suffix.as_slice()].concat());
        total += 1;
        match run(&source, &meta) {
            Ok(actual) => {
                let actual: Units = actual.into();
                if actual.0 != expected && !alternatives.contains(&actual.0) {
                    failures.push(format!("{source}\n    esperado {}\n    veio     {}", escape_units(&expected), escape_units(&actual.0)));
                }
            }
            Err(reason) => failures.push(format!("{source}\n    esperado {}\n    {reason}", escape_units(&expected))),
        }
    }
    assert!(range.is_some() || total >= min_total, "golden com só {total} programas");
    assert!(failures.is_empty(), "{} de {} divergem do bun:\n{}", failures.len(), total, failures.join("\n"));
}

/// Uma linha dos goldens de exceção não capturada (`uncaught_bun.tsv`, `post_message_uncaught_bun.tsv`, escritos por
/// `scripts/uncaught-run.js`): `JSON(fonte)<TAB>hex(stderr)<TAB>código de saída[<TAB>hex(stdout)]`, o fonte rodado como
/// `/app/main.js`. A quarta coluna é opcional; quando existe, o stdout também é comparado. Uma quinta (`1`/`0`) diz se o
/// bun ainda estava vivo ao estourar o limite de tempo, isto é, se o laço de eventos ficou segurado (e então o código
/// é `signal:...` e não se compara); sem ela, o laço tem de terminar.
pub struct MainScriptRow {
    pub source: String,
    expected_stderr: Vec<u8>,
    expected_code: Option<i32>,
    expected_stdout: Option<Vec<u8>>,
    expected_held: bool,
}

fn decode_hex(hex: &str) -> Vec<u8> {
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex")).collect()
}

impl MainScriptRow {
    pub fn parse(line: &str) -> MainScriptRow {
        let mut columns = line.split('\t');
        let source = json_string(columns.next().expect("fonte"));
        let expected_stderr = decode_hex(columns.next().expect("stderr"));
        let code_column = columns.next().expect("código");
        let expected_code = if code_column.starts_with("signal:") { None } else { Some(code_column.parse().expect("código numérico")) };
        let expected_stdout = columns.next().map(decode_hex);
        let expected_held = columns.next() == Some("1");
        MainScriptRow { source, expected_stderr, expected_code, expected_stdout, expected_held }
    }

    /// Roda o fonte como programa principal e devolve a descrição da divergência (código de saída ou stderr), se houver.
    /// Precisa estar numa thread de pilha grande (`run_with_stack`) com o orçamento do VM ajustado.
    pub fn check(&self, label: &str) -> Option<String> {
        let console = std::rc::Rc::new(zjsc::runtime::console_host::MemoryConsole::new(b""));
        let (code, held) = zjsc::api::eval::evaluate_main_script_reporting_hold(&self.source, "/app/main.js", console.clone());
        let stderr = console.stderr_bytes();
        let stdout = console.stdout_bytes();
        let stdout_differs = self.expected_stdout.as_ref().is_some_and(|expected| *expected != stdout);
        let code_differs = self.expected_code.is_some_and(|expected| expected != code);
        (code_differs || stderr != self.expected_stderr || stdout_differs || held != self.expected_held).then(|| {
            format!(
                "[{label}] {:?}\n  esperado: código {:?} laço segurado {} {:?}\n  obtido:   código {code} laço segurado {held} {:?}\n  stdout esperado {:?}\n  stdout obtido   {:?}",
                self.source,
                self.expected_code,
                self.expected_held,
                String::from_utf8_lossy(&self.expected_stderr),
                String::from_utf8_lossy(&stderr),
                self.expected_stdout.as_deref().map(String::from_utf8_lossy),
                String::from_utf8_lossy(&stdout)
            )
        })
    }
}

/// Roda `body` numa thread própria com pilha de `stack_bytes`: a pilha padrão das threads de teste (2 MiB) não cobre a
/// recursão nativa do motor. Pânico do corpo é repropagado, então a mensagem da asserção chega ao `cargo test`.
pub fn run_with_stack<F: FnOnce() + Send + 'static>(stack_bytes: usize, body: F) {
    std::thread::Builder::new()
        .stack_size(stack_bytes)
        .spawn(body)
        .expect("thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
}
