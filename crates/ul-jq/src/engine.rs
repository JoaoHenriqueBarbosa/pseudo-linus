//! Montagem do jq sobre o fork do jaq: tipo de dados com checkpoint, nativas que dependem do processo
//! (entradas, ambiente, `debug`, `stderr`, `halt`, tempo), prelúdio com as definições do jq 1.7.1 e
//! compilação com as mensagens de erro do jq.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use jaq_core::box_iter::box_once;
use jaq_core::compile::Compiler;
use jaq_core::data::HasLut;
use jaq_core::load::{Arena, File, Loader};
use jaq_core::native::{bome, Fun};
use jaq_core::{Bind, DataT, Error, Exn, Lut, ValX};
use jaq_json::jqfmt::{self, DumpOpts};
use jaq_json::{Map, Rc, Val};
use sysabi::Syscalls;

use crate::input::InputState;
use crate::{io, syntax, time};

/// Estado de uma execução do jq, alcançável pelas nativas.
pub struct Runtime {
    pub sys: Arc<dyn Syscalls>,
    /// Contador de nós avaliados (o checkpoint roda a cada 64).
    pub ticks: Cell<u64>,
    pub env_obj: Val,
    pub input: RefCell<InputState>,
    /// Opções de impressão do `debug` (as do `-S`, `-a`, `-C`, sem indentação).
    pub debug_opts: DumpOpts,
    /// `halt`/`halt_error`: código (`None` = `halt`, sai 0) e mensagem.
    pub halted: RefCell<Option<(Option<f64>, Option<Val>)>>,
    /// `get_jq_origin` e `get_prog_origin`.
    pub jq_origin: String,
    pub prog_origin: String,
    pub search_list: Val,
    pub tz: time::TimeZone,
}

impl Runtime {
    #[inline]
    pub fn tick(&self) {
        let n = self.ticks.get().wrapping_add(1);
        self.ticks.set(n);
        if n & 63 == 0 {
            self.sys.checkpoint();
        }
    }
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
        self.rt.tick();
        self.lut
    }
}

pub type Filter = jaq_core::Filter<JqKind>;

fn err<'a>(msg: impl Into<String>) -> ValX<'a, Val> {
    Err(Exn::from(Error::new(Val::from(msg.into()))))
}

fn rt<'a>(cv: &jaq_core::Cv<'a, JqKind>) -> &'a Runtime {
    cv.0.data().rt
}

/// Lista do `builtins` do jq 1.7.1 do Debian, na ordem em que ele devolve.
pub const BUILTINS: &str = include_str!("builtins.json");

fn v(n: usize) -> Box<[Bind]> {
    std::iter::repeat_n(Bind::Var(()), n).collect()
}

/// Nativas que dependem do processo.
fn process_funs() -> Vec<Fun<JqKind>> {
    let run = jaq_core::native::run::<JqKind>;
    vec![
        run(("env", v(0), |cv| box_once(Ok(rt(&cv).env_obj.clone())))),
        run(("now", v(0), |cv| box_once(Ok(Val::num(time::now(&*rt(&cv).sys)))))),
        run(("input", v(0), |cv| {
            let r = rt(&cv).input.borrow_mut().next_value();
            box_once(match r {
                Some(Ok(v)) => Ok(v),
                Some(Err(e)) => err(e),
                None => err("break"),
            })
        })),
        run(("input_filename", v(0), |cv| {
            let name = rt(&cv).input.borrow().current_filename.clone();
            box_once(Ok(name.map(Val::from).unwrap_or(Val::Null)))
        })),
        run(("input_line_number", v(0), |cv| {
            let line = rt(&cv).input.borrow().current_line();
            box_once(Ok(Val::from(line as usize)))
        })),
        run(("debug", v(0), |cv| {
            let rt = rt(&cv);
            let arr: Val = [Val::str("DEBUG:"), cv.1.clone()].into_iter().collect();
            let mut line = jqfmt::dump(&arr, &rt.debug_opts);
            line.push(b'\n');
            io::stderr(&line);
            box_once(Ok(cv.1))
        })),
        run(("stderr", v(0), |cv| {
            match &cv.1 {
                Val::TStr(b) | Val::BStr(b) => io::stderr(b),
                other => io::stderr(jqfmt::dump_compact(other).as_bytes()),
            }
            box_once(Ok(cv.1))
        })),
        run(("halt", v(0), |cv| {
            *rt(&cv).halted.borrow_mut() = Some((None, None));
            box_once(Err(Exn::halt(0)))
        })),
        run(("halt_error", v(1), |mut cv| {
            let code = cv.0.pop_var();
            let Some(c) = code.as_f64() else {
                return box_once(Err(Exn::from(jaq_json::type_error(&cv.1, "halt_error/1: number required"))));
            };
            let rt = rt(&cv);
            *rt.halted.borrow_mut() = Some((Some(c), Some(cv.1)));
            box_once(Err(Exn::halt(0)))
        })),
        run(("builtins", v(0), |_| {
            box_once(jaq_json::jqparse::parse_single(BUILTINS.as_bytes()).map_err(|e| Exn::from(Error::new(Val::from(e)))))
        })),
        run(("get_search_list", v(0), |cv| box_once(Ok(rt(&cv).search_list.clone())))),
        run(("get_jq_origin", v(0), |cv| box_once(Ok(Val::from(rt(&cv).jq_origin.clone()))))),
        run(("get_prog_origin", v(0), |cv| box_once(Ok(Val::from(rt(&cv).prog_origin.clone()))))),
        run(("modulemeta", v(0), |cv| {
            box_once(match cv.1.as_str() {
                Some(name) => err(format!("module not found: {name}")),
                None => err("modulemeta input module name must be a string"),
            })
        })),
        run(("gmtime", v(0), |cv| bome(time::gmtime(&cv.1)))),
        run(("localtime", v(0), |cv| bome(time::localtime(&cv.1, &rt(&cv).tz)))),
        run(("mktime", v(0), |cv| bome(time::mktime(&cv.1)))),
        run(("strftime", v(1), |mut cv| {
            let fmt = cv.0.pop_var();
            bome(time::strftime(&cv.1, &fmt, None))
        })),
        run(("strflocaltime", v(1), |mut cv| {
            let fmt = cv.0.pop_var();
            let tz = &rt(&cv).tz;
            bome(time::strftime(&cv.1, &fmt, Some(tz)))
        })),
        run(("strptime", v(1), |mut cv| {
            let fmt = cv.0.pop_var();
            bome(time::strptime(&cv.1, &fmt))
        })),
        run(("_match_impl", v(3), |mut cv| {
            let test = cv.0.pop_var();
            let modifiers = cv.0.pop_var();
            let re = cv.0.pop_var();
            bome(jaq_std::onig::f_match(&cv.1, &re, &modifiers, matches!(test, Val::Bool(true))))
        })),
    ]
}

/// Prelúdio: as definições do jq 1.7.1.
const PRELUDE: &str = include_str!("prelude.jq");

/// Erro de compilação já formatado como o jq faria (sem o prefixo "jq: error: ").
pub struct CompileError {
    pub messages: Vec<String>,
}

/// Todas as nativas, na ordem de prioridade (a primeira com o nome ganha).
fn all_funs() -> Vec<Fun<JqKind>> {
    let mut funs = process_funs();
    funs.extend(jaq_json::funs::<JqKind>());
    funs.extend(jaq_core::funs::<JqKind>());
    funs
}

/// Compila um programa com as variáveis globais dadas (nomes sem `$`).
pub fn compile(program: &str, globals: &[String]) -> Result<Filter, CompileError> {
    let prelude = jaq_core::load::parse(PRELUDE, |p| p.defs()).expect("prelúdio do jq válido");
    let loader = Loader::new(prelude);
    let arena = Arena::default();
    let file = File { code: program, path: () };
    let modules = match loader.load(&arena, file) {
        Ok(m) => m,
        Err(errs) => return Err(CompileError { messages: syntax::load_errors(program, errs) }),
    };
    let global_names: Vec<String> = globals.iter().map(|g| format!("${g}")).collect();
    let compiler = Compiler::default()
        .with_funs(all_funs())
        .with_global_vars(global_names.iter().map(String::as_str));
    compiler.compile(modules).map_err(|errs| CompileError { messages: syntax::compile_errors(program, errs) })
}

/// Objeto `$ENV`/`env` a partir do ambiente do processo (`NAME=valor`; sem `=`, o valor é `null`).
pub fn env_object(environ: &[Vec<u8>]) -> Val {
    let mut m = Map::default();
    for kv in environ {
        match kv.iter().position(|b| *b == b'=') {
            Some(eq) => {
                let k = jaq_json::jqparse::utf8_lossy(&kv[..eq]);
                let v = jaq_json::jqparse::utf8_lossy(&kv[eq + 1..]);
                m.insert(Val::from(k), Val::from(v));
            }
            None => {
                m.insert(Val::from(jaq_json::jqparse::utf8_lossy(kv)), Val::Null);
            }
        }
    }
    Val::obj(m)
}

/// Array de strings.
pub fn str_array(items: &[&str]) -> Val {
    Val::Arr(Rc::new(items.iter().map(|s| Val::str(s)).collect()))
}
