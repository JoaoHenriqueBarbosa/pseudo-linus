//! Montagem do jaq (`jaq-core` + `jaq-std` + `jaq-json`) com a camada nossa:
//!
//! - um `DataT` próprio cujo `HasLut::lut()` passa pelo [`Hook`] de checkpoint: o avaliador do jaq
//!   chama `lut()` a cada nó avaliado (`Id::run`, `paths`, `update` e cada nativa), então esse é um
//!   ponto de preempção e de kill sem fork;
//! - nativas sobrescritas (o compilador do jaq resolve a primeira nativa com o nome, então as nossas
//!   vêm antes): `tojson`, `fromjson`, `env`, `now`, `input`, `inputs`, `input_filename`,
//!   `input_line_number`, `stderr_empty`, `debug_empty` e, no modo de sondagem, `range/3`;
//! - definições jq sobrescritas (as do fim da lista ganham): `tostring`, `@text`, `halt_error`.

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use jaq_core::box_iter::box_once;
use jaq_core::compile::Compiler;
use jaq_core::data::HasLut;
use jaq_core::load::{Arena, File, Loader};
use jaq_core::native::{Fun, v};
use jaq_core::{DataT, Error, Exn, Lut, ValX};
use jaq_json::{Num, Val};

use super::input::InputState;
use super::json::{self, DumpOpts};

/// Payload do unwind de interrupção (o mesmo mecanismo que o kernel usaria pro SIGKILL).
#[derive(Debug)]
pub struct Interrupted;

/// Ponto de checkpoint chamado pelo avaliador do jaq a cada nó.
#[derive(Default)]
pub struct Hook {
    pub calls: Cell<u64>,
    /// Quando ligado (por outra thread), o próximo checkpoint desenrola a pilha com [`Interrupted`].
    pub interrupt: Option<Arc<AtomicBool>>,
    /// Instante em que o checkpoint viu a interrupção.
    pub interrupted_at: Cell<Option<Instant>>,
    /// Mede o maior intervalo entre dois checkpoints (custa um `Instant::now()` por chamada).
    pub timing: bool,
    pub last: Cell<Option<Instant>>,
    pub max_gap_ns: Cell<u64>,
    /// Cópia das estatísticas que sobrevive ao desenrolar da pilha (usada nas sondagens).
    pub shared: Option<Arc<HookStats>>,
}

#[derive(Debug, Default)]
pub struct HookStats {
    pub calls: AtomicU64,
    pub max_gap_ns: AtomicU64,
}

impl Hook {
    #[inline]
    pub fn check(&self) {
        self.calls.set(self.calls.get() + 1);
        if self.timing {
            let now = Instant::now();
            if let Some(prev) = self.last.get() {
                let gap = now.duration_since(prev).as_nanos() as u64;
                if gap > self.max_gap_ns.get() {
                    self.max_gap_ns.set(gap);
                }
            }
            self.last.set(Some(now));
            if let Some(s) = &self.shared {
                s.calls.store(self.calls.get(), Ordering::Relaxed);
                s.max_gap_ns.store(self.max_gap_ns.get(), Ordering::Relaxed);
            }
        }
        if let Some(flag) = &self.interrupt
            && flag.load(Ordering::Relaxed)
        {
            self.interrupted_at.set(Some(Instant::now()));
            std::panic::resume_unwind(Box::new(Interrupted));
        }
    }
}

/// Estado de uma execução, alcançável pelas nativas via `cv.0.data()`.
pub struct Runtime {
    pub stderr: RefCell<Vec<u8>>,
    pub env_obj: Val,
    /// Relógio injetado (faketime); `None` usa o relógio real.
    pub now: Option<f64>,
    pub input: RefCell<InputState>,
    pub hook: Hook,
}

pub struct JqKind;

impl DataT for JqKind {
    type V<'a> = Val;
    type Data<'a> = &'a Data<'a>;
}

pub struct Data<'a> {
    pub lut: &'a Lut<JqKind>,
    pub rt: &'a Runtime,
}

impl<'a> HasLut<'a, JqKind> for &'a Data<'a> {
    fn lut(&self) -> &'a Lut<JqKind> {
        self.rt.hook.check();
        self.lut
    }
}

pub type Filter = jaq_core::Filter<JqKind>;

#[derive(Clone, Copy, Default)]
pub struct EngineOpts {
    /// Troca a nativa `range/3` por uma que passa pelo checkpoint a cada elemento.
    pub checkpoint_range: bool,
}

fn err<'a>(msg: impl ToString) -> ValX<'a, Val> {
    Err(Exn::from(Error::str(msg.to_string())))
}

fn rt<'a>(cv: &jaq_core::Cv<'a, JqKind>) -> &'a Runtime {
    cv.0.data().rt
}

fn our_funs(opts: EngineOpts) -> Vec<Fun<JqKind>> {
    let run = jaq_core::native::run::<JqKind>;
    let mut funs: Vec<Fun<JqKind>> = vec![
        run(("tojson", v(0), |cv| box_once(Ok(Val::utf8_str(json::dump(&cv.1, &DumpOpts::COMPACT)))))),
        run(("fromjson", v(0), |cv| {
            let r = match &cv.1 {
                Val::TStr(b) | Val::BStr(b) => json::parse_single(b).map_err(|e| Exn::from(Error::str(e))),
                other => Err(Exn::from(Error::str(format!(
                    "{} ({}) cannot be parsed as JSON",
                    json::type_name(other),
                    json::dump_trunc(other)
                )))),
            };
            box_once(r)
        })),
        run(("_jq_trunc", v(0), |cv| box_once(Ok(Val::utf8_str(json::dump_trunc(&cv.1)))))),
        run(("env", v(0), |cv| box_once(Ok(rt(&cv).env_obj.clone())))),
        run(("now", v(0), |cv| {
            let t = rt(&cv).now.unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0)
            });
            box_once(Ok(Val::from(t)))
        })),
        run(("input", v(0), |cv| {
            let r = rt(&cv).input.borrow_mut().next_value();
            box_once(match r {
                Some(Ok(v)) => Ok(v),
                Some(Err(e)) => err(e),
                // O jq 1.7.1 do Debian reporta "break" quando as entradas acabam.
                None => err("break"),
            })
        })),
        run(("inputs", v(0), |cv| {
            let rt = rt(&cv);
            Box::new(std::iter::from_fn(move || match rt.input.borrow_mut().next_value() {
                Some(Ok(v)) => Some(Ok(v)),
                Some(Err(e)) => Some(err(e)),
                None => None,
            }))
        })),
        run(("input_filename", v(0), |cv| {
            let name = rt(&cv).input.borrow().current_filename();
            box_once(Ok(name.map(Val::utf8_str).unwrap_or(Val::Null)))
        })),
        run(("input_line_number", v(0), |cv| {
            let line = rt(&cv).input.borrow().current_line();
            box_once(Ok(Val::Num(Num::Int(line as isize))))
        })),
        run(("stderr_empty", v(0), |cv| {
            let rt = rt(&cv);
            match &cv.1 {
                Val::TStr(b) | Val::BStr(b) => rt.stderr.borrow_mut().extend_from_slice(b),
                other => rt.stderr.borrow_mut().extend_from_slice(json::dump(other, &DumpOpts::COMPACT).as_bytes()),
            }
            Box::new(std::iter::empty())
        })),
        run(("debug_empty", v(0), |cv| {
            let rt = rt(&cv);
            let line = format!("[\"DEBUG:\",{}]\n", json::dump(&cv.1, &DumpOpts::COMPACT));
            rt.stderr.borrow_mut().extend_from_slice(line.as_bytes());
            Box::new(std::iter::empty())
        })),
        run(("_jq_halt_error_msg", v(0), |cv| {
            let rt = rt(&cv);
            match &cv.1 {
                Val::TStr(b) | Val::BStr(b) => rt.stderr.borrow_mut().extend_from_slice(b),
                Val::Null => {}
                other => {
                    let s = format!("{}\n", json::dump(other, &DumpOpts::COMPACT));
                    rt.stderr.borrow_mut().extend_from_slice(s.as_bytes());
                }
            }
            Box::new(std::iter::empty())
        })),
    ];
    if opts.checkpoint_range {
        funs.push(run(("range", v(3), |mut cv| {
            let by = cv.0.pop_var();
            let to = cv.0.pop_var();
            let from = cv.0.pop_var();
            let rt = rt(&cv);
            let zero = Val::Num(Num::Int(0));
            let mut cur = Some(from);
            Box::new(std::iter::from_fn(move || {
                rt.hook.check();
                let x = cur.take()?;
                let go = if by > zero {
                    x < to
                } else if by < zero {
                    x > to
                } else {
                    x < to
                };
                if !go {
                    return None;
                }
                match x.clone() + by.clone() {
                    Ok(next) => cur = Some(next),
                    Err(e) => return Some(Err(Exn::from(e))),
                }
                Some(Ok(x))
            }))
        })));
    }
    funs
}

/// Definições jq nossas, avaliadas depois das do jaq (as últimas ganham). Onde o jaq diverge do jq
/// numa função que o jq define em jq (`src/builtin.jq` da 1.7.1, licença MIT), usamos a definição do
/// jq; `@csv`, `@tsv` e `@base32` reproduzem o C do jq (o jaq não tem os dois primeiros e o jq do
/// Debian não tem o `@base32`).
const OUR_DEFS: &str = r#"
def tostring: if type == "string" then . else tojson end;
def @text: tostring;
def @json: tojson;
def halt_error($exit_code): _jq_halt_error_msg, halt($exit_code);
def halt_error: halt_error(5);
def _jq_type_error($msg): error("\(type) (\(_jq_trunc)) \($msg)");
def _jq_join_strs($sep): reduce .[] as $s (null; if . == null then $s else . + $sep + $s end) // "";
def @csv: if type != "array" then _jq_type_error("cannot be csv-formatted, only array")
  else map(if type == "null" then "" elif type == "boolean" then tojson
    elif type == "number" then (if isnan then "" else tojson end)
    elif type == "string" then "\"" + (split("\"") | _jq_join_strs("\"\"")) + "\""
    else _jq_type_error("is not valid in a csv row") end) | _jq_join_strs(",") end;
def @tsv: if type != "array" then _jq_type_error("cannot be tsv-formatted, only array")
  else map(if type == "null" then "" elif type == "boolean" then tojson
    elif type == "number" then (if isnan then "" else tojson end)
    elif type == "string" then (split("\\") | _jq_join_strs("\\\\") | split("\t") | _jq_join_strs("\\t")
      | split("\r") | _jq_join_strs("\\r") | split("\n") | _jq_join_strs("\\n"))
    else _jq_type_error("is not valid in a csv row") end) | _jq_join_strs("\t") end;
def @base32: error("base32 is not a valid format");
def @base32d: error("base32d is not a valid format");
def from_entries: reduce .[] as $x ({};
  ($x | .key // .Key // .name // .Name) as $k
  | if ($k | type) != "string" then error("Cannot use \($k | type) (\($k | _jq_trunc)) as object key")
    else . + {($k): ($x | if has("value") then .value else .Value end)} end);
def join($x): reduce .[] as $i (null;
  (if . == null then "" else . + $x end) +
  ($i | if type == "boolean" or type == "number" then tostring else . // "" end)) // "";
def limit($n; f): if $n > 0 then label $out | foreach f as $item ($n; . - 1; $item, if . <= 0 then break $out else empty end)
  elif $n == 0 then empty else f end;
def ltrimstr($x): if type == "string" and ($x | type) == "string" and startswith($x) then .[($x | length):] else . end;
def rtrimstr($x): if type == "string" and ($x | type) == "string" and endswith($x) then .[:length - ($x | length)] else . end;
def scan($re; $flags): match($re; "g" + $flags) | if (.captures | length > 0) then [.captures | .[] | .string] else .string end;
def scan($re): scan($re; null);
def _nwise($n): def n: if length <= $n then . else .[0:$n], (.[$n:] | n) end; n;
def splits($re; flags): . as $s | [match($re; "g" + flags) | (.offset, .offset + .length)]
  | [0] + . + [$s | length] | _nwise(2) | $s[.[0]:.[1]];
def splits($re): splits($re; null);
def split($re; flags): [splits($re; flags)];
def tostream: path(def r: (.[]? | r), .; r) as $p | getpath($p) | reduce path(.[]?) as $q ([$p, .]; [$p + $q]);
def truncate_stream(stream): . as $n | null | stream | . as $input
  | if (.[0] | length) > $n then setpath([0]; $input[0][$n:]) else empty end;
def gamma: lgamma;
def todate: strftime("%Y-%m-%dT%H:%M:%SZ");
def todateiso8601: strftime("%Y-%m-%dT%H:%M:%SZ");
def first(g): label $out | g | ., break $out;
def last(g): reduce g as $item (null; $item);
def nth($n; g): if $n < 0 then error("nth doesn't support negative indices")
  else label $out | foreach g as $item ($n + 1; . - 1; if . <= 0 then $item, break $out else empty end) end;
def isempty(g): first((g | false), true);
def INDEX(stream; idx_expr): reduce stream as $row ({}; .[$row | idx_expr | tostring] = $row);
def INDEX(idx_expr): INDEX(.[]; idx_expr);
def JOIN($idx; idx_expr): [.[] | [., $idx[idx_expr]]];
def JOIN($idx; stream; idx_expr): stream | [., $idx[idx_expr]];
def JOIN($idx; stream; idx_expr; join_expr): stream | [., $idx[idx_expr]] | join_expr;
def IN(s): any(s == .; .);
def IN(src; s): any(src == s; .);
def format($f): if $f == "text" then @text elif $f == "json" then @json elif $f == "csv" then @csv
  elif $f == "tsv" then @tsv elif $f == "html" then @html elif $f == "uri" then @uri elif $f == "sh" then @sh
  elif $f == "base64" then @base64 elif $f == "base64d" then @base64d
  else error("\($f) is not a valid format") end;
"#;

/// Erro de compilação já formatado como o jq faria (sem o prefixo "jq: error: ").
pub struct CompileError {
    pub messages: Vec<String>,
}

/// Compila um programa com as variáveis globais dadas (nomes sem `$`).
pub fn compile(program: &str, globals: &[String], opts: EngineOpts) -> Result<Filter, CompileError> {
    let program = substitute_loc(program);
    let defs = jaq_core::defs()
        .chain(jaq_std::defs())
        .chain(jaq_json::defs())
        .chain(jaq_core::load::parse(OUR_DEFS, |p| p.defs()).expect("defs nossas válidas"));
    let funs = our_funs(opts)
        .into_iter()
        .chain(jaq_core::funs::<JqKind>())
        .chain(jaq_std::funs::<JqKind>())
        .chain(jaq_json::funs::<JqKind>());
    let loader = Loader::new(defs);
    let arena = Arena::default();
    let file = File { code: program.as_str(), path: () };
    let modules = match loader.load(&arena, file) {
        Ok(m) => m,
        Err(errs) => {
            let mut messages = Vec::new();
            for (_file, e) in errs {
                match e {
                    jaq_core::load::Error::Io(v) => {
                        for (p, m) in v {
                            messages.push(locate(&program, p, &m.to_string()));
                        }
                    }
                    jaq_core::load::Error::Lex(v) => {
                        for (_expect, at) in v {
                            messages.push(locate(&program, at, &syntax_msg(Some(at))));
                        }
                    }
                    jaq_core::load::Error::Parse(v) => {
                        for (_expect, found) in v {
                            messages.push(locate(&program, found, &syntax_msg(Some(found))));
                        }
                    }
                }
            }
            return Err(CompileError { messages });
        }
    };
    let global_names: Vec<String> = globals.iter().map(|g| format!("${g}")).collect();
    let compiler = Compiler::default()
        .with_funs(funs)
        .with_global_vars(global_names.iter().map(String::as_str));
    match compiler.compile(modules) {
        Ok(f) => Ok(f),
        Err(errs) => {
            let mut messages = Vec::new();
            for (_file, list) in errs {
                for (name, undef) in list {
                    let what = match undef {
                        jaq_core::compile::Undefined::Filter(arity) => format!("{name}/{arity} is not defined"),
                        jaq_core::compile::Undefined::Var => format!("{name} is not defined"),
                        jaq_core::compile::Undefined::Label => format!("$*label-{name} is not defined"),
                        jaq_core::compile::Undefined::Mod => format!("module {name} not found"),
                        _ => format!("{name} is not defined"),
                    };
                    messages.push(locate(&program, name, &what));
                }
            }
            Err(CompileError { messages })
        }
    }
}

/// Nomes "nome/aridade" que um programa enxerga: definições jq e nativas (jaq puro ou com a camada).
pub fn builtin_names(with_layer: bool) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let ours = jaq_core::load::parse(OUR_DEFS, |p| p.defs()).expect("defs nossas válidas");
    let defs = jaq_core::defs()
        .chain(jaq_std::defs())
        .chain(jaq_json::defs())
        .chain(if with_layer { ours } else { Vec::new() });
    for d in defs {
        out.insert(format!("{}/{}", d.name, d.args.len()));
    }
    let layer = if with_layer { our_funs(EngineOpts::default()) } else { Vec::new() };
    let funs = layer
        .into_iter()
        .chain(jaq_core::funs::<JqKind>())
        .chain(jaq_std::funs::<JqKind>())
        .chain(jaq_json::funs::<JqKind>());
    for (name, args, _) in funs {
        out.insert(format!("{name}/{}", args.len()));
    }
    out.retain(|n| !n.starts_with('_') && !n.starts_with('@'));
    out
}

/// Avalia um programa no jaq puro (`JustLut`, sem nativas nossas nem gancho), pra linha de base de custo.
pub fn baseline_eval(program: &str) -> std::time::Duration {
    use jaq_core::data::JustLut;
    type D = JustLut<Val>;
    let defs = jaq_core::defs().chain(jaq_std::defs()).chain(jaq_json::defs());
    let funs = jaq_core::funs::<D>().chain(jaq_std::funs::<D>()).chain(jaq_json::funs::<D>());
    let loader = Loader::new(defs);
    let arena = Arena::default();
    let Ok(modules) = loader.load(&arena, File { code: program, path: () }) else {
        panic!("programa da linha de base inválido: {program}");
    };
    let Ok(filter) = Compiler::default().with_funs(funs).compile(modules) else {
        panic!("programa da linha de base não compila: {program}");
    };
    let ctx = jaq_core::Ctx::<D>::new(&filter.lut, jaq_core::Vars::new([]));
    let start = Instant::now();
    let n = filter.id.run((ctx, Val::Null)).count();
    std::hint::black_box(n);
    start.elapsed()
}

fn syntax_msg(found: Option<&str>) -> String {
    match found {
        None => "syntax error, unexpected end of file (Unix shell quoting issues?)".to_string(),
        Some("") => "syntax error, unexpected end of file (Unix shell quoting issues?)".to_string(),
        Some(tok) => format!("syntax error, unexpected {tok} (Unix shell quoting issues?)"),
    }
}

/// `locfile_locate`: "<msg> at <top-level>, line N:\n<linha><espaços até a coluna>".
fn locate(program: &str, part: &str, msg: &str) -> String {
    let start = part_offset(program, part).unwrap_or(0);
    let line_start = program[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_no = program[..start].matches('\n').count() + 1;
    let line_end = program[start..].find('\n').map(|i| start + i + 1).unwrap_or(program.len());
    let text = &program[line_start..line_end];
    format!("{msg} at <top-level>, line {line_no}:\n{text}{}", " ".repeat(start - line_start))
}

fn part_offset(whole: &str, part: &str) -> Option<usize> {
    let w = whole.as_ptr() as usize;
    let p = part.as_ptr() as usize;
    (p >= w && p <= w + whole.len()).then(|| p - w)
}

/// `$__loc__` é léxico no jq; o jaq não o conhece, então trocamos fora de strings e comentários.
pub fn substitute_loc(program: &str) -> String {
    const NEEDLE: &str = "$__loc__";
    if !program.contains(NEEDLE) {
        return program.to_string();
    }
    let bytes = program.as_bytes();
    let mut out = String::with_capacity(program.len());
    let mut i = 0;
    let mut line = 1;
    let mut in_str = false;
    let mut interp_depth: Vec<usize> = Vec::new();
    let mut paren = 0usize;
    // Pilha de colchetes fora de strings, pra reconhecer o atalho `{$__loc__}`.
    let mut brackets: Vec<u8> = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if c == b'\\' && i + 1 < bytes.len() {
                if bytes[i + 1] == b'(' {
                    in_str = false;
                    interp_depth.push(paren);
                    paren += 1;
                    brackets.push(b'(');
                    out.push_str("\\(");
                    i += 2;
                    continue;
                }
                out.push(c as char);
                out.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            push_byte(&mut out, program, &mut i);
            continue;
        }
        match c {
            b'"' => {
                in_str = true;
                push_byte(&mut out, program, &mut i);
            }
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    push_byte(&mut out, program, &mut i);
                }
            }
            b'(' => {
                paren += 1;
                brackets.push(b'(');
                push_byte(&mut out, program, &mut i);
            }
            b'[' | b'{' => {
                brackets.push(c);
                push_byte(&mut out, program, &mut i);
            }
            b']' | b'}' => {
                brackets.pop();
                push_byte(&mut out, program, &mut i);
            }
            b')' => {
                paren = paren.saturating_sub(1);
                brackets.pop();
                if interp_depth.last() == Some(&paren) {
                    interp_depth.pop();
                    in_str = true;
                }
                push_byte(&mut out, program, &mut i);
            }
            b'\n' => {
                line += 1;
                push_byte(&mut out, program, &mut i);
            }
            _ if program[i..].starts_with(NEEDLE)
                && !program[i + NEEDLE.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') =>
            {
                let value = format!("{{\"file\":\"<top-level>\",\"line\":{line}}}");
                let prev = out.trim_end().chars().last();
                if brackets.last() == Some(&b'{') && matches!(prev, Some('{') | Some(',')) {
                    out.push_str(&format!("\"__loc__\": {value}"));
                } else {
                    out.push_str(&value);
                }
                i += NEEDLE.len();
            }
            _ => push_byte(&mut out, program, &mut i),
        }
    }
    out
}

fn push_byte(out: &mut String, s: &str, i: &mut usize) {
    let ch = s[*i..].chars().next().expect("char");
    out.push(ch);
    *i += ch.len_utf8();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loc_substitution_respects_strings() {
        assert_eq!(substitute_loc("$__loc__"), r#"{"file":"<top-level>","line":1}"#);
        assert_eq!(substitute_loc("\"$__loc__\""), "\"$__loc__\"");
        assert_eq!(substitute_loc("1 |\n$__loc__"), "1 |\n{\"file\":\"<top-level>\",\"line\":2}");
        assert_eq!(substitute_loc("\"\\($__loc__)\""), "\"\\({\"file\":\"<top-level>\",\"line\":1})\"");
    }
}
