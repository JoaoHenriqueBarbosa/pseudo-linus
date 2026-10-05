//! `tic` do ncurses 6.5.20250216 (`tic.c`, `comp_scan.c`, `comp_parse.c`, `parse_entry.c`,
//! `write_entry.c`): compila uma fonte terminfo para o formato binário do banco (`~/.terminfo` ou o
//! diretório do `-o`), ou a reescreve em texto (`-I`, `-L`, `-C`, `-K`, e os nomes `captoinfo` e
//! `infotocap`).
//!
//! Escopo desta versão: leitura da fonte terminfo (nomes, booleanos, números, cadeias, cancelamentos
//! `nome@`, `use=`), capacidades estendidas com `-x`, resolução de `use=` (primeiro no próprio arquivo,
//! depois no banco instalado), gravação no formato legado (números de 16 bits) ou no estendido de
//! 32 bits quando algum número passa de 32767, com os apelidos como links. A saída em texto reaproveita
//! o `Dump` do `infocmp` e ecoa os comentários que antecedem cada entrada da fonte.
//!
//! Fica de fora: fontes em sintaxe termcap (`captoinfo` de verdade), as verificações semânticas do
//! `_nc_check_termtype2` (`check_acs`, `check_colors`, `check_screen`, ...) e o `postprocess_terminfo`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{AtFlags, Ctx, Errno, Fd, FileType, OFlags, sys};

use super::dump::{Dump, OutForm, PredFn, SortMode, dump_predicate, repair_acsc};
use super::terminfo::{Str, TermType, db_candidates, db_dirs, name_match, read_file_entry};
use super::{
    BOOLCOUNT, BOOLS, BOOLWRITE, Kind, NUMCOUNT, NUMS, NUMWRITE, STRCOUNT, STRS, STRWRITE, VERSION,
    find_type_entry, first_name, rootname, strtol,
};
use crate::util::Getopt;
use crate::util::io;

/// `MAX_TERMINFO_LENGTH` e `MAX_TERMCAP_LENGTH`: o limite que o `-c` confere.
const MAX_TERMINFO_LENGTH: i32 = 4096;
const MAX_TERMCAP_LENGTH: i32 = 1023;

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
    /// O trecho de comentários que antecede a entrada na fonte (`cstart` a `cend`), com os `\n`.
    comment: Vec<u8>,
    /// A linha da fonte onde a entrada começa (zero quando veio do banco).
    line: usize,
    /// Compilada com `-x`: as capacidades predefinidas além do que o formato legado grava (as
    /// `OT...` do termcap) vão pro arquivo como estendidas.
    keep_obsolete: bool,
}

impl Entry {
    fn new(names: Vec<u8>) -> Entry {
        Entry {
            keep_obsolete: false,
            names,
            bools: vec![0; BOOLCOUNT],
            nums: vec![-1; NUMCOUNT],
            strs: vec![Str::Absent; STRCOUNT],
            ext: Vec::new(),
            uses: Vec::new(),
            comment: Vec::new(),
            line: 0,
        }
    }

    fn from_termtype(tt: &TermType) -> Entry {
        let mut e = Entry::new(tt.names.clone());
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
                // Um cancelamento (`nome@`, guardado como cadeia cancelada) de outro tipo cede ao
                // valor que vem da base.
                self.ext.retain(|c| {
                    !(c.name == cap.name && matches!(c.val, ExtVal::Str(Str::Cancelled)))
                });
                self.ext.push(cap.clone());
            }
        }
    }

    /// As estendidas por tipo (booleanos, números, cadeias) e, dentro do tipo, em ordem de nome.
    fn sorted_ext(&self) -> [Vec<&ExtCap>; 3] {
        let mut b: Vec<&ExtCap> = Vec::new();
        let mut n: Vec<&ExtCap> = Vec::new();
        let mut s: Vec<&ExtCap> = Vec::new();
        for c in &self.ext {
            match c.val {
                ExtVal::Bool(_) => b.push(c),
                ExtVal::Num(_) => n.push(c),
                ExtVal::Str(_) => s.push(c),
            }
        }
        b.sort_by(|x, y| x.name.cmp(&y.name));
        n.sort_by(|x, y| x.name.cmp(&y.name));
        s.sort_by(|x, y| x.name.cmp(&y.name));
        [b, n, s]
    }
}

/// A entrada como o `Dump` do `infocmp` a enxerga: as estendidas depois das predefinidas, em ordem
/// de tipo (booleanos, números, cadeias) e, dentro do tipo, na ordem do fonte.
fn to_termtype(e: &Entry) -> TermType {
    let mut tt = TermType::empty();
    tt.names = e.names.clone();
    tt.bools = e.bools.clone();
    tt.nums = e.nums.clone();
    tt.strs = e.strs.clone();
    let mut names: Vec<Vec<u8>> = Vec::new();
    let [sb, sn, ss] = e.sorted_ext();
    for c in sb {
        if let ExtVal::Bool(b) = c.val {
            tt.bools.push(b);
            tt.ext_bools += 1;
            names.push(c.name.clone());
        }
    }
    for c in sn {
        if let ExtVal::Num(n) = c.val {
            tt.nums.push(n);
            tt.ext_nums += 1;
            names.push(c.name.clone());
        }
    }
    for c in ss {
        if let ExtVal::Str(s) = &c.val {
            tt.strs.push(s.clone());
            tt.ext_strs += 1;
            names.push(c.name.clone());
        }
    }
    tt.ext_names = names;
    tt
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

/// Posição (linha, coluna) logo depois de consumir um byte, como o `next_char` conta: a coluna
/// cresce de um a cada caractere (o tab vale um) e volta a zero a cada linha nova.
type Pos = (usize, usize);

/// O texto cru de uma entrada: as linhas unidas (a continuação vira um espaço), a posição de cada
/// byte e o bloco de comentários que vinha antes.
struct Raw {
    body: Vec<u8>,
    pos: Vec<Pos>,
    line: usize,
    comment: Vec<u8>,
    /// A fonte termina em `\n`.
    eof_newline: bool,
}

/// Um campo terminado por vírgula: o texto e onde o scanner estava ao consumir a vírgula.
#[derive(Clone)]
struct Field {
    text: Vec<u8>,
    end: Pos,
    /// Índice em `Raw::body` onde o texto do campo começa.
    start: usize,
    /// O campo acabou no fim do arquivo, sem a vírgula.
    unterminated: bool,
}

/// Separa os campos por vírgulas não escapadas. Um último campo sem vírgula é descartado.
fn split_fields(raw: &Raw) -> Vec<Field> {
    let body = &raw.body;
    let mut fields: Vec<Field> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut i = 0;
    let mut start = 0;
    while i < body.len() {
        if body[i] == b'\\' && i + 1 < body.len() {
            cur.push(body[i]);
            cur.push(body[i + 1]);
            i += 2;
            continue;
        }
        if body[i] == b',' {
            fields.push(Field {
                text: std::mem::take(&mut cur),
                end: raw.pos[i],
                start,
                unterminated: false,
            });
            start = i + 1;
        } else {
            cur.push(body[i]);
        }
        i += 1;
    }
    // Uma capacidade no fim do arquivo sem vírgula ainda vale, com um aviso (o scanner avisa depois de
    // ler o fim do arquivo: uma coluna adiante do último caractere, duas se não há `\n` no fim).
    if !fields.is_empty() && !trim_start(&cur).is_empty() {
        if let Some(&(l, c)) = raw.pos.last() {
            fields.push(Field {
                text: cur,
                end: (l, c + if raw.eof_newline { 1 } else { 2 }),
                start,
                unterminated: true,
            });
        }
    }
    fields
}

/// Divide a fonte em entradas: linha que começa em coluna 0 abre uma, as que começam com espaço a
/// continuam, `#` em coluna 0 é comentário e vai pra frente da próxima entrada (com as linhas em
/// branco que ficam entre dois comentários). Devolve também a posição do fim do arquivo, onde o
/// scanner para quando a resolução dos `use=` reclama.
fn split_entries(data: &[u8]) -> (Vec<Raw>, Pos) {
    let mut out: Vec<Raw> = Vec::new();
    let mut cur: Option<Raw> = None;
    let mut pending: Vec<u8> = Vec::new();
    let mut gap: Vec<u8> = Vec::new();
    let mut last_line = 0usize;
    let mut last_col = 0usize;
    let mut lines: Vec<&[u8]> = data.split(|b| *b == b'\n').collect();
    if data.is_empty() || data.last() == Some(&b'\n') {
        lines.pop();
    }
    for (n, line) in lines.iter().enumerate() {
        let lineno = n + 1;
        last_line = lineno;
        if line.first() == Some(&b'#') {
            if !pending.is_empty() {
                pending.extend_from_slice(&gap);
            }
            gap.clear();
            pending.extend_from_slice(line);
            pending.push(b'\n');
            last_col = 0;
            continue;
        }
        last_col = line.len() + 1;
        if line.iter().all(|b| super::c_isspace(*b)) {
            if !pending.is_empty() {
                gap.extend_from_slice(line);
                gap.push(b'\n');
            }
            continue;
        }
        if super::c_isspace(line[0]) {
            if let Some(r) = cur.as_mut() {
                r.body.push(b' ');
                r.pos.push((lineno, 0));
                for (i, b) in line.iter().enumerate() {
                    r.body.push(*b);
                    r.pos.push((lineno, i + 1));
                }
            }
        } else {
            if let Some(r) = cur.take() {
                out.push(r);
            }
            gap.clear();
            let mut r = Raw {
                body: Vec::new(),
                pos: Vec::new(),
                line: lineno,
                comment: std::mem::take(&mut pending),
                eof_newline: data.last() == Some(&b'\n'),
            };
            for (i, b) in line.iter().enumerate() {
                r.body.push(*b);
                r.pos.push((lineno, i + 1));
            }
            cur = Some(r);
        }
    }
    if let Some(r) = cur.take() {
        out.push(r);
    }
    (out, (last_line, last_col))
}

/// A primeira linha que não é comentário nem está em branco, quando ela começa com espaço (a fonte
/// não abre com os nomes de um terminal em coluna um).
fn leading_indented_line(data: &[u8]) -> Option<(usize, &[u8])> {
    for (n, line) in data.split(|b| *b == b'\n').enumerate() {
        if line.first() == Some(&b'#') || line.iter().all(|b| super::c_isspace(*b)) {
            continue;
        }
        return if super::c_isspace(line[0]) {
            Some((n + 1, line))
        } else {
            None
        };
    }
    None
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

/// `unctrl` pro caractere que o scanner não aceita.
fn unctrl(c: u8) -> String {
    if (0x20..0x7f).contains(&c) {
        (c as char).to_string()
    } else if c == 0x7f {
        "^?".to_string()
    } else if c < 0x20 {
        format!("^{}", (c + b'@') as char)
    } else {
        format!("\\{c:03o}")
    }
}

/// `_nc_warning` (`comp_error.c`): `"arquivo", line N, col M, terminal 'x': mensagem`. Sem linha e
/// coluna (o `-1` do `tic -I`) saem só o arquivo e o terminal.
struct Diag {
    file: String,
    /// Um erro que o scanner marca como falha: o `tic` sai com 1 sem gravar nada.
    failed: std::cell::Cell<bool>,
}

impl Diag {
    fn warn(&self, line: Option<usize>, col: Option<usize>, term: &[u8], msg: &str) {
        let _ = io::flush_stdout();
        let mut s = format!("\"{}\"", self.file);
        if let Some(l) = line {
            s.push_str(&format!(", line {l}"));
        }
        if let Some(c) = col {
            s.push_str(&format!(", col {c}"));
        }
        let t = first_name(term);
        if !t.is_empty() {
            s.push_str(&format!(", terminal '{}'", io::lossy(t)));
        }
        s.push_str(": ");
        s.push_str(msg);
        s.push('\n');
        io::eprint(s);
    }
}

/// `parse_entry`: uma entrada da fonte.
fn parse_entry(raw: &Raw, xflag: bool, aflag: bool, diag: &Diag) -> Option<Entry> {
    let fields = split_fields(raw);
    let first = fields.first()?;
    let names = trim_end(trim_start(&first.text)).to_vec();
    if names.is_empty() {
        return None;
    }
    // O scanner já leu o caractere depois da vírgula do campo de nomes: se ele é o fim da linha, a
    // coluna avisada é a seguinte; se é outro caractere da mesma linha, fica na própria vírgula.
    let comma_idx = first.start + first.text.len();
    let at_eol = match raw.pos.get(comma_idx + 1) {
        Some(&(l, _)) => l != first.end.0,
        None => true,
    };
    let (nline, ncol) = (first.end.0, if at_eol { first.end.1 + 1 } else { first.end.1 });
    // `check_name`/`_nc_parse_entry`: um nome com barra não vale (a descrição, depois do último
    // `|`, pode ter barra), e uma descrição sem espaço pode ser tomada por apelido pelos tics antigos.
    let name_part: &[u8] = match names.iter().rposition(|b| *b == b'|') {
        Some(p) => &names[..p],
        None => &names,
    };
    if name_part.contains(&b'/') {
        diag.warn(
            Some(nline),
            Some(ncol),
            &names,
            "slashes aren't allowed in names or aliases",
        );
        diag.warn(
            Some(nline),
            Some(ncol),
            &names,
            &format!("invalid entry name \"{}\"", io::lossy(first_name(&names))),
        );
    }
    if let Some(p) = names.iter().rposition(|b| *b == b'|') {
        if !names[p + 1..].contains(&b' ') {
            diag.warn(
                Some(nline),
                Some(ncol),
                &names,
                "older tic versions may treat the description field as an alias",
            );
        }
    }
    let mut e = Entry::new(names);
    e.comment = raw.comment.clone();
    e.line = raw.line;
    e.keep_obsolete = xflag;
    let mut queue: Vec<Field> = fields[1..].to_vec();
    let mut qi = 0;
    while qi < queue.len() {
        let fld = queue[qi].clone();
        qi += 1;
        let f: &[u8] = trim_start(&fld.text);
        if f.is_empty() {
            continue;
        }
        // Um nome com ponto na frente é uma capacidade comentada: vale só com `-a`, e então fica
        // como estendida com o ponto no nome.
        let mut dotted = false;
        if f[0] == b'.' {
            if !aflag {
                continue;
            }
            if f.len() == 1 {
                continue;
            }
            dotted = true;
        }
        let (line, col) = fld.end;
        let term = e.names.clone();
        let warn = |msg: String| diag.warn(Some(line), Some(col), &term, &msg);
        let c0 = if dotted { f[1] } else { f[0] };
        if !(c0.is_ascii_alphanumeric() || c0 == b'_' || b"@%&*!#".contains(&c0)) {
            // O scanner leu o caractere ilegal: a coluna avisada é a do caractere mais um.
            let off = f.as_ptr() as usize - fld.text.as_ptr() as usize;
            let (il, ic) = raw.pos[fld.start + off];
            diag.warn(
                Some(il),
                Some(if c0 == b':' { ic } else { ic + 1 }),
                &term,
                &format!(
                    "Illegal character (expected alphanumeric or @%&*!#) - '{}'",
                    unctrl(c0)
                ),
            );
            continue;
        }
        // Um ':' no lugar do separador do nome da capacidade é sintaxe termcap misturada.
        if let Some(p) = f
            .iter()
            .position(|b| matches!(*b, b'=' | b'#' | b'@' | b':'))
        {
            if f[p] == b':' {
                let off = (f.as_ptr() as usize - fld.text.as_ptr() as usize) + p;
                let (l, c) = raw.pos[fld.start + off];
                diag.warn(Some(l), Some(c), &term, "Separator inconsistent with syntax");
                diag.failed.set(true);
                continue;
            }
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
        if fld.unterminated {
            warn(format!("Missing separator for `{nm}'"));
        }
        // O tipo avisado é o da capacidade real, não o usado na fonte.
        let real_kind = || {
            if find_type_entry(name, Kind::Bool).is_some() {
                "boolean"
            } else if find_type_entry(name, Kind::Num).is_some() {
                "numeric"
            } else {
                "string"
            }
        };
        let wrong_type = || format!("wrong type used for {} capability '{nm}'", real_kind());
        let unknown = || format!("unknown capability '{nm}'");
        match sep {
            0 => {
                if let Some(i) = find_type_entry(name, Kind::Bool) {
                    e.bools[i] = 1;
                } else if find_type_entry(name, Kind::Num).is_some()
                    || find_type_entry(name, Kind::Str).is_some()
                {
                    warn(wrong_type());
                } else if xflag {
                    set_ext(&mut e, name, ExtVal::Bool(1));
                } else {
                    warn(unknown());
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
                    set_ext(&mut e, name, ExtVal::Str(Str::Cancelled));
                } else {
                    warn(unknown());
                }
            }
            b'#' => {
                if let Some(&bad) = value.first() {
                    if !bad.is_ascii_digit() && !super::c_isspace(bad) {
                        // O scanner só lê dígitos: sem nenhum, a capacidade fica com 0 e o resto do
                        // campo (depois do primeiro caractere) vira outro token.
                        let off = (value.as_ptr() as usize) - (fld.text.as_ptr() as usize);
                        let (bl, bc) = raw.pos[fld.start + off];
                        diag.warn(Some(bl), Some(bc), &term, &format!("no value given for `{nm}'"));
                        diag.warn(
                            Some(bl),
                            Some(bc),
                            &term,
                            &format!("Missing separator for `{nm}'"),
                        );
                        if let Some(i) = find_type_entry(name, Kind::Num) {
                            e.nums[i] = 0;
                        }
                        if value.len() > 1 {
                            queue.insert(
                                qi,
                                Field {
                                    text: value[1..].to_vec(),
                                    end: fld.end,
                                    start: fld.start + off + 1,
                                    unterminated: false,
                                },
                            );
                        }
                        continue;
                    }
                }
                let (v, _) = strtol(trim_start(value));
                let v = v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
                if let Some(i) = find_type_entry(name, Kind::Num) {
                    e.nums[i] = v;
                } else if find_type_entry(name, Kind::Bool).is_some()
                    || find_type_entry(name, Kind::Str).is_some()
                {
                    warn(wrong_type());
                } else if xflag {
                    set_ext(&mut e, name, ExtVal::Num(v));
                } else {
                    warn(unknown());
                }
            }
            _ => {
                let v = trans_string(value);
                if let Some(i) = find_type_entry(name, Kind::Str) {
                    e.strs[i] = Str::Val(v);
                } else if find_type_entry(name, Kind::Bool).is_some()
                    || find_type_entry(name, Kind::Num).is_some()
                {
                    warn(wrong_type());
                } else if xflag {
                    set_ext(&mut e, name, ExtVal::Str(Str::Val(v)));
                } else {
                    warn(unknown());
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

/// O problema que a resolução dos `use=` encontrou: o terminal e a mensagem do `_nc_warning`.
struct ResolveErr {
    term: Vec<u8>,
    /// As mensagens, na ordem em que o `_nc_resolve_uses2` as emite.
    msgs: Vec<String>,
    /// Uma referência circular: cada nível acima acrescenta o seu `problem with use=`.
    circular: bool,
}

/// `_nc_resolve_uses2`: aplica os `use=` da entrada `idx`, recursivamente. Os `use=` valem na ordem
/// em que aparecem, e o que veio antes prevalece.
fn resolve(
    entries: &mut Vec<Entry>,
    done: &mut Vec<bool>,
    idx: usize,
    stack: &mut Vec<usize>,
) -> Result<(), ResolveErr> {
    if done[idx] {
        return Ok(());
    }
    stack.push(idx);
    let uses = entries[idx].uses.clone();
    let mut merged = entries[idx].clone();
    for u in &uses {
        // Uma entrada não é candidata ao próprio `use=`: ele cai no banco instalado.
        let found = (0..entries.len()).find(|j| *j != idx && name_match(&entries[*j].names, u));
        let base = match found {
            Some(j) => {
                if stack.contains(&j) {
                    return Err(ResolveErr {
                        term: entries[idx].names.clone(),
                        msgs: vec![
                            format!("problem with use={}", io::lossy(u)),
                            "merge failed, infinite loop".to_string(),
                        ],
                        circular: true,
                    });
                }
                if let Err(mut er) = resolve(entries, done, j, stack) {
                    if er.circular {
                        er.msgs.insert(0, format!("problem with use={}", io::lossy(u)));
                    }
                    return Err(er);
                }
                entries[j].clone()
            }
            None => match lookup_db(u) {
                Some(b) => b,
                None => {
                    return Err(ResolveErr {
                        term: entries[idx].names.clone(),
                        msgs: vec![format!("resolution of use={} failed", io::lossy(u))],
                        circular: false,
                    });
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
fn compile(entry: &Entry) -> Vec<u8> {
    // As predefinidas que o formato legado não grava (`OTbs`, `OTug`, `OTi2`...) viajam como
    // estendidas, com o nome terminfo delas.
    let mut full = entry.clone();
    if entry.keep_obsolete {
        for i in BOOLWRITE..BOOLCOUNT.min(entry.bools.len()) {
            if entry.bools[i] == 1 {
                full.ext.push(ExtCap {
                    name: BOOLS[i].info.as_bytes().to_vec(),
                    val: ExtVal::Bool(1),
                });
            }
        }
        for i in NUMWRITE..NUMCOUNT.min(entry.nums.len()) {
            if entry.nums[i] >= 0 {
                full.ext.push(ExtCap {
                    name: NUMS[i].info.as_bytes().to_vec(),
                    val: ExtVal::Num(entry.nums[i]),
                });
            }
        }
        for i in STRWRITE..STRCOUNT.min(entry.strs.len()) {
            if entry.strs[i].valid() {
                full.ext.push(ExtCap {
                    name: STRS[i].info.as_bytes().to_vec(),
                    val: ExtVal::Str(entry.strs[i].clone()),
                });
            }
        }
    }
    let e = &full;
    let [ext_b, ext_n, ext_s] = e.sorted_ext();
    // Só os números predefinidos decidem o formato; os estendidos seguem o formato escolhido.
    let wide = e.nums.iter().any(|n| *n > 0x7fff);

    // Só o booleano verdadeiro conta e é gravado como 1 (um cancelado vira 0).
    let bool_count = (0..BOOLWRITE.min(e.bools.len()))
        .rev()
        .find(|i| e.bools[*i] == 1)
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
        out.push(u8::from(*b == 1));
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
        let mut names_part: Vec<u8> = Vec::new();
        for c in ext_b.iter().chain(ext_n.iter()).chain(ext_s.iter()) {
            name_offsets.push(names_part.len() as i32);
            names_part.extend_from_slice(&c.name);
            names_part.push(0);
        }
        etable.extend_from_slice(&names_part);

        push16(&mut out, ext_b.len() as i32);
        push16(&mut out, ext_n.len() as i32);
        push16(&mut out, ext_s.len() as i32);
        // O ncurses não conta o valor de uma string cancelada no limite (golden `tic-extended-cancel`).
        let live_strs = ext_s.iter().filter(|c| matches!(c.val, ExtVal::Str(Str::Val(_)))).count();
        push16(&mut out, (ext_b.len() + ext_n.len() + ext_s.len() + live_strs) as i32);
        push16(&mut out, etable.len() as i32);
        for c in &ext_b {
            if let ExtVal::Bool(b) = c.val {
                out.push(u8::from(b == 1));
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

/// `_nc_set_writedir`: o diretório de saída existe (ou é criado), e é um diretório.
fn ensure_dir(path: &[u8], file: &str, term: &[u8]) -> Result<(), String> {
    match sys::stat(path) {
        Ok(st) => {
            if matches!(st.file_type(), FileType::Directory) {
                Ok(())
            } else {
                Err(format!(
                    "\"{}\", line 1, terminal '{}': {}: (errno {}) {}",
                    file,
                    io::lossy(term),
                    io::lossy(path),
                    Errno::EPERM.0,
                    Errno::EPERM.message()
                ))
            }
        }
        Err(_) => match mkdir_p(path) {
            Ok(()) => Ok(()),
            Err(er) => Err(format!(
                "\"{}\", line 1, terminal '{}': {}: (errno {}) {}",
                file,
                io::lossy(term),
                io::lossy(path),
                er.0,
                er.message()
            )),
        },
    }
}

/// `_nc_write_entry`: grava `dir/c/nome` e liga os apelidos a ele.
fn write_entry(dir: &[u8], e: &Entry, progname: &str, diag: &Diag) -> Result<(), String> {
    let primary = first_name(&e.names).to_vec();
    if primary.is_empty() {
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
    if mkdir_p(&sub).is_err() {
        // Segue: o `open` abaixo dá o erro certo.
    }
    let mut path = sub.clone();
    path.push(b'/');
    path.extend_from_slice(&primary);
    let _ = sys::current().unlinkat(Fd::CWD, &path, AtFlags::empty());
    let fd = sys::open(&path, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, 0o644).map_err(|er| {
        // `_nc_syserr_abort` do `write_entry.c`, com a linha em que a entrada começa.
        format!(
            "\"{}\", line {}, terminal '{}': cannot open {}: (errno {}) {}",
            diag.file,
            e.line,
            io::lossy(&primary),
            io::lossy(&path),
            er.0,
            er.message()
        )
    })?;
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
                // O alvo do link simbólico é relativo ao diretório do apelido.
                let target = if alias[0] == primary[0] {
                    primary.clone()
                } else {
                    let mut t = b"../".to_vec();
                    t.push(primary[0]);
                    t.push(b'/');
                    t.extend_from_slice(&primary);
                    t
                };
                let _ = sys::current().symlinkat(&target, Fd::CWD, &apath);
            }
        }
    }
    Ok(())
}

/// `nametrans` do `tic.c`: o nome terminfo de uma capacidade vira o nome termcap.
fn nametrans(name: &[u8]) -> Option<&'static str> {
    if let Some(c) = BOOLS.iter().find(|c| c.info.as_bytes() == name) {
        return Some(c.tc);
    }
    if let Some(c) = NUMS.iter().find(|c| c.info.as_bytes() == name) {
        return Some(c.tc);
    }
    STRS.iter().find(|c| c.info.as_bytes() == name).map(|c| c.tc)
}

/// `put_translate`: os comentários da fonte saem com `<nome>` trocado pelo nome termcap, entre
/// dois-pontos, quando a saída é termcap. O estado atravessa as entradas, como o `static` do original.
#[derive(Default)]
struct Translate {
    in_name: bool,
    buf: Vec<u8>,
}

impl Translate {
    fn feed(&mut self, text: &[u8], out: &mut Vec<u8>) {
        for &c in text {
            if self.in_name {
                if c == b'\n' || c == b'@' {
                    out.push(b'<');
                    out.extend_from_slice(&self.buf);
                    out.push(c);
                    self.in_name = false;
                } else if c != b'>' {
                    self.buf.push(c);
                } else {
                    self.in_name = false;
                    let mut name = std::mem::take(&mut self.buf);
                    let mut suffix: Vec<u8> = Vec::new();
                    let up = name
                        .iter()
                        .position(|b| *b == b'#' || *b == b'=')
                        .or_else(|| {
                            name.iter()
                                .position(|b| *b == b'@')
                                .filter(|p| name.get(p + 1) == Some(&b'>'))
                        });
                    if let Some(p) = up {
                        suffix = name.split_off(p);
                    }
                    match nametrans(&name) {
                        Some(tp) => {
                            out.push(b':');
                            out.extend_from_slice(tp.as_bytes());
                            out.extend_from_slice(&suffix);
                            out.push(b':');
                        }
                        None => {
                            out.push(b'<');
                            out.extend_from_slice(&name);
                            out.extend_from_slice(&suffix);
                            out.push(b'>');
                        }
                    }
                }
            } else {
                self.buf.clear();
                if c == b'<' {
                    self.in_name = true;
                } else {
                    out.push(c);
                }
            }
        }
    }
}

/// `matches`: a entrada está na lista do `-e` (ou não há lista)?
fn matches_list(list: &Option<Vec<Vec<u8>>>, names: &[u8]) -> bool {
    match list {
        None => true,
        Some(l) => l.iter().any(|n| name_match(names, n)),
    }
}

/// `make_namelist`: o argumento do `-e` é uma lista separada por vírgulas, ou um arquivo.
fn make_namelist(src: &[u8]) -> Vec<Vec<u8>> {
    let text: Vec<u8> = if src.contains(&b'/') {
        sys::read_file(src).unwrap_or_default()
    } else {
        src.to_vec()
    };
    text.split(|b| *b == b',' || super::c_isspace(*b))
        .filter(|s| !s.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

fn usage(progname: &str) -> ! {
    const OPTIONS: &[&str] = &[
        "  -0         format translation output all capabilities on one line",
        "  -1         format translation output one capability per line",
        "  -a         retain commented-out capabilities (sets -x also)",
        "  -C         translate entries to termcap source form",
        "  -D         print list of tic's database locations (first must be writable)",
        "  -c         check only, validate input without compiling or translating",
        "  -e<names>  translate/compile only entries named by comma-separated list",
        "  -f         format complex strings for readability",
        "  -G         format %{number} to %'char'",
        "  -g         format %'char' to %{number}",
        "  -I         translate entries to terminfo source form",
        "  -K         translate entries to termcap source form with BSD syntax",
        "  -L         translate entries to full terminfo source form",
        "  -N         disable smart defaults for source translation",
        "  -o<dir>    set output directory for compiled entry writes",
        "  -Q[n]      dump compiled description",
        "  -q    brief listing, removes headers",
        "  -R<name>   restrict translation to given terminfo/termcap version",
        "  -r         force resolution of all use entries in source translation",
        "  -s         print summary statistics",
        "  -T         remove size-restrictions on compiled description",
        "  -t         suppress commented-out capabilities",
        "  -U         suppress post-processing of entries",
        "  -V         print version",
        "  -W         wrap long strings according to -w[n] option",
        "  -v[n]      set verbosity level",
        "  -w[n]      set format width for translation output",
        "  -x         treat unknown capabilities as user-defined",
    ];
    let mut text = format!("Usage: {progname} {USAGE_SYNOPSIS}\n\nOptions:\n");
    for line in OPTIONS {
        text.push_str(line);
        text.push('\n');
    }
    text.push_str("\nParameters:\n  <file>     file to translate or compile\n");
    io::eprint(text);
    sys::exit(1)
}

/// A sinopse que o `tic` repete nas mensagens curtas e no texto de uso completo.
const USAGE_SYNOPSIS: &str = "[-e names] [-o dir] [-R name] [-v[n]] [-V] [-w[n]] [-1aCDcfGgIKLNrsTtUx] source-file";

/// `_nc_err_abort` do `tic.c`: a mensagem curta seguida da sinopse, e saída com 1.
fn usage_short(progname: &str, msg: &str) -> ! {
    io::eprint(format!(
        "{progname}: {msg}.  Usage:\n\t{progname} {USAGE_SYNOPSIS}\n"
    ));
    sys::exit(1)
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let progname: String = io::lossy(rootname(&argv[0])).to_string();

    // Os links `captoinfo` e `infotocap` são o mesmo programa em modo texto.
    let mut infodump = progname == "captoinfo";
    let mut capdump = progname == "infotocap";
    let mut outform = if capdump {
        OutForm::Termcap
    } else {
        OutForm::Terminfo
    };
    let mut sortmode = if capdump {
        SortMode::Termcap
    } else if infodump {
        SortMode::Terminfo
    } else {
        SortMode::Default
    };
    let mut tversion: Option<String> = None;
    let mut width: i32 = 60;
    let mut height: i32 = 65535;
    let mut v_opt: i32 = -1;
    let mut v_seen = false;
    let mut last_opt = '?';
    let mut formatted = false;
    let mut literal = false;
    let mut smart_defaults = true;
    let mut numbers: i32 = 0;
    let mut limited = true;
    let mut forceresolve = false;
    let mut wrap_strings = false;
    let mut quickdump = 0;
    let mut quiet = false;
    let mut showsummary = false;
    let mut strict_bsd = false;
    let mut suppress_untranslatable = false;
    let mut xflag = false;
    let mut aflag = false;
    let mut check_only = false;
    let mut outdir: Option<Vec<u8>> = None;
    let mut namelst: Option<Vec<Vec<u8>>> = None;

    let mut g = Getopt::from_env(
        &argv[1..],
        "0123456789CDIKLNQ:R:TUVWace:fGgo:qrstvwx",
        &[],
    );
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                usage(&progname)
            }
        };
        let arg = o.arg.clone().unwrap_or_default();
        let c = o.short().unwrap_or('?');
        if let Some(d) = c.to_digit(10) {
            // Os dígitos depois de `-v` e `-w` formam o número; os demais só valem como `-0` e `-1`.
            let d = d as i32;
            match last_opt {
                'v' => v_opt = v_opt.saturating_mul(10).saturating_add(d),
                'w' => width = width.saturating_mul(10).saturating_add(d),
                _ => {
                    if d != 0 && d != 1 {
                        usage(&progname);
                    }
                    last_opt = c;
                    if d == 0 {
                        width = 65535;
                        height = 1;
                    } else {
                        width = 0;
                        height = 65535;
                    }
                }
            }
            continue;
        }
        match c {
            'K' | 'C' => {
                if c == 'K' {
                    strict_bsd = true;
                }
                capdump = true;
                outform = OutForm::Termcap;
                // Só o `-K` restringe ao subconjunto BSD; o `-C` mantém todas as capacidades.
                if c == 'K' {
                    tversion = Some("BSD".to_string());
                }
                if sortmode == SortMode::Default {
                    sortmode = SortMode::Termcap;
                }
            }
            'D' => {
                let mut out = io::stdout();
                // Com `TERMINFO` definido, os diretórios de sistema não entram na lista.
                let from_env = sys::getenv("TERMINFO").is_some_and(|v| !v.is_empty());
                for d in db_candidates(None) {
                    if from_env
                        && matches!(
                            d.as_slice(),
                            b"/etc/terminfo" | b"/lib/terminfo" | b"/usr/share/terminfo"
                        )
                    {
                        continue;
                    }
                    let _ = out.write_all(&d);
                    let _ = out.write_all(b"\n");
                }
                return 0;
            }
            'I' => {
                infodump = true;
                outform = OutForm::Terminfo;
                if sortmode == SortMode::Default {
                    sortmode = SortMode::Terminfo;
                }
                tversion = None;
            }
            'L' => {
                infodump = true;
                outform = OutForm::Variable;
                if sortmode == SortMode::Default {
                    sortmode = SortMode::Variable;
                }
                tversion = None;
            }
            'N' => {
                smart_defaults = false;
                literal = true;
            }
            'Q' => quickdump = strtol(&arg).0 as i32,
            'R' => tversion = Some(io::lossy(&arg).to_string()),
            'T' => limited = false,
            'U' => literal = true,
            'V' => {
                let _ = writeln!(io::stdout(), "{VERSION}");
                return 0;
            }
            'W' => wrap_strings = true,
            'a' => {
                aflag = true;
                xflag = true;
            }
            'c' => check_only = true,
            'e' => namelst = Some(make_namelist(&arg)),
            'f' => formatted = true,
            'G' => numbers = 1,
            'g' => numbers = -1,
            'o' => outdir = o.arg.clone(),
            'q' => quiet = true,
            'r' => forceresolve = true,
            's' => showsummary = true,
            't' => suppress_untranslatable = true,
            'v' => {
                v_opt = 0;
                v_seen = true;
            }
            'w' => width = 0,
            'x' => xflag = true,
            _ => usage(&progname),
        }
        last_opt = c;
    }
    let _ = (literal, smart_defaults, quiet);

    let operands = g.operands();
    if operands.len() > 1 {
        usage_short(&progname, "Too many file names");
    }
    let file: Vec<u8> = match operands.first() {
        Some(f) => f.clone(),
        None => {
            if progname == "captoinfo" {
                b"/etc/termcap".to_vec()
            } else {
                usage_short(&progname, "File name needed");
            }
        }
    };
    let data = if file.as_slice() == b"-" {
        sys::read_to_end(Fd::STDIN)
    } else {
        sys::read_file(&file)
    };
    let data = match data {
        Ok(d) => d,
        Err(_) => {
            // `perror(source_file)`: o motivo vem de um `open` que repete o que falhou.
            let reason = match sys::open(&file, OFlags::RDONLY, 0) {
                Err(er) => er.message().to_string(),
                Ok(fd) => {
                    let _ = sys::close(fd);
                    "Is a directory".to_string()
                }
            };
            let _ = io::flush_stdout();
            io::eprint(format!(
                "{progname}: cannot open '{}': {reason}\n",
                io::lossy(&file)
            ));
            return 1;
        }
    };
    let file_name: String = if file.as_slice() == b"-" {
        "<stdin>".to_string()
    } else {
        io::lossy(&file).to_string()
    };
    let diag = Diag {
        file: file_name,
        failed: std::cell::Cell::new(false),
    };
    let text_mode = infodump || capdump;

    // Uma fonte cuja primeira linha útil começa com espaço não tem nomes na coluna um: o scanner
    // a toma por lista de capacidades e para no primeiro caractere ilegal.
    if let Some((lineno, line)) = leading_indented_line(&data) {
        let start = line.iter().position(|b| !super::c_isspace(*b)).unwrap_or(0);
        let bad = line[start..]
            .iter()
            .position(|b| !(b.is_ascii_alphanumeric() || *b == b'_' || b"@%&*!#".contains(b)));
        let col = match bad {
            Some(p) => {
                let col = start + p + 1;
                diag.warn(
                    Some(lineno),
                    Some(col),
                    b"",
                    &format!("Illegal character - '{}'", unctrl(line[start + p])),
                );
                col
            }
            None => line.len() + 1,
        };
        diag.warn(
            Some(lineno),
            Some(col),
            b"",
            "Entry does not start with terminal names in column one",
        );
        return 1;
    }

    let (raws, eof_pos) = split_entries(&data);
    let mut entries: Vec<Entry> = Vec::new();
    for raw in &raws {
        match parse_entry(raw, xflag, aflag, &diag) {
            Some(e) => entries.push(e),
            None => {
                let _ = io::flush_stdout();
                io::eprint(format!(
                    "\"{}\", line {}: unexpected end of entry\n",
                    diag.file, raw.line
                ));
                return 1;
            }
        }
    }

    if diag.failed.get() {
        return 1;
    }

    // `-I`, `-L` e `-C` só resolvem os `use=` com `-r`; compilar e conferir sempre resolvem.
    if check_only || !text_mode || forceresolve {
        let mut done = vec![false; entries.len()];
        for i in 0..entries.len() {
            let mut stack = Vec::new();
            if let Err(re) = resolve(&mut entries, &mut done, i, &mut stack) {
                // Sem coluna: o `_nc_warning` sai com a linha onde o scanner parou (o fim da fonte).
                for m in &re.msgs {
                    diag.warn(Some(eof_pos.0), None, &re.term, m);
                }
                // Com `-c` o `tic` só avisa e segue conferindo as demais entradas.
                if check_only {
                    continue;
                }
                return 1;
            }
        }
    }

    let mut dump = Dump::new(
        tversion.as_deref(),
        outform,
        sortmode,
        wrap_strings,
        width,
        height,
        if v_opt > 0 { v_opt as u32 } else { 0 },
        formatted,
        check_only,
        quickdump,
        &progname,
    );
    dump.user_definable = xflag;
    dump.strict_bsd = strict_bsd;

    if check_only && text_mode {
        let limit = if infodump {
            MAX_TERMINFO_LENGTH
        } else {
            MAX_TERMCAP_LENGTH
        };
        for e in &entries {
            if !matches_list(&namelst, &e.names) {
                continue;
            }
            let mut tt = to_termtype(e);
            let pred: PredFn<'_> = &dump_predicate;
            // O `dump_entry` solta os avisos "(... removed to fit entry within N bytes)" no stdout.
            if !infodump {
                let mut probe = tt.clone();
                dump.dump_entry(&mut probe, suppress_untranslatable, limited, numbers, pred);
            }
            let len = dump.fmt_entry(&mut tt, pred, true, true, infodump, numbers);
            if len > limit {
                let _ = io::flush_stdout();
                io::eprint(format!(
                    "tic: resolved {} entry is {} bytes long\n",
                    io::lossy(first_name(&e.names)),
                    len
                ));
            }
        }
    }
    if check_only {
        return 0;
    }

    if !text_mode {
        let dir = output_dir(outdir.as_deref());
        let first_term = entries.first().map_or(&b""[..], |e| first_name(&e.names));
        if let Err(msg) = ensure_dir(&dir, &diag.file, first_term) {
            let _ = io::flush_stdout();
            io::eprint(format!("{msg}\n"));
            return 1;
        }
        // `_nc_set_writedir` guarda o caminho absoluto depois de conferir o diretório.
        let dir = if dir.first() == Some(&b'/') {
            dir
        } else {
            match sys::current().getcwd() {
                Ok(mut cwd) => {
                    if cwd.last() != Some(&b'/') {
                        cwd.push(b'/');
                    }
                    cwd.extend_from_slice(&dir);
                    cwd
                }
                Err(_) => dir,
            }
        };
        let mut written = 0usize;
        let mut written_names: Vec<Vec<u8>> = Vec::new();
        for e in &entries {
            if !matches_list(&namelst, &e.names) {
                continue;
            }
            // Com `-v`, as conferências de `_nc_check_termtype2` avisam o que faz falta.
            if v_seen {
                let has = |n: &[u8]| {
                    find_type_entry(n, Kind::Str).is_some_and(|i| e.strs[i].valid())
                };
                if !(has(b"cup") || (has(b"hpa") && has(b"vpa"))) {
                    diag.warn(
                        Some(e.line),
                        None,
                        &e.names,
                        "terminal lacks cursor addressing",
                    );
                }
            }
            // `_nc_write_entry`: um nome já gravado nesta execução é definido duas vezes.
            let primary = first_name(&e.names).to_vec();
            if written_names.contains(&primary) {
                diag.warn(Some(e.line), None, &e.names, "name multiply defined.");
            }
            written_names.push(primary);
            if let Err(msg) = write_entry(&dir, e, &progname, &diag) {
                let _ = io::flush_stdout();
                io::eprint(format!("{msg}\n"));
                return 1;
            }
            written += 1;
        }
        if showsummary && written != 0 {
            let _ = io::flush_stdout();
            io::eprint(format!(
                "{written} entries written to {}\n",
                io::lossy(&dir)
            ));
        }
        return 0;
    }

    // O `-K` (termcap BSD estrito) do ncurses 6.5 sai com 0 sem escrever nada (golden `tic-text-K-bsd-termcap`).
    if strict_bsd {
        return 0;
    }

    let mut tr = Translate::default();
    for e in &entries {
        if !matches_list(&namelst, &e.names) {
            continue;
        }
        if infodump && quickdump & 1 != 0 {
            let mut s = String::from("hex:");
            for b in compile(e) {
                s.push_str(&format!("{b:02X}"));
            }
            s.push('\n');
            let _ = io::stdout().write_all(s.as_bytes());
            continue;
        }
        if infodump {
            let _ = io::stdout().write_all(&e.comment);
        } else {
            let mut buf: Vec<u8> = Vec::new();
            tr.feed(&e.comment, &mut buf);
            let _ = io::stdout().write_all(&buf);
        }
        let mut tt = to_termtype(e);
        repair_acsc(&mut tt);
        let pred: PredFn<'_> = &dump_predicate;
        dump.dump_entry(&mut tt, suppress_untranslatable, limited, numbers, pred);
        for u in &e.uses {
            dump.dump_uses(u, !capdump);
        }
        let _len = dump.show_entry();
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::terminfo::read_termtype;

    fn parse_one(src: &[u8], x: bool) -> Entry {
        let diag = Diag {
            file: "t".to_string(),
            failed: std::cell::Cell::new(false),
        };
        let (raws, _) = split_entries(src);
        parse_entry(&raws[0], x, false, &diag).unwrap()
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
        let diag = Diag {
            file: "t".to_string(),
            failed: std::cell::Cell::new(false),
        };
        let (raws, _) = split_entries(src);
        let mut entries: Vec<Entry> = raws
            .iter()
            .map(|r| parse_entry(r, false, false, &diag).unwrap())
            .collect();
        let mut done = vec![false; 2];
        for i in 0..2 {
            resolve(&mut entries, &mut done, i, &mut Vec::new())
                .map_err(|e| e.msgs)
                .unwrap();
        }
        assert_eq!(entries[1].nums[find_type_entry(b"cols", Kind::Num).unwrap()], 100);
        assert_eq!(entries[1].bools[find_type_entry(b"am", Kind::Bool).unwrap()], 1);
    }

    #[test]
    fn comments_go_with_the_next_entry() {
        let src = b"# um\n# dois\na|x,\n\tam,\n\nb|y,\n\tcols#80,\n";
        let (raws, eof) = split_entries(src);
        assert_eq!(raws.len(), 2);
        assert_eq!(raws[0].comment, b"# um\n# dois\n".to_vec());
        assert!(raws[1].comment.is_empty());
        assert_eq!(eof, (7, 10));
    }

    #[test]
    fn field_positions_follow_the_comma() {
        let (raws, _) = split_entries(b"t|x,\n  am, cols#80,\n");
        let fields = split_fields(&raws[0]);
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].end, (1, 4));
        assert_eq!(fields[1].end, (2, 5));
        assert_eq!(fields[2].end, (2, 14));
    }

    #[test]
    fn comment_translation_to_termcap() {
        let mut tr = Translate::default();
        let mut out = Vec::new();
        tr.feed(b"# usa <cup> e <zzz>\n", &mut out);
        assert_eq!(out, b"# usa :cm: e <zzz>\n".to_vec());
    }
}
