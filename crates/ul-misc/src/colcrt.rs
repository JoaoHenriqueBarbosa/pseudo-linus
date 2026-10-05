//! `colcrt` do util-linux 2.41 (pacote bsdextrautils do Debian 13): filtra a saída do nroff pra
//! visualização em terminal, juntando as sequências de meia linha (`ESC 7`, `ESC 8`) e passando o
//! sublinhado pra uma segunda linha de hifens (`-` suprime, `-2` imprime todas as meias linhas).
//!
//! Porte direto do `text-utils/colcrt.c`: cada linha vira um vetor de 132 colunas (o que passa
//! disso é descartado), com os mesmos buracos que o original deixa (uma posição nunca escrita acaba
//! a linha na hora de imprimir, porque o `fputws` para no primeiro NUL).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, Fd, Whence, sys};

use crate::util::io::{self, File};
use crate::util::ul::{self, WideReader, is_wspace};
use crate::util::{Getopt, HasArg, LongOpt};

const OUTPUT_COLS: usize = 132;
const NO_UL_OPTION: i32 = 256;

const LONGS: &[LongOpt] = &[
    LongOpt::new("no-underlining", HasArg::No, NO_UL_OPTION),
    LongOpt::new("half-lines", HasArg::No, b'2' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 colcrt [options] [<file>...]

Filter nroff output for CRT previewing.

Options:
 -,  --no-underlining    suppress all underlining
 -2, --half-lines        print all half-lines

 -h, --help              display this help
 -V, --version           display version

For more details see colcrt(1).
";

struct Ctl<W: Write> {
    line: [char; OUTPUT_COLS + 1],
    line_under: [char; OUTPUT_COLS + 1],
    print_nl: bool,
    need_line_under: bool,
    no_underlining: bool,
    half_lines: bool,
    out: W,
}

/// Uma entrada lida inteira, decodificada como o `fgetwc` da glibc.
struct Src {
    r: WideReader,
    /// `ftell`/`fseek` funcionam (arquivo comum); em pipe não.
    seekable: bool,
}

impl Src {
    /// `fgetwc_or_err`: `Ok(None)` no fim, `Err` com o errno que o `err()` imprimiria.
    fn getwc(&mut self) -> Result<Option<char>, Errno> {
        self.r.getwc()
    }

    fn at_eof(&self) -> bool {
        self.r.at_eof()
    }
}

/// `iswprint` em C.UTF-8: tudo menos controles e separadores de linha e parágrafo.
fn is_print(c: char) -> bool {
    !c.is_control() && c != '\u{2028}' && c != '\u{2029}'
}

/// `wcslen`: o comprimento até o primeiro NUL.
fn wcslen(s: &[char]) -> usize {
    s.iter().position(|&c| c == '\0').unwrap_or(s.len())
}

/// `trim_trailing_spaces`.
fn trim_trailing_spaces(s: &mut [char]) {
    let size = wcslen(s);
    if size == 0 {
        return;
    }
    let mut end = size as isize - 1;
    while end >= 0 && is_wspace(s[end as usize]) {
        end -= 1;
    }
    s[(end + 1) as usize] = '\0';
}

impl<W: Write> Ctl<W> {
    fn put_chars(&mut self, which_under: bool) {
        let mut buf = String::new();
        let src = if which_under { &self.line_under } else { &self.line };
        for &c in src.iter().take_while(|&&c| c != '\0') {
            buf.push(c);
        }
        let _ = self.out.write_all(buf.as_bytes());
    }

    fn output_lines(&mut self, col: usize) {
        // first line
        trim_trailing_spaces(&mut self.line);
        self.put_chars(false);

        if self.print_nl {
            let _ = self.out.write_all(b"\n");
        }
        if !self.half_lines && !self.no_underlining {
            self.print_nl = false;
        }

        for c in self.line.iter_mut().take(OUTPUT_COLS) {
            *c = '\0';
        }

        // second line
        if self.need_line_under {
            self.need_line_under = false;
            self.line_under[col] = '\0';
            trim_trailing_spaces(&mut self.line_under);
            self.put_chars(true);
            let _ = self.out.write_all(b"\n");
            for c in self.line_under.iter_mut().take(OUTPUT_COLS) {
                *c = ' ';
            }
        } else if self.half_lines && col > 0 {
            let _ = self.out.write_all(b"\n");
        }
    }

    fn rubchars(&mut self, mut col: isize, mut n: i32) -> isize {
        while n > 0 && col > 0 {
            self.line[col as usize] = '\0';
            self.line_under[col as usize] = ' ';
            n -= 1;
            col -= 1;
        }
        col
    }

    /// `colcrt`: `Err` quando a leitura falha (o chamador imprime a mensagem e sai com 1).
    fn colcrt(&mut self, src: &mut Src) -> Result<(), Errno> {
        self.print_nl = true;
        if self.half_lines {
            let _ = self.out.write_all(b"\n");
        }

        let mut col: isize = 0;
        loop {
            if col > OUTPUT_COLS as isize - 1 {
                self.output_lines(col as usize);
                // Descarta o resto da linha. Em pipe o ftell falha e o fseek também: a leitura
                // para no primeiro caractere que não seja a quebra de linha.
                loop {
                    let c = src.getwc()?;
                    if c == Some('\n') {
                        break;
                    }
                    if src.at_eof() {
                        return Ok(());
                    }
                    if !src.seekable {
                        return Ok(());
                    }
                }
                col = 0;
                continue;
            }
            let c = src.getwc()?;
            match c {
                Some('\u{1b}') => {
                    let c2 = src.getwc()?;
                    if c2 == Some('8') {
                        col = self.rubchars(col, 1);
                    } else if c2 == Some('7') {
                        col = self.rubchars(col, 2);
                    }
                }
                None => {
                    self.print_nl = false;
                    self.output_lines(col as usize);
                    return Ok(());
                }
                Some('\n') => {
                    self.output_lines(col as usize);
                    col = -1;
                }
                Some('\t') => {
                    while col % 8 != 0 && col < OUTPUT_COLS as isize {
                        self.line[col as usize] = ' ';
                        col += 1;
                    }
                    col -= 1;
                }
                Some('_') => {
                    self.line[col as usize] = ' ';
                    if !self.no_underlining {
                        self.need_line_under = true;
                        self.line_under[col as usize] = '-';
                    }
                }
                Some(ch) => {
                    if !is_print(ch) {
                        col -= 1;
                    } else {
                        self.print_nl = true;
                        self.line[col as usize] = ch;
                    }
                }
            }
            col += 1;
        }
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn is_seekable(fd: Fd) -> bool {
    sys::current().lseek(fd, 0, Whence::Cur).is_ok()
}

fn run(args: &[OsString]) -> i32 {
    let mut argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    // Take care of lonely hyphen option: um "-" sozinho em qualquer posição liga -.
    let mut no_underlining = false;
    let mut i = 0;
    while i < argv.len() {
        if argv[i] == b"-" {
            no_underlining = true;
            argv.remove(i);
        } else {
            i += 1;
        }
    }

    let mut half_lines = false;
    let mut g = Getopt::from_env(&argv[1..], "2Vh", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        match o.id {
            NO_UL_OPTION => no_underlining = true,
            id if id == i32::from(b'2') => half_lines = true,
            id if id == i32::from(b'V') => {
                ul::print_version(&short);
                return 0;
            }
            id if id == i32::from(b'h') => {
                let mut out = io::stdout();
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }
    let files = g.operands();

    let mut ctl = Ctl {
        line: ['\0'; OUTPUT_COLS + 1],
        line_under: ['\0'; OUTPUT_COLS + 1],
        print_nl: false,
        need_line_under: false,
        no_underlining,
        half_lines,
        out: io::stdout(),
    };

    let mut idx = 0;
    loop {
        for c in ctl.line.iter_mut().take(OUTPUT_COLS) {
            *c = '\0';
        }
        for c in ctl.line_under.iter_mut().take(OUTPUT_COLS) {
            *c = ' ';
        }

        let mut src = if idx < files.len() {
            let path = &files[idx];
            idx += 1;
            match File::open(path) {
                Ok(mut f) => {
                    let seekable = is_seekable(f.fd());
                    Src { r: WideReader::new(f.read_to_end_sys()), seekable }
                }
                Err(e) => {
                    let _ = ctl.out.flush();
                    ul::warn(&short, format!("cannot open {}", io::lossy(path)), e);
                    return 1;
                }
            }
        } else {
            let seekable = is_seekable(Fd::STDIN);
            Src { r: WideReader::new(io::read_stdin()), seekable }
        };

        if let Err(e) = ctl.colcrt(&mut src) {
            ul::warn(&short, "fgetwc() failed", e);
            return 1;
        }
        sys::checkpoint();
        if idx >= files.len() {
            break;
        }
    }
    if ctl.out.flush().is_err() {
        return 1;
    }
    0
}
