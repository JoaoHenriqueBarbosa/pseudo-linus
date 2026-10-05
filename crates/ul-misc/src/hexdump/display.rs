//! Leitura dos blocos e impressão (porte do `text-utils/hexdump-display.c` e do
//! `hexdump-conv.c` do util-linux 2.41, BSD; ver o cabeçalho de licença em `mod.rs`).

use sysabi::{Errno, FileType, Whence, sys};

use super::cfmt::Arg;
use super::{Hexdump, Input, Kind, Pr, VFlag, fatal};
use crate::util::io;

/// Nomes do `%_u` pros bytes de controle (o od usava `nl`, aqui é `lf`).
const CONV_U_NAMES: [&str; 32] = [
    "nul", "soh", "stx", "etx", "eot", "enq", "ack", "bel", "bs", "ht", "lf", "vt", "ff", "cr",
    "so", "si", "dle", "dc1", "dc2", "dc3", "dc4", "nak", "syn", "etb", "can", "em", "sub", "esc",
    "fs", "gs", "rs", "us",
];

/// `isprint` do C.UTF-8 pra um byte isolado: só o ASCII imprimível.
fn is_print(b: u8) -> bool {
    (0x20..0x7f).contains(&b)
}

/// `n` bytes a partir de `off` (zero depois do fim, como a memória zerada do `calloc`).
fn bytes_at(buf: &[u8], off: usize, n: usize) -> [u8; 8] {
    let mut v = [0u8; 8];
    for (k, slot) in v.iter_mut().enumerate().take(n.min(8)) {
        *slot = buf.get(off + k).copied().unwrap_or(0);
    }
    v
}

/// `fread`: lê até `buf` encher, o arquivo acabar ou dar erro.
fn fread(fd: sysabi::Fd, buf: &mut [u8]) -> (usize, Option<Errno>) {
    let mut n = 0;
    while n < buf.len() {
        match sys::read(fd, &mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(Errno::EINTR) => {}
            Err(e) => return (n, Some(e)),
        }
    }
    (n, None)
}

/// `bpad`: a conversão vira `%s` com cadeia vazia, sem os flags (sobra só a largura).
fn bpad(pr: &mut Pr) {
    pr.kind = Kind::Bpad;
    pr.fmt.truncate(pr.cchar);
    pr.fmt.push(b's');
    if let Some(pct) = pr.fmt.iter().position(|&b| b == b'%') {
        let mut k = pct + 1;
        while k < pr.fmt.len() && b" -0+#".contains(&pr.fmt[k]) {
            k += 1;
        }
        let removed = k - (pct + 1);
        pr.fmt.drain(pct + 1..k);
        pr.cchar -= removed;
        if let Some(ns) = pr.nospace.as_mut() {
            *ns = ns.saturating_sub(removed);
        }
    }
}

impl Hexdump {
    /// `color_cond`: a primeira unidade de cor que vale pra esta conversão.
    fn color_cond(&self, pr: &Pr, block: &[u8], bp: usize, bcnt: i32) -> Option<&'static str> {
        let list = pr.colorlist.as_ref()?;
        let address = self.address;
        for clr in list {
            let mut offt = clr.offt;
            let mut matched = false;
            if offt < 0 {
                offt = address;
            }
            if offt < address || offt + i64::from(clr.range) > address + i64::from(bcnt) {
                continue;
            }
            let rel = (offt - address) as usize;
            if let Some(s) = &clr.str {
                if pr.kind != Kind::Address {
                    // `strncmp`: compara até `range` bytes, parando no NUL dos dois lados.
                    let n = clr.range.max(0) as usize;
                    let mut eq = true;
                    for k in 0..n {
                        let a = s.get(k).copied().unwrap_or(0);
                        let b = block.get(bp + rel + k).copied().unwrap_or(0);
                        if a != b {
                            eq = false;
                            break;
                        }
                        if a == 0 {
                            break;
                        }
                    }
                    matched = eq;
                }
            } else if clr.val != -1 {
                if pr.kind == Kind::Address {
                    matched = i64::from(clr.val) == address;
                } else {
                    let n = (clr.range.max(0) as usize).min(4);
                    let b = bytes_at(block, bp + rel, n);
                    let val = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                    matched = val == clr.val;
                }
            } else {
                return (!clr.fmt.is_empty()).then_some(clr.fmt);
            }
            if matched ^ clr.invert {
                return (!clr.fmt.is_empty()).then_some(clr.fmt);
            }
        }
        None
    }

    /// `print`: uma unidade de impressão sobre os bytes em `block[bp..]`.
    fn print_pr(&mut self, pr: &mut Pr, fmt_len: usize, block: &[u8], bp: usize) {
        let color = if pr.colorlist.is_some() {
            self.color_cond(pr, block, bp, pr.bcnt)
        } else {
            None
        };
        if let Some(c) = color {
            self.out.extend_from_slice(c.as_bytes());
        }
        let b0 = block.get(bp).copied().unwrap_or(0);
        match pr.kind {
            Kind::Address => {
                let fmt = pr.fmt[..fmt_len].to_vec();
                self.printf(&fmt, Arg::Int(self.address as u64));
            }
            Kind::Bpad => {
                let fmt = pr.fmt[..fmt_len].to_vec();
                self.printf(&fmt, Arg::Str(b""));
            }
            Kind::C => {
                let s: Option<&[u8]> = match b0 {
                    0 => Some(b"\\0"),
                    0x07 => Some(b"\\a"),
                    0x08 => Some(b"\\b"),
                    0x0c => Some(b"\\f"),
                    b'\n' => Some(b"\\n"),
                    b'\r' => Some(b"\\r"),
                    b'\t' => Some(b"\\t"),
                    0x0b => Some(b"\\v"),
                    _ => None,
                };
                if s.is_none() && is_print(b0) {
                    pr.fmt[pr.cchar] = b'c';
                    let fmt = pr.fmt[..fmt_len].to_vec();
                    self.printf(&fmt, Arg::Int(u64::from(b0)));
                } else {
                    let oct = format!("{b0:03o}");
                    let s = s.map(<[u8]>::to_vec).unwrap_or_else(|| oct.into_bytes());
                    pr.fmt[pr.cchar] = b's';
                    let fmt = pr.fmt[..fmt_len].to_vec();
                    self.printf(&fmt, Arg::Str(&s));
                }
            }
            Kind::Char => {
                let fmt = pr.fmt[..fmt_len].to_vec();
                self.printf(&fmt, Arg::Int(u64::from(b0)));
            }
            Kind::Dbl => {
                let b = bytes_at(block, bp, pr.bcnt as usize);
                let v = if pr.bcnt == 4 {
                    f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                } else {
                    f64::from_le_bytes(b)
                };
                let fmt = pr.fmt[..fmt_len].to_vec();
                self.printf(&fmt, Arg::Dbl(v));
            }
            Kind::Int => {
                let b = bytes_at(block, bp, pr.bcnt as usize);
                let v: i64 = match pr.bcnt {
                    1 => i64::from(b[0] as i8),
                    2 => i64::from(i16::from_le_bytes([b[0], b[1]])),
                    4 => i64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])),
                    _ => i64::from_le_bytes(b),
                };
                let fmt = pr.fmt[..fmt_len].to_vec();
                self.printf(&fmt, Arg::Int(v as u64));
            }
            Kind::P => {
                let fmt = pr.fmt[..fmt_len].to_vec();
                self.printf(
                    &fmt,
                    Arg::Int(u64::from(if is_print(b0) { b0 } else { b'.' })),
                );
            }
            Kind::Str => {
                let tail = block.get(bp..).unwrap_or(&[]);
                let end = tail.iter().position(|&c| c == 0).unwrap_or(tail.len());
                let s = tail[..end].to_vec();
                let fmt = pr.fmt[..fmt_len].to_vec();
                self.printf(&fmt, Arg::Str(&s));
            }
            Kind::Text => {
                let t = pr.fmt[..fmt_len].to_vec();
                self.out.extend_from_slice(&t);
            }
            Kind::U => {
                if b0 <= 0x1f || b0 == 0x7f {
                    let name = if b0 == 0x7f {
                        "del"
                    } else {
                        CONV_U_NAMES[usize::from(b0)]
                    };
                    pr.fmt[pr.cchar] = b's';
                    let fmt = pr.fmt[..fmt_len].to_vec();
                    self.printf(&fmt, Arg::Str(name.as_bytes()));
                } else {
                    pr.fmt[pr.cchar] = if is_print(b0) { b'c' } else { b'x' };
                    let fmt = pr.fmt[..fmt_len].to_vec();
                    self.printf(&fmt, Arg::Int(u64::from(b0)));
                }
            }
            Kind::Uint => {
                let b = bytes_at(block, bp, pr.bcnt as usize);
                let v: u64 = match pr.bcnt {
                    1 => u64::from(b[0]),
                    2 => u64::from(u16::from_le_bytes([b[0], b[1]])),
                    4 => u64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
                    _ => u64::from_le_bytes(b),
                };
                let fmt = pr.fmt[..fmt_len].to_vec();
                self.printf(&fmt, Arg::Int(v));
            }
        }
        if color.is_some() {
            self.out.extend_from_slice(b"\x1b[0m");
        }
    }

    /// `display`: imprime cada bloco com todos os formatos e, no fim, o endereço final.
    pub(super) fn display(&mut self) {
        let mut fss = std::mem::take(&mut self.fss);
        while self.get() {
            sys::checkpoint();
            let block = std::mem::take(&mut self.curp);
            let saveaddress = self.address;
            for fs in fss.iter_mut() {
                let mut bp = 0usize;
                let mut rem = self.blocksize as i64;
                for fu in fs.fus.iter_mut() {
                    if fu.ignore {
                        break;
                    }
                    let mut cnt = fu.reps;
                    while cnt > 0 && rem >= 0 {
                        for pr in fu.prs.iter_mut() {
                            if self.eaddress != 0
                                && self.address >= self.eaddress
                                && !matches!(pr.kind, Kind::Text | Kind::Bpad)
                            {
                                bpad(pr);
                            }
                            let fmt_len = match pr.nospace {
                                Some(ns) if cnt == 1 => ns,
                                _ => pr.fmt.len(),
                            };
                            self.print_pr(pr, fmt_len, &block, bp);
                            self.address += i64::from(pr.bcnt);
                            rem -= i64::from(pr.bcnt);
                            if rem < 0 {
                                break;
                            }
                            bp += pr.bcnt.max(0) as usize;
                        }
                        cnt -= 1;
                        if cnt % 4096 == 0 {
                            sys::checkpoint();
                        }
                    }
                }
                self.address = saveaddress;
            }
            self.curp = block;
            if self.out.len() >= 4096 {
                self.flush_out();
            }
        }
        if let Some((fsi, fui)) = self.endfu {
            if self.eaddress == 0 {
                if self.address == 0 {
                    self.fss = fss;
                    return;
                }
                self.eaddress = self.address;
            }
            let prs = fss[fsi].fus[fui].prs.clone();
            for pr in &prs {
                let color = if self.colors && pr.colorlist.is_some() {
                    self.color_cond(pr, &[], 0, pr.bcnt)
                } else {
                    None
                };
                if let Some(c) = color {
                    self.out.extend_from_slice(c.as_bytes());
                }
                match pr.kind {
                    Kind::Address => {
                        let fmt = pr.fmt.clone();
                        self.printf(&fmt, Arg::Int(self.eaddress as u64));
                    }
                    Kind::Text => self.out.extend_from_slice(&pr.fmt),
                    _ => {}
                }
                if color.is_some() {
                    self.out.extend_from_slice(b"\x1b[0m");
                }
            }
        }
        self.fss = fss;
    }

    /// `get`: o próximo bloco em `curp`; `false` no fim.
    fn get(&mut self) -> bool {
        let bs = self.blocksize;
        if !self.started {
            self.started = true;
            let mut a = Vec::new();
            let mut b = Vec::new();
            if a.try_reserve_exact(bs).is_err() || b.try_reserve_exact(bs).is_err() {
                let p = self.prog.clone();
                self.flush_out();
                io::eprint(format!(
                    "{p}: cannot allocate {bs} bytes: {}\n",
                    Errno::ENOMEM.message()
                ));
                sys::exit(1);
            }
            a.resize(bs, 0);
            b.resize(bs, 0);
            self.curp = a;
            self.savp = b;
        } else {
            std::mem::swap(&mut self.curp, &mut self.savp);
            self.address += bs as i64;
        }
        let mut need = bs;
        let mut nread = 0usize;
        loop {
            if self.length == 0 || (self.ateof && !self.next()) {
                if need == bs {
                    return false;
                }
                if need == 0 && self.vflag != VFlag::All && self.curp[..nread] == self.savp[..nread]
                {
                    if self.vflag != VFlag::Dup {
                        self.out.extend_from_slice(b"*\n");
                    }
                    return false;
                }
                for b in &mut self.curp[nread..] {
                    *b = 0;
                }
                self.eaddress = self.address + nread as i64;
                return true;
            }
            let Some(fd) = self.input.fd() else {
                self.warn("all input file arguments failed");
                return false;
            };
            let want = if self.length == -1 {
                need
            } else {
                (self.length.max(0) as usize).min(need)
            };
            let (n, err) = fread(fd, &mut self.curp[nread..nread + want]);
            if n == 0 {
                if let Some(e) = err {
                    let name = io::lossy(&self.last_name);
                    self.warn(&format!("{name}: {}", e.message()));
                }
                self.ateof = true;
                continue;
            }
            self.ateof = false;
            if self.length != -1 {
                self.length -= n as i64;
            }
            need -= n;
            if need == 0 {
                if self.vflag == VFlag::All || self.vflag == VFlag::First || self.curp != self.savp
                {
                    if self.vflag == VFlag::Dup || self.vflag == VFlag::First {
                        self.vflag = VFlag::Wait;
                    }
                    return true;
                }
                if self.vflag == VFlag::Wait {
                    self.out.extend_from_slice(b"*\n");
                }
                self.vflag = VFlag::Dup;
                self.address += bs as i64;
                need = bs;
                nread = 0;
                sys::checkpoint();
            } else {
                nread += n;
            }
        }
    }

    /// `next`: abre o próximo arquivo (o `freopen` do stdin) e aplica o `-s`; `false` quando não há
    /// mais entrada.
    fn next(&mut self) -> bool {
        loop {
            let statok;
            let mut name: Vec<u8> = b"stdin".to_vec();
            if self.argi < self.files.len() {
                let f = self.files[self.argi].clone();
                match io::File::open(&f) {
                    Ok(file) => {
                        self.input = Input::File(file);
                        statok = true;
                        self.done = true;
                        name = f;
                    }
                    Err(e) => {
                        self.input = Input::Closed;
                        self.warn(&format!("{}: {}", io::lossy(&f), e.message()));
                        self.exitval = 1;
                        self.argi += 1;
                        continue;
                    }
                }
            } else {
                if self.done {
                    return false;
                }
                self.done = true;
                statok = false;
            }
            if self.skip != 0 {
                self.doskip(&name, statok);
            }
            if self.argi < self.files.len() {
                self.last_name = self.files[self.argi].clone();
                self.argi += 1;
            }
            if self.skip == 0 {
                return true;
            }
        }
    }

    /// `doskip`: pula `-s` bytes; arquivo regular menor que o salto é pulado inteiro.
    fn doskip(&mut self, fname: &[u8], statok: bool) {
        let fd = self.input.fd();
        if statok {
            let st = match fd.map(|fd| sys::current().fstat(fd)) {
                Some(Ok(st)) => st,
                Some(Err(e)) => fatal(self, &format!("{}: {}", io::lossy(fname), e.message())),
                None => fatal(
                    self,
                    &format!("{}: {}", io::lossy(fname), Errno::EBADF.message()),
                ),
            };
            if st.file_type() == FileType::Regular && self.skip > st.size as i64 {
                self.skip -= st.size as i64;
                self.address += st.size as i64;
                return;
            }
        }
        let r = match fd {
            Some(fd) if self.skip >= 0 => {
                sys::current().lseek(fd, self.skip, Whence::Set).map(|_| ())
            }
            Some(_) => Err(Errno::EINVAL),
            None => Err(Errno::EBADF),
        };
        if let Err(e) = r {
            fatal(self, &format!("{}: {}", io::lossy(fname), e.message()));
        }
        self.address += self.skip;
        self.skip = 0;
    }
}
