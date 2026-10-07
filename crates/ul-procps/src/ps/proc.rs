//! Leitura de processos como a libproc2 (readproc.c, devname.c, wchan.c, namespace.c, pwcache.c):
//! `/proc/<pid>/stat` e `status` são lidos na hora da varredura, o resto (cmdline, environ, io,
//! smaps, cgroup...) só quando uma coluna ou ordenação pede. A falta de um arquivo deixa o campo no
//! valor padrão do original.

use std::cell::OnceCell;

use sysabi::{Fd, sys};

use super::Ps;
use super::util::escape_str_lib;
use crate::common::{dev_major, dev_minor};

/// `MAX_BUFSZ` da readproc.c.
pub const MAX_BUFSZ: usize = 1024 * 64 * 2;
/// `P_G_SZ` do pwcache: nomes com este tamanho ou mais viram número.
pub const P_G_SZ: usize = 33;

/// Lê um arquivo do `/proc` inteiro.
pub fn read_path(path: &str) -> Option<Vec<u8>> {
    sys::read_file(path.as_bytes()).ok()
}

/// Um processo ou uma thread (`proc_t` da libproc2).
#[derive(Default)]
pub struct Pt {
    pub base: String,
    pub tgid: i32,
    pub tid: i32,
    pub is_proc: bool,
    pub euid: u32,
    pub egid: u32,
    pub ruid: u32,
    pub suid: u32,
    pub fuid: u32,
    pub rgid: u32,
    pub sgid: u32,
    pub fgid: u32,
    pub state: u8,
    pub ppid: i32,
    pub pgrp: i32,
    pub session: i32,
    pub tty: i32,
    pub tpgid: i32,
    pub flags: u64,
    pub min_flt: u64,
    pub cmin_flt: u64,
    pub maj_flt: u64,
    pub cmaj_flt: u64,
    pub utime: u64,
    pub stime: u64,
    pub cutime: u64,
    pub cstime: u64,
    pub priority: i32,
    pub nice: i32,
    pub nlwp: i32,
    pub start_time: u64,
    pub vsize: u64,
    pub rss: u64,
    pub rss_rlim: u64,
    pub start_code: u64,
    pub end_code: u64,
    pub start_stack: u64,
    pub kstk_esp: u64,
    pub kstk_eip: u64,
    pub exit_signal: i32,
    pub processor: i32,
    pub rtprio: i32,
    pub sched: i32,
    pub cmd: Vec<u8>,
    pub has_cmd: bool,
    pub vm_data: u64,
    pub vm_exe: u64,
    pub vm_lock: u64,
    pub vm_lib: u64,
    pub vm_rss: u64,
    pub vm_size: u64,
    pub vm_stack: u64,
    pub vm_swap: u64,
    pub vm_rss_anon: u64,
    pub vm_rss_file: u64,
    pub vm_rss_shared: u64,
    pub signal: Vec<u8>,
    pub blocked: Vec<u8>,
    pub sigcatch: Vec<u8>,
    pub sigignore: Vec<u8>,
    pub sigpnd: Vec<u8>,
    pub supgid: Option<Vec<u8>>,
    cmdline_c: OnceCell<Vec<u8>>,
    environ_c: OnceCell<Vec<u8>>,
    exe_c: OnceCell<Vec<u8>>,
    cgroup_c: OnceCell<(Vec<u8>, Vec<u8>)>,
    lxc_c: OnceCell<Vec<u8>>,
    io_c: OnceCell<[u64; 7]>,
    smaps_c: OnceCell<[u64; 20]>,
    statm_c: OnceCell<[u64; 7]>,
    ns_c: OnceCell<[u64; 8]>,
    oom_c: OnceCell<(i32, i32)>,
    autogrp_c: OnceCell<(i32, i32)>,
    luid_c: OnceCell<i32>,
    wchan_c: OnceCell<Vec<u8>>,
}

// ---------------------------------------------------------------------------------------------
// Conversões no estilo sscanf/strtol.

/// `%lu`/`%llu` do sscanf: sinal opcional e dígitos; negativo dá a volta como o strtoul.
pub fn scan_u(t: Option<&&[u8]>) -> Option<u64> {
    let t = *t?;
    let (neg, digits) = match t.first()? {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    let end = digits.iter().position(|b| !b.is_ascii_digit()).unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    let mut v: u64 = 0;
    for b in &digits[..end] {
        v = v.saturating_mul(10).saturating_add(u64::from(b - b'0'));
    }
    Some(if neg { v.wrapping_neg() } else { v })
}

/// `%d` do sscanf.
pub fn scan_i(t: Option<&&[u8]>) -> Option<i32> {
    let t = *t?;
    let (neg, digits) = match t.first()? {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    let end = digits.iter().position(|b| !b.is_ascii_digit()).unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    let mut v: i64 = 0;
    for b in &digits[..end] {
        v = v.saturating_mul(10).saturating_add(i64::from(b - b'0'));
    }
    let v = if neg { -v } else { v };
    Some(v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
}

/// `strtol(s, &s, 10)` sobre um pedaço de linha: pula espaço, lê o número e devolve o resto.
fn strtol_prefix(s: &[u8]) -> (i64, &[u8]) {
    let c = ul_common::ctype::strtol(s, 10);
    if c.used == 0 { (0, s) } else { (c.value, &s[c.used..]) }
}

// ---------------------------------------------------------------------------------------------
// stat e status.

fn stat2proc(data: &[u8], p: &mut Pt) {
    p.processor = 0;
    p.rtprio = -1;
    p.sched = -1;
    p.nlwp = 0;
    let Some(open) = data.iter().position(|b| *b == b'(') else { return };
    let rest = &data[open + 1..];
    let Some(close) = rest.iter().rposition(|b| *b == b')') else { return };
    // `!tmp[1]`: nada depois do ')'.
    if close + 1 >= rest.len() {
        return;
    }
    if !p.has_cmd {
        let mut raw = rest[..close].to_vec();
        raw.truncate(63);
        if let Some(z) = raw.iter().position(|b| *b == 0) {
            raw.truncate(z);
        }
        p.cmd = escape_str_lib(&raw, 64);
        p.has_cmd = true;
    }
    // Pula ") ".
    let tail = if close + 2 <= rest.len() { &rest[close + 2..] } else { &rest[rest.len()..] };
    let toks: Vec<&[u8]> = tail.split(|b| b.is_ascii_whitespace()).filter(|t| !t.is_empty()).collect();
    let mut it = toks.iter();
    macro_rules! next {
        () => {
            it.next()
        };
    }
    let Some(st) = next!() else { return };
    p.state = st[0];
    macro_rules! geti {
        ($f:expr) => {
            match scan_i(next!()) {
                Some(v) => $f = v,
                None => return fin(p),
            }
        };
    }
    macro_rules! getu {
        ($f:expr) => {
            match scan_u(next!()) {
                Some(v) => $f = v,
                None => return fin(p),
            }
        };
    }
    geti!(p.ppid);
    geti!(p.pgrp);
    geti!(p.session);
    geti!(p.tty);
    geti!(p.tpgid);
    getu!(p.flags);
    getu!(p.min_flt);
    getu!(p.cmin_flt);
    getu!(p.maj_flt);
    getu!(p.cmaj_flt);
    getu!(p.utime);
    getu!(p.stime);
    getu!(p.cutime);
    getu!(p.cstime);
    geti!(p.priority);
    geti!(p.nice);
    geti!(p.nlwp);
    // alarm: lido e descartado.
    if scan_u(next!()).is_none() {
        return fin(p);
    }
    getu!(p.start_time);
    getu!(p.vsize);
    getu!(p.rss);
    getu!(p.rss_rlim);
    getu!(p.start_code);
    getu!(p.end_code);
    getu!(p.start_stack);
    getu!(p.kstk_esp);
    getu!(p.kstk_eip);
    // pending, blocked, sigign, sigcatch: descartados.
    for _ in 0..4 {
        if next!().is_none() {
            return fin(p);
        }
    }
    // wchan: lido e descartado.
    if scan_u(next!()).is_none() {
        return fin(p);
    }
    // nswap, cnswap.
    for _ in 0..2 {
        if next!().is_none() {
            return fin(p);
        }
    }
    geti!(p.exit_signal);
    geti!(p.processor);
    geti!(p.rtprio);
    geti!(p.sched);
    fin(p);

    fn fin(p: &mut Pt) {
        if p.nlwp == 0 {
            p.nlwp = 1;
        }
    }
}

/// Os 16 caracteres de uma máscara de sinais.
fn sig16(v: &[u8]) -> Vec<u8> {
    v.iter().take(16).take_while(|b| **b != b'\n').copied().collect()
}

fn status2proc(data: &[u8], p: &mut Pt, is_proc: bool) {
    let mut threads: i64 = 0;
    let mut tgid: i64 = 0;
    let mut pid: i64 = 0;
    let mut shd_pnd: Vec<u8> = Vec::new();
    for line in data.split(|b| *b == b'\n') {
        if line.len() < 4 {
            break;
        }
        let Some(colon) = line.iter().position(|b| *b == b':') else { break };
        if line.get(colon + 1) != Some(&b'\t') {
            break;
        }
        let key = &line[..colon];
        let val = &line[colon + 2..];
        match key {
            b"Name" => {
                if !p.has_cmd {
                    let mut raw: Vec<u8> = Vec::new();
                    let mut i = 0;
                    while i < val.len() && raw.len() < 63 {
                        let mut c = val[i];
                        i += 1;
                        if c == b'\\' {
                            if i >= val.len() {
                                break;
                            }
                            c = val[i];
                            i += 1;
                            if c == b'n' {
                                c = b'\n';
                            }
                        }
                        raw.push(c);
                    }
                    p.cmd = escape_str_lib(&raw, 64);
                    p.has_cmd = true;
                }
            }
            b"ShdPnd" => shd_pnd = sig16(val),
            b"SigBlk" => p.blocked = sig16(val),
            b"SigCgt" => p.sigcatch = sig16(val),
            b"SigIgn" => p.sigignore = sig16(val),
            b"SigPnd" => p.sigpnd = sig16(val),
            b"State" => p.state = val.first().copied().unwrap_or(0),
            b"Tgid" => tgid = strtol_prefix(val).0,
            b"Pid" => pid = strtol_prefix(val).0,
            b"PPid" => p.ppid = strtol_prefix(val).0 as i32,
            b"Threads" => threads = strtol_prefix(val).0,
            b"Uid" => {
                let (a, r) = strtol_prefix(val);
                let (b, r) = strtol_prefix(r);
                let (c, r) = strtol_prefix(r);
                let (d, _) = strtol_prefix(r);
                p.ruid = a as u32;
                p.euid = b as u32;
                p.suid = c as u32;
                p.fuid = d as u32;
            }
            b"Gid" => {
                let (a, r) = strtol_prefix(val);
                let (b, r) = strtol_prefix(r);
                let (c, r) = strtol_prefix(r);
                let (d, _) = strtol_prefix(r);
                p.rgid = a as u32;
                p.egid = b as u32;
                p.sgid = c as u32;
                p.fgid = d as u32;
            }
            b"VmData" => p.vm_data = strtol_prefix(val).0 as u64,
            b"VmExe" => p.vm_exe = strtol_prefix(val).0 as u64,
            b"VmLck" => p.vm_lock = strtol_prefix(val).0 as u64,
            b"VmLib" => p.vm_lib = strtol_prefix(val).0 as u64,
            b"VmRSS" => p.vm_rss = strtol_prefix(val).0 as u64,
            b"VmSize" => p.vm_size = strtol_prefix(val).0 as u64,
            b"VmStk" => p.vm_stack = strtol_prefix(val).0 as u64,
            b"VmSwap" => p.vm_swap = strtol_prefix(val).0 as u64,
            b"RssAnon" => p.vm_rss_anon = strtol_prefix(val).0 as u64,
            b"RssFile" => p.vm_rss_file = strtol_prefix(val).0 as u64,
            b"RssShmem" => p.vm_rss_shared = strtol_prefix(val).0 as u64,
            b"Groups" => {
                let mut s = 0;
                while s < val.len() && (val[s] == b' ' || val[s] == b'\t') {
                    s += 1;
                }
                if s < val.len() {
                    let mut g = val[s..].to_vec();
                    let mut j = g.len() - 1;
                    if g[j] != b' ' {
                        j += 1;
                    }
                    g.truncate(j);
                    for b in g.iter_mut().skip(1) {
                        if *b == b' ' {
                            *b = b',';
                        }
                    }
                    p.supgid = Some(g);
                }
            }
            _ => {}
        }
    }
    p.signal = if is_proc && !shd_pnd.is_empty() { shd_pnd } else { p.sigpnd.clone() };
    if threads != 0 {
        p.nlwp = threads as i32;
        p.tgid = tgid as i32;
        p.tid = pid as i32;
    } else {
        p.nlwp = 1;
        p.tgid = pid as i32;
        p.tid = pid as i32;
    }
    if p.supgid.is_none() {
        p.supgid = Some(b"-".to_vec());
    }
}

/// `simple_readproc` e `simple_readtask`: lê o diretório `path` (um processo, ou uma thread dentro
/// de `task/`). `None` se o diretório sumiu ou o `stat` não pôde ser lido.
pub fn load_pt(path: &str, tgid: i32, tid: i32, is_proc: bool) -> Option<Pt> {
    let st = sys::stat(path.as_bytes()).ok()?;
    let mut p = Pt { base: path.to_string(), tgid, tid, is_proc, euid: st.uid, egid: st.gid, ..Pt::default() };
    let stat = read_path(&format!("{path}/stat")).filter(|d| !d.is_empty())?;
    stat2proc(&stat, &mut p);
    if let Some(s) = read_path(&format!("{path}/status")).filter(|d| !d.is_empty()) {
        status2proc(&s, &mut p, is_proc);
    }
    Some(p)
}

/// Ids numéricos de um diretório (`/proc`, `/proc/<pid>/task`), na ordem em que o procfs lista.
pub fn dir_ids(dir: &str) -> Option<Vec<i32>> {
    let entries = sys::read_dir(dir.as_bytes()).ok()?;
    let mut ids: Vec<i32> = entries
        .iter()
        .filter(|e| matches!(e.name.first(), Some(b'1'..=b'9')))
        .filter_map(|e| {
            let digits: Vec<u8> = e.name.iter().copied().take_while(u8::is_ascii_digit).collect();
            std::str::from_utf8(&digits).ok()?.parse::<u32>().ok().map(|v| v as i32)
        })
        .collect();
    ids.sort_unstable();
    ids.dedup();
    Some(ids)
}

/// Varre os processos (`PIDS_FETCH_TASKS_ONLY`) ou, com `threads`, também as threads
/// (`PIDS_FETCH_THREADS_TOO`: uma linha por thread, cada processo com as suas).
pub fn reap(threads: bool) -> Vec<Pt> {
    let hide_kernel = sys::getenv("LIBPROC_HIDE_KERNEL").is_some();
    let mut out = Vec::new();
    let Some(ids) = dir_ids("/proc") else { return out };
    for (n, pid) in ids.into_iter().enumerate() {
        if n % 64 == 0 {
            sys::checkpoint();
        }
        push_proc(&mut out, pid, pid, threads, hide_kernel);
    }
    out
}

fn push_proc(out: &mut Vec<Pt>, tgid: i32, tid: i32, threads: bool, hide_kernel: bool) {
    if !threads {
        if let Some(p) = load_pt(&format!("/proc/{tgid}"), tgid, tid, true)
            && !(hide_kernel && (p.ppid == 2 || p.tid == 2)) {
                out.push(p);
            }
        return;
    }
    let Some(tids) = dir_ids(&format!("/proc/{tgid}/task")) else { return };
    for t in tids {
        if let Some(p) = load_pt(&format!("/proc/{tgid}/task/{t}"), tgid, t, false)
            && !(hide_kernel && (p.ppid == 2 || p.tid == 2)) {
                out.push(p);
            }
    }
}

/// `procps_pids_select` com lista de pids: só esses, na ordem dada, cada um com o Tgid do status.
pub fn select_pids(pids: &[u32], threads: bool) -> Vec<Pt> {
    let hide_kernel = sys::getenv("LIBPROC_HIDE_KERNEL").is_some();
    let mut out = Vec::new();
    for &pid in pids {
        let pid = pid as i32;
        let mut tgid = pid;
        if let Some(s) = read_path(&format!("/proc/{pid}/status"))
            && let Some(i) = find_sub(&s, b"Tgid:") {
                tgid = strtol_prefix(&s[i + 5..]).0 as i32;
            }
        if threads {
            push_proc(&mut out, tgid, pid, true, hide_kernel);
        } else if let Some(p) = load_pt(&format!("/proc/{pid}"), tgid, pid, true)
            && !(hide_kernel && (p.ppid == 2 || p.tid == 2)) {
                out.push(p);
            }
    }
    out
}

fn find_sub(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

// ---------------------------------------------------------------------------------------------
// Campos lidos sob demanda.

/// `read_unvectored`: o arquivo inteiro (até `MAX_BUFSZ - 1` bytes), NUL e `\n` viram `sep`, zeros
/// do fim somem e um espaço no último byte lido também. Devolve (texto até o primeiro NUL, bytes
/// lidos).
fn read_unvectored(path: &str, sep: u8) -> (Vec<u8>, usize) {
    let Some(data) = read_path(path) else { return (Vec::new(), 0) };
    let mut dst = data;
    let mut n = dst.len();
    if n >= MAX_BUFSZ {
        n = MAX_BUFSZ - 1;
        dst.truncate(n);
    }
    if n > 0 {
        let mut i = n;
        while i > 0 && dst[i - 1] == 0 {
            i -= 1;
        }
        while i > 0 {
            i -= 1;
            if dst[i] == b'\n' || dst[i] == 0 {
                dst[i] = sep;
            }
        }
        if dst[n - 1] == b' ' {
            dst[n - 1] = 0;
        }
    }
    let end = dst.iter().position(|b| *b == 0).unwrap_or(dst.len());
    dst.truncate(end);
    (dst, n)
}

impl Pt {
    /// `escape_command` com colchetes e `<defunct>`: o `[comm]` dos processos sem cmdline.
    fn bracketed_cmd(&self) -> Vec<u8> {
        let mut s = vec![b'['];
        s.extend(escape_str_lib(&self.cmd, MAX_BUFSZ));
        s.push(b']');
        if self.state == b'Z' {
            s.extend_from_slice(b" <defunct>");
        }
        s
    }

    /// `PIDS_CMDLINE`: argumentos separados por espaço, ou `[comm]` sem argumentos.
    pub fn cmdline(&self) -> &[u8] {
        self.cmdline_c.get_or_init(|| {
            let (text, n) = read_unvectored(&format!("{}/cmdline", self.base), b' ');
            let v = if n > 0 { escape_str_lib(&text, MAX_BUFSZ) } else { self.bracketed_cmd() };
            if v.is_empty() { b"?".to_vec() } else { v }
        })
    }

    /// `PIDS_ENVIRON`: variáveis separadas por espaço, ou `-`.
    pub fn environ(&self) -> &[u8] {
        self.environ_c.get_or_init(|| {
            let (text, n) = read_unvectored(&format!("{}/environ", self.base), b' ');
            let v = if n > 0 { escape_str_lib(&text, MAX_BUFSZ) } else { Vec::new() };
            if v.is_empty() { b"-".to_vec() } else { v }
        })
    }

    /// `PIDS_EXE`: o alvo de `/proc/<pid>/exe`, ou `-`.
    pub fn exe(&self) -> &[u8] {
        self.exe_c.get_or_init(|| match sys::current().readlinkat(Fd::CWD, format!("{}/exe", self.base).as_bytes()) {
            Ok(t) if !t.is_empty() => escape_str_lib(&t[..t.len().min(MAX_BUFSZ - 1)], MAX_BUFSZ),
            _ => b"-".to_vec(),
        })
    }

    fn cgroups(&self) -> &(Vec<u8>, Vec<u8>) {
        self.cgroup_c.get_or_init(|| {
            let mut dst: Vec<u8> = Vec::new();
            // Um grupo por linha; os que terminam em '/' (cgroup raiz vazio) são pulados.
            if let Some(raw) = read_path(&format!("{}/cgroup", self.base)) {
                for line in raw.split(|b| *b == b'\n') {
                    if line.is_empty() || line.last() == Some(&b'/') {
                        continue;
                    }
                    if !dst.is_empty() {
                        dst.push(b',');
                    }
                    dst.extend(escape_str_lib(line, MAX_BUFSZ));
                }
            }
            let cg = if dst.is_empty() { b"-".to_vec() } else { dst };
            let name = match find_sub(&cg, b":name=") {
                Some(i) if i + 6 < cg.len() => cg[i + 6..].to_vec(),
                _ => cg.clone(),
            };
            (cg, name)
        })
    }

    pub fn cgroup(&self) -> &[u8] {
        &self.cgroups().0
    }

    pub fn cgname(&self) -> &[u8] {
        &self.cgroups().1
    }

    /// Nome do contêiner lxc do cgroup, ou `-`.
    pub fn lxcname(&self) -> &[u8] {
        self.lxc_c.get_or_init(|| {
            let Some(raw) = read_path(&format!("{}/cgroup", self.base)).filter(|d| !d.is_empty()) else {
                return b"-".to_vec();
            };
            for delim in [&b"lxc.payload."[..], &b"lxc.payload/"[..], &b"lxc/"[..]] {
                if let Some(i) = find_sub(&raw, delim) {
                    let line_end = raw[i..].iter().position(|b| *b == b'\n').map_or(raw.len(), |e| i + e);
                    let line = &raw[i..line_end];
                    // Contêineres aninhados: vale a última ocorrência do delimitador na linha.
                    let mut start = delim.len();
                    while let Some(j) = find_sub(&line[start..], delim) {
                        start = start + j + delim.len();
                    }
                    let name = &line[start..];
                    let name = match name.iter().position(|b| *b == b'/') {
                        Some(k) => &name[..k],
                        None => name,
                    };
                    return name.to_vec();
                }
            }
            b"-".to_vec()
        })
    }

    /// `/proc/<pid>/loginuid`, ou -1.
    pub fn luid(&self) -> i32 {
        *self.luid_c.get_or_init(|| match read_path(&format!("{}/loginuid", self.base)) {
            Some(d) if !d.is_empty() => strtol_prefix(&d).0 as i32,
            _ => -1,
        })
    }

    /// (id, nice) do autogroup, ou (-1, 0).
    pub fn autogroup(&self) -> (i32, i32) {
        *self.autogrp_c.get_or_init(|| {
            let mut out = (-1, 0);
            if let Some(d) = read_path(&format!("{}/autogroup", self.base)) {
                let s = String::from_utf8_lossy(&d).into_owned();
                // "/autogroup-%d nice %d"
                if let Some(rest) = s.strip_prefix("/autogroup-") {
                    let mut it = rest.split_whitespace();
                    if let Some(id) = it.next().and_then(|v| v.parse::<i32>().ok()) {
                        out.0 = id;
                        if it.next() == Some("nice")
                            && let Some(n) = it.next().and_then(|v| v.parse::<i32>().ok()) {
                                out.1 = n;
                            }
                    }
                }
            }
            out
        })
    }

    /// rchar, wchar, syscr, syscw, read_bytes, write_bytes, cancelled_write_bytes.
    pub fn io(&self) -> [u64; 7] {
        *self.io_c.get_or_init(|| {
            let mut v = [0u64; 7];
            if let Some(d) = read_path(&format!("{}/io", self.base)) {
                let text = String::from_utf8_lossy(&d).into_owned();
                let labels = ["rchar:", "wchar:", "syscr:", "syscw:", "read_bytes:", "write_bytes:", "cancelled_write_bytes:"];
                let mut it = text.split_whitespace();
                for (i, l) in labels.iter().enumerate() {
                    if it.next() != Some(l) {
                        break;
                    }
                    match it.next().and_then(|x| x.parse::<u64>().ok()) {
                        Some(x) => v[i] = x,
                        None => break,
                    }
                }
            }
            v
        })
    }

    /// Todos os campos do smaps_rollup, na ordem: Rss, Pss, Pss_Anon, Pss_File, Pss_Shmem,
    /// Shared_Clean, Shared_Dirty, Private_Clean, Private_Dirty, Referenced, Anonymous, LazyFree,
    /// AnonHugePages, ShmemPmdMapped, FilePmdMapped, Shared_Hugetlb, Private_Hugetlb, Swap, SwapPss,
    /// Locked (em kB).
    pub fn smaps_all(&self) -> [u64; 20] {
        *self.smaps_c.get_or_init(|| {
            let Some(d) = read_path(&format!("{}/smaps_rollup", self.base)) else { return [0; 20] };
            const ITEMS: [&str; 20] = [
                "Rss:", "Pss:", "Pss_Anon:", "Pss_File:", "Pss_Shmem:", "Shared_Clean:", "Shared_Dirty:", "Private_Clean:",
                "Private_Dirty:", "Referenced:", "Anonymous:", "LazyFree:", "AnonHugePages:", "ShmemPmdMapped:",
                "FilePmdMapped:", "Shared_Hugetlb:", "Private_Hugetlb:", "Swap:", "SwapPss:", "Locked:",
            ];
            let mut vals = [0u64; 20];
            let mut s: &[u8] = &d;
            for (i, item) in ITEMS.iter().enumerate() {
                let Some(at) = find_sub(s, item.as_bytes()) else { continue };
                let head = &s[at + item.len()..];
                let (v, rest) = strtol_prefix(head);
                vals[i] = v as u64;
                s = rest;
            }
            vals
        })
    }

    /// (Pss, Private_Clean + Private_Dirty) do smaps_rollup.
    pub fn smaps(&self) -> (u64, u64) {
        let v = self.smaps_all();
        (v[1], v[7] + v[8])
    }

    /// Os sete números do statm, em páginas: size, resident, share, trs, lrs, drs, dt (os que o
    /// arquivo não traz ficam em 0, como o `sscanf` parcial do original).
    pub fn statm_all(&self) -> [u64; 7] {
        *self.statm_c.get_or_init(|| {
            let mut out = [0u64; 7];
            if let Some(d) = read_path(&format!("{}/statm", self.base)) {
                let v: Vec<u64> = String::from_utf8_lossy(&d).split_whitespace().take(7).map_while(|x| x.parse().ok()).collect();
                for (i, x) in v.into_iter().enumerate() {
                    out[i] = x;
                }
            }
            out
        })
    }

    /// (resident, share) do statm, em páginas.
    pub fn statm(&self) -> (u64, u64) {
        let v = self.statm_all();
        (v[1], v[2])
    }

    /// Números de inode dos namespaces: cgroup, ipc, mnt, net, pid, time, user, uts.
    pub fn ns(&self) -> [u64; 8] {
        *self.ns_c.get_or_init(|| {
            let mut v = [0u64; 8];
            for (i, n) in ["cgroup", "ipc", "mnt", "net", "pid", "time", "user", "uts"].iter().enumerate() {
                if let Ok(st) = sys::stat(format!("/proc/{}/ns/{}", self.tid, n).as_bytes()) {
                    v[i] = st.ino;
                }
            }
            v
        })
    }

    /// (oom_score, oom_score_adj).
    pub fn oom(&self) -> (i32, i32) {
        *self.oom_c.get_or_init(|| {
            let rd = |name: &str| -> i32 {
                read_path(&format!("{}/{name}", self.base)).filter(|d| !d.is_empty()).map_or(0, |d| strtol_prefix(&d).0 as i32)
            };
            (rd("oom_score"), rd("oom_score_adj"))
        })
    }

    /// `lookup_wchan`: o símbolo do `/proc/<tid>/wchan`, `-` se `0`, `?` se não lê.
    pub fn wchan_name(&self) -> &[u8] {
        self.wchan_c.get_or_init(|| {
            let Some(mut buf) = read_path(&format!("/proc/{}/wchan", self.tid)) else { return b"?".to_vec() };
            buf.truncate(63);
            if buf.is_empty() {
                return b"?".to_vec();
            }
            if buf == b"0" {
                return b"-".to_vec();
            }
            let mut s: &[u8] = &buf;
            if s.first() == Some(&b'.') {
                s = &s[1..];
            }
            while s.first() == Some(&b'_') {
                s = &s[1..];
            }
            s.to_vec()
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Nome do terminal (devname.c).

/// Uma linha do `/proc/tty/drivers`.
#[derive(Clone)]
pub struct TtyMapNode {
    devfs_type: bool,
    major: u32,
    minor_first: u32,
    minor_last: u32,
    name: String,
}

fn load_drivers() -> Vec<TtyMapNode> {
    let mut list: Vec<TtyMapNode> = Vec::new();
    let Some(mut buf) = read_path("/proc/tty/drivers") else { return list };
    buf.truncate(9999);
    let mut pos = 0usize;
    while let Some(at) = find_sub(&buf[pos..], b" /dev/") {
        let mut p = pos + at + 6;
        let Some(end_rel) = buf[p..].iter().position(|b| *b == b' ') else {
            pos = p;
            continue;
        };
        let mut end = p + end_rel;
        let mut len = end - p;
        let mut devfs = false;
        if len >= 3 && &buf[end - 2..end] == b"%d" {
            len -= 2;
            devfs = true;
        }
        if len >= 16 {
            len = 15;
        }
        let name = String::from_utf8_lossy(&buf[p..p + len]).into_owned();
        p = end;
        while p < buf.len() && buf[p] == b' ' {
            p += 1;
        }
        let ds = p;
        while p < buf.len() && buf[p].is_ascii_digit() {
            p += 1;
        }
        let major: u32 = String::from_utf8_lossy(&buf[ds..p]).parse().unwrap_or(0);
        while p < buf.len() && buf[p] == b' ' {
            p += 1;
        }
        // "%u-%u" ou "%u".
        let a_start = p;
        while p < buf.len() && buf[p].is_ascii_digit() {
            p += 1;
        }
        let a = String::from_utf8_lossy(&buf[a_start..p]).parse::<u32>().ok();
        let mut b: Option<u32> = None;
        if a.is_some() && p < buf.len() && buf[p] == b'-' {
            let b_start = p + 1;
            let mut q = b_start;
            while q < buf.len() && buf[q].is_ascii_digit() {
                q += 1;
            }
            b = String::from_utf8_lossy(&buf[b_start..q]).parse::<u32>().ok();
        }
        end = p;
        if let Some(first) = a {
            list.insert(0, TtyMapNode { devfs_type: devfs, major, minor_first: first, minor_last: b.unwrap_or(first), name });
        }
        pos = end.max(pos + at + 6);
    }
    list
}

fn rdev_matches(path: &str, maj: u32, min: u32) -> bool {
    match sys::stat(path.as_bytes()) {
        Ok(st) => dev_minor(st.rdev) == u64::from(min) && dev_major(st.rdev) == u64::from(maj),
        Err(_) => false,
    }
}

fn driver_name(ps: &mut Ps, maj: u32, min: u32) -> Option<String> {
    if ps.tty_map.is_none() {
        ps.tty_map = Some(load_drivers());
    }
    let tmn = ps.tty_map.as_ref()?.iter().find(|t| t.major == maj && t.minor_first <= min && t.minor_last >= min)?.clone();
    let mut buf = format!("/dev/{}{}", tmn.name, min);
    if sys::stat(buf.as_bytes()).is_err() {
        buf = format!("/dev/{}/{}", tmn.name, min);
        if sys::stat(buf.as_bytes()).is_err() {
            if tmn.devfs_type {
                return None;
            }
            buf = format!("/dev/{}", tmn.name);
            sys::stat(buf.as_bytes()).ok()?;
        }
    }
    rdev_matches(&buf, maj, min).then_some(buf)
}

const LOW_DENSITY: &[&str] = &[
    "LU0", "LU1", "LU2", "LU3", "FB0", "SA0", "SA1", "SA2", "SC0", "SC1", "SC2", "SC3", "FW0", "FW1", "FW2", "FW3", "AM0",
    "AM1", "AM2", "AM3", "AM4", "AM5", "AM6", "AM7", "AM8", "AM9", "AM10", "AM11", "AM12", "AM13", "AM14", "AM15", "DB0",
    "DB1", "DB2", "DB3", "DB4", "DB5", "DB6", "DB7", "SG0", "SMX0", "SMX1", "SMX2", "MM0", "MM1", "CPM0", "CPM1", "CPM2",
    "CPM3", "IOC0", "IOC1", "IOC2", "IOC3", "IOC4", "IOC5", "IOC6", "IOC7", "IOC8", "IOC9", "IOC10", "IOC11", "IOC12",
    "IOC13", "IOC14", "IOC15", "IOC16", "IOC17", "IOC18", "IOC19", "IOC20", "IOC21", "IOC22", "IOC23", "IOC24", "IOC25",
    "IOC26", "IOC27", "IOC28", "IOC29", "IOC30", "IOC31", "VR0", "VR1", "IOC84", "IOC85", "IOC86", "IOC87", "IOC88",
    "IOC89", "IOC90", "IOC91", "IOC92", "IOC93", "IOC94", "IOC95", "IOC96", "IOC97", "IOC98", "IOC99", "IOC100", "IOC101",
    "IOC102", "IOC103", "IOC104", "IOC105", "IOC106", "IOC107", "IOC108", "IOC109", "IOC110", "IOC111", "IOC112", "IOC113",
    "IOC114", "IOC115", "SIOC0", "SIOC1", "SIOC2", "SIOC3", "SIOC4", "SIOC5", "SIOC6", "SIOC7", "SIOC8", "SIOC9", "SIOC10",
    "SIOC11", "SIOC12", "SIOC13", "SIOC14", "SIOC15", "SIOC16", "SIOC17", "SIOC18", "SIOC19", "SIOC20", "SIOC21", "SIOC22",
    "SIOC23", "SIOC24", "SIOC25", "SIOC26", "SIOC27", "SIOC28", "SIOC29", "SIOC30", "SIOC31", "PSC0", "PSC1", "PSC2",
    "PSC3", "PSC4", "PSC5", "AT0", "AT1", "AT2", "AT3", "AT4", "AT5", "AT6", "AT7", "AT8", "AT9", "AT10", "AT11", "AT12",
    "AT13", "AT14", "AT15", "NX0", "NX1", "NX2", "NX3", "NX4", "NX5", "NX6", "NX7", "NX8", "NX9", "NX10", "NX11", "NX12",
    "NX13", "NX14", "NX15", "J0", "UL0", "UL1", "UL2", "UL3", "xvc0", "PZ0", "PZ1", "PZ2", "PZ3", "TX0", "TX1", "TX2",
    "TX3", "TX4", "TX5", "TX6", "TX7", "SC0", "SC1", "SC2", "SC3", "MAX0", "MAX1", "MAX2", "MAX3",
];

fn guess_name(maj: u32, min: u32) -> Option<String> {
    let buf = match maj {
        3 => {
            if min > 255 {
                return None;
            }
            let t0 = b"pqrstuvwxyzabcde"[(min >> 4) as usize] as char;
            let t1 = b"0123456789abcdef"[(min & 0x0f) as usize] as char;
            format!("/dev/tty{t0}{t1}")
        }
        4 => {
            if min < 64 {
                format!("/dev/tty{min}")
            } else {
                format!("/dev/ttyS{}", min - 64)
            }
        }
        11 => format!("/dev/ttyB{min}"),
        17 => format!("/dev/ttyH{min}"),
        19 => format!("/dev/ttyC{min}"),
        22 | 23 => format!("/dev/ttyD{min}"),
        24 => format!("/dev/ttyE{min}"),
        32 => format!("/dev/ttyX{min}"),
        43 => format!("/dev/ttyI{min}"),
        46 => format!("/dev/ttyR{min}"),
        48 => format!("/dev/ttyL{min}"),
        57 => format!("/dev/ttyP{min}"),
        71 => format!("/dev/ttyF{min}"),
        75 => format!("/dev/ttyW{min}"),
        78 | 112 => format!("/dev/ttyM{min}"),
        105 => format!("/dev/ttyV{min}"),
        136..=143 => format!("/dev/pts/{}", min + (maj - 136) * 256),
        148 => format!("/dev/ttyT{min}"),
        154 => format!("/dev/ttySR{min}"),
        156 => format!("/dev/ttySR{}", min + 256),
        164 => format!("/dev/ttyCH{min}"),
        166 => format!("/dev/ttyACM{min}"),
        172 => format!("/dev/ttyMX{min}"),
        174 => format!("/dev/ttySI{min}"),
        188 => format!("/dev/ttyUSB{min}"),
        204 => {
            let n = LOW_DENSITY.get(min as usize)?;
            format!("/dev/tty{n}")
        }
        208 => format!("/dev/ttyU{min}"),
        216 => format!("/dev/ttyUB{min}"),
        224 => format!("/dev/ttyY{min}"),
        227 => format!("/dev/3270/tty{min}"),
        229 => format!("/dev/iseries/vtty{min}"),
        256 => format!("/dev/ttyEQ{min}"),
        _ => return None,
    };
    rdev_matches(&buf, maj, min).then_some(buf)
}

fn link_name(maj: u32, min: u32, pid: i32, name: &str) -> Option<String> {
    let path = format!("/proc/{pid}/{name}");
    let t = sys::current().readlinkat(Fd::CWD, path.as_bytes()).ok()?;
    if t.is_empty() || t.len() >= 127 {
        return None;
    }
    let target = String::from_utf8_lossy(&t).into_owned();
    rdev_matches(&target, maj, min).then_some(target)
}

/// `dev_to_tty`: nome do terminal de `dev` (o `tty_nr` do stat) para o processo `pid`. `abbrev_all`
/// tira também `tty` e `pts/` (a coluna `tty4`); sempre tira `/dev/`. Sem terminal, `?`.
pub fn dev_to_tty(ps: &mut Ps, dev_in: i32, pid: i32, abbrev_all: bool) -> Vec<u8> {
    let dev = dev_in as u32;
    if dev == 0 {
        return b"?".to_vec();
    }
    let (maj, min) = (dev_major(u64::from(dev)) as u32, dev_minor(u64::from(dev)) as u32);
    let found = driver_name(ps, maj, min)
        .or_else(|| link_name(maj, min, pid, "fd/2"))
        .or_else(|| guess_name(maj, min))
        .or_else(|| link_name(maj, min, pid, "fd/255"));
    let Some(full) = found else { return b"?".to_vec() };
    let mut s: &str = &full;
    if let Some(r) = s.strip_prefix("/dev/")
        && !r.is_empty() {
            s = r;
        }
    if abbrev_all {
        if let Some(r) = s.strip_prefix("tty")
            && !r.is_empty() {
                s = r;
            }
        if let Some(r) = s.strip_prefix("pts/")
            && !r.is_empty() {
                s = r;
            }
    }
    s.bytes().take(63).map(|c| if c <= b' ' || c > 126 { b'?' } else { c }).collect()
}

impl Ps {
    /// `pwcache_get_user`: nome do usuário, ou o número se não existe ou o nome é longo demais.
    pub fn user_name(&mut self, uid: u32) -> Vec<u8> {
        match self.names.user(uid) {
            Some(n) if n.len() < P_G_SZ => n.into_bytes(),
            _ => uid.to_string().into_bytes(),
        }
    }

    /// `pwcache_get_group`.
    pub fn group_name(&mut self, gid: u32) -> Vec<u8> {
        match self.names.group(gid) {
            Some(n) if n.len() < P_G_SZ => n.into_bytes(),
            _ => gid.to_string().into_bytes(),
        }
    }
}
