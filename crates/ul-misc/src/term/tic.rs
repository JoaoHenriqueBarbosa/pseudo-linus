//! `tic` do ncurses 6.5.20250216 (`tic.c`, `comp_parse.c`, `parse_entry.c`, `write_entry.c`): compila
//! uma fonte terminfo para o formato binário do banco (`~/.terminfo` ou o diretório do `-o`).
//!
//! Escopo desta versão: leitura da fonte (nomes, booleanos, números, cadeias, cancelamentos `nome@`,
//! `use=`), capacidades estendidas com `-x`, resolução de `use=` (primeiro no próprio arquivo, depois
//! no banco instalado), gravação no formato legado (números de 16 bits) ou no estendido de 32 bits
//! quando algum número passa de 32767, com a tabela de cadeias na ordem das capacidades e os
//! apelidos como links físicos. As opções de saída em texto (`-I`, `-C`, `-L`, `-r`) não existem
//! ainda e terminam com erro; `-c` só confere a fonte.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{AtFlags, Ctx, Errno, Fd, OFlags, sys};

use super::terminfo::{Str, TermType, db_dirs, name_match, read_file_entry};
use super::{
    BOOLCOUNT, BOOLWRITE, Kind, NUMCOUNT, NUMWRITE, STRCOUNT, STRWRITE, VERSION, find_type_entry,
    first_name, rootname, strtol,
};
use crate::util::Getopt;
use crate::util::io;

/// Valor de uma capacidade estendida (`-x`), pelo tipo.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ExtVal {
    Bool(i8),
    Num(i32),
    Str(Str),
}

impl ExtVal {
    fn same_kind(&self, other: &ExtVal) -> bool {
        matches!(
            (self, other),
            (ExtVal::Bool(_), ExtVal::Bool(_))
                | (ExtVal::Num(_), ExtVal::Num(_))
                | (ExtVal::Str(_), ExtVal::Str(_))
        )
    }
}

#[derive(Clone, Debug)]
struct ExtCap {
    name: Vec<u8>,
    val: ExtVal,
}

/// Uma entrada da fonte: as capacidades predefinidas pelo índice da tabela `Caps`, as estendidas
/// na ordem de definição e os `use=` ainda por resolver.
#[derive(Clone, Debug)]
struct Entry {
    names: Vec<u8>,
    bools: Vec<i8>,
    nums: Vec<i32>,
    strs: Vec<Str>,
    ext: Vec<ExtCap>,
    uses: Vec<Vec<u8>>,
    line: usize,
}

impl Entry {
    fn new(names: Vec<u8>, line: usize) -> Entry {
        Entry {
            names,
            bools: vec![0; BOOLCOUNT],
            nums: vec![-1; NUMCOUNT],
            strs: vec![Str::Absent; STRCOUNT],
            ext: Vec::new(),
            uses: Vec::new(),
            line,
        }
    }

    fn from_termtype(tt: &TermType) -> Entry {
        let mut e = Entry::new(tt.names.clone(), 0);
        for i in 0..BOOLCOUNT {
            e.bools[i] = tt.bools.get(i).copied().unwrap_or(0);
        }
        for i in 0..NUMCOUNT {
            e.nums[i] = tt.nums.get(i).copied().unwrap_or(-1);
        }
        for i in 0..STRCOUNT {
            e.strs[i] = tt.strs.get(i).cloned().unwrap_or(Str::Absent);
        }
        let mut k = 0;
        for i in 0..tt.ext_bools {
            let name = tt.ext_names.get(k).cloned().unwrap_or_default();
            k += 1;
            let v = tt.bools.get(BOOLCOUNT + i).copied().unwrap_or(0);
            e.ext.push(ExtCap {
                name,
                val: ExtVal::Bool(v),
            });
        }
        for i in 0..tt.ext_nums {
            let name = tt.ext_names.get(k).cloned().unwrap_or_default();
            k += 1;
            let v = tt.nums.get(NUMCOUNT + i).copied().unwrap_or(-1);
            e.ext.push(ExtCap {
                name,
                val: ExtVal::Num(v),
            });
        }
        for i in 0..tt.ext_strs {
            let name = tt.ext_names.get(k).cloned().unwrap_or_default();
            k += 1;
            let v = tt.strs.get(STRCOUNT + i).cloned().unwrap_or(Str::Absent);
            e.ext.push(ExtCap {
                name,
                val: ExtVal::Str(v),
            });
        }
        e
    }

    /// `_nc_merge_entry`: o que falta nesta entrada vem da outra (o que a entrada já tem, inclusive
    /// um cancelamento, prevalece).
    fn merge_from(&mut self, src: &Entry) {
        for i in 0..BOOLCOUNT {
            if self.bools[i] == 0 && src.bools[i] == 1 {
                self.bools[i] = 1;
            }
        }
        for i in 0..NUMCOUNT {
            if self.nums[i] == -1 && src.nums[i] >= 0 {
                self.nums[i] = src.nums[i];
            }
        }
        for i in 0..STRCOUNT {
            if self.strs[i] == Str::Absent && src.strs[i].valid() {
                self.strs[i] = src.strs[i].clone();
            }
        }
        for cap in &src.ext {
            let present = self
                .ext
                .iter()
                .any(|c| c.name == cap.name && c.val.same_kind(&cap.val));
            let valid = match &cap.val {
                ExtVal::Bool(b) => *b == 1,
                ExtVal::Num(n) => *n >= 0,
                ExtVal::Str(s) => s.valid(),
            };
            if !present && valid {
                self.ext.push(cap.clone());
            }
        }
    }
}

/// `_nc_trans_string`: `\E`, `\n`, `\NNN`, `^X` e afins viram os bytes; um NUL octal vira `0200`.
fn trans_string(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'^' && i + 1 < s.len() {
            let n = s[i + 1];
            out.push(if n == b'?' { 0x7f } else { n & 0x1f });
            i += 2;
        } else if c == b'\\' && i + 1 < s.len() {
            let n = s[i + 1];
            i += 2;
            match n {
                b'0'..=b'7' => {
                    let mut v = u32::from(n - b'0');
                    let mut count = 1;
                    while count < 3 && i < s.len() && (b'0'..=b'7').contains(&s[i]) {
                        v = v * 8 + u32::from(s[i] - b'0');
                        i += 1;
                        count += 1;
                    }
                    let mut b = (v & 0xff) as u8;
                    if b == 0 {
                        b = 0o200;
                    }
                    out.push(b);
                }
                b'e' | b'E' => out.push(0x1b),
                b'n' | b'l' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'b' => out.push(0x08),
                b'f' => out.push(0x0c),
                b's' => out.push(b' '),
                b'a' => out.push(0x07),
                other => out.push(other),
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Separa os campos por vírgulas não escapadas.
fn split_fields(body: &[u8]) -> Vec<Vec<u8>> {
    let mut fields: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < body.len() {
        if body[i] == b'\\' && i + 1 < body.len() {
            cur.push(body[i]);
            cur.push(body[i + 1]);
            i += 2;
            continue;
        }
        if body[i] == b',' {
            fields.push(std::mem::take(&mut cur));
        } else {
            cur.push(body[i]);
        }
        i += 1;
    }
    fields
}

/// Divide a fonte em entradas: linha que começa em coluna 0 abre uma, as que começam com espaço a
/// continuam, `#` em coluna 0 é comentário. Devolve o texto e a linha onde cada entrada começa.
fn split_entries(data: &[u8]) -> Vec<(Vec<u8>, usize)> {
    let mut out: Vec<(Vec<u8>, usize)> = Vec::new();
    let mut body: Vec<u8> = Vec::new();
    let mut start = 0usize;
    let mut have = false;
    for (n, line) in data.split(|b| *b == b'\n').enumerate() {
        let lineno = n + 1;
        if line.first() == Some(&b'#') {
            continue;
        }
        if line.iter().all(|b| super::c_isspace(*b)) {
            continue;
        }
        if super::c_isspace(line[0]) {
            if have {
                body.push(b' ');
                body.extend_from_slice(line);
            }
        } else {
            if have {
                out.push((std::mem::take(&mut body), start));
            }
            have = true;
            start = lineno;
            body.extend_from_slice(line);
        }
    }
    if have {
        out.push((body, start));
    }
    out
}

fn trim_start(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|b| super::c_isspace(**b)).count();
    &s[n..]
}

fn trim_end(s: &[u8]) -> &[u8] {
    let mut e = s.len();
    while e > 0 && super::c_isspace(s[e - 1]) {
        e -= 1;
    }
    &s[..e]
}

struct Diag<'a> {
    file: &'a str,
}

impl Diag<'_> {
    fn warn(&self, line: usize, term: &[u8], msg: &str) {
        let _ = io::flush_stdout();
        io::eprint(format!(
            "\"{}\", line {}, terminal '{}': {}\n",
            self.file,
            line,
            io::lossy(first_name(term)),
            msg
        ));
    }
}

/// `parse_entry`: uma entrada da fonte.
fn parse_entry(body: &[u8], line: usize, xflag: bool, diag: &Diag) -> Option<Entry> {
    let fields = split_fields(body);
    let first = fields.first()?;
    let names = trim_end(trim_start(first)).to_vec();
    if names.is_empty() {
        return None;
    }
    let mut e = Entry::new(names, line);
    for f in &fields[1..] {
        let f = trim_start(f);
        if f.is_empty() {
            continue;
        }
        let sep_pos = f.iter().position(|b| matches!(*b, b'=' | b'#' | b'@'));
        let (name, sep, value): (&[u8], u8, &[u8]) = match sep_pos {
            Some(p) => (trim_end(&f[..p]), f[p], &f[p + 1..]),
            None => (trim_end(f), 0, &[]),
        };
        if sep == b'=' && name == b"use" {
            e.uses.push(trim_end(value).to_vec());
            continue;
        }
        let nm = String::from_utf8_lossy(name).into_owned();
        match sep {
            0 => {
                if let Some(i) = find_type_entry(name, Kind::Bool) {
                    e.bools[i] = 1;
                } else if find_type_entry(name, Kind::Num).is_some()
                    || find_type_entry(name, Kind::Str).is_some()
                {
                    diag.warn(line, &e.names, &format!("Wrong type used for capability \"{nm}\""));
                } else if xflag {
                    set_ext(&mut e, name, ExtVal::Bool(1));
                } else {
                    diag.warn(line, &e.names, &format!("Unknown Capability - \"{nm}\""));
                }
            }
            b'@' => {
                if let Some(i) = find_type_entry(name, Kind::Bool) {
                    e.bools[i] = -2;
                } else if let Some(i) = find_type_entry(name, Kind::Num) {
                    e.nums[i] = -2;
                } else if let Some(i) = find_type_entry(name, Kind::Str) {
                    e.strs[i] = Str::Cancelled;
                } else if xflag {
                    set_ext(&mut e, name, ExtVal::Bool(-2));
                } else {
                    diag.warn(line, &e.names, &format!("Unknown Capability - \"{nm}\""));
                }
            }
            b'#' => {
                let (v, _) = strtol(trim_start(value));
                let v = v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
                if let Some(i) = find_type_entry(name, Kind::Num) {
                    e.nums[i] = v;
                } else if find_type_entry(name, Kind::Bool).is_some()
                    || find_type_entry(name, Kind::Str).is_some()
                {
                    diag.warn(line, &e.names, &format!("Wrong type used for capability \"{nm}\""));
                } else if xflag {
                    set_ext(&mut e, name, ExtVal::Num(v));
                } else {
                    diag.warn(line, &e.names, &format!("Unknown Capability - \"{nm}\""));
                }
            }
            _ => {
                let v = trans_string(value);
                if let Some(i) = find_type_entry(name, Kind::Str) {
                    e.strs[i] = Str::Val(v);
                } else if find_type_entry(name, Kind::Bool).is_some()
                    || find_type_entry(name, Kind::Num).is_some()
                {
                    diag.warn(line, &e.names, &format!("Wrong type used for capability \"{nm}\""));
                } else if xflag {
                    set_ext(&mut e, name, ExtVal::Str(Str::Val(v)));
                } else {
                    diag.warn(line, &e.names, &format!("Unknown Capability - \"{nm}\""));
                }
            }
        }
    }
    Some(e)
}

fn set_ext(e: &mut Entry, name: &[u8], val: ExtVal) {
    if let Some(c) = e
        .ext
        .iter_mut()
        .find(|c| c.name == name && c.val.same_kind(&val))
    {
        c.val = val;
    } else {
        e.ext.push(ExtCap {
            name: name.to_vec(),
            val,
        });
    }
}

/// Procura a entrada `name` no banco instalado.
fn lookup_db(name: &[u8]) -> Option<Entry> {
    let c = *name.first()?;
    if name.contains(&b'/') {
        return None;
    }
    for dir in db_dirs(None) {
        if dir.starts_with(b"b64:") || dir.starts_with(b"hex:") {
            continue;
        }
        let mut path = dir.clone();
        path.push(b'/');
        path.push(c);
        path.push(b'/');
        path.extend_from_slice(name);
        if let Some(tt) = read_file_entry(&path, true) {
            return Some(Entry::from_termtype(&tt));
        }
    }
    None
}

/// `_nc_resolve_uses2`: aplica os `use=` da entrada `idx`, recursivamente. Os `use=` valem na ordem
/// em que aparecem, e o que veio antes prevalece. `Err` leva a mensagem do problema.
fn resolve(
    entries: &mut Vec<Entry>,
    done: &mut Vec<bool>,
    idx: usize,
    stack: &mut Vec<usize>,
) -> Result<(), String> {
    if done[idx] {
        return Ok(());
    }
    stack.push(idx);
    let uses = entries[idx].uses.clone();
    let mut merged = entries[idx].clone();
    for u in &uses {
        let found = (0..entries.len()).find(|j| name_match(&entries[*j].names, u));
        let base = match found {
            Some(j) => {
                if stack.contains(&j) {
                    return Err(format!(
                        "circular use= (or like) reference to {}",
                        io::lossy(u)
                    ));
                }
                resolve(entries, done, j, stack)?;
                entries[j].clone()
            }
            None => match lookup_db(u) {
                Some(b) => b,
                None => {
                    return Err(format!(
                        "terminal '{}': couldn't resolve use={}",
                        io::lossy(first_name(&entries[idx].names)),
                        io::lossy(u)
                    ));
                }
            },
        };
        merged.merge_from(&base);
    }
    merged.uses.clear();
    entries[idx] = merged;
    done[idx] = true;
    stack.pop();
    Ok(())
}

fn push16(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&(v as i16).to_le_bytes());
}

fn push_num(out: &mut Vec<u8>, v: i32, wide: bool) {
    if wide {
        out.extend_from_slice(&v.to_le_bytes());
    } else {
        push16(out, v);
    }
}

/// Soma a uma tabela de cadeias e devolve o deslocamento que vai pro vetor de offsets.
fn table_offset(table: &mut Vec<u8>, s: &Str) -> i32 {
    match s {
        Str::Absent => -1,
        Str::Cancelled => -2,
        Str::Val(v) => {
            let off = table.len() as i32;
            table.extend_from_slice(v);
            table.push(0);
            off
        }
    }
}

/// `write_object`: o arquivo compilado (formato legado, ou estendido de 32 bits quando algum número
/// não cabe em 16 bits) com a parte das capacidades estendidas quando existem.
fn compile(e: &Entry) -> Vec<u8> {
    let ext_b: Vec<&ExtCap> = e
        .ext
        .iter()
        .filter(|c| matches!(c.val, ExtVal::Bool(_)))
        .collect();
    let ext_n: Vec<&ExtCap> = e
        .ext
        .iter()
        .filter(|c| matches!(c.val, ExtVal::Num(_)))
        .collect();
    let ext_s: Vec<&ExtCap> = e
        .ext
        .iter()
        .filter(|c| matches!(c.val, ExtVal::Str(_)))
        .collect();
    let wide = e.nums.iter().any(|n| *n > 0x7fff)
        || ext_n
            .iter()
            .any(|c| matches!(c.val, ExtVal::Num(n) if n > 0x7fff));

    let bool_count = (0..BOOLWRITE.min(e.bools.len()))
        .rev()
        .find(|i| e.bools[*i] != 0)
        .map_or(0, |i| i + 1);
    let num_count = (0..NUMWRITE.min(e.nums.len()))
        .rev()
        .find(|i| e.nums[*i] != -1)
        .map_or(0, |i| i + 1);
    let str_count = (0..STRWRITE.min(e.strs.len()))
        .rev()
        .find(|i| e.strs[*i] != Str::Absent)
        .map_or(0, |i| i + 1);

    let mut table: Vec<u8> = Vec::new();
    let mut offsets: Vec<i32> = Vec::with_capacity(str_count);
    for s in &e.strs[..str_count] {
        offsets.push(table_offset(&mut table, s));
    }

    let name_size = e.names.len() + 1;
    let mut out: Vec<u8> = Vec::new();
    push16(&mut out, if wide { 0o1036 } else { 0o432 });
    push16(&mut out, name_size as i32);
    push16(&mut out, bool_count as i32);
    push16(&mut out, num_count as i32);
    push16(&mut out, str_count as i32);
    push16(&mut out, table.len() as i32);
    out.extend_from_slice(&e.names);
    out.push(0);
    for b in &e.bools[..bool_count] {
        out.push(*b as u8);
    }
    if !(name_size + bool_count).is_multiple_of(2) {
        out.push(0);
    }
    for n in &e.nums[..num_count] {
        push_num(&mut out, *n, wide);
    }
    for o in &offsets {
        push16(&mut out, *o);
    }
    out.extend_from_slice(&table);

    if !e.ext.is_empty() {
        if !table.len().is_multiple_of(2) {
            out.push(0);
        }
        let mut etable: Vec<u8> = Vec::new();
        let mut value_offsets: Vec<i32> = Vec::new();
        for c in &ext_s {
            if let ExtVal::Str(s) = &c.val {
                value_offsets.push(table_offset(&mut etable, s));
            }
        }
        let mut name_offsets: Vec<i32> = Vec::new();
        let base = etable.len() as i32;
        let mut names_part: Vec<u8> = Vec::new();
        for c in ext_b.iter().chain(ext_n.iter()).chain(ext_s.iter()) {
            name_offsets.push(names_part.len() as i32);
            names_part.extend_from_slice(&c.name);
            names_part.push(0);
        }
        let _ = base;
        etable.extend_from_slice(&names_part);

        push16(&mut out, ext_b.len() as i32);
        push16(&mut out, ext_n.len() as i32);
        push16(&mut out, ext_s.len() as i32);
        push16(&mut out, (ext_b.len() + ext_n.len() + 2 * ext_s.len()) as i32);
        push16(&mut out, etable.len() as i32);
        for c in &ext_b {
            if let ExtVal::Bool(b) = c.val {
                out.push(b as u8);
            }
        }
        if !ext_b.len().is_multiple_of(2) {
            out.push(0);
        }
        for c in &ext_n {
            if let ExtVal::Num(n) = c.val {
                push_num(&mut out, n, wide);
            }
        }
        for o in &value_offsets {
            push16(&mut out, *o);
        }
        for o in &name_offsets {
            push16(&mut out, *o);
        }
        out.extend_from_slice(&etable);
    }
    out
}

/// Diretório de saída: `-o`, depois `$TERMINFO`, depois `$HOME/.terminfo` e por fim o do sistema.
fn output_dir(opt: Option<&[u8]>) -> Vec<u8> {
    if let Some(d) = opt {
        return d.to_vec();
    }
    if let Some(d) = sys::getenv("TERMINFO").filter(|d| !d.is_empty()) {
        return d;
    }
    if let Some(mut h) = sys::getenv("HOME").filter(|h| !h.is_empty()) {
        h.extend_from_slice(b"/.terminfo");
        return h;
    }
    b"/etc/terminfo".to_vec()
}

fn mkdir_p(path: &[u8]) -> Result<(), Errno> {
    match sys::current().mkdirat(Fd::CWD, path, 0o755) {
        Ok(()) | Err(Errno::EEXIST) => Ok(()),
        Err(e) => Err(e),
    }
}

/// `_nc_write_entry`: grava `dir/c/nome` e liga os apelidos a ele.
fn write_entry(dir: &[u8], e: &Entry, progname: &str) -> Result<(), String> {
    let primary = first_name(&e.names).to_vec();
    if primary.is_empty() || primary.contains(&b'/') {
        return Err(format!(
            "{progname}: invalid terminal name '{}'",
            io::lossy(&primary)
        ));
    }
    let data = compile(e);
    let sub = {
        let mut p = dir.to_vec();
        p.push(b'/');
        p.push(primary[0]);
        p
    };
    let fail = |what: &[u8], err: Errno| {
        format!(
            "{progname}: error: {}: {}",
            io::lossy(what),
            err.message()
        )
    };
    if mkdir_p(dir).is_err() || mkdir_p(&sub).is_err() {
        // Segue: o `open` abaixo dá o erro certo.
    }
    let mut path = sub.clone();
    path.push(b'/');
    path.extend_from_slice(&primary);
    let _ = sys::current().unlinkat(Fd::CWD, &path, AtFlags::empty());
    let fd = sys::open(&path, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, 0o644)
        .map_err(|er| fail(&path, er))?;
    let w = sys::write_all(fd, &data);
    let _ = sys::close(fd);
    w.map_err(|er| fail(&path, er))?;

    let names: Vec<&[u8]> = e.names.split(|b| *b == b'|').collect();
    if names.len() > 2 {
        for alias in &names[1..names.len() - 1] {
            if alias.is_empty() || alias.contains(&b'/') || *alias == primary.as_slice() {
                continue;
            }
            let mut apath = dir.to_vec();
            apath.push(b'/');
            apath.push(alias[0]);
            let _ = mkdir_p(&apath);
            apath.push(b'/');
            apath.extend_from_slice(alias);
            let _ = sys::current().unlinkat(Fd::CWD, &apath, AtFlags::empty());
            if sys::current()
                .linkat(Fd::CWD, &path, Fd::CWD, &apath, AtFlags::empty())
                .is_err()
            {
                let _ = sys::current().symlinkat(&primary, Fd::CWD, &apath);
            }
        }
    }
    Ok(())
}

fn usage(progname: &str) -> ! {
    io::eprint(format!(
        "Usage: {progname} [-0acfGgrsTUVvwx1] [-e names] [-o dir] [-R subset] [-w[n]] source-file\n"
    ));
    sys::exit(1)
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let progname = io::lossy(rootname(&argv[0]));
    let mut xflag = false;
    let mut check_only = false;
    let mut outdir: Option<Vec<u8>> = None;
    let mut g = Getopt::from_env(
        &argv[1..],
        "0123456789CDEGILKNR:TUVWacde:fgo:prstuvw:x1",
        &[],
    );
    while let Some(r) = g.next_opt() {
        match r {
            Ok(o) => match o.short().unwrap_or('?') {
                c if c.is_ascii_digit() => {}
                'V' => {
                    let _ = writeln!(io::stdout(), "{VERSION}");
                    return 0;
                }
                'x' => xflag = true,
                'c' => check_only = true,
                'o' => outdir = o.arg.clone(),
                'a' | 'f' | 'g' | 'G' | 's' | 'T' | 'U' | 'v' | 'w' | 'e' | 'R' | 'W' | 'N'
                | 'D' | 'E' | 'K' | 'u' | 'p' | 'd' => {}
                'C' | 'I' | 'L' | 'r' => {
                    io::eprint(format!(
                        "{progname}: option -{} (text output) is not supported\n",
                        o.short().unwrap_or('?')
                    ));
                    return 1;
                }
                _ => usage(&progname),
            },
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                usage(&progname)
            }
        }
    }
    let operands = g.operands();
    if operands.len() != 1 {
        usage(&progname);
    }
    let file = &operands[0];
    let data = if file.as_slice() == b"-" {
        sys::read_to_end(Fd::STDIN)
    } else {
        sys::read_file(file)
    };
    let data = match data {
        Ok(d) => d,
        Err(_) => {
            let _ = io::flush_stdout();
            io::eprint(format!(
                "{progname}: couldn't open '{}'\n",
                io::lossy(file)
            ));
            return 1;
        }
    };
    let file_name = io::lossy(file).to_string();
    let diag = Diag { file: &file_name };
    let mut entries: Vec<Entry> = Vec::new();
    for (body, line) in split_entries(&data) {
        match parse_entry(&body, line, xflag, &diag) {
            Some(e) => entries.push(e),
            None => {
                let _ = io::flush_stdout();
                io::eprint(format!("\"{file_name}\", line {line}: unexpected end of entry\n"));
                return 1;
            }
        }
    }
    let mut done = vec![false; entries.len()];
    for i in 0..entries.len() {
        let mut stack = Vec::new();
        if let Err(msg) = resolve(&mut entries, &mut done, i, &mut stack) {
            let _ = io::flush_stdout();
            io::eprint(format!(
                "\"{file_name}\", line {}: {msg}\n",
                entries[i].line
            ));
            return 1;
        }
    }
    if check_only {
        return 0;
    }
    let dir = output_dir(outdir.as_deref());
    for e in &entries {
        if let Err(msg) = write_entry(&dir, e, &progname) {
            let _ = io::flush_stdout();
            io::eprint(format!("{msg}\n"));
            return 1;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::terminfo::read_termtype;

    fn parse_one(src: &[u8], x: bool) -> Entry {
        let diag = Diag { file: "t" };
        let parts = split_entries(src);
        parse_entry(&parts[0].0, parts[0].1, x, &diag).unwrap()
    }

    #[test]
    fn strings_translate() {
        assert_eq!(trans_string(b"\\E[H^G\\n\\s\\0"), b"\x1b[H\x07\n \x80".to_vec());
        assert_eq!(trans_string(b"^?"), vec![0x7f]);
        assert_eq!(trans_string(b"\\,\\:\\^"), b",:^".to_vec());
    }

    #[test]
    fn legacy_roundtrip() {
        let e = parse_one(b"t|test term,\n\tam, cols#80, bel=^G,\n", false);
        let bytes = compile(&e);
        assert_eq!(&bytes[..2], &[0x1a, 0x01]);
        let tt = read_termtype(&bytes, true).unwrap();
        assert_eq!(tt.names, b"t|test term".to_vec());
        assert!(tt.b("auto_right_margin"));
        assert_eq!(tt.n("columns"), 80);
        assert_eq!(tt.sv("bell"), Some(&b"\x07"[..]));
    }

    #[test]
    fn wide_and_extended_roundtrip() {
        let e = parse_one(b"w|wide,\n\tcols#40000, XT, Ms=abc, Nx#3,\n", true);
        let bytes = compile(&e);
        assert_eq!(&bytes[..2], &[0x1e, 0x02]);
        let tt = read_termtype(&bytes, true).unwrap();
        assert_eq!(tt.n("columns"), 40000);
        assert_eq!(tt.ext_bools, 1);
        assert_eq!(tt.ext_nums, 1);
        assert_eq!(tt.ext_strs, 1);
        assert_eq!(tt.ext_names, vec![b"XT".to_vec(), b"Nx".to_vec(), b"Ms".to_vec()]);
    }

    #[test]
    fn use_in_same_file_merges() {
        let src = b"base|b,\n\tcols#80, am,\nchild|c,\n\tcols#100, use=base,\n";
        let diag = Diag { file: "t" };
        let mut entries: Vec<Entry> = split_entries(src)
            .into_iter()
            .map(|(b, l)| parse_entry(&b, l, false, &diag).unwrap())
            .collect();
        let mut done = vec![false; 2];
        for i in 0..2 {
            resolve(&mut entries, &mut done, i, &mut Vec::new()).unwrap();
        }
        assert_eq!(entries[1].nums[find_type_entry(b"cols", Kind::Num).unwrap()], 100);
        assert_eq!(entries[1].bools[find_type_entry(b"am", Kind::Bool).unwrap()], 1);
    }
}
