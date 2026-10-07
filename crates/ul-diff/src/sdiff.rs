//! `sdiff` (GNU diffutils 3.10).
//!
//! Sem `-o`, o GNU troca de programa pelo `diff -y` com as opções traduzidas (`-w` vira `-W`, `-W`
//! vira `-w`, `-l` vira `--left-column`, `-s` vira `--suppress-common-lines` no fim); aqui o `diff` roda
//! em processo com o mesmo argv, então mensagens e códigos de saída são os dele. Com `-o`, o modo de
//! fusão: mostra o lado a lado, para em cada diferença com o prompt `%` e lê comandos da entrada padrão
//! (`l`, `r`, `s`, `v`, `q`, `e`, `eb`, `ed`, `el`/`e1`, `er`/`e2`), gravando o resultado no arquivo de
//! saída. Os comandos de edição rodam o `$EDITOR` (ou `ed`) como programa, sem shell, como o GNU.

use std::ffi::OsString;

use sysabi::{Ctx, Errno, Fd, FileType, OFlags, ProcAttrs, SpawnSpec, WaitOptions, WaitStatus, WaitTarget};
use ul_common::getopt::{Getopt, HasArg, Item, LongOpt};

use crate::diff::format::Change;
use crate::diff::options::{self, Parsed};
use crate::diff::side::Geometry;
use crate::diff::{self, text};
use crate::sysutil::{self, Output};

const HELP: &str = r#"Usage: sdiff [OPTION]... FILE1 FILE2
Side-by-side merge of differences between FILE1 and FILE2.

Mandatory arguments to long options are mandatory for short options too.
  -o, --output=FILE            operate interactively, sending output to FILE

  -i, --ignore-case            consider upper- and lower-case to be the same
  -E, --ignore-tab-expansion   ignore changes due to tab expansion
  -Z, --ignore-trailing-space  ignore white space at line end
  -b, --ignore-space-change    ignore changes in the amount of white space
  -W, --ignore-all-space       ignore all white space
  -B, --ignore-blank-lines     ignore changes whose lines are all blank
  -I, --ignore-matching-lines=RE  ignore changes all whose lines match RE
      --strip-trailing-cr      strip trailing carriage return on input
  -a, --text                   treat all files as text

  -w, --width=NUM              output at most NUM (default 130) print columns
  -l, --left-column            output only the left column of common lines
  -s, --suppress-common-lines  do not output common lines

  -t, --expand-tabs            expand tabs to spaces in output
      --tabsize=NUM            tab stops at every NUM (default 8) print columns

  -d, --minimal                try hard to find a smaller set of changes
  -H, --speed-large-files      assume large files, many scattered small changes
      --diff-program=PROGRAM   use PROGRAM to compare files

      --help                   display this help and exit
  -v, --version                output version information and exit

If a FILE is '-', read standard input.
Exit status is 0 if inputs are the same, 1 if different, 2 if trouble.

Report bugs to: bug-diffutils@gnu.org
GNU diffutils home page: <https://www.gnu.org/software/diffutils/>
General help using GNU software: <https://www.gnu.org/gethelp/>
"#;

const VERSION: &str = "sdiff (GNU diffutils) 3.10
Copyright (C) 2023 Free Software Foundation, Inc.
License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>.
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.

Written by Thomas Lord.
";

/// Ajuda dos comandos do modo interativo (vai pro stderr quando o comando é inválido).
const COMMANDS: &str = "ed:\tEdit then use both versions, each decorated with a header.
eb:\tEdit then use both versions.
el or e1:\tEdit then use the left version.
er or e2:\tEdit then use the right version.
e:\tDiscard both versions then edit a new one.
l or 1:\tUse the left version.
r or 2:\tUse the right version.
s:\tSilently include common lines.
v:\tVerbosely include common lines.
q:\tQuit.
";

const DIFF_PROGRAM: i32 = 1000;
const HELP_ID: i32 = 1001;
const STRIP_TRAILING_CR: i32 = 1002;
const TABSIZE: i32 = 1003;

const LONGS: &[LongOpt] = &[
    LongOpt::new("diff-program", HasArg::Required, DIFF_PROGRAM),
    LongOpt::new("expand-tabs", HasArg::No, b't' as i32),
    LongOpt::new("help", HasArg::No, HELP_ID),
    LongOpt::new("ignore-all-space", HasArg::No, b'W' as i32),
    LongOpt::new("ignore-blank-lines", HasArg::No, b'B' as i32),
    LongOpt::new("ignore-case", HasArg::No, b'i' as i32),
    LongOpt::new("ignore-matching-lines", HasArg::Required, b'I' as i32),
    LongOpt::new("ignore-space-change", HasArg::No, b'b' as i32),
    LongOpt::new("ignore-tab-expansion", HasArg::No, b'E' as i32),
    LongOpt::new("ignore-trailing-space", HasArg::No, b'Z' as i32),
    LongOpt::new("left-column", HasArg::No, b'l' as i32),
    LongOpt::new("minimal", HasArg::No, b'd' as i32),
    LongOpt::new("output", HasArg::Required, b'o' as i32),
    LongOpt::new("speed-large-files", HasArg::No, b'H' as i32),
    LongOpt::new("strip-trailing-cr", HasArg::No, STRIP_TRAILING_CR),
    LongOpt::new("suppress-common-lines", HasArg::No, b's' as i32),
    LongOpt::new("tabsize", HasArg::Required, TABSIZE),
    LongOpt::new("text", HasArg::No, b'a' as i32),
    LongOpt::new("version", HasArg::No, b'v' as i32),
    LongOpt::new("width", HasArg::Required, b'w' as i32),
];

pub fn main(ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv = sysutil::args_bytes(args);
    let argv0 = sysutil::argv0(&argv);
    let try_help = |msg: &str| -> i32 {
        sysutil::eprint(format!("{argv0}: {msg}\n{argv0}: Try '{argv0} --help' for more information.\n"));
        2
    };
    let mut dargs: Vec<Vec<u8>> = vec![b"diff".to_vec()];
    let mut suppress = false;
    let mut output: Option<Vec<u8>> = None;
    let mut operands = Vec::new();
    for item in Getopt::from_env(&argv, "abBdEHiI:lo:stvw:WZ", LONGS).after_argv0() {
        let opt = match item {
            Ok(Item::Operand(v)) => {
                operands.push(v);
                continue;
            }
            Ok(Item::Opt(o)) => o,
            Err(e) => {
                sysutil::eprint(e.message_line(&argv0));
                sysutil::eprint(format!("{argv0}: Try '{argv0} --help' for more information.\n"));
                return 2;
            }
        };
        let arg = opt.arg.clone().unwrap_or_default();
        match opt.id {
            x if x == b'a' as i32 => dargs.push(b"-a".to_vec()),
            x if x == b'b' as i32 => dargs.push(b"-b".to_vec()),
            x if x == b'B' as i32 => dargs.push(b"-B".to_vec()),
            x if x == b'd' as i32 => dargs.push(b"-d".to_vec()),
            x if x == b'E' as i32 => dargs.push(b"-E".to_vec()),
            x if x == b'H' as i32 => dargs.push(b"-H".to_vec()),
            x if x == b'i' as i32 => dargs.push(b"-i".to_vec()),
            x if x == b'I' as i32 => {
                dargs.push(b"-I".to_vec());
                dargs.push(arg);
            }
            x if x == b'l' as i32 => dargs.push(b"--left-column".to_vec()),
            x if x == b'o' as i32 => output = Some(arg),
            x if x == b's' as i32 => suppress = true,
            x if x == b't' as i32 => dargs.push(b"-t".to_vec()),
            x if x == b'w' as i32 => {
                dargs.push(b"-W".to_vec());
                dargs.push(arg);
            }
            x if x == b'W' as i32 => dargs.push(b"-w".to_vec()),
            x if x == b'Z' as i32 => dargs.push(b"-Z".to_vec()),
            x if x == b'v' as i32 => {
                let mut out = Output::stdout();
                out.write_str(VERSION);
                return if out.finish().is_ok() { 0 } else { 2 };
            }
            HELP_ID => {
                let mut out = Output::stdout();
                out.write_str(HELP);
                return if out.finish().is_ok() { 0 } else { 2 };
            }
            STRIP_TRAILING_CR => dargs.push(b"--strip-trailing-cr".to_vec()),
            TABSIZE => {
                dargs.push(b"--tabsize".to_vec());
                dargs.push(arg);
            }
            _ => {}
        }
    }
    if operands.len() < 2 {
        let last = operands.last().map(|o| String::from_utf8_lossy(o).into_owned()).unwrap_or_else(|| argv0.clone());
        return try_help(&format!("missing operand after '{last}'"));
    }
    if operands.len() > 2 {
        return try_help(&format!("extra operand '{}'", String::from_utf8_lossy(&operands[2])));
    }
    if suppress {
        dargs.push(b"--suppress-common-lines".to_vec());
    }
    dargs.push(b"-y".to_vec());
    let Some(output) = output else {
        dargs.push(b"--".to_vec());
        dargs.extend(operands);
        let os = sysabi::ctx::to_os_args(&dargs);
        return diff::main(ctx, &os);
    };
    Merge::run(&argv0, dargs, &operands, &output, suppress)
}

/// Estado do modo interativo.
struct Merge<'a> {
    argv0: &'a str,
    names: [Vec<u8>; 2],
    out: Output,
    file: Fd,
    stdin_buf: Vec<u8>,
    stdin_pos: usize,
    stdin_eof: bool,
    silent: bool,
}

/// Fim antecipado: código de saída (as mensagens já saíram).
struct Abort(i32);

impl<'a> Merge<'a> {
    fn run(argv0: &'a str, mut dargs: Vec<Vec<u8>>, operands: &[Vec<u8>], output: &[u8], suppress: bool) -> i32 {
        // O sdiff confere os arquivos antes de tudo.
        let mut stats = Vec::new();
        for name in operands {
            let st = if name.as_slice() == b"-" { sysutil::fstat(Fd::STDIN) } else { sysutil::stat(name) };
            match st {
                Ok(s) => stats.push(s),
                Err(e) => {
                    sysutil::error_path(argv0, name, e);
                    return 2;
                }
            }
        }
        if stats.iter().all(|s| s.file_type() == FileType::Directory) {
            sysutil::error(argv0, "both files to be compared are directories");
            return 2;
        }
        let file = match sysabi::sys::open(output, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC, 0o666) {
            Ok(fd) => fd,
            Err(e) => {
                sysutil::error_path(argv0, output, e);
                return 2;
            }
        };
        dargs.push(b"--sdiff-merge-assist".to_vec());
        dargs.push(b"--".to_vec());
        dargs.extend(operands.iter().cloned());
        let opts = match options::parse(&dargs) {
            Parsed::Run(o) => *o,
            Parsed::Exit(code) => {
                sysutil::error(argv0, format!("subsidiary program 'diff' failed (exit status {code})"));
                let _ = sysabi::sys::close(file);
                return 2;
            }
        };
        let mut m = Merge {
            argv0,
            names: [operands[0].clone(), operands[1].clone()],
            out: Output::stdout(),
            file,
            stdin_buf: Vec::new(),
            stdin_pos: 0,
            stdin_eof: false,
            silent: suppress,
        };
        let code = match m.compare(&opts, &stats) {
            Ok(c) => c,
            Err(Abort(c)) => c,
        };
        m.out.flush();
        let _ = sysabi::sys::close(m.file);
        code
    }

    fn read_input(&self, k: usize, stdin_used: &mut Option<Vec<u8>>) -> Result<Vec<u8>, Abort> {
        let name = &self.names[k];
        let r = if name.as_slice() == b"-" {
            if stdin_used.is_none() {
                *stdin_used = Some(sysutil::read_fd(Fd::STDIN).unwrap_or_default());
            }
            Ok(stdin_used.clone().unwrap_or_default())
        } else {
            sysutil::read_path(name)
        };
        r.map_err(|e| {
            let mut m = b"diff: ".to_vec();
            m.extend_from_slice(name);
            m.extend_from_slice(b": ");
            m.extend_from_slice(e.message().as_bytes());
            m.push(b'\n');
            sysutil::eprint(m);
            Abort(2)
        })
    }

    fn compare(&mut self, opts: &options::Opts, stats: &[sysabi::Stat]) -> Result<i32, Abort> {
        let mut stdin_used = None;
        let mut a = self.read_input(0, &mut stdin_used)?;
        let mut b = self.read_input(1, &mut stdin_used)?;
        let blk = |k: usize| stats[k].blksize.max(1) as usize;
        if !opts.text && (diff::looks_binary(&a, blk(0)) || diff::looks_binary(&b, blk(1))) {
            if a == b {
                return Ok(0);
            }
            let msg = [b"Binary files ".as_slice(), &self.names[0], b" and ", &self.names[1], b" differ\n"].concat();
            self.out.write(&msg);
            return Ok(1);
        }
        if opts.strip_trailing_cr {
            a = text::strip_trailing_cr(&a);
            b = text::strip_trailing_cr(&b);
        }
        let la = text::split_lines(&a);
        let lb = text::split_lines(&b);
        let script = diff::script_for(opts, &la, &lb);
        let g = Geometry::new(opts.width, opts.tabsize, opts.expand_tabs);
        let differs = script.iter().any(|c| !c.ignore);
        let (mut next0, mut next1) = (0usize, 0usize);
        for ch in &script {
            if ch.ignore {
                continue;
            }
            self.common(&g, opts.left_column, &la, &lb, (next0, ch.line0), (next1, ch.line1))?;
            self.hunk(&g, &la, &lb, ch)?;
            next0 = ch.line0 + ch.deleted;
            next1 = ch.line1 + ch.inserted;
        }
        self.common(&g, opts.left_column, &la, &lb, (next0, la.len()), (next1, lb.len()))?;
        Ok(differs as i32)
    }

    fn write_file(&mut self, data: &[u8]) -> Result<(), Abort> {
        if let Err(e) = sysabi::sys::write_all(self.file, data) {
            self.out.flush();
            sysutil::error(self.argv0, format!("write failed: {}", e.message()));
            return Err(Abort(2));
        }
        Ok(())
    }

    /// Linhas comuns: mostra (a menos do modo silencioso) e copia o lado esquerdo pra saída.
    fn common(
        &mut self,
        g: &Geometry,
        left_column: bool,
        la: &[&[u8]],
        lb: &[&[u8]],
        r0: (usize, usize),
        r1: (usize, usize),
    ) -> Result<(), Abort> {
        let (mut i0, lim0) = r0;
        let (mut i1, lim1) = r1;
        let mut shown = Vec::new();
        if !self.silent {
            let (mut a0, mut a1) = (i0, i1);
            while a0 < lim0 && a1 < lim1 {
                if left_column {
                    g.line(&mut shown, Some(la[a0]), b'(', None);
                } else {
                    g.line(&mut shown, Some(la[a0]), b' ', Some(lb[a1]));
                }
                a0 += 1;
                a1 += 1;
            }
            while a1 < lim1 {
                g.line(&mut shown, None, b')', Some(lb[a1]));
                a1 += 1;
            }
            while a0 < lim0 {
                g.line(&mut shown, Some(la[a0]), b'(', None);
                a0 += 1;
            }
        }
        self.out.write(&shown);
        let mut data = Vec::new();
        while i0 < lim0 {
            data.extend_from_slice(la[i0]);
            i0 += 1;
        }
        i1 = i1.max(lim1);
        let _ = i1;
        self.write_file(&data)
    }

    fn hunk(&mut self, g: &Geometry, la: &[&[u8]], lb: &[&[u8]], ch: &Change) -> Result<(), Abort> {
        let left = &la[ch.line0..ch.line0 + ch.deleted];
        let right = &lb[ch.line1..ch.line1 + ch.inserted];
        let mut shown = Vec::new();
        let (mut i, mut j) = (0usize, 0usize);
        if !left.is_empty() && !right.is_empty() {
            while i < left.len() && j < right.len() {
                g.line(&mut shown, Some(left[i]), b'|', Some(right[j]));
                i += 1;
                j += 1;
            }
        }
        while j < right.len() {
            g.line(&mut shown, None, b'>', Some(right[j]));
            j += 1;
        }
        while i < left.len() {
            g.line(&mut shown, Some(left[i]), b'<', None);
            i += 1;
        }
        self.out.write(&shown);
        loop {
            self.out.write(b"%");
            self.out.flush();
            let Some(line) = self.read_command() else {
                return Err(Abort(2));
            };
            match parse_command(&line) {
                Some(Cmd::Left) => return self.write_file(&left.concat()),
                Some(Cmd::Right) => return self.write_file(&right.concat()),
                Some(Cmd::Silent) => self.silent = true,
                Some(Cmd::Verbose) => self.silent = false,
                Some(Cmd::Quit) => return Err(Abort(2)),
                Some(Cmd::Edit(kind)) => {
                    let content = self.edit_content(kind, ch, left, right);
                    let edited = self.edit(&content)?;
                    return self.write_file(&edited);
                }
                None => {
                    self.out.flush();
                    sysutil::eprint(COMMANDS);
                }
            }
        }
    }

    /// Lê uma linha de comando da entrada padrão (com o `\n`); `None` no fim sem `\n`.
    fn read_command(&mut self) -> Option<Vec<u8>> {
        loop {
            if let Some(p) = self.stdin_buf[self.stdin_pos..].iter().position(|&b| b == b'\n') {
                let line = self.stdin_buf[self.stdin_pos..self.stdin_pos + p + 1].to_vec();
                self.stdin_pos += p + 1;
                return Some(line);
            }
            if self.stdin_eof {
                if self.stdin_pos < self.stdin_buf.len() {
                    // Comando sem `\n` no fim da entrada: inválido, e depois o fim.
                    self.stdin_pos = self.stdin_buf.len();
                    return Some(b"\0".to_vec());
                }
                return None;
            }
            let mut buf = [0u8; 4096];
            match sysabi::sys::read(Fd::STDIN, &mut buf) {
                Ok(0) => self.stdin_eof = true,
                Ok(n) => self.stdin_buf.extend_from_slice(&buf[..n]),
                Err(Errno::EINTR) => {}
                Err(_) => self.stdin_eof = true,
            }
        }
    }

    fn edit_content(&self, kind: EditKind, ch: &Change, left: &[&[u8]], right: &[&[u8]]) -> Vec<u8> {
        let range = |lo: usize, n: usize| -> String {
            if n <= 1 { format!("{}", lo + 1) } else { format!("{},{}", lo + 1, lo + n) }
        };
        let mut v = Vec::new();
        match kind {
            EditKind::Decorated => {
                if !left.is_empty() {
                    v.extend_from_slice(b"--- ");
                    v.extend_from_slice(&self.names[0]);
                    v.extend_from_slice(format!(" {}\n", range(ch.line0, ch.deleted)).as_bytes());
                    v.extend_from_slice(&left.concat());
                }
                if !right.is_empty() {
                    v.extend_from_slice(b"+++ ");
                    v.extend_from_slice(&self.names[1]);
                    v.extend_from_slice(format!(" {}\n", range(ch.line1, ch.inserted)).as_bytes());
                    v.extend_from_slice(&right.concat());
                }
            }
            EditKind::Both => {
                v.extend_from_slice(&left.concat());
                v.extend_from_slice(&right.concat());
            }
            EditKind::Left => v.extend_from_slice(&left.concat()),
            EditKind::Right => v.extend_from_slice(&right.concat()),
            EditKind::Empty => {}
        }
        v
    }

    /// Grava o texto num arquivo temporário, roda o editor nele e devolve o conteúdo editado.
    fn edit(&mut self, content: &[u8]) -> Result<Vec<u8>, Abort> {
        self.out.flush();
        let tmpdir = sysutil::getenv("TMPDIR").filter(|t| !t.is_empty()).unwrap_or_else(|| b"/tmp".to_vec());
        let sys = sysabi::sys::current();
        let mut path = Vec::new();
        let mut fd = None;
        for _ in 0..100 {
            let mut rnd = [0u8; 6];
            let _ = sys.getrandom(&mut rnd);
            let suffix: String = rnd
                .iter()
                .map(|b| {
                    let cs = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
                    cs[*b as usize % cs.len()] as char
                })
                .collect();
            path = sysutil::join(&tmpdir, format!("sdiff{suffix}").as_bytes());
            match sys.openat(Fd::CWD, &path, OFlags::RDWR | OFlags::CREAT | OFlags::EXCL | OFlags::CLOEXEC, 0o600) {
                Ok(f) => {
                    fd = Some(f);
                    break;
                }
                Err(Errno::EEXIST) => continue,
                Err(e) => {
                    sysutil::error_path(self.argv0, &path, e);
                    return Err(Abort(2));
                }
            }
        }
        let Some(fd) = fd else {
            sysutil::error_path(self.argv0, &path, Errno::EEXIST);
            return Err(Abort(2));
        };
        let w = sysabi::sys::write_all(fd, content);
        let _ = sys.close(fd);
        if let Err(e) = w {
            sysutil::error_path(self.argv0, &path, e);
            let _ = sys.unlinkat(Fd::CWD, &path, sysabi::AtFlags::empty());
            return Err(Abort(2));
        }
        let editor = sysutil::getenv("EDITOR").unwrap_or_else(|| b"ed".to_vec());
        let shown = String::from_utf8_lossy(&editor).into_owned();
        let result = match resolve_program(&editor) {
            None => Err(format!("subsidiary program '{shown}' not found")),
            Some(prog) => {
                let spec = SpawnSpec { path: prog, argv: vec![editor.clone(), path.clone()], attrs: ProcAttrs::default() };
                match sys.spawn(spec) {
                    Err(Errno::ENOENT) => Err(format!("subsidiary program '{shown}' not found")),
                    Err(e) => Err(format!("subsidiary program '{shown}' could not be invoked: {}", e.message())),
                    Ok(pid) => match sys.wait4(WaitTarget::Pid(pid), WaitOptions::empty()) {
                        Ok(Some((_, WaitStatus::Exited(0)))) => Ok(()),
                        Ok(Some((_, WaitStatus::Exited(c)))) => {
                            Err(format!("subsidiary program '{shown}' failed (exit status {c})"))
                        }
                        Ok(Some((_, WaitStatus::Signaled { signal, .. }))) => {
                            Err(format!("subsidiary program '{shown}' failed (signal {})", signal.0))
                        }
                        _ => Err(format!("subsidiary program '{shown}' failed")),
                    },
                }
            }
        };
        if let Err(msg) = result {
            sysutil::error(self.argv0, msg);
            let _ = sys.unlinkat(Fd::CWD, &path, sysabi::AtFlags::empty());
            return Err(Abort(2));
        }
        let edited = sysutil::read_path(&path);
        let _ = sys.unlinkat(Fd::CWD, &path, sysabi::AtFlags::empty());
        edited.map_err(|e| {
            sysutil::error_path(self.argv0, &path, e);
            Abort(2)
        })
    }
}

/// Procura um programa no PATH (ou usa o caminho, se tem barra).
fn resolve_program(name: &[u8]) -> Option<Vec<u8>> {
    if name.is_empty() {
        return None;
    }
    if name.contains(&b'/') {
        return sysutil::exists(name).then(|| name.to_vec());
    }
    let path = sysutil::getenv("PATH").unwrap_or_else(|| b"/usr/local/bin:/usr/bin:/bin".to_vec());
    for dir in path.split(|&b| b == b':') {
        let dir: &[u8] = if dir.is_empty() { b"." } else { dir };
        let candidate = sysutil::join(dir, name);
        if sysutil::stat(&candidate).is_ok_and(|s| s.file_type() == FileType::Regular) {
            return Some(candidate);
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditKind {
    Decorated,
    Both,
    Left,
    Right,
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmd {
    Left,
    Right,
    Silent,
    Verbose,
    Quit,
    Edit(EditKind),
}

/// Interpreta uma linha de comando: espaço em volta é aceito; o comando tem que terminar em `\n`.
fn parse_command(line: &[u8]) -> Option<Cmd> {
    let body = line.strip_suffix(b"\n")?;
    let t = body.iter().copied().skip_while(|c| *c == b' ' || *c == b'\t').collect::<Vec<u8>>();
    let mut end = t.len();
    while end > 0 && (t[end - 1] == b' ' || t[end - 1] == b'\t') {
        end -= 1;
    }
    match &t[..end] {
        b"l" | b"1" => Some(Cmd::Left),
        b"r" | b"2" => Some(Cmd::Right),
        b"s" => Some(Cmd::Silent),
        b"v" => Some(Cmd::Verbose),
        b"q" => Some(Cmd::Quit),
        b"e" => Some(Cmd::Edit(EditKind::Empty)),
        b"eb" => Some(Cmd::Edit(EditKind::Both)),
        b"ed" => Some(Cmd::Edit(EditKind::Decorated)),
        b"el" | b"e1" => Some(Cmd::Edit(EditKind::Left)),
        b"er" | b"e2" => Some(Cmd::Edit(EditKind::Right)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands() {
        assert_eq!(parse_command(b" l \n"), Some(Cmd::Left));
        assert_eq!(parse_command(b"l"), None);
        assert_eq!(parse_command(b"e2\n"), Some(Cmd::Edit(EditKind::Right)));
        assert_eq!(parse_command(b"zz\n"), None);
        assert_eq!(parse_command(b"\n"), None);
    }
}
