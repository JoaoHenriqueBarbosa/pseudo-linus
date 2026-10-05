//! Execução do programa do sed sobre a entrada, linha a linha (streaming: `yes | sed 5q` termina).
//!
//! Regras observadas no sed 4.9 que moldam o código:
//!
//! - Linha final sem newline sai sem newline; o newline que falta é escrito assim que qualquer
//!   outra coisa vai pro mesmo fluxo. Cada buffer (padrão e reserva) carrega essa marca: `x` troca,
//!   `g`/`G` copiam da reserva, `h`/`H` copiam do padrão, `N` pega a da linha lida.
//! - `$` olha adiante, atravessando arquivos (arquivo que não abre não conta).
//! - A fila de `a`, `r` e `R` sai no fim do ciclo ou quando `n`/`N` leem a próxima linha; `D` com
//!   newline recomeça o ciclo sem esvaziá-la; `Q` a descarta.
//! - `s///g`: casada vazia colada no fim da anterior não conta.

use std::sync::Arc;

use sysabi::{Errno, Fd, FileType, OFlags, sys};

use super::script::{Addr, Addr2, CaseOp, Cmd, Kind, OutTarget, Program, Repl, SedRegex, Subst, TextKind};
use crate::io::{Out, errno_msg, error, safe_read};

/// Opções de execução.
#[derive(Clone, Debug, Default)]
pub struct RunOptions {
    pub quiet: bool,
    pub separate: bool,
    /// `-i`: sufixo de backup (vazio = sem backup).
    pub in_place: Option<Vec<u8>>,
    pub null_data: bool,
    pub unbuffered: bool,
    pub line_len: usize,
    /// `--posix` ou `POSIXLY_CORRECT`: `N` no fim não imprime.
    pub posix_n: bool,
    pub follow_symlinks: bool,
    pub debug: bool,
}

/// Uma linha lida.
struct Line {
    data: Vec<u8>,
    /// Terminava com o delimitador.
    chomped: bool,
}

/// Leitor de linhas sobre um fd.
struct LineReader {
    fd: Fd,
    buf: Vec<u8>,
    start: usize,
    eof: bool,
    /// Lê um byte por vez (`-u` em pipe), pra não consumir além do necessário.
    minimal: bool,
}

impl LineReader {
    fn new(fd: Fd, minimal: bool) -> LineReader {
        LineReader { fd, buf: Vec::new(), start: 0, eof: false, minimal }
    }

    fn read_line(&mut self, delim: u8) -> Result<Option<Line>, Errno> {
        loop {
            if let Some(i) = self.buf[self.start..].iter().position(|&b| b == delim) {
                let data = self.buf[self.start..self.start + i].to_vec();
                self.start += i + 1;
                return Ok(Some(Line { data, chomped: true }));
            }
            if self.eof {
                if self.start < self.buf.len() {
                    let data = self.buf[self.start..].to_vec();
                    self.start = self.buf.len();
                    return Ok(Some(Line { data, chomped: false }));
                }
                return Ok(None);
            }
            if self.start > 0 && self.start == self.buf.len() {
                self.buf.clear();
                self.start = 0;
            } else if self.start > 65536 {
                self.buf.drain(..self.start);
                self.start = 0;
            }
            let chunk = if self.minimal { 1 } else { 65536 };
            let old = self.buf.len();
            if self.buf.try_reserve(chunk).is_err() {
                return Err(Errno::ENOMEM);
            }
            self.buf.resize(old + chunk, 0);
            let r = safe_read(self.fd, &mut self.buf[old..]);
            let n = *r.as_ref().unwrap_or(&0);
            self.buf.truncate(old + n);
            r?;
            if n == 0 {
                self.eof = true;
            }
            sys::checkpoint();
        }
    }
}

/// Fluxo de saída com a marca de "falta o delimitador".
struct Output {
    out: Out,
    missing: bool,
    unbuffered: bool,
}

impl Output {
    fn new(fd: Fd, unbuffered: bool) -> Output {
        let mut out = Out::new(fd);
        if unbuffered {
            out.set_line_buffered(true);
        }
        Output { out, missing: false, unbuffered }
    }

    fn fix_missing(&mut self, delim: u8) {
        if self.missing {
            self.out.byte(delim);
            self.missing = false;
        }
    }

    /// Escreve uma linha (com o delimitador se `chomped`).
    fn line(&mut self, data: &[u8], chomped: bool, delim: u8) {
        self.fix_missing(delim);
        self.out.write(data);
        if chomped {
            self.out.byte(delim);
        } else {
            self.missing = true;
        }
        if self.unbuffered {
            self.out.flush();
        }
    }

    /// Escreve bytes crus (texto de `a`/`i`/`c`, `r`, `=`, `l`).
    fn raw(&mut self, data: &[u8], delim: u8) {
        self.fix_missing(delim);
        self.out.write(data);
        if self.unbuffered {
            self.out.flush();
        }
    }
}

enum Pending {
    Text(Vec<u8>),
    File(Vec<u8>),
    Line(Line),
}

#[derive(Clone, Copy, Default)]
struct Range {
    active: bool,
    end_line: u64,
}

/// Como o ciclo terminou.
enum Flow {
    /// Fim do script: autoprint e fila.
    End,
    /// `d`: sem autoprint.
    Delete,
    /// `D` com newline: recomeça sem ler.
    Restart,
    /// `q`/`Q`/fim da entrada em `n`/`N`.
    Quit { code: i32, autoprint: bool, dump: bool },
}

/// Fonte de linhas: um arquivo por vez, com uma linha de antecipação.
struct Input<'a> {
    names: &'a [Vec<u8>],
    next: usize,
    cur: Option<(LineReader, Vec<u8>)>,
    ahead: Option<(Line, Vec<u8>)>,
    /// Nome do arquivo da linha corrente (`F`).
    current_name: Vec<u8>,
    /// Um arquivo por vez (`-s`, `-i`): não atravessa pro próximo.
    single: bool,
    minimal: bool,
    delim: u8,
}

/// Erro fatal de execução (sai com 4).
struct Fatal(Vec<u8>);

pub struct Exec<'p> {
    prog: &'p Program,
    o: &'p RunOptions,
    progname: &'p [u8],
    delim: u8,
    ps: Vec<u8>,
    ps_chomped: bool,
    hs: Vec<u8>,
    hs_chomped: bool,
    line_no: u64,
    replaced: bool,
    queue: Vec<Pending>,
    ranges: Vec<Range>,
    last_regex: Option<Arc<SedRegex>>,
    /// 0 = stdout de verdade, 1 = stderr, 2.. = arquivos de `w`.
    outs: Vec<Output>,
    /// Índice da saída principal (stdout, ou o temporário do `-i`).
    main: usize,
    /// Índice em `outs` de cada saída de `w` do programa.
    w_index: Vec<usize>,
    readers: Vec<Option<Option<LineReader>>>,
    bad_input: bool,
    steps: u32,
}

impl<'p> Exec<'p> {
    pub fn new(prog: &'p Program, o: &'p RunOptions, progname: &'p [u8]) -> Exec<'p> {
        Exec {
            prog,
            o,
            progname,
            delim: if o.null_data { 0 } else { b'\n' },
            ps: Vec::new(),
            ps_chomped: true,
            hs: Vec::new(),
            hs_chomped: true,
            line_no: 0,
            replaced: false,
            queue: Vec::new(),
            // `0,/re/` já começa dentro da faixa (sem `-s`, `reset_stream` não roda no começo).
            ranges: prog.cmds.iter().map(|c| Range { active: matches!(c.a1, Some(Addr::Zero)), end_line: 0 }).collect(),
            last_regex: None,
            outs: Vec::new(),
            main: 0,
            w_index: Vec::new(),
            readers: Vec::new(),
            bad_input: false,
            steps: 0,
        }
    }

    /// Roda o programa sobre os arquivos; devolve o código de saída.
    pub fn run(&mut self, files: &[Vec<u8>]) -> i32 {
        self.outs.push(Output::new(Fd::STDOUT, self.o.unbuffered));
        let mut err = Output::new(Fd::STDERR, true);
        err.unbuffered = true;
        self.outs.push(err);
        // Saídas de `w`: criadas (truncadas) antes de ler a entrada.
        for t in &self.prog.outputs {
            let idx = match t {
                OutTarget::Stdout => 0,
                OutTarget::Stderr => 1,
                OutTarget::File(path) => {
                    let flags = OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC;
                    match sys::current().openat(Fd::CWD, path, flags, 0o666) {
                        Ok(fd) => {
                            self.outs.push(Output::new(fd, self.o.unbuffered));
                            self.outs.len() - 1
                        }
                        Err(e) => {
                            let mut m = b"couldn't open file ".to_vec();
                            m.extend_from_slice(&errno_msg(path, e));
                            return self.fatal(Fatal(m));
                        }
                    }
                }
            };
            self.w_index.push(idx);
        }
        self.readers = (0..self.prog.readers.len()).map(|_| None).collect();
        let result = if let Some(suffix) = self.o.in_place.clone() {
            self.run_in_place(files, &suffix)
        } else {
            let mut input = Input::new(files, self.o.separate, self.o.unbuffered, self.delim);
            self.run_stream(&mut input, files)
        };
        let code = match result {
            Ok(Some(code)) => code,
            Ok(None) => {
                if self.bad_input {
                    2
                } else {
                    0
                }
            }
            Err(f) => return self.fatal(f),
        };
        if let Err(f) = self.flush_all() {
            return self.fatal(f);
        }
        code
    }

    fn fatal(&mut self, f: Fatal) -> i32 {
        let _ = self.flush_all();
        error(self.progname, &f.0);
        4
    }

    fn flush_all(&mut self) -> Result<(), Fatal> {
        for (i, o) in self.outs.iter_mut().enumerate() {
            o.out.flush();
            if let Some(e) = o.out.error.take()
                && i == 0
            {
                return Err(Fatal(errno_msg(b"couldn't write items to stdout", e)));
            }
        }
        Ok(())
    }

    /// Processa a entrada; `Some(código)` se o programa saiu com `q`/`Q`.
    fn run_stream(&mut self, input: &mut Input<'_>, files: &[Vec<u8>]) -> Result<Option<i32>, Fatal> {
        loop {
            let has_next = input.peek(self)?;
            if !has_next {
                if input.single && input.next < files.len() {
                    // `-s`: próximo arquivo, numeração e faixas zeradas.
                    input.start_next_file(self)?;
                    self.reset_stream();
                    continue;
                }
                return Ok(None);
            }
            let Some(line) = input.take() else { return Ok(None) };
            if input.single && self.line_no == 0 {
                self.reset_stream();
            }
            self.load_line(line);
            if let Some(code) = self.cycle(input)? {
                return Ok(Some(code));
            }
        }
    }

    /// Começo de um fluxo (arquivo, com `-s`/`-i`): faixas, `0,/re/` e leitores do `R`.
    fn reset_stream(&mut self) {
        for (i, c) in self.prog.cmds.iter().enumerate() {
            self.ranges[i] = Range { active: matches!(c.a1, Some(Addr::Zero)), end_line: 0 };
        }
        if self.o.separate || self.o.in_place.is_some() {
            for r in &mut self.readers {
                *r = None;
            }
        }
    }

    fn load_line(&mut self, line: Line) {
        self.ps = line.data;
        self.ps_chomped = line.chomped;
        self.line_no += 1;
        self.replaced = false;
    }

    /// Um ciclo (com os recomeços do `D`). `Some(código)` se o programa termina.
    fn cycle(&mut self, input: &mut Input<'_>) -> Result<Option<i32>, Fatal> {
        loop {
            let flow = self.execute(input)?;
            match flow {
                Flow::End => {
                    if !self.o.quiet {
                        self.emit_ps(self.main);
                    }
                    self.dump_queue()?;
                    return Ok(None);
                }
                Flow::Delete => {
                    self.dump_queue()?;
                    return Ok(None);
                }
                Flow::Restart => continue,
                Flow::Quit { code, autoprint, dump } => {
                    if autoprint && !self.o.quiet {
                        self.emit_ps(self.main);
                    }
                    if dump {
                        self.dump_queue()?;
                    } else {
                        self.queue.clear();
                    }
                    return Ok(Some(code));
                }
            }
        }
    }

    fn emit_ps(&mut self, out: usize) {
        let d = self.delim;
        let (ps, chomped) = (std::mem::take(&mut self.ps), self.ps_chomped);
        self.outs[out].line(&ps, chomped, d);
        self.ps = ps;
    }

    fn raw(&mut self, out: usize, data: &[u8]) {
        let d = self.delim;
        self.outs[out].raw(data, d);
    }

    fn dump_queue(&mut self) -> Result<(), Fatal> {
        let items = std::mem::take(&mut self.queue);
        for it in items {
            match it {
                Pending::Text(t) => self.raw(self.main, &t),
                Pending::File(path) => {
                    if let Some(data) = read_whole(&path) {
                        self.raw(self.main, &data);
                    }
                }
                Pending::Line(l) => {
                    let d = self.delim;
                    let m = self.main;
                    self.outs[m].line(&l.data, l.chomped, d);
                }
            }
        }
        Ok(())
    }

    fn tick(&mut self) {
        self.steps += 1;
        if self.steps >= 4096 {
            self.steps = 0;
            sys::checkpoint();
        }
    }

    /// Executa o script uma vez sobre o espaço de padrão.
    fn execute(&mut self, input: &mut Input<'_>) -> Result<Flow, Fatal> {
        let cmds = &self.prog.cmds;
        let mut pc = 0usize;
        while pc < cmds.len() {
            self.tick();
            let cmd = &cmds[pc];
            if !self.selected(pc, cmd, input)? {
                pc = match cmd.kind {
                    Kind::Block { end } => end + 1,
                    _ => pc + 1,
                };
                continue;
            }
            match &cmd.kind {
                Kind::Block { .. } | Kind::BlockEnd | Kind::Label | Kind::Nop => {}
                Kind::Branch(t) => {
                    pc = t.unwrap_or(cmds.len());
                    continue;
                }
                Kind::BranchSub(t) => {
                    if self.replaced {
                        self.replaced = false;
                        pc = t.unwrap_or(cmds.len());
                        continue;
                    }
                }
                Kind::BranchNoSub(t) => {
                    if !self.replaced {
                        pc = t.unwrap_or(cmds.len());
                        continue;
                    }
                    self.replaced = false;
                }
                Kind::Text { kind, text } => match kind {
                    TextKind::Append => self.queue.push(Pending::Text(text.clone())),
                    TextKind::Insert => self.raw(self.main, &text.clone()),
                    TextKind::Change => {
                        let range_open = cmd.a2.is_some() && !cmd.negate && self.ranges[pc].active;
                        if !range_open {
                            self.raw(self.main, &text.clone());
                        }
                        return Ok(Flow::Delete);
                    }
                },
                Kind::Exec(cmdline) => match cmdline {
                    Some(c) => {
                        let out = run_shell(c);
                        self.raw(self.main, &out);
                    }
                    None => {
                        let mut out = run_shell(&self.ps.clone());
                        if out.last() == Some(&b'\n') {
                            out.pop();
                        }
                        self.ps = out;
                    }
                },
                Kind::LineNumber => {
                    let mut s = self.line_no.to_string().into_bytes();
                    s.push(self.delim);
                    self.raw(self.main, &s);
                }
                Kind::Delete => return Ok(Flow::Delete),
                Kind::DeleteFirst => match self.ps.iter().position(|&b| b == self.delim_join()) {
                    None => return Ok(Flow::Delete),
                    Some(i) => {
                        self.ps.drain(..=i);
                        return Ok(Flow::Restart);
                    }
                },
                Kind::FileName => {
                    let mut name = input.current_name.clone();
                    name.push(b'\n');
                    self.raw(self.main, &name);
                }
                Kind::Get => {
                    self.ps = self.hs.clone();
                    self.ps_chomped = self.hs_chomped;
                }
                Kind::GetAppend => {
                    let j = self.delim_join();
                    self.ps.push(j);
                    self.ps.extend_from_slice(&self.hs.clone());
                    self.ps_chomped = self.hs_chomped;
                }
                Kind::Hold => {
                    self.hs = self.ps.clone();
                    self.hs_chomped = self.ps_chomped;
                }
                Kind::HoldAppend => {
                    let j = self.delim_join();
                    self.hs.push(j);
                    self.hs.extend_from_slice(&self.ps.clone());
                    self.hs_chomped = self.ps_chomped;
                }
                Kind::Exchange => {
                    std::mem::swap(&mut self.ps, &mut self.hs);
                    std::mem::swap(&mut self.ps_chomped, &mut self.hs_chomped);
                }
                Kind::List(n) => {
                    let width = n.unwrap_or(self.o.line_len);
                    let text = list_text(&self.ps, width, self.delim);
                    self.raw(self.main, &text);
                }
                Kind::BadL => return Err(Fatal(b"INTERNAL ERROR: Bad cmd L".to_vec())),
                Kind::Next => {
                    if !input.peek(self)? {
                        if !self.o.quiet {
                            self.emit_ps(self.main);
                        }
                        return Ok(Flow::Quit { code: 0, autoprint: false, dump: true });
                    }
                    if !self.o.quiet {
                        self.emit_ps(self.main);
                    }
                    self.dump_queue()?;
                    if let Some(line) = input.take() {
                        self.ps = line.data;
                        self.ps_chomped = line.chomped;
                        self.line_no += 1;
                    }
                }
                Kind::NextAppend => {
                    if !input.peek(self)? {
                        if self.o.posix_n {
                            return Ok(Flow::Quit { code: 0, autoprint: false, dump: true });
                        }
                        return Ok(Flow::Quit { code: 0, autoprint: true, dump: true });
                    }
                    self.dump_queue()?;
                    if let Some(line) = input.take() {
                        let j = self.delim_join();
                        self.ps.push(j);
                        self.ps.extend_from_slice(&line.data);
                        self.ps_chomped = line.chomped;
                        self.line_no += 1;
                    }
                }
                Kind::Print => self.emit_ps(self.main),
                Kind::PrintFirst => self.print_first(self.main),
                Kind::Quit(code) => return Ok(Flow::Quit { code: *code, autoprint: true, dump: true }),
                Kind::QuitSilent(code) => return Ok(Flow::Quit { code: *code, autoprint: false, dump: false }),
                Kind::ReadFile { path, prepend } => {
                    if *prepend {
                        if let Some(data) = read_whole(path) {
                            self.raw(self.main, &data);
                        }
                    } else {
                        self.queue.push(Pending::File(path.clone()));
                    }
                }
                Kind::ReadLine(id) => {
                    if let Some(line) = self.read_r_line(*id) {
                        self.queue.push(Pending::Line(line));
                    }
                }
                Kind::Subst(s) => self.subst(s, cmd)?,
                Kind::Translit(map) => self.translit(map),
                Kind::Write(id) => {
                    let o = self.w_index[*id];
                    self.emit_ps(o);
                }
                Kind::WriteFirst(id) => {
                    let o = self.w_index[*id];
                    self.print_first(o);
                }
                Kind::Zap => self.ps.clear(),
            }
            pc += 1;
        }
        Ok(Flow::End)
    }

    /// Delimitador usado pra juntar linhas (`N`, `G`, `H`) e achar a primeira (`D`, `P`).
    fn delim_join(&self) -> u8 {
        self.delim
    }

    fn print_first(&mut self, out: usize) {
        let j = self.delim_join();
        match self.ps.iter().position(|&b| b == j) {
            Some(i) => {
                let first = self.ps[..i].to_vec();
                let d = self.delim;
                self.outs[out].line(&first, true, d);
            }
            None => self.emit_ps(out),
        }
    }

    fn read_r_line(&mut self, id: usize) -> Option<Line> {
        if self.readers[id].is_none() {
            let path = &self.prog.readers[id];
            let opened = if path.as_slice() == b"/dev/stdin" {
                Some(LineReader::new(Fd::STDIN, true))
            } else {
                sys::open(path, OFlags::RDONLY | OFlags::CLOEXEC, 0).ok().map(|fd| LineReader::new(fd, false))
            };
            self.readers[id] = Some(opened);
        }
        let delim = self.delim;
        match &mut self.readers[id] {
            Some(Some(r)) => r.read_line(delim).ok().flatten(),
            _ => None,
        }
    }

    /// Endereço do comando `pc` casa a linha corrente?
    fn selected(&mut self, pc: usize, cmd: &Cmd, input: &mut Input<'_>) -> Result<bool, Fatal> {
        let r = match (&cmd.a1, &cmd.a2) {
            (None, _) => true,
            (Some(a1), None) => self.match_a1(a1, input)?,
            (Some(a1), Some(a2)) => self.match_range(pc, a1, a2, input)?,
        };
        Ok(r != cmd.negate)
    }

    fn match_a1(&mut self, a: &Addr, input: &mut Input<'_>) -> Result<bool, Fatal> {
        Ok(match a {
            Addr::Line(n) => self.line_no == *n,
            Addr::Last => input.is_last(self)?,
            Addr::Re(r) => {
                let re = self.resolve(r)?;
                re.re.is_match(&self.ps)
            }
            Addr::Step(first, step) => self.line_no >= *first && (self.line_no - first).is_multiple_of(*step),
            Addr::Zero => false,
        })
    }

    fn match_range(&mut self, pc: usize, a1: &Addr, a2: &Addr2, input: &mut Input<'_>) -> Result<bool, Fatal> {
        let line = self.line_no;
        if self.ranges[pc].active {
            let end = match a2 {
                Addr2::Line(n) => {
                    if line > *n {
                        // A faixa passou do fim (linhas puladas por `n`/`N`): acaba sem casar.
                        self.ranges[pc].active = false;
                        return Ok(false);
                    }
                    line >= *n
                }
                Addr2::Last => input.is_last(self)?,
                Addr2::Re(r) => {
                    let re = self.resolve(r)?;
                    re.re.is_match(&self.ps)
                }
                Addr2::Plus(_) => line >= self.ranges[pc].end_line,
                Addr2::Mult(m) => *m == 0 || line.is_multiple_of(*m),
                Addr2::Step(f, s) => line >= *f && (line - f).is_multiple_of(*s),
            };
            if end {
                self.ranges[pc].active = false;
            }
            return Ok(true);
        }
        if !self.match_a1(a1, input)? {
            return Ok(false);
        }
        let stays = match a2 {
            Addr2::Line(n) => *n > line,
            Addr2::Last => !input.is_last(self)?,
            Addr2::Re(_) => true,
            Addr2::Plus(n) => {
                self.ranges[pc].end_line = line + n;
                *n > 0
            }
            Addr2::Mult(m) => !(*m == 0 || line.is_multiple_of(*m)),
            Addr2::Step(f, s) => !(line >= *f && (line - f).is_multiple_of(*s)),
        };
        self.ranges[pc].active = stays;
        Ok(true)
    }

    /// Regex efetiva (a vazia é a última usada) e registra como última.
    fn resolve(&mut self, r: &Option<Arc<SedRegex>>) -> Result<Arc<SedRegex>, Fatal> {
        match r {
            Some(re) => {
                self.last_regex = Some(re.clone());
                Ok(re.clone())
            }
            None => match &self.last_regex {
                Some(re) => Ok(re.clone()),
                None => Err(self.runtime_error("no previous regular expression")),
            },
        }
    }

    /// Erro de execução no formato do sed (`-e expression #N, char 0: ...`), sai com 1.
    fn runtime_error(&self, msg: &str) -> Fatal {
        let loc = match &self.prog.last_origin {
            Some(super::script::Origin::Expr(n)) => format!("-e expression #{n}, char 0: {msg}").into_bytes(),
            Some(super::script::Origin::File(name)) => {
                let mut v = b"file ".to_vec();
                v.extend_from_slice(name);
                v.extend_from_slice(format!(" line 0: {msg}").as_bytes());
                v
            }
            None => msg.as_bytes().to_vec(),
        };
        let mut v = loc;
        v.insert(0, 1);
        Fatal(v)
    }

    fn subst(&mut self, s: &Subst, _cmd: &Cmd) -> Result<(), Fatal> {
        let re = self.resolve(&s.re)?;
        if s.re.is_none() && s.max_ref > re.nsub {
            return Err(self.runtime_error(&format!("invalid reference \\{} on `s' command's RHS", s.max_ref)));
        }
        let need_groups = s.max_ref > 0;
        let ps = std::mem::take(&mut self.ps);
        let mut out: Vec<u8> = Vec::with_capacity(ps.len());
        let mut start = 0usize;
        let mut last_end = 0usize;
        let mut count: u64 = 0;
        let mut replaced = false;
        loop {
            if start > ps.len() {
                break;
            }
            let caps = if need_groups {
                re.re.captures_at(&ps, start)
            } else {
                re.re.find_at(&ps, start).map(|m| {
                    let _ = m;
                    regex_posix::Captures::clone(&dummy_caps(m.start, m.end))
                })
            };
            let Some(caps) = caps else { break };
            let m = caps.whole();
            if start < m.start {
                out.extend_from_slice(&ps[start..m.start]);
            }
            let countable = !m.is_empty() || count == 0 || m.start > last_end;
            let mut go_on = true;
            if countable {
                count += 1;
            }
            if countable && count >= s.nth {
                append_replacement(&mut out, &s.repl, &caps, &ps);
                replaced = true;
                go_on = s.global;
                start = m.end;
                if m.is_empty() {
                    // Casada vazia substituída: o próximo caractere sai como está.
                    if m.end < ps.len() {
                        let l = char_len(&ps, m.end);
                        out.extend_from_slice(&ps[m.end..m.end + l]);
                        start = m.end + l;
                    } else {
                        start = ps.len() + 1;
                    }
                }
            } else {
                out.extend_from_slice(&ps[m.start..m.end]);
                start = m.end;
                if m.is_empty() {
                    if m.end < ps.len() {
                        let l = char_len(&ps, m.end);
                        out.extend_from_slice(&ps[m.end..m.end + l]);
                        start = m.end + l;
                    } else {
                        start = ps.len() + 1;
                    }
                }
            }
            last_end = m.end;
            if !go_on {
                break;
            }
            self.tick();
        }
        if start < ps.len() {
            out.extend_from_slice(&ps[start..]);
        }
        if !replaced {
            self.ps = ps;
            return Ok(());
        }
        self.ps = out;
        self.replaced = true;
        if s.print & 1 != 0 {
            self.emit_ps(self.main);
        }
        if s.eval {
            let mut res = run_shell(&self.ps.clone());
            if res.last() == Some(&b'\n') {
                res.pop();
            }
            self.ps = res;
        }
        if s.print & 2 != 0 {
            self.emit_ps(self.main);
        }
        if let Some(id) = s.write {
            let o = self.w_index[id];
            self.emit_ps(o);
        }
        Ok(())
    }

    fn translit(&mut self, map: &[(Vec<u8>, Vec<u8>)]) {
        let mut out = Vec::with_capacity(self.ps.len());
        let mut i = 0;
        while i < self.ps.len() {
            let l = char_len(&self.ps, i);
            let ch = &self.ps[i..i + l];
            match map.iter().find(|(s, _)| s.as_slice() == ch) {
                Some((_, d)) => out.extend_from_slice(d),
                None => out.extend_from_slice(ch),
            }
            i += l;
        }
        self.ps = out;
    }

    /// `-i`: cada arquivo vira um fluxo, com saída num temporário que substitui o original.
    fn run_in_place(&mut self, files: &[Vec<u8>], suffix: &[u8]) -> Result<Option<i32>, Fatal> {
        if files.is_empty() {
            return Err(Fatal(b"no input files".to_vec()));
        }
        let sys = sys::current();
        for name in files {
            let target = if self.o.follow_symlinks { resolve_symlinks(name) } else { name.clone() };
            let fd = match sys.openat(Fd::CWD, &target, OFlags::RDONLY | OFlags::CLOEXEC, 0) {
                Ok(fd) => fd,
                Err(e) => {
                    let mut m = b"can't read ".to_vec();
                    m.extend_from_slice(&errno_msg(name, e));
                    error(self.progname, &m);
                    self.bad_input = true;
                    continue;
                }
            };
            let st = match sys.fstat(fd) {
                Ok(st) => st,
                Err(e) => {
                    let _ = sys.close(fd);
                    return Err(Fatal(errno_msg(name, e)));
                }
            };
            if st.file_type() != FileType::Regular {
                let _ = sys.close(fd);
                let mut m = b"couldn't edit ".to_vec();
                m.extend_from_slice(name);
                m.extend_from_slice(b": not a regular file");
                return Err(Fatal(m));
            }
            let dir = match target.iter().rposition(|&b| b == b'/') {
                Some(i) => target[..=i].to_vec(),
                None => Vec::new(),
            };
            let (tmp_path, tmp_fd) = create_temp(&dir).map_err(|e| {
                let mut m = b"couldn't open temporary file ".to_vec();
                let mut p = dir.clone();
                p.extend_from_slice(b"sedXXXXXX");
                m.extend_from_slice(&errno_msg(&p, e));
                Fatal(m)
            })?;
            let _ = sys.fchmod(tmp_fd, st.perm());
            let _ = sys.fchownat(Fd::CWD, &tmp_path, Some(st.uid), Some(st.gid), sysabi::AtFlags::empty());
            self.outs.push(Output::new(tmp_fd, false));
            self.main = self.outs.len() - 1;
            let one = [name.clone()];
            let mut input = Input::new(&one, true, false, self.delim);
            input.preopened(LineReader::new(fd, false), name.clone());
            self.line_no = 0;
            self.reset_stream();
            let r = self.run_stream(&mut input, &one);
            // Fecha o temporário e troca pelo original.
            let main = self.main;
            self.outs[main].out.flush();
            let werr = self.outs[main].out.error.take();
            self.main = 0;
            let _ = sys.close(tmp_fd);
            let _ = sys.close(fd);
            if let Some(e) = werr {
                let _ = sys.unlinkat(Fd::CWD, &tmp_path, sysabi::AtFlags::empty());
                let mut m = b"couldn't write items to ".to_vec();
                m.extend_from_slice(&errno_msg(&tmp_path, e));
                return Err(Fatal(m));
            }
            if !suffix.is_empty() {
                let backup = backup_name(&target, suffix);
                if let Err(e) = sys.renameat2(Fd::CWD, &target, Fd::CWD, &backup, sysabi::RenameFlags::empty()) {
                    let mut m = b"cannot rename ".to_vec();
                    m.extend_from_slice(&errno_msg(&target, e));
                    return Err(Fatal(m));
                }
            }
            if let Err(e) = sys.renameat2(Fd::CWD, &tmp_path, Fd::CWD, &target, sysabi::RenameFlags::empty()) {
                let mut m = b"cannot rename ".to_vec();
                m.extend_from_slice(&errno_msg(&tmp_path, e));
                return Err(Fatal(m));
            }
            match r {
                Ok(Some(code)) => return Ok(Some(code)),
                Ok(None) => {}
                Err(f) => return Err(f),
            }
        }
        Ok(None)
    }
}

impl<'a> Input<'a> {
    fn new(names: &'a [Vec<u8>], single: bool, minimal: bool, delim: u8) -> Input<'a> {
        Input { names, next: 0, cur: None, ahead: None, current_name: Vec::new(), single, minimal, delim }
    }

    fn preopened(&mut self, r: LineReader, name: Vec<u8>) {
        self.cur = Some((r, name));
        self.next = self.names.len();
    }

    /// Abre o próximo arquivo da lista; erros de abertura saem no stderr e contam pro código 2.
    fn open_next(&mut self, ex: &mut Exec<'_>) -> bool {
        while self.next < self.names.len() {
            let name = self.names[self.next].clone();
            self.next += 1;
            if name == b"-" {
                self.cur = Some((LineReader::new(Fd::STDIN, self.minimal), name));
                return true;
            }
            match sys::open(&name, OFlags::RDONLY | OFlags::CLOEXEC, 0) {
                Ok(fd) => {
                    self.cur = Some((LineReader::new(fd, false), name));
                    return true;
                }
                Err(e) => {
                    let mut m = b"can't read ".to_vec();
                    m.extend_from_slice(&errno_msg(&name, e));
                    error(ex.progname, &m);
                    ex.bad_input = true;
                }
            }
        }
        false
    }

    /// `-s`: passa pro próximo arquivo.
    fn start_next_file(&mut self, ex: &mut Exec<'_>) -> Result<(), Fatal> {
        self.close_current();
        self.open_next(ex);
        ex.line_no = 0;
        Ok(())
    }

    fn close_current(&mut self) {
        if let Some((r, name)) = self.cur.take()
            && name != b"-"
        {
            let _ = sys::close(r.fd);
        }
    }

    /// Garante a linha de antecipação; `false` se a entrada (do fluxo) acabou.
    fn peek(&mut self, ex: &mut Exec<'_>) -> Result<bool, Fatal> {
        if self.ahead.is_some() {
            return Ok(true);
        }
        loop {
            if self.cur.is_none() {
                if self.single && (self.next > 0 || self.names.is_empty()) {
                    return Ok(false);
                }
                if !self.open_next(ex) {
                    return Ok(false);
                }
            }
            let delim = self.delim;
            let (r, name) = self.cur.as_mut().expect("arquivo aberto");
            match r.read_line(delim) {
                Ok(Some(line)) => {
                    let n = name.clone();
                    self.ahead = Some((line, n));
                    return Ok(true);
                }
                Ok(None) => {
                    if self.single {
                        return Ok(false);
                    }
                    self.close_current();
                }
                Err(e) => {
                    let mut m = b"read error on ".to_vec();
                    let shown = if name.as_slice() == b"-" { b"stdin".to_vec() } else { name.clone() };
                    m.extend_from_slice(&errno_msg(&shown, e));
                    return Err(Fatal(m));
                }
            }
        }
    }

    fn take(&mut self) -> Option<Line> {
        let (line, name) = self.ahead.take()?;
        self.current_name = name;
        Some(line)
    }

    /// A linha corrente é a última (do fluxo)?
    fn is_last(&mut self, ex: &mut Exec<'_>) -> Result<bool, Fatal> {
        Ok(!self.peek(ex)?)
    }
}

fn char_len(s: &[u8], i: usize) -> usize {
    regex_posix::nfa::decode_at(s, i).map(|(_, l)| l).unwrap_or(1)
}

fn dummy_caps(s: usize, e: usize) -> regex_posix::Captures {
    regex_posix::Captures::whole_only(s, e)
}

/// Monta a substituição com as conversões de caixa (`\U`, `\L`, `\E`, `\u`, `\l`).
fn append_replacement(out: &mut Vec<u8>, parts: &[Repl], caps: &regex_posix::Captures, hay: &[u8]) {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Case {
        None,
        Upper,
        Lower,
    }
    let mut mode = Case::None;
    let mut one = Case::None;
    let push = |text: &[u8], out: &mut Vec<u8>, mode: Case, one: &mut Case| {
        if text.is_empty() {
            return;
        }
        if mode == Case::None && *one == Case::None {
            out.extend_from_slice(text);
            return;
        }
        let mut i = 0;
        let mut first = true;
        while i < text.len() {
            let l = char_len(text, i);
            let c = &text[i..i + l];
            let how = if first && *one != Case::None { *one } else { mode };
            first = false;
            match (std::str::from_utf8(c).ok().and_then(|s| s.chars().next()), how) {
                (Some(ch), Case::Upper) => push_char(out, upper(ch)),
                (Some(ch), Case::Lower) => push_char(out, lower(ch)),
                _ => out.extend_from_slice(c),
            }
            i += l;
        }
        *one = Case::None;
    };
    for p in parts {
        match p {
            Repl::Lit(t) => push(t, out, mode, &mut one),
            Repl::Group(n) => {
                if let Some(m) = caps.get(*n) {
                    push(&hay[m.start..m.end], out, mode, &mut one);
                }
            }
            Repl::Case(op) => match op {
                CaseOp::Upper => {
                    mode = Case::Upper;
                    one = Case::None;
                }
                CaseOp::Lower => {
                    mode = Case::Lower;
                    one = Case::None;
                }
                CaseOp::End => {
                    mode = Case::None;
                    one = Case::None;
                }
                CaseOp::UpperOne => one = Case::Upper,
                CaseOp::LowerOne => one = Case::Lower,
            },
        }
    }
}

fn push_char(out: &mut Vec<u8>, c: char) {
    let mut b = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
}

fn upper(c: char) -> char {
    regex_posix::parse::to_upper(c)
}

fn lower(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// Saída do `l`: escapes C, octal pros bytes não imprimíveis e não ASCII, quebra com `\` antes de
/// passar de `width - 1` colunas (0 desliga), `$` no fim.
fn list_text(ps: &[u8], width: usize, delim: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let mut col = 0usize;
    for &b in ps {
        let unit: Vec<u8> = match b {
            b'\\' => b"\\\\".to_vec(),
            0x07 => b"\\a".to_vec(),
            0x08 => b"\\b".to_vec(),
            0x0c => b"\\f".to_vec(),
            b'\n' => b"\\n".to_vec(),
            b'\r' => b"\\r".to_vec(),
            b'\t' => b"\\t".to_vec(),
            0x0b => b"\\v".to_vec(),
            0x20..=0x7e => vec![b],
            _ => format!("\\{b:03o}").into_bytes(),
        };
        if width > 0 && col + unit.len() > width - 1 {
            out.extend_from_slice(b"\\\n");
            col = 0;
        }
        col += unit.len();
        out.extend_from_slice(&unit);
    }
    out.push(b'$');
    out.push(delim);
    out
}

/// Conteúdo inteiro de um arquivo (`r`); `None` se não abre (o sed ignora em silêncio).
fn read_whole(path: &[u8]) -> Option<Vec<u8>> {
    if path == b"/dev/stdin" {
        return crate::io::read_all(Fd::STDIN).ok();
    }
    sys::read_file(path).ok()
}

/// Roda `sh -c cmd` e devolve a saída padrão dele.
fn run_shell(cmd: &[u8]) -> Vec<u8> {
    use sysabi::{FdAction, ProcAttrs, SpawnSpec, WaitOptions, WaitTarget};
    let sys = sys::current();
    let Ok((r, w)) = sys.pipe2(OFlags::CLOEXEC) else { return Vec::new() };
    let spec = SpawnSpec {
        path: b"/bin/sh".to_vec(),
        argv: vec![b"sh".to_vec(), b"-c".to_vec(), cmd.to_vec()],
        attrs: ProcAttrs { fd_actions: vec![FdAction::Dup2 { from: w, to: Fd::STDOUT }], ..ProcAttrs::default() },
    };
    let pid = sys.spawn(spec);
    let _ = sys.close(w);
    let out = crate::io::read_all(r).unwrap_or_default();
    let _ = sys.close(r);
    if let Ok(pid) = pid {
        let _ = sys.wait4(WaitTarget::Pid(pid), WaitOptions::empty());
    }
    out
}

/// Cria `dir/sedXXXXXX` exclusivo.
fn create_temp(dir: &[u8]) -> Result<(Vec<u8>, Fd), Errno> {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let sys = sys::current();
    for _ in 0..100 {
        let mut rnd = [0u8; 6];
        let _ = sys.getrandom(&mut rnd);
        let mut path = dir.to_vec();
        path.extend_from_slice(b"sed");
        path.extend(rnd.iter().map(|b| CHARS[*b as usize % CHARS.len()]));
        match sys.openat(Fd::CWD, &path, OFlags::WRONLY | OFlags::CREAT | OFlags::EXCL | OFlags::CLOEXEC, 0o600) {
            Ok(fd) => return Ok((path, fd)),
            Err(Errno::EEXIST) => continue,
            Err(e) => return Err(e),
        }
    }
    Err(Errno::EEXIST)
}

/// Nome do backup: com `*`, cada um vira o nome do arquivo; sem, o sufixo vai no fim. Fica no
/// diretório do arquivo.
fn backup_name(path: &[u8], suffix: &[u8]) -> Vec<u8> {
    let (dir, base) = match path.iter().rposition(|&b| b == b'/') {
        Some(i) => (&path[..=i], &path[i + 1..]),
        None => (&path[..0], path),
    };
    let mut name = Vec::new();
    if suffix.contains(&b'*') {
        for &b in suffix {
            if b == b'*' {
                name.extend_from_slice(base);
            } else {
                name.push(b);
            }
        }
    } else {
        name.extend_from_slice(base);
        name.extend_from_slice(suffix);
    }
    if name.contains(&b'/') && !name.starts_with(b"/") {
        let mut v = dir.to_vec();
        v.extend_from_slice(&name);
        return v;
    }
    let mut v = dir.to_vec();
    v.extend_from_slice(&name);
    v
}

/// Segue symlinks até o arquivo de verdade (`--follow-symlinks`).
fn resolve_symlinks(path: &[u8]) -> Vec<u8> {
    let mut cur = path.to_vec();
    for _ in 0..40 {
        match sys::current().readlinkat(Fd::CWD, &cur) {
            Ok(target) => {
                if target.starts_with(b"/") {
                    cur = target;
                } else {
                    let dir = match cur.iter().rposition(|&b| b == b'/') {
                        Some(i) => cur[..=i].to_vec(),
                        None => Vec::new(),
                    };
                    let mut v = dir;
                    v.extend_from_slice(&target);
                    cur = v;
                }
            }
            Err(_) => return cur,
        }
    }
    cur
}
