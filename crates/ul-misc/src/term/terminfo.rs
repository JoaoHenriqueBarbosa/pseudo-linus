//! O banco terminfo: o formato compilado (`read_entry.c`), a lista de diretórios de busca
//! (`db_iterator.c`), o `setupterm` (`lib_setup.c`) e os `tigetflag`/`tigetnum`/`tigetstr`
//! (`lib_ti.c`).

use sysabi::{Fd, FileType, sys};

use super::{
    ABSENT_NUMERIC, BOOLCOUNT, CANCELLED_NUMERIC, Kind, NUMCOUNT, STRCOUNT, bool_index, find_type_entry, num_index,
    str_index,
};

const MAGIC: i32 = 0o432;
const MAGIC2: i32 = 0o1036;
const MAX_ENTRY_SIZE: usize = 32768;
const MAX_ENTRY_SIZE1: usize = 4096;
const MAX_NAME_SIZE: usize = 512;

/// `TGETENT_*`.
pub const TGETENT_ERR: i32 = -1;
pub const TGETENT_NO: i32 = 0;
pub const TGETENT_YES: i32 = 1;

/// Uma capacidade de cadeia: ausente, cancelada (`name@`) ou com valor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Str {
    Absent,
    Cancelled,
    Val(Vec<u8>),
}

impl Str {
    /// `VALID_STRING`.
    pub fn valid(&self) -> bool {
        matches!(self, Str::Val(_))
    }

    pub fn val(&self) -> Option<&[u8]> {
        match self {
            Str::Val(v) => Some(v),
            _ => None,
        }
    }
}

/// `TERMTYPE2`: uma descrição de terminal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TermType {
    /// `term_names`: `nome|alias|descrição`.
    pub names: Vec<u8>,
    pub bools: Vec<i8>,
    pub nums: Vec<i32>,
    pub strs: Vec<Str>,
    pub ext_bools: usize,
    pub ext_nums: usize,
    pub ext_strs: usize,
    /// `ext_Names`: nomes dos booleanos, depois dos números, depois das cadeias estendidas.
    pub ext_names: Vec<Vec<u8>>,
}

impl TermType {
    /// Uma descrição vazia com as capacidades predefinidas ausentes (`_nc_init_termtype`).
    pub fn empty() -> TermType {
        TermType {
            names: Vec::new(),
            bools: vec![0; BOOLCOUNT],
            nums: vec![ABSENT_NUMERIC; NUMCOUNT],
            strs: vec![Str::Absent; STRCOUNT],
            ext_bools: 0,
            ext_nums: 0,
            ext_strs: 0,
            ext_names: Vec::new(),
        }
    }

    /// Nome do booleano estendido na posição `i` (`i >= BOOLCOUNT`).
    pub fn ext_bool_name(&self, i: usize) -> &[u8] {
        let base = self.bools.len() - self.ext_bools;
        self.ext_names.get(i - base).map_or(&b""[..], |v| v.as_slice())
    }

    pub fn ext_num_name(&self, i: usize) -> &[u8] {
        let base = self.nums.len() - self.ext_nums;
        self.ext_names.get(i - base + self.ext_bools).map_or(&b""[..], |v| v.as_slice())
    }

    pub fn ext_str_name(&self, i: usize) -> &[u8] {
        let base = self.strs.len() - self.ext_strs;
        self.ext_names.get(i - base + self.ext_bools + self.ext_nums).map_or(&b""[..], |v| v.as_slice())
    }

    /// Cadeia predefinida pelo nome da variável C (`clear_screen`...).
    pub fn s(&self, var: &str) -> &Str {
        static ABSENT: Str = Str::Absent;
        self.strs.get(str_index(var)).unwrap_or(&ABSENT)
    }

    /// Valor de uma cadeia predefinida, se válida.
    pub fn sv(&self, var: &str) -> Option<&[u8]> {
        self.s(var).val()
    }

    pub fn n(&self, var: &str) -> i32 {
        self.nums.get(num_index(var)).copied().unwrap_or(ABSENT_NUMERIC)
    }

    pub fn b(&self, var: &str) -> bool {
        self.bools.get(bool_index(var)).copied().unwrap_or(0) == 1
    }
}

/// Leitura sequencial do `fake_read`: devolve o que couber.
struct Rd<'a> {
    buf: &'a [u8],
    off: usize,
}

impl<'a> Rd<'a> {
    fn take(&mut self, want: usize) -> &'a [u8] {
        let have = self.buf.len().saturating_sub(self.off);
        let n = want.min(have);
        let out = &self.buf[self.off..self.off + n];
        self.off += n;
        out
    }
}

fn short(b: &[u8], i: usize) -> i32 {
    i32::from(i16::from_le_bytes([b[i], b[i + 1]]))
}

fn is_neg1(b: &[u8], i: usize) -> bool {
    b[i] == 0xff && b[i + 1] == 0xff
}

fn is_neg2(b: &[u8], i: usize) -> bool {
    b[i] == 0xfe && b[i + 1] == 0xff
}

fn convert_numbers(buf: &[u8], count: usize, wide: bool) -> Vec<i32> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        if wide {
            let b = &buf[4 * i..4 * i + 4];
            out.push(i32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        } else {
            out.push(short(buf, 2 * i));
        }
    }
    out
}

/// `convert_strings`: `offsets` são `count` shorts; devolve `None` se os dados estão corrompidos.
fn convert_strings(offsets: &[u8], count: usize, table: &[u8], size: usize, always: bool) -> Option<Vec<Str>> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let item = if is_neg1(offsets, 2 * i) {
            Str::Absent
        } else if is_neg2(offsets, 2 * i) {
            Str::Cancelled
        } else if short(offsets, 2 * i) > size as i32 {
            Str::Absent
        } else {
            let nn = short(offsets, 2 * i);
            if nn >= 0 && (nn as usize) < size {
                let start = nn as usize;
                let limit = size.min(table.len());
                // Sem NUL até o fim da tabela: a cadeia é ignorada.
                match table.get(start..limit).and_then(|s| s.iter().position(|b| *b == 0)) {
                    None => Str::Absent,
                    Some(0) if always => return None,
                    Some(p) => Str::Val(table[start..start + p].to_vec()),
                }
            } else {
                return None;
            }
        };
        if always && (is_neg1(offsets, 2 * i) || is_neg2(offsets, 2 * i) || short(offsets, 2 * i) > size as i32) {
            return None;
        }
        out.push(item);
    }
    Some(out)
}

/// `_nc_read_termtype`: lê uma descrição compilada. `user_definable` é `_nc_user_definable`.
pub fn read_termtype(buf: &[u8], user_definable: bool) -> Option<TermType> {
    let mut r = Rd { buf, off: 0 };
    let hdr = r.take(12);
    if hdr.len() < 12 {
        return None;
    }
    let magic = i32::from(hdr[0]) + 256 * i32::from(hdr[1]);
    if magic != MAGIC && magic != MAGIC2 {
        return None;
    }
    let wide = magic == MAGIC2;
    let size_of_numbers: usize = if wide { 4 } else { 2 };
    let max_entry_size = if wide { MAX_ENTRY_SIZE } else { MAX_ENTRY_SIZE1 };
    let name_size = short(hdr, 2);
    let bool_count = short(hdr, 4);
    let num_count = short(hdr, 6);
    let str_count = short(hdr, 8);
    let str_size = short(hdr, 10);
    if name_size < 0
        || bool_count < 0
        || num_count < 0
        || str_count < 0
        || bool_count as usize > BOOLCOUNT
        || num_count as usize > NUMCOUNT
        || str_count as usize > STRCOUNT
        || str_size < 0
    {
        return None;
    }
    let (name_size, bool_count, num_count, str_count, str_size) =
        (name_size as usize, bool_count as usize, num_count as usize, str_count as usize, str_size as usize);
    if str_count * 2 >= max_entry_size {
        return None;
    }
    let want = MAX_NAME_SIZE.min(name_size);
    let mut raw_names = r.take(want).to_vec();
    raw_names.resize(want, 0);
    if let Some(p) = raw_names.iter().position(|b| *b == 0) {
        raw_names.truncate(p);
    }
    let mut tt = TermType::empty();
    tt.names = raw_names;

    let bools = r.take(bool_count);
    if bools.len() < bool_count {
        return None;
    }
    for (i, b) in bools.iter().enumerate() {
        tt.bools[i] = *b as i8;
    }
    if (name_size + bool_count) % 2 != 0 {
        r.take(1);
    }

    let nbuf = r.take(num_count * size_of_numbers);
    if nbuf.len() < num_count * size_of_numbers {
        return None;
    }
    for (i, v) in convert_numbers(nbuf, num_count, wide).into_iter().enumerate() {
        tt.nums[i] = v;
    }

    if str_count != 0 {
        let offsets = r.take(str_count * 2);
        if offsets.len() < str_count * 2 {
            return None;
        }
        let table = r.take(str_size);
        if table.len() != str_size {
            return None;
        }
        let strs = convert_strings(offsets, str_count, table, str_size, false)?;
        for (i, v) in strs.into_iter().enumerate() {
            tt.strs[i] = v;
        }
    }

    if str_size % 2 != 0 {
        r.take(1);
    }
    let ext = r.take(10);
    let valid = ext.len() == 10 && (0..5).any(|n| short(ext, n * 2) > 0);
    if user_definable && valid {
        let ext_bool_count = short(ext, 0);
        let ext_num_count = short(ext, 2);
        let ext_str_count = short(ext, 4);
        let ext_str_usage = short(ext, 6);
        let ext_str_limit = short(ext, 8);
        let need = ext_bool_count + ext_num_count + ext_str_count;
        if need >= (max_entry_size / 2) as i32
            || ext_str_usage >= max_entry_size as i32
            || ext_str_limit >= max_entry_size as i32
            || ext_bool_count < 0
            || ext_num_count < 0
            || ext_str_count < 0
            || ext_str_usage < 0
            || ext_str_limit < 0
        {
            return None;
        }
        let (ebc, enc, esc, limit) =
            (ext_bool_count as usize, ext_num_count as usize, ext_str_count as usize, ext_str_limit as usize);
        let need = need as usize;
        tt.ext_bools = ebc;
        tt.ext_nums = enc;
        tt.ext_strs = esc;
        if ebc != 0 {
            let b = r.take(ebc);
            if b.len() != ebc {
                return None;
            }
            tt.bools.extend(b.iter().map(|x| *x as i8));
        }
        if ebc % 2 != 0 {
            r.take(1);
        }
        if enc != 0 {
            let b = r.take(enc * size_of_numbers);
            if b.len() != enc * size_of_numbers {
                return None;
            }
            tt.nums.extend(convert_numbers(b, enc, wide));
        }
        if esc + need >= max_entry_size / 2 {
            return None;
        }
        let offsets = if esc + need != 0 {
            let o = r.take((esc + need) * 2);
            if o.len() != (esc + need) * 2 {
                return None;
            }
            o
        } else {
            &[][..]
        };
        let table: &[u8] = if limit != 0 {
            let t = r.take(limit);
            if t.len() != limit {
                return None;
            }
            t
        } else {
            &[][..]
        };
        let mut base = 0usize;
        if esc != 0 {
            let vals = convert_strings(offsets, esc, table, limit, false)?;
            for v in &vals {
                if let Str::Val(s) = v {
                    base += s.len() + 1;
                }
            }
            tt.strs.extend(vals);
        }
        if need != 0 {
            let names_table = table.get(base..).unwrap_or(&[]);
            let names = convert_strings(&offsets[2 * esc..], need, names_table, limit, true)?;
            tt.ext_names = names.into_iter().map(|s| s.val().map(<[u8]>::to_vec).unwrap_or_default()).collect();
        }
    }
    Some(tt)
}

/// `_nc_read_file_entry`: lê o arquivo compilado.
pub fn read_file_entry(path: &[u8], user_definable: bool) -> Option<TermType> {
    let st = sys::stat(path).ok()?;
    // `_nc_safe_fopen`: nada de dispositivo nem diretório.
    if matches!(st.file_type(), FileType::Directory | FileType::CharDevice | FileType::BlockDevice) {
        return None;
    }
    let data = sys::read_file(path).ok()?;
    let limit = data.len().min(MAX_ENTRY_SIZE + 1);
    if limit == 0 {
        return None;
    }
    read_termtype(&data[..limit], user_definable)
}

fn quick_prefix(name: &[u8]) -> bool {
    name.starts_with(b"b64:") || name.starts_with(b"hex:")
}

fn hex_val(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

/// `decode_quickdump`: dados compilados embutidos no próprio caminho (`hex:` ou `b64:`).
fn decode_quickdump(source: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(rest) = source.strip_prefix(b"b64:") {
        let value = |ch: u8| -> Option<u32> {
            match ch {
                b'A'..=b'Z' => Some(u32::from(ch - b'A')),
                b'a'..=b'z' => Some(26 + u32::from(ch - b'a')),
                b'0'..=b'9' => Some(52 + u32::from(ch - b'0')),
                b'-' | b'+' => Some(62),
                b'_' | b'/' => Some(63),
                b'=' => Some(64),
                _ => None,
            }
        };
        let mut i = 0;
        while i < rest.len() {
            let mut bits = [0u32; 4];
            let mut pad = 0;
            for (j, b) in bits.iter_mut().enumerate() {
                let Some(&ch) = rest.get(i + j) else { return Vec::new() };
                match value(ch) {
                    Some(v) => {
                        if v == 64 {
                            pad += 1;
                        }
                        *b = v;
                    }
                    None => return Vec::new(),
                }
            }
            i += 4;
            let added = 3 - pad;
            if out.len() + added >= MAX_ENTRY_SIZE {
                return Vec::new();
            }
            out.push(((bits[0] << 2) | (bits[1] >> 4)) as u8);
            if bits[2] < 64 {
                out.push(((bits[1] << 4) | (bits[2] >> 2)) as u8);
                if bits[3] < 64 {
                    out.push(((bits[2] << 6) | bits[3]) as u8);
                }
            }
        }
    } else if let Some(rest) = source.strip_prefix(b"hex:") {
        let mut i = 0;
        while i < rest.len() {
            let hi = rest.get(i).copied().and_then(hex_val);
            let lo = rest.get(i + 1).copied().and_then(hex_val);
            match (hi, lo) {
                (Some(h), Some(l)) if out.len() < MAX_ENTRY_SIZE => out.push((h << 4) | l),
                _ => return Vec::new(),
            }
            i += 2;
        }
    }
    out
}

/// `_nc_name_match(namelst, name, "|")`.
pub fn name_match(namelst: &[u8], name: &[u8]) -> bool {
    namelst.split(|b| *b == b'|').any(|n| n == name)
}

/// A lista de diretórios do banco (`_nc_first_db`/`_nc_next_db`): `$TERMINFO`, `$HOME/.terminfo`,
/// `$TERMINFO_DIRS`, os diretórios compilados no ncurses do Debian, sem repetidos nem inexistentes.
pub fn db_dirs(tic_dir: Option<&[u8]>) -> Vec<Vec<u8>> {
    let env = |n: &str| sys::getenv(n).filter(|v| !v.is_empty());
    let mut values: Vec<Vec<u8>> = Vec::new();
    values.push(tic_dir.map(<[u8]>::to_vec).unwrap_or_default());
    values.push(env("TERMINFO").unwrap_or_default());
    values.push(match sys::getenv("HOME") {
        Some(h) => {
            let mut v = h;
            v.extend_from_slice(b"/.terminfo");
            v
        }
        None => Vec::new(),
    });
    values.push(env("TERMINFO_DIRS").unwrap_or_default());
    values.push(b"/etc/terminfo:/lib/terminfo:/usr/share/terminfo".to_vec());
    values.push(b"/etc/terminfo".to_vec());

    let mut blob: Vec<u8> = Vec::new();
    for v in &values {
        if !v.is_empty() {
            if !blob.is_empty() {
                blob.push(b':');
            }
            blob.extend_from_slice(v);
        }
    }
    // Divide em ':', exceto o ':' de um prefixo `hex:` ou `b64:` no começo de um elemento.
    let mut list: Vec<Vec<u8>> = vec![Vec::new()];
    for (j, ch) in blob.iter().enumerate() {
        let cur = list.last().unwrap();
        if *ch == b':' && !(cur.len() == 3 && quick_prefix(&blob[j - 3..j + 1])) {
            list.push(Vec::new());
        } else {
            list.last_mut().unwrap().push(*ch);
        }
    }
    let mut cleaned: Vec<Vec<u8>> = Vec::new();
    for mut item in list {
        if item.is_empty() {
            item = b"/etc/terminfo".to_vec();
        }
        // `trim_formatting`: some `\n` e `\t` e o `\` que precede um `\n`.
        let mut trimmed = Vec::with_capacity(item.len());
        let mut k = 0;
        while k < item.len() {
            let ch = item[k];
            k += 1;
            if (ch == b'\\' && item.get(k) == Some(&b'\n')) || ch == b'\n' || ch == b'\t' {
                continue;
            }
            trimmed.push(ch);
        }
        let item = trimmed;
        if !cleaned.contains(&item) {
            cleaned.push(item);
        }
    }
    let mut seen: Vec<(u64, u64)> = Vec::new();
    let mut out: Vec<Vec<u8>> = Vec::new();
    for item in cleaned {
        let mut found = false;
        let mut ident = (0u64, 0u64);
        if quick_prefix(&item) {
            found = true;
        } else if let Ok(st) = sys::stat(&item)
            && (st.file_type() == FileType::Directory || (st.file_type() == FileType::Regular && st.size > 0))
        {
            found = true;
            ident = (st.dev, st.ino);
        }
        if found && !quick_prefix(&item) {
            if seen.contains(&ident) {
                found = false;
            } else {
                seen.push(ident);
            }
        }
        if found {
            out.push(item);
        }
    }
    out
}

/// O resultado de `_nc_read_entry2`.
pub struct ReadResult {
    pub code: i32,
    pub filename: Vec<u8>,
    pub tt: Option<TermType>,
}

/// `_nc_read_tic_entry`: o arquivo `<dir>/<primeira letra>/<nome>` (ou os dados embutidos).
fn read_tic_entry(path: &[u8], name: &[u8], user_definable: bool) -> (i32, Vec<u8>, Option<TermType>) {
    let used = decode_quickdump(path);
    if !used.is_empty()
        && let Some(tt) = read_termtype(&used, user_definable)
        && name_match(&tt.names, name)
    {
        return (TGETENT_YES, b"$TERMINFO".to_vec(), Some(tt));
    }
    let mut filename = path.to_vec();
    filename.push(b'/');
    filename.push(name[0]);
    filename.push(b'/');
    filename.extend_from_slice(name);
    if filename.len() > 4095 {
        return (TGETENT_NO, filename, None);
    }
    match read_file_entry(&filename, user_definable) {
        Some(tt) => (TGETENT_YES, filename, Some(tt)),
        None => (TGETENT_NO, filename, None),
    }
}

/// `_nc_read_entry2`: procura `name` nos diretórios do banco.
pub fn read_entry(name: &[u8], user_definable: bool) -> ReadResult {
    let mut filename = name.iter().copied().take(4095).collect::<Vec<u8>>();
    if name.is_empty()
        || name == b"."
        || name == b".."
        || name.contains(&b'/')
        || name.contains(&b':')
    {
        return ReadResult { code: TGETENT_NO, filename, tt: None };
    }
    let mut code = TGETENT_ERR;
    for dir in db_dirs(None) {
        let (c, f, tt) = read_tic_entry(&dir, name, user_definable);
        code = c;
        filename = f;
        if c == TGETENT_YES {
            return ReadResult { code, filename, tt };
        }
    }
    ReadResult { code, filename, tt: None }
}

/// Um terminal aberto (`TERMINAL`).
#[derive(Clone, Debug)]
pub struct Term {
    pub tt: TermType,
    pub fd: Fd,
    pub termname: Vec<u8>,
}

/// Como o `setupterm` falhou: o `errret`, o texto que o ncurses escreve quando `errret` é NULL e,
/// quando o erro deixa o terminal usável (`hardcopy`), o próprio terminal.
#[derive(Debug)]
pub struct SetupFail {
    pub code: i32,
    pub message: String,
    pub term: Option<Term>,
}

#[derive(Copy, Clone, Debug)]
pub struct SetupOpts {
    pub use_env: bool,
    pub use_tioctl: bool,
}

impl Default for SetupOpts {
    fn default() -> SetupOpts {
        SetupOpts { use_env: true, use_tioctl: false }
    }
}

pub fn isatty(fd: Fd) -> bool {
    sys::try_current().is_some_and(|s| s.isatty(fd))
}

fn getenv_num(name: &str) -> i32 {
    let Some(src) = sys::getenv(name) else { return -1 };
    let (value, end) = super::strtol(&src);
    if value < 0 || end == 0 || end != src.len() || i64::from(value as i32) != value {
        -1
    } else {
        value as i32
    }
}

/// `_nc_get_screensize`: preenche `lines`/`columns` com o ambiente, o terminal e o terminfo.
fn get_screensize(term: &mut Term, opts: SetupOpts) -> (i32, i32) {
    let li = num_index("lines");
    let co = num_index("columns");
    let mut lines = term.tt.nums[li];
    let mut cols = term.tt.nums[co];
    if opts.use_env || opts.use_tioctl {
        if isatty(term.fd)
            && let Some(s) = sys::try_current()
            && let Ok(ws) = s.tcgetwinsize(term.fd)
        {
            lines = i32::from(ws.rows);
            cols = i32::from(ws.cols);
        }
        if opts.use_env {
            if opts.use_tioctl {
                if getenv_num("LINES") > 0 {
                    set_env_num("LINES", lines);
                }
                if getenv_num("COLUMNS") > 0 {
                    set_env_num("COLUMNS", cols);
                }
            }
            let v = getenv_num("LINES");
            if v > 0 {
                lines = v.min(512);
            }
            let v = getenv_num("COLUMNS");
            if v > 0 {
                cols = v.min(512);
            }
            // `_nc_default_screensize`
            if lines <= 0 {
                lines = term.tt.nums[li];
            }
            if cols <= 0 {
                cols = term.tt.nums[co];
            }
            if lines <= 0 {
                lines = 24;
            }
            if cols <= 0 {
                cols = 80;
            }
        }
        term.tt.nums[li] = lines;
        term.tt.nums[co] = cols;
    }
    (lines, cols)
}

fn set_env_num(name: &str, value: i32) {
    if value >= 0
        && let Some(s) = sys::try_current()
    {
        let _ = s.setenv(name.as_bytes(), value.to_string().as_bytes());
    }
}

/// `setupterm(tname, fd, ...)`. A falha vai numa caixa porque carrega o `Term` inteiro.
pub fn setupterm(tname: Option<&[u8]>, fd: Fd, opts: SetupOpts) -> Result<Term, Box<SetupFail>> {
    let fail = |code: i32, message: String| Box::new(SetupFail { code, message, term: None });
    let name: Vec<u8> = match tname {
        Some(n) => n.to_vec(),
        None => match sys::getenv("TERM") {
            Some(n) if !n.is_empty() => n,
            _ => return Err(fail(TGETENT_ERR, "TERM environment variable not set.\n".to_string())),
        },
    };
    if name.len() > MAX_NAME_SIZE {
        return Err(fail(TGETENT_ERR, "TERM environment must be 1..512 characters.\n".to_string()));
    }
    let shown = String::from_utf8_lossy(&name).into_owned();
    // Com a saída redirecionada, as atualizações de tela vão pro erro padrão.
    let mut fd = fd;
    if fd == Fd::STDOUT && !isatty(fd) {
        fd = Fd::STDERR;
    }
    let res = read_entry(&name, true);
    let mut tt = match (res.code, res.tt) {
        (TGETENT_YES, Some(tt)) => tt,
        (TGETENT_ERR, _) => return Err(fail(TGETENT_ERR, "terminals database is inaccessible\n".to_string())),
        (TGETENT_NO, _) => return Err(fail(TGETENT_NO, format!("'{shown}': unknown terminal type.\n"))),
        _ => return Err(fail(res.code, "unexpected return-code\n".to_string())),
    };
    // `_nc_setup_tinfo`: booleanos inválidos viram falsos e cadeias canceladas, ausentes.
    for b in tt.bools.iter_mut() {
        if (*b as u8) > 1 {
            *b = 0;
        }
    }
    for s in tt.strs.iter_mut() {
        if *s == Str::Cancelled {
            *s = Str::Absent;
        }
    }
    let mut term = Term { tt, fd, termname: name };
    cmdch(&mut term);
    get_screensize(&mut term, opts);
    if term.tt.b("generic_type") {
        let t = &term.tt;
        let addressable = t.s("cursor_address").valid() || (t.s("cursor_down").valid() && t.s("cursor_home").valid());
        if addressable && t.s("clear_screen").valid() {
            return Err(Box::new(SetupFail {
                code: TGETENT_YES,
                message: format!("'{shown}': terminal is not really generic.\n"),
                term: Some(term),
            }));
        }
        return Err(fail(TGETENT_NO, format!("'{shown}': I need something more specific.\n")));
    } else if term.tt.b("hard_copy") {
        return Err(Box::new(SetupFail {
            code: TGETENT_YES,
            message: format!("'{shown}': I can't handle hardcopy terminals.\n"),
            term: Some(term),
        }));
    }
    Ok(term)
}

/// `_nc_tinfo_cmdch`: troca o caractere de comando pelo de `$CC` (um só caractere).
fn cmdch(term: &mut Term) {
    let proto = match term.tt.sv("command_character") {
        Some(c) if !c.is_empty() => c[0],
        _ => return,
    };
    let Some(cc) = sys::getenv("CC") else { return };
    if cc.len() != 1 {
        return;
    }
    for s in term.tt.strs.iter_mut() {
        if let Str::Val(v) = s {
            for b in v.iter_mut() {
                if *b == proto {
                    *b = cc[0];
                }
            }
        }
    }
}

/// O que `tigetstr` devolve.
#[derive(Debug, PartialEq, Eq)]
pub enum TiStr<'a> {
    /// `CANCELLED_STRING`: o nome não é de uma cadeia.
    NotCap,
    /// `NULL`: a cadeia existe e está ausente.
    Absent,
    Val(&'a [u8]),
}

impl Term {
    /// `tigetflag`: -1 se não é um booleano.
    pub fn tigetflag(&self, name: &[u8]) -> i32 {
        let tt = &self.tt;
        let mut j: Option<usize> = find_type_entry(name, Kind::Bool);
        if j.is_none() {
            for i in (tt.bools.len() - tt.ext_bools)..tt.bools.len() {
                if tt.ext_bool_name(i) == name {
                    j = Some(i);
                    break;
                }
            }
        }
        match j {
            Some(j) => i32::from(tt.bools[j]),
            None => -1,
        }
    }

    /// `tigetnum`: -2 se não é um número, -1 se está ausente.
    pub fn tigetnum(&self, name: &[u8]) -> i32 {
        let tt = &self.tt;
        let mut j: Option<usize> = find_type_entry(name, Kind::Num);
        if j.is_none() {
            for i in (tt.nums.len() - tt.ext_nums)..tt.nums.len() {
                if tt.ext_num_name(i) == name {
                    j = Some(i);
                    break;
                }
            }
        }
        match j {
            Some(j) => {
                if tt.nums[j] >= 0 {
                    tt.nums[j]
                } else {
                    ABSENT_NUMERIC
                }
            }
            None => CANCELLED_NUMERIC,
        }
    }

    pub fn tigetstr(&self, name: &[u8]) -> TiStr<'_> {
        let tt = &self.tt;
        let mut j: Option<usize> = find_type_entry(name, Kind::Str);
        if j.is_none() {
            for i in (tt.strs.len() - tt.ext_strs)..tt.strs.len() {
                if tt.ext_str_name(i) == name {
                    j = Some(i);
                    break;
                }
            }
        }
        match j {
            Some(j) => match &tt.strs[j] {
                Str::Val(v) => TiStr::Val(v),
                _ => TiStr::Absent,
            },
            None => TiStr::NotCap,
        }
    }

    /// `longname()`: o que vem depois do último `|` dos nomes (só os primeiros 255 bytes contam).
    pub fn longname(&self) -> Vec<u8> {
        let names: Vec<u8> = self.tt.names.iter().copied().take(255).collect();
        let n = names.len();
        let mut p = n;
        while p > 0 {
            if p < n && names[p] == b'|' {
                return names[p + 1..].to_vec();
            }
            p -= 1;
        }
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_matching() {
        assert!(name_match(b"xterm|xterm-debian|X terminal", b"xterm-debian"));
        assert!(!name_match(b"xterm|X terminal", b"term"));
    }

    #[test]
    fn short_header_is_rejected() {
        assert!(read_termtype(&[0x1a, 0x01, 0, 0], true).is_none());
        let mut bad = vec![0u8; 12];
        bad[0] = 0x1a;
        bad[1] = 0x01;
        bad[2] = 1;
        // nome de 1 byte, nenhum booleano: falta o byte de nome
        assert!(read_termtype(&bad[..12], true).is_some());
    }
}
