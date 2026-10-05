//! `ul` do util-linux 2.41 (pacote bsdextrautils do Debian 13): traduz sublinhado e negrito do nroff
//! (backspaces e `_`) pras capacidades do terminal.
//!
//! Porte direto do `text-utils/ul.c`. As capacidades vêm da tabela [`terms`], extraída do banco
//! terminfo do Debian 13: `-t`/`-T` ou `TERM` com um terminal conhecido usam as sequências dele,
//! qualquer outro cai em `dumb` (sem sublinhado nem negrito na saída), com os mesmos avisos do
//! original (`terminal `x' is not known, defaulting to `dumb'` só com `-t`; `trouble reading
//! terminfo` quando `TERM` nem existe).
//!
//! SIGINT e SIGTERM encerram com código 0, como o `_exit(EXIT_SUCCESS)` do manipulador original;
//! a verificação acontece a cada linha emitida.

mod terms;

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, SigDisposition, Signal, sys};

use crate::util::io::{self, File};
use crate::util::ul::{self, WideReader, wcwidth};
use crate::util::{Getopt, HasArg, LongOpt};

const SO: char = '\u{e}';
const SI: char = '\u{f}';
const ESC: char = '\u{1b}';
const HFWD: char = '9';
const HREV: char = '8';
const FREV: char = '7';

const NORMAL_CHARSET: i32 = 0;
const ALTERNATIVE_CHARSET: i32 = 1;
const SUPERSCRIPT: i32 = 1 << 1;
const SUBSCRIPT: i32 = 1 << 2;
const UNDERLINE: i32 = 1 << 3;
const BOLD: i32 = 1 << 4;

/// `BUFSIZ` da glibc: o tamanho inicial do buffer de colunas.
const BUFSIZ: usize = 8192;

const LONGS: &[LongOpt] = &[
    LongOpt::new("terminal", HasArg::Required, b't' as i32),
    LongOpt::new("indicated", HasArg::No, b'i' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 ul [options] [<file> ...]

Do underlining.

Options:
 -t, -T, --terminal TERMINAL  override the TERM environment variable
 -i, --indicated              underlining is indicated via a separate line
 -h, --help                   display this help
 -V, --version                display version

For more details see ul(1).
";

/// As capacidades de terminal que o `ul` usa (`struct term_caps`).
#[derive(Default)]
struct Caps {
    curs_up: Option<&'static str>,
    curs_right: Option<&'static str>,
    curs_left: Option<&'static str>,
    enter_standout: Option<&'static str>,
    exit_standout: Option<&'static str>,
    enter_underline: Option<&'static str>,
    exit_underline: Option<&'static str>,
    enter_dim: Option<&'static str>,
    enter_bold: Option<&'static str>,
    enter_reverse: Option<&'static str>,
    under_char: Option<&'static str>,
    exit_attributes: Option<&'static str>,
    must_use_uc: bool,
    must_overstrike: bool,
}

/// `init_term_caps`: lê as capacidades do terminal e aplica as substituições do original.
fn init_term_caps(t: &terms::T) -> Caps {
    let mut c = Caps {
        curs_up: t.0,
        curs_right: t.1,
        curs_left: t.2.or(Some("\u{8}")),
        enter_standout: t.3,
        exit_standout: t.4,
        enter_underline: t.5,
        exit_underline: t.6,
        enter_dim: t.7,
        enter_bold: t.8,
        enter_reverse: t.9,
        exit_attributes: t.10,
        ..Caps::default()
    };
    if c.enter_bold.is_none() && c.enter_reverse.is_some() {
        c.enter_bold = c.enter_reverse;
    }
    if c.enter_bold.is_none() && c.enter_standout.is_some() {
        c.enter_bold = c.enter_standout;
    }
    if c.enter_underline.is_none() && c.enter_standout.is_some() {
        c.enter_underline = c.enter_standout;
        c.exit_underline = c.exit_standout;
    }
    if c.enter_dim.is_none() && c.enter_standout.is_some() {
        c.enter_dim = c.enter_standout;
    }
    if c.enter_reverse.is_none() && c.enter_standout.is_some() {
        c.enter_reverse = c.enter_standout;
    }
    if c.exit_attributes.is_none() && c.exit_standout.is_some() {
        c.exit_attributes = c.exit_standout;
    }
    // Usa REVERSE pro conjunto alternativo, não as capacidades as/ae (o modelo é o teletipo 37).
    c.under_char = t.11;
    c.must_use_uc = c.under_char.is_some() && c.enter_underline.is_none();
    let (os, ul_flag) = (t.12, t.13);
    if (os && c.enter_bold.is_none())
        || (ul_flag && c.enter_underline.is_none() && c.under_char.is_none())
    {
        c.must_overstrike = true;
    }
    c
}

#[derive(Copy, Clone, Default)]
struct UlChar {
    c: char,
    width: i32,
    mode: i32,
}

/// Falha do filtro, tratada por quem chamou.
enum Fail {
    /// `fgetwc() failed` com o errno.
    Fgetwc(Errno),
    /// `unknown escape sequence in input: %o, %o`, com o caractere (None é WEOF).
    Escape(Option<char>),
}

struct Ul<W: Write> {
    caps: Caps,
    out: W,
    column: usize,
    max_column: usize,
    half_position: i32,
    up_line: i32,
    mode: i32,
    current_mode: i32,
    buf: Vec<UlChar>,
    indicated_opt: bool,
}

impl<W: Write> Ul<W> {
    fn putwchar(&mut self, c: char) {
        let mut b = [0u8; 4];
        let _ = self.out.write_all(c.encode_utf8(&mut b).as_bytes());
    }

    fn fputws(&mut self, chars: &[char]) {
        let s: String = chars.iter().take_while(|&&c| c != '\0').collect();
        let _ = self.out.write_all(s.as_bytes());
    }

    /// `print_line`: o `tputs` escreve cada byte como um caractere largo.
    fn print_line(&mut self, line: Option<&'static str>) {
        if let Some(s) = line {
            for b in s.bytes() {
                self.putwchar(char::from(b));
            }
        }
    }

    fn need_column(&mut self, new_max: usize) {
        self.max_column = new_max;
        while new_max >= self.buf.len() {
            let n = self.buf.len() * 2;
            self.buf.resize(n, UlChar::default());
        }
    }

    fn set_column(&mut self, column: usize) {
        self.column = column;
        if self.max_column < self.column {
            self.need_column(self.column);
        }
    }

    fn init_buffer(&mut self) {
        // Zera só as primeiras max_column posições, como o memset original.
        let n = self.max_column.min(self.buf.len());
        for cell in &mut self.buf[..n] {
            *cell = UlChar::default();
        }
        self.set_column(0);
        self.max_column = 0;
        self.mode &= ALTERNATIVE_CHARSET;
    }

    fn ul_setmode(&mut self, new_mode: i32) {
        if !self.indicated_opt {
            if self.current_mode != NORMAL_CHARSET && new_mode != NORMAL_CHARSET {
                self.ul_setmode(NORMAL_CHARSET);
            }
            match new_mode {
                NORMAL_CHARSET => match self.current_mode {
                    NORMAL_CHARSET => {}
                    UNDERLINE => self.print_line(self.caps.exit_underline),
                    _ => self.print_line(self.caps.exit_attributes),
                },
                ALTERNATIVE_CHARSET => self.print_line(self.caps.enter_reverse),
                SUPERSCRIPT => {
                    // Só funciona em poucos terminais.
                    self.print_line(self.caps.enter_underline);
                    self.print_line(self.caps.enter_dim);
                }
                SUBSCRIPT => self.print_line(self.caps.enter_dim),
                UNDERLINE => self.print_line(self.caps.enter_underline),
                BOLD => self.print_line(self.caps.enter_bold),
                _ => self.print_line(self.caps.enter_standout),
            }
        }
        self.current_mode = new_mode;
    }

    fn indicate_attribute(&mut self) {
        let mut buf: Vec<char> = Vec::with_capacity(self.max_column + 1);
        for i in 0..self.max_column {
            buf.push(match self.buf[i].mode {
                NORMAL_CHARSET => ' ',
                ALTERNATIVE_CHARSET => 'g',
                SUPERSCRIPT => '^',
                SUBSCRIPT => 'v',
                UNDERLINE => '_',
                BOLD => '!',
                _ => 'X',
            });
        }
        while buf.last() == Some(&' ') {
            buf.pop();
        }
        self.fputws(&buf);
        self.putwchar('\n');
    }

    fn output_char(&mut self, c: char, width: i32) {
        self.putwchar(c);
        if self.caps.must_use_uc && (self.current_mode & UNDERLINE) != 0 {
            for _ in 0..width {
                self.print_line(self.caps.curs_left);
            }
            for _ in 0..width {
                self.print_line(self.caps.under_char);
            }
        }
    }

    /// Pra terminais que sobrescrevem: sublinha e negrita por sobreposição (`\r`).
    fn overstrike(&mut self) {
        let mut buf: Vec<char> = Vec::with_capacity(self.max_column + 1);
        let mut had_bold = false;
        let mut i = 0;
        while i < self.max_column {
            let cell = self.buf[i];
            match cell.mode {
                UNDERLINE => buf.push('_'),
                BOLD => {
                    buf.push(cell.c);
                    if cell.width > 1 {
                        i += cell.width as usize - 1;
                    }
                    had_bold = true;
                }
                _ => buf.push(' '),
            }
            i += 1;
        }
        self.putwchar('\r');
        while buf.last() == Some(&' ') {
            buf.pop();
        }
        self.fputws(&buf);
        if had_bold {
            for _ in 0..2 {
                self.putwchar('\r');
                for &c in buf.iter().take_while(|&&c| c != '\0') {
                    self.putwchar(if c == '_' { ' ' } else { c });
                }
            }
        }
    }

    fn flush_line(&mut self) {
        let mut last_mode = NORMAL_CHARSET;
        let mut had_mode = false;
        let mut i = 0;
        while i < self.max_column {
            let cell = self.buf[i];
            if cell.mode != last_mode {
                had_mode = true;
                self.ul_setmode(cell.mode);
                last_mode = cell.mode;
            }
            if cell.c == '\0' {
                if self.up_line != 0 {
                    self.print_line(self.caps.curs_right);
                } else {
                    self.output_char(' ', 1);
                }
            } else {
                self.output_char(cell.c, cell.width);
            }
            if cell.width > 1 {
                i += cell.width as usize - 1;
            }
            i += 1;
        }
        if last_mode != NORMAL_CHARSET {
            self.ul_setmode(NORMAL_CHARSET);
        }
        if self.caps.must_overstrike && had_mode {
            self.overstrike();
        }
        self.putwchar('\n');
        if self.indicated_opt && had_mode {
            self.indicate_attribute();
        }
        let _ = self.out.flush();
        if self.up_line != 0 {
            self.up_line -= 1;
        }
        self.init_buffer();
        // O manipulador de SIGINT/SIGTERM faz _exit(0).
        if let Some(s) = sys::try_current()
            && !s.take_caught_signals().is_empty()
        {
            sys::exit(0);
        }
    }

    fn forward(&mut self) {
        let old_column = self.column;
        let old_maximum = self.max_column;
        self.flush_line();
        self.set_column(old_column);
        self.max_column = old_maximum;
    }

    fn reverse(&mut self) {
        self.up_line += 1;
        self.forward();
        self.print_line(self.caps.curs_up);
        self.print_line(self.caps.curs_up);
        self.up_line += 1;
    }

    /// `handle_escape`: `Ok(true)` quando a sequência é desconhecida (o caractere fica na entrada).
    fn handle_escape(&mut self, rd: &mut WideReader) -> Result<bool, Fail> {
        let c = rd.peek().map_err(Fail::Fgetwc)?;
        match c {
            Some(HREV) => {
                let _ = rd.getwc();
                if self.half_position > 0 {
                    self.mode &= !SUBSCRIPT;
                    self.half_position -= 1;
                } else if self.half_position == 0 {
                    self.mode |= SUPERSCRIPT;
                    self.half_position -= 1;
                } else {
                    self.half_position = 0;
                    self.reverse();
                }
                Ok(false)
            }
            Some(HFWD) => {
                let _ = rd.getwc();
                if self.half_position < 0 {
                    self.mode &= !SUPERSCRIPT;
                    self.half_position += 1;
                } else if self.half_position == 0 {
                    self.mode |= SUBSCRIPT;
                    self.half_position += 1;
                } else {
                    self.half_position = 0;
                    self.forward();
                }
                Ok(false)
            }
            Some(FREV) => {
                let _ = rd.getwc();
                self.reverse();
                Ok(false)
            }
            _ => Ok(true),
        }
    }

    fn filter(&mut self, rd: &mut WideReader) -> Result<(), Fail> {
        while let Some(c) = rd.getwc().map_err(Fail::Fgetwc)? {
            match c {
                '\u{8}' => {
                    let col = if self.column > 0 { self.column - 1 } else { 0 };
                    self.set_column(col);
                }
                '\t' => self.set_column((self.column + 8) & !7),
                '\r' => self.set_column(0),
                SO => self.mode |= ALTERNATIVE_CHARSET,
                SI => self.mode &= !ALTERNATIVE_CHARSET,
                ESC => {
                    if self.handle_escape(rd)? {
                        let c = rd.getwc().map_err(Fail::Fgetwc)?;
                        return Err(Fail::Escape(c));
                    }
                }
                '_' => {
                    let cell = self.buf[self.column];
                    if cell.c != '\0' || cell.width < 0 {
                        while self.buf[self.column].width < 0 && self.column > 0 {
                            self.column -= 1;
                        }
                        let width = self.buf[self.column].width;
                        for _ in 0..width.max(0) {
                            let m = UNDERLINE | self.mode;
                            self.buf[self.column].mode |= m;
                            self.column += 1;
                        }
                        let col = self.column;
                        self.set_column(col);
                        continue;
                    }
                    self.buf[self.column].c = '_';
                    self.buf[self.column].width = 1;
                    self.set_column(self.column + 1);
                }
                ' ' => self.set_column(self.column + 1),
                '\n' => self.flush_line(),
                '\u{c}' => {
                    self.flush_line();
                    self.putwchar('\u{c}');
                }
                _ => {
                    if c.is_control() || c == '\u{2028}' || c == '\u{2029}' {
                        // non printable
                        continue;
                    }
                    let width = wcwidth(c).max(0);
                    let w = width as usize;
                    self.need_column(self.column + w);
                    let col = self.column;
                    let mode = self.mode;
                    let mut advance = w;
                    if self.buf[col].c == '\0' {
                        self.buf[col].c = c;
                        for i in 0..w {
                            self.buf[col + i].mode = mode;
                        }
                        self.buf[col].width = width;
                        for i in 1..w {
                            self.buf[col + i].width = -1;
                        }
                    } else if self.buf[col].c == '_' {
                        self.buf[col].c = c;
                        for i in 0..w {
                            self.buf[col + i].mode |= UNDERLINE | mode;
                        }
                        self.buf[col].width = width;
                        for i in 1..w {
                            self.buf[col + i].width = -1;
                        }
                    } else if self.buf[col].c == c {
                        for i in 0..w {
                            self.buf[col + i].mode |= BOLD | mode;
                        }
                    } else {
                        // Outro caractere na célula: o antigo fica, só o modo é refeito, e o cursor
                        // anda a largura do antigo.
                        let ow = self.buf[col].width.max(0) as usize;
                        for i in 0..ow {
                            self.buf[col + i].mode = mode;
                        }
                        advance = ow;
                    }
                    self.set_column(col + advance);
                }
            }
        }
        if self.max_column != 0 {
            self.flush_line();
        }
        Ok(())
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    if let Some(s) = sys::try_current() {
        let _ = s.sigaction(Signal::SIGINT, SigDisposition::Catch);
        let _ = s.sigaction(Signal::SIGTERM, SigDisposition::Catch);
    }

    let mut termtype: Option<Vec<u8>> = sys::getenv("TERM");
    let mut opt_terminal = false;
    let mut indicated = false;

    let mut g = Getopt::from_env(&argv[1..], "it:T:Vh", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        match o.short() {
            // for nroff compatibility
            Some('t') | Some('T') => {
                termtype = o.arg.clone();
                opt_terminal = true;
            }
            Some('i') => indicated = true,
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            Some('h') => {
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

    // setupterm: 1 achou, 0 terminal desconhecido, -1 sem TERM (ou sem banco de terminfo).
    let found = termtype.as_deref().and_then(|name| {
        terms::TERMS
            .iter()
            .find(|(n, _)| n.as_bytes() == name)
            .map(|(_, t)| t)
    });
    let caps_src: &terms::T = match (&termtype, found) {
        (_, Some(t)) => t,
        (maybe, None) => {
            if maybe.is_none() {
                ul::warnx(&short, "trouble reading terminfo");
            }
            if opt_terminal {
                let name = termtype.as_deref().map(io::lossy).unwrap_or_default();
                ul::warnx(
                    &short,
                    format!("terminal `{name}' is not known, defaulting to `dumb'"),
                );
            }
            match terms::TERMS.iter().find(|(n, _)| *n == "dumb") {
                Some((_, t)) => t,
                None => return 1,
            }
        }
    };
    let caps = init_term_caps(caps_src);

    let mut ul_state = Ul {
        caps,
        out: io::stdout(),
        column: 0,
        max_column: 0,
        half_position: 0,
        up_line: 0,
        mode: 0,
        current_mode: NORMAL_CHARSET,
        buf: vec![UlChar::default(); BUFSIZ],
        indicated_opt: indicated,
    };
    ul_state.init_buffer();

    if files.is_empty() {
        let mut rd = WideReader::new(io::read_stdin());
        if let Err(f) = ul_state.filter(&mut rd) {
            return report(&mut ul_state, &short, f);
        }
    } else {
        for path in &files {
            let content = match File::open(path) {
                Ok(mut f) => f.read_to_end_sys(),
                Err(e) => {
                    ul::warn(&short, format!("cannot open {}", io::lossy(path)), e);
                    return 1;
                }
            };
            let mut rd = WideReader::new(content);
            if let Err(f) = ul_state.filter(&mut rd) {
                return report(&mut ul_state, &short, f);
            }
        }
    }
    if ul_state.out.flush().is_err() {
        return 1;
    }
    0
}

/// Imprime a falha do filtro (o `err`/`errx` do original descarrega o stdout antes) e devolve 1.
fn report<W: Write>(state: &mut Ul<W>, short: &str, f: Fail) -> i32 {
    let _ = state.out.flush();
    match f {
        Fail::Fgetwc(e) => ul::warn(short, "fgetwc() failed", e),
        Fail::Escape(c) => {
            let code = c.map_or(u32::MAX, |ch| ch as u32);
            ul::warnx(
                short,
                format!("unknown escape sequence in input: {:o}, {:o}", 0o33, code),
            );
        }
    }
    1
}
