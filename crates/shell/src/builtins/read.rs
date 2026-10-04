//! `read` e `mapfile`/`readarray`.

use std::time::Duration;

use sysabi::{Errno, Fd, PollEvents, PollFd, Whence};

use super::{opt_error, parse_int, parse_opts};
use crate::shell::{Exec, Flow, Shell, sys, write_fd};
use crate::vars::Value;

/// Leitor de um fd que não consome além do necessário: em fd com seek lê em blocos e devolve o
/// excesso com `lseek`; em pipe e terminal lê um byte por vez (como o bash).
struct Source {
    fd: Fd,
    buf: Vec<u8>,
    pos: usize,
    seekable: bool,
    eof: bool,
    timeout: Option<Duration>,
    timed_out: bool,
}

const BLOCK: usize = 4096;

impl Source {
    fn new(fd: Fd, timeout: Option<Duration>) -> Source {
        let seekable = sys().lseek(fd, 0, Whence::Cur).is_ok() && sys().fstat(fd).is_ok_and(|st| st.file_type() == sysabi::FileType::Regular);
        Source { fd, buf: Vec::new(), pos: 0, seekable, eof: false, timeout, timed_out: false }
    }

    /// Próximo byte; `Ok(None)` no fim; `Err` em erro de leitura.
    fn next(&mut self, sh: &mut Shell) -> Result<Option<u8>, Errno> {
        if self.pos < self.buf.len() {
            let b = self.buf[self.pos];
            self.pos += 1;
            return Ok(Some(b));
        }
        if self.eof || self.timed_out {
            return Ok(None);
        }
        let s = sys();
        if let Some(t) = self.timeout {
            let mut fds = [PollFd { fd: self.fd, events: PollEvents::IN, revents: PollEvents::empty() }];
            match s.poll(&mut fds, Some(t)) {
                Ok(0) => {
                    self.timed_out = true;
                    return Ok(None);
                }
                Ok(_) => {}
                Err(Errno::EINTR) => {
                    let _ = sh.run_pending_traps();
                    return self.next(sh);
                }
                Err(e) => return Err(e),
            }
        }
        let want = if self.seekable { BLOCK } else { 1 };
        self.buf.resize(want, 0);
        self.pos = 0;
        loop {
            match s.read(self.fd, &mut self.buf) {
                Ok(0) => {
                    self.buf.clear();
                    self.eof = true;
                    return Ok(None);
                }
                Ok(n) => {
                    self.buf.truncate(n);
                    self.pos = 1;
                    return Ok(Some(self.buf[0]));
                }
                Err(Errno::EINTR) => {
                    let _ = sh.run_pending_traps();
                }
                Err(e) => {
                    self.buf.clear();
                    return Err(e);
                }
            }
        }
    }

    /// Devolve ao fd o que foi lido a mais.
    fn finish(&mut self) {
        if self.seekable && self.pos < self.buf.len() {
            let extra = (self.buf.len() - self.pos) as i64;
            let _ = sys().lseek(self.fd, -extra, Whence::Cur);
        }
        self.buf.clear();
        self.pos = 0;
    }
}

/// Lê uma linha crua até `delim` (sem incluí-lo); `None` em EOF sem dados.
pub fn read_line(sh: &mut Shell, fd: Fd, delim: u8) -> Option<Vec<u8>> {
    let mut src = Source::new(fd, None);
    let mut line = Vec::new();
    let mut got = false;
    while let Ok(Some(b)) = src.next(sh) {
        got = true;
        if b == delim {
            src.finish();
            return Some(line);
        }
        line.push(b);
    }
    src.finish();
    if got { Some(line) } else { None }
}

fn parse_timeout(v: &[u8]) -> Option<Duration> {
    let s = std::str::from_utf8(v).ok()?;
    if s.is_empty() || s.starts_with('-') {
        return None;
    }
    let (int, frac) = match s.split_once('.') {
        Some((a, b)) => (a, b),
        None => (s, ""),
    };
    if !int.bytes().all(|c| c.is_ascii_digit()) || !frac.bytes().all(|c| c.is_ascii_digit()) || (int.is_empty() && frac.is_empty()) {
        return None;
    }
    let secs: u64 = if int.is_empty() { 0 } else { int.parse().ok()? };
    let mut nanos: u64 = 0;
    let mut scale = 100_000_000;
    for c in frac.bytes().take(9) {
        nanos += (c - b'0') as u64 * scale;
        scale /= 10;
    }
    Some(Duration::new(secs, nanos as u32))
}

/// Bytes lidos com marca de "escapado" (protegido da divisão por IFS).
struct Marked {
    bytes: Vec<u8>,
    escaped: Vec<bool>,
}

fn utf8_needed(first: u8) -> usize {
    match first {
        0xC2..=0xDF => 1,
        0xE0..=0xEF => 2,
        0xF0..=0xF4 => 3,
        _ => 0,
    }
}

pub fn read(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let opts = match parse_opts(argv, "ersa:d:i:n:N:p:t:u:", false) {
        Ok(o) => o,
        Err(e) => {
            return Ok(opt_error(sh, "read", e, "read [-ers] [-a array] [-d delim] [-i text] [-n nchars] [-N nchars] [-p prompt] [-t timeout] [-u fd] [name ...]"));
        }
    };
    let raw = opts.has(b'r');
    let delim: u8 = match opts.value(b'd') {
        Some(d) => d.first().copied().unwrap_or(0),
        None => b'\n',
    };
    let exact = opts.value(b'N').is_some();
    let nchars: Option<usize> = match opts.value(b'N').or(opts.value(b'n')) {
        Some(v) => match parse_int(v) {
            Some(n) if n >= 0 => Some(n as usize),
            _ => {
                sh.builtin_error("read", format!("{}: invalid number", String::from_utf8_lossy(v)));
                return Ok(1);
            }
        },
        None => None,
    };
    let timeout = match opts.value(b't') {
        Some(v) => match parse_timeout(v) {
            Some(t) => Some(t),
            None => {
                sh.builtin_error("read", format!("{}: invalid timeout specification", String::from_utf8_lossy(v)));
                return Ok(1);
            }
        },
        None => sh.get_scalar("TMOUT").and_then(|v| parse_timeout(&v)).filter(|d| !d.is_zero()),
    };
    let fd = match opts.value(b'u') {
        Some(v) => match parse_int(v) {
            Some(n) if n >= 0 && sys().fstat(Fd(n as i32)).is_ok() => Fd(n as i32),
            Some(_) | None => {
                let shown = String::from_utf8_lossy(v);
                if parse_int(v).is_some() {
                    sh.builtin_error("read", format!("{shown}: invalid file descriptor: Bad file descriptor"));
                } else {
                    sh.builtin_error("read", format!("{shown}: invalid file descriptor specification"));
                }
                return Ok(1);
            }
        },
        None => Fd::STDIN,
    };
    let array = opts.value(b'a').map(|v| String::from_utf8_lossy(v).into_owned());
    let names: Vec<String> = argv[opts.rest..].iter().map(|n| String::from_utf8_lossy(n).into_owned()).collect();
    for n in names.iter().chain(array.iter()) {
        if !crate::word::is_name(n.as_bytes()) {
            sh.builtin_error("read", format!("`{n}': not a valid identifier"));
            return Ok(1);
        }
    }
    if let Some(p) = opts.value(b'p') {
        if sys().isatty(fd) {
            let _ = write_fd(Fd::STDERR, p);
        }
    }
    // `-t 0`: só diz se há dado.
    if timeout == Some(Duration::ZERO) {
        let mut fds = [PollFd { fd, events: PollEvents::IN, revents: PollEvents::empty() }];
        return Ok(match sys().poll(&mut fds, Some(Duration::ZERO)) {
            Ok(n) if n > 0 => 0,
            _ => 1,
        });
    }

    let mut src = Source::new(fd, timeout);
    let mut m = Marked { bytes: Vec::new(), escaped: Vec::new() };
    let mut status = 0;
    let mut chars = 0usize;
    let mut pending_utf8 = 0usize;
    let utf8 = sh.utf8();
    loop {
        if let Some(n) = nchars {
            if chars >= n && pending_utf8 == 0 {
                break;
            }
        }
        let b = match src.next(sh) {
            Ok(Some(b)) => b,
            Ok(None) => {
                status = if src.timed_out { 128 + 14 } else { 1 };
                break;
            }
            Err(e) => {
                sh.builtin_error("read", format!("read error: {}: {}", fd.0, e.message()));
                status = 1;
                break;
            }
        };
        if pending_utf8 > 0 {
            if (0x80..=0xBF).contains(&b) {
                pending_utf8 -= 1;
                m.bytes.push(b);
                m.escaped.push(false);
                continue;
            }
            pending_utf8 = 0;
        }
        if !exact && b == delim {
            break;
        }
        if !raw && b == b'\\' {
            match src.next(sh) {
                Ok(Some(b'\n')) => {
                    if exact {
                        m.bytes.push(b'\n');
                        m.escaped.push(true);
                        chars += 1;
                    }
                    continue;
                }
                Ok(Some(c)) => {
                    m.bytes.push(c);
                    m.escaped.push(true);
                    chars += 1;
                    continue;
                }
                _ => {
                    status = 1;
                    break;
                }
            }
        }
        m.bytes.push(b);
        m.escaped.push(false);
        chars += 1;
        if utf8 {
            pending_utf8 = utf8_needed(b);
        }
    }
    src.finish();

    let ifs = sh.ifs();
    let is_ws = |c: u8| c == b' ' || c == b'\t' || c == b'\n';
    let delim_at = |i: usize| !m.escaped[i] && ifs.contains(&m.bytes[i]);
    let ws_at = |i: usize| delim_at(i) && is_ws(m.bytes[i]);
    let text = |a: usize, b: usize| m.bytes[a..b].to_vec();

    // Divide em campos respeitando os bytes escapados.
    let split_all = |start: usize, end: usize| -> Vec<(usize, usize)> {
        let mut fields = Vec::new();
        let mut i = start;
        while i < end && ws_at(i) {
            i += 1;
        }
        while i < end {
            let a = i;
            while i < end && !delim_at(i) {
                i += 1;
            }
            fields.push((a, i));
            if i >= end {
                break;
            }
            if ws_at(i) {
                while i < end && ws_at(i) {
                    i += 1;
                }
                if i < end && delim_at(i) && !ws_at(i) {
                    i += 1;
                    while i < end && ws_at(i) {
                        i += 1;
                    }
                }
            } else {
                i += 1;
                while i < end && ws_at(i) {
                    i += 1;
                }
            }
        }
        fields
    };
    let n = m.bytes.len();

    if let Some(arr) = array {
        let fields = if ifs.is_empty() { if n > 0 { vec![(0, n)] } else { Vec::new() } } else { split_all(0, n) };
        let items: Vec<(Option<Vec<u8>>, bool, Vec<u8>)> = fields.into_iter().map(|(a, b)| (None, false, text(a, b))).collect();
        sh.unset_var(&arr);
        if !sh.assign_array(&arr, &items, false)? {
            return Ok(1);
        }
        return Ok(status);
    }
    if names.is_empty() {
        sh.assign_scalar("REPLY", m.bytes.clone(), false)?;
        return Ok(status);
    }
    // Uma variável por campo; a última leva o resto.
    let mut i = 0;
    while i < n && ws_at(i) {
        i += 1;
    }
    for (k, name) in names.iter().enumerate() {
        let last = k + 1 == names.len();
        if ifs.is_empty() {
            let v = if k == 0 { text(0, n) } else { Vec::new() };
            assign(sh, name, v)?;
            continue;
        }
        if last {
            let rest = split_all(i, n);
            let v = if rest.len() <= 1 {
                rest.first().map(|(a, b)| text(*a, *b)).unwrap_or_default()
            } else {
                let mut end = n;
                while end > i && ws_at(end - 1) {
                    end -= 1;
                }
                text(i, end)
            };
            assign(sh, name, v)?;
            break;
        }
        let a = i;
        while i < n && !delim_at(i) {
            i += 1;
        }
        let v = text(a, i);
        if i < n {
            if ws_at(i) {
                while i < n && ws_at(i) {
                    i += 1;
                }
                if i < n && delim_at(i) && !ws_at(i) {
                    i += 1;
                    while i < n && ws_at(i) {
                        i += 1;
                    }
                }
            } else {
                i += 1;
                while i < n && ws_at(i) {
                    i += 1;
                }
            }
        }
        assign(sh, name, v)?;
    }
    Ok(status)
}

fn assign(sh: &mut Shell, name: &str, v: Vec<u8>) -> Result<(), Flow> {
    sh.assign_scalar(name, v, false)?;
    Ok(())
}

pub fn mapfile(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let builtin = String::from_utf8_lossy(&argv[0]).into_owned();
    let opts = match parse_opts(argv, "d:n:O:s:tu:C:c:", false) {
        Ok(o) => o,
        Err(e) => {
            return Ok(opt_error(sh, &builtin, e, "mapfile [-d delim] [-n count] [-O origin] [-s count] [-t] [-u fd] [-C callback] [-c quantum] [array]"));
        }
    };
    let delim = match opts.value(b'd') {
        Some(d) => d.first().copied().unwrap_or(0),
        None => b'\n',
    };
    let num = |c: u8| -> Result<Option<i64>, String> {
        match opts.value(c) {
            Some(v) => match parse_int(v) {
                Some(n) if n >= 0 => Ok(Some(n)),
                _ => Err(String::from_utf8_lossy(v).into_owned()),
            },
            None => Ok(None),
        }
    };
    let (count, origin, skip, quantum) = match (num(b'n'), num(b'O'), num(b's'), num(b'c')) {
        (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d),
        (Err(v), ..) | (_, Err(v), ..) | (_, _, Err(v), _) | (.., Err(v)) => {
            sh.builtin_error(&builtin, format!("{v}: invalid number"));
            return Ok(1);
        }
    };
    let fd = match opts.value(b'u') {
        Some(v) => match parse_int(v) {
            Some(n) if n >= 0 && sys().fstat(Fd(n as i32)).is_ok() => Fd(n as i32),
            _ => {
                sh.builtin_error(&builtin, format!("{}: invalid file descriptor: Bad file descriptor", String::from_utf8_lossy(v)));
                return Ok(1);
            }
        },
        None => Fd::STDIN,
    };
    let name = argv.get(opts.rest).map(|v| String::from_utf8_lossy(v).into_owned()).unwrap_or_else(|| "MAPFILE".to_string());
    if !crate::word::is_name(name.as_bytes()) {
        sh.builtin_error(&builtin, format!("`{name}': not a valid identifier"));
        return Ok(1);
    }
    let callback = opts.value(b'C').map(|v| String::from_utf8_lossy(v).into_owned());
    let quantum = quantum.unwrap_or(5000).max(1);
    let strip = opts.has(b't');
    let mut idx = origin.unwrap_or(0);
    if origin.is_none() {
        sh.unset_var(&name);
    }
    // Garante array indexado.
    {
        let v = sh.vars.entry(&name);
        if !matches!(v.value, Value::Indexed(_)) {
            v.value = Value::Indexed(Default::default());
        }
        v.attrs.set(crate::vars::Attrs::INDEXED);
    }
    let mut src = Source::new(fd, None);
    let mut skipped = 0;
    let mut read = 0;
    loop {
        if let Some(c) = count {
            if c > 0 && read >= c {
                break;
            }
        }
        let mut line = Vec::new();
        let mut got = false;
        let mut ended = false;
        loop {
            match src.next(sh) {
                Ok(Some(b)) => {
                    got = true;
                    if b == delim {
                        if !strip {
                            line.push(b);
                        }
                        ended = true;
                        break;
                    }
                    line.push(b);
                }
                _ => break,
            }
        }
        if !got {
            break;
        }
        let _ = ended;
        if (skipped as i64) < skip.unwrap_or(0) {
            skipped += 1;
            continue;
        }
        sh.assign_element(&name, idx.to_string().as_bytes(), line.clone(), false)?;
        read += 1;
        if let Some(cb) = &callback {
            if read % quantum == 0 {
                let q = |b: &[u8]| String::from_utf8_lossy(&crate::quote::printf_q(b, true)).into_owned();
                let cmd = format!("{cb} {idx} {}", q(&line));
                let line_no = sh.lineno;
                sh.run_text(&cmd, crate::exec::TextKind::Eval, std::sync::Arc::from("mapfile"), line_no)?;
            }
        }
        idx += 1;
    }
    src.finish();
    Ok(0)
}
