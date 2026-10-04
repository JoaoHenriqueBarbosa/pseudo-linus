//! Varredura de um arquivo: porte do `grep()`, `fillbuf`, `grepbuf`, `prtext`, `prpending`,
//! `prline`, `print_line_head` e `print_line_middle` do `grep.c` do GNU grep 3.11.
//!
//! O buffer guarda, antes de `bufbeg`, um byte sentinela (o anterior ao trecho salvo), como no GNU;
//! as leituras são de 96 KiB, então a detecção de binário (NUL no primeiro buffer) se comporta igual.

use sysabi::{Errno, Fd, Stat, Whence, sys};

use super::Opts;
use super::matcher::LineMatcher;
use crate::io::{Out, safe_read};

/// `INITIAL_BUFSIZE` do grep 3.11.
const READ_SIZE: usize = 96 * 1024;

pub const SEP_SELECTED: u8 = b':';
pub const SEP_REJECTED: u8 = b'-';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryFiles {
    Binary,
    Text,
    WithoutMatch,
}

/// Estado que atravessa arquivos (o `static bool used` do `prtext`, erros, saída).
pub struct Searcher<'a> {
    pub o: &'a Opts,
    pub m: &'a LineMatcher,
    pub out: &'a mut Out,
    pub prog: &'a [u8],
    pub errseen: bool,
    /// Já houve saída (pro separador de grupo `--`).
    used: bool,
    /// Valores globais do GNU (`out_quiet`, `done_on_match`), restaurados a cada arquivo.
    pub out_quiet: bool,
    pub done_on_match: bool,
    /// Nome do arquivo corrente pra prefixo e mensagens.
    pub filename: Vec<u8>,
    pub out_file: bool,
    st: File,
}

/// Estado de um arquivo (as variáveis estáticas do `grep.c`).
struct File {
    buf: Vec<u8>,
    bufbeg: usize,
    buflim: usize,
    fd: Fd,
    bufoffset: u64,
    seek_failed: bool,
    totalcc: u64,
    lastnl: usize,
    lastout: Option<usize>,
    totalnl: u64,
    outleft: i64,
    after_last_match: u64,
    pending: i64,
    encoding_error_output: bool,
    offset_width: usize,
}

impl File {
    fn new() -> File {
        File {
            buf: Vec::new(),
            bufbeg: 1,
            buflim: 1,
            fd: Fd::STDIN,
            bufoffset: 0,
            seek_failed: false,
            totalcc: 0,
            lastnl: 1,
            lastout: None,
            totalnl: 0,
            outleft: 0,
            after_last_match: 0,
            pending: 0,
            encoding_error_output: false,
            offset_width: 0,
        }
    }
}

/// Resultado de uma varredura.
pub struct Scan {
    pub nlines: i64,
    pub ineof: bool,
}

impl<'a> Searcher<'a> {
    pub fn new(o: &'a Opts, m: &'a LineMatcher, out: &'a mut Out, prog: &'a [u8]) -> Searcher<'a> {
        Searcher {
            o,
            m,
            out,
            prog,
            errseen: false,
            used: false,
            out_quiet: o.out_quiet,
            done_on_match: o.done_on_match,
            filename: Vec::new(),
            out_file: false,
            st: File::new(),
        }
    }

    /// `suppressible_error`: `grep: <arquivo>: <strerror>`.
    pub fn suppressible_error(&mut self, e: Errno) {
        if !self.o.suppress_errors {
            // Como no stdio: o stderr sai na hora e o stdout fica no buffer.
            let msg = crate::io::errno_msg(&self.filename, e);
            crate::io::error(self.prog, &msg);
        }
        self.errseen = true;
    }

    fn eol(&self) -> u8 {
        self.o.eol
    }

    /// `reset`.
    fn reset(&mut self, fd: Fd) -> bool {
        let eol = self.eol();
        let f = &mut self.st;
        f.buf.clear();
        f.buf.push(eol);
        f.bufbeg = 1;
        f.buflim = 1;
        f.fd = fd;
        f.bufoffset = 0;
        f.seek_failed = false;
        if fd == Fd::STDIN {
            match sys::current().lseek(fd, 0, Whence::Cur) {
                Ok(off) => f.bufoffset = off,
                Err(Errno::ESPIPE) => f.seek_failed = true,
                Err(e) => {
                    self.suppressible_error(e);
                    return false;
                }
            }
        }
        true
    }

    /// `fillbuf`: guarda os últimos `save` bytes (e o byte anterior, sentinela) e lê mais dados.
    fn fillbuf(&mut self, save: usize) -> Result<(), Errno> {
        let f = &mut self.st;
        let keep_from = f.buflim - save - 1;
        if keep_from > 0 {
            f.buf.drain(..keep_from);
        }
        f.buf.truncate(save + 1);
        f.bufbeg = 1;
        let start = f.buf.len();
        if f.buf.try_reserve(READ_SIZE).is_err() {
            return Err(Errno::ENOMEM);
        }
        f.buf.resize(start + READ_SIZE, 0);
        let r = safe_read(f.fd, &mut f.buf[start..]);
        let n = *r.as_ref().unwrap_or(&0);
        f.buf.truncate(start + n);
        f.bufoffset += n as u64;
        f.buflim = f.buf.len();
        sys::checkpoint();
        r.map(|_| ())
    }

    fn nlscan(&mut self, lim: usize) {
        let eol = self.eol();
        let f = &mut self.st;
        if f.lastnl < lim {
            f.totalnl += f.buf[f.lastnl..lim].iter().filter(|&&b| b == eol).count() as u64;
        }
        f.lastnl = lim;
    }

    fn print_offset(&mut self, pos: u64) {
        let s = format!("{:>width$}", pos, width = self.st.offset_width);
        self.out.write(s.as_bytes());
    }

    /// `print_line_head`; `false` se a linha foi suprimida por erro de codificação.
    fn print_line_head(&mut self, beg: usize, len: usize, lim: usize, sep: u8) -> bool {
        if self.o.binary_files != BinaryFiles::Text && has_encoding_errors(&self.st.buf[beg..beg + len]) {
            self.st.encoding_error_output = true;
            return false;
        }
        if self.out_file {
            self.out.write(&self.filename.clone());
            if self.o.null_after_name {
                self.out.byte(0);
            } else {
                self.out.byte(sep);
            }
        }
        if self.o.line_number {
            if self.st.lastnl < lim {
                self.nlscan(beg);
                self.st.totalnl += 1;
                self.st.lastnl = lim;
            }
            self.print_offset(self.st.totalnl);
            self.out.byte(sep);
        }
        if self.o.byte_offset {
            let pos = self.st.totalcc + (beg - self.st.bufbeg) as u64;
            self.print_offset(pos);
            self.out.byte(sep);
        }
        if self.o.initial_tab && (self.out_file || self.o.line_number || self.o.byte_offset) && len != 0 {
            self.out.byte(b'\t');
        }
        true
    }

    /// `print_line_middle` (só o `-o`; cor não é suportada).
    fn print_line_middle(&mut self, beg: usize, lim: usize) -> Option<usize> {
        let line_end = lim - 1;
        let mut cur = beg;
        while cur < lim {
            let line = &self.st.buf[beg..line_end];
            let Some((s, e)) = self.m.find_in_line(line, cur - beg) else { break };
            let b = beg + s;
            let mut size = e - s;
            if b == lim {
                break;
            }
            if size == 0 {
                size = 1;
            } else {
                let sep = if self.o.invert { SEP_REJECTED } else { SEP_SELECTED };
                if !self.print_line_head(b, size, lim, sep) {
                    return None;
                }
                let m = self.st.buf[b..b + size].to_vec();
                self.out.write(&m);
                self.out.byte(self.eol());
            }
            cur = b + size;
        }
        Some(lim)
    }

    /// `prline`.
    fn prline(&mut self, beg: usize, lim: usize, sep: u8) {
        if !self.o.only_matching && !self.print_line_head(beg, lim - beg - 1, lim, sep) {
            return;
        }
        let matching = (sep == SEP_SELECTED) ^ self.o.invert;
        let mut beg = beg;
        if self.o.only_matching && matching {
            match self.print_line_middle(beg, lim) {
                Some(b) => beg = b,
                None => return,
            }
        }
        if !self.o.only_matching && lim > beg {
            let data = self.st.buf[beg..lim].to_vec();
            self.out.write(&data);
        }
        if self.o.line_buffered {
            self.out.flush();
        }
        if let Some(e) = self.out.error {
            self.write_error(e);
        }
        self.st.lastout = Some(lim);
    }

    /// `die (EXIT_TROUBLE, errno, "write error")`.
    fn write_error(&mut self, e: Errno) -> ! {
        let msg = crate::io::errno_msg(b"write error", e);
        crate::io::error(self.prog, &msg);
        sys::exit(2)
    }

    fn find_eol(&self, from: usize) -> usize {
        let eol = self.eol();
        let f = &self.st;
        f.buf[from..f.buflim].iter().position(|&b| b == eol).map(|i| from + i).unwrap_or(f.buflim - 1)
    }

    /// `prpending`.
    fn prpending(&mut self, lim: usize) {
        if self.st.lastout.is_none() {
            self.st.lastout = Some(self.st.bufbeg);
        }
        while self.st.pending > 0 && self.st.lastout.is_some_and(|l| l < lim) {
            let lo = self.st.lastout.unwrap_or(lim);
            let nl = self.find_eol(lo);
            self.prline(lo, nl + 1, SEP_REJECTED);
            self.st.pending -= 1;
        }
    }

    /// `prtext`.
    fn prtext(&mut self, beg: usize, lim: usize) {
        let eol = self.eol();
        if !self.out_quiet && self.st.pending > 0 {
            self.prpending(beg);
        }
        let mut p = beg;
        if !self.out_quiet {
            let bp = self.st.lastout.unwrap_or(self.st.bufbeg);
            for _ in 0..self.o.before.max(0) {
                if p > bp {
                    loop {
                        p -= 1;
                        if self.st.buf[p - 1] == eol {
                            break;
                        }
                    }
                }
            }
            if (self.o.before >= 0 || self.o.after >= 0)
                && self.used
                && Some(p) != self.st.lastout
                && let Some(sep) = &self.o.group_separator
            {
                self.out.write(sep);
                self.out.byte(b'\n');
            }
            while p < beg {
                let nl = self.find_eol(p) + 1;
                self.prline(p, nl, SEP_REJECTED);
                p = nl;
            }
        }
        let n: i64;
        if self.o.invert {
            let mut k = 0;
            while p < lim && k < self.st.outleft {
                let nl = self.find_eol(p) + 1;
                if !self.out_quiet {
                    self.prline(p, nl, SEP_SELECTED);
                }
                p = nl;
                k += 1;
            }
            n = k;
        } else {
            if !self.out_quiet {
                self.prline(beg, lim, SEP_SELECTED);
            }
            n = 1;
            p = lim;
        }
        self.st.after_last_match = self.st.bufoffset - (self.st.buflim - p) as u64;
        self.st.pending = if self.out_quiet { 0 } else { self.o.after.max(0) };
        self.used = true;
        self.st.outleft -= n;
    }

    /// `grepbuf`: linhas selecionadas em `[beg, lim)`.
    fn grepbuf(&mut self, beg: usize, lim: usize) -> i64 {
        let outleft0 = self.st.outleft;
        let eol = self.eol();
        let mut p = beg;
        while p < lim {
            sys::checkpoint();
            let found = self.m.next_line(&self.st.buf, p, lim, eol);
            let (b, endp) = match found {
                Some(x) => x,
                None => {
                    if !self.o.invert {
                        break;
                    }
                    (lim, lim)
                }
            };
            if !self.o.invert && b == lim {
                break;
            }
            if !self.o.invert || p < b {
                let (prbeg, prend) = if self.o.invert { (p, b) } else { (b, endp) };
                self.prtext(prbeg, prend);
                if self.st.outleft == 0 || self.done_on_match {
                    if self.o.exit_on_match {
                        self.out.flush();
                        sys::exit(0);
                    }
                    break;
                }
            }
            p = endp;
        }
        outleft0 - self.st.outleft
    }

    /// `grep()`: varre o fd. `st` é o `fstat` do arquivo.
    pub fn grep(&mut self, fd: Fd, st: &Stat) -> Scan {
        let eol = self.eol();
        let done_on_match_0 = self.done_on_match;
        let out_quiet_0 = self.out_quiet;
        let mut nlines_first_null: i64 = -1;
        let mut nul_zapper: u8 = 0;
        let mut scan = Scan { nlines: 0, ineof: false };
        if !self.reset(fd) {
            return scan;
        }
        {
            let f = &mut self.st;
            f.totalcc = 0;
            f.lastout = None;
            f.totalnl = 0;
            f.outleft = self.o.max_count;
            f.after_last_match = 0;
            f.pending = 0;
            f.encoding_error_output = false;
        }
        let mut residue: usize = 0;
        let mut save: usize = 0;
        if let Err(e) = self.fillbuf(save) {
            self.suppressible_error(e);
            return scan;
        }
        self.st.offset_width = 0;
        if self.o.initial_tab {
            let regular = sysabi::FileType::from_mode(st.mode) == sysabi::FileType::Regular;
            let mut num: u64 = if regular { st.size } else { i64::MAX as u64 };
            if self.o.line_number && num < i64::MAX as u64 {
                num += 1;
            }
            loop {
                self.st.offset_width += 1;
                num /= 10;
                if num == 0 {
                    break;
                }
            }
        }
        let mut firsttime = true;
        'outer: loop {
            if nlines_first_null < 0
                && eol != 0
                && self.o.binary_files != BinaryFiles::Text
                && (self.st.buf[self.st.bufbeg..self.st.buflim].contains(&0)
                    || (firsttime && self.file_must_have_nulls(fd, st)))
            {
                if self.o.binary_files == BinaryFiles::WithoutMatch {
                    return Scan { nlines: 0, ineof: false };
                }
                if !self.o.count_matches {
                    self.done_on_match = true;
                    self.out_quiet = true;
                }
                nlines_first_null = scan.nlines;
                nul_zapper = eol;
            }
            firsttime = false;
            self.st.lastnl = self.st.bufbeg;
            if self.st.lastout.is_some() {
                self.st.lastout = Some(self.st.bufbeg);
            }
            let mut beg = self.st.bufbeg + save;
            if beg == self.st.buflim {
                scan.ineof = true;
                break;
            }
            if nul_zapper != 0 {
                let lim = self.st.buflim;
                for b in &mut self.st.buf[beg..lim] {
                    if *b == 0 {
                        *b = nul_zapper;
                    }
                }
            }
            // Resíduo: linha incompleta no fim do buffer.
            let buflim = self.st.buflim;
            let mut lim = self.st.buf[beg..buflim].iter().rposition(|&b| b == eol).map(|i| beg + i + 1).unwrap_or(beg);
            if lim == beg {
                lim = beg - residue;
            }
            beg -= residue;
            residue = buflim - lim;
            if beg < lim {
                if self.st.outleft > 0 {
                    scan.nlines += self.grepbuf(beg, lim);
                }
                if self.st.pending > 0 {
                    self.prpending(lim);
                }
                if (self.st.outleft == 0 && self.st.pending == 0)
                    || (self.done_on_match && nlines_first_null.max(0) < scan.nlines)
                {
                    break 'outer;
                }
            }
            // As últimas `before` linhas podem servir de contexto pra próxima leitura.
            let mut i = 0;
            let mut b2 = lim;
            while i < self.o.before.max(0) && b2 > self.st.bufbeg && Some(b2) != self.st.lastout {
                i += 1;
                loop {
                    b2 -= 1;
                    if self.st.buf[b2 - 1] == eol {
                        break;
                    }
                }
            }
            if Some(b2) != self.st.lastout {
                self.st.lastout = None;
            }
            save = residue + lim - b2;
            if self.o.byte_offset {
                self.st.totalcc += (self.st.buflim - self.st.bufbeg - save) as u64;
            }
            if self.o.line_number {
                self.nlscan(b2);
            }
            // Os índices mudam na leitura; `lastout` (que, se existe, é `b2`) e `lastnl` são
            // recolocados no começo do trecho salvo no topo do laço, como no GNU.
            if let Err(e) = self.fillbuf(save) {
                self.suppressible_error(e);
                break 'outer;
            }
        }
        if scan.ineof && residue > 0 {
            self.st.buf.push(eol);
            self.st.buflim += 1;
            let start = self.st.bufbeg + save - residue;
            let lim = self.st.buflim;
            if self.st.outleft > 0 {
                scan.nlines += self.grepbuf(start, lim);
            }
            if self.st.pending > 0 {
                self.prpending(lim);
            }
        }
        self.done_on_match = done_on_match_0;
        self.out_quiet = out_quiet_0;
        if self.o.binary_files == BinaryFiles::Binary
            && !self.out_quiet
            && (self.st.encoding_error_output || (nlines_first_null >= 0 && nlines_first_null < scan.nlines))
        {
            let mut msg = self.filename.clone();
            msg.extend_from_slice(b": binary file matches");
            crate::io::error(self.prog, &msg);
        }
        scan
    }

    /// `file_must_have_nulls`: arquivo com buraco tem NUL.
    fn file_must_have_nulls(&mut self, fd: Fd, st: &Stat) -> bool {
        let size = (self.st.buflim - self.st.bufbeg) as u64;
        let regular = sysabi::FileType::from_mode(st.mode) == sysabi::FileType::Regular;
        if self.st.seek_failed || !regular || size >= st.size {
            return false;
        }
        let sys = sys::current();
        let cur = if fd == Fd::STDIN {
            match sys.lseek(fd, 0, Whence::Cur) {
                Ok(c) => c,
                Err(_) => return false,
            }
        } else {
            size
        };
        match sys.lseek(fd, cur as i64, Whence::Hole) {
            Ok(hole) => {
                if let Err(e) = sys.lseek(fd, cur as i64, Whence::Set) {
                    self.suppressible_error(e);
                }
                hole < st.size
            }
            Err(_) => false,
        }
    }

    /// `finalize_input`: deixa o stdin onde o GNU deixaria (depois da última linha usada, ou no
    /// fim).
    pub fn finalize_input(&mut self, fd: Fd, st: &Stat, ineof: bool) {
        if fd != Fd::STDIN {
            return;
        }
        let sys = sys::current();
        if self.st.outleft > 0 {
            if ineof {
                return;
            }
            let at_end = !self.st.seek_failed && sys.lseek(fd, 0, Whence::End).is_ok();
            if !at_end && let Err(e) = self.drain(fd, st) {
                self.suppressible_error(e);
            }
        } else if self.st.bufoffset != self.st.after_last_match
            && !self.st.seek_failed
            && let Err(e) = sys.lseek(fd, self.st.after_last_match as i64, Whence::Set)
        {
            self.suppressible_error(e);
        }
    }

    fn drain(&mut self, fd: Fd, _st: &Stat) -> Result<(), Errno> {
        let mut buf = vec![0u8; READ_SIZE];
        loop {
            if safe_read(fd, &mut buf)? == 0 {
                return Ok(());
            }
            sys::checkpoint();
        }
    }

    pub fn outleft(&self) -> i64 {
        self.st.outleft
    }
}

/// `buf_has_encoding_errors` em UTF-8.
pub fn has_encoding_errors(data: &[u8]) -> bool {
    std::str::from_utf8(data).is_err()
}
