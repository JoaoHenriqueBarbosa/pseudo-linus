//! Expansão de palavras, na ordem do bash: chaves, til, parâmetro, substituição de comando,
//! aritmética e substituição de processo (da esquerda pra direita), divisão por IFS, expansão de
//! caminhos e remoção de aspas.
//!
//! Durante a expansão cada byte carrega uma classe: entre aspas (literal, não divide nem faz glob),
//! resultado de expansão sem aspas (divide e faz glob) ou literal sem aspas (só faz glob). A string
//! vazia entre aspas vira um marcador de largura zero que conta como conteúdo na divisão (o CTLNUL
//! do bash) e some no fim.

use std::sync::Arc;

use sysabi::{Errno, Fd, FdAction, OFlags, ProcAttrs, WaitOptions, WaitTarget};

use crate::ast::*;
use crate::pattern::{MatchOpts, Pattern};
use crate::shell::{Flow, Shell, sys};
use crate::vars::{Attrs, Value, Var};

/// Byte entre aspas.
const Q: u8 = 1;
/// Resultado de expansão sem aspas: sujeito à divisão por IFS.
const SPLIT: u8 = 2;
/// Marcador de string vazia entre aspas (o byte em si é ignorado).
const NULLMARK: u8 = 4;

#[derive(Clone, Debug, Default)]
struct Field {
    bytes: Vec<u8>,
    flags: Vec<u8>,
}

impl Field {
    fn push(&mut self, data: &[u8], flag: u8) {
        self.bytes.extend_from_slice(data);
        self.flags.extend(std::iter::repeat_n(flag, data.len()));
    }

    fn mark(&mut self) {
        self.bytes.push(0);
        self.flags.push(Q | NULLMARK);
    }

    fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Bytes finais (sem marcadores).
    fn text(&self) -> Vec<u8> {
        self.bytes.iter().zip(&self.flags).filter(|(_, f)| **f & NULLMARK == 0).map(|(b, _)| *b).collect()
    }
}

/// Construtor dos campos de uma palavra.
#[derive(Default)]
struct Builder {
    fields: Vec<Field>,
    /// Classe dos literais sem aspas (dentro da palavra de `${x:-w}` sem aspas eles dividem).
    lit_flag: u8,
}

impl Builder {
    fn new() -> Builder {
        Builder { fields: vec![Field::default()], lit_flag: 0 }
    }

    fn cur(&mut self) -> &mut Field {
        if self.fields.is_empty() {
            self.fields.push(Field::default());
        }
        let n = self.fields.len();
        &mut self.fields[n - 1]
    }

    fn push(&mut self, data: &[u8], flag: u8) {
        self.cur().push(data, flag);
    }

    fn mark(&mut self) {
        self.cur().mark();
    }

    fn break_field(&mut self) {
        self.fields.push(Field::default());
    }

    fn total_len(&self) -> usize {
        self.fields.iter().map(|f| f.bytes.len()).sum::<usize>() + self.fields.len()
    }
}

/// Valor de um parâmetro: escalar (talvez não definido) ou lista (`$@`, `${a[@]}`).
#[derive(Clone, Debug)]
enum PVal {
    Str(Option<Vec<u8>>),
    /// `star`: veio de `*` (junta com o primeiro caractere de IFS entre aspas).
    List(Vec<Vec<u8>>, bool),
}

impl PVal {
    fn is_unset(&self) -> bool {
        match self {
            PVal::Str(v) => v.is_none(),
            PVal::List(v, _) => v.is_empty(),
        }
    }

    fn is_null(&self) -> bool {
        match self {
            PVal::Str(v) => v.as_ref().is_none_or(|x| x.is_empty()),
            PVal::List(v, _) => v.is_empty() || (v.len() == 1 && v[0].is_empty()),
        }
    }
}

/// Bytes que são especiais em padrão de glob (pra escapar o que veio entre aspas).
fn is_pattern_special(c: u8) -> bool {
    matches!(c, b'*' | b'?' | b'[' | b']' | b'\\' | b'(' | b')' | b'|' | b'!' | b'@' | b'+' | b'^' | b'-')
}

// ---- funções de caixa e de caractere, usadas também por `declare -l/-u/-c` ----

/// Fronteiras de caractere (UTF-8 quando `utf8`; byte inválido conta sozinho).
pub fn char_bounds(v: &[u8], utf8: bool) -> Vec<usize> {
    let mut out = Vec::with_capacity(v.len() + 1);
    let mut i = 0;
    while i < v.len() {
        out.push(i);
        i += if utf8 { utf8_char_len(&v[i..]) } else { 1 };
    }
    out.push(v.len());
    out
}

/// Tamanho do caractere UTF-8 no começo de `s` (1 se inválido).
pub fn utf8_char_len(s: &[u8]) -> usize {
    let c = s[0];
    let n = match c {
        0x00..=0x7F => return 1,
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => return 1,
    };
    if s.len() < n || !s[1..n].iter().all(|b| (0x80..=0xBF).contains(b)) {
        return 1;
    }
    if std::str::from_utf8(&s[..n]).is_err() {
        return 1;
    }
    n
}

pub fn char_count(v: &[u8], utf8: bool) -> usize {
    char_bounds(v, utf8).len() - 1
}

fn map_char(ch: &[u8], upper: bool, utf8: bool) -> Vec<u8> {
    if ch.len() == 1 {
        return vec![if upper { ch[0].to_ascii_uppercase() } else { ch[0].to_ascii_lowercase() }];
    }
    if !utf8 {
        return ch.to_vec();
    }
    match std::str::from_utf8(ch).ok().and_then(|s| s.chars().next()) {
        Some(c) => {
            let mut it: Vec<char> = if upper { c.to_uppercase().collect() } else { c.to_lowercase().collect() };
            // towupper/towlower mapeiam um caractere em um.
            if it.len() != 1 {
                it = vec![c];
            }
            it[0].to_string().into_bytes()
        }
        None => ch.to_vec(),
    }
}

fn is_upper_char(ch: &[u8]) -> bool {
    match std::str::from_utf8(ch).ok().and_then(|s| s.chars().next()) {
        Some(c) => c.is_uppercase(),
        None => false,
    }
}

pub fn case_all(v: &[u8], upper: bool, utf8: bool) -> Vec<u8> {
    let b = char_bounds(v, utf8);
    let mut out = Vec::with_capacity(v.len());
    for w in b.windows(2) {
        out.extend(map_char(&v[w[0]..w[1]], upper, utf8));
    }
    out
}

pub fn case_first(v: &[u8], upper: bool, utf8: bool) -> Vec<u8> {
    if v.is_empty() {
        return Vec::new();
    }
    let n = if utf8 { utf8_char_len(v) } else { 1 };
    let mut out = map_char(&v[..n], upper, utf8);
    out.extend_from_slice(&v[n..]);
    out
}

/// Separa um texto pelo IFS (o `read`, o `$*`, o split de campos usam a mesma regra).
pub fn ifs_split(v: &[u8], ifs: &[u8], max_fields: usize) -> Vec<Vec<u8>> {
    let mut f = Field::default();
    f.push(v, SPLIT);
    split_field(f, ifs, max_fields).into_iter().map(|x| x.text()).collect()
}

fn is_ifs_ws(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\n'
}

/// Divide um campo nos bytes divisíveis que estão no IFS. `max_fields` > 0 limita (o último
/// campo leva o resto, como no `read`).
fn split_field(f: Field, ifs: &[u8], max_fields: usize) -> Vec<Field> {
    let n = f.bytes.len();
    let delim = |i: usize| f.flags[i] & SPLIT != 0 && f.flags[i] & Q == 0 && ifs.contains(&f.bytes[i]);
    let ws = |i: usize| delim(i) && is_ifs_ws(f.bytes[i]);
    if !(0..n).any(delim) {
        return if f.is_empty() { Vec::new() } else { vec![f] };
    }
    let mut out: Vec<Field> = Vec::new();
    let mut i = 0;
    while i < n && ws(i) {
        i += 1;
    }
    while i < n {
        let mut cur = Field::default();
        if max_fields > 0 && out.len() + 1 == max_fields {
            // Último campo do `read`: o resto da linha, sem os brancos de IFS do fim.
            let mut end = n;
            while end > i && ws(end - 1) {
                end -= 1;
            }
            cur.bytes.extend_from_slice(&f.bytes[i..end]);
            cur.flags.extend_from_slice(&f.flags[i..end]);
            out.push(cur);
            return out;
        }
        while i < n && !delim(i) {
            cur.bytes.push(f.bytes[i]);
            cur.flags.push(f.flags[i]);
            i += 1;
        }
        out.push(cur);
        if i >= n {
            break;
        }
        // Consome a sequência de delimitadores.
        if ws(i) {
            while i < n && ws(i) {
                i += 1;
            }
            if i < n && delim(i) && !ws(i) {
                i += 1;
                while i < n && ws(i) {
                    i += 1;
                }
            }
        } else {
            i += 1;
            while i < n && ws(i) {
                i += 1;
            }
        }
    }
    out
}

/// Resultado de uma expansão em contexto de string (sem divisão nem glob).
fn join_fields(fields: Vec<Field>) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        out.extend(f.text());
    }
    out
}

impl Shell {
    pub fn match_opts(&self, for_glob: bool) -> MatchOpts {
        MatchOpts {
            extglob: self.opts.shopt("extglob"),
            nocase: if for_glob { self.opts.shopt("nocaseglob") } else { self.opts.shopt("nocasematch") },
            utf8: self.utf8(),
            period: for_glob,
            dotglob: self.opts.shopt("dotglob"),
        }
    }

    // ---- pontos de entrada ----

    /// Expansão completa (chaves, divisão, glob): argumentos de comando, listas do `for`.
    pub fn expand_words(&mut self, words: &[Word]) -> Result<Vec<Vec<u8>>, Flow> {
        let mut out = Vec::new();
        for w in words {
            out.extend(self.expand_word_fields(w)?);
        }
        Ok(out)
    }

    pub fn expand_word_fields(&mut self, w: &Word) -> Result<Vec<Vec<u8>>, Flow> {
        let alternatives = if self.opts.get("braceexpand") {
            brace_expand(&w.parts)
        } else if w.parts.iter().any(|p| matches!(p, Part::Brace(_) | Part::BraceSeq(_))) {
            let reparsed = crate::word::parse_word(&w.raw, crate::word::WordOpts::plain(self.lineno)).unwrap_or_default();
            vec![reparsed]
        } else {
            vec![w.parts.to_vec()]
        };
        let mut out = Vec::new();
        for parts in alternatives {
            let mut b = Builder::new();
            self.expand_parts(&parts, &mut b, false)?;
            let fields = self.split_fields(b.fields);
            for f in fields {
                self.glob_field(f, &mut out)?;
            }
        }
        Ok(out)
    }

    /// Expansão sem divisão nem glob (atribuição, alvo de here-string, palavra do `case`...).
    pub fn expand_word_string(&mut self, w: &Word) -> Result<Vec<u8>, Flow> {
        self.expand_parts_string(&w.parts)
    }

    pub fn expand_parts_string(&mut self, parts: &[Part]) -> Result<Vec<u8>, Flow> {
        let mut b = Builder::new();
        self.expand_parts(parts, &mut b, false)?;
        Ok(join_fields(b.fields))
    }

    /// Expansão sem divisão nem glob, preservando a separação dos elementos de `"$@"`
    /// (atribuição de array `a=("$@")` usa `expand_words`; isto é pra declarações).
    pub fn expand_word_pattern(&mut self, w: &Word) -> Result<Vec<u8>, Flow> {
        self.expand_parts_pattern(&w.parts)
    }

    /// Padrão: o que veio entre aspas é escapado com `\`.
    pub fn expand_parts_pattern(&mut self, parts: &[Part]) -> Result<Vec<u8>, Flow> {
        let mut b = Builder::new();
        self.expand_parts(parts, &mut b, false)?;
        let mut out = Vec::new();
        for (i, f) in b.fields.iter().enumerate() {
            if i > 0 {
                out.push(b' ');
            }
            for (c, fl) in f.bytes.iter().zip(&f.flags) {
                if fl & NULLMARK != 0 {
                    continue;
                }
                if fl & Q != 0 && is_pattern_special(*c) {
                    out.push(b'\\');
                }
                out.push(*c);
            }
        }
        Ok(out)
    }

    /// Regex do `=~`: o que veio entre aspas é escapado pra casar literal (ERE).
    pub fn expand_word_regex(&mut self, w: &Word) -> Result<Vec<u8>, Flow> {
        let mut b = Builder::new();
        self.expand_parts(&w.parts, &mut b, false)?;
        let mut out = Vec::new();
        for (i, f) in b.fields.iter().enumerate() {
            if i > 0 {
                out.push(b' ');
            }
            for (c, fl) in f.bytes.iter().zip(&f.flags) {
                if fl & NULLMARK != 0 {
                    continue;
                }
                if fl & Q != 0 && matches!(c, b'.' | b'[' | b']' | b'(' | b')' | b'{' | b'}' | b'*' | b'+' | b'?' | b'|' | b'^' | b'$' | b'\\') {
                    out.push(b'\\');
                }
                out.push(*c);
            }
        }
        Ok(out)
    }

    /// Expansão pra redireção: tem que dar exatamente uma palavra.
    pub fn expand_redirect_target(&mut self, w: &Word) -> Result<Option<Vec<u8>>, Flow> {
        let mut b = Builder::new();
        self.expand_parts(&w.parts, &mut b, false)?;
        let fields = self.split_fields(b.fields);
        let mut out = Vec::new();
        for f in fields {
            if self.posix {
                out.push(f.text());
            } else {
                self.glob_field(f, &mut out)?;
            }
        }
        if out.len() == 1 { Ok(out.pop()) } else { Ok(None) }
    }

    fn split_fields(&self, fields: Vec<Field>) -> Vec<Field> {
        let ifs = self.ifs();
        let mut out = Vec::new();
        for f in fields {
            if ifs.is_empty() {
                if !f.is_empty() {
                    out.push(f);
                }
                continue;
            }
            out.extend(split_field(f, &ifs, 0));
        }
        out
    }

    /// Expansão de caminhos de um campo.
    fn glob_field(&mut self, f: Field, out: &mut Vec<Vec<u8>>) -> Result<(), Flow> {
        let extglob = self.opts.shopt("extglob");
        let has_unquoted_meta = !self.opts.get("noglob")
            && f.bytes.iter().zip(&f.flags).any(|(c, fl)| fl & Q == 0 && (matches!(c, b'*' | b'?' | b'[') || (extglob && matches!(c, b'(' ))));
        if !has_unquoted_meta {
            out.push(f.text());
            return Ok(());
        }
        let mut pat = Vec::with_capacity(f.bytes.len());
        for (c, fl) in f.bytes.iter().zip(&f.flags) {
            if fl & NULLMARK != 0 {
                continue;
            }
            if fl & Q != 0 && is_pattern_special(*c) {
                pat.push(b'\\');
            }
            pat.push(*c);
        }
        if !crate::pattern::has_glob_meta(&pat, extglob) {
            out.push(f.text());
            return Ok(());
        }
        let matches = crate::glob::glob(self, &pat);
        if matches.is_empty() {
            if self.opts.shopt("failglob") {
                self.error_bytes(&[b"no match: ".as_slice(), &f.text()].concat());
                return Err(Flow::Discard);
            }
            if self.opts.shopt("nullglob") {
                return Ok(());
            }
            out.push(f.text());
            return Ok(());
        }
        out.extend(matches);
        Ok(())
    }

    // ---- partes ----

    fn expand_parts(&mut self, parts: &[Part], b: &mut Builder, in_dq: bool) -> Result<(), Flow> {
        for p in parts {
            self.expand_part(p, b, in_dq)?;
            if b.total_len() > MAX_EXPANSION {
                self.error("expansion too large");
                return Err(Flow::Discard);
            }
        }
        Ok(())
    }

    fn expand_part(&mut self, part: &Part, b: &mut Builder, in_dq: bool) -> Result<(), Flow> {
        match part {
            Part::Lit(v) => {
                let flag = if in_dq { Q } else { b.lit_flag };
                b.push(v, flag);
            }
            Part::Quoted(v) => {
                if v.is_empty() {
                    b.mark();
                } else {
                    b.push(v, Q);
                }
            }
            Part::AnsiC(body) => {
                let v = crate::quote::decode_ansi_c(body, self.utf8());
                if v.is_empty() {
                    b.mark();
                } else {
                    b.push(&v, Q);
                }
            }
            Part::Double(inner) => {
                let before = b.total_len();
                let saved = b.lit_flag;
                b.lit_flag = 0;
                self.expand_parts(inner, b, true)?;
                b.lit_flag = saved;
                let only_at_lists = !inner.is_empty() && inner.iter().all(is_at_list);
                if b.total_len() == before && !only_at_lists {
                    b.mark();
                }
            }
            Part::Tilde(prefix) => {
                let v = self.tilde(prefix);
                b.push(&v, Q);
            }
            Part::Param(pe) => self.expand_param(pe, b, in_dq)?,
            Part::CmdSub(cs) => {
                let out = self.command_subst(cs)?;
                b.push(&out, if in_dq { Q } else { SPLIT });
            }
            Part::Arith(a) => {
                let v = self.arith_expansion(a)?;
                b.push(v.to_string().as_bytes(), if in_dq { Q } else { SPLIT });
            }
            Part::Brace(_) | Part::BraceSeq(_) => {
                // Só aparece aqui com chaves desligadas por contexto; vira o texto literal.
                let text = brace_text(part);
                b.push(&text, if in_dq { Q } else { b.lit_flag });
            }
            Part::ProcSub(ps) => {
                let path = self.process_subst(ps)?;
                b.push(&path, Q);
            }
        }
        Ok(())
    }

    pub fn arith_expansion(&mut self, a: &ArithExp) -> Result<i64, Flow> {
        let text = self.expand_parts_string(&a.parts)?;
        self.arith_eval(&text)
    }

    /// `~`, `~+`, `~-`, `~N`, `~nome`.
    pub fn tilde(&mut self, prefix: &[u8]) -> Vec<u8> {
        let lit = || {
            let mut v = b"~".to_vec();
            v.extend_from_slice(prefix);
            v
        };
        match prefix {
            b"" => match self.var_bytes("HOME") {
                Some(h) => h.to_vec(),
                None => self.user_home(b"root").unwrap_or_else(lit),
            },
            b"+" => self.var_bytes("PWD").map(|v| v.to_vec()).unwrap_or_else(lit),
            b"-" => self.var_bytes("OLDPWD").map(|v| v.to_vec()).unwrap_or_else(lit),
            _ => {
                let s = String::from_utf8_lossy(prefix).into_owned();
                let (plus, digits) = if let Some(d) = s.strip_prefix('+') {
                    (Some(true), d)
                } else if let Some(d) = s.strip_prefix('-') {
                    (Some(false), d)
                } else {
                    (None, s.as_str())
                };
                if !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) {
                    let n: usize = digits.parse().unwrap_or(usize::MAX);
                    let mut stack = vec![self.var_bytes("PWD").map(|v| v.to_vec()).unwrap_or_default()];
                    stack.extend(self.dirstack.iter().rev().cloned());
                    let idx = if plus == Some(false) { stack.len().checked_sub(n + 1) } else { Some(n) };
                    return idx.and_then(|i| stack.get(i).cloned()).unwrap_or_else(lit);
                }
                self.user_home(prefix).unwrap_or_else(lit)
            }
        }
    }

    fn user_home(&self, user: &[u8]) -> Option<Vec<u8>> {
        let data = sysabi::sys::read_file(b"/etc/passwd").ok()?;
        for line in data.split(|c| *c == b'\n') {
            let fields: Vec<&[u8]> = line.split(|c| *c == b':').collect();
            if fields.len() >= 6 && fields[0] == user {
                return Some(fields[5].to_vec());
            }
        }
        None
    }

    // ---- parâmetros ----

    fn positional_list(&self, include_zero: bool) -> Vec<Vec<u8>> {
        let mut v = Vec::with_capacity(self.params.len() + 1);
        if include_zero {
            v.push(self.arg0.clone());
        }
        v.extend(self.params.iter().cloned());
        v
    }

    /// Lê o valor cru do parâmetro (sem operador).
    fn param_value(&mut self, name: &ParamName, index: Option<&Index>) -> Result<PVal, Flow> {
        match name {
            ParamName::Positional(0) => Ok(PVal::Str(Some(self.arg0.clone()))),
            ParamName::Positional(n) => Ok(PVal::Str(self.params.get(*n as usize - 1).cloned())),
            ParamName::Special(c) => Ok(match c {
                b'@' => PVal::List(self.params.clone(), false),
                b'*' => PVal::List(self.params.clone(), true),
                b'#' => PVal::Str(Some(self.params.len().to_string().into_bytes())),
                b'?' => PVal::Str(Some(self.status.to_string().into_bytes())),
                b'-' => {
                    let mut f = self.opts.flags();
                    if self.interactive {
                        f.push(b'i');
                    }
                    if self.dash_c {
                        f.push(b'c');
                    }
                    PVal::Str(Some(f))
                }
                b'$' => PVal::Str(Some(self.pid.to_string().into_bytes())),
                b'!' => PVal::Str(self.last_bg.map(|p| p.to_string().into_bytes())),
                b'0' => PVal::Str(Some(self.arg0.clone())),
                _ => PVal::Str(None),
            }),
            ParamName::Var(n) => {
                let var = self.lookup(n).map(|v| v.into_owned());
                match index {
                    None => Ok(PVal::Str(var.and_then(|v| v.scalar_value().map(|x| x.to_vec())))),
                    Some(Index::At) | Some(Index::Star) => {
                        let star = matches!(index, Some(Index::Star));
                        let items = match var.map(|v| v.value) {
                            None | Some(Value::Unset) => Vec::new(),
                            Some(Value::Scalar(s)) => vec![s],
                            Some(Value::Indexed(m)) => m.into_values().collect(),
                            Some(Value::Assoc(a)) => a.iter().map(|(_, v)| v.clone()).collect(),
                        };
                        Ok(PVal::List(items, star))
                    }
                    Some(Index::Expr(w)) => {
                        let key = self.expand_word_string(w)?;
                        self.element_value(n, var.as_ref(), &key).map(PVal::Str)
                    }
                }
            }
        }
    }

    /// `${a[chave]}`.
    fn element_value(&mut self, name: &str, var: Option<&Var>, key: &[u8]) -> Result<Option<Vec<u8>>, Flow> {
        let assoc = var.is_some_and(|v| matches!(v.value, Value::Assoc(_)) || v.attrs.has(Attrs::ASSOC));
        if assoc {
            return Ok(match var.map(|v| &v.value) {
                Some(Value::Assoc(a)) => a.get(key).cloned(),
                _ => None,
            });
        }
        let idx = self.arith_eval(key)?;
        let real = self.resolve_nameref(name);
        let Some(idx) = self.resolve_index(&real, idx) else {
            if var.is_some_and(|v| matches!(v.value, Value::Indexed(_))) {
                self.error(format!("{name}[{}]: bad array subscript", String::from_utf8_lossy(key)));
                return Err(Flow::Discard);
            }
            return Ok(None);
        };
        Ok(match var.map(|v| &v.value) {
            Some(Value::Indexed(m)) => m.get(&idx).cloned(),
            Some(Value::Scalar(s)) if idx == 0 => Some(s.clone()),
            _ => None,
        })
    }

    fn param_display(pe: &ParamExp) -> String {
        match &pe.name {
            ParamName::Var(n) => match &pe.index {
                Some(Index::At) => format!("{n}[@]"),
                Some(Index::Star) => format!("{n}[*]"),
                Some(Index::Expr(w)) => format!("{n}[{}]", w.raw),
                None => n.clone(),
            },
            ParamName::Positional(n) => format!("${n}"),
            ParamName::Special(c) => format!("${}", *c as char),
        }
    }

    fn unbound(&self, pe: &ParamExp) -> Flow {
        self.error(format!("{}: unbound variable", Self::param_display(pe)));
        Flow::Exit(127)
    }

    /// Resolve `${!x}`: o valor de `x` é o nome do parâmetro de fato.
    fn indirect_target(&mut self, pe: &ParamExp) -> Result<Option<(ParamName, Option<Index>)>, Flow> {
        let v = self.param_value(&pe.name, pe.index.as_ref())?;
        let target = match v {
            PVal::Str(Some(s)) => s,
            PVal::List(items, _) => items.join(&b' '),
            PVal::Str(None) => {
                if self.opts.get("nounset") {
                    return Err(self.unbound(pe));
                }
                return Ok(None);
            }
        };
        let t = String::from_utf8_lossy(&target).into_owned();
        let parsed = crate::word::parse_param_inner(&t, false, self.lineno).ok();
        match parsed {
            Some(p) if matches!(p.op, ParamOp::None) && !p.indirect => Ok(Some((p.name, p.index))),
            _ => {
                self.error(format!("{t}: invalid indirect expansion"));
                Err(Flow::Discard)
            }
        }
    }

    fn expand_param(&mut self, pe: &ParamExp, b: &mut Builder, in_dq: bool) -> Result<(), Flow> {
        if matches!(pe.op, ParamOp::Bad) {
            self.error(format!("{}: bad substitution", pe.raw));
            return Err(Flow::Discard);
        }
        // Formas que não leem o valor.
        match &pe.op {
            ParamOp::Names { prefix, star } => {
                let names: Vec<Vec<u8>> =
                    self.all_var_names().into_iter().filter(|n| n.starts_with(prefix.as_str())).map(String::into_bytes).collect();
                self.push_value(PVal::List(names, *star), b, in_dq);
                return Ok(());
            }
            ParamOp::Keys { star } => {
                let ParamName::Var(n) = &pe.name else { return Ok(()) };
                let keys = match self.lookup(n).map(|v| v.into_owned().value) {
                    Some(Value::Indexed(m)) => m.keys().map(|k| k.to_string().into_bytes()).collect(),
                    Some(Value::Assoc(a)) => a.keys().into_iter().cloned().collect(),
                    Some(Value::Scalar(_)) => vec![b"0".to_vec()],
                    _ => Vec::new(),
                };
                self.push_value(PVal::List(keys, *star), b, in_dq);
                return Ok(());
            }
            _ => {}
        }

        let (name, index) = if pe.indirect {
            match self.indirect_target(pe)? {
                Some(t) => t,
                None => {
                    // `${!x}` com x vazio: nada (com operador, segue como não definido).
                    (ParamName::Var(String::new()), None)
                }
            }
        } else {
            (pe.name.clone(), pe.index.clone())
        };
        let value = if matches!(&name, ParamName::Var(n) if n.is_empty()) {
            PVal::Str(None)
        } else {
            self.param_value(&name, index.as_ref())?
        };
        let shown = ParamExp { name: name.clone(), index: index.clone(), indirect: false, op: ParamOp::None, braced: true, raw: pe.raw.clone() };

        match &pe.op {
            ParamOp::None => {
                if value.is_unset() && self.opts.get("nounset") && !is_at_or_star(&name, index.as_ref()) {
                    return Err(self.unbound(&shown));
                }
                self.push_value(value, b, in_dq);
            }
            ParamOp::Length => {
                let n = match &value {
                    PVal::List(items, _) => items.len(),
                    PVal::Str(None) => {
                        if self.opts.get("nounset") {
                            return Err(self.unbound(&shown));
                        }
                        0
                    }
                    PVal::Str(Some(s)) => char_count(s, self.utf8()),
                };
                // `${#@}`, `${#*}`: número de parâmetros.
                b.push(n.to_string().as_bytes(), if in_dq { Q } else { SPLIT });
            }
            ParamOp::Default { colon, kind, word } => {
                let test = if *colon { value.is_null() } else { value.is_unset() };
                match kind {
                    DefaultKind::Use => {
                        if test {
                            self.expand_sub_word(word, b, in_dq)?;
                        } else {
                            self.push_value(value, b, in_dq);
                        }
                    }
                    DefaultKind::Alt => {
                        if !test {
                            self.expand_sub_word(word, b, in_dq)?;
                        } else if in_dq {
                            // `"${x+y}"` com x não definido ainda é um campo vazio.
                        }
                    }
                    DefaultKind::Assign => {
                        if test {
                            let v = self.expand_parts_string(word)?;
                            match &name {
                                ParamName::Var(n) => {
                                    let ok = match index.as_ref() {
                                        Some(Index::Expr(w)) => {
                                            let key = self.expand_word_string(w)?;
                                            self.assign_element(n, &key, v.clone(), false)?
                                        }
                                        _ => self.assign_scalar(n, v.clone(), false)?,
                                    };
                                    if !ok {
                                        return Err(Flow::Discard);
                                    }
                                    let now = self.param_value(&name, index.as_ref())?;
                                    self.push_value(now, b, in_dq);
                                }
                                _ => {
                                    self.error(format!("{}: cannot assign in this way", Self::param_display(&shown)));
                                    return Err(Flow::Discard);
                                }
                            }
                        } else {
                            self.push_value(value, b, in_dq);
                        }
                    }
                    DefaultKind::Error => {
                        if test {
                            let msg = if word.is_empty() {
                                if *colon { b"parameter null or not set".to_vec() } else { b"parameter not set".to_vec() }
                            } else {
                                self.expand_parts_string(word)?
                            };
                            let shown_name = match &name {
                                ParamName::Var(n) => n.clone(),
                                other => Self::param_display(&ParamExp { name: other.clone(), ..shown.clone() }).trim_start_matches('$').to_string(),
                            };
                            let mut line = format!("{shown_name}: ").into_bytes();
                            line.extend(msg);
                            self.error_bytes(&line);
                            return Err(Flow::Exit(127));
                        }
                        self.push_value(value, b, in_dq);
                    }
                }
            }
            ParamOp::RemovePrefix { longest, pattern } | ParamOp::RemoveSuffix { longest, pattern } => {
                let prefix = matches!(pe.op, ParamOp::RemovePrefix { .. });
                if value.is_unset() && self.opts.get("nounset") && !is_at_or_star(&name, index.as_ref()) {
                    return Err(self.unbound(&shown));
                }
                let pat = self.expand_parts_pattern(pattern)?;
                let p = Pattern::new(&pat, self.match_opts(false));
                let f = |s: Vec<u8>| -> Vec<u8> {
                    if prefix {
                        match p.match_prefix(&s, *longest) {
                            Some(end) => s[end..].to_vec(),
                            None => s,
                        }
                    } else {
                        match p.match_suffix(&s, *longest) {
                            Some(start) => s[..start].to_vec(),
                            None => s,
                        }
                    }
                };
                let v = map_pval(value, f);
                self.push_value(v, b, in_dq);
            }
            ParamOp::Replace { kind, pattern, replacement } => {
                if value.is_unset() && self.opts.get("nounset") && !is_at_or_star(&name, index.as_ref()) {
                    return Err(self.unbound(&shown));
                }
                let pat = self.expand_parts_pattern(pattern)?;
                let rep = match replacement {
                    Some(r) => self.expand_replacement(r)?,
                    None => Vec::new(),
                };
                let p = Pattern::new(&pat, self.match_opts(false));
                let amp = self.opts.shopt("patsub_replacement");
                let utf8 = self.utf8();
                let v = map_pval(value, |s| replace(&p, &s, &rep, *kind, amp, utf8, pat.is_empty()));
                self.push_value(v, b, in_dq);
            }
            ParamOp::Substring { offset, length } => {
                if value.is_unset() && self.opts.get("nounset") && !is_at_or_star(&name, index.as_ref()) {
                    return Err(self.unbound(&shown));
                }
                let off_text = self.expand_parts_string(offset)?;
                let off = self.arith_eval(&off_text)?;
                let len = match length {
                    Some(l) => {
                        let t = self.expand_parts_string(l)?;
                        Some(self.arith_eval(&t)?)
                    }
                    None => None,
                };
                let is_positional_list = matches!(name, ParamName::Special(b'@') | ParamName::Special(b'*'));
                let v = match value {
                    PVal::Str(s) => {
                        let s = s.unwrap_or_default();
                        let utf8 = self.utf8();
                        match substring(&s, off, len, utf8) {
                            Some(x) => PVal::Str(Some(x)),
                            None => {
                                self.error(format!("{}: substring expression < 0", len.unwrap_or(0)));
                                return Err(Flow::Discard);
                            }
                        }
                    }
                    PVal::List(items, star) => {
                        let items = if is_positional_list { self.positional_list(true) } else { items };
                        match slice_list(&items, off, len, is_positional_list) {
                            Some(x) => PVal::List(x, star),
                            None => {
                                self.error(format!("{}: substring expression < 0", len.unwrap_or(0)));
                                return Err(Flow::Discard);
                            }
                        }
                    }
                };
                self.push_value(v, b, in_dq);
            }
            ParamOp::Case { op, all, pattern } => {
                if value.is_unset() && self.opts.get("nounset") && !is_at_or_star(&name, index.as_ref()) {
                    return Err(self.unbound(&shown));
                }
                let pat = match pattern {
                    Some(p) => Some(Pattern::new(&self.expand_parts_pattern(p)?, self.match_opts(false))),
                    None => None,
                };
                let utf8 = self.utf8();
                let v = map_pval(value, |s| change_case(&s, *op, *all, pat.as_ref(), utf8));
                self.push_value(v, b, in_dq);
            }
            ParamOp::Transform(c) => {
                if value.is_unset() && self.opts.get("nounset") && !is_at_or_star(&name, index.as_ref()) {
                    return Err(self.unbound(&shown));
                }
                let v = self.transform(*c, &name, index.as_ref(), value)?;
                self.push_value(v, b, in_dq);
            }
            ParamOp::Names { .. } | ParamOp::Keys { .. } | ParamOp::Bad => {}
        }
        Ok(())
    }

    /// Palavra de `${x:-w}`: sem aspas, os literais dela também dividem.
    fn expand_sub_word(&mut self, word: &[Part], b: &mut Builder, in_dq: bool) -> Result<(), Flow> {
        let saved = b.lit_flag;
        if !in_dq {
            b.lit_flag = SPLIT;
        }
        let before = b.total_len();
        let r = self.expand_parts(word, b, in_dq);
        b.lit_flag = saved;
        r?;
        if in_dq && b.total_len() == before {
            // `"${x:-}"` é um campo vazio.
        }
        Ok(())
    }

    /// Substituição de `${x/p/r}`: `&` sem aspas vira o trecho casado (patsub_replacement).
    fn expand_replacement(&mut self, parts: &[Part]) -> Result<Vec<u8>, Flow> {
        let mut b = Builder::new();
        self.expand_parts(parts, &mut b, false)?;
        // Marca `&` literal escapando com `\`; `&` sem aspas fica como está.
        let mut out = Vec::new();
        for f in &b.fields {
            for (c, fl) in f.bytes.iter().zip(&f.flags) {
                if fl & NULLMARK != 0 {
                    continue;
                }
                if fl & Q != 0 && (*c == b'&' || *c == b'\\') {
                    out.push(b'\\');
                }
                out.push(*c);
            }
        }
        Ok(out)
    }

    /// Coloca um valor no construtor, com as regras de aspas.
    fn push_value(&mut self, v: PVal, b: &mut Builder, in_dq: bool) {
        match v {
            PVal::Str(s) => {
                let s = s.unwrap_or_default();
                b.push(&s, if in_dq { Q } else { SPLIT });
            }
            PVal::List(items, star) => {
                if in_dq && star {
                    let ifs = self.ifs();
                    let sep: Vec<u8> = match self.vars.get("IFS") {
                        None => vec![b' '],
                        Some(_) => ifs.first().map(|c| vec![*c]).unwrap_or_default(),
                    };
                    let joined = items.join(sep.as_slice());
                    if joined.is_empty() {
                        b.mark();
                    } else {
                        b.push(&joined, Q);
                    }
                    return;
                }
                let flag = if in_dq { Q } else { SPLIT };
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        b.break_field();
                    }
                    if it.is_empty() && in_dq {
                        b.mark();
                    } else {
                        b.push(it, flag);
                    }
                }
            }
        }
    }

    /// Nomes de todas as variáveis visíveis, ordenados (pra `${!prefixo@}`).
    pub fn all_var_names(&self) -> Vec<String> {
        self.vars.names().into_iter().filter(|n| self.vars.get(n).is_some_and(|v| v.is_set() || v.attrs.0 != 0)).collect()
    }

    fn transform(&mut self, op: u8, name: &ParamName, index: Option<&Index>, value: PVal) -> Result<PVal, Flow> {
        let utf8 = self.utf8();
        Ok(match op {
            b'Q' => map_pval_opt(value, |s| s.map(|v| crate::quote::single_quote(&v))),
            b'E' => map_pval(value, |s| crate::quote::decode_ansi_c(&s, utf8)),
            b'P' => map_pval(value, |s| self.prompt_expand(&s)),
            b'U' => map_pval(value, |s| case_all(&s, true, utf8)),
            b'u' => map_pval(value, |s| case_first(&s, true, utf8)),
            b'L' => map_pval(value, |s| case_all(&s, false, utf8)),
            b'a' => {
                let attrs = match name {
                    ParamName::Var(n) => self.lookup(n).map(|v| crate::builtins::declare::attr_letters(&v)).unwrap_or_default(),
                    _ => String::new(),
                };
                match value {
                    PVal::List(items, star) => PVal::List(items.iter().map(|_| attrs.clone().into_bytes()).collect(), star),
                    PVal::Str(_) => PVal::Str(Some(attrs.into_bytes())),
                }
            }
            b'A' => {
                let ParamName::Var(n) = name else { return Ok(PVal::Str(None)) };
                if value.is_unset() {
                    return Ok(PVal::Str(None));
                }
                let var = self.lookup(n).map(|v| v.into_owned());
                match (var, index) {
                    (Some(v), None) if !v.is_array() => {
                        let letters = crate::builtins::declare::attr_letters(&v);
                        let val = crate::quote::single_quote(v.scalar_value().unwrap_or_default());
                        let mut out = Vec::new();
                        if !letters.is_empty() {
                            out.extend_from_slice(format!("declare -{letters} ").as_bytes());
                        }
                        out.extend_from_slice(n.as_bytes());
                        out.push(b'=');
                        out.extend(val);
                        PVal::Str(Some(out))
                    }
                    (Some(v), _) => PVal::Str(Some(crate::builtins::declare::declare_line(n, &v).into_bytes())),
                    (None, _) => PVal::Str(None),
                }
            }
            b'K' | b'k' => {
                let ParamName::Var(n) = name else { return Ok(value) };
                let var = self.lookup(n).map(|v| v.into_owned());
                let pairs: Vec<(Vec<u8>, Vec<u8>)> = match var.map(|v| v.value) {
                    Some(Value::Indexed(m)) if index.is_some_and(|i| matches!(i, Index::At | Index::Star)) => {
                        m.into_iter().map(|(k, v)| (k.to_string().into_bytes(), v)).collect()
                    }
                    Some(Value::Assoc(a)) if index.is_some_and(|i| matches!(i, Index::At | Index::Star)) => {
                        a.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                    }
                    _ => return Ok(map_pval_opt(value, |s| s.map(|v| crate::quote::single_quote(&v)))),
                };
                let mut items = Vec::new();
                for (k, v) in pairs {
                    if op == b'K' {
                        items.push(k);
                        items.push(crate::quote::double_quote_value(&v));
                    } else {
                        items.push(k);
                        items.push(v);
                    }
                }
                if op == b'K' {
                    PVal::Str(Some(items.join(&b' ')))
                } else {
                    PVal::List(items, false)
                }
            }
            _ => value,
        })
    }

    /// Expansão de prompt (`\u`, `\h`, `\w`, `\$`...), como `${x@P}` e o PS1.
    pub fn prompt_expand(&mut self, s: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < s.len() {
            if s[i] != b'\\' || i + 1 >= s.len() {
                out.push(s[i]);
                i += 1;
                continue;
            }
            let c = s[i + 1];
            i += 2;
            match c {
                b'u' => out.extend(self.var_bytes("USER").unwrap_or(b"root")),
                b'h' => {
                    let h = sys().uname().nodename;
                    out.extend(h.split(|c| *c == b'.').next().unwrap_or(&[]));
                }
                b'H' => out.extend(sys().uname().nodename),
                b'w' | b'W' => {
                    let pwd = self.var_bytes("PWD").unwrap_or(b"").to_vec();
                    let home = self.var_bytes("HOME").unwrap_or(b"").to_vec();
                    let shown = if !home.is_empty() && pwd.starts_with(&home) && (pwd.len() == home.len() || pwd[home.len()] == b'/') {
                        let mut v = b"~".to_vec();
                        v.extend_from_slice(&pwd[home.len()..]);
                        v
                    } else {
                        pwd
                    };
                    if c == b'W' && shown != b"~" && shown != b"/" {
                        out.extend(shown.rsplit(|c| *c == b'/').next().unwrap_or(&[]));
                    } else {
                        out.extend(shown);
                    }
                }
                b'$' => out.push(if sys().geteuid() == 0 { b'#' } else { b'$' }),
                b's' => out.extend(b"bash"),
                b'v' => out.extend(b"5.2"),
                b'V' => out.extend(b"5.2.37"),
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b'a' => out.push(7),
                b'e' => out.push(27),
                b'\\' => out.push(b'\\'),
                b'[' | b']' => {}
                b'0'..=b'7' => {
                    let mut v: u32 = (c - b'0') as u32;
                    let mut k = 0;
                    while k < 2 && i < s.len() && (b'0'..=b'7').contains(&s[i]) {
                        v = v * 8 + (s[i] - b'0') as u32;
                        i += 1;
                        k += 1;
                    }
                    out.push(v as u8);
                }
                _ => {
                    out.push(b'\\');
                    out.push(c);
                }
            }
        }
        out
    }

    // ---- substituições ----

    /// Roda `$(...)` num subshell e devolve a saída sem os newlines finais.
    pub fn command_subst(&mut self, cs: &CmdSub) -> Result<Vec<u8>, Flow> {
        // `$(< arquivo)`: lê direto, sem subshell.
        if let Some(path_word) = cat_shortcut(&cs.program) {
            let path = match self.expand_redirect_target(&path_word)? {
                Some(p) => p,
                None => {
                    self.error(format!("{}: ambiguous redirect", path_word.raw));
                    self.status = 1;
                    return Ok(Vec::new());
                }
            };
            return match sysabi::sys::read_file(&path) {
                Ok(mut data) => {
                    while data.last() == Some(&b'\n') {
                        data.pop();
                    }
                    self.last_cmdsub_status = Some(0);
                    Ok(data)
                }
                Err(e) => {
                    self.error_bytes(&[path.as_slice(), b": ", e.message().as_bytes()].concat());
                    self.last_cmdsub_status = Some(1);
                    Ok(Vec::new())
                }
            };
        }
        let s = sys();
        let (r, w) = match s.pipe2(OFlags::CLOEXEC) {
            Ok(p) => p,
            Err(e) => {
                self.error(format!("cannot make pipe for command substitution: {}", e.message()));
                return Err(Flow::Discard);
            }
        };
        let program = cs.program.clone();
        let mut child = self.subshell_clone();
        child.xtrace_level += 1;
        if !self.opts.shopt("inherit_errexit") {
            child.opts.set("errexit", false);
        }
        let attrs = ProcAttrs {
            fd_actions: vec![FdAction::Dup2 { from: w, to: Fd::STDOUT }, FdAction::Close(r), FdAction::Close(w)],
            ..ProcAttrs::default()
        };
        let spawned = s.spawn_fn(attrs, b"bash".to_vec(), Box::new(move || child.run_subshell_program(&program)));
        let _ = s.close(w);
        let pid = match spawned {
            Ok(p) => p,
            Err(e) => {
                let _ = s.close(r);
                self.error(format!("fork: {}", e.message()));
                return Err(Flow::Discard);
            }
        };
        let mut out = Vec::new();
        let mut buf = vec![0u8; 65536];
        loop {
            match s.read(r, &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if out.len() + n > MAX_EXPANSION {
                        break;
                    }
                    out.extend_from_slice(&buf[..n]);
                }
                Err(Errno::EINTR) => {
                    let _ = self.run_pending_traps();
                }
                Err(_) => break,
            }
        }
        let _ = s.close(r);
        let st = self.wait_pid(pid);
        self.last_cmdsub_status = Some(st);
        if out.contains(&0) {
            self.error("warning: command substitution: ignored null byte in input");
            out.retain(|c| *c != 0);
        }
        while out.last() == Some(&b'\n') {
            out.pop();
        }
        Ok(out)
    }

    /// `<(...)` / `>(...)`: devolve `/dev/fd/N`.
    pub fn process_subst(&mut self, ps: &ProcSub) -> Result<Vec<u8>, Flow> {
        let s = sys();
        let (r, w) = match s.pipe2(OFlags::empty()) {
            Ok(p) => p,
            Err(e) => {
                self.error(format!("cannot make pipe for process substitution: {}", e.message()));
                return Err(Flow::Discard);
            }
        };
        let (keep, give) = if ps.write { (w, r) } else { (r, w) };
        let target_fd = if ps.write { Fd::STDIN } else { Fd::STDOUT };
        let body = ps.body.clone();
        let child = self.subshell_clone();
        let attrs = ProcAttrs {
            fd_actions: vec![FdAction::Dup2 { from: give, to: target_fd }, FdAction::Close(r), FdAction::Close(w)],
            ..ProcAttrs::default()
        };
        let mut child = child;
        let spawned = s.spawn_fn(attrs, b"bash".to_vec(), Box::new(move || child.run_subshell_list(&body)));
        let _ = s.close(give);
        let pid = match spawned {
            Ok(p) => p,
            Err(e) => {
                let _ = s.close(keep);
                self.error(format!("fork: {}", e.message()));
                return Err(Flow::Discard);
            }
        };
        self.last_bg = Some(pid);
        self.procsub_pids.push(pid);
        // Como o bash: o fd vai pro maior número livre abaixo de 64.
        let open = s.open_fds();
        let mut target = 63;
        while target > 3 && open.contains(&Fd(target)) {
            target -= 1;
        }
        let fd = if keep.0 == target {
            keep
        } else {
            match s.dup3(keep, Fd(target), false) {
                Ok(fd) => {
                    let _ = s.close(keep);
                    fd
                }
                Err(_) => keep,
            }
        };
        self.procsub_fds.push(fd);
        Ok(format!("/dev/fd/{}", fd.0).into_bytes())
    }

    /// Fecha os fds de substituição de processo do comando que terminou e colhe quem já saiu.
    pub fn cleanup_procsubs(&mut self) {
        let s = sys();
        for fd in std::mem::take(&mut self.procsub_fds) {
            let _ = s.close(fd);
        }
        let pids = std::mem::take(&mut self.procsub_pids);
        for pid in pids {
            match s.wait4(WaitTarget::Pid(pid), WaitOptions::NOHANG) {
                Ok(Some(_)) | Err(_) => {}
                Ok(None) => self.procsub_pids.push(pid),
            }
        }
    }
}

/// Teto de tamanho de uma expansão (evita derrubar o processo com `$(yes)` e afins).
const MAX_EXPANSION: usize = 1 << 30;

fn is_at_list(p: &Part) -> bool {
    match p {
        Part::Param(pe) => {
            matches!(pe.op, ParamOp::None)
                && (matches!(pe.name, ParamName::Special(b'@')) || matches!(pe.index, Some(Index::At)))
                && !pe.indirect
        }
        _ => false,
    }
}

fn is_at_or_star(name: &ParamName, index: Option<&Index>) -> bool {
    matches!(name, ParamName::Special(b'@') | ParamName::Special(b'*')) || matches!(index, Some(Index::At) | Some(Index::Star))
}

fn map_pval(v: PVal, mut f: impl FnMut(Vec<u8>) -> Vec<u8>) -> PVal {
    match v {
        PVal::Str(Some(s)) => PVal::Str(Some(f(s))),
        PVal::Str(None) => PVal::Str(None),
        PVal::List(items, star) => PVal::List(items.into_iter().map(f).collect(), star),
    }
}

fn map_pval_opt(v: PVal, mut f: impl FnMut(Option<Vec<u8>>) -> Option<Vec<u8>>) -> PVal {
    match v {
        PVal::Str(s) => PVal::Str(f(s)),
        PVal::List(items, star) => PVal::List(items.into_iter().map(|x| f(Some(x)).unwrap_or_default()).collect(), star),
    }
}

/// `${x:off:len}` em caracteres. `None` quando o comprimento negativo passa do início.
fn substring(s: &[u8], off: i64, len: Option<i64>, utf8: bool) -> Option<Vec<u8>> {
    let b = char_bounds(s, utf8);
    let n = (b.len() - 1) as i64;
    let mut start = if off < 0 { n + off } else { off };
    if start < 0 {
        return Some(Vec::new());
    }
    if start > n {
        start = n;
    }
    let end = match len {
        None => n,
        Some(l) if l < 0 => {
            let e = n + l;
            if e < start {
                return if e < 0 { None } else { Some(Vec::new()) };
            }
            e
        }
        Some(l) => (start + l).min(n),
    };
    Some(s[b[start as usize]..b[end as usize]].to_vec())
}

/// `${@:off:len}` e `${a[@]:off:len}`.
fn slice_list(items: &[Vec<u8>], off: i64, len: Option<i64>, _positional: bool) -> Option<Vec<Vec<u8>>> {
    let n = items.len() as i64;
    let start = if off < 0 { n + off } else { off };
    if start < 0 || start > n {
        return Some(Vec::new());
    }
    let end = match len {
        None => n,
        Some(l) if l < 0 => {
            let e = n + l;
            if e < 0 {
                return None;
            }
            e.max(start)
        }
        Some(l) => (start + l).min(n),
    };
    Some(items[start as usize..end as usize].to_vec())
}

/// `${x/p/r}` e família.
fn replace(p: &Pattern, s: &[u8], rep: &[u8], kind: ReplaceKind, amp: bool, utf8: bool, empty_pattern: bool) -> Vec<u8> {
    let render = |m: &[u8]| -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < rep.len() {
            match rep[i] {
                b'\\' if i + 1 < rep.len() && (rep[i + 1] == b'&' || rep[i + 1] == b'\\') => {
                    out.push(rep[i + 1]);
                    i += 2;
                }
                b'&' if amp => {
                    out.extend_from_slice(m);
                    i += 1;
                }
                c => {
                    out.push(c);
                    i += 1;
                }
            }
        }
        out
    };
    if empty_pattern && !matches!(kind, ReplaceKind::Prefix | ReplaceKind::Suffix) {
        return s.to_vec();
    }
    match kind {
        ReplaceKind::Prefix => match p.match_prefix(s, true) {
            Some(end) => {
                let mut out = render(&s[..end]);
                out.extend_from_slice(&s[end..]);
                out
            }
            None => s.to_vec(),
        },
        ReplaceKind::Suffix => match p.match_suffix(s, true) {
            Some(start) => {
                let mut out = s[..start].to_vec();
                out.extend(render(&s[start..]));
                out
            }
            None => s.to_vec(),
        },
        ReplaceKind::First | ReplaceKind::All => {
            let mut out = Vec::new();
            let mut pos = 0;
            while pos <= s.len() {
                match p.find(s, pos) {
                    Some((a, e)) => {
                        out.extend_from_slice(&s[pos..a]);
                        out.extend(render(&s[a..e]));
                        if e == a {
                            // Casamento vazio: copia um caractere e anda.
                            if a < s.len() {
                                let n = if utf8 { utf8_char_len(&s[a..]) } else { 1 };
                                out.extend_from_slice(&s[a..a + n]);
                                pos = a + n;
                            } else {
                                pos = a + 1;
                            }
                        } else {
                            pos = e;
                        }
                        if kind == ReplaceKind::First {
                            if pos <= s.len() {
                                out.extend_from_slice(&s[pos.min(s.len())..]);
                            }
                            return out;
                        }
                    }
                    None => {
                        out.extend_from_slice(&s[pos..]);
                        return out;
                    }
                }
            }
            out
        }
    }
}

fn change_case(s: &[u8], op: CaseOp, all: bool, pat: Option<&Pattern>, utf8: bool) -> Vec<u8> {
    let b = char_bounds(s, utf8);
    let mut out = Vec::with_capacity(s.len());
    for (i, w) in b.windows(2).enumerate() {
        let ch = &s[w[0]..w[1]];
        let applies = (all || i == 0) && pat.is_none_or(|p| p.matches(ch));
        if !applies {
            out.extend_from_slice(ch);
            continue;
        }
        let mapped = match op {
            CaseOp::Upper => map_char(ch, true, utf8),
            CaseOp::Lower => map_char(ch, false, utf8),
            CaseOp::Toggle => {
                if is_upper_char(ch) {
                    map_char(ch, false, utf8)
                } else {
                    map_char(ch, true, utf8)
                }
            }
        };
        out.extend(mapped);
    }
    out
}

/// `$(< arquivo)`: programa que é um único `< palavra` sem comando.
fn cat_shortcut(p: &Program) -> Option<Word> {
    if p.commands.len() != 1 {
        return None;
    }
    let list = &p.commands[0];
    if list.items.len() != 1 || list.items[0].background {
        return None;
    }
    let ao = &list.items[0].and_or;
    if !ao.rest.is_empty() || ao.first.negated || ao.first.commands.len() != 1 || ao.first.time.is_some() {
        return None;
    }
    let Command::Simple(s) = &ao.first.commands[0] else { return None };
    if !s.words.is_empty() || !s.assigns.is_empty() || s.redirects.len() != 1 {
        return None;
    }
    let r = &s.redirects[0];
    if r.op != RedirOp::Read || !matches!(r.fd, RedirFd::Default | RedirFd::Num(0)) {
        return None;
    }
    match &r.target {
        RedirTarget::Word(w) => Some(w.clone()),
        _ => None,
    }
}

/// Texto literal de uma expressão de chaves (quando as chaves não expandem).
fn brace_text(p: &Part) -> Vec<u8> {
    match p {
        Part::BraceSeq(BraceSeq::Num { start, end, step, .. }) => {
            if *step == 1 { format!("{{{start}..{end}}}").into_bytes() } else { format!("{{{start}..{end}..{step}}}").into_bytes() }
        }
        Part::BraceSeq(BraceSeq::Char { start, end, step }) => {
            if *step == 1 {
                format!("{{{}..{}}}", *start as char, *end as char).into_bytes()
            } else {
                format!("{{{}..{}..{step}}}", *start as char, *end as char).into_bytes()
            }
        }
        Part::Brace(alts) => {
            let mut out = b"{".to_vec();
            for (i, a) in alts.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                for x in a {
                    match x {
                        Part::Lit(v) | Part::Quoted(v) => out.extend_from_slice(v),
                        other => out.extend(brace_text(other)),
                    }
                }
            }
            out.push(b'}');
            out
        }
        _ => Vec::new(),
    }
}

/// Expansão de chaves: cada combinação vira uma lista de partes.
pub fn brace_expand(parts: &[Part]) -> Vec<Vec<Part>> {
    let Some(pos) = parts.iter().position(|p| matches!(p, Part::Brace(_) | Part::BraceSeq(_))) else {
        return vec![parts.to_vec()];
    };
    let prefix = &parts[..pos];
    let suffix = &parts[pos + 1..];
    let alternatives: Vec<Vec<Part>> = match &parts[pos] {
        Part::Brace(alts) => alts.clone(),
        Part::BraceSeq(seq) => seq_items(seq).into_iter().map(|t| vec![Part::Lit(t)]).collect(),
        _ => unreachable_alts(),
    };
    let mut out = Vec::new();
    for alt in alternatives {
        let mut combined: Vec<Part> = prefix.to_vec();
        combined.extend(alt);
        combined.extend_from_slice(suffix);
        for e in brace_expand(&combined) {
            out.push(e);
            if out.len() > 4_000_000 {
                return out;
            }
        }
    }
    out
}

fn unreachable_alts() -> Vec<Vec<Part>> {
    Vec::new()
}

fn seq_items(seq: &BraceSeq) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    match *seq {
        BraceSeq::Num { start, end, step, width } => {
            let step = if step == 0 { 1 } else { step.unsigned_abs() as i64 };
            let fmt = |n: i64| {
                if width > 0 {
                    if n < 0 { format!("-{:0w$}", -n, w = width - 1) } else { format!("{n:0width$}") }
                } else {
                    n.to_string()
                }
            };
            if start <= end {
                let mut n = start;
                while n <= end {
                    out.push(fmt(n).into_bytes());
                    if out.len() > 4_000_000 {
                        break;
                    }
                    n = match n.checked_add(step) {
                        Some(x) => x,
                        None => break,
                    };
                }
            } else {
                let mut n = start;
                while n >= end {
                    out.push(fmt(n).into_bytes());
                    if out.len() > 4_000_000 {
                        break;
                    }
                    n = match n.checked_sub(step) {
                        Some(x) => x,
                        None => break,
                    };
                }
            }
        }
        BraceSeq::Char { start, end, step } => {
            let step = if step == 0 { 1 } else { step.unsigned_abs() as i64 };
            let (s, e) = (start as i64, end as i64);
            if s <= e {
                let mut c = s;
                while c <= e {
                    out.push(vec![c as u8]);
                    c += step;
                }
            } else {
                let mut c = s;
                while c >= e {
                    out.push(vec![c as u8]);
                    c -= step;
                }
            }
        }
    }
    out
}

/// Um pedido de leitura de fd sem bloquear o resto (helper pros builtins que leem).
pub fn read_all_fd(fd: Fd) -> Result<Vec<u8>, Errno> {
    sysabi::sys::read_to_end(fd)
}

/// Usado pelos testes de unidade de expansão.
pub fn split_for_test(v: &[u8], ifs: &[u8]) -> Vec<Vec<u8>> {
    ifs_split(v, ifs, 0)
}

#[allow(dead_code)]
fn _assert_send() {
    fn is_send<T: Send>() {}
    is_send::<Arc<Program>>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ifs_splitting_rules() {
        let s = |v: &str, ifs: &str| -> Vec<String> {
            ifs_split(v.as_bytes(), ifs.as_bytes(), 0).into_iter().map(|x| String::from_utf8(x).unwrap()).collect()
        };
        assert_eq!(s("a   b    c", " \t\n"), ["a", "b", "c"]);
        assert_eq!(s("  a b  ", " \t\n"), ["a", "b"]);
        assert_eq!(s("a,,b", ","), ["a", "", "b"]);
        assert_eq!(s("a,b,", ","), ["a", "b"]);
        assert_eq!(s(",a", ","), ["", "a"]);
        assert_eq!(s("a , b", " ,"), ["a", "b"]);
        assert_eq!(s("p,q,r", ","), ["p", "q", "r"]);
    }

    #[test]
    fn substring_by_chars() {
        assert_eq!(substring(b"abcdefghij", 2, Some(3), true).unwrap(), b"cde");
        assert_eq!(substring(b"abcdefghij", -3, None, true).unwrap(), b"hij");
        assert_eq!(substring(b"abcdefghij", -4, Some(2), true).unwrap(), b"gh");
        assert_eq!(substring("ação ñ".as_bytes(), 0, Some(4), true).unwrap(), "ação".as_bytes());
    }

    #[test]
    fn brace_sequences() {
        let items = seq_items(&BraceSeq::Num { start: 1, end: 10, step: 3, width: 2 });
        assert_eq!(items, vec![b"01".to_vec(), b"04".to_vec(), b"07".to_vec(), b"10".to_vec()]);
        let items = seq_items(&BraceSeq::Char { start: b'e', end: b'a', step: 2 });
        assert_eq!(items, vec![b"e".to_vec(), b"c".to_vec(), b"a".to_vec()]);
    }
}
