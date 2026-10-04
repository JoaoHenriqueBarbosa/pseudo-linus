//! O interpretador: avaliação de expressões e comandos sobre a árvore do parser, com as variáveis
//! especiais, registros e campos, entrada principal, saídas e as mensagens de erro do gawk 5.2.1.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use sysabi::{Errno, Fd, OFlags, Pid, Syscalls};

use crate::array::{AwkArray, Subscript};
use crate::ast::*;
use crate::format;
use crate::io::{self, OutKind, OutStream, PipeState, Reader, RsMode};
use crate::parser::{SPECIALS, location, sv};
use crate::regex::Regex;
use crate::value::*;

pub type Array = AwkArray<Cell>;
pub type ArrRef = Rc<RefCell<Array>>;

/// Conteúdo de uma variável ou elemento.
#[derive(Clone, Debug)]
pub enum Cell {
    Uninit,
    Val(Value),
    Arr(ArrRef),
    /// Parâmetro não tipado recebido por referência: aponta pra variável (ou elemento) do chamador.
    Ref(RefTarget),
}

/// Para onde aponta um parâmetro não tipado.
#[derive(Clone, Debug)]
pub enum RefTarget {
    Slot(Slot),
    Elem(ArrRef, Subscript),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Global(u32),
    Local(usize),
}

/// Desvio de fluxo (e erro fatal, cuja mensagem já foi escrita).
#[derive(Debug)]
pub enum Flow {
    Next,
    NextFile,
    Exit,
    Return(Value),
    Break,
    Continue,
    Fatal,
}

pub type R<T> = Result<T, Flow>;

/// Configuração vinda da linha de comando.
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub prog_name: String,
    /// `ARGV` (o índice 0 é o nome do programa).
    pub argv: Vec<Vec<u8>>,
    /// `-v nome=valor`, na ordem.
    pub assigns: Vec<Vec<u8>>,
    /// `-F fs` (já processado: `t` vira tab).
    pub fs: Option<Vec<u8>>,
    pub posix: bool,
    pub traditional: bool,
    pub non_decimal: bool,
    pub sandbox: bool,
    /// A linha de comando inteira (`PROCINFO["argv"]`).
    pub full_argv: Vec<Vec<u8>>,
}

/// Como o registro corrente é dividido em campos.
#[derive(Clone)]
pub enum SplitMode {
    /// `FS = " "`.
    Default,
    /// Um caractere literal (bytes em UTF-8) e se ignora caixa.
    Char(Vec<u8>, bool),
    /// `FS = ""`: um campo por caractere.
    Chars,
    Regex(Rc<Regex>),
    /// `FIELDWIDTHS`: (pular, largura); largura `None` é `*` (o resto).
    Widths(Vec<(usize, Option<usize>)>),
    /// `FPAT`.
    Fpat(Rc<Regex>),
}

/// Lugar resolvido de uma atribuição (subscritos avaliados uma vez só).
pub enum Place {
    Var(Var),
    Field(usize),
    Elem(ArrRef, Subscript),
}

struct Frame {
    base: usize,
    func: u32,
}

pub(crate) struct InStream {
    pub name: Vec<u8>,
    pub reader: Reader,
    pub pid: Option<Pid>,
}

struct MainInput {
    /// Próximo índice do ARGV a examinar.
    argi: usize,
    reader: Option<Reader>,
    /// Já apareceu algum operando de arquivo.
    used_file: bool,
    /// Terminou a entrada principal.
    done: bool,
}

pub struct Interp<'p> {
    pub(crate) p: &'p Program,
    pub(crate) sys: Arc<dyn Syscalls>,
    pub(crate) name: String,
    pub(crate) cfg: Config,
    pub(crate) globals: Vec<Cell>,
    pub(crate) locals: Vec<Cell>,
    /// Origem de cada local recebido por referência (pras mensagens `a (from x)`).
    local_from: Vec<Option<Rc<str>>>,
    frames: Vec<Frame>,
    // registro corrente
    pub(crate) record: Str,
    pub(crate) fields: Vec<Value>,
    pub(crate) nf: usize,
    split_done: bool,
    record_stale: bool,
    /// O `$0` atual foi atribuído com um texto (não é strnum).
    record_str: bool,
    pub(crate) nr: f64,
    pub(crate) fnr: f64,
    pub(crate) split_mode: SplitMode,
    record_split: SplitMode,
    pub(crate) rs_mode: RsMode,
    pub(crate) paragraph: bool,
    pub(crate) convfmt: Vec<u8>,
    pub(crate) ofmt: Vec<u8>,
    pub(crate) ofs: Vec<u8>,
    pub(crate) ors: Vec<u8>,
    pub(crate) subsep: Vec<u8>,
    pub(crate) ignorecase: bool,
    // regexes
    re_const: Vec<[Option<Rc<Regex>>; 2]>,
    re_dyn: HashMap<(Vec<u8>, bool), Rc<Regex>>,
    // saída e entrada
    pub(crate) stdout_buf: Vec<u8>,
    stdout_tty: bool,
    pub(crate) outputs: Vec<OutStream>,
    pub(crate) inputs: Vec<InStream>,
    main: MainInput,
    ranges: Vec<bool>,
    // estado
    pub(crate) loc: Option<(u16, u32)>,
    pub(crate) exit_code: i32,
    in_end: bool,
    exit_called: bool,
    pub(crate) rand: crate::builtins::Random,
    pub(crate) seed: f64,
    depth: usize,
    rule_ctx: RuleCtx,
    steps: u32,
    /// Avisos de regex já emitidos (o gawk avisa cada escape uma vez por execução).
    pub(crate) regex_warned: std::collections::HashSet<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RuleCtx {
    Begin,
    Main,
    End,
    BeginFile,
    EndFile,
}

/// Limite de profundidade de chamadas de função (o gawk só para quando acaba a memória).
const MAX_DEPTH: usize = 100_000;

impl<'p> Interp<'p> {
    pub fn new(p: &'p Program, cfg: Config, sys: Arc<dyn Syscalls>) -> Interp<'p> {
        let stdout_tty = sys.isatty(Fd::STDOUT);
        let mut it = Interp {
            p,
            name: cfg.prog_name.clone(),
            cfg,
            globals: vec![Cell::Uninit; p.globals.len()],
            locals: Vec::new(),
            local_from: Vec::new(),
            frames: Vec::new(),
            record: empty_str(),
            fields: Vec::new(),
            nf: 0,
            split_done: true,
            record_stale: false,
            record_str: false,
            nr: 0.0,
            fnr: 0.0,
            split_mode: SplitMode::Default,
            record_split: SplitMode::Default,
            rs_mode: RsMode::Newline,
            paragraph: false,
            convfmt: b"%.6g".to_vec(),
            ofmt: b"%.6g".to_vec(),
            ofs: b" ".to_vec(),
            ors: b"\n".to_vec(),
            subsep: b"\x1c".to_vec(),
            ignorecase: false,
            re_const: vec![[None, None]; p.regexes.len()],
            re_dyn: HashMap::new(),
            stdout_buf: Vec::new(),
            stdout_tty,
            outputs: Vec::new(),
            inputs: Vec::new(),
            main: MainInput { argi: 1, reader: None, used_file: false, done: false },
            ranges: vec![false; p.range_count as usize],
            loc: None,
            exit_code: 0,
            in_end: false,
            exit_called: false,
            rand: crate::builtins::Random::new(),
            seed: 0.0,
            depth: 0,
            rule_ctx: RuleCtx::Begin,
            steps: 0,
            regex_warned: std::collections::HashSet::new(),
            sys,
        };
        it.init_specials();
        it
    }

    fn init_specials(&mut self) {
        let set = |it: &mut Interp, idx: u32, v: Value| it.globals[idx as usize] = Cell::Val(v);
        set(self, sv::FS, Value::from_bytes(b" "));
        set(self, sv::OFS, Value::from_bytes(b" "));
        set(self, sv::ORS, Value::from_bytes(b"\n"));
        set(self, sv::RS, Value::from_bytes(b"\n"));
        set(self, sv::SUBSEP, Value::from_bytes(b"\x1c"));
        set(self, sv::CONVFMT, Value::from_bytes(b"%.6g"));
        set(self, sv::OFMT, Value::from_bytes(b"%.6g"));
        set(self, sv::RSTART, Value::Num(0.0));
        set(self, sv::RLENGTH, Value::Num(-1.0));
        set(self, sv::FILENAME, Value::from_bytes(b""));
        set(self, sv::FIELDWIDTHS, Value::from_bytes(b""));
        set(self, sv::FPAT, Value::from_bytes(b"[^[:space:]]+"));
        set(self, sv::IGNORECASE, Value::Num(0.0));
        set(self, sv::BINMODE, Value::Num(0.0));
        set(self, sv::LINT, Value::Num(0.0));
        set(self, sv::ERRNO, Value::from_bytes(b""));
        set(self, sv::RT, Value::from_bytes(b""));
        set(self, sv::TEXTDOMAIN, Value::from_bytes(b"messages"));
        set(self, sv::ARGIND, Value::Num(0.0));
        set(self, sv::PREC, Value::Num(53.0));
        set(self, sv::ROUNDMODE, Value::from_bytes(b"N"));
        // ENVIRON
        let mut env = Array::new();
        for kv in self.sys.environ() {
            if let Some(eq) = kv.iter().position(|b| *b == b'=') {
                let k = Subscript::from_bytes(Rc::from(&kv[..eq]));
                env.insert(k, Cell::Val(Value::strnum(&kv[eq + 1..])));
            }
        }
        self.globals[sv::ENVIRON as usize] = Cell::Arr(Rc::new(RefCell::new(env)));
        // ARGV / ARGC
        let mut argv = Array::new();
        for (i, a) in self.cfg.argv.iter().enumerate() {
            let v = if i == 0 { Value::from_bytes(a) } else { Value::strnum(a) };
            argv.insert(Subscript::from_int(i as i64), Cell::Val(v));
        }
        let argc = self.cfg.argv.len();
        self.globals[sv::ARGV as usize] = Cell::Arr(Rc::new(RefCell::new(argv)));
        set(self, sv::ARGC, Value::Num(argc as f64));
        // PROCINFO
        let procinfo = self.build_procinfo();
        self.globals[sv::PROCINFO as usize] = Cell::Arr(Rc::new(RefCell::new(procinfo)));
        self.globals[sv::SYMTAB as usize] = Cell::Arr(Rc::new(RefCell::new(Array::new())));
        let mut functab = Array::new();
        for f in &self.p.functions {
            if f.defined {
                let k = Subscript::from_bytes(Rc::from(f.name.as_bytes()));
                functab.insert(k, Cell::Val(Value::from_bytes(f.name.as_bytes())));
            }
        }
        for b in crate::lexer::BUILTINS {
            let k = Subscript::from_bytes(Rc::from(b.as_bytes()));
            functab.insert(k, Cell::Val(Value::from_bytes(b.as_bytes())));
        }
        self.globals[sv::FUNCTAB as usize] = Cell::Arr(Rc::new(RefCell::new(functab)));
    }

    fn build_procinfo(&self) -> Array {
        let mut a = Array::new();
        let mut put = |k: &str, v: Value| {
            a.insert(Subscript::from_bytes(Rc::from(k.as_bytes())), Cell::Val(v));
        };
        let sys = &self.sys;
        put("version", Value::from_bytes(b"5.2.1"));
        put("strftime", Value::from_bytes(b"%a %b %e %H:%M:%S %Z %Y"));
        put("FS", Value::from_bytes(b"FS"));
        put("platform", Value::from_bytes(b"posix"));
        put("api_major", Value::Num(3.0));
        put("api_minor", Value::Num(2.0));
        put("gmp_version", Value::from_bytes(b"GNU MP 6.3.0"));
        put("mpfr_version", Value::from_bytes(b"GNU MPFR 4.2.2"));
        put("prec_max", Value::Num(9223372036854775808.0));
        put("prec_min", Value::Num(1.0));
        put("pid", Value::Num(sys.getpid() as f64));
        put("ppid", Value::Num(sys.getppid() as f64));
        put("pgrpid", Value::Num(sys.getpgid(0).unwrap_or(0) as f64));
        put("uid", Value::Num(sys.getuid() as f64));
        put("euid", Value::Num(sys.geteuid() as f64));
        put("gid", Value::Num(sys.getgid() as f64));
        put("egid", Value::Num(sys.getegid() as f64));
        for (i, g) in sys.getgroups().iter().enumerate() {
            put(&format!("group{}", i + 1), Value::Num(*g as f64));
        }
        let mut argv = Array::new();
        for (i, arg) in self.cfg.full_argv.iter().enumerate() {
            argv.insert(Subscript::from_int(i as i64), Cell::Val(Value::from_bytes(arg)));
        }
        a.insert(Subscript::from_bytes(Rc::from(&b"argv"[..])), Cell::Arr(Rc::new(RefCell::new(argv))));
        let mut ids = Array::new();
        for (i, g) in self.p.globals.iter().enumerate() {
            let kind: &[u8] = if (i as u32) < sv::COUNT {
                if matches!(i as u32, sv::ENVIRON | sv::ARGV | sv::PROCINFO | sv::SYMTAB | sv::FUNCTAB) { b"array" } else { b"scalar" }
            } else {
                b"untyped"
            };
            ids.insert(Subscript::from_bytes(Rc::from(g.as_bytes())), Cell::Val(Value::from_bytes(kind)));
        }
        for f in &self.p.functions {
            if f.defined {
                ids.insert(Subscript::from_bytes(Rc::from(f.name.as_bytes())), Cell::Val(Value::from_bytes(b"user")));
            }
        }
        for b in crate::lexer::BUILTINS {
            ids.insert(Subscript::from_bytes(Rc::from(b.as_bytes())), Cell::Val(Value::from_bytes(b"builtin")));
        }
        a.insert(Subscript::from_bytes(Rc::from(&b"identifiers"[..])), Cell::Arr(Rc::new(RefCell::new(ids))));
        a
    }

    // ------------------------------------------------------------------ mensagens

    fn ctx_prefix(&self) -> String {
        let mut s = String::new();
        if self.fnr > 0.0 {
            let fname = match &self.globals[sv::FILENAME as usize] {
                Cell::Val(v) => String::from_utf8_lossy(&self.to_str(v)).into_owned(),
                _ => String::new(),
            };
            s = format!("(FILENAME={} FNR={}) ", fname, self.fnr as i64);
        }
        s
    }

    fn loc_prefix(&self) -> String {
        match self.loc {
            Some((src, line)) if line > 0 => format!("{}: {}: ", self.name, location(&self.p.sources, src, line)),
            _ => format!("{}: ", self.name),
        }
    }

    /// Escreve uma mensagem fatal e devolve o desvio correspondente.
    pub(crate) fn fatal(&mut self, msg: impl AsRef<str>) -> Flow {
        let line = format!("{}{}fatal: {}\n", self.loc_prefix(), self.ctx_prefix(), msg.as_ref());
        self.write_stderr(line.as_bytes());
        Flow::Fatal
    }

    pub(crate) fn warning(&mut self, msg: impl AsRef<str>) {
        let line = format!("{}{}warning: {}\n", self.loc_prefix(), self.ctx_prefix(), msg.as_ref());
        self.write_stderr(line.as_bytes());
    }

    pub(crate) fn write_stderr(&mut self, data: &[u8]) {
        // O stderr do gawk não tem buffer; o stdout pendente não precisa sair antes (fluxos separados).
        let _ = io::write_all(&self.sys, Fd::STDERR, data);
    }

    // ------------------------------------------------------------------ execução do programa

    /// Roda o programa inteiro e devolve o código de saída.
    pub fn run(&mut self) -> i32 {
        let r = self.run_inner();
        match r {
            Err(Flow::Fatal) => {
                self.shutdown_io(true);
                2
            }
            _ => {
                let code = self.shutdown_io(false);
                if code != 0 { code } else { self.exit_code }
            }
        }
    }

    fn run_inner(&mut self) -> R<()> {
        // -F e -v antes do BEGIN.
        if let Some(fs) = self.cfg.fs.clone() {
            self.set_global(sv::FS, Value::from_bytes(&fs))?;
        }
        for a in self.cfg.assigns.clone() {
            self.command_assign(&a, true)?;
        }
        self.rule_ctx = RuleCtx::Begin;
        for block in &self.p.begin {
            match self.exec_block(block) {
                Ok(()) => {}
                Err(Flow::Exit) => {
                    self.exit_called = true;
                    break;
                }
                Err(Flow::Fatal) => return Err(Flow::Fatal),
                Err(Flow::Next | Flow::NextFile) => {}
                Err(_) => {}
            }
        }
        let needs_input = !self.p.rules.is_empty() || !self.p.end.is_empty() || !self.p.beginfile.is_empty() || !self.p.endfile.is_empty();
        if !self.exit_called && needs_input {
            self.main_loop()?;
        }
        if !self.exit_called || !self.in_end {
            self.run_end()?;
        }
        Ok(())
    }

    fn run_end(&mut self) -> R<()> {
        if self.in_end {
            return Ok(());
        }
        self.in_end = true;
        self.rule_ctx = RuleCtx::End;
        // O exit no BEGIN pula a entrada, mas o END roda (salvo se o exit veio do próprio END).
        for block in &self.p.end {
            match self.exec_block(block) {
                Ok(()) => {}
                Err(Flow::Exit) => break,
                Err(Flow::Fatal) => return Err(Flow::Fatal),
                Err(_) => {}
            }
        }
        Ok(())
    }

    fn main_loop(&mut self) -> R<()> {
        loop {
            let rec = match self.next_main_record() {
                Ok(Some(r)) => r,
                Ok(None) => break,
                Err(Flow::Exit) => {
                    self.exit_called = true;
                    return Ok(());
                }
                Err(e) => return Err(e),
            };
            self.nr += 1.0;
            self.fnr += 1.0;
            self.set_rt(rec.1);
            self.set_record(Rc::from(rec.0));
            self.rule_ctx = RuleCtx::Main;
            match self.run_rules() {
                Ok(()) | Err(Flow::Next) => {}
                Err(Flow::NextFile) => {
                    self.skip_current_file()?;
                }
                Err(Flow::Exit) => {
                    self.exit_called = true;
                    return Ok(());
                }
                Err(e) => return Err(e),
            }
            self.steps = self.steps.wrapping_add(1);
            if self.steps & 63 == 0 {
                self.sys.checkpoint();
            }
        }
        Ok(())
    }

    fn run_rules(&mut self) -> R<()> {
        let p = self.p;
        for rule in &p.rules {
            self.loc = Some((rule.src, rule.line));
            let matched = match &rule.pattern {
                Pattern::All => true,
                Pattern::Expr(e) => self.eval_cond(e)?,
                Pattern::Range(a, b, id) => {
                    let id = *id as usize;
                    if self.ranges[id] {
                        if self.eval_cond(b)? {
                            self.ranges[id] = false;
                        }
                        true
                    } else if self.eval_cond(a)? {
                        if !self.eval_cond(b)? {
                            self.ranges[id] = true;
                        }
                        true
                    } else {
                        false
                    }
                }
            };
            if !matched {
                continue;
            }
            match &rule.action {
                Some(body) => self.exec_block(body)?,
                None => {
                    let rec = self.get_record_value();
                    let mut out = rec.to_vec();
                    out.extend_from_slice(&self.ors.clone());
                    self.write_stdout(&out)?;
                }
            }
        }
        Ok(())
    }

    /// Condição de padrão: regex sozinha casa contra `$0`.
    fn eval_cond(&mut self, e: &Expr) -> R<bool> {
        let v = self.eval(e)?;
        Ok(v.truthy())
    }

    // ------------------------------------------------------------------ entrada principal

    /// Próximo registro da entrada principal (abrindo os arquivos do ARGV conforme precisa).
    pub(crate) fn next_main_record(&mut self) -> R<Option<(Vec<u8>, Vec<u8>)>> {
        loop {
            if self.main.done {
                return Ok(None);
            }
            if self.main.reader.is_none() && !self.open_next_main_file()? {
                self.main.done = true;
                return Ok(None);
            }
            let rs = self.rs_mode.clone();
            let sys = self.sys.clone();
            let reader = self.main.reader.as_mut().expect("leitor");
            match reader.read_record(&sys, &rs) {
                Ok(Some(rec)) => return Ok(Some(rec)),
                Ok(None) => {
                    self.finish_main_file()?;
                }
                Err(e) => {
                    let fname = self.filename_string();
                    return Err(self.fatal(format!("error reading input file `{fname}': {}", e.message())));
                }
            }
        }
    }

    fn filename_string(&self) -> String {
        match &self.globals[sv::FILENAME as usize] {
            Cell::Val(v) => String::from_utf8_lossy(&self.to_str(v)).into_owned(),
            _ => String::new(),
        }
    }

    /// Fecha o arquivo corrente rodando o ENDFILE.
    fn finish_main_file(&mut self) -> R<()> {
        if let Some(r) = self.main.reader.take() {
            if r.owns_fd() {
                let _ = self.sys.close(r.fd);
            }
        }
        if !self.p.endfile.is_empty() {
            let saved = self.rule_ctx;
            self.rule_ctx = RuleCtx::EndFile;
            for block in &self.p.endfile {
                match self.exec_block(block) {
                    Ok(()) => {}
                    Err(Flow::Next | Flow::NextFile) => {}
                    Err(e) => {
                        self.rule_ctx = saved;
                        return Err(e);
                    }
                }
            }
            self.rule_ctx = saved;
        }
        Ok(())
    }

    /// `nextfile`: descarta o resto do arquivo corrente.
    fn skip_current_file(&mut self) -> R<()> {
        if self.main.reader.is_some() {
            self.finish_main_file()?;
        }
        Ok(())
    }

    fn argv_get(&mut self, i: usize) -> Option<Value> {
        let Cell::Arr(a) = &self.globals[sv::ARGV as usize] else { return None };
        let a = a.borrow();
        match a.get(&Subscript::from_int(i as i64)) {
            Some(Cell::Val(v)) => Some(v.clone()),
            Some(Cell::Uninit) => Some(Value::Uninit),
            _ => None,
        }
    }

    /// Abre o próximo arquivo de entrada; falso quando acabou.
    fn open_next_main_file(&mut self) -> R<bool> {
        loop {
            let argc = self.read_global_num(sv::ARGC) as usize;
            if self.main.argi >= argc {
                if self.main.used_file {
                    return Ok(false);
                }
                // Sem operandos de arquivo: stdin.
                self.main.used_file = true;
                self.start_main_file(b"-".to_vec(), None)?;
                if self.main.reader.is_some() {
                    return Ok(true);
                }
                continue;
            }
            let i = self.main.argi;
            self.main.argi += 1;
            let Some(arg) = self.argv_get(i) else { continue };
            let arg = self.to_str(&arg).to_vec();
            if arg.is_empty() {
                continue;
            }
            if is_assignment(&arg) {
                self.command_assign(&arg, false)?;
                continue;
            }
            self.main.used_file = true;
            self.set_global(sv::ARGIND, Value::Num(i as f64))?;
            self.start_main_file(arg, Some(i))?;
            if self.main.reader.is_some() {
                return Ok(true);
            }
        }
    }

    /// Abre um arquivo da entrada principal, roda o BEGINFILE e trata erro de abertura.
    fn start_main_file(&mut self, name: Vec<u8>, _argi: Option<usize>) -> R<()> {
        self.set_global(sv::FILENAME, Value::from_bytes(&name))?;
        self.fnr = 0.0;
        let opened: Result<Reader, Errno> = if name == b"-" || name == b"/dev/stdin" {
            Ok(Reader::new(Fd::STDIN, false))
        } else {
            match self.sys.openat(Fd::CWD, &name, OFlags::RDONLY | OFlags::CLOEXEC, 0) {
                Ok(fd) => match self.sys.fstat(fd) {
                    Ok(st) if st.file_type() == sysabi::FileType::Directory => {
                        let _ = self.sys.close(fd);
                        Err(Errno::EISDIR)
                    }
                    _ => Ok(Reader::new(fd, true)),
                },
                Err(e) => Err(e),
            }
        };
        let err = opened.as_ref().err().copied();
        if let Some(e) = err {
            self.set_global(sv::ERRNO, Value::from_bytes(e.message().as_bytes()))?;
        }
        if !self.p.beginfile.is_empty() {
            let saved = self.rule_ctx;
            self.rule_ctx = RuleCtx::BeginFile;
            for block in &self.p.beginfile {
                match self.exec_block(block) {
                    Ok(()) => {}
                    Err(Flow::NextFile) => {
                        self.rule_ctx = saved;
                        if let Ok(r) = opened {
                            if r.owns_fd() {
                                let _ = self.sys.close(r.fd);
                            }
                        }
                        return Ok(());
                    }
                    Err(e) => {
                        self.rule_ctx = saved;
                        return Err(e);
                    }
                }
            }
            self.rule_ctx = saved;
        }
        match opened {
            Ok(r) => {
                self.main.reader = Some(r);
                Ok(())
            }
            Err(Errno::EISDIR) => {
                let n = String::from_utf8_lossy(&name).into_owned();
                self.warning(format!("command line argument `{n}' is a directory: skipped"));
                Ok(())
            }
            Err(e) => {
                let n = String::from_utf8_lossy(&name).into_owned();
                Err(self.fatal(format!("cannot open file `{n}' for reading: {}", e.message())))
            }
        }
    }

    /// Atribuição `nome=valor` da linha de comando (`-v` ou operando).
    fn command_assign(&mut self, arg: &[u8], is_v: bool) -> R<()> {
        let eq = arg.iter().position(|b| *b == b'=').unwrap_or(arg.len());
        let name = String::from_utf8_lossy(&arg[..eq]).into_owned();
        let raw = &arg[(eq + 1).min(arg.len())..];
        let value = process_escapes(raw);
        let Some(idx) = self.p.globals.iter().position(|g| **g == *name) else {
            // Variável que o programa não usa: não tem efeito observável.
            let _ = is_v;
            return Ok(());
        };
        if self.p.functions.iter().any(|f| f.defined && *f.name == *name) {
            return Err(self.fatal(format!("cannot use function `{name}' as variable name")));
        }
        self.set_global(idx as u32, Value::strnum(&value))
    }

    // ------------------------------------------------------------------ comandos

    pub(crate) fn exec_block(&mut self, stmts: &[Stmt]) -> R<()> {
        for s in stmts {
            self.exec(s)?;
        }
        Ok(())
    }

    fn exec(&mut self, s: &Stmt) -> R<()> {
        self.loc = Some((s.src, s.line));
        match &s.kind {
            StmtKind::Expr(e) => {
                self.eval(e)?;
            }
            StmtKind::Print(args, redir) => self.exec_print(args, redir.as_ref())?,
            StmtKind::Printf(args, redir) => self.exec_printf(args, redir.as_ref())?,
            StmtKind::If(c, a, b) => {
                if self.eval(c)?.truthy() {
                    self.exec(a)?;
                } else if let Some(b) = b {
                    self.exec(b)?;
                }
            }
            StmtKind::While(c, body) => loop {
                self.tick();
                if !self.eval(c)?.truthy() {
                    break;
                }
                match self.exec(body) {
                    Ok(()) | Err(Flow::Continue) => {}
                    Err(Flow::Break) => break,
                    Err(e) => return Err(e),
                }
            },
            StmtKind::DoWhile(body, c) => loop {
                self.tick();
                match self.exec(body) {
                    Ok(()) | Err(Flow::Continue) => {}
                    Err(Flow::Break) => break,
                    Err(e) => return Err(e),
                }
                if !self.eval(c)?.truthy() {
                    break;
                }
            },
            StmtKind::For(init, cond, incr, body) => {
                if let Some(i) = init {
                    self.exec(i)?;
                }
                loop {
                    self.tick();
                    if let Some(c) = cond {
                        if !self.eval(c)?.truthy() {
                            break;
                        }
                    }
                    match self.exec(body) {
                        Ok(()) | Err(Flow::Continue) => {}
                        Err(Flow::Break) => break,
                        Err(e) => return Err(e),
                    }
                    if let Some(i) = incr {
                        self.exec(i)?;
                    }
                }
            }
            StmtKind::ForIn(var, arr, path, body) => self.exec_for_in(*var, *arr, path, body)?,
            StmtKind::Block(b) => self.exec_block(b)?,
            StmtKind::Next => {
                if matches!(self.rule_ctx, RuleCtx::Begin | RuleCtx::End | RuleCtx::BeginFile | RuleCtx::EndFile) {
                    let which = match self.rule_ctx {
                        RuleCtx::Begin => "BEGIN",
                        RuleCtx::End => "END",
                        RuleCtx::BeginFile => "BEGINFILE",
                        _ => "ENDFILE",
                    };
                    return Err(self.fatal(format!("`next' cannot be called from a `{which}' rule")));
                }
                return Err(Flow::Next);
            }
            StmtKind::NextFile => {
                if matches!(self.rule_ctx, RuleCtx::Begin | RuleCtx::End | RuleCtx::EndFile) {
                    let which = match self.rule_ctx {
                        RuleCtx::Begin => "BEGIN",
                        RuleCtx::End => "END",
                        _ => "ENDFILE",
                    };
                    return Err(self.fatal(format!("`nextfile' cannot be called from a `{which}' rule")));
                }
                return Err(Flow::NextFile);
            }
            StmtKind::Exit(e) => {
                if let Some(e) = e {
                    let v = self.eval(e)?;
                    self.exit_code = (self.to_num(&v) as i64 & 0xff) as i32;
                }
                return Err(Flow::Exit);
            }
            StmtKind::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(e)?,
                    None => Value::Uninit,
                };
                return Err(Flow::Return(v));
            }
            StmtKind::Break => return Err(Flow::Break),
            StmtKind::Continue => return Err(Flow::Continue),
            StmtKind::Delete(var, groups) => self.exec_delete(*var, groups)?,
            StmtKind::Switch(e, cases) => self.exec_switch(e, cases)?,
            StmtKind::Nop => {}
        }
        Ok(())
    }

    #[inline]
    fn tick(&mut self) {
        self.steps = self.steps.wrapping_add(1);
        if self.steps & 255 == 0 {
            self.sys.checkpoint();
        }
    }

    fn exec_switch(&mut self, e: &Expr, cases: &[(Option<CaseLabel>, Vec<Stmt>)]) -> R<()> {
        let v = self.eval(e)?;
        let mut start = None;
        for (i, (label, _)) in cases.iter().enumerate() {
            let Some(label) = label else { continue };
            let hit = match label {
                CaseLabel::Num(n) => {
                    if v.is_numeric() || matches!(v, Value::Num(_)) {
                        self.to_num(&v) == *n
                    } else {
                        let s = self.to_str(&v);
                        *s == *format::num_to_str(*n, &self.convfmt)
                    }
                }
                CaseLabel::Str(s) => *self.to_str(&v) == **s,
                CaseLabel::Regex(id) => {
                    let re = self.const_regex(*id)?;
                    let s = self.to_str(&v);
                    re.is_match(&s)
                }
            };
            if hit {
                start = Some(i);
                break;
            }
        }
        if start.is_none() {
            start = cases.iter().position(|(l, _)| l.is_none());
        }
        let Some(start) = start else { return Ok(()) };
        for (_, body) in &cases[start..] {
            match self.exec_block(body) {
                Ok(()) => {}
                Err(Flow::Break) => return Ok(()),
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn exec_for_in(&mut self, var: Var, arr: Var, path: &[Vec<Expr>], body: &Stmt) -> R<()> {
        let a = self.array_at(arr, path)?;
        let keys = self.for_in_keys(&a)?;
        for k in keys {
            // Elemento removido durante o laço não é visitado.
            if !a.borrow().contains(&k) {
                continue;
            }
            self.tick();
            self.write_var(var, Value::Str(k.text().clone()))?;
            match self.exec(body) {
                Ok(()) | Err(Flow::Continue) => {}
                Err(Flow::Break) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn exec_delete(&mut self, var: Var, groups: &[Vec<Expr>]) -> R<()> {
        match var {
            Var::Global(sv::SYMTAB) => return Err(self.fatal("`delete' is not allowed with SYMTAB")),
            Var::Global(sv::FUNCTAB) => return Err(self.fatal("`delete' is not allowed with FUNCTAB")),
            _ => {}
        }
        if groups.is_empty() {
            let a = self.get_array(var)?;
            a.borrow_mut().clear();
            return Ok(());
        }
        let (last, path) = groups.split_last().expect("grupos");
        let a = self.array_at(var, path)?;
        let k = self.subscript(last)?;
        a.borrow_mut().remove(&k);
        Ok(())
    }

    // ------------------------------------------------------------------ print / printf

    fn exec_print(&mut self, args: &[Expr], redir: Option<&Redirect>) -> R<()> {
        let mut out = Vec::new();
        if args.is_empty() {
            out.extend_from_slice(&self.get_record_value());
        } else {
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    out.extend_from_slice(&self.ofs);
                }
                let v = self.eval(a)?;
                let s = self.to_output_str(&v);
                out.extend_from_slice(&s);
            }
        }
        out.extend_from_slice(&self.ors);
        self.emit(redir, &out)
    }

    fn exec_printf(&mut self, args: &[Expr], redir: Option<&Redirect>) -> R<()> {
        if args.is_empty() {
            return Err(self.fatal("printf: no arguments"));
        }
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.eval(a)?);
        }
        let out = self.sprintf(&vals)?;
        self.emit(redir, &out)
    }

    /// `sprintf` com os valores já avaliados (o primeiro é o formato).
    pub(crate) fn sprintf(&mut self, vals: &[Value]) -> R<Vec<u8>> {
        let fmt = self.to_str(&vals[0]);
        let convfmt = self.convfmt.clone();
        let args: Vec<FmtValue> = vals[1..].iter().map(|v| FmtValue { v, convfmt: &convfmt }).collect();
        let refs: Vec<&dyn format::FmtArg> = args.iter().map(|a| a as &dyn format::FmtArg).collect();
        let mut warnings = Vec::new();
        let r = format::format(&fmt, &refs, &mut warnings);
        for w in warnings {
            self.warning(w);
        }
        match r {
            Ok(out) => Ok(out),
            Err(e) => Err(self.fatal(e.0)),
        }
    }

    /// Manda bytes pro destino do redirecionamento (ou pro stdout).
    fn emit(&mut self, redir: Option<&Redirect>, data: &[u8]) -> R<()> {
        let Some(redir) = redir else { return self.write_stdout(data) };
        let (target, mode) = match redir {
            Redirect::File(e) => (e, OutMode::Truncate),
            Redirect::Append(e) => (e, OutMode::Append),
            Redirect::Pipe(e) => (e, OutMode::Pipe),
            Redirect::Coproc(e) => (e, OutMode::Coproc),
        };
        let v = self.eval(target)?;
        let name = self.to_str(&v).to_vec();
        if name.is_empty() {
            let what = match mode {
                OutMode::Pipe | OutMode::Coproc => "expression for `|' redirection has null string value",
                OutMode::Append => "expression for `>>' redirection has null string value",
                OutMode::Truncate => "expression for `>' redirection has null string value",
            };
            return Err(self.fatal(what));
        }
        match self.output_stream(&name, mode)? {
            Some(idx) => self.write_stream(idx, data),
            None => Ok(()),
        }
    }

    pub(crate) fn write_stdout(&mut self, data: &[u8]) -> R<()> {
        self.stdout_buf.extend_from_slice(data);
        if self.stdout_buf.len() >= io::OUT_FLUSH_LIMIT || (self.stdout_tty && data.contains(&b'\n')) {
            self.flush_stdout()?;
        }
        Ok(())
    }

    pub(crate) fn flush_stdout(&mut self) -> R<()> {
        if self.stdout_buf.is_empty() {
            return Ok(());
        }
        let data = std::mem::take(&mut self.stdout_buf);
        match io::write_all(&self.sys, Fd::STDOUT, &data) {
            Ok(()) => Ok(()),
            Err(Errno::EPIPE) => {
                // No Linux o SIGPIPE já teria matado; se o kernel só devolveu EPIPE, morre igual.
                Err(Flow::Fatal)
            }
            Err(e) => Err(self.fatal(format!("print to \"standard output\" failed ({})", e.message()))),
        }
    }

    /// Índice do fluxo de saída `name`, abrindo se preciso.
    /// `PROCINFO["NONFATAL"]` ou `PROCINFO[nome, "NONFATAL"]`: erro de E/S vira ERRNO em vez de fatal.
    pub(crate) fn nonfatal(&self, name: &[u8]) -> bool {
        let Cell::Arr(p) = &self.globals[sv::PROCINFO as usize] else { return false };
        let p = p.borrow();
        if p.contains(&Subscript::from_bytes(Rc::from(&b"NONFATAL"[..]))) {
            return true;
        }
        let mut k = name.to_vec();
        k.extend_from_slice(&self.subsep);
        k.extend_from_slice(b"NONFATAL");
        p.contains(&Subscript::from_bytes(Rc::from(k)))
    }

    /// Falha ao abrir um redirecionamento: fatal, ou ERRNO com `NONFATAL`.
    fn open_failed(&mut self, name: &[u8], msg: String, e: Errno) -> R<Option<usize>> {
        if self.nonfatal(name) {
            self.set_global(sv::ERRNO, Value::from_bytes(e.message().as_bytes()))?;
            return Ok(None);
        }
        Err(self.fatal(msg))
    }

    fn output_stream(&mut self, name: &[u8], mode: OutMode) -> R<Option<usize>> {
        if let Some(i) = self.outputs.iter().position(|o| o.name == name) {
            return Ok(Some(i));
        }
        let kind = match mode {
            OutMode::Pipe => {
                if self.cfg.sandbox {
                    return Err(self.fatal("redirection not allowed in sandbox mode"));
                }
                // O gawk sincroniza a saída antes de abrir um pipe novo.
                self.flush_all()?;
                OutKind::Pipe(PipeState::Pending)
            }
            OutMode::Coproc => {
                if name.starts_with(b"/inet") {
                    // Arquivos especiais de rede do gawk: ainda sem suporte (pendência no STATUS).
                    let n = String::from_utf8_lossy(name).into_owned();
                    return self.open_failed(
                        name,
                        format!("cannot open two way pipe `{n}' for input/output: {}", Errno::ECONNREFUSED.message()),
                        Errno::ECONNREFUSED,
                    );
                }
                OutKind::Coproc { write: PipeState::Pending, reader: None, pid: None }
            }
            OutMode::Truncate | OutMode::Append => {
                if name == b"/dev/stdout" || name == b"-" && false {
                    OutKind::Stdout
                } else if name == b"/dev/stderr" {
                    OutKind::Stderr
                } else if let Some(fd) = dev_fd(name) {
                    match self.sys.dup(Fd(fd)) {
                        Ok(nfd) => {
                            let _ = self.sys.set_cloexec(nfd, true);
                            OutKind::File(nfd)
                        }
                        Err(e) => {
                            let n = String::from_utf8_lossy(name).into_owned();
                            return self.open_failed(name, format!("cannot redirect to `{n}': {}", e.message()), e);
                        }
                    }
                } else {
                    if self.cfg.sandbox {
                        return Err(self.fatal("redirection not allowed in sandbox mode"));
                    }
                    let flags = OFlags::WRONLY
                        | OFlags::CREAT
                        | OFlags::CLOEXEC
                        | if mode == OutMode::Append { OFlags::APPEND } else { OFlags::TRUNC };
                    match self.sys.openat(Fd::CWD, name, flags, 0o666) {
                        Ok(fd) => OutKind::File(fd),
                        Err(e) => {
                            let n = String::from_utf8_lossy(name).into_owned();
                            return self.open_failed(name, format!("cannot redirect to `{n}': {}", e.message()), e);
                        }
                    }
                }
            }
        };
        self.outputs.push(OutStream { name: name.to_vec(), kind, buf: Vec::new() });
        Ok(Some(self.outputs.len() - 1))
    }

    fn write_stream(&mut self, idx: usize, data: &[u8]) -> R<()> {
        match &self.outputs[idx].kind {
            OutKind::Stdout => return self.write_stdout(data),
            OutKind::Stderr => {
                let _ = io::write_all(&self.sys, Fd::STDERR, data);
                return Ok(());
            }
            _ => {}
        }
        self.outputs[idx].buf.extend_from_slice(data);
        if self.outputs[idx].buf.len() >= io::OUT_FLUSH_LIMIT {
            self.flush_stream(idx)?;
        }
        Ok(())
    }

    /// Descarrega um fluxo (criando o filho de um pipe pendente).
    pub(crate) fn flush_stream(&mut self, idx: usize) -> R<()> {
        let sys = self.sys.clone();
        let o = &mut self.outputs[idx];
        let name = o.name.clone();
        let r: Result<(), (Errno, &'static str)> = match &mut o.kind {
            OutKind::Stdout => return self.flush_stdout(),
            OutKind::Stderr => Ok(()),
            OutKind::File(fd) => {
                if o.buf.is_empty() {
                    Ok(())
                } else {
                    let data = std::mem::take(&mut o.buf);
                    io::write_all(&sys, *fd, &data).map_err(|e| (e, "file"))
                }
            }
            OutKind::Pipe(state) => match state {
                PipeState::Pending => {
                    self.flush_stdout()?;
                    let o = &mut self.outputs[idx];
                    let mut buf = std::mem::take(&mut o.buf);
                    match io::start_output_pipe(&sys, &name, &mut buf) {
                        Ok((fd, pid)) => {
                            self.outputs[idx].kind = OutKind::Pipe(PipeState::Running { fd, pid });
                            Ok(())
                        }
                        Err(e) => Err((e, "pipe")),
                    }
                }
                PipeState::Running { fd, .. } => {
                    if o.buf.is_empty() {
                        Ok(())
                    } else {
                        let data = std::mem::take(&mut o.buf);
                        io::write_pipe(&sys, *fd, &data).map_err(|e| (e, "pipe"))
                    }
                }
                PipeState::WriteClosed { .. } => Ok(()),
            },
            OutKind::Coproc { write, .. } => match write {
                PipeState::Running { fd, .. } => {
                    if o.buf.is_empty() {
                        Ok(())
                    } else {
                        let data = std::mem::take(&mut o.buf);
                        io::write_pipe(&sys, *fd, &data).map_err(|e| (e, "pipe"))
                    }
                }
                PipeState::Pending => {
                    self.start_coproc(idx, false)?;
                    Ok(())
                }
                PipeState::WriteClosed { .. } => Ok(()),
            },
        };
        match r {
            Ok(()) => Ok(()),
            Err((e, _)) => {
                let n = String::from_utf8_lossy(&name).into_owned();
                if e == Errno::EPIPE {
                    Err(self.fatal(format!("print to \"{n}\" failed: {}", e.message())))
                } else {
                    Err(self.fatal(format!("print to \"{n}\" failed ({})", e.message())))
                }
            }
        }
    }

    /// Cria o coprocesso `idx` (entregando o pendente); `close_write` fecha a escrita em seguida.
    pub(crate) fn start_coproc(&mut self, idx: usize, close_write: bool) -> R<()> {
        let sys = self.sys.clone();
        self.flush_stdout()?;
        let name = self.outputs[idx].name.clone();
        let mut buf = std::mem::take(&mut self.outputs[idx].buf);
        match io::start_coproc(&sys, &name, &mut buf, close_write) {
            Ok((wfd, reader, pid)) => {
                let write = match wfd {
                    Some(fd) => PipeState::Running { fd, pid },
                    None => PipeState::WriteClosed { pid },
                };
                self.outputs[idx].kind = OutKind::Coproc { write, reader: Some(reader), pid: Some(pid) };
                Ok(())
            }
            Err(e) => {
                let n = String::from_utf8_lossy(&name).into_owned();
                Err(self.fatal(format!("cannot open two way pipe `{n}' for input/output: {}", e.message())))
            }
        }
    }

    /// Descarrega tudo: stdout e todos os fluxos (o `flush_io` do gawk).
    pub(crate) fn flush_all(&mut self) -> R<()> {
        self.flush_stdout()?;
        for i in 0..self.outputs.len() {
            self.flush_stream(i)?;
        }
        Ok(())
    }

    /// Fecha um fluxo de saída e devolve o valor do `close`.
    pub(crate) fn close_output(&mut self, idx: usize) -> R<f64> {
        let flushed = self.flush_stream(idx);
        let o = self.outputs.remove(idx);
        if let Err(e) = flushed {
            return Err(e);
        }
        let sys = self.sys.clone();
        Ok(match o.kind {
            OutKind::File(fd) => {
                if sys.close(fd).is_ok() { 0.0 } else { -1.0 }
            }
            OutKind::Stdout | OutKind::Stderr => 0.0,
            OutKind::Pipe(PipeState::Running { fd, pid }) => {
                let _ = sys.close(fd);
                io::wait_pid(&sys, pid).map(io::status_value).unwrap_or(-1.0)
            }
            OutKind::Pipe(PipeState::WriteClosed { pid }) => io::wait_pid(&sys, pid).map(io::status_value).unwrap_or(-1.0),
            OutKind::Pipe(PipeState::Pending) => 0.0,
            OutKind::Coproc { write, reader, pid } => {
                if let PipeState::Running { fd, .. } = write {
                    let _ = sys.close(fd);
                }
                if let Some(r) = reader {
                    let _ = sys.close(r.fd);
                }
                match pid {
                    Some(pid) => io::wait_pid(&sys, pid).map(io::status_value).unwrap_or(-1.0),
                    None => 0.0,
                }
            }
        })
    }

    /// Fim do programa: fecha os fluxos (o mais recente primeiro, como o gawk) e descarrega o stdout.
    fn shutdown_io(&mut self, fatal: bool) -> i32 {
        let mut code = 0;
        while let Some(idx) = self.outputs.len().checked_sub(1) {
            if self.close_output(idx).is_err() && !fatal {
                code = 2;
            }
        }
        for s in std::mem::take(&mut self.inputs) {
            if s.reader.owns_fd() {
                let _ = self.sys.close(s.reader.fd);
            }
            if let Some(pid) = s.pid {
                let _ = io::wait_pid(&self.sys, pid);
            }
        }
        if let Some(r) = self.main.reader.take() {
            if r.owns_fd() {
                let _ = self.sys.close(r.fd);
            }
        }
        if self.flush_stdout().is_err() {
            code = 2;
        }
        code
    }

    // ------------------------------------------------------------------ valores

    pub(crate) fn to_num(&self, v: &Value) -> f64 {
        v.to_num()
    }

    /// Texto de um valor com `CONVFMT`.
    pub(crate) fn to_str(&self, v: &Value) -> Str {
        match v {
            Value::Str(s) | Value::StrNum(s) => s.clone(),
            Value::Uninit => empty_str(),
            Value::Num(n) => Rc::from(num_str(*n, &self.convfmt)),
            Value::Regex(_, s) => s.clone(),
            Value::Bool(b) => Rc::from(if *b { &b"1"[..] } else { &b"0"[..] }),
        }
    }

    /// Texto de saída do `print` (números com `OFMT`).
    pub(crate) fn to_output_str(&self, v: &Value) -> Str {
        match v {
            Value::Num(n) => Rc::from(num_str(*n, &self.ofmt)),
            _ => self.to_str(v),
        }
    }

    pub(crate) fn compare(&self, a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
        let a_num = matches!(a, Value::Uninit) || a.is_numeric();
        let b_num = matches!(b, Value::Uninit) || b.is_numeric();
        if a_num && b_num {
            return cmp_num(a.to_num(), b.to_num());
        }
        let sa = self.to_str(a);
        let sb = self.to_str(b);
        if self.ignorecase {
            let la = crate::builtins::lower_bytes(&sa);
            let lb = crate::builtins::lower_bytes(&sb);
            return Some(cmp_bytes(&la, &lb));
        }
        Some(cmp_bytes(&sa, &sb))
    }

    // ------------------------------------------------------------------ variáveis

    pub(crate) fn frame_base(&self) -> usize {
        self.frames.last().map(|f| f.base).unwrap_or(0)
    }

    fn slot(&self, v: Var) -> Slot {
        match v {
            Var::Global(i) => Slot::Global(i),
            Var::Local(i) => Slot::Local(self.frames.last().map(|f| f.base).unwrap_or(0) + i as usize),
        }
    }

    pub(crate) fn cell(&self, s: Slot) -> &Cell {
        match s {
            Slot::Global(i) => &self.globals[i as usize],
            Slot::Local(i) => &self.locals[i],
        }
    }

    pub(crate) fn cell_mut(&mut self, s: Slot) -> &mut Cell {
        match s {
            Slot::Global(i) => &mut self.globals[i as usize],
            Slot::Local(i) => &mut self.locals[i],
        }
    }

    /// Nome de uma variável pras mensagens.
    pub(crate) fn var_name(&self, v: Var) -> String {
        match v {
            Var::Global(i) => self.p.globals[i as usize].to_string(),
            Var::Local(i) => {
                let f = self.frames.last().map(|f| f.func).unwrap_or(0);
                self.p.functions.get(f as usize).and_then(|f| f.params.get(i as usize)).map(|s| s.to_string()).unwrap_or_default()
            }
        }
    }

    fn read_global_num(&self, idx: u32) -> f64 {
        match &self.globals[idx as usize] {
            Cell::Val(v) => v.to_num(),
            _ => 0.0,
        }
    }

    pub(crate) fn read_var(&mut self, v: Var) -> R<Value> {
        if let Var::Global(i) = v {
            match i {
                sv::NF => {
                    self.ensure_split()?;
                    return Ok(Value::Num(self.nf as f64));
                }
                sv::NR => return Ok(Value::Num(self.nr)),
                sv::FNR => return Ok(Value::Num(self.fnr)),
                _ => {}
            }
        }
        let s = self.slot(v);
        match self.cell(s) {
            Cell::Val(x) => Ok(x.clone()),
            Cell::Uninit => {
                // Ler uma variável não tipada como escalar fixa o tipo (o gawk faz o mesmo).
                *self.cell_mut(s) = Cell::Val(Value::Uninit);
                Ok(Value::Uninit)
            }
            Cell::Arr(_) => {
                let n = self.var_name_from(v);
                Err(self.fatal(format!("attempt to use array `{n}' in a scalar context")))
            }
            Cell::Ref(_) => {
                // Parâmetro não tipado: vira escalar aqui e na variável do chamador.
                if self.scalarize_ref(s) {
                    let n = self.var_name_from(v);
                    return Err(self.fatal(format!("attempt to use array `{n}' in a scalar context")));
                }
                Ok(Value::Uninit)
            }
        }
    }

    /// Célula apontada por uma referência (elemento ausente conta como não tipado).
    pub(crate) fn target_cell(&self, t: &RefTarget) -> Cell {
        match t {
            RefTarget::Slot(s) => self.cell(*s).clone(),
            RefTarget::Elem(a, k) => a.borrow().get(k).cloned().unwrap_or(Cell::Uninit),
        }
    }

    fn set_target_cell(&mut self, t: &RefTarget, c: Cell) {
        match t {
            RefTarget::Slot(s) => *self.cell_mut(*s) = c,
            RefTarget::Elem(a, k) => {
                a.borrow_mut().insert(k.clone(), c);
            }
        }
    }

    /// Segue a cadeia de referências de `s` e fixa como escalar as que ainda estão sem tipo.
    /// Devolve verdadeiro se a ponta da cadeia já é um array (o chamador virou array).
    fn scalarize_ref(&mut self, s: Slot) -> bool {
        let mut chain = vec![RefTarget::Slot(s)];
        let mut cur = self.cell(s).clone();
        while let Cell::Ref(t) = cur {
            cur = self.target_cell(&t);
            chain.push(t);
            if chain.len() > 10_000 {
                break;
            }
        }
        if let Cell::Arr(_) = cur {
            return true;
        }
        for t in chain {
            if matches!(self.target_cell(&t), Cell::Uninit | Cell::Ref(_)) {
                self.set_target_cell(&t, Cell::Val(Value::Uninit));
            }
        }
        false
    }

    /// Nome com a origem de um parâmetro array (`a (from arr)`), como nas mensagens do gawk.
    pub(crate) fn var_name_from(&self, v: Var) -> String {
        let n = self.var_name(v);
        if let Var::Local(i) = v {
            if let Some(Some(from)) = self.local_from.get(self.frame_base() + i as usize) {
                return format!("{n} (from {from})");
            }
        }
        n
    }

    pub(crate) fn write_var(&mut self, v: Var, val: Value) -> R<()> {
        if let Var::Global(i) = v {
            return self.set_global(i, val);
        }
        let s = self.slot(v);
        match self.cell(s) {
            Cell::Arr(_) => {
                let n = self.var_name_from(v);
                Err(self.fatal(format!("attempt to use array `{n}' in a scalar context")))
            }
            Cell::Ref(_) => {
                if self.scalarize_ref(s) {
                    let n = self.var_name_from(v);
                    return Err(self.fatal(format!("attempt to use array `{n}' in a scalar context")));
                }
                *self.cell_mut(s) = Cell::Val(val);
                Ok(())
            }
            _ => {
                *self.cell_mut(s) = Cell::Val(val);
                Ok(())
            }
        }
    }

    /// Atribui uma global, com os efeitos das especiais.
    pub(crate) fn set_global(&mut self, i: u32, val: Value) -> R<()> {
        if let Cell::Arr(_) = self.globals[i as usize] {
            let n = self.p.globals[i as usize].to_string();
            return Err(self.fatal(format!("attempt to use array `{n}' in a scalar context")));
        }
        if i >= sv::COUNT {
            self.globals[i as usize] = Cell::Val(val);
            return Ok(());
        }
        match i {
            sv::NF => {
                let n = self.to_num(&val);
                return self.set_nf(n);
            }
            sv::NR => {
                self.nr = self.to_num(&val).trunc();
                return Ok(());
            }
            sv::FNR => {
                self.fnr = self.to_num(&val).trunc();
                return Ok(());
            }
            sv::ENVIRON | sv::ARGV | sv::PROCINFO | sv::SYMTAB | sv::FUNCTAB => {
                let n = SPECIALS[i as usize];
                return Err(self.fatal(format!("attempt to use array `{n}' in a scalar context")));
            }
            _ => {}
        }
        self.globals[i as usize] = Cell::Val(val.clone());
        let bytes = self.to_str(&val).to_vec();
        match i {
            sv::FS => {
                self.set_procinfo_fs(b"FS");
                if let Value::Regex(id, _) = val {
                    // Regex tipada vale como regex mesmo com um caractere só.
                    self.split_mode = SplitMode::Regex(self.const_regex(id)?);
                } else {
                    self.update_fs(&bytes)?;
                }
            }
            sv::RS => {
                if let Value::Regex(id, _) = val {
                    self.paragraph = false;
                    self.rs_mode = RsMode::Regex(self.const_regex(id)?);
                } else {
                    self.update_rs(&bytes)?;
                }
            }
            sv::OFS => {
                // O gawk reconstrói o $0 pendente com o OFS antigo antes da troca.
                if self.record_stale {
                    self.rebuild_record();
                }
                self.ofs = bytes;
            }
            sv::ORS => self.ors = bytes,
            sv::SUBSEP => self.subsep = bytes,
            sv::CONVFMT => self.convfmt = bytes,
            sv::OFMT => self.ofmt = bytes,
            sv::IGNORECASE => {
                self.ignorecase = val.truthy();
                let fs = self.global_bytes(sv::FS);
                if !matches!(self.split_mode, SplitMode::Widths(_) | SplitMode::Fpat(_)) {
                    self.update_fs(&fs)?;
                }
                let rs = self.global_bytes(sv::RS);
                self.update_rs(&rs)?;
            }
            sv::FIELDWIDTHS => {
                self.set_procinfo_fs(b"FIELDWIDTHS");
                self.update_fieldwidths(&bytes)?;
            }
            sv::FPAT => {
                self.set_procinfo_fs(b"FPAT");
                let re = self.dyn_regex(&bytes)?;
                self.split_mode = SplitMode::Fpat(re);
            }
            _ => {}
        }
        Ok(())
    }

    fn set_procinfo_fs(&mut self, which: &[u8]) {
        if let Cell::Arr(a) = &self.globals[sv::PROCINFO as usize] {
            a.borrow_mut().insert(Subscript::from_bytes(Rc::from(&b"FS"[..])), Cell::Val(Value::from_bytes(which)));
        }
    }

    pub(crate) fn global_bytes(&self, i: u32) -> Vec<u8> {
        match &self.globals[i as usize] {
            Cell::Val(v) => self.to_str(v).to_vec(),
            _ => Vec::new(),
        }
    }

    fn update_fs(&mut self, fs: &[u8]) -> R<()> {
        self.split_mode = self.split_mode_for(fs, self.ignorecase)?;
        Ok(())
    }

    /// Modo de divisão de um FS (também usado pelo `split`).
    pub(crate) fn split_mode_for(&mut self, fs: &[u8], icase: bool) -> R<SplitMode> {
        Ok(if fs == b" " {
            SplitMode::Default
        } else if fs.is_empty() {
            SplitMode::Chars
        } else if char_len(fs) == fs.len() && fs != b"\\" {
            // FS de um caractere é literal e não depende do IGNORECASE (medido no gawk 5.2.1).
            let _ = icase;
            SplitMode::Char(fs.to_vec(), false)
        } else if fs == b"\\" {
            SplitMode::Char(fs.to_vec(), false)
        } else {
            SplitMode::Regex(self.dyn_regex(fs)?)
        })
    }

    fn update_rs(&mut self, rs: &[u8]) -> R<()> {
        self.paragraph = rs.is_empty();
        self.rs_mode = if rs == b"\n" {
            RsMode::Newline
        } else if rs.is_empty() {
            RsMode::Paragraph
        } else if char_len(rs) == rs.len() {
            RsMode::Char(rs.to_vec(), false)
        } else {
            RsMode::Regex(self.dyn_regex(rs)?)
        };
        Ok(())
    }

    /// `FIELDWIDTHS`: `[pular:]largura` separados por espaço, `*` só no fim; mensagens do gawk.
    fn update_fieldwidths(&mut self, spec: &[u8]) -> R<()> {
        let s = spec;
        let mut widths = Vec::new();
        let mut i = 0;
        let mut field = 0;
        let ws = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
        let digits = |i: &mut usize| -> Option<usize> {
            let start = *i;
            while *i < s.len() && s[*i].is_ascii_digit() {
                *i += 1;
            }
            if *i == start {
                return None;
            }
            std::str::from_utf8(&s[start..*i]).ok()?.parse::<usize>().ok()
        };
        loop {
            while i < s.len() && ws(s[i]) {
                i += 1;
            }
            if i >= s.len() {
                break;
            }
            field += 1;
            let near = |it: &mut Interp, at: usize| -> Flow {
                let rest = String::from_utf8_lossy(&s[at.min(s.len())..]).into_owned();
                it.fatal(format!("invalid FIELDWIDTHS value, for field {field}, near `{rest}'"))
            };
            let mut skip = 0;
            let width;
            if s[i] == b'*' {
                width = None;
                i += 1;
            } else {
                let first_at = i;
                let Some(n) = digits(&mut i) else { return Err(near(self, first_at)) };
                if i < s.len() && s[i] == b':' {
                    skip = n;
                    i += 1;
                    if i < s.len() && s[i] == b'*' {
                        width = None;
                        i += 1;
                    } else {
                        let w_at = i;
                        match digits(&mut i) {
                            Some(w) if w > 0 && (i >= s.len() || ws(s[i])) => width = Some(w),
                            _ => return Err(near(self, w_at)),
                        }
                    }
                } else if n == 0 || (i < s.len() && !ws(s[i])) {
                    return Err(near(self, first_at));
                } else {
                    width = Some(n);
                }
            }
            if width.is_none() {
                let mut j = i;
                while j < s.len() && ws(s[j]) {
                    j += 1;
                }
                if j < s.len() {
                    return Err(self.fatal("`*' must be the last designator in FIELDWIDTHS"));
                }
            }
            widths.push((skip, width));
        }
        self.split_mode = SplitMode::Widths(widths);
        Ok(())
    }

    // ------------------------------------------------------------------ arrays

    /// O array de uma variável (criando se estiver não inicializada).
    pub(crate) fn get_array(&mut self, v: Var) -> R<ArrRef> {
        if let Var::Global(sv::SYMTAB) = v {
            return Ok(self.symtab_snapshot());
        }
        let s = self.slot(v);
        self.array_from_slot(s, v)
    }

    fn array_from_slot(&mut self, s: Slot, v: Var) -> R<ArrRef> {
        match self.cell(s).clone() {
            Cell::Arr(a) => Ok(a),
            Cell::Uninit => {
                if let Slot::Global(i) = s {
                    if i < sv::COUNT {
                        let n = SPECIALS[i as usize];
                        return Err(self.fatal(format!("attempt to use scalar `{n}' as an array")));
                    }
                }
                let a = Rc::new(RefCell::new(Array::new()));
                *self.cell_mut(s) = Cell::Arr(a.clone());
                Ok(a)
            }
            Cell::Val(_) => {
                let n = self.var_name(v);
                if let Var::Local(_) = v {
                    return Err(self.fatal(format!("attempt to use scalar parameter `{n}' as an array")));
                }
                Err(self.fatal(format!("attempt to use scalar `{n}' as an array")))
            }
            Cell::Ref(t) => {
                let a = match &t {
                    RefTarget::Slot(ts) => self.array_from_slot(*ts, v)?,
                    RefTarget::Elem(arr, k) => {
                        let cur = arr.borrow().get(k).cloned();
                        match cur {
                            Some(Cell::Arr(x)) => x,
                            Some(Cell::Val(_)) => {
                                let n = self.var_name(v);
                                return Err(self.fatal(format!("attempt to use scalar parameter `{n}' as an array")));
                            }
                            _ => {
                                let x = Rc::new(RefCell::new(Array::new()));
                                arr.borrow_mut().insert(k.clone(), Cell::Arr(x.clone()));
                                x
                            }
                        }
                    }
                };
                *self.cell_mut(s) = Cell::Arr(a.clone());
                Ok(a)
            }
        }
    }

    /// Subscrito de uma lista de expressões (juntas com SUBSEP).
    pub(crate) fn subscript(&mut self, exprs: &[Expr]) -> R<Subscript> {
        if exprs.len() == 1 {
            let v = self.eval(&exprs[0])?;
            return Ok(self.value_subscript(&v));
        }
        let mut key = Vec::new();
        for (i, e) in exprs.iter().enumerate() {
            if i > 0 {
                key.extend_from_slice(&self.subsep);
            }
            let v = self.eval(e)?;
            key.extend_from_slice(&self.to_str(&v));
        }
        Ok(Subscript::from_bytes(Rc::from(key)))
    }

    pub(crate) fn value_subscript(&self, v: &Value) -> Subscript {
        match v {
            Value::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 9.0e15 {
                    Subscript::from_int(*n as i64)
                } else if n.is_finite() && n.fract() != 0.0 {
                    Subscript::from_non_integer(self.to_str(v))
                } else {
                    Subscript::from_bytes(self.to_str(v))
                }
            }
            _ => Subscript::from_bytes(self.to_str(v)),
        }
    }

    /// O array no fim de um caminho `a[i][j]` (criando subarrays como o gawk).
    pub(crate) fn array_at(&mut self, v: Var, path: &[Vec<Expr>]) -> R<ArrRef> {
        Ok(self.array_at_keys(v, path)?.0)
    }

    /// Nome do array de origem de uma variável (um parâmetro array é chamado pelo nome do array do
    /// chamador nas mensagens sobre elementos).
    fn array_root_name(&self, v: Var) -> String {
        if let Var::Local(i) = v {
            if let Some(Some(from)) = self.local_from.get(self.frame_base() + i as usize) {
                return from.rsplit(", from ").next().unwrap_or(from).to_string();
            }
        }
        self.var_name(v)
    }

    /// `a["1"]["2"]` pras mensagens.
    pub(crate) fn elem_name(&self, v: Var, keys: &[Subscript]) -> String {
        let mut s = self.array_root_name(v);
        for k in keys {
            s.push_str(&format!("[\"{}\"]", String::from_utf8_lossy(k.text())));
        }
        s
    }

    /// Como [`Interp::array_at`], devolvendo também os subscritos do caminho.
    fn array_at_keys(&mut self, v: Var, path: &[Vec<Expr>]) -> R<(ArrRef, Vec<Subscript>)> {
        let mut a = self.get_array(v)?;
        let mut keys = Vec::with_capacity(path.len());
        for g in path {
            let k = self.subscript(g)?;
            let next = {
                let mut b = a.borrow_mut();
                let cell = b.get_or_insert_with(&k, || Cell::Uninit);
                match cell {
                    Cell::Arr(x) => Ok(x.clone()),
                    Cell::Uninit => {
                        let x = Rc::new(RefCell::new(Array::new()));
                        *cell = Cell::Arr(x.clone());
                        Ok(x)
                    }
                    _ => Err(()),
                }
            };
            keys.push(k);
            match next {
                Ok(x) => a = x,
                Err(()) => {
                    let n = self.elem_name(v, &keys);
                    return Err(self.fatal(format!("attempt to use scalar `{n}' as an array")));
                }
            }
        }
        Ok((a, keys))
    }

    fn read_elem(&mut self, v: Var, groups: &[Vec<Expr>]) -> R<Value> {
        if let Var::Global(sv::SYMTAB) = v {
            return self.symtab_read(groups);
        }
        let (last, path) = groups.split_last().expect("grupos");
        let (a, mut keys) = self.array_at_keys(v, path)?;
        let k = self.subscript(last)?;
        if let Var::Global(sv::FUNCTAB) = v {
            if path.is_empty() && !a.borrow().contains(&k) {
                let name = String::from_utf8_lossy(k.text()).into_owned();
                return Err(self.fatal(format!("reference to uninitialized element `FUNCTAB[\"{name}\"] is not allowed'")));
            }
        }
        let r = {
            let mut b = a.borrow_mut();
            match b.get_or_insert_with(&k, || Cell::Uninit) {
                Cell::Val(x) => Ok(x.clone()),
                Cell::Uninit | Cell::Ref(_) => Ok(Value::Uninit),
                Cell::Arr(_) => Err(()),
            }
        };
        match r {
            Ok(x) => Ok(x),
            Err(()) => {
                keys.push(k);
                let n = self.elem_name(v, &keys);
                Err(self.fatal(format!("attempt to use array `{n}' in a scalar context")))
            }
        }
    }

    fn symtab_read(&mut self, groups: &[Vec<Expr>]) -> R<Value> {
        let k = self.subscript(&groups[0])?;
        match self.symtab_global(&k) {
            Some(i) => {
                if groups.len() > 1 {
                    return self.read_elem(Var::Global(i), &groups[1..]);
                }
                self.read_var(Var::Global(i))
            }
            None => {
                let name = String::from_utf8_lossy(k.text()).into_owned();
                Err(self.fatal(format!("reference to uninitialized element `SYMTAB[\"{name}\"] is not allowed'")))
            }
        }
    }

    /// Índice da global que um subscrito do SYMTAB nomeia (SYMTAB e FUNCTAB não aparecem nele).
    fn symtab_global(&self, k: &Subscript) -> Option<u32> {
        let name = std::str::from_utf8(k.text()).ok()?;
        let i = self.p.globals.iter().position(|g| **g == *name)? as u32;
        if i == sv::SYMTAB || i == sv::FUNCTAB {
            return None;
        }
        Some(i)
    }

    /// O SYMTAB como array (as globais na ordem em que o gawk as instala na tabela de símbolos).
    fn symtab_snapshot(&mut self) -> ArrRef {
        let mut a = Array::new();
        for name in symtab_install_order(self.p) {
            let i = self.p.globals.iter().position(|g| **g == *name).expect("global") as u32;
            let cell = match i {
                sv::NF => {
                    let _ = self.ensure_split();
                    Cell::Val(Value::Num(self.nf as f64))
                }
                sv::NR => Cell::Val(Value::Num(self.nr)),
                sv::FNR => Cell::Val(Value::Num(self.fnr)),
                _ => self.globals[i as usize].clone(),
            };
            a.insert(Subscript::from_bytes(Rc::from(name.as_bytes())), cell);
        }
        Rc::new(RefCell::new(a))
    }

    // ------------------------------------------------------------------ lugares (atribuição)

    pub(crate) fn place(&mut self, lv: &LValue) -> R<Place> {
        Ok(match lv {
            LValue::Var(v) => Place::Var(*v),
            LValue::Field(e) => Place::Field(self.field_index(e)?),
            LValue::Index(v, groups) => {
                if let Var::Global(sv::SYMTAB) = v {
                    let k = self.subscript(&groups[0])?;
                    let Some(i) = self.symtab_global(&k) else {
                        if groups.len() > 1 {
                            let name = String::from_utf8_lossy(k.text()).into_owned();
                            return Err(self.fatal(format!("reference to uninitialized element `SYMTAB[\"{name}\"] is not allowed'")));
                        }
                        return Err(self.fatal("cannot assign to arbitrary elements of SYMTAB"));
                    };
                    if groups.len() == 1 {
                        return Ok(Place::Var(Var::Global(i)));
                    }
                    let (last, path) = groups[1..].split_last().expect("grupos");
                    let a = self.array_at(Var::Global(i), path)?;
                    let k = self.subscript(last)?;
                    return Ok(Place::Elem(a, k));
                }
                if let Var::Global(sv::FUNCTAB) = v {
                    return Err(self.fatal("cannot assign to elements of FUNCTAB"));
                }
                let (last, path) = groups.split_last().expect("grupos");
                let (a, mut keys) = self.array_at_keys(*v, path)?;
                let k = self.subscript(last)?;
                if let Some(Cell::Arr(_)) = a.borrow().get(&k) {
                    keys.push(k);
                    let n = self.elem_name(*v, &keys);
                    return Err(self.fatal(format!("attempt to use array `{n}' in a scalar context")));
                }
                Place::Elem(a, k)
            }
        })
    }

    pub(crate) fn read_place(&mut self, p: &Place) -> R<Value> {
        match p {
            Place::Var(v) => self.read_var(*v),
            Place::Field(i) => self.get_field(*i),
            Place::Elem(a, k) => {
                let mut b = a.borrow_mut();
                Ok(match b.get_or_insert_with(k, || Cell::Uninit) {
                    Cell::Val(x) => x.clone(),
                    _ => Value::Uninit,
                })
            }
        }
    }

    pub(crate) fn write_place(&mut self, p: &Place, v: Value) -> R<()> {
        match p {
            Place::Var(var) => self.write_var(*var, v),
            Place::Field(i) => self.set_field(*i, v),
            Place::Elem(a, k) => {
                a.borrow_mut().insert(k.clone(), Cell::Val(v));
                Ok(())
            }
        }
    }

    pub(crate) fn assign(&mut self, lv: &LValue, v: Value) -> R<()> {
        let p = self.place(lv)?;
        self.write_place(&p, v)
    }

    // ------------------------------------------------------------------ registro e campos

    pub(crate) fn field_index(&mut self, e: &Expr) -> R<usize> {
        let v = self.eval(e)?;
        let n = self.to_num(&v);
        if n < 0.0 {
            return Err(self.fatal(format!("attempt to access field {}", n as i64)));
        }
        if n > 2.0e9 {
            return Err(self.fatal(format!("attempt to access field {}", n as i64)));
        }
        Ok(n as usize)
    }

    pub(crate) fn set_record(&mut self, rec: Str) {
        self.record = rec;
        self.record_str = false;
        self.split_done = false;
        self.record_stale = false;
        self.record_split = self.split_mode.clone();
    }

    pub(crate) fn set_rt(&mut self, rt: Vec<u8>) {
        self.globals[sv::RT as usize] = Cell::Val(Value::from_bytes(&rt));
    }

    /// `$0` atual (reconstruído se algum campo mudou).
    pub(crate) fn get_record_value(&mut self) -> Str {
        if self.record_stale {
            self.rebuild_record();
        }
        self.record.clone()
    }

    fn rebuild_record(&mut self) {
        let mut out = Vec::new();
        for i in 0..self.nf {
            if i > 0 {
                out.extend_from_slice(&self.ofs);
            }
            let s = self.to_str(&self.fields[i]);
            out.extend_from_slice(&s);
        }
        self.record = Rc::from(out);
        self.record_stale = false;
        self.record_str = false;
    }

    pub(crate) fn ensure_split(&mut self) -> R<()> {
        if self.split_done {
            return Ok(());
        }
        let rec = self.record.clone();
        let mode = self.record_split.clone();
        let mut fields = Vec::new();
        crate::fields::split_record(self, &rec, &mode, self.paragraph, &mut fields, None)?;
        self.nf = fields.len();
        self.fields = fields.into_iter().map(|f| Value::StrNum(Rc::from(f))).collect();
        self.split_done = true;
        Ok(())
    }

    pub(crate) fn get_field(&mut self, i: usize) -> R<Value> {
        if i == 0 {
            let rec = self.get_record_value();
            return Ok(if self.record_str { Value::Str(rec) } else { Value::StrNum(rec) });
        }
        self.ensure_split()?;
        // Campo além de NF é texto vazio (não é "não inicializado": `$5 == 0` é falso).
        Ok(if i <= self.nf { self.fields[i - 1].clone() } else { Value::StrNum(empty_str()) })
    }

    pub(crate) fn set_field(&mut self, i: usize, v: Value) -> R<()> {
        if i == 0 {
            let is_str = matches!(v, Value::Str(_));
            let s = self.to_str(&v);
            self.set_record(s);
            // `$0 = "0"` guarda um texto, não um strnum.
            self.record_str = is_str;
            return Ok(());
        }
        self.ensure_split()?;
        if i > self.nf {
            self.fields.resize(i, Value::StrNum(empty_str()));
            self.nf = i;
        }
        self.fields[i - 1] = v;
        self.record_stale = true;
        Ok(())
    }

    fn set_nf(&mut self, n: f64) -> R<()> {
        if n < 0.0 {
            return Err(self.fatal("NF set to negative value"));
        }
        self.ensure_split()?;
        let n = n as usize;
        self.fields.resize(n, Value::StrNum(empty_str()));
        self.nf = n;
        self.record_stale = true;
        Ok(())
    }

    // ------------------------------------------------------------------ regex

    /// Regex constante `id`, compilada com o IGNORECASE vigente.
    pub(crate) fn const_regex(&mut self, id: u32) -> R<Rc<Regex>> {
        let ic = self.ignorecase as usize;
        if let Some(r) = &self.re_const[id as usize][ic] {
            return Ok(r.clone());
        }
        let src = self.p.regexes[id as usize].clone();
        // Os avisos das regexes constantes já saíram no fim do parse.
        let re = match Regex::new(&src, self.ignorecase) {
            Ok((re, _)) => Rc::new(re),
            Err(e) => return Err(self.fatal(e.message)),
        };
        self.re_const[id as usize][ic] = Some(re.clone());
        Ok(re)
    }

    /// Regex dinâmica (texto usado como regex), com cache.
    pub(crate) fn dyn_regex(&mut self, src: &[u8]) -> R<Rc<Regex>> {
        let key = (src.to_vec(), self.ignorecase);
        if let Some(r) = self.re_dyn.get(&key) {
            return Ok(r.clone());
        }
        let re = match Regex::new(src, self.ignorecase) {
            Ok((re, warns)) => {
                for w in warns {
                    if self.regex_warned.insert(w.clone()) {
                        self.warning(w);
                    }
                }
                Rc::new(re)
            }
            Err(e) => return Err(self.fatal(e.message)),
        };
        if self.re_dyn.len() > 1000 {
            self.re_dyn.clear();
        }
        self.re_dyn.insert(key, re.clone());
        Ok(re)
    }

    /// Regex de uma expressão em posição de regex (constante, tipada ou dinâmica).
    pub(crate) fn regex_of(&mut self, e: &Expr) -> R<Rc<Regex>> {
        match e {
            Expr::Regex(id) | Expr::TypedRegex(id) => self.const_regex(*id),
            _ => {
                let v = self.eval(e)?;
                self.regex_of_value(&v)
            }
        }
    }

    pub(crate) fn regex_of_value(&mut self, v: &Value) -> R<Rc<Regex>> {
        match v {
            Value::Regex(id, _) => self.const_regex(*id),
            _ => {
                let s = self.to_str(v);
                self.dyn_regex(&s)
            }
        }
    }

    // ------------------------------------------------------------------ expressões

    pub(crate) fn eval(&mut self, e: &Expr) -> R<Value> {
        match e {
            Expr::Num(n) => Ok(Value::Num(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Regex(id) => {
                let re = self.const_regex(*id)?;
                let rec = self.get_record_value();
                Ok(Value::Num(re.is_match(&rec) as i32 as f64))
            }
            Expr::TypedRegex(id) => Ok(Value::Regex(*id, self.p.regexes[*id as usize].clone())),
            Expr::Var(v) => self.read_var(*v),
            Expr::Field(e) => {
                let i = self.field_index(e)?;
                self.get_field(i)
            }
            Expr::Index(v, groups) => self.read_elem(*v, groups),
            Expr::Group(e) => self.eval(e),
            Expr::Assign(lv, e) => {
                let v = self.eval(e)?;
                let v = match v {
                    Value::Uninit => Value::Uninit,
                    other => other,
                };
                self.assign(lv, v.clone())?;
                Ok(v)
            }
            Expr::AugAssign(op, lv, e) => {
                let r = self.eval(e)?;
                let p = self.place(lv)?;
                let cur = self.read_place(&p)?;
                let v = self.arith(*op, self.to_num(&cur), self.to_num(&r))?;
                self.write_place(&p, Value::Num(v))?;
                Ok(Value::Num(v))
            }
            Expr::Cond(c, a, b) => {
                if self.eval(c)?.truthy() {
                    self.eval(a)
                } else {
                    self.eval(b)
                }
            }
            Expr::And(a, b) => {
                let r = self.eval(a)?.truthy() && self.eval(b)?.truthy();
                Ok(Value::Num(r as i32 as f64))
            }
            Expr::Or(a, b) => {
                let r = self.eval(a)?.truthy() || self.eval(b)?.truthy();
                Ok(Value::Num(r as i32 as f64))
            }
            Expr::Not(a) => {
                let v = self.eval(a)?;
                Ok(Value::Num(!v.truthy() as i32 as f64))
            }
            Expr::Neg(a) => {
                let v = self.eval(a)?;
                Ok(Value::Num(-self.to_num(&v)))
            }
            Expr::Plus(a) => {
                let v = self.eval(a)?;
                Ok(Value::Num(self.to_num(&v)))
            }
            Expr::Binary(op, a, b) => {
                let x = self.eval(a)?;
                let y = self.eval(b)?;
                let r = self.arith(*op, self.to_num(&x), self.to_num(&y))?;
                Ok(Value::Num(r))
            }
            Expr::Cmp(op, a, b) => {
                let x = self.eval(a)?;
                let y = self.eval(b)?;
                let ord = self.compare(&x, &y);
                use std::cmp::Ordering::*;
                let r = match (op, ord) {
                    (CmpOp::Ne, None) => true,
                    (_, None) => false,
                    (CmpOp::Lt, Some(o)) => o == Less,
                    (CmpOp::Le, Some(o)) => o != Greater,
                    (CmpOp::Gt, Some(o)) => o == Greater,
                    (CmpOp::Ge, Some(o)) => o != Less,
                    (CmpOp::Eq, Some(o)) => o == Equal,
                    (CmpOp::Ne, Some(o)) => o != Equal,
                };
                Ok(Value::Num(r as i32 as f64))
            }
            Expr::Match(neg, lhs, re) => {
                let s = self.eval(lhs)?;
                let s = self.to_str(&s);
                let re = self.regex_of(re)?;
                let m = re.is_match(&s);
                Ok(Value::Num((m != *neg) as i32 as f64))
            }
            Expr::Concat(a, b) => {
                let x = self.eval(a)?;
                let y = self.eval(b)?;
                let xs = self.to_str(&x);
                let ys = self.to_str(&y);
                let mut out = Vec::new();
                if out.try_reserve(xs.len() + ys.len()).is_err() {
                    return Err(self.fatal("concatenation: out of memory"));
                }
                out.extend_from_slice(&xs);
                out.extend_from_slice(&ys);
                Ok(Value::Str(Rc::from(out)))
            }
            Expr::In(keys, v, path) => {
                let a = self.array_at(*v, path)?;
                let k = self.subscript(keys)?;
                let r = a.borrow().contains(&k);
                Ok(Value::Num(r as i32 as f64))
            }
            Expr::IncDec(lv, pre, delta) => {
                let p = self.place(lv)?;
                let cur = self.read_place(&p)?;
                let old = self.to_num(&cur);
                let new = old + delta;
                self.write_place(&p, Value::Num(new))?;
                Ok(Value::Num(if *pre { new } else { old }))
            }
            Expr::Call(f, args) => self.call_user(*f, args),
            Expr::IndirectCall(v, args) => self.call_indirect(*v, args),
            Expr::Builtin(b, args) => self.call_builtin(*b, args),
            Expr::Getline(src, target) => self.getline(src, target.as_deref()),
            Expr::List(_) => Err(self.fatal("internal error: expression list")),
        }
    }

    pub(crate) fn arith(&mut self, op: BinOp, x: f64, y: f64) -> R<f64> {
        Ok(match op {
            BinOp::Add => x + y,
            BinOp::Sub => x - y,
            BinOp::Mul => x * y,
            BinOp::Div => {
                if y == 0.0 {
                    return Err(self.fatal("division by zero attempted"));
                }
                x / y
            }
            BinOp::Mod => {
                if y == 0.0 {
                    return Err(self.fatal("division by zero attempted in `%'"));
                }
                x % y
            }
            BinOp::Pow => pow(x, y),
        })
    }

    // ------------------------------------------------------------------ funções do usuário

    fn call_user(&mut self, f: u32, args: &[Expr]) -> R<Value> {
        let func = &self.p.functions[f as usize];
        if !func.defined {
            let n = func.name.to_string();
            return Err(self.fatal(format!("function `{n}' not defined")));
        }
        let nparams = func.params.len();
        let mut cells = Vec::with_capacity(nparams.max(args.len()));
        let mut froms = Vec::with_capacity(nparams.max(args.len()));
        for a in args {
            let (c, from) = self.arg_cell(a)?;
            cells.push(c);
            froms.push(from);
        }
        cells.truncate(nparams);
        froms.truncate(nparams);
        while cells.len() < nparams {
            cells.push(Cell::Uninit);
            froms.push(None);
        }
        self.invoke_from(f, cells, froms)
    }

    /// Origem de um array passado por referência, pras mensagens (`b, from a, from foo`).
    fn origin_of(&self, v: Var) -> Rc<str> {
        let n = self.var_name(v);
        if let Var::Local(i) = v {
            if let Some(Some(from)) = self.local_from.get(self.frame_base() + i as usize) {
                return Rc::from(format!("{n}, from {from}"));
            }
        }
        Rc::from(n)
    }

    /// Avalia um argumento: array e variável não tipada vão por referência.
    fn arg_cell(&mut self, a: &Expr) -> R<(Cell, Option<Rc<str>>)> {
        match a {
            Expr::Var(v) => {
                if let Var::Global(i) = v {
                    if *i < sv::COUNT && !matches!(*i, sv::ENVIRON | sv::ARGV | sv::PROCINFO | sv::SYMTAB | sv::FUNCTAB) {
                        return Ok((Cell::Val(self.read_var(*v)?), None));
                    }
                }
                let s = self.slot(*v);
                let from = Some(self.origin_of(*v));
                Ok(match self.cell(s).clone() {
                    Cell::Arr(a) => (Cell::Arr(a), from),
                    Cell::Uninit => (Cell::Ref(RefTarget::Slot(s)), from),
                    Cell::Ref(t) => match self.target_cell(&t) {
                        Cell::Arr(a) => (Cell::Arr(a), from),
                        _ => (Cell::Ref(t), from),
                    },
                    Cell::Val(v) => (Cell::Val(v), None),
                })
            }
            Expr::Index(v, groups) => {
                let (last, path) = groups.split_last().expect("grupos");
                let (arr, mut keys) = self.array_at_keys(*v, path)?;
                let k = self.subscript(last)?;
                let cell = arr.borrow_mut().get_or_insert_with(&k, || Cell::Uninit).clone();
                keys.push(k.clone());
                let from = Some(Rc::from(self.elem_name(*v, &keys)));
                Ok(match cell {
                    Cell::Arr(a) => (Cell::Arr(a), from),
                    Cell::Val(x) => (Cell::Val(x), None),
                    // Elemento sem tipo vai por referência: se a função o usar como array, vira subarray.
                    _ => (Cell::Ref(RefTarget::Elem(arr, k)), from),
                })
            }
            _ => Ok((Cell::Val(self.eval(a)?), None)),
        }
    }

    pub(crate) fn invoke(&mut self, f: u32, cells: Vec<Cell>) -> R<Value> {
        let n = cells.len();
        self.invoke_from(f, cells, vec![None; n])
    }

    fn invoke_from(&mut self, f: u32, cells: Vec<Cell>, froms: Vec<Option<Rc<str>>>) -> R<Value> {
        if self.depth >= MAX_DEPTH {
            return Err(self.fatal("function call nesting too deep"));
        }
        self.tick();
        let base = self.locals.len();
        self.locals.extend(cells);
        self.local_from.truncate(base);
        self.local_from.extend(froms);
        self.local_from.resize(self.locals.len(), None);
        self.frames.push(Frame { base, func: f });
        self.depth += 1;
        let p = self.p;
        let body = &p.functions[f as usize].body;
        let saved_loc = self.loc;
        let r = stacker::maybe_grow(64 * 1024, 1024 * 1024, || self.exec_block(body));
        self.depth -= 1;
        self.frames.pop();
        self.locals.truncate(base);
        self.local_from.truncate(base);
        self.loc = saved_loc;
        match r {
            Ok(()) => Ok(Value::Uninit),
            Err(Flow::Return(v)) => Ok(v),
            Err(e) => Err(e),
        }
    }

    fn call_indirect(&mut self, v: Var, args: &[Expr]) -> R<Value> {
        let name = self.read_var(v)?;
        let name = String::from_utf8_lossy(&self.to_str(&name)).into_owned();
        let name = name.strip_prefix("awk::").map(str::to_string).unwrap_or(name);
        if let Some(fi) = self.p.functions.iter().position(|f| f.defined && *f.name == *name) {
            return self.call_user(fi as u32, args);
        }
        if let Some(b) = Builtin::from_name(&name) {
            if matches!(b, Builtin::Sub | Builtin::Gsub) && args.len() != 2 {
                return Err(self.fatal(format!("{name}: can be called indirectly only with two arguments")));
            }
            return self.call_builtin(b, args);
        }
        let vn = self.var_name(v);
        Err(self.fatal(format!("function name `{name}' (from indirect call through `{vn}') is not defined")))
    }

    // ------------------------------------------------------------------ getline

    fn getline(&mut self, src: &GetlineSrc, target: Option<&LValue>) -> R<Value> {
        match src {
            GetlineSrc::Main => {
                if self.rule_ctx == RuleCtx::End && self.main.done {
                    return Ok(Value::Num(0.0));
                }
                let saved_ctx = self.rule_ctx;
                let r = self.next_main_record();
                self.rule_ctx = saved_ctx;
                match r? {
                    Some((rec, rt)) => {
                        self.nr += 1.0;
                        self.fnr += 1.0;
                        self.set_rt(rt);
                        match target {
                            Some(lv) => self.assign(lv, Value::strnum(&rec))?,
                            None => self.set_record(Rc::from(rec)),
                        }
                        Ok(Value::Num(1.0))
                    }
                    None => Ok(Value::Num(0.0)),
                }
            }
            GetlineSrc::File(e) => {
                let v = self.eval(e)?;
                let name = self.to_str(&v).to_vec();
                let idx = match self.input_stream(&name, false)? {
                    Some(i) => i,
                    None => return Ok(Value::Num(-1.0)),
                };
                self.read_input_into(idx, target, false)
            }
            GetlineSrc::Cmd(e) => {
                let v = self.eval(e)?;
                let name = self.to_str(&v).to_vec();
                let idx = match self.input_stream(&name, true)? {
                    Some(i) => i,
                    None => return Ok(Value::Num(-1.0)),
                };
                self.read_input_into(idx, target, true)
            }
            GetlineSrc::Coproc(e) => {
                let v = self.eval(e)?;
                let name = self.to_str(&v).to_vec();
                let idx = match self.outputs.iter().position(|o| o.name == name) {
                    Some(i) => i,
                    None => {
                        self.outputs.push(OutStream { name: name.clone(), kind: OutKind::Coproc { write: PipeState::Pending, reader: None, pid: None }, buf: Vec::new() });
                        self.outputs.len() - 1
                    }
                };
                if let OutKind::Coproc { reader: None, .. } = &self.outputs[idx].kind {
                    self.start_coproc(idx, false)?;
                } else if let OutKind::Coproc { write: PipeState::Running { .. }, .. } = &self.outputs[idx].kind {
                    self.flush_stream(idx)?;
                }
                let rs = self.rs_mode.clone();
                let sys = self.sys.clone();
                let r = match &mut self.outputs[idx].kind {
                    OutKind::Coproc { reader: Some(rd), .. } => rd.read_record(&sys, &rs),
                    _ => return Ok(Value::Num(-1.0)),
                };
                self.finish_getline(r, target, true)
            }
        }
    }

    /// Índice do fluxo de entrada `name` (abrindo se preciso); `None` se não abre (ERRNO ajustado).
    fn input_stream(&mut self, name: &[u8], is_cmd: bool) -> R<Option<usize>> {
        if let Some(i) = self.inputs.iter().position(|s| s.name == name && s.pid.is_some() == is_cmd) {
            return Ok(Some(i));
        }
        if is_cmd {
            if self.cfg.sandbox {
                return Err(self.fatal("redirection not allowed in sandbox mode"));
            }
            self.flush_all()?;
            match io::start_input_pipe(&self.sys, name) {
                Ok((reader, pid)) => {
                    self.inputs.push(InStream { name: name.to_vec(), reader, pid: Some(pid) });
                    Ok(Some(self.inputs.len() - 1))
                }
                Err(e) => {
                    self.set_global(sv::ERRNO, Value::from_bytes(e.message().as_bytes()))?;
                    Ok(None)
                }
            }
        } else {
            let reader = if name == b"-" || name == b"/dev/stdin" {
                Reader::new(Fd::STDIN, false)
            } else if let Some(fd) = dev_fd(name) {
                Reader::new(Fd(fd), false)
            } else {
                match self.sys.openat(Fd::CWD, name, OFlags::RDONLY | OFlags::CLOEXEC, 0) {
                    Ok(fd) => {
                        if let Ok(st) = self.sys.fstat(fd) {
                            if st.file_type() == sysabi::FileType::Directory {
                                let _ = self.sys.close(fd);
                                self.set_global(sv::ERRNO, Value::from_bytes(Errno::EISDIR.message().as_bytes()))?;
                                return Ok(None);
                            }
                        }
                        Reader::new(fd, true)
                    }
                    Err(e) => {
                        self.set_global(sv::ERRNO, Value::from_bytes(e.message().as_bytes()))?;
                        return Ok(None);
                    }
                }
            };
            self.inputs.push(InStream { name: name.to_vec(), reader, pid: None });
            Ok(Some(self.inputs.len() - 1))
        }
    }

    fn read_input_into(&mut self, idx: usize, target: Option<&LValue>, counts_nr: bool) -> R<Value> {
        let rs = self.rs_mode.clone();
        let sys = self.sys.clone();
        let r = self.inputs[idx].reader.read_record(&sys, &rs);
        self.finish_getline(r, target, counts_nr)
    }

    fn finish_getline(&mut self, r: Result<Option<(Vec<u8>, Vec<u8>)>, Errno>, target: Option<&LValue>, counts_nr: bool) -> R<Value> {
        match r {
            Ok(Some((rec, rt))) => {
                if counts_nr {
                    self.nr += 1.0;
                }
                self.set_rt(rt);
                match target {
                    Some(lv) => self.assign(lv, Value::strnum(&rec))?,
                    None => {
                        if counts_nr {
                            // `cmd | getline` muda NR mas não FNR.
                        }
                        self.set_record(Rc::from(rec));
                    }
                }
                Ok(Value::Num(1.0))
            }
            Ok(None) => Ok(Value::Num(0.0)),
            Err(e) => {
                self.set_global(sv::ERRNO, Value::from_bytes(e.message().as_bytes()))?;
                Ok(Value::Num(-1.0))
            }
        }
    }

    /// `close(name)`: fecha saída e/ou entrada com esse nome.
    pub(crate) fn close_named(&mut self, name: &[u8], how: Option<&[u8]>) -> R<f64> {
        if let Some(idx) = self.outputs.iter().position(|o| o.name == name) {
            if let OutKind::Coproc { .. } = self.outputs[idx].kind {
                if how == Some(b"to") {
                    return self.close_coproc_write(idx);
                }
            }
            return self.close_output(idx);
        }
        if let Some(idx) = self.inputs.iter().position(|s| s.name == name) {
            let s = self.inputs.remove(idx);
            if s.reader.owns_fd() {
                let _ = self.sys.close(s.reader.fd);
            }
            return Ok(match s.pid {
                Some(pid) => io::wait_pid(&self.sys, pid).map(io::status_value).unwrap_or(-1.0),
                None => 0.0,
            });
        }
        self.set_global(sv::ERRNO, Value::from_bytes(b"close of redirection that was never opened"))?;
        Ok(-1.0)
    }

    fn close_coproc_write(&mut self, idx: usize) -> R<f64> {
        match &self.outputs[idx].kind {
            OutKind::Coproc { write: PipeState::Pending, .. } => {
                self.start_coproc(idx, true)?;
            }
            OutKind::Coproc { write: PipeState::Running { .. }, .. } => {
                self.flush_stream(idx)?;
                if let OutKind::Coproc { write, .. } = &mut self.outputs[idx].kind {
                    if let PipeState::Running { fd, pid } = *write {
                        let _ = self.sys.close(fd);
                        *write = PipeState::WriteClosed { pid };
                    }
                }
            }
            _ => {}
        }
        Ok(0.0)
    }

    /// Chaves do for-in, na ordem do `PROCINFO["sorted_in"]` ou na do gawk.
    fn for_in_keys(&mut self, a: &ArrRef) -> R<Vec<Subscript>> {
        let how = match &self.globals[sv::PROCINFO as usize] {
            Cell::Arr(p) => match p.borrow().get(&Subscript::from_bytes(Rc::from(&b"sorted_in"[..]))) {
                Some(Cell::Val(v)) => Some(self.to_str(v).to_vec()),
                _ => None,
            },
            _ => None,
        };
        match how {
            Some(h) if !h.is_empty() && h != b"@unsorted" => crate::sort::sorted_keys(self, a, &h),
            _ => Ok(a.borrow().keys()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutMode {
    Truncate,
    Append,
    Pipe,
    Coproc,
}

/// Argumento do `printf` visto pelo formatador.
struct FmtValue<'a> {
    v: &'a Value,
    convfmt: &'a [u8],
}

impl format::FmtArg for FmtValue<'_> {
    fn to_num(&self) -> f64 {
        self.v.to_num()
    }

    fn to_str(&self) -> std::borrow::Cow<'_, [u8]> {
        match self.v {
            Value::Str(s) | Value::StrNum(s) | Value::Regex(_, s) => std::borrow::Cow::Borrowed(s),
            Value::Uninit => std::borrow::Cow::Borrowed(b""),
            Value::Num(n) => std::borrow::Cow::Owned(num_str(*n, self.convfmt)),
            Value::Bool(b) => std::borrow::Cow::Borrowed(if *b { b"1" } else { b"0" }),
        }
    }

    fn is_numeric(&self) -> bool {
        matches!(self.v, Value::Num(_) | Value::Bool(_)) || self.v.is_numeric()
    }
}

/// Número pra texto, com o caminho rápido dos inteiros.
pub fn num_str(n: f64, fmt: &[u8]) -> Vec<u8> {
    if n.fract() == 0.0 && n.abs() < 1.0e15 {
        let i = n as i64;
        return i.to_string().into_bytes();
    }
    format::num_to_str(n, fmt)
}

/// Comprimento em bytes do primeiro caractere UTF-8 de `s`.
pub fn char_len(s: &[u8]) -> usize {
    let n = match s.first() {
        Some(0xc0..=0xdf) => 2,
        Some(0xe0..=0xef) => 3,
        Some(0xf0..=0xf7) => 4,
        Some(_) => 1,
        None => 0,
    };
    n.min(s.len())
}

/// `nome=valor` com nome de variável válido?
pub fn is_assignment(arg: &[u8]) -> bool {
    let Some(eq) = arg.iter().position(|b| *b == b'=') else { return false };
    let name = &arg[..eq];
    !name.is_empty() && (name[0].is_ascii_alphabetic() || name[0] == b'_') && name.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b':')
}

/// Escapes de string do awk no valor de uma atribuição de linha de comando.
pub fn process_escapes(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\\' && i + 1 < raw.len() {
            let mut pos = i;
            let (bytes, _) = crate::lexer::escape_sequence(raw, &mut pos, false);
            out.extend_from_slice(&bytes);
            i = pos;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    out
}

/// Variáveis especiais na ordem em que o gawk 5.2.1 as instala na tabela de símbolos (vale pra ordem
/// do `for (k in SYMTAB)`, que segue a tabela de hash de textos).
const SYMTAB_SPECIAL_ORDER: &[&str] = &[
    "ARGC", "ARGIND", "ARGV", "BINMODE", "CONVFMT", "ENVIRON", "ERRNO", "FIELDWIDTHS", "FILENAME", "FNR", "FPAT", "FS",
    "IGNORECASE", "LINT", "NF", "NR", "OFMT", "OFS", "ORS", "PREC", "PROCINFO", "RLENGTH", "ROUNDMODE", "RS",
    "RSTART", "RT", "SUBSEP", "TEXTDOMAIN",
];

/// Nomes do SYMTAB na ordem de instalação: as especiais e depois as globais do programa.
fn symtab_install_order(p: &Program) -> Vec<Rc<str>> {
    let mut out: Vec<Rc<str>> = SYMTAB_SPECIAL_ORDER.iter().map(|s| Rc::from(*s)).collect();
    for g in &p.globals[sv::COUNT as usize..] {
        out.push(g.clone());
    }
    out
}

/// `/dev/fd/N`.
fn dev_fd(name: &[u8]) -> Option<i32> {
    let rest = name.strip_prefix(b"/dev/fd/")?;
    std::str::from_utf8(rest).ok()?.parse().ok()
}
