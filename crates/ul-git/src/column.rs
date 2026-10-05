//! Saída em colunas do git (`column.c`): as opções de `--column[=<opções>]`, `column.ui` e
//! `column.<comando>`, e o layout por coluna ou por linha, com e sem `dense`.

use crate::config::Config;
use crate::error::{Fail, R, error};
use crate::os;

pub const LAYOUT_MASK: u32 = 0x000f;
pub const ENABLE_MASK: u32 = 0x0030;
/// O valor veio de `--column`/`--no-column` (o `explicitly_enable_column`).
pub const PARSEOPT: u32 = 0x0040;
pub const DENSE: u32 = 0x0080;

pub const DISABLED: u32 = 0x0000;
pub const ENABLED: u32 = 0x0010;
pub const AUTO: u32 = 0x0020;

pub const COLUMN: u32 = 0;
pub const ROW: u32 = 1;
pub const PLAIN: u32 = 15;

const ENABLE_SET: u32 = 1;
const LAYOUT_SET: u32 = 2;

/// As palavras aceitas: nome, valor e máscara (máscara zero = bit que aceita o prefixo `no`).
const WORDS: [(&str, u32, u32); 7] = [
    ("always", ENABLED, ENABLE_MASK),
    ("never", DISABLED, ENABLE_MASK),
    ("auto", AUTO, ENABLE_MASK),
    ("plain", PLAIN, LAYOUT_MASK),
    ("column", COLUMN, LAYOUT_MASK),
    ("row", ROW, LAYOUT_MASK),
    ("dense", DENSE, 0),
];

/// Uma palavra (`parse_option`). `rest` é o texto dali até o fim, que é o que o C mostra no erro.
fn parse_word(word: &[u8], rest: &[u8], colopts: &mut u32, group_set: &mut u32) -> Result<(), ()> {
    for (name, value, mask) in WORDS {
        let (w, set) = match word.strip_prefix(b"no") {
            Some(w) if mask == 0 && !w.is_empty() => (w, false),
            _ => (word, true),
        };
        if w != name.as_bytes() {
            continue;
        }
        match mask {
            ENABLE_MASK => *group_set |= ENABLE_SET,
            LAYOUT_MASK => *group_set |= LAYOUT_SET,
            _ => {}
        }
        if mask != 0 {
            *colopts = (*colopts & !mask) | value;
        } else if set {
            *colopts |= value;
        } else {
            *colopts &= !value;
        }
        return Ok(());
    }
    error(&format!("unsupported option '{}'", os::lossy(rest)));
    Err(())
}

/// Uma lista de palavras separadas por espaço ou vírgula (`parse_config`). Escolher só o
/// layout liga a saída em colunas.
pub fn parse_config(colopts: &mut u32, value: &[u8]) -> Result<(), ()> {
    let is_sep = |c: &u8| *c == b' ' || *c == b',';
    let mut group_set = 0;
    let mut i = 0;
    while i < value.len() {
        let len = value[i..].iter().position(is_sep).unwrap_or(value.len() - i);
        if len > 0 {
            parse_word(&value[i..i + len], &value[i..], colopts, &mut group_set)?;
            i += len;
        }
        i += value[i..].iter().take_while(|c| is_sep(c)).count();
    }
    if group_set & LAYOUT_SET != 0 && group_set & ENABLE_SET == 0 {
        *colopts = (*colopts & !ENABLE_MASK) | ENABLED;
    }
    Ok(())
}

/// O callback de `--column[=<opções>]` e `--no-column`; `Err` vira exit 129 sem o uso.
pub fn parse_option(colopts: &mut u32, negated: bool, value: Option<&[u8]>) -> R<()> {
    *colopts |= PARSEOPT;
    *colopts &= !ENABLE_MASK;
    if negated {
        return Ok(());
    }
    *colopts |= ENABLED;
    match value {
        Some(v) if parse_config(colopts, v).is_err() => Err(Fail::Exit(129)),
        _ => Ok(()),
    }
}

/// `column.ui` e `column.<command>` na ordem em que aparecem na configuração
/// (`git_column_config`). Valor inválido morre como o leitor de configuração do git.
pub fn from_config(config: &Config, command: &str) -> R<u32> {
    let mut colopts = 0;
    for (file, e) in config.entries() {
        let Some(key) = e.key.strip_prefix("column.") else { continue };
        if key != "ui" && key != command {
            continue;
        }
        let ok = match &e.value {
            None => {
                error(&format!("missing value for '{}'", e.key));
                false
            }
            Some(v) => {
                let ok = parse_config(&mut colopts, v).is_ok();
                if !ok {
                    error(&format!("invalid column.{key} mode {}", os::lossy(v)));
                }
                ok
            }
        };
        if !ok {
            return Err(Fail::Fatal(if file.command_line {
                format!("unable to parse '{}' from command-line config", e.key)
            } else {
                let line = file.data[..e.span.0].iter().filter(|c| **c == b'\n').count() + 1;
                format!("bad config variable '{}' in file '{}' at line {line}", e.key, os::lossy(&file.path))
            }));
        }
    }
    Ok(colopts)
}

/// `finalize_colopts`: `auto` vira ligado só com o stdout num terminal.
pub fn finalize(colopts: &mut u32) {
    if *colopts & ENABLE_MASK == AUTO {
        *colopts &= !ENABLE_MASK;
        if os::sysc().isatty(sysabi::Fd::STDOUT) {
            *colopts |= ENABLED;
        }
    }
}

pub fn active(colopts: u32) -> bool {
    colopts & ENABLE_MASK == ENABLED
}

/// Ligado por `--column` (não só pela configuração).
pub fn explicitly_enabled(colopts: u32) -> bool {
    colopts & PARSEOPT != 0 && active(colopts)
}

/// Largura de exibição de um item, sem as sequências de cor.
fn item_width(s: &[u8]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < s.len() {
        if s[i] == 0x1b && s.get(i + 1) == Some(&b'[') {
            i += 2;
            while i < s.len() && (s[i].is_ascii_digit() || s[i] == b';') {
                i += 1;
            }
            i += 1;
            continue;
        }
        if s[i] & 0xc0 != 0x80 {
            n += 1;
        }
        i += 1;
    }
    n
}

/// Opções do layout (`struct column_options`); `width` zero = largura do terminal menos 1.
#[derive(Default)]
pub struct Options<'a> {
    pub width: usize,
    pub padding: usize,
    pub indent: &'a str,
}

/// `print_columns`: devolve o texto da lista no layout pedido.
pub fn print_columns(list: &[Vec<u8>], colopts: u32, o: &Options) -> Vec<u8> {
    print_columns_nl(list, colopts, o, b"\n")
}

/// `print_columns` com o fim de linha dado (`git column --nl`): vale no layout `plain` e no fim de
/// cada linha da tabela.
pub fn print_columns_nl(list: &[Vec<u8>], colopts: u32, o: &Options, nl: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if list.is_empty() {
        return out;
    }
    let width = if o.width > 0 { o.width } else { crate::diff::term_columns().saturating_sub(1) };
    let plain = |out: &mut Vec<u8>, indent: &str, nl: &[u8]| {
        for s in list {
            out.extend_from_slice(indent.as_bytes());
            out.extend_from_slice(s);
            out.extend_from_slice(nl);
        }
    };
    if !active(colopts) {
        plain(&mut out, "", &b"\n"[..]);
    } else if colopts & LAYOUT_MASK == PLAIN {
        plain(&mut out, o.indent, nl);
    } else {
        display_table(&mut out, list, colopts, width, o, nl);
    }
    out
}

/// O estado do `display_table` (`struct column_data`).
struct Table {
    by_column: bool,
    n: usize,
    rows: usize,
    cols: usize,
    len: Vec<usize>,
    /// Com `dense`: por coluna, o índice do item mais largo dela.
    width: Option<Vec<usize>>,
}

impl Table {
    fn linear(&self, x: usize, y: usize) -> usize {
        if self.by_column { x * self.rows + y } else { y * self.cols + x }
    }

    fn compute_column_width(&mut self) {
        let mut w = vec![0; self.cols];
        for (x, wx) in w.iter_mut().enumerate() {
            *wx = self.linear(x, 0);
            for y in 0..self.rows {
                let i = self.linear(x, y);
                if i < self.n && self.len[*wx] < self.len[i] {
                    *wx = i;
                }
            }
        }
        self.width = Some(w);
    }

    /// `shrink_columns`: tira linhas enquanto as colunas, cada uma com a sua largura, couberem.
    fn shrink(&mut self, width: usize, o: &Options) {
        while self.rows > 1 {
            let (rows, cols) = (self.rows, self.cols);
            self.rows -= 1;
            self.cols = self.n.div_ceil(self.rows);
            self.compute_column_width();
            let w = self.width.as_ref().map_or(&[][..], |w| w.as_slice());
            let total = o.indent.len() + w.iter().map(|&i| self.len[i] + o.padding).sum::<usize>();
            if total > width {
                self.rows = rows;
                self.cols = cols;
                break;
            }
        }
        self.compute_column_width();
    }
}

fn display_table(out: &mut Vec<u8>, list: &[Vec<u8>], colopts: u32, width: usize, o: &Options, nl: &[u8]) {
    let len: Vec<usize> = list.iter().map(|s| item_width(s)).collect();
    let initial_width = len.iter().copied().max().unwrap_or(0) + o.padding;
    let cols = (width.saturating_sub(o.indent.len()) / initial_width.max(1)).max(1);
    let mut t = Table { by_column: colopts & LAYOUT_MASK == COLUMN, n: list.len(), rows: list.len().div_ceil(cols), cols, len, width: None };
    if colopts & DENSE != 0 {
        t.shrink(width, o);
    }
    for y in 0..t.rows {
        for x in 0..t.cols {
            let i = t.linear(x, y);
            if i >= t.n {
                break;
            }
            // `display_cell`: com `dense`, coluna mais estreita que a inicial anda menos.
            let mut l = t.len[i];
            if let Some(w) = &t.width
                && t.len[w[x]] < initial_width
            {
                l += initial_width - t.len[w[x]];
                l = l.saturating_sub(o.padding);
            }
            let newline = if t.by_column { i + t.rows >= t.n } else { x == t.cols - 1 || i == t.n - 1 };
            if x == 0 {
                out.extend_from_slice(o.indent.as_bytes());
            }
            out.extend_from_slice(&list[i]);
            if newline {
                out.extend_from_slice(nl);
            } else {
                out.resize(out.len() + initial_width.saturating_sub(l), b' ');
            }
        }
    }
}
