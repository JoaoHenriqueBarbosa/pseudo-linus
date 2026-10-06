//! Módulo `textwrap` do CPython 3.13, com o mesmo algoritmo do `textwrap.py`: `dedent`, `indent`,
//! `wrap`, `fill` e `shorten`, com todas as opções do `TextWrapper` aceitas como nomeados.
//!
//! O separador de palavras (`wordsep_re`) é reimplementado à mão, sem regex. Ficam de fora: a classe
//! `TextWrapper` (precisa de classes de usuário). Comprimentos contam pontos de código, não largura
//! de tela, como no CPython.

use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::native_util::{bind, no_kwargs, want_int, want_str};
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{exc, PyResult, Vm};

#[derive(Clone, Debug)]
pub struct Opts {
    pub width: i64,
    pub initial_indent: String,
    pub subsequent_indent: String,
    pub expand_tabs: bool,
    pub replace_whitespace: bool,
    pub fix_sentence_endings: bool,
    pub break_long_words: bool,
    pub drop_whitespace: bool,
    pub break_on_hyphens: bool,
    pub tabsize: i64,
    pub max_lines: Option<i64>,
    pub placeholder: String,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            width: 70,
            initial_indent: String::new(),
            subsequent_indent: String::new(),
            expand_tabs: true,
            replace_whitespace: true,
            fix_sentence_endings: false,
            break_long_words: true,
            drop_whitespace: true,
            break_on_hyphens: true,
            tabsize: 8,
            max_lines: None,
            placeholder: " [...]".to_string(),
        }
    }
}

fn clen(s: &str) -> usize {
    s.chars().count()
}

fn is_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ')
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_letter(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_word_punct(c: char) -> bool {
    is_word(c) || matches!(c, '!' | '"' | '\'' | '&' | '.' | ',' | '?')
}

/// `str.expandtabs(tabsize)`.
pub fn expandtabs(s: &str, tabsize: i64) -> String {
    let mut out = String::with_capacity(s.len());
    let mut col: i64 = 0;
    for c in s.chars() {
        match c {
            '\t' => {
                if tabsize > 0 {
                    let n = tabsize - col % tabsize;
                    for _ in 0..n {
                        out.push(' ');
                    }
                    col += n;
                }
            }
            '\n' | '\r' => {
                out.push(c);
                col = 0;
            }
            _ => {
                out.push(c);
                col += 1;
            }
        }
    }
    out
}

fn munge_whitespace(text: &str, o: &Opts) -> String {
    let mut t = if o.expand_tabs { expandtabs(text, o.tabsize) } else { text.to_string() };
    if o.replace_whitespace {
        t = t.chars().map(|c| if is_ws(c) { ' ' } else { c }).collect();
    }
    t
}

/// `dashes_then_word(chars, at)`: em-traço (dois ou mais `-`) seguido de caractere de palavra.
fn dash_run_then_word(chars: &[char], at: usize) -> Option<usize> {
    let mut j = at;
    while j < chars.len() && chars[j] == '-' {
        j += 1;
    }
    if j - at >= 2 && j < chars.len() && is_word(chars[j]) {
        Some(j)
    } else {
        None
    }
}

/// Divide o texto em pedaços (palavras, espaços, trechos hifenizados), como `TextWrapper._split`.
fn split_chunks(text: &str, break_on_hyphens: bool) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut chunks: Vec<String> = Vec::new();
    if !break_on_hyphens {
        let mut i = 0;
        while i < n {
            let ws = is_ws(chars[i]);
            let mut j = i;
            while j < n && is_ws(chars[j]) == ws {
                j += 1;
            }
            chunks.push(chars[i..j].iter().collect());
            i = j;
        }
        return chunks;
    }
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if is_ws(c) {
            let mut j = i;
            while j < n && is_ws(chars[j]) {
                j += 1;
            }
            chunks.push(chars[i..j].iter().collect());
            i = j;
            continue;
        }
        if c == '-' && i > 0 && is_word_punct(chars[i - 1]) {
            if let Some(j) = dash_run_then_word(&chars, i) {
                chunks.push(chars[i..j].iter().collect());
                i = j;
                continue;
            }
        }
        // Palavra: o menor trecho de não-espaços que termine num dos três marcadores.
        let mut e = i + 1;
        loop {
            if e < n && chars[e] == '-' {
                let behind = (e >= 2 && is_letter(chars[e - 2]) && is_letter(chars[e - 1]))
                    || (e >= 3 && is_letter(chars[e - 3]) && chars[e - 2] == '-' && is_letter(chars[e - 1]));
                let ahead = e + 1 < n
                    && is_letter(chars[e + 1])
                    && ((e + 3 < n && chars[e + 2] == '-' && is_letter(chars[e + 3]))
                        || (e + 2 < n && is_letter(chars[e + 2])));
                if behind && ahead {
                    chunks.push(chars[i..=e].iter().collect());
                    i = e + 1;
                    break;
                }
            }
            if e >= n || is_ws(chars[e]) {
                chunks.push(chars[i..e].iter().collect());
                i = e;
                break;
            }
            if is_word_punct(chars[e - 1]) && chars[e] == '-' && dash_run_then_word(&chars, e).is_some() {
                chunks.push(chars[i..e].iter().collect());
                i = e;
                break;
            }
            e += 1;
        }
    }
    chunks
}

fn sentence_end(chunk: &str) -> bool {
    let cs: Vec<char> = chunk.chars().collect();
    let mut n = cs.len();
    if n > 0 && (cs[n - 1] == '"' || cs[n - 1] == '\'') {
        n -= 1;
    }
    n >= 2 && matches!(cs[n - 1], '.' | '!' | '?') && cs[n - 2].is_ascii_lowercase()
}

fn fix_sentence_endings(chunks: &mut [String]) {
    let mut i = 0;
    while i + 1 < chunks.len() {
        if chunks[i + 1] == " " && sentence_end(&chunks[i]) {
            chunks[i + 1] = "  ".to_string();
            i += 2;
        } else {
            i += 1;
        }
    }
}

fn handle_long_word(chunks: &mut [String], cur_line: &mut Vec<String>, cur_len: i64, width: i64, o: &Opts) -> bool {
    let space_left = if width < 1 { 1 } else { width - cur_len };
    if o.break_long_words {
        let Some(last) = chunks.last_mut() else { return false };
        let cc: Vec<char> = last.chars().collect();
        let space = space_left.max(0) as usize;
        let mut end = space;
        if o.break_on_hyphens && cc.len() > space {
            if let Some(h) = cc[..space].iter().rposition(|&c| c == '-') {
                if h > 0 && cc[..h].iter().any(|&c| c != '-') {
                    end = h + 1;
                }
            }
        }
        let end = end.min(cc.len());
        cur_line.push(cc[..end].iter().collect());
        *last = cc[end..].iter().collect();
        false
    } else {
        // Sem quebrar palavras longas: só entra sozinha numa linha vazia (o chamador faz o pop).
        cur_line.is_empty()
    }
}

/// `TextWrapper._wrap_chunks`. O erro é a mensagem do `ValueError`.
fn wrap_chunks(mut chunks: Vec<String>, o: &Opts) -> Result<Vec<String>, String> {
    if o.width <= 0 {
        return Err(format!("invalid width {} (must be > 0)", o.width));
    }
    if let Some(ml) = o.max_lines {
        let indent = if ml > 1 { &o.subsequent_indent } else { &o.initial_indent };
        if (clen(indent) + clen(o.placeholder.trim_start())) as i64 > o.width {
            return Err("placeholder too large for max width".to_string());
        }
    }
    chunks.reverse();
    let mut lines: Vec<String> = Vec::new();
    while !chunks.is_empty() {
        let mut cur_line: Vec<String> = Vec::new();
        let mut cur_len: i64 = 0;
        let indent: &str = if lines.is_empty() { &o.initial_indent } else { &o.subsequent_indent };
        let width = o.width - clen(indent) as i64;
        if o.drop_whitespace && chunks.last().is_some_and(|c| c.trim().is_empty()) && !lines.is_empty() {
            chunks.pop();
        }
        loop {
            let l = match chunks.last() {
                Some(c) => clen(c) as i64,
                None => break,
            };
            if cur_len + l <= width {
                cur_line.push(chunks.pop().unwrap());
                cur_len += l;
            } else {
                break;
            }
        }
        let long = chunks.last().is_some_and(|c| clen(c) as i64 > width);
        if long {
            let take_whole = handle_long_word(&mut chunks, &mut cur_line, cur_len, width, o);
            if take_whole {
                cur_line.push(chunks.pop().unwrap());
            }
            cur_len = cur_line.iter().map(|s| clen(s) as i64).sum();
        }
        let drop_last = o.drop_whitespace && cur_line.last().is_some_and(|l| l.trim().is_empty());
        if drop_last {
            let l = cur_line.pop().unwrap();
            cur_len -= clen(&l) as i64;
        }
        if !cur_line.is_empty() {
            let fits = match o.max_lines {
                None => true,
                Some(ml) => {
                    (lines.len() as i64 + 1 < ml)
                        || ((chunks.is_empty()
                            || (o.drop_whitespace && chunks.len() == 1 && chunks[0].trim().is_empty()))
                            && cur_len <= width)
                }
            };
            if fits {
                lines.push(format!("{}{}", indent, cur_line.concat()));
            } else {
                loop {
                    let info = cur_line.last().map(|l| (!l.trim().is_empty(), clen(l) as i64));
                    match info {
                        None => {
                            if let Some(prev) = lines.last() {
                                let prev_line = prev.trim_end().to_string();
                                if clen(&prev_line) + clen(&o.placeholder) <= o.width as usize {
                                    let at = lines.len() - 1;
                                    lines[at] = format!("{}{}", prev_line, o.placeholder);
                                    break;
                                }
                            }
                            lines.push(format!("{}{}", indent, o.placeholder.trim_start()));
                            break;
                        }
                        Some((non_blank, len)) => {
                            if non_blank && cur_len + clen(&o.placeholder) as i64 <= width {
                                cur_line.push(o.placeholder.clone());
                                lines.push(format!("{}{}", indent, cur_line.concat()));
                                break;
                            }
                            cur_len -= len;
                            cur_line.pop();
                        }
                    }
                }
                break;
            }
        }
    }
    Ok(lines)
}

/// `textwrap.wrap(text, **opts)`.
pub fn wrap_text(text: &str, o: &Opts) -> Result<Vec<String>, String> {
    let t = munge_whitespace(text, o);
    let mut chunks = split_chunks(&t, o.break_on_hyphens);
    if o.fix_sentence_endings {
        fix_sentence_endings(&mut chunks);
    }
    wrap_chunks(chunks, o)
}

/// `textwrap.fill(text, **opts)`.
pub fn fill_text(text: &str, o: &Opts) -> Result<String, String> {
    Ok(wrap_text(text, o)?.join("\n"))
}

/// `textwrap.shorten(text, width, **opts)`.
pub fn shorten_text(text: &str, width: i64, o: &Opts) -> Result<String, String> {
    let mut o = o.clone();
    o.width = width;
    o.max_lines = Some(1);
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    fill_text(&collapsed, &o)
}

/// `textwrap.dedent(text)`.
pub fn dedent_text(text: &str) -> String {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| if !l.is_empty() && l.chars().all(|c| c == ' ' || c == '\t') { "" } else { l })
        .collect();
    let mut margin: Option<String> = None;
    for l in &lines {
        let Some(pos) = l.find(|c: char| c != ' ' && c != '\t') else { continue };
        let indent = &l[..pos];
        match &margin {
            None => margin = Some(indent.to_string()),
            Some(m) => {
                if indent.starts_with(m.as_str()) {
                    // mantém a margem
                } else if m.starts_with(indent) {
                    margin = Some(indent.to_string());
                } else {
                    let common: String = m.chars().zip(indent.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a).collect();
                    margin = Some(common);
                }
            }
        }
    }
    let margin = margin.unwrap_or_default();
    lines
        .iter()
        .map(|l| if !margin.is_empty() && l.starts_with(margin.as_str()) { &l[margin.len()..] } else { *l })
        .collect::<Vec<&str>>()
        .join("\n")
}

/// Linhas com o terminador (`str.splitlines(True)`).
fn splitlines_keepends(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        cur.push(c);
        match c {
            '\r' => {
                if it.peek() == Some(&'\n') {
                    cur.push('\n');
                    it.next();
                }
                out.push(std::mem::take(&mut cur));
            }
            '\n' | '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                out.push(std::mem::take(&mut cur));
            }
            _ => {}
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// ---------------------------------------------------------------------------
// Funções Python
// ---------------------------------------------------------------------------

const OPT_NAMES: [&str; 13] = [
    "text",
    "width",
    "initial_indent",
    "subsequent_indent",
    "expand_tabs",
    "replace_whitespace",
    "fix_sentence_endings",
    "break_long_words",
    "drop_whitespace",
    "break_on_hyphens",
    "tabsize",
    "max_lines",
    "placeholder",
];

fn parse_opts(fname: &str, slots: &[Option<Value>]) -> PyResult<Opts> {
    let mut o = Opts::default();
    if let Some(v) = &slots[1] {
        o.width = want_int(v)?;
    }
    if let Some(v) = &slots[2] {
        o.initial_indent = want_str(fname, v)?.to_string();
    }
    if let Some(v) = &slots[3] {
        o.subsequent_indent = want_str(fname, v)?.to_string();
    }
    if let Some(v) = &slots[4] {
        o.expand_tabs = v.is_true();
    }
    if let Some(v) = &slots[5] {
        o.replace_whitespace = v.is_true();
    }
    if let Some(v) = &slots[6] {
        o.fix_sentence_endings = v.is_true();
    }
    if let Some(v) = &slots[7] {
        o.break_long_words = v.is_true();
    }
    if let Some(v) = &slots[8] {
        o.drop_whitespace = v.is_true();
    }
    if let Some(v) = &slots[9] {
        o.break_on_hyphens = v.is_true();
    }
    if let Some(v) = &slots[10] {
        o.tabsize = want_int(v)?;
    }
    match &slots[11] {
        None | Some(Value::None) => {}
        Some(v) => o.max_lines = Some(want_int(v)?),
    }
    if let Some(v) = &slots[12] {
        o.placeholder = want_str(fname, v)?.to_string();
    }
    Ok(o)
}

fn value_err(msg: String) -> crate::vm::PyException {
    exc("ValueError", msg)
}

fn wrap(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("wrap", args, kw, &OPT_NAMES, 1)?;
    let text = want_str("wrap", s[0].as_ref().unwrap())?;
    let o = parse_opts("wrap", &s)?;
    let lines = wrap_text(text, &o).map_err(value_err)?;
    Ok(Value::list(lines.into_iter().map(Value::str).collect()))
}

fn fill(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("fill", args, kw, &OPT_NAMES, 1)?;
    let text = want_str("fill", s[0].as_ref().unwrap())?;
    let o = parse_opts("fill", &s)?;
    fill_text(text, &o).map(Value::str).map_err(value_err)
}

fn shorten(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("shorten", args, kw, &OPT_NAMES, 2)?;
    let text = want_str("shorten", s[0].as_ref().unwrap())?;
    let o = parse_opts("shorten", &s)?;
    shorten_text(text, o.width, &o).map(Value::str).map_err(value_err)
}

fn dedent(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("dedent", &kw)?;
    crate::native_util::exactly("dedent", &args, 1)?;
    let text = want_str("dedent", &args[0])?;
    Ok(Value::str(dedent_text(text)))
}

fn indent(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("indent", args, kw, &["text", "prefix", "predicate"], 2)?;
    let text = want_str("indent", s[0].as_ref().unwrap())?;
    let prefix = want_str("indent", s[1].as_ref().unwrap())?;
    let predicate = match &s[2] {
        None | Some(Value::None) => None,
        Some(f) => Some(f.clone()),
    };
    let mut out = String::new();
    for line in splitlines_keepends(text) {
        let wanted = match &predicate {
            None => !line.trim().is_empty(),
            Some(f) => vm.call_value(f, vec![Value::str(line.clone())], Vec::new())?.is_true(),
        };
        if wanted {
            out.push_str(prefix);
        }
        out.push_str(&line);
    }
    Ok(Value::str(out))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("textwrap")
        .func("wrap", wrap)
        .func("fill", fill)
        .func("shorten", shorten)
        .func("dedent", dedent)
        .func("indent", indent)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{repr, to_str};

    fn w(text: &str, width: i64) -> Vec<String> {
        let o = Opts { width, ..Opts::default() };
        wrap_text(text, &o).unwrap()
    }

    #[test]
    fn dedent_cases() {
        assert_eq!(dedent_text("  hello\n    world\n"), "hello\n  world\n");
        assert_eq!(dedent_text("\n  a\n  b\n"), "\na\nb\n");
        // Margem comum vazia: o texto não muda.
        assert_eq!(dedent_text("  a\n\t b\n"), "  a\n\t b\n");
        // Linhas só de espaço viram vazias e não entram na margem.
        assert_eq!(dedent_text("  a\n   \n  b"), "a\n\nb");
    }

    #[test]
    fn indent_through_module_function() {
        let mut vm = Vm::new();
        let r = indent(&mut vm, vec![Value::str("a\n\nb\n"), Value::str("> ")], Vec::new()).unwrap();
        assert_eq!(to_str(&r), "> a\n\n> b\n");
    }

    #[test]
    fn wrap_basic() {
        assert_eq!(w("The quick brown fox jumps over the lazy dog", 15), vec!["The quick brown", "fox jumps over", "the lazy dog"]);
        assert_eq!(w("hello", 3), vec!["hel", "lo"]);
        assert_eq!(w("well-known thing", 6), vec!["well-", "known", "thing"]);
        assert_eq!(w("", 10), Vec::<String>::new());
    }

    #[test]
    fn wrap_indents_and_options() {
        let o = Opts {
            width: 5,
            initial_indent: "* ".to_string(),
            subsequent_indent: "  ".to_string(),
            ..Opts::default()
        };
        assert_eq!(wrap_text("aaa bbb", &o).unwrap(), vec!["* aaa", "  bbb"]);
        let o = Opts { width: 3, ..Opts::default() };
        assert_eq!(fill_text("a b c", &o).unwrap(), "a b\nc");
        let o = Opts { width: 3, break_long_words: false, ..Opts::default() };
        assert_eq!(wrap_text("abcdef gh", &o).unwrap(), vec!["abcdef", "gh"]);
    }

    #[test]
    fn shorten_doc_examples() {
        let o = Opts::default();
        assert_eq!(shorten_text("Hello  world!", 12, &o).unwrap(), "Hello world!");
        assert_eq!(shorten_text("Hello  world!", 11, &o).unwrap(), "Hello [...]");
    }

    #[test]
    fn wrap_through_module_function() {
        let mut vm = Vm::new();
        let kw = vec![("width".to_string(), Value::Int(15))];
        let r = wrap(&mut vm, vec![Value::str("The quick brown fox jumps over the lazy dog")], kw).unwrap();
        assert_eq!(repr(&r), "['The quick brown', 'fox jumps over', 'the lazy dog']");
        let e = wrap(&mut vm, vec![Value::str("x"), Value::Int(0)], Vec::new()).unwrap_err();
        assert_eq!((e.kind, e.msg.as_str()), ("ValueError", "invalid width 0 (must be > 0)"));
    }

    #[test]
    fn expandtabs_cases() {
        assert_eq!(expandtabs("a\tb", 8), "a       b");
        assert_eq!(expandtabs("ab\tc\nd\te", 4), "ab  c\nd   e");
    }
}
