//! `watch` do procps-ng 4.0.4 (Debian 13, com `--enable-watch8bit --enable-colorwatch`).
//!
//! Opções, mensagens de uso e de erro, `WATCH_INTERVAL`, `-n`, `-g`, `-e`, `-q`, `-t`, `-w`, `-x`,
//! `-p`, `-r` e o laço que roda o comando, preenche a tela (largura e altura de `COLUMNS`/`LINES`) e
//! decide a saída são os do original.
//!
//! O original desenha com o ncurses; num sandbox sem tty o que importa é a abertura do terminal: sem
//! `TERM` (ou com um `TERM` sem entrada no terminfo) o ncurses recusa com
//! `Error opening terminal: unknown.` e o watch sai com 1 antes de rodar o comando, e é isso que sai
//! aqui, byte a byte. Com um terminal conhecido o laço roda de verdade; o desenho segue o que o
//! ncurses emite num terminal sem endereçamento de cursor (`dumb`, medido no oráculo): a tela é
//! limpa com brancos e cada atualização escreve só as células que mudaram, linha a linha, sem
//! movimento de cursor, terminando com `\r` no `endwin`. Terminais com endereçamento (xterm...)
//! recebem esse mesmo desenho simplificado, não as sequências de escape do ncurses.

use std::ffi::OsString;
use std::time::Duration;

use sysabi::{Ctx, Errno, Fd, OFlags, sys};
use sysio::fd::{FromRawFd, OwnedFd};
use sysio::process::{Command, Stdio};
use ul_misc::util::getopt::{Getopt, HasArg, LongOpt};
use ul_misc::util::io;

use crate::common::out;
use crate::ps::util::{decode_utf8, wcwidth};

const USAGE: &str = "\nUsage:\n watch [options] command\n\nOptions:\n  -b, --beep             beep if command has a non-zero exit\n  -c, --color            interpret ANSI color and style sequences\n  -C, --no-color         do not interpret ANSI color and style sequences\n  -d, --differences[=<permanent>]\n                         highlight changes between updates\n  -e, --errexit          exit if command has a non-zero exit\n  -g, --chgexit          exit when output from command changes\n  -q, --equexit <cycles>\n                         exit when output from command does not change\n  -n, --interval <secs>  seconds to wait between updates\n  -p, --precise          attempt run command in precise intervals\n  -r, --no-rerun         do not rerun program on window resize\n  -t, --no-title         turn off header\n  -w, --no-wrap          turn off line wrapping\n  -x, --exec             pass command to exec instead of \"sh -c\"\n\n -h, --help     display this help and exit\n -v, --version  output version information and exit\n\nFor more details see watch(1).\n";

const WATCH_DIFF: u32 = 1 << 1;
const WATCH_CUMUL: u32 = 1 << 2;
const WATCH_EXEC: u32 = 1 << 3;
const WATCH_BEEP: u32 = 1 << 4;
const WATCH_COLOR: u32 = 1 << 5;
const WATCH_ERREXIT: u32 = 1 << 6;
const WATCH_CHGEXIT: u32 = 1 << 7;
const WATCH_EQUEXIT: u32 = 1 << 8;
const WATCH_NORERUN: u32 = 1 << 9;

const LONGS: &[LongOpt] = &[
    LongOpt::new("color", HasArg::No, 'c' as i32),
    LongOpt::new("no-color", HasArg::No, 'C' as i32),
    LongOpt::new("differences", HasArg::Optional, 'd' as i32),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("interval", HasArg::Required, 'n' as i32),
    LongOpt::new("beep", HasArg::No, 'b' as i32),
    LongOpt::new("errexit", HasArg::No, 'e' as i32),
    LongOpt::new("chgexit", HasArg::No, 'g' as i32),
    LongOpt::new("equexit", HasArg::Required, 'q' as i32),
    LongOpt::new("exec", HasArg::No, 'x' as i32),
    LongOpt::new("precise", HasArg::No, 'p' as i32),
    LongOpt::new("no-rerun", HasArg::No, 'r' as i32),
    LongOpt::new("no-title", HasArg::No, 't' as i32),
    LongOpt::new("no-wrap", HasArg::No, 'w' as i32),
    LongOpt::new("version", HasArg::No, 'v' as i32),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// `strtod(3)` sobre o texto inteiro: `Some(valor)` se consumiu tudo, `Err(consumiu_algo)` senão.
fn strtod_full(s: &[u8]) -> Result<f64, bool> {
    let text = String::from_utf8_lossy(s).into_owned();
    let t = text.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let lower = t.to_ascii_lowercase();
    let (sign, body) = match lower.strip_prefix('-') {
        Some(r) => (-1.0, r),
        None => (1.0, lower.strip_prefix('+').unwrap_or(&lower)),
    };
    let named = |name: &str, v: f64| -> Option<(f64, usize)> { body.starts_with(name).then_some((v, name.len())) };
    let special = named("infinity", f64::INFINITY).or_else(|| named("inf", f64::INFINITY)).or_else(|| named("nan", f64::NAN));
    if let Some((v, n)) = special {
        let consumed_all = body.len() == n;
        return if consumed_all { Ok(sign * v) } else { Err(true) };
    }
    // Número decimal: dígitos, ponto, dígitos, expoente.
    let b = body.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i;
    let mut frac_digits = 0;
    if i < b.len() && b[i] == b'.' {
        let mut j = i + 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        frac_digits = j - i - 1;
        i = j;
    }
    if int_digits + frac_digits == 0 {
        return Err(false);
    }
    if i < b.len() && b[i] == b'e' {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let ds = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > ds {
            i = j;
        }
    }
    if i != b.len() {
        return Err(true);
    }
    match body[..i].parse::<f64>() {
        Ok(v) => Ok(sign * v),
        Err(_) => Err(true),
    }
}

/// `strtod_nol_or_err`: número inteiro do texto ou a mensagem de erro e a saída 1.
fn strtod_or_err(s: &[u8], what: &str) -> Result<f64, i32> {
    if s.is_empty() {
        io::eprint(format!("watch: {what}: ''\n"));
        return Err(1);
    }
    match strtod_full(s) {
        Ok(v) => Ok(v),
        Err(consumed) => {
            let shown = String::from_utf8_lossy(s);
            if consumed {
                io::eprint(format!("watch: {what}: '{shown}'\n"));
            } else {
                io::eprint(format!("watch: {what}: '{shown}': {}\n", Errno::EINVAL.message()));
            }
            Err(1)
        }
    }
}

/// O `TERM` tem entrada no terminfo? (`$TERMINFO`, `~/.terminfo`, `$TERMINFO_DIRS` e os diretórios
/// do sistema.)
pub(crate) fn terminfo_exists(term: &[u8]) -> bool {
    if term.is_empty() || term.contains(&b'/') {
        return false;
    }
    let mut dirs: Vec<Vec<u8>> = Vec::new();
    if let Some(d) = sys::getenv("TERMINFO").filter(|d| !d.is_empty()) {
        dirs.push(d);
    }
    if let Some(h) = sys::getenv("HOME").filter(|h| !h.is_empty()) {
        let mut p = h;
        p.extend_from_slice(b"/.terminfo");
        dirs.push(p);
    }
    if let Some(list) = sys::getenv("TERMINFO_DIRS") {
        for d in list.split(|b| *b == b':') {
            dirs.push(if d.is_empty() { b"/usr/share/terminfo".to_vec() } else { d.to_vec() });
        }
    }
    for d in ["/etc/terminfo", "/lib/terminfo", "/usr/share/terminfo"] {
        dirs.push(d.as_bytes().to_vec());
    }
    for mut d in dirs {
        d.push(b'/');
        d.push(term[0]);
        d.push(b'/');
        d.extend_from_slice(term);
        if sys::stat(&d).is_ok() {
            return true;
        }
        // Layout em hexadecimal (`/usr/share/terminfo/78/xterm`).
        let mut h = d[..d.len() - term.len() - 2].to_vec();
        h.extend_from_slice(format!("{:x}/", term[0]).as_bytes());
        h.extend_from_slice(term);
        if sys::stat(&h).is_ok() {
            return true;
        }
    }
    false
}

/// `strtol(s, &end, 0)` para `COLUMNS`/`LINES`: valor e se consumiu tudo.
fn env_long(s: &[u8]) -> (i64, bool) {
    let mut i = 0;
    while i < s.len() && s[i].is_ascii_whitespace() {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
        neg = s[i] == b'-';
        i += 1;
    }
    let (v, used) = crate::ps::util::strtoul0(&s[i..]);
    if used == 0 {
        return (0, false);
    }
    let v = i64::try_from(v).unwrap_or(i64::MAX);
    (if neg { -v } else { v }, i + used == s.len())
}

/// Célula da tela.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Cell {
    ch: char,
    /// Destaque do `-d` (o `standout`).
    attr: bool,
    /// Segunda coluna de um caractere largo.
    cont: bool,
}

const BLANK: Cell = Cell { ch: ' ', attr: false, cont: false };

struct Watch {
    flags: u32,
    interval: f64,
    max_cycles: i32,
    show_title: i32,
    line_wrap: bool,
    precise: bool,
    width: i64,
    height: i64,
    first_screen: bool,
    command: Vec<u8>,
    command_argv: Vec<Vec<u8>>,
    /// O que está na tela (o `stdscr`).
    screen: Vec<Vec<Cell>>,
    /// O que o terminal já mostra (o `curscr`); `None` antes da primeira atualização.
    shown: Option<Vec<Vec<Cell>>>,
    /// Posição e conteúdo lidos do pipe do comando.
    pipe_fd: Option<Fd>,
    buf: Vec<u8>,
    pos: usize,
    eof: bool,
}

impl Watch {
    fn blank_screen(&self) -> Vec<Vec<Cell>> {
        let w = self.width.max(0) as usize;
        let h = self.height.max(0) as usize;
        vec![vec![BLANK; w]; h]
    }

    fn put(&mut self, y: i64, x: i64, c: Cell) {
        if y >= 0 && x >= 0 && (y as usize) < self.screen.len() && (x as usize) < self.screen.first().map_or(0, Vec::len) {
            self.screen[y as usize][x as usize] = c;
        }
    }

    fn get(&self, y: i64, x: i64) -> Cell {
        if y >= 0 && x >= 0 && (y as usize) < self.screen.len() && (x as usize) < self.screen.first().map_or(0, Vec::len) {
            self.screen[y as usize][x as usize]
        } else {
            BLANK
        }
    }

    fn put_str(&mut self, y: i64, x: i64, s: &str, limit: Option<usize>) {
        for (x, (n, ch)) in (x..).zip(s.chars().enumerate()) {
            if limit.is_some_and(|l| n >= l) {
                break;
            }
            self.put(y, x, Cell { ch, attr: false, cont: false });
        }
    }

    /// `refresh()`: escreve no stdout o que mudou desde a última vez.
    fn refresh(&mut self) {
        let mut o: Vec<u8> = Vec::new();
        let w = self.screen.first().map_or(0, Vec::len);
        let h = self.screen.len();
        let prev = match self.shown.take() {
            Some(p) => p,
            None => {
                // Sem `clear_screen` no terminal, o ncurses limpa escrevendo brancos.
                o.extend(std::iter::repeat_n(b' ', w * h));
                vec![vec![BLANK; w]; h]
            }
        };
        'rows: for (row, prow) in self.screen.iter().zip(&prev) {
            let mut first: Option<usize> = None;
            let mut last = 0usize;
            for (x, (c, pc)) in row.iter().zip(prow).enumerate() {
                if c != pc {
                    if first.is_none() {
                        first = Some(x);
                    }
                    last = x;
                }
            }
            if let Some(f) = first {
                for c in &row[f..=last] {
                    if !c.cont {
                        let mut b = [0u8; 4];
                        o.extend_from_slice(c.ch.encode_utf8(&mut b).as_bytes());
                    }
                }
                // Linha escrita até a última coluna: o ncurses perde a posição do cursor e o
                // resto da atualização não sai.
                if last + 1 == w {
                    break 'rows;
                }
            }
        }
        self.shown = Some(self.screen.clone());
        out(&o);
        let _ = io::flush_stdout();
    }

    /// `endwin()`: leva o cursor ao começo da linha.
    fn endwin(&mut self) {
        out(b"\r");
        let _ = io::flush_stdout();
    }

    /// Próximo byte do pipe, lendo do fd quando precisa.
    fn getc(&mut self) -> Option<u8> {
        if self.pos >= self.buf.len() {
            if self.eof {
                return None;
            }
            let fd = self.pipe_fd?;
            let mut chunk = [0u8; 4096];
            match sys::read(fd, &mut chunk) {
                Ok(0) | Err(_) => {
                    self.eof = true;
                    return None;
                }
                Ok(n) => {
                    self.buf = chunk[..n].to_vec();
                    self.pos = 0;
                }
            }
        }
        let b = self.buf[self.pos];
        self.pos += 1;
        Some(b)
    }

    /// `my_getwc`: um caractere UTF-8 do pipe; `None` no fim. Byte inválido é pulado.
    fn getwc(&mut self) -> Option<char> {
        loop {
            let b0 = self.getc()?;
            if b0 < 0x80 {
                return Some(char::from(b0));
            }
            let mut bytes = vec![b0];
            loop {
                match decode_utf8(&bytes) {
                    Some((cp, _)) => return char::from_u32(cp),
                    None => {
                        if bytes.len() >= 4 {
                            break;
                        }
                        match self.getc() {
                            Some(b) if b & 0xc0 == 0x80 => bytes.push(b),
                            Some(b) => {
                                // Reinsere o byte que não é continuação.
                                self.pos -= 1;
                                let _ = b;
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }
        }
    }

    fn find_eol(&mut self) {
        while let Some(c) = self.getwc() {
            if c == '\n' {
                break;
            }
        }
    }

    /// `output_header`.
    fn output_header(&mut self) {
        let t = ul_misc::util::time::now().sec;
        let tz = ul_misc::util::time::local_tz();
        let ts = format!("{}\n", ul_misc::util::time::ctime(t, &tz));
        let host = String::from_utf8_lossy(&sys::current().uname().nodename).into_owned();
        let header = format!("Every {:.1}s: ", self.interval);
        let right = format!("{host}: {ts}");
        let hlen = header.chars().count() as i64;
        let rhlen = right.chars().count() as i64;
        let width = self.width;
        if width < rhlen {
            return;
        }
        if rhlen + hlen < width {
            self.put_str(0, 0, &header, None);
            if rhlen + hlen + 2 <= width {
                if width < rhlen + hlen + 4 {
                    self.put_str(0, width - rhlen - 4, "... ", None);
                } else {
                    let cmd = String::from_utf8_lossy(&self.command).into_owned();
                    let command_columns: i64 = cmd.chars().map(|c| i64::from(wcwidth(c as u32).max(0))).sum();
                    if width < rhlen + hlen + command_columns {
                        // Corta o comando para caber com as reticências.
                        let available = width - rhlen - hlen;
                        let chars: Vec<char> = cmd.chars().collect();
                        let mut n = chars.len();
                        let cols = |n: usize| -> i64 { chars[..n].iter().map(|c| i64::from(wcwidth(*c as u32).max(0))).sum() };
                        while n > 0 && available - 4 < cols(n) {
                            n -= 1;
                        }
                        let s: String = chars[..n].iter().collect();
                        self.put_str(0, hlen, &s, None);
                        self.put_str(0, width - rhlen - 4, "... ", None);
                    } else {
                        self.put_str(0, hlen, &cmd, None);
                    }
                }
            }
        }
        // O '\n' final do ctime só limpa o resto da linha, que já está em branco.
        let shown: String = right.trim_end_matches('\n').to_string();
        self.put_str(0, width - rhlen + 1, &shown, None);
    }

    /// `run_command`. `Ok(exit_early)`, ou `Err(código)` para sair do programa na hora.
    fn run_command(&mut self) -> Result<bool, i32> {
        let sysc = sys::current();
        let (rfd, wfd) = match sysc.pipe2(OFlags::CLOEXEC) {
            Ok(p) => p,
            Err(_) => {
                io::eprint("watch: unable to create IPC pipes\n");
                return Err(7);
            }
        };
        let _ = io::flush_stdout();
        let w_owned = OwnedFd::from_raw_fd(wfd.0);
        let w2 = w_owned.try_clone().ok();
        let mut cmd;
        if self.flags & WATCH_EXEC != 0 {
            cmd = Command::new(OsString::from(String::from_utf8_lossy(&self.command_argv[0]).into_owned()));
            for a in &self.command_argv[1..] {
                cmd.arg(OsString::from(String::from_utf8_lossy(a).into_owned()));
            }
        } else {
            cmd = Command::new("sh");
            cmd.arg("-c").arg(OsString::from(String::from_utf8_lossy(&self.command).into_owned()));
        }
        cmd.stdin(Stdio::inherit()).stdout(Stdio::from(w_owned));
        match w2 {
            Some(w) => {
                cmd.stderr(Stdio::from(w));
            }
            None => {
                cmd.stderr(Stdio::inherit());
            }
        }
        let spawned = cmd.spawn();
        drop(cmd);
        let mut child = None;
        let mut synthetic: Option<Vec<u8>> = None;
        match spawned {
            Ok(c) => child = Some(c),
            Err(e) => {
                let name = String::from_utf8_lossy(self.command_argv.first().map_or(&b""[..], |v| v.as_slice())).into_owned();
                synthetic = Some(format!("watch: unable to execute '{name}': {}\n", sysio::errno::strerror(&e)).into_bytes());
            }
        }
        self.pipe_fd = Some(rfd);
        self.buf = synthetic.take().unwrap_or_default();
        self.pos = 0;
        self.eof = child.is_none();
        let mut exit_early = false;
        let mut buffer_size = 0u64;
        let mut unchanged_buffer = 0u64;
        let mut oldeolseen = true;
        let width = self.width;
        let height = self.height;
        let color = self.flags & WATCH_COLOR != 0;
        let mut y = i64::from(self.show_title);
        while y < height {
            let mut eolseen = false;
            let mut tabpending = false;
            let mut carry: Option<char> = None;
            let mut x: i64 = 0;
            while x < width {
                let mut c: Option<char> = Some(' ');
                if !eolseen {
                    if !tabpending {
                        loop {
                            let n = match carry.take() {
                                Some(k) => Some(k),
                                None => self.getwc(),
                            };
                            c = n;
                            match n {
                                None => break,
                                Some(ch) => {
                                    let cp = ch as u32;
                                    let skip = !is_print(ch)
                                        && cp < 128
                                        && wcwidth(cp) == 0
                                        && ch != '\u{7}'
                                        && ch != '\n'
                                        && ch != '\t'
                                        && (ch != '\u{1b}' || !color);
                                    if !skip {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    if c == Some('\n') {
                        if !oldeolseen && x == 0 {
                            // Linha em branco logo depois de uma linha que encheu a largura.
                            continue;
                        }
                        eolseen = true;
                    } else if c == Some('\t') {
                        tabpending = true;
                    } else if c == Some('\u{7}') {
                        // beep(); o `continue` do for do original ainda avança a coluna.
                        x += 1;
                        continue;
                    }
                    if let Some(ch) = c
                        && x == width - 1 && wcwidth(ch as u32) == 2 {
                            y += 1;
                            x = 0;
                            carry = Some(ch);
                            continue;
                        }
                    if c.is_none() || c == Some('\n') || c == Some('\t') {
                        c = Some(' ');
                    }
                    if tabpending && (x + 1) % 8 == 0 {
                        tabpending = false;
                    }
                }
                let ch = c.unwrap_or(' ');
                let ch = if (ch as u32) < 32 || ch as u32 == 127 { '^' } else { ch };
                let old = self.get(y, x);
                if !self.first_screen && !exit_early && self.flags & WATCH_CHGEXIT != 0 {
                    exit_early = ch != old.ch;
                }
                if !self.first_screen && !exit_early && self.flags & WATCH_EQUEXIT != 0 {
                    buffer_size += 1;
                    if ch == old.ch {
                        unchanged_buffer += 1;
                    }
                }
                let mut attr = false;
                if self.flags & WATCH_DIFF != 0 {
                    attr = !self.first_screen && (ch != old.ch || (self.flags & WATCH_CUMUL != 0 && old.attr));
                }
                self.put(y, x, Cell { ch, attr, cont: false });
                if wcwidth(ch as u32) == 2 {
                    x += 1;
                    self.put(y, x, Cell { ch: ' ', attr, cont: true });
                } else if wcwidth(ch as u32) == 0 {
                    x -= 1;
                }
                x += 1;
            }
            oldeolseen = eolseen;
            if !self.line_wrap && !eolseen {
                self.find_eol();
            }
            y += 1;
        }
        // fclose(p): o filho que ainda escreve recebe SIGPIPE.
        if let Some(fd) = self.pipe_fd.take() {
            let _ = sys::close(fd);
        }
        let status_failed = match child {
            Some(mut c) => match c.wait() {
                Ok(st) => !st.success(),
                Err(_) => {
                    io::eprint("watch: waitpid\n");
                    return Err(8);
                }
            },
            None => true,
        };
        if status_failed {
            if self.flags & WATCH_BEEP != 0 {
                // beep(): sem campainha no terminal, nada a escrever.
            }
            if self.flags & WATCH_ERREXIT != 0 {
                let msg = "command exit with a non-zero status, press a key to exit";
                self.put_str(height - 1, 0, msg, None);
                self.refresh();
                // fgetc(stdin)
                let mut b = [0u8; 1];
                let _ = sys::read(Fd::STDIN, &mut b);
                self.endwin();
                return Err(8);
            }
        }
        if self.flags & WATCH_EQUEXIT != 0 && unchanged_buffer == buffer_size {
            exit_early = true;
        }
        self.first_screen = false;
        self.refresh();
        Ok(exit_early)
    }
}

/// `iswprint` aproximado.
fn is_print(c: char) -> bool {
    let cp = c as u32;
    if cp < 0x20 || cp == 0x7f {
        return false;
    }
    !(0x80..0xa0).contains(&cp)
}

fn usage(to_stdout: bool) -> i32 {
    if to_stdout {
        out(USAGE);
        0
    } else {
        io::eprint(USAGE);
        1
    }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut w = Watch {
        flags: WATCH_COLOR,
        interval: 2.0,
        max_cycles: 1,
        show_title: 2,
        line_wrap: true,
        precise: false,
        width: 80,
        height: 24,
        first_screen: true,
        command: Vec::new(),
        command_argv: Vec::new(),
        screen: Vec::new(),
        shown: None,
        pipe_fd: None,
        buf: Vec::new(),
        pos: 0,
        eof: false,
    };
    if let Some(s) = sys::getenv("WATCH_INTERVAL") {
        match strtod_or_err(&s, "Could not parse interval from WATCH_INTERVAL") {
            Ok(v) => w.interval = v,
            Err(c) => return c,
        }
    }
    let mut g = Getopt::from_env(&argv[1..], "+bCced::ghq:n:prtwvx", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                return usage(false);
            }
        };
        match o.short() {
            Some('b') => w.flags |= WATCH_BEEP,
            Some('c') => w.flags |= WATCH_COLOR,
            Some('C') => w.flags &= !WATCH_COLOR,
            Some('d') => {
                w.flags |= WATCH_DIFF;
                if o.arg.is_some() {
                    w.flags |= WATCH_CUMUL;
                }
            }
            Some('e') => w.flags |= WATCH_ERREXIT,
            Some('g') => w.flags |= WATCH_CHGEXIT,
            Some('q') => {
                w.flags |= WATCH_EQUEXIT;
                match strtod_or_err(o.arg.as_deref().unwrap_or(b""), "failed to parse argument") {
                    Ok(v) => w.max_cycles = v as i32,
                    Err(c) => return c,
                }
            }
            Some('r') => w.flags |= WATCH_NORERUN,
            Some('t') => w.show_title = 0,
            Some('w') => w.line_wrap = false,
            Some('x') => w.flags |= WATCH_EXEC,
            Some('n') => match strtod_or_err(o.arg.as_deref().unwrap_or(b""), "failed to parse argument") {
                Ok(v) => w.interval = v,
                Err(c) => return c,
            },
            Some('p') => w.precise = true,
            Some('h') => return usage(true),
            Some('v') => {
                out("watch from procps-ng 4.0.4\n");
                return 0;
            }
            _ => return usage(false),
        }
    }
    if w.interval < 0.1 {
        w.interval = 0.1;
    }
    if w.interval > f64::from(u32::MAX) {
        w.interval = f64::from(u32::MAX);
    }
    let operands = g.operands();
    if operands.is_empty() {
        return usage(false);
    }
    w.command_argv = operands.clone();
    w.command = operands.join(&b' ');
    // get_terminal_size: COLUMNS e LINES do ambiente, depois o ioctl no stderr.
    let mut incoming_cols: i64 = -1;
    let mut incoming_rows: i64 = -1;
    let sysc = sys::current();
    if let Some(s) = sys::getenv("COLUMNS").filter(|s| !s.is_empty()) {
        let (t, whole) = env_long(&s);
        if whole && t > 0 {
            incoming_cols = t;
        }
        // O original atribui mesmo quando o valor é inválido (-1).
        w.width = incoming_cols;
        let _ = sysc.setenv(b"COLUMNS", format!("{}", w.width).as_bytes());
    }
    if let Some(s) = sys::getenv("LINES").filter(|s| !s.is_empty()) {
        let (t, whole) = env_long(&s);
        if whole && t > 0 {
            incoming_rows = t;
        }
        w.height = incoming_rows;
        let _ = sysc.setenv(b"LINES", format!("{}", w.height).as_bytes());
    }
    if let Ok(ws) = sysc.tcgetwinsize(Fd::STDERR)
        && (incoming_cols < 0 || incoming_rows < 0) {
            if incoming_rows < 0 && ws.rows > 0 {
                w.height = i64::from(ws.rows);
                let _ = sysc.setenv(b"LINES", format!("{}", w.height).as_bytes());
            }
            if incoming_cols < 0 && ws.cols > 0 {
                w.width = i64::from(ws.cols);
                let _ = sysc.setenv(b"COLUMNS", format!("{}", w.width).as_bytes());
            }
        }
    // initscr(): abre o terminal pelo terminfo.
    let term = sys::getenv("TERM").filter(|t| !t.is_empty());
    let found = term.as_deref().is_some_and(terminfo_exists);
    if !found {
        let mut m = b"Error opening terminal: ".to_vec();
        m.extend_from_slice(term.as_deref().unwrap_or(b"unknown"));
        m.extend_from_slice(b".\n");
        io::eprint(m);
        return 1;
    }
    // Sem cores num terminal que o ncurses trata como sem `colors` (o `dumb`).
    if term.as_deref() == Some(b"dumb") {
        w.flags &= !WATCH_COLOR;
    }
    w.screen = w.blank_screen();
    let mut cycle_count = 0;
    let mut last_run: u64 = 0;
    let mut next_loop: u64 = 0;
    let now_usec = || -> u64 {
        let t = ul_misc::util::time::now();
        (t.sec as u64) * 1_000_000 + u64::from(t.nsec) / 1000
    };
    if w.precise {
        next_loop = now_usec();
    }
    loop {
        if w.show_title != 0 {
            w.output_header();
        }
        if w.flags & WATCH_NORERUN == 0 || now_usec().wrapping_sub(last_run) as f64 > w.interval * 1_000_000.0 {
            last_run = now_usec();
            let exit = match w.run_command() {
                Ok(e) => e,
                Err(code) => return code,
            };
            if w.flags & WATCH_EQUEXIT != 0 {
                if cycle_count == w.max_cycles && exit {
                    break;
                } else if exit {
                    cycle_count += 1;
                } else {
                    cycle_count = 0;
                }
            } else if exit {
                break;
            }
        } else {
            w.refresh();
        }
        let micros = if w.precise {
            let cur = now_usec();
            next_loop += (1_000_000.0 * w.interval) as u64;
            next_loop.saturating_sub(cur)
        } else {
            (w.interval * 1_000_000.0) as u64
        };
        if sysc.nanosleep(Duration::from_micros(micros)).is_err() {
            sys::checkpoint();
        }
    }
    w.endwin();
    0
}
