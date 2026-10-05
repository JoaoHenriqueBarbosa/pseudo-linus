//! `col` do util-linux 2.41 (pacote bsdextrautils do Debian 13): filtra avanços de linha reversos da
//! entrada padrão, resolvendo sobreposições com backspace, meias linhas e tabulações.
//!
//! É um porte direto do `text-utils/col.c`: as linhas ficam num buffer (uma fila de linhas, com a
//! linha corrente), os caracteres entram com coluna e largura, e as linhas saem quando passam de
//! `-l` linhas pra trás do cursor. Entrada UTF-8 inválida vira `\xNN` (um byte por vez) como no
//! original; uma sequência incompleta no fim da entrada é descartada.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, KillTarget, Signal, sys};

use crate::util::io;
use crate::util::ul::{self, Wide, is_wspace, peek_wide, wcwidth};
use crate::util::{Getopt, HasArg, LongOpt};

const BS: char = '\u{8}';
const NL: char = '\n';
const CR: char = '\r';
const TAB: char = '\t';
const VT: char = '\u{b}';
const ESC: char = '\u{1b}';
const RLF: char = '\u{7}';
const RHLF: char = BS;
const FHLF: char = TAB;
const SO: char = '\u{e}';
const SI: char = '\u{f}';

const BUFFER_MARGIN: i64 = 32;

const CS_NORMAL: u8 = 0;
const CS_ALTERNATE: u8 = 1;

const LONGS: &[LongOpt] = &[
    LongOpt::new("no-backspaces", HasArg::No, b'b' as i32),
    LongOpt::new("fine", HasArg::No, b'f' as i32),
    LongOpt::new("pass", HasArg::No, b'p' as i32),
    LongOpt::new("tabs", HasArg::No, b'h' as i32),
    LongOpt::new("spaces", HasArg::No, b'x' as i32),
    LongOpt::new("lines", HasArg::Required, b'l' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'H' as i32),
];

const USAGE: &str = "
Usage:
 col [options]

Filter out reverse line feeds from standard input.

Options:
 -b, --no-backspaces    do not output backspaces
 -f, --fine             permit forward half line feeds
 -p, --pass             pass unknown control sequences
 -h, --tabs             convert spaces to tabs
 -x, --spaces           convert tabs to spaces
 -l, --lines NUM        buffer at least NUM lines
 -H, --help             display this help
 -V, --version          display version

For more details see col(1).
";

#[derive(Copy, Clone)]
struct ColChar {
    column: usize,
    ch: char,
    width: i32,
    set: u8,
}

#[derive(Default)]
struct ColLine {
    chars: Vec<ColChar>,
    max_col: usize,
    needs_sort: bool,
}

struct Col<W: Write> {
    lines: VecDeque<ColLine>,
    /// Índice da linha corrente em `lines`.
    li: usize,
    max_bufd_lines: i64,
    nblank_lines: i64,
    last_set: u8,
    compress_spaces: bool,
    fine: bool,
    no_backspaces: bool,
    pass_unknown_seqs: bool,
    out: W,
    write_failed: bool,
    short: String,
}

/// O estado de leitura (`struct col_lines`).
struct Lns {
    /// Largura do último caractere guardado (o `lns->c->c_width`); `None` antes do primeiro.
    last_width: Option<i32>,
    ch: char,
    adjust: i64,
    cur_col: usize,
    cur_line: i64,
    extra_lines: i64,
    max_line: i64,
    nflushd_lines: i64,
    this_line: i64,
    cur_set: u8,
    warned: bool,
}

/// Termina o processo com SIGSEGV, como o `col` original em colunas fora de faixa.
fn crash() -> ! {
    let s = sys::current();
    let _ = s.kill(KillTarget::Pid(s.getpid()), Signal::SIGSEGV);
    sys::exit(139)
}

/// `iswgraph`: imprimível e não espaço.
fn is_wgraph(c: char) -> bool {
    !c.is_control() && !is_wspace(c)
}

impl<W: Write> Col<W> {
    fn put(&mut self, ch: char) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        if self.out.write_all(s.as_bytes()).is_err() {
            self.write_failed = true;
        }
    }

    fn flush_blanks(&mut self) {
        let mut half = false;
        let mut nb = self.nblank_lines;
        if nb & 1 != 0 {
            if self.fine {
                half = true;
            } else {
                nb += 1;
            }
        }
        nb /= 2;
        for _ in 0..nb.max(0) {
            self.put(NL);
        }
        if half {
            self.put(ESC);
            self.put('9');
            if nb == 0 {
                self.put(CR);
            }
        }
        self.nblank_lines = 0;
    }

    fn flush_line(&mut self, mut line: ColLine) {
        let mut nchars = line.chars.len() as isize;
        if line.needs_sort {
            // Ordenação estável por coluna (o original faz uma contagem O(n) que preserva a ordem).
            line.chars.sort_by_key(|c| c.column);
        }
        let chars = &line.chars;
        let mut last_col: usize = 0;
        let mut c: usize = 0;

        while nchars > 0 {
            let this_col = chars[c].column;
            let mut endc = c;
            // find last character
            loop {
                endc += 1;
                nchars -= 1;
                if !(nchars > 0 && endc < chars.len() && this_col == chars[endc].column) {
                    break;
                }
            }

            if self.no_backspaces {
                // print only the last character
                c = endc - 1;
                if nchars > 0 && endc < chars.len() && chars[endc].column < this_col.wrapping_add(chars[c].width as isize as usize) {
                    continue;
                }
            }

            if last_col < this_col {
                // tabs and spaces handling
                let mut nspace = (this_col - last_col) as isize;
                if self.compress_spaces && nspace > 1 {
                    let ntabs = (this_col / 8) as isize - (last_col / 8) as isize;
                    if ntabs > 0 {
                        nspace = (this_col & 7) as isize;
                        for _ in 0..ntabs {
                            self.put(TAB);
                        }
                    }
                }
                for _ in 0..nspace.max(0) {
                    self.put(' ');
                }
                last_col = this_col;
            }

            loop {
                // SO / SI character set changing
                if chars[c].set != self.last_set {
                    match chars[c].set {
                        CS_NORMAL => self.put(SI),
                        _ => self.put(SO),
                    }
                    self.last_set = chars[c].set;
                }

                // output a character
                self.put(chars[c].ch);

                // rubout control chars from output
                if c + 1 < endc {
                    for _ in 0..chars[c].width {
                        self.put(BS);
                    }
                }

                c += 1;
                if endc <= c {
                    break;
                }
            }
            last_col = last_col.wrapping_add(chars[c - 1].width as isize as usize);
        }
    }

    fn flush_lines(&mut self, nflush: i64) {
        for _ in 0..nflush.max(0) {
            let Some(l) = self.lines.pop_front() else { break };
            self.li = self.li.saturating_sub(1);
            if !l.chars.is_empty() {
                self.flush_blanks();
                self.flush_line(l);
            }
            self.nblank_lines += 1;
        }
    }

    /// `handle_not_graphic`: `true` quando o caractere foi tratado (não vai pra linha).
    fn handle_not_graphic(&mut self, lns: &mut Lns, input: &mut Input) -> bool {
        match lns.ch {
            BS => {
                if lns.cur_col == 0 {
                    return true; // can't go back further
                }
                match lns.last_width {
                    Some(w) => {
                        let back = w as isize as usize;
                        if w > 0 && back > lns.cur_col {
                            // O original recua além da coluna 0 (size_t dá a volta) e acaba
                            // derrubado por SIGSEGV ao arrumar a linha; aqui o processo cai igual.
                            crash();
                        }
                        lns.cur_col = lns.cur_col.wrapping_sub(back);
                    }
                    None => lns.cur_col -= 1,
                }
                return true;
            }
            CR => {
                lns.cur_col = 0;
                return true;
            }
            ESC => {
                // just ignore EOF
                match input.next_wide() {
                    Some(RLF) => lns.cur_line -= 2,
                    Some(RHLF) => lns.cur_line -= 1,
                    Some(FHLF) => {
                        lns.cur_line += 1;
                        if lns.cur_line > 0 && lns.max_line < lns.cur_line {
                            lns.max_line = lns.cur_line;
                        }
                    }
                    _ => {}
                }
                return true;
            }
            NL => {
                lns.cur_line += 2;
                if lns.cur_line > 0 && lns.max_line < lns.cur_line {
                    lns.max_line = lns.cur_line;
                }
                lns.cur_col = 0;
                return true;
            }
            ' ' => {
                lns.cur_col += 1;
                return true;
            }
            SI => {
                lns.cur_set = CS_NORMAL;
                return true;
            }
            SO => {
                lns.cur_set = CS_ALTERNATE;
                return true;
            }
            TAB => {
                // adjust column
                lns.cur_col |= 7;
                lns.cur_col += 1;
                return true;
            }
            VT => {
                lns.cur_line -= 2;
                return true;
            }
            _ => {}
        }
        if is_wspace(lns.ch) {
            let w = wcwidth(lns.ch);
            if w > 0 {
                lns.cur_col += w as usize;
            }
            return true;
        }
        !self.pass_unknown_seqs
    }

    fn update_cur_line(&mut self, lns: &mut Lns) {
        lns.adjust = 0;
        let mut nmove = lns.cur_line - lns.this_line;
        if !self.fine && lns.cur_line & 1 != 0 {
            // round up to next line
            lns.adjust = 1;
            nmove += 1;
        }
        if nmove < 0 {
            while nmove < 0 && self.li > 0 {
                self.li -= 1;
                nmove += 1;
            }
            if nmove != 0 {
                if lns.nflushd_lines == 0 {
                    // Allow backup past first line if nothing has been flushed yet.
                    while nmove < 0 {
                        self.lines.push_front(ColLine::default());
                        self.li = 0;
                        lns.extra_lines += 1;
                        nmove += 1;
                    }
                } else {
                    if !lns.warned {
                        let what = if lns.cur_line < 0 { "past first line" } else { "-- line already flushed" };
                        let _ = self.out.flush();
                        io::eprint(format!("{}: warning: can't back up {what}.\n", self.short));
                        lns.warned = true;
                    }
                    lns.cur_line -= nmove;
                }
            }
        } else {
            // may need to allocate here
            while nmove > 0 && self.li + 1 < self.lines.len() {
                self.li += 1;
                nmove -= 1;
            }
            while nmove > 0 {
                self.lines.push_back(ColLine::default());
                self.li = self.lines.len() - 1;
                nmove -= 1;
            }
        }

        lns.this_line = lns.cur_line + lns.adjust;
        let nmove = lns.this_line - lns.nflushd_lines;

        if nmove > 0 && self.max_bufd_lines + BUFFER_MARGIN <= nmove {
            lns.nflushd_lines += nmove - self.max_bufd_lines;
            self.flush_lines(nmove - self.max_bufd_lines);
        }
    }

    fn process_char(&mut self, lns: &mut Lns, input: &mut Input) {
        // Deal printable characters
        if !is_wgraph(lns.ch) && self.handle_not_graphic(lns, input) {
            return;
        }

        // Must stuff ch in a line - are we at the right one?
        if lns.cur_line != lns.this_line - lns.adjust {
            self.update_cur_line(lns);
        }

        // Store character
        let width = wcwidth(lns.ch);
        let column = if lns.cur_col > 0 { lns.cur_col } else { 0 };
        let line = &mut self.lines[self.li];
        line.chars.push(ColChar { column, ch: lns.ch, width, set: lns.cur_set });
        lns.last_width = Some(width);

        // If things are put in out of order, they will need sorting when it is flushed.
        if lns.cur_col < line.max_col {
            line.needs_sort = true;
        } else {
            line.max_col = lns.cur_col;
        }
        if width > 0 {
            lns.cur_col += width as usize;
        }
    }
}

/// A entrada, decodificada como o `getwchar` em C.UTF-8.
struct Input {
    data: Vec<u8>,
    pos: usize,
}

impl Input {
    /// O próximo caractere sem consumir; sequência incompleta no fim da entrada é fim de arquivo.
    fn peek(&self) -> Wide {
        match peek_wide(&self.data, self.pos) {
            Wide::Truncated => Wide::Eof,
            w => w,
        }
    }

    /// O próximo caractere (consumindo), ou `None` no fim ou em sequência inválida (que fica).
    fn next_wide(&mut self) -> Option<char> {
        match self.peek() {
            Wide::Char(c) => {
                self.pos += c.len_utf8();
                Some(c)
            }
            _ => None,
        }
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut max_bufd_lines = BUFFER_MARGIN * 2;
    let (mut compress, mut fine, mut nobs, mut pass) = (true, false, false, false);
    let mut seen_tabs_spaces: Option<char> = None;

    let mut g = Getopt::from_env(&argv[1..], "bfhl:pxVH", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        let c = o.short().unwrap_or('?');
        // err_exclusive_options: -h e -x não convivem.
        if c == 'h' || c == 'x' {
            match seen_tabs_spaces {
                None => seen_tabs_spaces = Some(c),
                Some(p) if p != c => {
                    io::eprint(format!("{short}: mutually exclusive arguments: --tabs --spaces\n"));
                    return 1;
                }
                _ => {}
            }
        }
        match c {
            'b' => nobs = true,
            'f' => fine = true,
            'h' => compress = true,
            'l' => match ul::strtou32_or_err(o.arg.as_deref().unwrap_or(b""), "bad -l argument") {
                Ok(n) => max_bufd_lines = i64::from(n) * 2,
                Err(m) => {
                    io::eprint(format!("{short}: {m}\n"));
                    return 1;
                }
            },
            'p' => pass = true,
            'x' => compress = false,
            'V' => {
                ul::print_version(&short);
                return 0;
            }
            'H' => {
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
    if !g.operands().is_empty() {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }

    let data = io::read_stdin().unwrap_or_default();
    let mut input = Input { data, pos: 0 };
    let mut col = Col {
        lines: VecDeque::from([ColLine::default()]),
        li: 0,
        max_bufd_lines,
        nblank_lines: 0,
        last_set: CS_NORMAL,
        compress_spaces: compress,
        fine,
        no_backspaces: nobs,
        pass_unknown_seqs: pass,
        out: io::stdout(),
        write_failed: false,
        short: short.clone(),
    };
    let mut lns = Lns {
        last_width: None,
        ch: '\0',
        adjust: 0,
        cur_col: 0,
        cur_line: 0,
        extra_lines: 0,
        max_line: 0,
        nflushd_lines: 0,
        this_line: 0,
        cur_set: CS_NORMAL,
        warned: false,
    };

    loop {
        match input.peek() {
            Wide::Eof | Wide::Truncated => break,
            Wide::Char(c) => {
                input.pos += c.len_utf8();
                lns.ch = c;
                col.process_char(&mut lns, &mut input);
            }
            Wide::Invalid(b) => {
                // Illegal multibyte sequence: o byte vira \xNN, um caractere por vez.
                input.pos += 1;
                for ch in format!("\\x{b:02x}").chars() {
                    lns.ch = ch;
                    col.process_char(&mut lns, &mut input);
                }
            }
        }
        if input.pos.is_multiple_of(4096) {
            sys::checkpoint();
        }
    }

    // goto the last line that had a character on it
    while col.li + 1 < col.lines.len() {
        col.li += 1;
        lns.this_line += 1;
    }
    if lns.max_line == 0 && lns.cur_col == 0 {
        return finish(&mut col, &short);
    }
    col.flush_lines(lns.this_line - lns.nflushd_lines + lns.extra_lines + 1);

    // make sure we leave things in a sane state
    if col.last_set != CS_NORMAL {
        col.put(SI);
    }

    // flush out the last few blank lines
    col.nblank_lines = lns.max_line - lns.this_line;
    if lns.max_line & 1 != 0 {
        col.nblank_lines += 1;
    } else if col.nblank_lines == 0 {
        // missing a \n on the last line?
        col.nblank_lines = 2;
    }
    col.flush_blanks();
    finish(&mut col, &short)
}

fn finish<W: Write>(col: &mut Col<W>, short: &str) -> i32 {
    if col.write_failed || col.out.flush().is_err() {
        io::eprint(format!("{short}: write failed\n"));
        return 1;
    }
    0
}
