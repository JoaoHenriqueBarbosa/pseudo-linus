//! `infocmp` do ncurses 6.5.20250216 (`infocmp.c`): decompila uma descrição de terminal do banco
//! compilado, ou compara duas. A leitura de arquivos-fonte terminfo (`-F`) e o `-Q` (formato
//! compilado em hexadecimal) dependem do compilador `tic` e não existem aqui.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use super::dump::{CmpKind, Dump, FAIL, OutForm, PredFn, SortMode, dump_predicate, repair_acsc};
use super::expand::tic_expand;
use super::terminfo::{
    Str, TGETENT_ERR, TGETENT_NO, TermType, db_dirs, read_entry, read_file_entry,
};
use super::{
    BOOLCOUNT, BOOLWRITE, Kind, NUMCOUNT, NUMWRITE, STRCOUNT, STRS, STRWRITE, VERSION, rootname,
    strtol,
};
use crate::util::io;
use crate::util::{Getopt, GetoptError};

const MAX_STRING: usize = 1024;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Compare {
    Default,
    Difference,
    Common,
    Nand,
    UseAll,
}

/// `capcmp`: 0 quando as duas cadeias são iguais (sem o preenchimento, com `-p`).
fn capcmp(idx: usize, s: &Str, t: &Str, ignorepads: bool) -> i32 {
    match (s.val(), t.val()) {
        (None, None) => i32::from(s != t),
        (Some(a), Some(b)) => {
            if idx == super::str_index("acs_chars") || !ignorepads {
                i32::from(a != b)
            } else {
                capcmp_pad(a, b)
            }
        }
        _ => 1,
    }
}

/// `_nc_capcmp`: compara duas cadeias ignorando os `$<...>`.
fn capcmp_pad(s: &[u8], t: &[u8]) -> i32 {
    let skip = |v: &[u8], mut p: usize| -> usize {
        if v.get(p) == Some(&b'$') && v.get(p + 1) == Some(&b'<') {
            p += 2;
            while p < v.len()
                && (v[p].is_ascii_digit()
                    || v[p] == b'.'
                    || v[p] == b'*'
                    || v[p] == b'/'
                    || v[p] == b'>')
            {
                p += 1;
            }
        }
        p
    };
    let (mut i, mut j) = (0usize, 0usize);
    loop {
        i = skip(s, i);
        j = skip(t, j);
        let (a, b) = (
            s.get(i).copied().unwrap_or(0),
            t.get(j).copied().unwrap_or(0),
        );
        if a == 0 && b == 0 {
            return 0;
        }
        if a != b {
            return i32::from(b) - i32::from(a);
        }
        i += 1;
        j += 1;
    }
}

struct Cfg {
    limited: bool,
    quiet: bool,
    literal: bool,
    bool_sep: &'static str,
    s_absent: &'static str,
    s_cancel: &'static str,
    mwidth: i32,
    mheight: i32,
    numbers: i32,
    outform: OutForm,
    ignorepads: bool,
    compare: Compare,
    user_definable: bool,
    itrace: u32,
}

impl Cfg {
    fn same_markers(&self) -> bool {
        self.s_absent == self.s_cancel
    }

    fn no_boolean(&self, v: i8) -> bool {
        if self.same_markers() {
            (v as u8) > 1
        } else {
            v == -1
        }
    }

    fn no_numeric(&self, v: i32) -> bool {
        if self.same_markers() { v < 0 } else { v == -1 }
    }

    fn no_string(&self, v: &Str) -> bool {
        if self.same_markers() {
            !v.valid()
        } else {
            *v == Str::Absent
        }
    }

    fn dump_boolean(&self, v: i8) -> &'static str {
        match v {
            -1 => self.s_absent,
            -2 => self.s_cancel,
            0 => "F",
            1 => "T",
            _ => "?",
        }
    }

    fn dump_numeric(&self, v: i32) -> String {
        match v {
            -1 => self.s_absent.to_string(),
            -2 => self.s_cancel.to_string(),
            _ => v.to_string(),
        }
    }

    fn dump_string(&self, v: &Str) -> Vec<u8> {
        match v {
            Str::Absent => self.s_absent.as_bytes().to_vec(),
            Str::Cancelled => self.s_cancel.as_bytes().to_vec(),
            Str::Val(s) => {
                let mut e = tic_expand(s, self.outform == OutForm::Terminfo, self.numbers);
                e.truncate(MAX_STRING - 3);
                let mut out = b"'".to_vec();
                out.extend_from_slice(&e);
                out.push(b'\'');
                out
            }
        }
    }

    fn tic_expand(&self, s: &Str) -> Vec<u8> {
        match s {
            Str::Val(v) => tic_expand(v, self.outform == OutForm::Terminfo, self.numbers),
            _ => Vec::new(),
        }
    }
}

/// `use_predicate`: o que mostrar num `infocmp -u` (o que difere da união dos `use`).
fn use_predicate(
    rest: &[TermType],
    ignorepads: bool,
    tt: &TermType,
    kind: Kind,
    idx: usize,
) -> i32 {
    match kind {
        Kind::Bool => {
            if idx < tt.bools.len() {
                let mut is_set = false;
                for ep in rest {
                    if idx < ep.bools.len() {
                        is_set = ep.bools[idx] != 0;
                        if is_set {
                            break;
                        }
                    }
                }
                if i8::from(is_set) != tt.bools[idx] {
                    return i32::from(!is_set);
                }
            }
            FAIL
        }
        Kind::Num => {
            if idx < tt.nums.len() {
                let mut value = super::ABSENT_NUMERIC;
                for ep in rest {
                    if idx < ep.nums.len() && ep.nums[idx] >= 0 {
                        value = ep.nums[idx];
                        break;
                    }
                }
                if value != tt.nums[idx] {
                    return i32::from(value != super::ABSENT_NUMERIC);
                }
            }
            FAIL
        }
        Kind::Str => {
            let termstr = &tt.strs[idx];
            let mut usestr = Str::Absent;
            if idx < tt.strs.len() {
                for ep in rest {
                    if idx < ep.strs.len() && ep.strs[idx] != Str::Absent {
                        usestr = ep.strs[idx].clone();
                        break;
                    }
                }
                if usestr == Str::Cancelled && *termstr == Str::Absent {
                    return FAIL;
                } else if usestr == Str::Cancelled && *termstr == Str::Cancelled {
                    return 1;
                } else if usestr == Str::Absent && *termstr == Str::Absent {
                    return FAIL;
                } else if usestr == Str::Absent
                    || *termstr == Str::Absent
                    || capcmp(idx, &usestr, termstr, ignorepads) != 0
                {
                    return 1;
                }
            }
            FAIL
        }
    }
}

/// `show_comparing`.
fn show_comparing(cfg: &Cfg, progname: &str, names: &[Vec<u8>]) {
    if cfg.itrace != 0 {
        match cfg.compare {
            Compare::Difference | Compare::Nand => {
                io::eprint(format!("{progname}: dumping differences\n"))
            }
            Compare::Common => io::eprint(format!("{progname}: dumping common capabilities\n")),
            _ => {}
        }
    }
    if let Some(first) = names.first() {
        let mut o = io::stdout();
        let _ = write!(o, "comparing {}", io::lossy(first));
        if let Some(second) = names.get(1) {
            let _ = write!(o, " to {}", io::lossy(second));
            for n in &names[2..] {
                let _ = write!(o, ", {}", io::lossy(n));
            }
        }
        let _ = writeln!(o, ".");
    }
}

/// O predicado de comparação (`compare_predicate`) sobre as entradas lidas.
fn compare_predicate(cfg: &Cfg, entries: &[TermType], kind: CmpKind, idx: usize, name: &[u8]) {
    let mut o = io::stdout();
    let name_s = io::lossy(name);
    let e1 = &entries[0];
    let skip_user = |limit: usize| !cfg.user_definable && idx > limit;
    match kind {
        CmpKind::Boolean => {
            if skip_user(BOOLWRITE) {
                return;
            }
            let b1 = e1.bools[idx];
            match cfg.compare {
                Compare::Difference => {
                    let b2 = entries[1].bools[idx];
                    if !(cfg.no_boolean(b1) && cfg.no_boolean(b2)) && b1 != b2 {
                        let _ = writeln!(
                            o,
                            "\t{}: {}{}{}.",
                            name_s,
                            cfg.dump_boolean(b1),
                            cfg.bool_sep,
                            cfg.dump_boolean(b2)
                        );
                    }
                }
                Compare::Common => {
                    if b1 != -1 {
                        let found = entries[1..].iter().all(|e| e.bools[idx] == b1);
                        if found {
                            let _ = writeln!(o, "\t{}= {}.", name_s, cfg.dump_boolean(b1));
                        }
                    }
                }
                Compare::Nand if b1 == -1 => {
                    let found = entries[1..].iter().all(|e| e.bools[idx] == b1);
                    if found {
                        let _ = writeln!(o, "\t!{name_s}.");
                    }
                }
                _ => {}
            }
        }
        CmpKind::Number => {
            if skip_user(NUMWRITE) {
                return;
            }
            let n1 = e1.nums[idx];
            match cfg.compare {
                Compare::Difference => {
                    let n2 = entries[1].nums[idx];
                    if !(cfg.no_numeric(n1) && cfg.no_numeric(n2)) && n1 != n2 {
                        let _ = writeln!(
                            o,
                            "\t{}: {}, {}.",
                            name_s,
                            cfg.dump_numeric(n1),
                            cfg.dump_numeric(n2)
                        );
                    }
                }
                Compare::Common => {
                    if n1 != super::ABSENT_NUMERIC {
                        let found = entries[1..].iter().all(|e| e.nums[idx] == n1);
                        if found {
                            let _ = writeln!(o, "\t{}= {}.", name_s, cfg.dump_numeric(n1));
                        }
                    }
                }
                Compare::Nand if n1 == super::ABSENT_NUMERIC => {
                    let found = entries[1..].iter().all(|e| e.nums[idx] == n1);
                    if found {
                        let _ = writeln!(o, "\t!{name_s}.");
                    }
                }
                _ => {}
            }
        }
        CmpKind::String => {
            if skip_user(STRWRITE) {
                return;
            }
            let s1 = &e1.strs[idx];
            match cfg.compare {
                Compare::Difference => {
                    let s2 = &entries[1].strs[idx];
                    if !(cfg.no_string(s1) && cfg.no_string(s2))
                        && capcmp(idx, s1, s2, cfg.ignorepads) != 0
                    {
                        let b1 = cfg.dump_string(s1);
                        let b2 = cfg.dump_string(s2);
                        if b1 != b2 {
                            let _ = o.write_all(format!("\t{name_s}: ").as_bytes());
                            let _ = o.write_all(&b1);
                            let _ = o.write_all(b", ");
                            let _ = o.write_all(&b2);
                            let _ = o.write_all(b".\n");
                        }
                    }
                }
                Compare::Common => {
                    if *s1 != Str::Absent {
                        let found = entries[1..]
                            .iter()
                            .all(|e| capcmp(idx, s1, &e.strs[idx], cfg.ignorepads) == 0);
                        if found {
                            let _ = o.write_all(format!("\t{name_s}= '").as_bytes());
                            let _ = o.write_all(&cfg.tic_expand(s1));
                            let _ = o.write_all(b"'.\n");
                        }
                    }
                }
                Compare::Nand if *s1 == Str::Absent => {
                    let found = entries[1..].iter().all(|e| e.strs[idx] == *s1);
                    if found {
                        let _ = writeln!(o, "\t!{name_s}.");
                    }
                }
                _ => {}
            }
        }
        CmpKind::Use => {
            // As entradas compiladas não têm `use`: só o `-n` mostra algo.
            if cfg.compare == Compare::Nand {
                let _ = writeln!(o, "\t!use.");
            }
        }
    }
}

// ---- análise das cadeias de inicialização (`-i`) ----

const STD_CAPS: &[(&[u8], &str)] = &[
    (b"\x1bc", "RIS"),
    (b"\x1b7", "SC"),
    (b"\x1b8", "RC"),
    (b"\x1b[r", "RSR"),
    (b"\x1b[m", "SGR0"),
    (b"\x1b[2J", "ED2"),
    (b"\x1b(0", "ISO DEC G0"),
    (b"\x1b(A", "ISO UK G0"),
    (b"\x1b(B", "ISO US G0"),
    (b"\x1b)0", "ISO DEC G1"),
    (b"\x1b)A", "ISO UK G1"),
    (b"\x1b)B", "ISO US G1"),
    (b"\x1b=", "DECPAM"),
    (b"\x1b>", "DECPNM"),
    (b"\x1b<", "DECANSI"),
    (b"\x1b[!p", "DECSTR"),
    (b"\x1b F", "S7C1T"),
];

const STD_MODES: &[(&str, &str)] = &[("2", "AM"), ("4", "IRM"), ("12", "SRM"), ("20", "LNM")];

const PRIVATE_MODES: &[(&str, &str)] = &[
    ("1", "CKM"),
    ("2", "ANM"),
    ("3", "COLM"),
    ("4", "SCLM"),
    ("5", "SCNM"),
    ("6", "OM"),
    ("7", "AWM"),
    ("8", "ARM"),
];

const ECMA_HIGHLIGHTS: &[(&str, &str)] = &[
    ("0", "NORMAL"),
    ("1", "+BOLD"),
    ("2", "+DIM"),
    ("3", "+ITALIC"),
    ("4", "+UNDERLINE"),
    ("5", "+BLINK"),
    ("6", "+FASTBLINK"),
    ("7", "+REVERSE"),
    ("8", "+INVISIBLE"),
    ("9", "+DELETED"),
    ("10", "MAIN-FONT"),
    ("11", "ALT-FONT-1"),
    ("12", "ALT-FONT-2"),
    ("13", "ALT-FONT-3"),
    ("14", "ALT-FONT-4"),
    ("15", "ALT-FONT-5"),
    ("16", "ALT-FONT-6"),
    ("17", "ALT-FONT-7"),
    ("18", "ALT-FONT-1"),
    ("19", "ALT-FONT-1"),
    ("20", "FRAKTUR"),
    ("21", "DOUBLEUNDER"),
    ("22", "-DIM"),
    ("23", "-ITALIC"),
    ("24", "-UNDERLINE"),
    ("25", "-BLINK"),
    ("26", "-FASTBLINK"),
    ("27", "-REVERSE"),
    ("28", "-INVISIBLE"),
    ("29", "-DELETED"),
];

fn skip_csi(cap: &[u8]) -> usize {
    let g = |i: usize| cap.get(i).copied().unwrap_or(0);
    if g(0) == 0x1b && g(1) == b'[' {
        2
    } else if g(0) == 0x9b {
        1
    } else {
        0
    }
}

fn same_param(table: &[u8], param: &[u8], length: usize) -> bool {
    param.len() >= length
        && table[..length] == param[..length]
        && !param.get(length).is_some_and(u8::is_ascii_digit)
}

/// `lookup_params`: traduz uma lista `a;b;c` pela tabela (os nomes que não casam ficam como estão).
fn lookup_params(table: &[(&str, &str)], dst: &mut Vec<u8>, src: &[u8]) -> bool {
    let tokens: Vec<&[u8]> = src
        .split(|b| *b == b';')
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return false;
    }
    for ep in tokens {
        let mut found = false;
        for (from, to) in table {
            if same_param(from.as_bytes(), ep, from.len()) {
                dst.extend_from_slice(to.as_bytes());
                found = true;
                break;
            }
        }
        if !found {
            dst.extend_from_slice(ep);
        }
        dst.push(b';');
    }
    dst.pop();
    true
}

fn span_digits_semi(s: &[u8]) -> usize {
    s.iter()
        .take_while(|b| b.is_ascii_digit() || **b == b';')
        .count()
}

fn analyze_string(cfg: &Cfg, name: &str, cap_idx: usize, tp: &TermType) {
    let Str::Val(cap) = &tp.strs[cap_idx] else {
        return;
    };
    let mut o = io::stdout();
    let _ = write!(o, "{name}: ");
    let tp_lines = tp.nums[2];
    let mut sp = 0usize;
    let at = |i: usize| cap.get(i).copied().unwrap_or(0);
    while sp < cap.len() {
        let mut len: usize = 0;
        let mut expansion: Option<Vec<u8>> = None;
        let rest = &cap[sp..];
        // primeiro, as outras capacidades desta entrada (menos as teclas de função)
        for (i, (def, value)) in STRS.iter().zip(tp.strs.iter()).enumerate().take(STRCOUNT) {
            let nm = def.info;
            if nm.starts_with("kf") {
                continue;
            }
            if let Str::Val(cp) = value
                && !cp.is_empty()
                && i != cap_idx
            {
                len = cp.len();
                let mut buf2: Vec<u8> = rest.iter().copied().take(len).collect();
                buf2.truncate(len);
                if capcmp_pad(cp, &buf2) != 0 {
                    continue;
                }
                let isrs = |s: &str| s.starts_with("is") || s.starts_with("rs");
                if (isrs(name) || isrs(nm)) && cap_idx < i {
                    continue;
                }
                expansion = Some(nm.as_bytes().to_vec());
                break;
            }
        }
        // depois as capacidades padrão
        if expansion.is_none() {
            let csi = skip_csi(rest);
            for (from, to) in STD_CAPS {
                let adj = if csi != 0 { 2 } else { 0 };
                let l = from.len();
                if csi != 0 && skip_csi(from) != csi {
                    continue;
                }
                if l > adj && rest.get(csi..).is_some_and(|r| r.starts_with(&from[adj..])) {
                    expansion = Some(to.as_bytes().to_vec());
                    len = l - adj + csi;
                    break;
                }
            }
        }
        // sequências de modo padrão
        if expansion.is_none() {
            let csi = skip_csi(rest);
            if csi != 0 {
                let l = span_digits_semi(&rest[csi..]);
                if l != 0 && l < 4096 {
                    let next = csi + l;
                    let c = rest.get(next).copied().unwrap_or(0);
                    if c == b'h' || c == b'l' {
                        let mut buf2 = if c == b'h' {
                            b"ECMA+".to_vec()
                        } else {
                            b"ECMA-".to_vec()
                        };
                        if lookup_params(STD_MODES, &mut buf2, &rest[csi..csi + l]) {
                            expansion = Some(buf2);
                        }
                        len = l;
                    }
                }
            }
        }
        // sequências de modo privado
        if expansion.is_none() {
            let csi = skip_csi(rest);
            if csi != 0 && rest.get(csi) == Some(&b'?') {
                let l = span_digits_semi(&rest[csi + 1..]);
                if l != 0 && l < 4096 {
                    let next = csi + 1 + l;
                    let c = rest.get(next).copied().unwrap_or(0);
                    if c == b'h' || c == b'l' {
                        let mut buf2 = if c == b'h' {
                            b"DEC+".to_vec()
                        } else {
                            b"DEC-".to_vec()
                        };
                        if lookup_params(PRIVATE_MODES, &mut buf2, &rest[csi + 1..csi + 1 + l]) {
                            expansion = Some(buf2);
                        }
                        len = l;
                    }
                }
            }
        }
        // sequências de realce ECMA
        if expansion.is_none() {
            let csi = skip_csi(rest);
            if csi != 0 {
                let l = span_digits_semi(&rest[csi..]);
                if l != 0 && l < 4096 && rest.get(csi + l) == Some(&b'm') {
                    let mut buf2 = b"SGR:".to_vec();
                    let found = lookup_params(ECMA_HIGHLIGHTS, &mut buf2, &rest[csi..csi + l]);
                    len = l + csi + 1;
                    if found {
                        expansion = Some(buf2);
                    }
                }
            }
        }
        if expansion.is_none() {
            let csi = skip_csi(rest);
            if csi != 0 && rest.get(csi) == Some(&b'm') {
                len = csi + 1;
                let mut buf2 = b"SGR:".to_vec();
                buf2.extend_from_slice(ECMA_HIGHLIGHTS[0].1.as_bytes());
                expansion = Some(buf2);
            }
        }
        // reinício da região de rolagem
        if expansion.is_none() {
            let csi = skip_csi(rest);
            if csi != 0 {
                if rest.get(csi) == Some(&b'r') {
                    expansion = Some(b"RSR".to_vec());
                    len = 1;
                } else {
                    let buf2 = format!("1;{tp_lines}r");
                    len = buf2.len();
                    if rest[csi.min(rest.len())..].starts_with(buf2.as_bytes()) {
                        expansion = Some(b"RSR".to_vec());
                    }
                }
                len += csi;
            }
        }
        // canto inferior esquerdo
        if expansion.is_none() {
            let csi = skip_csi(rest);
            if csi != 0 {
                let buf2 = format!("{tp_lines};1H");
                len = buf2.len();
                if rest[csi.min(rest.len())..].starts_with(buf2.as_bytes()) {
                    expansion = Some(b"LL".to_vec());
                } else {
                    let buf2 = format!("{tp_lines}H");
                    len = buf2.len();
                    if rest[csi.min(rest.len())..].starts_with(buf2.as_bytes()) {
                        expansion = Some(b"LL".to_vec());
                    }
                }
                len += csi;
            }
        }
        match expansion {
            Some(e) => {
                let _ = o.write_all(b"{");
                let _ = o.write_all(&e);
                let _ = o.write_all(b"}");
                sp += len.max(1);
            }
            None => {
                let one = [at(sp)];
                let _ = o.write_all(&tic_expand(
                    &one,
                    cfg.outform == OutForm::Terminfo,
                    cfg.numbers,
                ));
                sp += 1;
            }
        }
    }
    let _ = o.write_all(b"\n");
}

// ---- inicializadores em C (`-e` e `-E`) ----

fn any_initializer(names: &[u8], fmt: &str, ty: &[u8]) -> String {
    let mut s = String::new();
    for &b in names {
        if b == b'|' {
            break;
        }
        s.push(if b.is_ascii_alphanumeric() {
            b as char
        } else {
            '_'
        });
    }
    s.push_str(&fmt.replace("%s", &io::lossy(ty)));
    s
}

fn dump_initializers(term: &TermType) {
    let mut o = io::stdout();
    let name_init = |ty: &str| any_initializer(&term.names, "_%s_data", ty.as_bytes());
    let str_var = |ty: &[u8]| any_initializer(&term.names, "_s_%s", ty);
    let _ = write!(
        o,
        "\nstatic char {}[] = \"{}\";\n\n",
        name_init("alias"),
        io::lossy(&term.names)
    );
    let str_name = |i: usize| -> Vec<u8> {
        if i >= STRCOUNT {
            term.ext_str_name(i).to_vec()
        } else {
            STRS[i].info.as_bytes().to_vec()
        }
    };
    for n in 0..term.strs.len() {
        if let Str::Val(v) = &term.strs[n] {
            let mut buf = String::from("\"");
            let mut count = 1usize;
            for &c in v {
                if count + 5 >= MAX_STRING - 6 {
                    break;
                }
                if c.is_ascii() && (0x20..0x7f).contains(&c) && c != b'\\' && c != b'"' {
                    buf.push(c as char);
                    count += 1;
                } else {
                    buf.push_str(&format!("\\{c:03o}"));
                    count += 4;
                }
            }
            buf.push('"');
            let _ = writeln!(o, "static char {:<20}[] = {};", str_var(&str_name(n)), buf);
        }
    }
    let _ = writeln!(o);
    let _ = writeln!(o, "static char {}[] = {{", name_init("bool"));
    for n in 0..term.bools.len() {
        let st = match term.bools[n] {
            1 => "TRUE",
            0 => "FALSE",
            -1 => "ABSENT_BOOLEAN",
            _ => "CANCELLED_BOOLEAN",
        };
        let nm = if n >= BOOLCOUNT {
            term.ext_bool_name(n).to_vec()
        } else {
            super::BOOLS[n].info.as_bytes().to_vec()
        };
        let _ = writeln!(o, "\t/* {:3}: {:<8} */\t{},", n, io::lossy(&nm), st);
    }
    let _ = writeln!(o, "}};");
    let _ = writeln!(o, "static short {}[] = {{", name_init("number"));
    for n in 0..term.nums.len() {
        let st = match term.nums[n] {
            -1 => "ABSENT_NUMERIC".to_string(),
            -2 => "CANCELLED_NUMERIC".to_string(),
            v => v.to_string(),
        };
        let nm = if n >= NUMCOUNT {
            term.ext_num_name(n).to_vec()
        } else {
            super::NUMS[n].info.as_bytes().to_vec()
        };
        let _ = writeln!(o, "\t/* {:3}: {:<8} */\t{},", n, io::lossy(&nm), st);
    }
    let _ = writeln!(o, "}};");
    let _ = writeln!(o, "static char * {}[] = {{", name_init("string"));
    for n in 0..term.strs.len() {
        let st = match &term.strs[n] {
            Str::Absent => "ABSENT_STRING".to_string(),
            Str::Cancelled => "CANCELLED_STRING".to_string(),
            Str::Val(_) => str_var(&str_name(n)),
        };
        let _ = writeln!(
            o,
            "\t/* {:3}: {:<8} */\t{},",
            n,
            io::lossy(&str_name(n)),
            st
        );
    }
    let _ = writeln!(o, "}};");
    if term.bools.len() != BOOLCOUNT || term.nums.len() != NUMCOUNT || term.strs.len() != STRCOUNT {
        let _ = writeln!(o, "static char * {}[] = {{", name_init("string_ext"));
        for n in BOOLCOUNT..term.bools.len() {
            let _ = writeln!(
                o,
                "\t/* {:3}: bool */\t\"{}\",",
                n,
                io::lossy(term.ext_bool_name(n))
            );
        }
        for n in NUMCOUNT..term.nums.len() {
            let _ = writeln!(
                o,
                "\t/* {:3}: num */\t\"{}\",",
                n,
                io::lossy(term.ext_num_name(n))
            );
        }
        for n in STRCOUNT..term.strs.len() {
            let _ = writeln!(
                o,
                "\t/* {:3}: str */\t\"{}\",",
                n,
                io::lossy(term.ext_str_name(n))
            );
        }
        let _ = writeln!(o, "}};");
    }
}

fn dump_termtype(term: &TermType) {
    let mut o = io::stdout();
    let name_init = |ty: &str| any_initializer(&term.names, "_%s_data", ty.as_bytes());
    let _ = write!(o, "\t{{\n\t\t{},\n", name_init("alias"));
    let _ = writeln!(o, "\t\t(char *)0,\t/* pointer to string table */");
    let _ = writeln!(o, "\t\t{},", name_init("bool"));
    let _ = writeln!(o, "\t\t{},", name_init("number"));
    let _ = writeln!(o, "\t\t{},", name_init("string"));
    let _ = writeln!(o, "#if NCURSES_XNAMES");
    let _ = writeln!(o, "\t\t(char *)0,\t/* pointer to extended string table */");
    let ext =
        term.bools.len() != BOOLCOUNT || term.nums.len() != NUMCOUNT || term.strs.len() != STRCOUNT;
    let _ = writeln!(
        o,
        "\t\t{},\t/* ...corresponding names */",
        if ext {
            name_init("string_ext")
        } else {
            "(char **)0".to_string()
        }
    );
    let _ = writeln!(o, "\t\t{},\t\t/* count total Booleans */", term.bools.len());
    let _ = writeln!(o, "\t\t{},\t\t/* count total Numbers */", term.nums.len());
    let _ = writeln!(o, "\t\t{},\t\t/* count total Strings */", term.strs.len());
    let _ = writeln!(
        o,
        "\t\t{},\t\t/* count extensions to Booleans */",
        term.bools.len() - BOOLCOUNT
    );
    let _ = writeln!(
        o,
        "\t\t{},\t\t/* count extensions to Numbers */",
        term.nums.len() - NUMCOUNT
    );
    let _ = writeln!(
        o,
        "\t\t{},\t\t/* count extensions to Strings */",
        term.strs.len() - STRCOUNT
    );
    let _ = writeln!(o, "#endif /* NCURSES_XNAMES */");
    let _ = writeln!(o, "\t}}");
}

// ---- alinhamento dos nomes estendidos (`_nc_align_termtype`) ----

fn merge_names(a: &[Vec<u8>], b: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => {
                out.push(a[i].clone());
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                out.push(b[j].clone());
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                out.push(a[i].clone());
                i += 1;
                j += 1;
            }
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
    out
}

fn realign(t: &mut TermType, eb: &[Vec<u8>], en: &[Vec<u8>], es: &[Vec<u8>]) {
    let old_b: Vec<(Vec<u8>, i8)> = (BOOLCOUNT..t.bools.len())
        .map(|i| (t.ext_bool_name(i).to_vec(), t.bools[i]))
        .collect();
    let old_n: Vec<(Vec<u8>, i32)> = (NUMCOUNT..t.nums.len())
        .map(|i| (t.ext_num_name(i).to_vec(), t.nums[i]))
        .collect();
    let old_s: Vec<(Vec<u8>, Str)> = (STRCOUNT..t.strs.len())
        .map(|i| (t.ext_str_name(i).to_vec(), t.strs[i].clone()))
        .collect();
    t.bools.truncate(BOOLCOUNT);
    t.nums.truncate(NUMCOUNT);
    t.strs.truncate(STRCOUNT);
    for n in eb {
        t.bools
            .push(old_b.iter().find(|(k, _)| k == n).map_or(0, |(_, v)| *v));
    }
    for n in en {
        t.nums.push(
            old_n
                .iter()
                .find(|(k, _)| k == n)
                .map_or(super::ABSENT_NUMERIC, |(_, v)| *v),
        );
    }
    for n in es {
        t.strs.push(
            old_s
                .iter()
                .find(|(k, _)| k == n)
                .map_or(Str::Absent, |(_, v)| v.clone()),
        );
    }
    t.ext_bools = eb.len();
    t.ext_nums = en.len();
    t.ext_strs = es.len();
    t.ext_names = eb.iter().chain(en).chain(es).cloned().collect();
}

/// Os nomes estendidos de uma descrição, separados em booleanos, números e cadeias.
type ExtNameParts = (Vec<Vec<u8>>, Vec<Vec<u8>>, Vec<Vec<u8>>);

/// `_nc_align_termtype(to, from)`: deixa as duas descrições com os mesmos nomes estendidos.
fn align_termtype(to: &mut TermType, from: &mut TermType) {
    let (na, nb) = (to.ext_names.len(), from.ext_names.len());
    if na == 0 && nb == 0 {
        return;
    }
    if na == nb
        && to.ext_bools == from.ext_bools
        && to.ext_nums == from.ext_nums
        && to.ext_strs == from.ext_strs
        && to.ext_names == from.ext_names
    {
        return;
    }
    let part = |t: &TermType| -> ExtNameParts {
        let b = t.ext_names[..t.ext_bools].to_vec();
        let n = t.ext_names[t.ext_bools..t.ext_bools + t.ext_nums].to_vec();
        let s = t.ext_names[t.ext_bools + t.ext_nums..].to_vec();
        (b, n, s)
    };
    let (tb, tn, ts) = part(to);
    let (fb, fn_, fs) = part(from);
    let eb = merge_names(&tb, &fb);
    let en = merge_names(&tn, &fn_);
    let es = merge_names(&ts, &fs);
    realign(to, &eb, &en, &es);
    realign(from, &eb, &en, &es);
}

// ---- programa principal ----

fn usage(progname: &str) -> ! {
    const OPTIONS: &[&str] = &[
        "  -0    print single-row",
        "  -1    print single-column",
        "  -C    use termcap-names",
        "  -D    print database locations",
        "  -E    format output as C tables",
        "  -F    compare terminfo-files",
        "  -G    format %{number} to %'char'",
        "  -I    use terminfo-names",
        "  -K    use termcap-names and BSD syntax",
        "  -L    use long names",
        "  -R subset (see manpage)",
        "  -T    eliminate size limits (test)",
        "  -U    do not post-process entries",
        "  -V    print version",
        "  -W    wrap long strings per -w[n]",
        "  -a    with -F, list commented-out caps",
        "  -c    list common capabilities",
        "  -d    list different capabilities",
        "  -e    format output for C initializer",
        "  -f    with -1, format complex strings",
        "  -g    format %'char' to %{number}",
        "  -i    analyze initialization/reset",
        "  -l    output terminfo names",
        "  -n    list capabilities in neither",
        "  -p    ignore padding specifiers",
        "  -Q number  dump compiled description",
        "  -q    brief listing, removes headers",
        "  -r    with -C, output in termcap form",
        "  -r    with -F, resolve use-references",
        "  -s [d|i|l|c] sort fields",
        "  -t    suppress commented-out capabilities",
        "  -u    produce source with 'use='",
        "  -v number  (verbose)",
        "  -w number  (width)",
        "  -x    unknown capabilities are user-defined",
    ];
    let last = OPTIONS.len();
    let left = last.div_ceil(2);
    let mut text = format!(
        "Usage: {progname} [options] [-A directory] [-B directory] [termname...]\nOptions:\n"
    );
    for (n, first) in OPTIONS.iter().enumerate().take(left) {
        match OPTIONS.get(n + left) {
            Some(second) => text.push_str(&format!("{first:<40.40}{second}\n")),
            None => text.push_str(&format!("{first}\n")),
        }
    }
    io::eprint(text);
    sys::exit(1)
}

fn terminal_env(progname: &str) -> Vec<u8> {
    match sys::getenv("TERM") {
        Some(t) => t,
        None => {
            io::eprint(format!("{progname}: environment variable TERM not set\n"));
            sys::exit(1)
        }
    }
}

fn optarg_to_number(arg: &[u8]) -> i32 {
    let (v, end) = strtol(arg);
    if end == 0 || end != arg.len() {
        io::eprint(format!("Expected a number, not \"{}\"\n", io::lossy(arg)));
        sys::exit(1);
    }
    v as i32
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let progname = io::lossy(rootname(&argv[0]));
    let mut cfg = Cfg {
        limited: true,
        quiet: false,
        literal: false,
        bool_sep: ":",
        s_absent: "NULL",
        s_cancel: "NULL",
        mwidth: 60,
        mheight: 65535,
        numbers: 0,
        outform: OutForm::Terminfo,
        ignorepads: false,
        compare: Compare::Default,
        user_definable: false,
        itrace: 0,
    };
    let mut sortmode = SortMode::Default;
    let mut tversion: Option<String> = None;
    let mut firstdir: Option<Vec<u8>> = None;
    let mut restdir: Option<Vec<u8>> = None;
    let mut formatted = false;
    let mut filecompare = false;
    let mut initdump = 0;
    let mut init_analyze = false;
    let mut suppress_untranslatable = false;
    let mut quickdump = 0;
    let mut wrap_strings = false;
    let mut strict_bsd = false;

    let mut g = Getopt::from_env(
        &argv[1..],
        "01A:aB:CcDdEeFfGgIiKLlnpQ:qR:rs:TtUuVv:Ww:x",
        &[],
    );
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => bad_option(&e, &argv0, &progname),
        };
        let arg = o.arg.clone().unwrap_or_default();
        match o.short().unwrap_or('?') {
            '0' => {
                cfg.mwidth = 65535;
                cfg.mheight = 1;
            }
            '1' => cfg.mwidth = 0,
            'A' => firstdir = Some(arg),
            'a' => cfg.user_definable = true,
            'B' => restdir = Some(arg),
            'K' | 'C' => {
                if o.short() == Some('K') {
                    strict_bsd = true;
                }
                cfg.outform = OutForm::Termcap;
                tversion = Some("BSD".to_string());
                if sortmode == SortMode::Default {
                    sortmode = SortMode::Termcap;
                }
            }
            'D' => {
                let mut out = io::stdout();
                for d in db_dirs(None) {
                    let _ = out.write_all(&d);
                    let _ = out.write_all(b"\n");
                }
                return 0;
            }
            'c' => cfg.compare = Compare::Common,
            'd' => cfg.compare = Compare::Difference,
            'E' => initdump |= 2,
            'e' => initdump |= 1,
            'F' => filecompare = true,
            'f' => formatted = true,
            'G' => cfg.numbers = 1,
            'g' => cfg.numbers = -1,
            'I' => {
                cfg.outform = OutForm::Terminfo;
                if sortmode == SortMode::Default {
                    sortmode = SortMode::Variable;
                }
                tversion = None;
            }
            'i' => init_analyze = true,
            'L' => {
                cfg.outform = OutForm::Variable;
                if sortmode == SortMode::Default {
                    sortmode = SortMode::Variable;
                }
            }
            'l' => cfg.outform = OutForm::Terminfo,
            'n' => cfg.compare = Compare::Nand,
            'p' => cfg.ignorepads = true,
            'Q' => quickdump = optarg_to_number(&arg),
            'q' => {
                cfg.quiet = true;
                cfg.s_absent = "-";
                cfg.s_cancel = "@";
                cfg.bool_sep = ", ";
            }
            'R' => tversion = Some(io::lossy(&arg)),
            'r' => tversion = None,
            's' => match arg.first() {
                Some(b'd') => sortmode = SortMode::NoSort,
                Some(b'i') => sortmode = SortMode::Terminfo,
                Some(b'l') => sortmode = SortMode::Variable,
                Some(b'c') => sortmode = SortMode::Termcap,
                _ => {
                    io::eprint(format!("{progname}: unknown sort mode\n"));
                    return 1;
                }
            },
            'T' => cfg.limited = false,
            't' => suppress_untranslatable = true,
            'U' => cfg.literal = true,
            'u' => cfg.compare = Compare::UseAll,
            'V' => {
                let _ = writeln!(io::stdout(), "{VERSION}");
                return 0;
            }
            'v' => cfg.itrace = optarg_to_number(&arg) as u32,
            'W' => wrap_strings = true,
            'w' => cfg.mwidth = optarg_to_number(&arg),
            'x' => cfg.user_definable = true,
            _ => usage(&progname),
        }
    }
    let _ = (cfg.literal, quickdump);

    let mut names: Vec<Vec<u8>> = g.operands();
    if sortmode == SortMode::Default {
        sortmode = SortMode::Terminfo;
    }
    if names.is_empty() {
        names.push(terminal_env(&progname));
    }
    if cfg.compare != Compare::Default && names.len() < 2 {
        names.push(terminal_env(&progname));
    }
    if cfg.compare == Compare::Default {
        match names.len() {
            1 => {}
            2 => cfg.compare = Compare::Difference,
            _ => {
                io::eprint(format!("{progname}: too many names to compare\n"));
                return 1;
            }
        }
    }

    let mut dump = Dump::new(
        tversion.as_deref(),
        cfg.outform,
        sortmode,
        wrap_strings,
        cfg.mwidth,
        cfg.mheight,
        cfg.itrace,
        formatted,
        false,
        quickdump,
        &progname,
    );
    dump.user_definable = cfg.user_definable;
    dump.strict_bsd = strict_bsd;

    if !filecompare {
        let mut entries: Vec<TermType> = Vec::new();
        let mut tfiles: Vec<Vec<u8>> = Vec::new();
        for (count, name) in names.iter().enumerate() {
            let directory = if count > 0 {
                restdir.as_ref()
            } else {
                firstdir.as_ref()
            };
            let (code, file, tt) = if let Some(dir) = directory {
                let mut f = dir.clone();
                f.push(b'/');
                f.push(name.first().copied().unwrap_or(0));
                f.push(b'/');
                f.extend_from_slice(name);
                if cfg.itrace != 0 {
                    io::eprint(format!(
                        "{progname}: reading entry {} from file {}\n",
                        io::lossy(name),
                        io::lossy(&f)
                    ));
                }
                match read_file_entry(&f, cfg.user_definable) {
                    Some(t) => (1, f, Some(t)),
                    None => (TGETENT_NO, f, None),
                }
            } else {
                if cfg.itrace != 0 {
                    io::eprint(format!(
                        "{progname}: reading entry {} from database\n",
                        io::lossy(name)
                    ));
                }
                let r = read_entry(name, cfg.user_definable);
                (r.code, r.filename, r.tt)
            };
            let Some(mut tt) = tt else {
                if code == TGETENT_NO {
                    io::eprint(format!(
                        "{progname}: error: no match in terminfo database for terminal type \"{}\"\n",
                        io::lossy(name)
                    ));
                } else if code == TGETENT_ERR {
                    io::eprint(format!(
                        "{progname}: error: unable to open terminfo database: {}\n",
                        sysabi::Errno::ENOENT.message()
                    ));
                }
                return 1;
            };
            repair_acsc(&mut tt);
            entries.push(tt);
            tfiles.push(file);
        }
        if entries.len() > 1 {
            let (first, rest) = entries.split_at_mut(1);
            for e in rest.iter_mut() {
                align_termtype(e, &mut first[0]);
            }
        }

        if initdump != 0 {
            if initdump & 1 != 0 {
                dump_termtype(&entries[0]);
            }
            if initdump & 2 != 0 {
                dump_initializers(&entries[0]);
            }
        } else if init_analyze {
            let t = entries[0].clone();
            for (nm, var) in [
                ("is1", "init_1string"),
                ("is2", "init_2string"),
                ("is3", "init_3string"),
                ("rs1", "reset_1string"),
                ("rs2", "reset_2string"),
                ("rs3", "reset_3string"),
                ("smcup", "enter_ca_mode"),
                ("rmcup", "exit_ca_mode"),
                ("smkx", "keypad_xmit"),
                ("rmkx", "keypad_local"),
            ] {
                analyze_string(&cfg, nm, super::str_index(var), &t);
            }
        } else {
            match cfg.compare {
                Compare::Default => {
                    if cfg.itrace != 0 {
                        io::eprint(format!(
                            "{progname}: about to dump {}\n",
                            io::lossy(&names[0])
                        ));
                    }
                    if !cfg.quiet {
                        let _ = writeln!(
                            io::stdout(),
                            "#\tReconstructed via {progname} from file: {}",
                            io::lossy(&tfiles[0])
                        );
                    }
                    let pred: PredFn<'_> = &dump_predicate;
                    dump.dump_entry(
                        &mut entries[0],
                        suppress_untranslatable,
                        cfg.limited,
                        cfg.numbers,
                        pred,
                    );
                    let len = dump.show_entry();
                    if cfg.itrace != 0 {
                        io::eprint(format!("{progname}: length {len}\n"));
                    }
                }
                Compare::Difference | Compare::Common | Compare::Nand => {
                    show_comparing(&cfg, &progname, &names);
                    let tp = entries[0].clone();
                    let cfg_ref = &cfg;
                    let ents: &[TermType] = &entries;
                    dump.compare_entry(
                        &mut |kind, idx, name| compare_predicate(cfg_ref, ents, kind, idx, name),
                        &tp,
                        cfg.quiet,
                    );
                }
                Compare::UseAll => {
                    if cfg.itrace != 0 {
                        io::eprint(format!("{progname}: dumping use entry\n"));
                    }
                    let (first, rest) = entries.split_at_mut(1);
                    let ignorepads = cfg.ignorepads;
                    let rest: &[TermType] = rest;
                    let pred = |tt: &TermType, kind: Kind, idx: usize| -> i32 {
                        use_predicate(rest, ignorepads, tt, kind, idx)
                    };
                    dump.dump_entry(
                        &mut first[0],
                        suppress_untranslatable,
                        cfg.limited,
                        cfg.numbers,
                        &pred,
                    );
                    let tc = matches!(cfg.outform, OutForm::Termcap | OutForm::TcConvErr);
                    for n in &names[1..] {
                        dump.dump_uses(n, !tc);
                    }
                    let len = dump.show_entry();
                    if cfg.itrace != 0 {
                        io::eprint(format!("{progname}: length {len}\n"));
                    }
                }
            }
        }
    } else if cfg.compare == Compare::UseAll {
        io::eprint("Sorry, -u doesn't work with -F\n");
    } else if cfg.compare == Compare::Default {
        io::eprint("Use `tic -[CI] <file>' for this.\n");
    } else if names.len() != 2 {
        io::eprint("File comparison needs exactly two file arguments.\n");
    } else {
        io::eprint(format!(
            "{progname}: reading terminfo source files is not supported\n"
        ));
        return 1;
    }
    0
}

fn bad_option(e: &GetoptError, argv0: &str, progname: &str) -> ! {
    io::eprint(format!("{}\n", e.message(argv0)));
    usage(progname)
}
