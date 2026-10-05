//! `pmap` do procps-ng 4.0.4: o mapa de memória de cada processo, de `/proc/<pid>/maps` (formato
//! padrão, `-d`) ou de `/proc/<pid>/smaps` (`-x`), com `-q` (sem cabeçalho nem rodapé), `-p`
//! (caminho inteiro no lugar do nome) e `-A` (só os mapeamentos que tocam o intervalo).
//!
//! Como no original: o processo que não existe não imprime nada e o código de saída vira 42; o
//! processo cujo `maps` não abre também não imprime nada (nem o cabeçalho) e soma 1 ao código.
//! `-X`, `-XX` e os arquivos de configuração (`-c`, `-C`, `-n`, `-N`) não estão portados e caem
//! no erro de opção inválida.

use std::ffi::OsString;

use sysabi::Pid;
use ul_misc::util::getopt::{Getopt, HasArg, LongOpt};
use ul_misc::util::io;

use crate::common::{self, out};
use crate::procfs;

const USAGE: &str = "\nUsage:\n pmap [options] PID [PID ...]\n\nOptions:\n -x, --extended              show details\n -X                          show even more details\n            WARNING: format changes according to /proc/PID/smaps\n -XX                         show everything the kernel provides\n -c, --read-rc               read the default rc\n -C, --read-rc-from=<file>   read the rc from file\n -n, --create-rc             create new default rc\n -N, --create-rc-to=<file>   create new rc to file\n            NOTE: pid arguments are not allowed with -n, -N\n -d, --device                show the device format\n -q, --quiet                 do not display header and footer\n -p, --show-path             show path in the mapping\n -A, --range=<low>[,<high>]  limit results to the given range\n\n -h, --help     display this help and exit\n -V, --version  output version information and exit\n\nFor more details see pmap(1).\n";

const LONGS: &[LongOpt] = &[
    LongOpt::new("extended", HasArg::No, 'x' as i32),
    LongOpt::new("device", HasArg::No, 'd' as i32),
    LongOpt::new("quiet", HasArg::No, 'q' as i32),
    LongOpt::new("show-path", HasArg::No, 'p' as i32),
    LongOpt::new("range", HasArg::Required, 'A' as i32),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

/// Opções que mudam a saída.
#[derive(Clone, Copy, Debug)]
pub struct Opts {
    pub extended: bool,
    pub device: bool,
    pub quiet: bool,
    pub show_path: bool,
    pub range_low: u64,
    pub range_high: u64,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts { extended: false, device: false, quiet: false, show_path: false, range_low: 0, range_high: u64::MAX }
    }
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage_error() -> i32 {
    io::eprint(USAGE);
    1
}

/// `strtoul(s, NULL, 16)`: prefixo `0x` opcional, para no primeiro byte que não é dígito
/// hexadecimal; nada lido é 0.
fn strtoul_hex(s: &str) -> u64 {
    let t = s.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    let mut v: u64 = 0;
    for c in t.chars() {
        let Some(d) = c.to_digit(16) else { break };
        v = v.saturating_mul(16).saturating_add(u64::from(d));
    }
    v
}

/// `range_arguments`: `low`, `low,high`, `low,` ou `,high`, em hexadecimal. Só `low` é o
/// intervalo de um endereço só.
pub fn parse_range(arg: &str) -> (u64, u64) {
    match arg.split_once(',') {
        None => {
            let v = strtoul_hex(arg);
            (v, v)
        }
        Some((lo, hi)) => {
            let low = if lo.is_empty() { 0 } else { strtoul_hex(lo) };
            let high = if hi.is_empty() { u64::MAX } else { strtoul_hex(hi) };
            (low, high)
        }
    }
}

/// `strtoul(walk, &end, 0)` exigindo que tudo seja consumido: base 8 com `0`, 16 com `0x`.
fn strtoul_auto(s: &str) -> Option<u64> {
    let (digits, radix) = if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        (h, 16)
    } else if s.len() > 1 && s.starts_with('0') {
        (&s[1..], 8)
    } else {
        (s, 10)
    };
    if digits.is_empty() {
        return None;
    }
    u64::from_str_radix(digits, radix).ok().or_else(|| digits.chars().all(|c| c.is_digit(radix)).then_some(u64::MAX))
}

/// Os pids dos operandos, como o laço do `main` do original: `/proc/NNNN` vale, `/proc/` seguido
/// de não dígito é ignorado, o resto tem que ser número de 1 a 0x7fffffff. `None` é erro de uso.
pub fn parse_pids(operands: &[String]) -> Option<Vec<Pid>> {
    let mut pids = Vec::new();
    for op in operands {
        let mut walk = op.as_str();
        if let Some(rest) = walk.strip_prefix("/proc/") {
            walk = rest;
            if !walk.starts_with(|c: char| c.is_ascii_digit()) {
                continue;
            }
        }
        if !walk.starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        let pid = strtoul_auto(walk)?;
        if !(1..=0x7fff_ffff).contains(&pid) {
            return None;
        }
        pids.push(pid as Pid);
    }
    Some(pids)
}

/// Uma linha de cabeçalho de mapeamento (`start-end perms offset maj:min inode [nome]`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapLine {
    pub start: u64,
    pub end: u64,
    pub perms: Vec<u8>,
    pub offset: u64,
    pub dev_major: u32,
    pub dev_minor: u32,
    pub inode: u64,
}

fn hex_prefix(s: &[u8]) -> Option<(u64, &[u8])> {
    let n = s.iter().take_while(|b| b.is_ascii_hexdigit()).count();
    if n == 0 {
        return None;
    }
    let v = u64::from_str_radix(std::str::from_utf8(&s[..n]).ok()?, 16).unwrap_or(u64::MAX);
    Some((v, &s[n..]))
}

fn skip_ws(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|b| b.is_ascii_whitespace()).count();
    &s[n..]
}

/// O `sscanf(mapbuf, "%lx-%lx %31s %llx %x:%x %llu", ...)` do original. `None` quando nem o
/// intervalo casou (a linha não é cabeçalho de mapeamento).
pub fn parse_map_line(line: &[u8]) -> Option<MapLine> {
    let (start, rest) = hex_prefix(skip_ws(line))?;
    let rest = rest.strip_prefix(b"-")?;
    let (end, rest) = hex_prefix(rest)?;
    let mut m = MapLine { start, end, ..MapLine::default() };
    let rest = skip_ws(rest);
    let n = rest.iter().take_while(|b| !b.is_ascii_whitespace()).count().min(31);
    if n == 0 {
        return None;
    }
    m.perms = rest[..n].to_vec();
    let rest = skip_ws(&rest[n..]);
    let Some((offset, rest)) = hex_prefix(rest) else { return Some(m) };
    m.offset = offset;
    let rest = skip_ws(rest);
    let Some((maj, rest)) = hex_prefix(rest) else { return Some(m) };
    m.dev_major = maj as u32;
    let Some(rest) = rest.strip_prefix(b":") else { return Some(m) };
    let Some((min, rest)) = hex_prefix(rest) else { return Some(m) };
    m.dev_minor = min as u32;
    let rest = skip_ws(rest);
    let n = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if n > 0 {
        m.inode = std::str::from_utf8(&rest[..n]).ok().and_then(|s| s.parse().ok()).unwrap_or(u64::MAX);
    }
    Some(m)
}

/// `mapping_name`: o nome depois da última `/` da linha (com `-p`, tudo a partir da primeira);
/// sem `/`, `  [ stack ]` quando o início da pilha cai no mapeamento, senão `  [ anon ]`. O
/// segmento de memória compartilhada do SysV só é reconhecido quando o menor do dispositivo do
/// shm é conhecido, e aqui ele não é (o `discover_shm_minor` do original também desiste quando
/// `shmget` falha).
pub fn mapping_name(line: &[u8], start: u64, len: u64, start_stack: u64, show_path: bool) -> Vec<u8> {
    if let Some(last) = line.iter().rposition(|b| *b == b'/') {
        if show_path {
            let first = line.iter().position(|b| *b == b'/').unwrap_or(last);
            return line[first..].to_vec();
        }
        return if last + 1 < line.len() { line[last + 1..].to_vec() } else { line[last..].to_vec() };
    }
    if start_stack >= start && start_stack <= start.wrapping_add(len) {
        b"  [ stack ]".to_vec()
    } else {
        b"  [ anon ]".to_vec()
    }
}

/// O par `chave: número` de uma linha do smaps (`%20[^:]: %llu`).
fn smaps_pair(line: &[u8]) -> Option<(&[u8], u64)> {
    let n = line.iter().take_while(|b| **b != b':').count();
    if n == 0 || n > 20 || n >= line.len() {
        return None;
    }
    let key = &line[..n];
    let rest = skip_ws(&line[n + 1..]);
    let d = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if d == 0 {
        return None;
    }
    let v = std::str::from_utf8(&rest[..d]).ok()?.parse().unwrap_or(u64::MAX);
    Some((key, v))
}

/// Troca os bytes não imprimíveis por `?`, como o laço de `isprint` do original.
fn printable(line: &[u8]) -> Vec<u8> {
    line.iter().map(|b| if (0x20..0x7f).contains(b) { *b } else { b'?' }).collect()
}

/// Totais acumulados de um processo.
#[derive(Default)]
struct Totals {
    shared: u64,
    private_writeable: u64,
    private_readonly: u64,
    rss: u64,
    dirty: u64,
}

/// Contabiliza as permissões como o original e devolve o campo `Mode` de cinco letras.
fn account(perms: &[u8], diff: u64, t: &mut Totals) -> Vec<u8> {
    let mut p = perms.to_vec();
    while p.len() < 5 {
        p.push(0);
    }
    if p[3] == b's' {
        t.shared += diff;
    }
    if p[3] == b'p' {
        p[3] = b'-';
        if p[1] == b'w' {
            t.private_writeable += diff;
        } else {
            t.private_readonly += diff;
        }
    }
    p[4] = b'-';
    p.truncate(5);
    // Um campo de permissões curto (nunca vem do kernel) teria NULs: o printf pararia neles.
    if let Some(z) = p.iter().position(|b| *b == 0) {
        p.truncate(z);
    }
    p
}

/// A saída de um processo; `None` quando o `maps` (ou `smaps`) não abre.
pub fn render(pid: Pid, cmdline: &[u8], start_stack: u64, data: Option<&[u8]>, o: &Opts) -> Option<Vec<u8>> {
    let data = data?;
    let mut s = Vec::new();
    if !o.quiet {
        s.extend_from_slice(format!("{pid}:   ").as_bytes());
        s.extend_from_slice(cmdline);
        s.push(b'\n');
        if o.extended {
            s.extend_from_slice(b"Address           Kbytes     RSS   Dirty Mode  Mapping\n");
        }
        if o.device {
            s.extend_from_slice(b"Address           Kbytes Mode  Offset           Device    Mapping\n");
        }
    }
    let mut t = Totals::default();
    // No `-x`, o mapeamento em curso espera os valores do smaps e sai na linha `Swap:`.
    let mut pending: Option<(u64, u64, Vec<u8>, Vec<u8>)> = None;
    let (mut rss, mut dirty) = (0u64, 0u64);
    for raw in data.split_inclusive(|b| *b == b'\n') {
        let line = raw.strip_suffix(b"\n").unwrap_or(raw);
        if o.extended
            && let Some((key, v)) = smaps_pair(line)
        {
            {
                match key {
                    b"Rss" => {
                        rss = v;
                        t.rss += v;
                    }
                    b"Shared_Dirty" | b"Private_Dirty" => {
                        dirty += v;
                        t.dirty += v;
                    }
                    b"Swap" => {
                        if let Some((start, diff, mode, name)) = pending.take() {
                            s.extend_from_slice(format!("{start:016x} {:7} {rss:7} {dirty:7} ", diff >> 10).as_bytes());
                            s.extend_from_slice(&mode);
                            s.extend_from_slice(b"  ");
                            s.extend_from_slice(&name);
                            s.push(b'\n');
                        }
                        rss = 0;
                        dirty = 0;
                    }
                    _ => {}
                }
                continue;
            }
        }
        let Some(m) = parse_map_line(line) else { continue };
        if m.end.wrapping_sub(1) < o.range_low || m.start > o.range_high {
            continue;
        }
        let clean = printable(line);
        let diff = m.end.wrapping_sub(m.start);
        let mode = account(&m.perms, diff, &mut t);
        let name = mapping_name(&clean, m.start, diff, start_stack, o.show_path);
        if o.extended {
            pending = Some((m.start, diff, mode, name));
            rss = 0;
            dirty = 0;
        } else if o.device {
            s.extend_from_slice(format!("{:016x} {:7} ", m.start, diff >> 10).as_bytes());
            s.extend_from_slice(&mode);
            s.extend_from_slice(format!(" {:016x} {:03x}:{:05x} ", m.offset, m.dev_major, m.dev_minor).as_bytes());
            s.extend_from_slice(&name);
            s.push(b'\n');
        } else {
            s.extend_from_slice(format!("{:016x} {:6}K ", m.start, diff >> 10).as_bytes());
            s.extend_from_slice(&mode);
            s.extend_from_slice(b"   ");
            s.extend_from_slice(&name);
            s.push(b'\n');
        }
    }
    if !o.quiet {
        let mapped = t.shared + t.private_writeable + t.private_readonly;
        if o.extended {
            s.extend_from_slice(b"---------------- ------- ------- ------- \n");
            s.extend_from_slice(format!("total kB         {:7} {:7} {:7}\n", mapped >> 10, t.rss, t.dirty).as_bytes());
        }
        if o.device {
            s.extend_from_slice(
                format!(
                    "mapped: {}K    writeable/private: {}K    shared: {}K\n",
                    mapped >> 10,
                    t.private_writeable >> 10,
                    t.shared >> 10
                )
                .as_bytes(),
            );
        }
        if !o.extended && !o.device {
            s.extend_from_slice(format!(" total {:>16}K\n", mapped >> 10).as_bytes());
        }
    }
    Some(s)
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut o = Opts::default();
    let mut g = Getopt::from_env(&argv[1..], "xdqpA:hV", LONGS);
    while let Some(r) = g.next_opt() {
        match r {
            Err(e) => {
                io::eprint(format!("{}\n{USAGE}", e.message(&argv0)));
                return 1;
            }
            Ok(opt) => match opt.short() {
                Some('x') => o.extended = true,
                Some('d') => o.device = true,
                Some('q') => o.quiet = true,
                Some('p') => o.show_path = true,
                Some('A') => {
                    let (lo, hi) = parse_range(&opt.arg_str());
                    o.range_low = lo;
                    o.range_high = hi;
                }
                Some('h') => {
                    out(USAGE);
                    return 0;
                }
                Some('V') => {
                    out("pmap from procps-ng 4.0.4\n");
                    return 0;
                }
                _ => unreachable!("tabela de opções do pmap"),
            },
        }
    }
    let operands: Vec<String> = g.operands().iter().map(|a| String::from_utf8_lossy(a).into_owned()).collect();
    if operands.is_empty() {
        return usage_error();
    }
    if o.device && o.extended {
        common::warn("pmap", "options -c, -C, -d, -n, -N, -x, -X are mutually exclusive");
        return 1;
    }
    let Some(pids) = parse_pids(&operands) else { return usage_error() };
    let mut ret = 0;
    let mut found = 0usize;
    for pid in &pids {
        sysabi::sys::checkpoint();
        let base = format!("/proc/{pid}");
        let Some(stat) = procfs::read(&format!("{base}/stat")).and_then(|d| procfs::parse_stat(&d)) else { continue };
        found += 1;
        let cmdline = procfs::Proc {
            tgid: *pid,
            tid: *pid,
            stat: stat.clone(),
            cmdline: procfs::read(&format!("{base}/cmdline")).map(|d| procfs::split_nul(&d)),
            ..procfs::Proc::default()
        }
        .cmdline_string();
        let file = if o.extended { "smaps" } else { "maps" };
        let data = procfs::read(&format!("{base}/{file}"));
        match render(*pid, &cmdline, stat.startstack, data.as_deref(), &o) {
            Some(text) => out(text),
            None => ret |= 1,
        }
    }
    if found < pids.len() {
        ret = 42;
    }
    ret
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAPS: &[u8] = b"55d0c0a00000-55d0c0a1c000 r--p 00000000 08:01 1835023                    /usr/bin/bash\n55d0c0a1c000-55d0c0b00000 r-xp 0001c000 08:01 1835023                    /usr/bin/bash\n55d0c1000000-55d0c1021000 rw-p 00000000 00:00 0                          [heap]\n7ffd00000000-7ffd00021000 rw-p 00000000 00:00 0                          [stack]\n";

    #[test]
    fn map_lines() {
        let m = parse_map_line(b"55d0c0a1c000-55d0c0b00000 r-xp 0001c000 08:01 1835023   /usr/bin/bash").unwrap();
        assert_eq!(m.start, 0x55d0_c0a1_c000);
        assert_eq!(m.perms, b"r-xp");
        assert_eq!(m.offset, 0x1c000);
        assert_eq!((m.dev_major, m.dev_minor, m.inode), (8, 1, 1_835_023));
        assert!(parse_map_line(b"Rss:  4 kB").is_none());
    }

    #[test]
    fn default_format() {
        let o = Opts::default();
        let s = render(7, b"bash", 0x7ffd_0001_0000, Some(MAPS), &o).unwrap();
        assert_eq!(
            String::from_utf8(s).unwrap(),
            "7:   bash\n000055d0c0a00000    112K r----   bash\n000055d0c0a1c000    912K r-x--   bash\n000055d0c1000000    132K rw---     [ anon ]\n00007ffd00000000    132K rw---     [ stack ]\n total             1288K\n"
        );
    }

    #[test]
    fn quiet_device_and_path() {
        let o = Opts { quiet: true, show_path: true, ..Opts::default() };
        let s = render(7, b"bash", 0, Some(MAPS), &o).unwrap();
        assert!(String::from_utf8(s).unwrap().starts_with("000055d0c0a00000    112K r----   /usr/bin/bash\n"));
        let o = Opts { device: true, ..Opts::default() };
        let s = String::from_utf8(render(7, b"bash", 0, Some(MAPS), &o).unwrap()).unwrap();
        assert!(s.contains("000055d0c0a1c000     912 r-x-- 000000000001c000 008:00001 bash\n"));
        assert!(s.ends_with("mapped: 1288K    writeable/private: 264K    shared: 0K\n"));
    }

    #[test]
    fn extended_from_smaps() {
        let smaps = b"55d0c0a00000-55d0c0a1c000 r--p 00000000 08:01 1835023 /usr/bin/bash\nSize:                112 kB\nRss:                 100 kB\nShared_Dirty:          0 kB\nPrivate_Dirty:         4 kB\nSwap:                  0 kB\nVmFlags: rd mr mw me\n";
        let o = Opts { extended: true, ..Opts::default() };
        let s = String::from_utf8(render(7, b"bash", 0, Some(smaps), &o).unwrap()).unwrap();
        assert_eq!(
            s,
            "7:   bash\nAddress           Kbytes     RSS   Dirty Mode  Mapping\n000055d0c0a00000     112     100       4 r----  bash\n---------------- ------- ------- ------- \ntotal kB             112     100       4\n"
        );
    }

    #[test]
    fn pids_and_ranges() {
        assert_eq!(parse_pids(&["/proc/12".into(), "0x10".into(), "/proc/self".into()]), Some(vec![12, 16]));
        assert_eq!(parse_pids(&["abc".into()]), None);
        assert_eq!(parse_pids(&["0".into()]), None);
        assert_eq!(parse_range("1000,2000"), (0x1000, 0x2000));
        assert_eq!(parse_range(",ff"), (0, 0xff));
        assert_eq!(parse_range("10"), (0x10, 0x10));
    }

    #[test]
    fn missing_maps() {
        assert!(render(7, b"bash", 0, None, &Opts::default()).is_none());
    }
}
