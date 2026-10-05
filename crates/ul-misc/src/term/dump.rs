//! A saída do `infocmp` (`dump_entry.c`): a formatação de uma descrição de terminal em terminfo,
//! em nomes de variável C ou em termcap, a quebra de linhas e a comparação de duas descrições.

use super::caps_table::{
    BOOL_TERMCAP_SORT, BOOL_TERMINFO_SORT, BOOL_VARIABLE_SORT, NUM_TERMCAP_SORT, NUM_TERMINFO_SORT, NUM_VARIABLE_SORT,
    STR_TERMCAP_SORT, STR_TERMINFO_SORT, STR_VARIABLE_SORT,
};
use super::expand::tic_expand;
use super::infotocap::infotocap;
use super::terminfo::{Str, TermType};
use super::tparm::{ParmState, tiparm};
use super::{BOOLCOUNT, BOOLS, Kind, NUMCOUNT, NUMS, STRCOUNT, STRS, bool_index, c_isspace, num_index, str_index};
use crate::util::io;

pub const FAIL: i32 = -1;
const WRAPPED: i32 = 32;
const MAX_TERMCAP_LENGTH: i32 = 1023;
const MAX_TERMINFO_LENGTH: i32 = 4096;

/// Formato de saída (`F_*`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OutForm {
    Terminfo,
    Variable,
    Termcap,
    TcConvErr,
    Literal,
}

/// Ordem de classificação (`S_*`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SortMode {
    Default,
    NoSort,
    Terminfo,
    Variable,
    Termcap,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum TVersion {
    AllCaps,
    Svr1,
    Hpux,
    Aix,
    Bsd,
}

/// Preenchimento do modo de quebra (`WRAPMODE`).
const W1ST: u32 = 1;
const W2ND: u32 = 2;
const WEND: u32 = 4;
const WERR: u32 = 8;

/// Predicado de `fmt_entry`: recebe a descrição viva, o tipo e o índice.
pub type PredFn<'a> = &'a dyn Fn(&TermType, Kind, usize) -> i32;

/// O estado de `dump_entry.c`.
#[derive(Debug)]
pub struct Dump {
    tversion: TVersion,
    pub outform: OutForm,
    pub sortmode: SortMode,
    width: i32,
    height: i32,
    column: i32,
    oldcol: i32,
    pretty: bool,
    wrapped: bool,
    did_wrap: bool,
    checking: bool,
    quickdump: i32,
    save_sgr: Str,
    outbuf: Vec<u8>,
    tmpbuf: Vec<u8>,
    separator: &'static str,
    trailer: &'static str,
    indent: i32,
    /// `_nc_user_definable` (`-x`).
    pub user_definable: bool,
    /// `_nc_strict_bsd` (`-K`).
    pub strict_bsd: bool,
    pub progname: String,
}

fn isalnum(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

impl Dump {
    /// `dump_init`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        version: Option<&str>,
        mode: OutForm,
        sort: SortMode,
        wrap_strings: bool,
        twidth: i32,
        theight: i32,
        traceval: u32,
        formatted: bool,
        check: bool,
        quick: i32,
        progname: &str,
    ) -> Dump {
        let tversion = match version {
            None => TVersion::AllCaps,
            Some("SVr1") | Some("SVR1") | Some("Ultrix") => TVersion::Svr1,
            Some("HP") => TVersion::Hpux,
            Some("AIX") => TVersion::Aix,
            Some("BSD") => TVersion::Bsd,
            Some(_) => TVersion::AllCaps,
        };
        let (separator, trailer) = match mode {
            OutForm::Literal | OutForm::Terminfo | OutForm::Variable => {
                (if twidth > 0 && theight > 1 { ", " } else { "," }, "\n\t")
            }
            OutForm::Termcap | OutForm::TcConvErr => (":", "\\\n\t:"),
        };
        if traceval != 0 {
            match sort {
                SortMode::NoSort => io::eprint(format!("{progname}: sorting by term structure order\n")),
                SortMode::Terminfo => io::eprint(format!("{progname}: sorting by terminfo name order\n")),
                SortMode::Variable => io::eprint(format!("{progname}: sorting by C variable order\n")),
                SortMode::Termcap => io::eprint(format!("{progname}: sorting by termcap name order\n")),
                SortMode::Default => {}
            }
            io::eprint(format!(
                "{progname}: width = {twidth}, tversion = {}, outform = {}\n",
                tversion as i32,
                mode as i32
            ));
        }
        Dump {
            tversion,
            outform: mode,
            sortmode: sort,
            width: twidth,
            height: theight,
            column: 0,
            oldcol: 0,
            pretty: formatted,
            wrapped: wrap_strings,
            did_wrap: twidth <= 0,
            checking: check,
            quickdump: quick & 3,
            save_sgr: Str::Absent,
            outbuf: Vec::new(),
            tmpbuf: Vec::new(),
            separator,
            trailer,
            indent: 8,
            user_definable: false,
            strict_bsd: false,
            progname: progname.to_string(),
        }
    }

    fn tc_output(&self) -> bool {
        matches!(self.outform, OutForm::Termcap | OutForm::TcConvErr)
    }

    fn is_obsolete(&self, name: &[u8]) -> bool {
        matches!(self.outform, OutForm::Terminfo | OutForm::Variable)
            && self.sortmode != SortMode::Variable
            && !self.user_definable
            && name.starts_with(b"OT")
    }

    // ---- índices e nomes ----

    fn bool_indirect(&self, j: usize) -> usize {
        if j >= BOOLCOUNT {
            j
        } else {
            match self.sortmode {
                SortMode::NoSort => j,
                SortMode::Variable => usize::from(BOOL_VARIABLE_SORT[j]),
                SortMode::Termcap => usize::from(BOOL_TERMCAP_SORT[j]),
                _ => usize::from(BOOL_TERMINFO_SORT[j]),
            }
        }
    }

    fn num_indirect(&self, j: usize) -> usize {
        if j >= NUMCOUNT {
            j
        } else {
            match self.sortmode {
                SortMode::NoSort => j,
                SortMode::Variable => usize::from(NUM_VARIABLE_SORT[j]),
                SortMode::Termcap => usize::from(NUM_TERMCAP_SORT[j]),
                _ => usize::from(NUM_TERMINFO_SORT[j]),
            }
        }
    }

    fn str_indirect(&self, j: usize) -> usize {
        if j >= STRCOUNT {
            j
        } else {
            match self.sortmode {
                SortMode::NoSort => j,
                SortMode::Variable => usize::from(STR_VARIABLE_SORT[j]),
                SortMode::Termcap => usize::from(STR_TERMCAP_SORT[j]),
                _ => usize::from(STR_TERMINFO_SORT[j]),
            }
        }
    }

    fn pick(&self, cap: &super::Cap) -> &'static str {
        match self.outform {
            OutForm::Variable => cap.var,
            OutForm::Termcap | OutForm::TcConvErr => cap.tc,
            _ => cap.info,
        }
    }

    fn bool_name(&self, tt: &TermType, i: usize) -> Vec<u8> {
        if i >= BOOLCOUNT { tt.ext_bool_name(i).to_vec() } else { self.pick(&BOOLS[i]).as_bytes().to_vec() }
    }

    fn num_name(&self, tt: &TermType, i: usize) -> Vec<u8> {
        if i >= NUMCOUNT { tt.ext_num_name(i).to_vec() } else { self.pick(&NUMS[i]).as_bytes().to_vec() }
    }

    fn str_name(&self, tt: &TermType, i: usize) -> Vec<u8> {
        if i >= STRCOUNT { tt.ext_str_name(i).to_vec() } else { self.pick(&STRS[i]).as_bytes().to_vec() }
    }

    /// `version_filter`: tira as capacidades que o formato escolhido não tem.
    fn version_filter(&self, kind: Kind, idx: usize) -> bool {
        let xon = bool_index("xon_xoff");
        let width_status = num_index("width_status_line");
        let label_width = num_index("label_width");
        let prtr_non = str_index("prtr_non");
        let fnkey = |i: usize| {
            (i >= str_index("key_f0") && i <= str_index("key_f9")) || (i >= str_index("key_f11") && i <= str_index("key_f63"))
        };
        match self.tversion {
            TVersion::AllCaps => true,
            TVersion::Svr1 => match kind {
                Kind::Bool => idx <= xon,
                Kind::Num => idx <= width_status,
                Kind::Str => idx <= prtr_non,
            },
            TVersion::Hpux => match kind {
                Kind::Bool => idx <= xon,
                Kind::Num => idx <= label_width,
                Kind::Str => {
                    idx <= prtr_non
                        || fnkey(idx)
                        || idx == str_index("plab_norm")
                        || idx == str_index("label_on")
                        || idx == str_index("label_off")
                }
            },
            TVersion::Aix => match kind {
                Kind::Bool => idx <= xon,
                Kind::Num => idx <= width_status,
                Kind::Str => idx <= prtr_non || fnkey(idx),
            },
            TVersion::Bsd => match kind {
                Kind::Bool => BOOLS.get(idx).is_some_and(|c| c.from_tc),
                Kind::Num => NUMS.get(idx).is_some_and(|c| c.from_tc),
                Kind::Str => STRS.get(idx).is_some_and(|c| c.from_tc),
            },
        }
    }

    // ---- saída com quebra ----

    fn trim_trailing(&mut self) {
        while self.outbuf.last() == Some(&b' ') {
            self.outbuf.pop();
        }
    }

    fn force_wrap(&mut self) {
        self.oldcol = self.column;
        self.trim_trailing();
        self.outbuf.extend_from_slice(self.trailer.as_bytes());
        self.column = self.indent;
    }

    fn op_length(&self, src: &[u8], offset: usize) -> i32 {
        let at = |i: usize| src.get(i).copied().unwrap_or(0);
        if offset > 0 && at(offset - 1) == b'\\' {
            return 0;
        }
        let mut result = 1;
        let ch = at(offset + result as usize);
        if self.tc_output() {
            if ch == b'>' {
                result += 3;
            } else if ch == b'+' {
                result += 2;
            } else {
                result += 1;
            }
        } else if ch == b'\'' {
            result += 3;
        } else if ch == b'{' {
            let mut n = result as usize;
            loop {
                let c = at(offset + n);
                if c == 0 {
                    break;
                }
                if c == b'}' {
                    n += 1;
                    result = n as i32;
                    break;
                }
                n += 1;
            }
        } else if b"pPg".contains(&ch) {
            result += 2;
        } else {
            result += 1;
        }
        result
    }

    /// `find_split`: evita partir uma sequência com barra invertida ou um operador `%`.
    fn find_split(&self, src: &[u8], step: i32, size: i32) -> i32 {
        let mut result = size;
        if size > 0 {
            let at = |i: i32| src.get(i as usize).copied().unwrap_or(0);
            let mut mark = size;
            let mut n = size - 1;
            while n > 0 {
                let ch = at(step + n);
                if ch == b'\\' {
                    if n > 0 && at(step + n - 1) == ch {
                        n -= 1;
                    }
                    mark = n;
                    break;
                } else if !isalnum(ch) {
                    break;
                }
                n -= 1;
            }
            if mark < size {
                result = mark;
            } else {
                n = size - 1;
                while n > 0 {
                    let ch = at(step + n);
                    if ch == b'%' {
                        let need = self.op_length(src, (step + n) as usize);
                        if n + need > size {
                            mark = n;
                        }
                        break;
                    }
                    n -= 1;
                }
                if mark < size {
                    result = mark;
                }
            }
        }
        result
    }

    fn wrap_concat(&mut self, src: &[u8], need: i32, mode: u32) {
        let gaps = self.separator.len() as i32;
        let want = gaps + need;
        let mut need = need;
        self.did_wrap = self.width <= 0;
        if mode & W1ST != 0 && self.column > self.indent && self.column + want > self.width {
            self.force_wrap();
        }
        if (mode & WEND != 0 && mode & WERR == 0) && self.wrapped && self.width >= 0 && (self.column + want) > self.width {
            let mut step: i32 = 0;
            let used = self.width.max(WRAPPED);
            let mut base: i32 = 0;
            let my_t = self.trailer;
            let mut fill: Vec<u8> = Vec::new();
            for &b in src {
                if b == b' ' {
                    fill.extend_from_slice(b"\\s");
                } else {
                    fill.push(b);
                }
            }
            let last = fill.len() as i32;
            need = last;
            if self.tc_output() {
                self.trailer = "\\\n\t ";
            }
            let align: Vec<u8>;
            if let (false, Some(p)) = (self.tc_output(), fill.iter().position(|b| *b == b'=')) {
                base = (p as i32 + 1).min(8);
                align = vec![b' '; base as usize];
            } else if self.column > 8 {
                base = (self.column - 8).min(8);
                align = vec![b' '; base as usize];
            } else {
                align = Vec::new();
            }
            // "pretty" vale mais que a quebra quando já dividiu a linha.
            if !self.pretty || !fill.contains(&b'\n') {
                let mut tag = 0;
                if self.tc_output() && !self.outbuf.is_empty() && mode & W1ST == 0 {
                    tag = 3;
                }
                while (self.column + (need + gaps)) > used {
                    let mut size = used - tag;
                    if step != 0 {
                        self.outbuf.extend_from_slice(&align);
                        size -= base;
                    }
                    if size > (last - step) {
                        size = last - step;
                    }
                    size = self.find_split(&fill, step, size);
                    let from = step as usize;
                    let to = (step + size).max(step) as usize;
                    self.outbuf.extend_from_slice(&fill[from.min(fill.len())..to.min(fill.len())]);
                    step += size;
                    need -= size;
                    if need > 0 {
                        self.force_wrap();
                        self.did_wrap = true;
                        tag = 0;
                    }
                }
            }
            if need > 0 {
                if step != 0 {
                    self.outbuf.extend_from_slice(&align);
                }
                self.outbuf.extend_from_slice(&fill[(step as usize).min(fill.len())..]);
            }
            if mode & WEND != 0 {
                self.outbuf.extend_from_slice(self.separator.as_bytes());
            }
            self.trailer = my_t;
            self.force_wrap();
        } else {
            self.outbuf.extend_from_slice(src);
            if mode & WEND != 0 {
                self.outbuf.extend_from_slice(self.separator.as_bytes());
            }
            self.column += src.len() as i32;
        }
    }

    fn wrap_concat1(&mut self, src: &[u8]) {
        self.wrap_concat(src, src.len() as i32, W1ST | WEND);
    }

    fn wrap_concat3(&mut self, name: &[u8], eqls: &[u8], value: &[u8]) {
        let (nlen, elen, vlen) = (name.len() as i32, eqls.len() as i32, value.len() as i32);
        self.wrap_concat(name, nlen + elen + vlen, W1ST);
        self.wrap_concat(eqls, elen + vlen, W2ND);
        self.wrap_concat(value, vlen, WEND);
    }

    // ---- formatação das cadeias com if/then/else ----

    fn indent_tmp(&mut self, level: i32) {
        for _ in 0..level {
            self.tmpbuf.push(b'\t');
        }
    }

    fn leading_tmp(&self, leading: &[u8]) -> bool {
        let buf = &self.tmpbuf;
        if buf.len() > leading.len() {
            let mut need = buf.len() - leading.len();
            if &buf[need..] == leading {
                loop {
                    need -= 1;
                    if need == 0 {
                        return true;
                    }
                    if buf[need] == b'\n' {
                        return true;
                    }
                    if buf[need] != b'\t' {
                        return false;
                    }
                }
            }
        }
        false
    }

    /// `fmt_complex`: devolve a posição em `src` onde parou.
    fn fmt_complex(&mut self, tterm: &TermType, capability: &[u8], src: &[u8], mut pos: usize, level: i32) -> usize {
        let at = |i: usize| src.get(i).copied().unwrap_or(0);
        let mut percent = false;
        let mut params = has_params(&src[pos.min(src.len())..], true);
        while at(pos) != 0 {
            match at(pos) {
                b'^' | b'\\' => {
                    // copia o `^` ou `\` e, logo abaixo, o caractere seguinte sem interpretar
                    percent = false;
                    self.tmpbuf.push(at(pos));
                    pos += 1;
                    if at(pos) == 0 {
                        return pos;
                    }
                }
                b'%' => {
                    percent = true;
                }
                b'?' | b't' | b'e' => {
                    if percent {
                        percent = false;
                        if let Some(l) = self.tmpbuf.last_mut() {
                            *l = b'\n';
                        }
                        if at(pos) == b'e' {
                            self.indent_tmp(level);
                            self.tmpbuf.push(b'%');
                            self.tmpbuf.push(b'e');
                            pos += 1;
                            params = has_params(&src[pos.min(src.len())..], true);
                            if !params && at(pos) != 0 && at(pos) != b'%' {
                                self.tmpbuf.push(b'\n');
                                self.indent_tmp(level + 1);
                            }
                        } else {
                            self.indent_tmp(level + 1);
                            self.tmpbuf.push(b'%');
                            self.tmpbuf.push(at(pos));
                            let was_if = at(pos) == b'?';
                            pos += 1;
                            if was_if {
                                pos = self.fmt_complex(tterm, capability, src, pos, level + 1);
                                if at(pos) != 0 && at(pos) != b'%' {
                                    self.tmpbuf.push(b'\n');
                                    self.indent_tmp(level + 1);
                                }
                            } else if level == 1 && self.checking {
                                io::eprint(format!(
                                    "{}: %{} without %? in {}\n",
                                    io::lossy(super::first_name(&tterm.names)),
                                    at(pos) as char,
                                    io::lossy(capability)
                                ));
                            }
                        }
                        continue;
                    }
                }
                b';' => {
                    if percent {
                        percent = false;
                        if level > 1 {
                            if let Some(l) = self.tmpbuf.last_mut() {
                                *l = b'\n';
                            }
                            self.indent_tmp(level);
                            self.tmpbuf.push(b'%');
                            self.tmpbuf.push(at(pos));
                            pos += 1;
                            if at(pos) == b'%' && at(pos + 1) != 0 && !b"?e;".contains(&at(pos + 1)) {
                                self.tmpbuf.push(b'\n');
                                self.indent_tmp(level);
                            }
                            return pos;
                        }
                        if self.checking {
                            io::eprint(format!(
                                "{}: %; without %? in {}\n",
                                io::lossy(super::first_name(&tterm.names)),
                                io::lossy(capability)
                            ));
                        }
                    }
                }
                b'p' => {
                    if percent && params && !self.leading_tmp(b"%") {
                        if let Some(l) = self.tmpbuf.last_mut() {
                            *l = b'\n';
                        }
                        self.indent_tmp(level + 1);
                        self.tmpbuf.push(b'%');
                    }
                    percent = false;
                }
                b' ' => {
                    self.tmpbuf.extend_from_slice(b"\\s");
                    pos += 1;
                    continue;
                }
                _ => {
                    percent = false;
                }
            }
            self.tmpbuf.push(at(pos));
            pos += 1;
        }
        pos
    }
}

/// `has_params`: a cadeia tem parâmetros (`%p`)? Com `formatting`, só vale quando é longa (mais de
/// 50 bytes) ou tem um if/then/else.
pub fn has_params(src: &[u8], formatting: bool) -> bool {
    let len = src.len();
    let mut result = false;
    let mut ifthen = false;
    let mut params = false;
    let mut n = 0;
    while n + 1 < len {
        if src[n..].starts_with(b"%p") {
            params = true;
        } else if src[n..].starts_with(b"%;") {
            ifthen = true;
            result = params;
            break;
        }
        n += 1;
    }
    if !ifthen {
        result = if formatting { len > 50 && params } else { params };
    }
    result
}

/// `number_format`: números grandes e próximos de uma potência de dois saem em hexadecimal.
fn number_format(outform: OutForm, value: i32) -> String {
    if outform != OutForm::Termcap && value > 255 {
        let lv = value as u64;
        for nn in 8..64u32 {
            let mm: u64 = 1u64 << nn;
            if mm - 16 <= lv && mm + 16 > lv {
                return format!("{:#x}", value);
            }
        }
    }
    value.to_string()
}

/// `dump_predicate`: o predicado comum de decompilação.
pub fn dump_predicate(tt: &TermType, kind: Kind, idx: usize) -> i32 {
    match kind {
        Kind::Bool => {
            if tt.bools[idx] == 0 {
                FAIL
            } else {
                i32::from(tt.bools[idx])
            }
        }
        Kind::Num => {
            if tt.nums[idx] == super::ABSENT_NUMERIC {
                FAIL
            } else {
                tt.nums[idx]
            }
        }
        Kind::Str => {
            if tt.strs[idx] != Str::Absent {
                1
            } else {
                FAIL
            }
        }
    }
}

/// `skip_padding` do `trim_sgr0.c`: o fim de um `$<...>` bem formado.
fn skip_padding(v: &[u8]) -> Option<usize> {
    if v.len() >= 2 && v[0] == b'$' && v[1] == b'<' {
        let mut i = 2;
        let mut state = 0;
        let mut result = None;
        while i < v.len() {
            let ch = v[i];
            i += 1;
            if ch == b'*' || ch == b'/' {
                if state == 0 {
                    break;
                }
            } else if ch == b'>' {
                if state != 0 {
                    result = Some(i);
                }
                break;
            } else if ch == b'.' {
                if state < 2 {
                    state = 2;
                } else {
                    break;
                }
            } else if ch.is_ascii_digit() {
                if state < 2 {
                    state = 1;
                } else if state == 2 {
                    state = 3;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        result
    } else {
        None
    }
}

fn strip_padding(value: &mut Vec<u8>) {
    let mut s = 0usize;
    while s < value.len() {
        let ch = value[s];
        if ch == b'\\' {
            s += 1;
            if s >= value.len() {
                break;
            }
            s += 1;
        } else {
            let d = if ch == b'$' { skip_padding(&value[s..]) } else { None };
            match d {
                Some(len) => {
                    value.drain(s..s + len);
                }
                None => s += 1,
            }
        }
    }
}

fn is_csi(s: &[u8]) -> usize {
    if s.first() == Some(&0x9b) {
        1
    } else if s.starts_with(b"\x1b[") {
        2
    } else {
        0
    }
}

fn skip_zero(s: &[u8], mut at: usize) -> usize {
    let g = |i: usize| s.get(i).copied().unwrap_or(0);
    if g(at) == b'0' {
        if g(at + 1) == b';' {
            at += 2;
        } else if g(at + 1).is_ascii_alphabetic() {
            at += 1;
        }
    }
    at
}

fn skip_delay(s: &[u8], mut at: usize) -> usize {
    let g = |i: usize| s.get(i).copied().unwrap_or(0);
    if g(at) == b'$' && g(at + 1) == b'<' {
        at += 2;
        while g(at).is_ascii_digit() || g(at) == b'/' {
            at += 1;
        }
        if g(at) == b'>' {
            at += 1;
        }
    }
    at
}

fn rewrite_sgr(s: &mut Vec<u8>, attr: Option<&[u8]>) {
    if let Some(attr) = attr {
        if s.len() > attr.len() && s.starts_with(attr) {
            s.drain(..attr.len());
            s.extend_from_slice(attr);
        }
    }
}

fn similar_sgr(a: &[u8], b: &[u8]) -> bool {
    let csi_a = is_csi(a);
    let csi_b = is_csi(b);
    let mut ia = 0;
    let mut ib = 0;
    if csi_a != 0 && csi_b != 0 && csi_a == csi_b {
        ia += csi_a;
        ib += csi_b;
        if a.get(ia) != b.get(ib) {
            ia = skip_zero(a, ia);
            ib = skip_zero(b, ib);
        }
    }
    let a = &a[ia.min(a.len())..];
    let b = &b[ib.min(b.len())..];
    if !a.is_empty() && !b.is_empty() {
        let n = a.len().min(b.len());
        a[..n] == b[..n]
    } else {
        false
    }
}

/// `chop_out(string, i, j)`: copia a cauda a partir de `j` sobre a posição `i`, byte a byte como o laço
/// do original (que não confere `j >= i`).
fn chop_out(s: &mut Vec<u8>, mut i: usize, mut j: usize) {
    s.push(0);
    while j < s.len() && s[j] != 0 && i < s.len() {
        s[i] = s[j];
        i += 1;
        j += 1;
    }
    s.truncate(i.min(s.len()));
}

fn compare_part(part: &[u8], full: &[u8]) -> usize {
    let (mut p, mut f) = (0usize, 0usize);
    let mut used_full = 0usize;
    let mut used_delay = 0usize;
    let g = |s: &[u8], i: usize| s.get(i).copied().unwrap_or(0);
    while g(part, p) != 0 {
        if g(part, p) != g(full, f) {
            return 0;
        }
        if used_delay != 0 {
            used_full += used_delay;
            used_delay = 0;
        }
        if g(part, p) == b'$' && g(full, f) == b'$' {
            let next_part = skip_delay(part, p);
            let next_full = skip_delay(full, f);
            if next_part != p && next_full != f {
                used_delay += next_full - f;
                f = next_full;
                p = next_part;
                continue;
            }
        }
        used_full += 1;
        p += 1;
        f += 1;
    }
    used_full
}

/// `_nc_trim_sgr0`: tira de `sgr0` o que desliga o conjunto de caracteres alternativo, que um programa
/// termcap não entende. Devolve o valor original quando nada muda.
pub fn trim_sgr0(tt: &TermType, sgr: &Str, sgr0: &[u8]) -> Vec<u8> {
    let Str::Val(sgr_val) = sgr else { return sgr0.to_vec() };
    let dummy = TermType::empty();
    let mut state = ParmState::default();
    let attr9 = |flag: i64| -> Option<Vec<u8>> {
        let mut st = ParmState::default();
        tiparm(&dummy, &mut st, 9, sgr_val, &[0, 0, 0, 0, 0, 0, 0, 0, flag]).map(|mut v| {
            strip_padding(&mut v);
            v
        })
    };
    let _ = &mut state;
    let mut on = attr9(1);
    let mut off = attr9(0);
    let mut end: Vec<u8> = sgr0.to_vec();
    let smacs = tt.sv("enter_alt_charset_mode");
    let rmacs = tt.sv("exit_alt_charset_mode");
    let mut result = sgr0.to_vec();
    let ok = match (&mut on, &mut off) {
        (Some(on), Some(off)) => {
            rewrite_sgr(on, smacs);
            rewrite_sgr(off, rmacs);
            rewrite_sgr(&mut end, rmacs);
            true
        }
        _ => false,
    };
    if ok {
        let (on, mut off) = (on.unwrap(), off.unwrap());
        if similar_sgr(&off, &end) && !similar_sgr(&off, &on) {
            let mut found = false;
            result = off.clone();
            if let Some(rm) = rmacs {
                let j = off.len();
                let k = rm.len();
                if j > k {
                    for i in 0..=(j - k) {
                        let k2 = compare_part(rm, &off[i..]);
                        if k2 != 0 {
                            found = true;
                            chop_out(&mut off, i, i + k2);
                            result = off.clone();
                            break;
                        }
                    }
                }
            }
            if !found {
                let i = is_csi(&off);
                if i != 0 && off.last() == Some(&b'm') {
                    let tmp = skip_zero(&off, i);
                    if off.get(tmp) == Some(&b'1') && skip_zero(&off, tmp + 1) != tmp + 1 {
                        let mut i2 = tmp;
                        if off[i2 - 1] == b';' {
                            i2 -= 1;
                        }
                        let j = skip_zero(&off, tmp + 1);
                        chop_out(&mut off, i2, j);
                        result = off.clone();
                        found = true;
                    }
                }
            }
            if !found {
                if let Some(pos) = end.windows(off.len().max(1)).position(|w| w == off.as_slice()) {
                    if end != off {
                        let mut tmp = end.clone();
                        chop_out(&mut tmp, pos, off.len());
                        result = tmp;
                    }
                }
            }
            if result == sgr0 {
                result = sgr0.to_vec();
            }
        }
    }
    result
}

impl Dump {
    /// `fmt_entry`: formata a descrição em `outbuf` e devolve o tamanho (do formato compilado, com
    /// `infodump`, ou do texto).
    pub fn fmt_entry(
        &mut self,
        tterm: &mut TermType,
        pred: PredFn<'_>,
        content_only: bool,
        suppress_untranslatable: bool,
        infodump: bool,
        numbers: i32,
    ) -> i32 {
        let mut len: i32 = 12;
        let mut num_bools: usize = 0;
        let mut num_values: usize = 0;
        let mut num_strings: usize = 0;
        let mut outcount = false;

        self.outbuf.clear();
        if content_only {
            self.column = self.indent;
        } else {
            self.outbuf.extend_from_slice(&tterm.names);
            // Dois-pontos vale em terminfo, mas não em termcap.
            if !infodump {
                for b in self.outbuf.iter_mut() {
                    if *b == b':' {
                        *b = b'=';
                    }
                }
            }
            self.outbuf.extend_from_slice(self.separator.as_bytes());
            self.column = self.outbuf.len() as i32;
            if self.height > 1 {
                self.force_wrap();
            }
        }

        for j in 0..tterm.bools.len() {
            let i = self.bool_indirect(j);
            let name = self.bool_name(tterm, i);
            if !self.version_filter(Kind::Bool, i) {
                continue;
            } else if self.is_obsolete(&name) {
                continue;
            }
            let predval = pred(tterm, Kind::Bool, i);
            if predval != FAIL {
                let mut buffer = name.clone();
                if predval <= 0 {
                    buffer.push(b'@');
                } else if i + 1 > num_bools {
                    num_bools = i + 1;
                }
                self.wrap_concat1(&buffer);
                outcount = true;
            }
        }
        if self.column != self.indent && self.height > 1 {
            self.force_wrap();
        }

        for j in 0..tterm.nums.len() {
            let i = self.num_indirect(j);
            let name = self.num_name(tterm, i);
            if !self.version_filter(Kind::Num, i) {
                continue;
            } else if self.is_obsolete(&name) {
                continue;
            }
            let predval = pred(tterm, Kind::Num, i);
            if predval != FAIL {
                let mut buffer = name.clone();
                if tterm.nums[i] < 0 {
                    buffer.push(b'@');
                } else {
                    buffer.push(b'#');
                    buffer.extend_from_slice(number_format(self.outform, tterm.nums[i]).as_bytes());
                    if i + 1 > num_values {
                        num_values = i + 1;
                    }
                }
                self.wrap_concat1(&buffer);
                outcount = true;
            }
        }
        if self.column != self.indent && self.height > 1 {
            self.force_wrap();
        }

        len += (num_bools + num_values * 2 + tterm.names.len() + 1) as i32;
        if len & 1 != 0 {
            len += 1;
        }

        if self.outform == OutForm::Termcap {
            let reset = tterm.s("termcap_reset").val().map(<[u8]>::to_vec);
            if let Some(reset) = reset {
                let i3 = str_index("init_3string");
                if tterm.strs[i3].val() == Some(reset.as_slice()) {
                    tterm.strs[i3] = Str::Absent;
                }
                let r2 = str_index("reset_2string");
                if tterm.strs[r2].val() == Some(reset.as_slice()) {
                    tterm.strs[r2] = Str::Absent;
                }
            }
        }

        let enter_insert_mode = str_index("enter_insert_mode");
        let exit_insert_mode = str_index("exit_insert_mode");
        let exit_attribute_mode = str_index("exit_attribute_mode");
        for j in 0..tterm.strs.len() {
            let i = self.str_indirect(j);
            let name = self.str_name(tterm, i);
            let mut capability: Str = tterm.strs[i].clone();
            if !self.version_filter(Kind::Str, i) {
                continue;
            } else if self.is_obsolete(&name) {
                continue;
            }
            // Nomes estendidos passam de 2 caracteres, que um programa termcap não lê.
            if self.outform == OutForm::Termcap && name.len() > 2 {
                continue;
            }
            if self.outform == OutForm::Termcap {
                // Vi antigo quer smir/rmir definidos pra ich/ich1 funcionarem.
                if tterm.s("insert_character").valid() || tterm.s("parm_ich").valid() {
                    if i == enter_insert_mode && tterm.strs[enter_insert_mode] == Str::Absent {
                        self.wrap_concat1(b"im=");
                        outcount = true;
                        continue;
                    }
                    if i == exit_insert_mode && tterm.strs[exit_insert_mode] == Str::Absent {
                        self.wrap_concat1(b"ei=");
                        outcount = true;
                        continue;
                    }
                }
                // Um sgr0 com rmacs confunde programas termcap (screen): tira.
                if tterm.s("exit_attribute_mode").valid() && i == exit_attribute_mode {
                    if let Some(cap) = capability.val() {
                        let trimmed = trim_sgr0(tterm, &self.save_sgr, cap);
                        if trimmed != cap {
                            capability = Str::Val(trimmed);
                        }
                    }
                }
            }
            let predval = pred(tterm, Kind::Str, i);
            if predval != FAIL {
                if capability.valid() && i + 1 > num_strings {
                    num_strings = i + 1;
                }
                match &capability {
                    Str::Val(cap) if self.tc_output() => {
                        let srccap = tic_expand(cap, true, numbers);
                        let params = if i < STRS.len() {
                            i32::from(STRS[i].param)
                        } else if srccap.first() == Some(&b'k') {
                            0
                        } else {
                            i32::from(has_params(&srccap, false))
                        };
                        let cv = infotocap(&srccap, params, self.strict_bsd);
                        match cv {
                            None => {
                                if self.outform == OutForm::TcConvErr {
                                    let mut b = name.clone();
                                    b.extend_from_slice(b"=!!! ");
                                    b.extend_from_slice(&srccap);
                                    b.extend_from_slice(b" WILL NOT CONVERT !!!");
                                    self.wrap_concat1(&b);
                                    outcount = true;
                                } else if suppress_untranslatable {
                                    continue;
                                } else {
                                    // `:` vira `\:` e a barra invertida leva o caractere seguinte.
                                    let mut d: Vec<u8> = Vec::new();
                                    let mut it = srccap.iter().copied();
                                    while let Some(c) = it.next() {
                                        if d.len() + 2 >= (MAX_TERMINFO_LENGTH + 20) as usize {
                                            io::eprint(format!(
                                                "{}: value for {} is too long\n",
                                                self.progname,
                                                io::lossy(&name)
                                            ));
                                            break;
                                        }
                                        if c == b':' {
                                            d.push(b'\\');
                                            d.push(b':');
                                        } else if c == b'\\' {
                                            d.push(c);
                                            match it.next() {
                                                Some(n) => d.push(n),
                                                None => break,
                                            }
                                        } else {
                                            d.push(c);
                                        }
                                    }
                                    let mut need = 3 + name.len() as i32 + d.len() as i32;
                                    self.wrap_concat(b"..", need, W1ST | WERR);
                                    need -= 2;
                                    self.wrap_concat(&name, need, WERR);
                                    need -= name.len() as i32;
                                    self.wrap_concat(b"=", need, W2ND | WERR);
                                    need -= 1;
                                    self.wrap_concat(&d, need, WEND | WERR);
                                    outcount = true;
                                }
                            }
                            Some(cv) => self.wrap_concat3(&name, b"=", &cv),
                        }
                        len += cap.len() as i32 + 1;
                    }
                    Str::Val(cap) => {
                        let src = tic_expand(cap, self.outform == OutForm::Terminfo, numbers);
                        self.tmpbuf.clear();
                        self.tmpbuf.extend_from_slice(&name);
                        self.tmpbuf.push(b'=');
                        if self.pretty && matches!(self.outform, OutForm::Terminfo | OutForm::Variable) {
                            self.fmt_complex(tterm, &name, &src, 0, 1);
                        } else {
                            self.tmpbuf.extend_from_slice(&src);
                        }
                        len += cap.len() as i32 + 1;
                        let t = self.tmpbuf.clone();
                        self.wrap_concat1(&t);
                        outcount = true;
                    }
                    _ => {
                        let mut b = name.clone();
                        b.push(b'@');
                        self.wrap_concat1(&b);
                        outcount = true;
                    }
                }
            }
        }
        len += (num_strings * 2) as i32;

        if self.tversion == TVersion::Hpux {
            for (var, label) in [("memory_lock", "meml"), ("memory_unlock", "memu")] {
                if let Some(v) = tterm.sv(var) {
                    let mut b = label.as_bytes().to_vec();
                    b.push(b'=');
                    b.extend_from_slice(v);
                    self.wrap_concat1(&b);
                    outcount = true;
                }
            }
        } else if self.tversion == TVersion::Aix {
            if let Some(acs) = tterm.sv("acs_chars") {
                let acstrans = b"lqkxjmwuvtn";
                let mut boxchars: Vec<u8> = Vec::new();
                let mut box_ok = true;
                for c in acstrans {
                    match acs.iter().position(|b| b == c) {
                        Some(p) if p + 1 < acs.len() => boxchars.push(acs[p + 1]),
                        Some(_) => boxchars.push(0),
                        None => {
                            box_ok = false;
                            break;
                        }
                    }
                }
                if box_ok {
                    let mut b = b"box1=".to_vec();
                    b.extend_from_slice(&tic_expand(&boxchars, self.outform == OutForm::Terminfo, numbers));
                    self.wrap_concat1(&b);
                    outcount = true;
                }
            }
        }

        // Tira o fim pra não deixar uma linha em branco a mais no `infocmp -u` sem diferenças.
        if outcount {
            let mut trimmed = false;
            let j = self.outbuf.len();
            if self.wrapped && self.did_wrap {
                // nada
            } else if j >= 2 && self.outbuf[j - 1] == b'\t' && self.outbuf[j - 2] == b'\n' {
                self.outbuf.truncate(j - 2);
                trimmed = true;
            } else if j >= 4
                && self.outbuf[j - 1] == b':'
                && self.outbuf[j - 2] == b'\t'
                && self.outbuf[j - 3] == b'\n'
                && self.outbuf[j - 4] == b'\\'
            {
                self.outbuf.truncate(j - 4);
                trimmed = true;
            }
            if trimmed {
                self.column = self.oldcol;
                self.outbuf.push(b' ');
            }
        }
        if infodump { len } else { self.outbuf.len() as i32 }
    }
}

/// `set_obsolete_termcaps` (`capdefaults.c`): calcula as capacidades obsoletas que o termcap usa.
pub fn set_obsolete_termcaps(tp: &mut TermType) {
    let delay = |s: &[u8]| -> i32 {
        match s.iter().position(|b| *b == b'*') {
            Some(p) => {
                let rest = &s[p + 1..];
                let mut i = 0;
                while i < rest.len() && super::c_isspace(rest[i]) {
                    i += 1;
                }
                let mut neg = false;
                if i < rest.len() && (rest[i] == b'+' || rest[i] == b'-') {
                    neg = rest[i] == b'-';
                    i += 1;
                }
                let mut v: i32 = 0;
                while i < rest.len() && rest[i].is_ascii_digit() {
                    v = v.wrapping_mul(10).wrapping_add(i32::from(rest[i] - b'0'));
                    i += 1;
                }
                i32::from((if neg { -v } else { v }) as i16)
            }
            None => 0,
        }
    };
    let set_num = |tp: &mut TermType, var: &str, val: i32| {
        let i = num_index(var);
        tp.nums[i] = val;
    };
    if let Some(cr) = tp.sv("carriage_return") {
        let d = delay(cr);
        if d != 0 {
            set_num(tp, "carriage_return_delay", d);
        }
    }
    if let Some(nl) = tp.sv("newline") {
        let d = delay(nl);
        if d != 0 {
            set_num(tp, "new_line_delay", d);
        }
    }
    let init2 = str_index("termcap_init2");
    let init3 = str_index("init_3string");
    if !tp.strs[init2].valid() && tp.strs[init3].valid() {
        tp.strs[init2] = tp.strs[init3].clone();
        tp.strs[init3] = Str::Absent;
    }
    let reset = str_index("termcap_reset");
    let r1 = str_index("reset_1string");
    let r2 = str_index("reset_2string");
    let r3 = str_index("reset_3string");
    if !tp.strs[reset].valid() && tp.strs[r2].valid() && !tp.strs[r1].valid() && !tp.strs[r3].valid() {
        tp.strs[reset] = tp.strs[r2].clone();
        tp.strs[r2] = Str::Absent;
    }
    let ul = num_index("magic_cookie_glitch_ul");
    let mcg = num_index("magic_cookie_glitch");
    if tp.nums[ul] == super::ABSENT_NUMERIC && tp.nums[mcg] != super::ABSENT_NUMERIC && tp.s("enter_underline_mode").valid() {
        tp.nums[ul] = tp.nums[mcg];
    }
    let nl_is = tp.sv("newline").is_some_and(|n| n == b"\n");
    tp.bools[bool_index("linefeed_is_newline")] = i8::from(nl_is);
    if let Some(cl) = tp.sv("cursor_left") {
        let d = delay(cl);
        if d != 0 {
            set_num(tp, "backspace_delay", d);
        }
    }
    if let Some(t) = tp.sv("tab") {
        let d = delay(t);
        if d != 0 {
            set_num(tp, "horizontal_tab_delay", d);
        }
    }
}

/// `repair_acsc`: põe o `acsc` na forma canônica (ordenado e sem repetição).
pub fn repair_acsc(tp: &mut TermType) {
    let idx = str_index("acs_chars");
    let Str::Val(acs) = &tp.strs[idx] else { return };
    let mut acs = acs.clone();
    let mut fix_needed = false;
    let mut source: u32 = 0;
    let mut n = 0usize;
    while n < acs.len() {
        let target = u32::from(acs[n]);
        if source >= target {
            fix_needed = true;
            break;
        }
        source = target;
        if n + 1 < acs.len() {
            n += 1;
        }
        n += 1;
    }
    if fix_needed {
        let mut mapped = [0u8; 256];
        let mut extra = 0u8;
        let mut n = 0usize;
        while n < acs.len() {
            let src = usize::from(acs[n]);
            if n + 1 < acs.len() {
                mapped[src] = acs[n + 1];
                n += 1;
            } else {
                extra = src as u8;
            }
            n += 1;
        }
        let mut out: Vec<u8> = Vec::new();
        for (c, m) in mapped.iter().enumerate() {
            if *m != 0 {
                out.push(c as u8);
                out.push(*m);
            }
        }
        if extra != 0 {
            out.push(extra);
        }
        acs = out;
        tp.strs[idx] = Str::Val(acs);
    }
}

/// `one_one_mapping`: o `acsc` é um mapa 1 pra 1 (como o do vt100)?
fn one_one_mapping(mapping: Option<&[u8]>) -> bool {
    let Some(m) = mapping else { return true };
    let mut n = 0;
    while n + 1 < m.len() {
        if b"lmkjtuvwqxn".contains(&m[n]) && m[n] != m[n + 1] {
            return false;
        }
        n += 2;
    }
    true
}

fn show_why(msg: &str) {
    use std::io::Write;
    let _ = io::stdout().write_all(msg.as_bytes());
}

impl Dump {
    fn purged_acs(&self, tterm: &mut TermType) -> bool {
        if tterm.s("acs_chars").valid() {
            if !one_one_mapping(tterm.sv("acs_chars")) {
                tterm.strs[str_index("enter_alt_charset_mode")] = Str::Absent;
                tterm.strs[str_index("exit_alt_charset_mode")] = Str::Absent;
                show_why("# (rmacs/smacs removed for consistency)\n");
            }
            return true;
        }
        false
    }

    fn find_string(&self, tterm: &TermType, name: &str) -> Option<usize> {
        for n in 0..tterm.strs.len().min(STRCOUNT) {
            if self.version_filter(Kind::Str, n) && STRS[n].info == name {
                return if tterm.strs[n].valid() { Some(n) } else { None };
            }
        }
        None
    }

    fn kill_labels(&self, tterm: &mut TermType, target: i32) -> i32 {
        let mut target = target;
        let mut result = 0;
        for n in 0..=10 {
            if let Some(i) = self.find_string(tterm, &format!("lf{n}")) {
                let cap_len = tterm.strs[i].val().map_or(0, <[u8]>::len) as i32;
                tterm.strs[i] = Str::Absent;
                target -= cap_len + 5;
                result += 1;
                if target < 0 {
                    break;
                }
            }
        }
        result
    }

    fn kill_fkeys(&self, tterm: &mut TermType, target: i32) -> i32 {
        let mut target = target;
        let mut result = 0;
        for n in (0..=60).rev() {
            if let Some(i) = self.find_string(tterm, &format!("kf{n}")) {
                let cap_len = tterm.strs[i].val().map_or(0, <[u8]>::len) as i32;
                tterm.strs[i] = Str::Absent;
                target -= cap_len + 5;
                result += 1;
                if target < 0 {
                    break;
                }
            }
        }
        result
    }

    /// `dump_entry`: formata uma entrada, com os cortes pra caber no limite do termcap.
    pub fn dump_entry(&mut self, tterm: &mut TermType, suppress_untranslatable: bool, limited: bool, numbers: i32, pred: PredFn<'_>) {
        let (critlen, legend, infodump) = if self.tc_output() {
            set_obsolete_termcaps(tterm);
            (MAX_TERMCAP_LENGTH, "older termcap", false)
        } else {
            (MAX_TERMINFO_LENGTH, "terminfo", true)
        };
        self.save_sgr = tterm.s("set_attributes").clone();
        let sgr_idx = str_index("set_attributes");
        let mut suppress = suppress_untranslatable;
        let first = self.fmt_entry(tterm, pred, false, suppress, infodump, numbers);
        if first > critlen && self.tc_output() && limited {
            let save_tterm = tterm.clone();
            if !suppress {
                show_why(&format!("# (untranslatable capabilities removed to fit entry within {critlen} bytes)\n"));
                suppress = true;
            }
            if self.fmt_entry(tterm, pred, false, suppress, infodump, numbers) > critlen {
                // Corta o `sgr`, que é uma otimização, e o `acsc`, que o termcap BSD não usa.
                let mut changed = false;
                for n in STRCOUNT..tterm.strs.len() {
                    let name = tterm.ext_str_name(n).to_vec();
                    if tterm.strs[n].valid() {
                        tterm.strs[sgr_idx] = Str::Absent;
                        if name.len() <= 2 {
                            show_why(&format!("# ({} removed to fit entry within {critlen} bytes)\n", io::lossy(&name)));
                        }
                        changed = true;
                        if self.fmt_entry(tterm, pred, false, suppress, infodump, numbers) <= critlen {
                            break;
                        }
                    }
                }
                if tterm.strs[sgr_idx].valid() {
                    tterm.strs[sgr_idx] = Str::Absent;
                    show_why(&format!("# (sgr removed to fit entry within {critlen} bytes)\n"));
                    changed = true;
                }
                if !changed || self.fmt_entry(tterm, pred, false, suppress, infodump, numbers) > critlen {
                    if self.purged_acs(tterm) {
                        tterm.strs[str_index("acs_chars")] = Str::Absent;
                        show_why(&format!("# (acsc removed to fit entry within {critlen} bytes)\n"));
                        changed = true;
                    }
                }
                if !changed || self.fmt_entry(tterm, pred, false, suppress, infodump, numbers) > critlen {
                    let oldversion = self.tversion;
                    self.tversion = TVersion::Bsd;
                    show_why(&format!("# (terminfo-only capabilities suppressed to fit entry within {critlen} bytes)\n"));
                    let mut len = self.fmt_entry(tterm, pred, false, suppress, infodump, numbers);
                    if len > critlen && self.kill_labels(tterm, len - critlen) != 0 {
                        show_why(&format!("# (some labels capabilities suppressed to fit entry within {critlen} bytes)\n"));
                        len = self.fmt_entry(tterm, pred, false, suppress, infodump, numbers);
                    }
                    if len > critlen && self.kill_fkeys(tterm, len - critlen) != 0 {
                        show_why(&format!("# (some function-key capabilities suppressed to fit entry within {critlen} bytes)\n"));
                        len = self.fmt_entry(tterm, pred, false, suppress, infodump, numbers);
                    }
                    if len > critlen {
                        io::eprint(format!("{}: {} entry is {len} bytes long\n", self.progname, io::lossy(super::first_name(&tterm.names))));
                        show_why(&format!("# WARNING: this entry, {len} bytes long, may core-dump {legend} libraries!\n"));
                    }
                    self.tversion = oldversion;
                }
            }
            *tterm = save_tterm;
        } else if !self.version_filter(Kind::Str, str_index("acs_chars")) {
            let save_tterm = tterm.clone();
            if self.purged_acs(tterm) {
                self.fmt_entry(tterm, pred, false, suppress, infodump, numbers);
            }
            *tterm = save_tterm;
        }
    }

    /// `dump_uses`: a cláusula `use=` (ou `tc=`).
    pub fn dump_uses(&mut self, value: &[u8], infodump: bool) {
        let cap = if infodump { "use" } else { "tc" };
        if self.tc_output() {
            self.trim_trailing();
        }
        let limit = value.len().min(32);
        let mut buffer = format!("{cap}=").into_bytes();
        buffer.extend_from_slice(&value[..limit]);
        self.wrap_concat1(&buffer);
    }

    /// `show_entry`: tira o branco do fim e escreve.
    pub fn show_entry(&mut self) -> i32 {
        use std::io::Write;
        if !self.outbuf.is_empty() {
            let infodump = !self.tc_output();
            let delim = if infodump { b',' } else { b':' };
            let mut used = self.outbuf.len();
            let mut j = used as isize - 1;
            while j > 0 {
                let ch = self.outbuf[j as usize];
                if ch == b'\n' {
                } else if c_isspace(ch) {
                    used = j as usize;
                } else if !infodump && ch == b'\\' {
                    used = j as usize;
                } else if ch == delim && self.outbuf[j as usize - 1] != b'\\' {
                    used = j as usize + 1;
                } else {
                    break;
                }
                j -= 1;
            }
            self.outbuf.truncate(used);
        }
        let mut o = io::stdout();
        let _ = o.write_all(&self.outbuf);
        let _ = o.write_all(b"\n");
        self.outbuf.len() as i32
    }

    /// `compare_entry`: chama o gancho pra cada capacidade na ordem escolhida.
    pub fn compare_entry(&self, hook: &mut dyn FnMut(CmpKind, usize, &[u8]), tp: &TermType, quiet: bool) {
        use std::io::Write;
        let mut o = io::stdout();
        if !quiet {
            let _ = o.write_all(b"    comparing booleans.\n");
        }
        for j in 0..tp.bools.len() {
            let i = self.bool_indirect(j);
            let name = self.bool_name(tp, i);
            if self.is_obsolete(&name) {
                continue;
            }
            hook(CmpKind::Boolean, i, &name);
        }
        if !quiet {
            let _ = o.write_all(b"    comparing numbers.\n");
        }
        for j in 0..tp.nums.len() {
            let i = self.num_indirect(j);
            let name = self.num_name(tp, i);
            if self.is_obsolete(&name) {
                continue;
            }
            hook(CmpKind::Number, i, &name);
        }
        if !quiet {
            let _ = o.write_all(b"    comparing strings.\n");
        }
        for j in 0..tp.strs.len() {
            let i = self.str_indirect(j);
            let name = self.str_name(tp, i);
            if self.is_obsolete(&name) {
                continue;
            }
            hook(CmpKind::String, i, &name);
        }
        hook(CmpKind::Use, 0, b"use");
    }
}

/// `CMP_*`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CmpKind {
    Boolean,
    Number,
    String,
    Use,
}

