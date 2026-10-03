// Porte pseudo-linus: substituto do `onig` (Oniguruma, biblioteca C ligada por FFI) usado pelo
// `-name`/`-path` (glob convertido em BRE) e pelo `-regex`. Traduz as sintaxes POSIX básica,
// estendida e Emacs pra sintaxe da crate `regex` e casa a string inteira, como o `is_match` do onig.
//
// Limites conhecidos (a crate `regex` não tem): retrovisor (`\1`), e a semântica de casamento é
// leftmost-first em vez de leftmost-longest (irrelevante aqui, porque o casamento é ancorado nos
// dois lados). Um padrão com retrovisor dá erro de compilação.

use regex::Regex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    /// POSIX básica (BRE) com as extensões GNU `\+`, `\?` e `\|`.
    Basic,
    /// POSIX estendida (ERE).
    Extended,
    /// A sintaxe padrão do `find -regex` do GNU.
    Emacs,
}

#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// Compila `pattern` pra casar a string inteira.
pub fn compile(pattern: &str, syntax: Syntax, ignore_case: bool) -> Result<Regex, Error> {
    let body = translate(pattern, syntax)?;
    let flags = if ignore_case { "(?si)" } else { "(?s)" };
    Regex::new(&format!("{flags}^(?:{body})$")).map_err(|e| Error(e.to_string()))
}

fn push_literal(out: &mut String, c: char) {
    if regex_syntax_meta(c) {
        out.push('\\');
    }
    out.push(c);
}

fn regex_syntax_meta(c: char) -> bool {
    matches!(
        c,
        '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' | '#' | '&' | '-' | '~'
    )
}

fn translate(pattern: &str, syntax: Syntax) -> Result<String, Error> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    // Em BRE, `*` no começo (ou logo depois de `\(` ou `^`) é literal.
    let mut at_start = true;
    while i < chars.len() {
        let c = chars[i];
        let was_start = at_start;
        at_start = false;
        match c {
            '[' => {
                i = bracket(&chars, i, &mut out)?;
                continue;
            }
            '\\' => {
                let Some(&n) = chars.get(i + 1) else {
                    return Err(Error("trailing backslash".into()));
                };
                i += 2;
                match (syntax, n) {
                    (Syntax::Basic | Syntax::Emacs, '(') => {
                        out.push('(');
                        at_start = true;
                    }
                    (Syntax::Basic | Syntax::Emacs, ')') => out.push(')'),
                    (Syntax::Basic | Syntax::Emacs, '|') => {
                        out.push('|');
                        at_start = true;
                    }
                    (Syntax::Basic, '{') => out.push('{'),
                    (Syntax::Basic, '}') => out.push('}'),
                    (Syntax::Basic, '+' | '?') => out.push(n),
                    (_, '1'..='9') => return Err(Error("back-references are not supported".into())),
                    (Syntax::Emacs | Syntax::Extended, 'w' | 'W' | 'b' | 'B') => {
                        out.push('\\');
                        out.push(n);
                    }
                    (Syntax::Emacs, '<' | '>') => out.push_str("\\b"),
                    (Syntax::Emacs, '`') => out.push_str("\\A"),
                    (Syntax::Emacs, '\'') => out.push_str("\\z"),
                    (_, 'n') => out.push_str("\\n"),
                    (_, 't') => out.push_str("\\t"),
                    _ => push_literal(&mut out, n),
                }
                continue;
            }
            '*' if syntax == Syntax::Basic && was_start => push_literal(&mut out, '*'),
            '.' | '*' => out.push(c),
            '^' => {
                out.push('^');
                at_start = true;
            }
            '$' => out.push('$'),
            '+' | '?' => match syntax {
                Syntax::Basic => push_literal(&mut out, c),
                _ => out.push(c),
            },
            '(' | ')' | '|' => match syntax {
                Syntax::Extended => {
                    out.push(c);
                    at_start = c != ')';
                }
                _ => push_literal(&mut out, c),
            },
            '{' | '}' => match syntax {
                Syntax::Extended => out.push(c),
                _ => push_literal(&mut out, c),
            },
            _ => push_literal(&mut out, c),
        }
        i += 1;
    }
    Ok(out)
}

/// Traduz uma expressão entre colchetes POSIX que começa em `chars[start] == '['`. Devolve o índice
/// logo depois do `]` final. Sem `]` final, o `[` é literal (como no glob e no GNU).
fn bracket(chars: &[char], start: usize, out: &mut String) -> Result<usize, Error> {
    let mut i = start + 1;
    let mut class = String::from("[");
    if chars.get(i) == Some(&'^') {
        class.push('^');
        i += 1;
    }
    let first = i;
    loop {
        let Some(&c) = chars.get(i) else {
            push_literal(out, '[');
            return Ok(start + 1);
        };
        if c == ']' && i != first {
            class.push(']');
            out.push_str(&class);
            return Ok(i + 1);
        }
        if c == '[' && matches!(chars.get(i + 1), Some(':' | '=' | '.')) {
            let delim = chars[i + 1];
            let mut j = i + 2;
            while j + 1 < chars.len() && !(chars[j] == delim && chars[j + 1] == ']') {
                j += 1;
            }
            if j + 1 >= chars.len() {
                return Err(Error("unterminated bracket".into()));
            }
            let name: String = chars[i + 2..j].iter().collect();
            match delim {
                ':' => {
                    class.push_str("[:");
                    class.push_str(&name);
                    class.push_str(":]");
                }
                _ => {
                    for ch in name.chars() {
                        push_literal(&mut class, ch);
                    }
                }
            }
            i = j + 2;
            continue;
        }
        let is_range_dash = c == '-'
            && i != first
            && chars.get(i + 1).is_some_and(|n| *n != ']');
        if is_range_dash {
            class.push('-');
        } else {
            push_literal(&mut class, c);
        }
        i += 1;
    }
}

// Os testes deste módulo ficam na bancada (`src/main.rs` do experimento), pelo `find -regex` e pelo
// `find -name`: os testes do findutils dependem de tempfile e do FS do host.
