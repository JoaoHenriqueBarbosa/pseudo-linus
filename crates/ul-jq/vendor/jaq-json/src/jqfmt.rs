//! Porte pseudo-linus: escritor de JSON igual ao `jv_dump_term` do jq 1.7.1 (`src/jv_print.c`), mais
//! `jv_dump_string_trunc` e `jv_kind_name`, que montam as mensagens de erro.
//!
//! Substitui o `write.rs` do jaq-json: indentação de 0 a 7 espaços ou tab, chaves ordenadas
//! (`jv_keys`), `--ascii-output`, cores ANSI (com `JQ_COLORS`), `<skipped: too deep>` depois de 256
//! níveis e números como o jq (literal preservado, `jvp_dtoa_fmt` para calculados).

use crate::Val;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

/// Profundidade máxima de impressão (`MAX_PRINT_DEPTH`).
pub const MAX_PRINT_DEPTH: usize = 256;

/// Cores padrão do jq 1.7.1, na ordem null, false, true, número, string, array, objeto, chave.
pub const DEFAULT_COLORS: [&str; 8] =
    ["\x1b[0;90m", "\x1b[0;39m", "\x1b[0;39m", "\x1b[0;39m", "\x1b[0;32m", "\x1b[1;39m", "\x1b[1;39m", "\x1b[1;34m"];

const COLRESET: &str = "\x1b[0m";

/// Opções de impressão (os `JV_PRINT_*` do jq).
#[derive(Clone, Debug, Default)]
pub struct DumpOpts {
    /// `JV_PRINT_PRETTY`: quebra de linha e indentação.
    pub pretty: bool,
    /// `JV_PRINT_TAB`: indenta com tab em vez de espaços.
    pub tab: bool,
    /// Espaços por nível (`JV_PRINT_SPACE*`), de 0 a 7.
    pub indent: usize,
    /// `JV_PRINT_SORTED`.
    pub sort_keys: bool,
    /// `JV_PRINT_ASCII`.
    pub ascii: bool,
    /// `JV_PRINT_COLOR`, com a tabela de cores em uso.
    pub colors: Option<[String; 8]>,
}

impl DumpOpts {
    /// Compacto, sem opções (o `jv_dump_string(x, 0)` das mensagens e do `tojson`).
    pub const fn compact() -> DumpOpts {
        DumpOpts { pretty: false, tab: false, indent: 0, sort_keys: false, ascii: false, colors: None }
    }
}

/// `jq_set_colors`: a variável `JQ_COLORS` troca as cores na ordem da tabela. `None` se o texto for
/// inválido (o jq avisa "Failed to set $JQ_COLORS" e segue com as cores padrão).
pub fn parse_colors(spec: &str) -> Option<[String; 8]> {
    let mut colors: [String; 8] = DEFAULT_COLORS.map(String::from);
    let mut rest = spec;
    let mut i = 0;
    while i < 8 && !rest.is_empty() {
        let (part, next) = match rest.find(':') {
            Some(p) => (&rest[..p], &rest[p + 1..]),
            None => (rest, ""),
        };
        if part.len() > 16 - 4 {
            return None;
        }
        if !part.bytes().all(|c| c.is_ascii_digit() || c == b';') {
            return None;
        }
        colors[i] = alloc::format!("\x1b[{part}m");
        rest = next;
        i += 1;
    }
    Some(colors)
}

/// Nome do tipo como o `jv_kind_name`.
pub fn kind_name(v: &Val) -> &'static str {
    match v {
        Val::Null => "null",
        Val::Bool(_) => "boolean",
        Val::Num(_) => "number",
        Val::TStr(_) | Val::BStr(_) => "string",
        Val::Arr(_) => "array",
        Val::Obj(_) => "object",
    }
}

fn color_index(v: &Val) -> usize {
    match v {
        Val::Null => 0,
        Val::Bool(false) => 1,
        Val::Bool(true) => 2,
        Val::Num(_) => 3,
        Val::TStr(_) | Val::BStr(_) => 4,
        Val::Arr(_) => 5,
        Val::Obj(_) => 6,
    }
}

/// Serializa `v` como o jq faria.
pub fn dump(v: &Val, opts: &DumpOpts) -> Vec<u8> {
    let mut out = Vec::new();
    dump_into(&mut out, v, opts, 0);
    out
}

/// Serializa compacto, em `String` (o conteúdo é sempre UTF-8 válido).
pub fn dump_compact(v: &Val) -> String {
    String::from_utf8(dump(v, &DumpOpts::compact())).unwrap_or_default()
}

/// `jv_dump_string_trunc`: os primeiros `bufsize - 1` bytes; se não couber, os últimos três viram
/// "...". O corte é por byte, e um caractere UTF-8 cortado vira U+FFFD (como o `jv_string_fmt`).
pub fn dump_trunc(v: &Val, bufsize: usize) -> String {
    let s = dump(v, &DumpOpts::compact());
    // `strlen`: o texto para no primeiro NUL (não acontece em JSON, que escapa NUL).
    let len = s.iter().position(|b| *b == 0).unwrap_or(s.len());
    let mut out: Vec<u8> = s[..len.min(bufsize - 1)].to_vec();
    if len > bufsize - 1 && bufsize >= 4 {
        let n = out.len();
        out[n - 3..].copy_from_slice(b"...");
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn put_indent(out: &mut Vec<u8>, n: usize, opts: &DumpOpts) {
    if opts.tab {
        out.extend(core::iter::repeat_n(b'\t', n));
    } else {
        out.extend(core::iter::repeat_n(b' ', n * opts.indent));
    }
}

/// `jvp_dump_string`.
pub fn dump_string(out: &mut Vec<u8>, bytes: &[u8], ascii: bool) {
    out.push(b'"');
    let s = String::from_utf8_lossy(bytes);
    let mut buf = String::new();
    for c in s.chars() {
        let code = c as u32;
        let mut unicode_escape = false;
        if (0x20..=0x7e).contains(&code) {
            if c == '"' || c == '\\' {
                out.push(b'\\');
            }
            out.push(code as u8);
        } else if code < 0x20 || code == 0x7f {
            match c {
                '\u{8}' => out.extend_from_slice(b"\\b"),
                '\t' => out.extend_from_slice(b"\\t"),
                '\r' => out.extend_from_slice(b"\\r"),
                '\n' => out.extend_from_slice(b"\\n"),
                '\u{c}' => out.extend_from_slice(b"\\f"),
                _ => unicode_escape = true,
            }
        } else if ascii {
            unicode_escape = true;
        } else {
            let mut tmp = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
        }
        if unicode_escape {
            buf.clear();
            if code <= 0xffff {
                let _ = write!(buf, "\\u{code:04x}");
            } else {
                let c2 = code - 0x10000;
                let _ = write!(buf, "\\u{:04x}\\u{:04x}", 0xD800 | ((c2 & 0xffc00) >> 10), 0xDC00 | (c2 & 0x3ff));
            }
            out.extend_from_slice(buf.as_bytes());
        }
    }
    out.push(b'"');
}

/// Bytes de uma chave (no jq, chave é sempre string).
pub fn key_bytes(k: &Val) -> &[u8] {
    match k {
        Val::TStr(b) | Val::BStr(b) => b,
        _ => &[],
    }
}

fn dump_into(out: &mut Vec<u8>, v: &Val, opts: &DumpOpts, indent: usize) {
    let color: Option<&str> = opts.colors.as_ref().map(|c| c[color_index(v)].as_str());
    if let Some(c) = color {
        out.extend_from_slice(c.as_bytes());
    }
    if indent > MAX_PRINT_DEPTH {
        out.extend_from_slice(b"<skipped: too deep>");
    } else {
        match v {
            Val::Null => out.extend_from_slice(b"null"),
            Val::Bool(true) => out.extend_from_slice(b"true"),
            Val::Bool(false) => out.extend_from_slice(b"false"),
            Val::Num(n) => {
                if n.is_nan() {
                    // `jv_dump_term(C, jv_null(), ...)`: com cor, o null ganha a cor dele também.
                    if let Some(colors) = &opts.colors {
                        out.extend_from_slice(colors[0].as_bytes());
                    }
                    out.extend_from_slice(b"null");
                    if opts.colors.is_some() {
                        out.extend_from_slice(COLRESET.as_bytes());
                    }
                } else {
                    out.extend_from_slice(n.dump().as_bytes());
                }
            }
            Val::TStr(b) | Val::BStr(b) => dump_string(out, b, opts.ascii),
            Val::Arr(a) => {
                if a.is_empty() {
                    out.extend_from_slice(b"[]");
                } else {
                    out.push(b'[');
                    if opts.pretty {
                        out.push(b'\n');
                        put_indent(out, indent + 1, opts);
                    }
                    for (i, x) in a.iter().enumerate() {
                        if i != 0 {
                            if opts.pretty {
                                out.extend_from_slice(b",\n");
                                put_indent(out, indent + 1, opts);
                            } else {
                                out.push(b',');
                            }
                        }
                        dump_into(out, x, opts, indent + 1);
                        if let Some(c) = color {
                            out.extend_from_slice(c.as_bytes());
                        }
                    }
                    if opts.pretty {
                        out.push(b'\n');
                        put_indent(out, indent, opts);
                    }
                    if let Some(c) = color {
                        out.extend_from_slice(c.as_bytes());
                    }
                    out.push(b']');
                }
            }
            Val::Obj(o) => {
                if o.is_empty() {
                    out.extend_from_slice(b"{}");
                } else {
                    out.push(b'{');
                    if opts.pretty {
                        out.push(b'\n');
                        put_indent(out, indent + 1, opts);
                    }
                    let mut entries: Vec<(&Val, &Val)> = o.iter().collect();
                    if opts.sort_keys {
                        entries.sort_by(|a, b| key_bytes(a.0).cmp(key_bytes(b.0)));
                    }
                    let field = opts.colors.as_ref().map(|c| c[7].as_str());
                    for (i, (k, x)) in entries.into_iter().enumerate() {
                        if i != 0 {
                            if opts.pretty {
                                out.extend_from_slice(b",\n");
                                put_indent(out, indent + 1, opts);
                            } else {
                                out.push(b',');
                            }
                        }
                        if color.is_some() {
                            out.extend_from_slice(COLRESET.as_bytes());
                        }
                        if let Some(f) = field {
                            out.extend_from_slice(f.as_bytes());
                        }
                        dump_string(out, key_bytes(k), opts.ascii);
                        if color.is_some() {
                            out.extend_from_slice(COLRESET.as_bytes());
                        }
                        if let Some(c) = color {
                            out.extend_from_slice(c.as_bytes());
                        }
                        out.extend_from_slice(if opts.pretty { b": " } else { b":" });
                        if color.is_some() {
                            out.extend_from_slice(COLRESET.as_bytes());
                        }
                        dump_into(out, x, opts, indent + 1);
                        if let Some(c) = color {
                            out.extend_from_slice(c.as_bytes());
                        }
                    }
                    if opts.pretty {
                        out.push(b'\n');
                        put_indent(out, indent, opts);
                    }
                    if let Some(c) = color {
                        out.extend_from_slice(c.as_bytes());
                    }
                    out.push(b'}');
                }
            }
        }
    }
    if color.is_some() {
        out.extend_from_slice(COLRESET.as_bytes());
    }
}
