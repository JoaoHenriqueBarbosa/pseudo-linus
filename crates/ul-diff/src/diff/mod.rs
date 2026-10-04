//! `diff` (GNU diffutils 3.10): o laço sobre operandos e diretórios, a leitura dos arquivos pelo
//! `sysabi`, a detecção de binário, os cabeçalhos com data e as mensagens e códigos de saída do GNU.
//!
//! As peças reaproveitáveis (motor de alinhamento, normalização, formatadores) são públicas pra `sdiff`
//! e `diff3`.

pub mod engine;
pub mod fnmatch;
pub mod format;
pub mod help;
pub mod ifdef;
pub mod options;
pub mod paginate;
pub mod side;
pub mod text;

use std::ffi::OsString;

use sysabi::{Ctx, Errno, Fd, FileType, Stat};

use crate::sysutil::{self, Output};
use crate::tz;

use format::{Change, Look, Paint, Printer};
use options::{ColorWhen, Opts, Parsed, Style};

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv = sysutil::args_bytes(args);
    let argv0 = sysutil::argv0(&argv);
    let opts = match options::parse(&argv) {
        Parsed::Run(o) => *o,
        Parsed::Exit(code) => return code,
    };
    let mut d = Differ::new(argv0, opts);
    let status = d.run();
    d.finish(status)
}

/// Um lado da comparação.
struct Side {
    /// Nome mostrado (caminho como foi dado, ou montado no diretório).
    name: Vec<u8>,
    stat: Option<Stat>,
    /// Ausente tratado como vazio (`-N`/`--unidirectional-new-file`).
    absent: bool,
    stdin: bool,
}

/// Linhas e dados lidos de um lado.
struct Content {
    data: Vec<u8>,
}

/// Entrada de diretório pra comparação.
#[derive(Clone)]
struct DirName {
    name: Vec<u8>,
}

pub(crate) struct Differ {
    argv0: String,
    opts: Opts,
    out: Output,
    tz: Option<jiff::tz::TimeZone>,
    color: bool,
    excludes: Vec<Vec<u8>>,
    excludes_loaded: bool,
}

/// Aspas dos nomes nos cabeçalhos (como o diffutils 3.10): entre aspas duplas, com escapes do C e octal,
/// quando o nome tem espaço, aspas, barra invertida, controle ou byte não ASCII.
pub fn quote_name(name: &[u8]) -> Vec<u8> {
    let needs = name.iter().any(|&b| b < 0x20 || b == b' ' || b == b'"' || b == b'\\' || b >= 0x80);
    if !needs {
        return name.to_vec();
    }
    let mut out = vec![b'"'];
    for &b in name {
        match b {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            0x07 => out.extend_from_slice(b"\\a"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x0c => out.extend_from_slice(b"\\f"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x0b => out.extend_from_slice(b"\\v"),
            b if b < 0x20 || b >= 0x80 => out.extend_from_slice(format!("\\{b:03o}").as_bytes()),
            b => out.push(b),
        }
    }
    out.push(b'"');
    out
}

/// Nome do tipo de arquivo como o `file_type` do gnulib descreve.
fn type_name(st: &Stat) -> &'static str {
    match st.file_type() {
        FileType::Regular => {
            if st.size == 0 {
                "regular empty file"
            } else {
                "regular file"
            }
        }
        FileType::Directory => "directory",
        FileType::Symlink => "symbolic link",
        FileType::Fifo => "fifo",
        FileType::CharDevice => "character special file",
        FileType::BlockDevice => "block special file",
        FileType::Socket => "socket",
    }
}

fn is_dir(st: &Option<Stat>) -> bool {
    st.as_ref().is_some_and(|s| s.file_type() == FileType::Directory)
}

fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = dir.to_vec();
    if !p.ends_with(b"/") {
        p.push(b'/');
    }
    p.extend_from_slice(name);
    p
}

/// Script de mudanças de um par de arquivos de texto já divididos em linhas, com a normalização, o
/// motor e as marcas de ignorável (`-B`, `-I`) das opções. Vazio quando não há diferença.
pub fn script_for(opts: &Opts, la: &[&[u8]], lb: &[&[u8]]) -> Vec<Change> {
    let interned = text::intern(la, lb, &opts.norm);
    if interned.a == interned.b {
        return Vec::new();
    }
    let eopts = engine::Options { minimal: opts.minimal, speed_large_files: opts.speed_large_files, horizon: opts.horizon };
    let al = engine::compare(la, lb, &interned.a, &interned.b, interned.classes, eopts);
    let mut script = format::build_script(&al.changed_a, &al.changed_b);
    if opts.ignore_blank_lines || !opts.ignore_regex.is_empty() {
        mark_ignorable(opts, &mut script, la, lb);
    }
    script
}

/// `-B`/`-I`: marca como ignoráveis os blocos em que toda linha apagada e inserida é ignorável (em
/// branco com `-B`; só espaço também conta como branco com `-b` ou `-w`).
fn mark_ignorable(opts: &Opts, script: &mut [Change], la: &[&[u8]], lb: &[&[u8]]) {
    let skip_space = opts.norm.ignore_all_space || opts.norm.ignore_space_change;
    let trivial = |line: &[u8]| -> bool {
        let body = line.strip_suffix(b"\n").unwrap_or(line);
        let blank = if skip_space { body.iter().all(|b| text::is_space(*b)) } else { body.is_empty() };
        opts.ignore_blank_lines && blank
    };
    for ch in script.iter_mut() {
        let del = &la[ch.line0..ch.line0 + ch.deleted];
        let ins = &lb[ch.line1..ch.line1 + ch.inserted];
        ch.ignore = del.iter().all(|l| trivial(l)) && ins.iter().all(|l| trivial(l));
    }
}

/// Se o conteúdo tem NUL no primeiro bloco (`blksize` bytes), como o GNU decide "binário".
pub fn looks_binary(data: &[u8], blksize: usize) -> bool {
    data[..data.len().min(blksize.max(1))].contains(&0)
}

impl Differ {
    fn new(argv0: String, opts: Opts) -> Differ {
        let color = match opts.color {
            ColorWhen::Never => false,
            ColorWhen::Always => true,
            ColorWhen::Auto => {
                let tty = sysabi::sys::try_current().is_some_and(|s| s.isatty(Fd::STDOUT));
                let term = sysutil::getenv("TERM");
                tty && term.is_some_and(|t| t != b"dumb")
            }
        };
        Differ { argv0, opts, out: Output::stdout(), tz: None, color, excludes: Vec::new(), excludes_loaded: false }
    }

    fn finish(&mut self, status: i32) -> i32 {
        match self.out.finish() {
            Ok(()) => status,
            Err(e) => {
                sysutil::error(&self.argv0, format!("standard output: {}", e.message()));
                2
            }
        }
    }

    fn error(&mut self, msg: impl AsRef<[u8]>) {
        self.out.flush();
        sysutil::error(&self.argv0, msg);
    }

    fn error_path(&mut self, path: &[u8], e: Errno) {
        self.out.flush();
        sysutil::error_path(&self.argv0, path, e);
    }

    fn try_help(&mut self, msg: &str) -> i32 {
        self.out.flush();
        let a = self.argv0.clone();
        sysutil::eprint(format!("{a}: {msg}\n{a}: Try '{a} --help' for more information.\n"));
        2
    }

    fn run(&mut self) -> i32 {
        let operands = self.opts.operands.clone();
        if let (Some(_), Some(_)) = (&self.opts.from_file, &self.opts.to_file) {
            self.error("--from-file and --to-file both specified");
            return 2;
        }
        if let Some(from) = self.opts.from_file.clone() {
            let mut status = 0;
            for op in &operands {
                status = status.max(self.compare_top(&from, op));
            }
            return status;
        }
        if let Some(to) = self.opts.to_file.clone() {
            let mut status = 0;
            for op in &operands {
                status = status.max(self.compare_top(op, &to));
            }
            return status;
        }
        if operands.len() < 2 {
            let last = match operands.last() {
                Some(o) => String::from_utf8_lossy(o).into_owned(),
                None => self.argv0.clone(),
            };
            return self.try_help(&format!("missing operand after '{last}'"));
        }
        if operands.len() > 2 {
            return self.try_help(&format!("extra operand '{}'", String::from_utf8_lossy(&operands[2])));
        }
        self.compare_top(&operands[0], &operands[1])
    }

    fn load_excludes(&mut self) -> i32 {
        if self.excludes_loaded {
            return 0;
        }
        self.excludes_loaded = true;
        self.excludes = self.opts.excludes.clone();
        let mut status = 0;
        for f in self.opts.exclude_from.clone() {
            match sysutil::read_path(&f) {
                Ok(data) => {
                    for line in data.split(|&b| b == b'\n') {
                        if !line.is_empty() {
                            self.excludes.push(line.to_vec());
                        }
                    }
                }
                Err(e) => {
                    self.error_path(&f, e);
                    status = 2;
                }
            }
        }
        status
    }

    fn excluded(&self, name: &[u8]) -> bool {
        self.excludes.iter().any(|p| fnmatch::fnmatch(p, name, false))
    }

    fn stat_of(&self, path: &[u8]) -> Result<Stat, Errno> {
        if path == b"-" {
            return sysutil::fstat(Fd::STDIN);
        }
        if self.opts.no_dereference { sysutil::lstat(path) } else { sysutil::stat(path) }
    }

    fn compare_top(&mut self, p0: &[u8], p1: &[u8]) -> i32 {
        let s = self.load_excludes();
        s.max(self.compare_paths(p0, p1, true, [false, false]))
    }

    /// Compara dois caminhos. `top` = operandos da linha de comando; `absent[k]` = já se sabe que o
    /// lado `k` não existe (diretório com `-N`).
    fn compare_paths(&mut self, p0: &[u8], p1: &[u8], top: bool, absent: [bool; 2]) -> i32 {
        let paths = [p0, p1];
        let mut sides: Vec<Side> = Vec::with_capacity(2);
        let mut trouble = false;
        for k in 0..2 {
            let p = paths[k];
            let mut side = Side { name: p.to_vec(), stat: None, absent: absent[k], stdin: p == b"-" };
            if !side.absent {
                match self.stat_of(p) {
                    Ok(st) => side.stat = Some(st),
                    Err(e) => {
                        let allow =
                            e == Errno::ENOENT && (self.opts.new_file || (k == 0 && self.opts.unidirectional_new_file));
                        let other_exists = top && self.stat_of(paths[1 - k]).is_ok();
                        if allow && (!top || other_exists) {
                            side.absent = true;
                        } else {
                            self.error_path(p, e);
                            trouble = true;
                        }
                    }
                }
            }
            sides.push(side);
        }
        if trouble {
            return 2;
        }
        let (d0, d1) = (is_dir(&sides[0].stat), is_dir(&sides[1].stat));

        // Arquivo contra diretório na linha de comando: compara com o arquivo de mesmo nome dentro.
        if top && d0 != d1 && !sides[0].absent && !sides[1].absent {
            let (dir_k, file_k) = if d0 { (0, 1) } else { (1, 0) };
            if sides[file_k].stdin {
                return self.try_help_like("cannot compare '-' to a directory");
            }
            let joined = join(&sides[dir_k].name, sysutil::basename(&sides[file_k].name));
            let (n0, n1) = if dir_k == 0 { (joined, sides[1].name.clone()) } else { (sides[0].name.clone(), joined) };
            return self.compare_paths(&n0, &n1, true, [false, false]);
        }

        if (d0 || sides[0].absent) && (d1 || sides[1].absent) && (d0 || d1) {
            if !top && !self.opts.recursive && d0 && d1 {
                let msg = [b"Common subdirectories: ".as_slice(), p0, b" and ", p1, b"\n"].concat();
                self.out.write(&msg);
                return 0;
            }
            if !top && !self.opts.recursive {
                // Diretório só de um lado, sem -r: o GNU mostra "Only in" no nível de cima; aqui não chega.
                return 0;
            }
            return self.diff_dirs(&sides, top);
        }
        if d0 != d1 && !sides[0].absent && !sides[1].absent {
            let (s0, s1) = (sides[0].stat.as_ref().expect("stat"), sides[1].stat.as_ref().expect("stat"));
            let msg = [
                b"File ".as_slice(),
                p0,
                b" is a ",
                type_name(s0).as_bytes(),
                b" while file ",
                p1,
                b" is a ",
                type_name(s1).as_bytes(),
                b"\n",
            ]
            .concat();
            self.out.write(&msg);
            return 1;
        }

        // Tipos diferentes (fifo contra arquivo, link sem seguir contra arquivo).
        if let (Some(s0), Some(s1)) = (&sides[0].stat, &sides[1].stat) {
            let (t0, t1) = (s0.file_type(), s1.file_type());
            if self.opts.no_dereference && t0 == FileType::Symlink && t1 == FileType::Symlink {
                let l0 = sysabi::sys::current().readlinkat(Fd::CWD, p0).unwrap_or_default();
                let l1 = sysabi::sys::current().readlinkat(Fd::CWD, p1).unwrap_or_default();
                if l0 == l1 {
                    return self.report_identical(&sides);
                }
                let msg = [b"Symbolic links ".as_slice(), p0, b" and ", p1, b" differ\n"].concat();
                self.out.write(&msg);
                return 1;
            }
            if t0 != t1 && !top && !(sides[0].stdin || sides[1].stdin) {
                let msg = [
                    b"File ".as_slice(),
                    p0,
                    b" is a ",
                    type_name(s0).as_bytes(),
                    b" while file ",
                    p1,
                    b" is a ",
                    type_name(s1).as_bytes(),
                    b"\n",
                ]
                .concat();
                self.out.write(&msg);
                return 1;
            }
            // O mesmo arquivo dos dois lados: idêntico sem ler (menos nos estilos de arquivo inteiro).
            if s0.dev == s1.dev && s0.ino == s1.ino && sides[0].stdin == sides[1].stdin && !self.whole_file_style() {
                return self.report_identical(&sides);
            }
        }
        self.compare_files(&sides, top)
    }

    fn try_help_like(&mut self, msg: &str) -> i32 {
        self.error(msg);
        2
    }

    fn report_identical(&mut self, sides: &[Side]) -> i32 {
        if self.opts.report_identical {
            let msg = [b"Files ".as_slice(), &sides[0].name, b" and ", &sides[1].name, b" are identical\n"].concat();
            self.out.write(&msg);
        }
        0
    }

    fn list_dir(&mut self, side: &Side) -> Result<Vec<DirName>, ()> {
        if side.absent {
            return Ok(Vec::new());
        }
        match sysabi::sys::read_dir(&side.name) {
            Ok(entries) => Ok(entries.into_iter().map(|e| DirName { name: e.name }).collect()),
            Err(e) => {
                self.error_path(&side.name, e);
                Err(())
            }
        }
    }

    fn name_cmp(&self, a: &[u8], b: &[u8]) -> std::cmp::Ordering {
        if self.opts.ignore_file_name_case {
            let la: Vec<u8> = a.to_ascii_lowercase();
            let lb: Vec<u8> = b.to_ascii_lowercase();
            la.cmp(&lb).then_with(|| a.cmp(b))
        } else {
            a.cmp(b)
        }
    }

    fn name_eq(&self, a: &[u8], b: &[u8]) -> bool {
        if self.opts.ignore_file_name_case { a.eq_ignore_ascii_case(b) } else { a == b }
    }

    fn diff_dirs(&mut self, sides: &[Side], top: bool) -> i32 {
        let Ok(mut n0) = self.list_dir(&sides[0]) else { return 2 };
        let Ok(mut n1) = self.list_dir(&sides[1]) else { return 2 };
        n0.retain(|e| !self.excluded(&e.name));
        n1.retain(|e| !self.excluded(&e.name));
        n0.sort_by(|a, b| self.name_cmp(&a.name, &b.name));
        n1.sort_by(|a, b| self.name_cmp(&a.name, &b.name));
        if top && let Some(start) = self.opts.starting_file.clone() {
            n0.retain(|e| self.name_cmp(&e.name, &start) != std::cmp::Ordering::Less);
            n1.retain(|e| self.name_cmp(&e.name, &start) != std::cmp::Ordering::Less);
        }
        let mut status = 0;
        let (mut i, mut j) = (0usize, 0usize);
        while i < n0.len() || j < n1.len() {
            sysabi::sys::checkpoint();
            let ord = match (n0.get(i), n1.get(j)) {
                (Some(a), Some(b)) => {
                    if self.name_eq(&a.name, &b.name) {
                        std::cmp::Ordering::Equal
                    } else {
                        self.name_cmp(&a.name, &b.name)
                    }
                }
                (Some(_), None) => std::cmp::Ordering::Less,
                _ => std::cmp::Ordering::Greater,
            };
            let s = match ord {
                std::cmp::Ordering::Equal => {
                    let p0 = join(&sides[0].name, &n0[i].name);
                    let p1 = join(&sides[1].name, &n1[j].name);
                    i += 1;
                    j += 1;
                    self.compare_paths(&p0, &p1, false, [false, false])
                }
                std::cmp::Ordering::Less => {
                    let name = n0[i].name.clone();
                    i += 1;
                    if self.opts.new_file {
                        let p0 = join(&sides[0].name, &name);
                        let p1 = join(&sides[1].name, &name);
                        self.compare_paths(&p0, &p1, false, [false, true])
                    } else {
                        self.only_in(&sides[0].name, &name)
                    }
                }
                std::cmp::Ordering::Greater => {
                    let name = n1[j].name.clone();
                    j += 1;
                    if self.opts.new_file || self.opts.unidirectional_new_file {
                        let p0 = join(&sides[0].name, &name);
                        let p1 = join(&sides[1].name, &name);
                        self.compare_paths(&p0, &p1, false, [true, false])
                    } else {
                        self.only_in(&sides[1].name, &name)
                    }
                }
            };
            status = status.max(s);
        }
        status
    }

    fn only_in(&mut self, dir: &[u8], name: &[u8]) -> i32 {
        let msg = [b"Only in ".as_slice(), dir, b": ", name, b"\n"].concat();
        self.out.write(&msg);
        1
    }

    fn read_side(&mut self, side: &Side) -> Result<Content, ()> {
        if side.absent {
            return Ok(Content { data: Vec::new() });
        }
        let r = if side.stdin { sysutil::read_fd(Fd::STDIN) } else { sysutil::read_path(&side.name) };
        match r {
            Ok(data) => Ok(Content { data }),
            Err(e) => {
                let name = side.name.clone();
                self.error_path(&name, e);
                Err(())
            }
        }
    }

    fn tz(&mut self) -> jiff::tz::TimeZone {
        if self.tz.is_none() {
            self.tz = Some(tz::local());
        }
        self.tz.clone().expect("fuso")
    }

    /// Rótulo do cabeçalho de um lado: `--label`, ou "nome\tdata".
    fn header_label(&mut self, k: usize, side: &Side) -> Vec<u8> {
        if let Some(l) = self.opts.labels.get(k) {
            return l.clone();
        }
        let (sec, nsec) = match (&side.stat, side.absent) {
            (Some(st), false) => (st.mtime.sec, st.mtime.nsec),
            _ => (0, 0),
        };
        let tz = self.tz();
        let mut v = quote_name(&side.name);
        v.push(b'\t');
        v.extend_from_slice(tz::format(sec, nsec, &tz, "%Y-%m-%d %H:%M:%S.%N %z").as_bytes());
        v
    }

    fn compare_files(&mut self, sides: &[Side], top: bool) -> i32 {
        let Ok(c0) = self.read_side(&sides[0]) else { return 2 };
        let Ok(c1) = self.read_side(&sides[1]) else { return 2 };
        let (mut a, mut b) = (c0.data, c1.data);
        let blk0 = sides[0].stat.as_ref().map(|s| s.blksize).filter(|b| *b > 0).unwrap_or(4096) as usize;
        let blk1 = sides[1].stat.as_ref().map(|s| s.blksize).filter(|b| *b > 0).unwrap_or(4096) as usize;
        let binary = !self.opts.text && (a[..a.len().min(blk0)].contains(&0) || b[..b.len().min(blk1)].contains(&0));
        if binary {
            if a == b {
                return self.report_identical(sides);
            }
            let word: &[u8] = if self.opts.brief { b"Files " } else { b"Binary files " };
            let msg = [word, &sides[0].name, b" and ", &sides[1].name, b" differ\n"].concat();
            self.out.write(&msg);
            return 1;
        }
        if self.opts.strip_trailing_cr {
            a = text::strip_trailing_cr(&a);
            b = text::strip_trailing_cr(&b);
        }
        let mut status = 0;
        // O ed não representa linha sem newline: o GNU avisa e acrescenta o newline antes de comparar.
        if matches!(self.opts.style, Style::Ed | Style::ForwardEd) {
            for (k, data) in [&mut a, &mut b].into_iter().enumerate() {
                if data.last().is_some_and(|&c| c != b'\n') {
                    let name = sides[k].name.clone();
                    self.error([name.as_slice(), b": No newline at end of file\n"].concat());
                    data.push(b'\n');
                    status = 2;
                }
            }
        }
        // Lado a lado e `-D` imprimem o arquivo inteiro mesmo quando não há diferença.
        let whole = self.whole_file_style();
        let trivially_equal = a == b;
        let ignoring = !self.opts.norm.is_identity() || self.opts.ignore_blank_lines || !self.opts.ignore_regex.is_empty();
        if trivially_equal && !whole {
            return status.max(self.report_identical(sides));
        }
        if self.opts.brief && !ignoring {
            return status.max(self.report_differ(sides));
        }
        let la = text::split_lines(&a);
        let lb = text::split_lines(&b);
        let script = script_for(&self.opts, &la, &lb);
        let differs = script.iter().any(|c| !c.ignore);
        if !differs && !whole {
            return status.max(self.report_identical(sides));
        }
        if self.opts.brief {
            return status.max(if differs { self.report_differ(sides) } else { self.report_identical(sides) });
        }
        if differs {
            status = status.max(1);
        }
        let mut body = Vec::new();
        self.render(&mut body, &script, &la, &lb, sides);
        if !differs {
            self.out.write(&body);
            return status.max(self.report_identical(sides));
        }
        // Com `-l` a linha "diff ..." vira o título da página do `pr`.
        if !top && !self.opts.paginate {
            let mut line = self.page_title(sides);
            line.push(b'\n');
            self.out.write(&line);
        }
        if self.opts.paginate {
            let title = self.page_title(sides);
            body = paginate::paginate(&body, &title);
        }
        self.out.write(&body);
        status
    }

    /// Estilos que imprimem o arquivo inteiro (`-y`, `-D` e formatos de grupo), menos com `-q`.
    fn whole_file_style(&self) -> bool {
        matches!(self.opts.style, Style::SideBySide | Style::Ifdef) && !self.opts.brief
    }

    fn page_title(&self, sides: &[Side]) -> Vec<u8> {
        let mut t = b"diff".to_vec();
        t.extend_from_slice(&self.opts.switches);
        t.push(b' ');
        t.extend_from_slice(&quote_name(self.opts.labels.first().unwrap_or(&sides[0].name)));
        t.push(b' ');
        t.extend_from_slice(&quote_name(self.opts.labels.get(1).unwrap_or(&sides[1].name)));
        t
    }

    fn report_differ(&mut self, sides: &[Side]) -> i32 {
        let msg = [b"Files ".as_slice(), &sides[0].name, b" and ", &sides[1].name, b" differ\n"].concat();
        self.out.write(&msg);
        1
    }

    /// `-B`/`-I`: marca como ignoráveis os blocos em que toda linha apagada e inserida é ignorável.
    fn look(&self) -> Look {
        Look {
            initial_tab: self.opts.initial_tab,
            suppress_blank_empty: self.opts.suppress_blank_empty,
            expand_tabs: self.opts.expand_tabs,
            tabsize: self.opts.tabsize,
            color: if self.color { Some(self.opts.palette.clone()) } else { None },
        }
    }

    fn render(&mut self, body: &mut Vec<u8>, script: &[Change], la: &[&[u8]], lb: &[&[u8]], sides: &[Side]) {
        let look = self.look();
        match self.opts.style {
            Style::Normal => format::format_normal(&mut Printer { out: body, look: &look }, script, la, lb),
            Style::Unified => {
                let h0 = self.header_label(0, &sides[0]);
                let h1 = self.header_label(1, &sides[1]);
                let mut p = Printer { out: body, look: &look };
                p.control_bytes(&[b"--- ".as_slice(), &h0].concat(), Paint::Header);
                p.control_bytes(&[b"+++ ".as_slice(), &h1].concat(), Paint::Header);
                format::format_unified(&mut p, script, la, lb, self.opts.context, None);
            }
            Style::Context => {
                let h0 = self.header_label(0, &sides[0]);
                let h1 = self.header_label(1, &sides[1]);
                let mut p = Printer { out: body, look: &look };
                p.control_bytes(&[b"*** ".as_slice(), &h0].concat(), Paint::Header);
                p.control_bytes(&[b"--- ".as_slice(), &h1].concat(), Paint::Header);
                format::format_context(&mut p, script, la, lb, self.opts.context, None);
            }
            Style::Ed | Style::ForwardEd => {
                if self.opts.style == Style::Ed {
                    format::format_ed(body, script, lb);
                } else {
                    format::format_forward_ed(body, script, lb);
                }
            }
            Style::Rcs => format::format_rcs(body, script, lb),
            Style::SideBySide => {
                let so = side::SideOptions {
                    width: self.opts.width,
                    tabsize: self.opts.tabsize,
                    expand_tabs: self.opts.expand_tabs,
                    left_column: self.opts.left_column,
                    suppress_common: self.opts.suppress_common,
                };
                side::format_side_by_side(body, script, la, lb, &so);
            }
            Style::Ifdef => ifdef::format_ifdef(body, script, la, lb, &self.opts.formats),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_quoting_like_gnu() {
        assert_eq!(quote_name(b"a.txt"), b"a.txt");
        assert_eq!(quote_name(b"s p"), b"\"s p\"");
        assert_eq!(quote_name(b"t\tq"), b"\"t\\tq\"");
        assert_eq!(quote_name("é".as_bytes()), b"\"\\303\\251\"");
        assert_eq!(quote_name(b"f\x7fg"), b"f\x7fg");
    }
}
