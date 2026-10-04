//! Mensagens de erro de compilação do jq 1.7.1 a partir dos erros do parser e do compilador do
//! jaq: o texto do bison ("syntax error, unexpected X, expecting Y (Unix shell quoting issues?)"),
//! os de símbolo indefinido ("foo/1 is not defined") e a localização do `locfile_locate`
//! ("at <top-level>, line N:" seguido da linha e de espaços até a coluna).

use jaq_core::compile::Undefined;
use jaq_core::load::{self, lex, parse};
use jaq_json::Num;

const QUOTING: &str = " (Unix shell quoting issues?)";

/// Posição de `part` dentro de `whole`, se for uma fatia dele.
fn offset(whole: &str, part: &str) -> Option<usize> {
    let w = whole.as_ptr() as usize;
    let p = part.as_ptr() as usize;
    (p >= w && p <= w + whole.len()).then(|| p - w)
}

/// `locfile_locate`: "<msg> at <top-level>, line N:\n<linha><espaços até a coluna>".
pub fn locate_at(program: &str, start: usize, msg: &str) -> String {
    let start = start.min(program.len());
    let line_start = program[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_no = program[..start].matches('\n').count() + 1;
    let line_end = program[start..].find('\n').map(|i| start + i + 1).unwrap_or(program.len());
    let text = &program[line_start..line_end];
    format!("{msg} at <top-level>, line {line_no}:\n{text}{}", " ".repeat(start - line_start))
}

fn locate(program: &str, part: &str, msg: &str) -> String {
    locate_at(program, offset(program, part).unwrap_or(0), msg)
}

/// Começo do último token do programa (onde o bison aponta um erro no fim do arquivo).
fn last_token_start(program: &str) -> usize {
    let b = program.as_bytes();
    let mut end = b.len();
    while end > 0 && matches!(b[end - 1], b' ' | b'\t' | b'\r' | b'\n') {
        end -= 1;
    }
    if end == 0 {
        return 0;
    }
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut start = end - 1;
    if word(b[start]) {
        while start > 0 && word(b[start - 1]) {
            start -= 1;
        }
        if start > 0 && matches!(b[start - 1], b'.' | b'$' | b'@') {
            start -= 1;
        }
        // Número com ponto (`1.5`).
        if start > 0 && b[start].is_ascii_digit() && b[start - 1] == b'.' {
            start -= 1;
            while start > 0 && b[start - 1].is_ascii_digit() {
                start -= 1;
            }
        }
        return start;
    }
    // Operadores de dois ou três caracteres.
    let ops = ["?//", "//=", "|=", "+=", "-=", "*=", "/=", "%=", "==", "!=", "<=", ">=", "//", ".."];
    for op in ops {
        if program[..end].ends_with(op) {
            return end - op.len();
        }
    }
    start
}

const KEYWORDS: &[&str] = &[
    "as", "def", "if", "then", "elif", "else", "end", "and", "or", "reduce", "foreach", "try", "catch", "label",
    "import", "include", "module", "__loc__",
];

/// Nome de um token como o bison do jq escreve.
fn token_name(found: &str) -> String {
    if found.is_empty() {
        return "end of file".into();
    }
    let first = found.chars().next().unwrap_or(' ');
    if found.starts_with("$__loc__") {
        return "\"$__loc__\"".into();
    }
    match first {
        '$' => "BINDING".into(),
        '@' => "FORMAT".into(),
        '"' => "QQSTRING_START".into(),
        '0'..='9' => "LITERAL".into(),
        '.' if found.len() > 1 && found.as_bytes()[1].is_ascii_digit() => "LITERAL".into(),
        '.' if found.starts_with("..") => "\"..\"".trim_matches('"').into(),
        '.' if found.len() > 1 => "FIELD".into(),
        c if c.is_ascii_alphabetic() || c == '_' => {
            let word: String = found.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == ':').collect();
            if KEYWORDS.contains(&word.as_str()) {
                word
            } else {
                "IDENT".into()
            }
        }
        '(' | '[' | '{' => format!("'{first}'"),
        _ => {
            let ops = ["?//", "//=", "|=", "+=", "-=", "*=", "/=", "%=", "==", "!=", "<=", ">=", "//"];
            for op in ops {
                if found.starts_with(op) {
                    return op.into();
                }
            }
            format!("'{first}'")
        }
    }
}

fn syntax(unexpected: &str, expecting: Option<&str>) -> String {
    match expecting {
        Some(e) => format!("syntax error, unexpected {unexpected}, expecting {e}{QUOTING}"),
        None => format!("syntax error, unexpected {unexpected}{QUOTING}"),
    }
}

/// Mensagem do jq para uma chave constante que não é string (`{(0): 1}`).
fn obj_key_message(block: &str) -> String {
    let inner = block.trim_start_matches('(').trim_end_matches(')').trim();
    let (kind, shown) = match inner {
        "null" => ("null", "null".to_string()),
        "true" | "false" => ("boolean", inner.to_string()),
        _ => {
            let neg = inner.starts_with('-');
            let lit = inner.trim_start_matches('-').trim();
            let n = Num::from_literal(lit);
            let shown = match (neg, n) {
                (false, Some(n)) => n.dump(),
                (true, Some(n)) => Num::from_f64(-n.as_f64()).dump(),
                (_, None) => inner.to_string(),
            };
            let shown = if shown.len() > 14 { format!("{}...", &shown[..11]) } else { shown };
            ("number", shown)
        }
    };
    format!("Cannot use {kind} ({shown}) as object key")
}

/// Erros de carga (léxico e sintaxe) no formato do jq. Só o primeiro erro de sintaxe é reportado
/// (o bison do jq para no primeiro); erros de chave de objeto são todos reportados.
pub fn load_errors(program: &str, errs: load::Errors<&str, ()>) -> Vec<String> {
    let mut out = Vec::new();
    for (_file, e) in errs {
        match e {
            load::Error::Io(v) => {
                for (p, m) in v {
                    out.push(locate(program, p, &m));
                }
            }
            load::Error::Lex(v) => {
                if let Some((expect, at)) = v.into_iter().next() {
                    out.push(lex_message(program, expect, at));
                }
            }
            load::Error::Parse(v) => {
                let mut syntax_done = false;
                for (expect, found) in v {
                    if let parse::Expect::ObjKey = expect {
                        out.push(locate(program, found, &obj_key_message(found)));
                        continue;
                    }
                    if syntax_done {
                        continue;
                    }
                    syntax_done = true;
                    out.push(parse_message(program, expect, found));
                }
            }
        }
    }
    out
}

fn lex_message(program: &str, expect: lex::Expect<&str>, at: &str) -> String {
    let pos = offset(program, at).unwrap_or(0);
    match expect {
        lex::Expect::Delim(open) if open.starts_with('"') => {
            let msg = syntax("end of file", Some("QQSTRING_TEXT or QQSTRING_INTERP_START or QQSTRING_END"));
            locate_at(program, last_token_start(program), &msg)
        }
        lex::Expect::Delim(open) => {
            let expecting = match open {
                "{" => Some("'}'"),
                _ => None,
            };
            if at.trim_start().is_empty() {
                locate_at(program, last_token_start(program), &syntax("end of file", expecting))
            } else {
                locate_at(program, pos, &syntax("INVALID_CHARACTER", None))
            }
        }
        lex::Expect::Token => {
            // Fecha-bloco sem abertura e caracteres desconhecidos viram INVALID_CHARACTER.
            locate_at(program, pos, &syntax("INVALID_CHARACTER", Some("end of file")))
        }
        lex::Expect::Escape | lex::Expect::Unicode => {
            locate_at(program, pos, &syntax("INVALID_CHARACTER", None))
        }
        _ => locate_at(program, pos, &syntax(&token_name(at), None)),
    }
}

fn parse_message(program: &str, expect: parse::Expect<&str>, found: &str) -> String {
    use parse::Expect;
    let eof = found.is_empty();
    let pos = if eof { last_token_start(program) } else { offset(program, found).unwrap_or(0) };
    let name = token_name(found);
    let msg = match expect {
        Expect::Nothing => syntax(&name, Some("end of file")),
        Expect::Pattern => syntax(&name, Some("BINDING or '[' or '{'")),
        Expect::Var => syntax(&name, Some("BINDING")),
        Expect::CommaOrRBrace if eof => syntax(&name, Some("'}'")),
        Expect::Just("(") => syntax(&name, Some("'('")),
        _ => syntax(&name, None),
    };
    if !eof && found.starts_with('}') && name == "'}'" {
        return locate_at(program, pos, &msg);
    }
    locate_at(program, pos, &msg)
}

/// Erros do compilador (símbolos indefinidos) no formato do jq.
pub fn compile_errors(program: &str, errs: jaq_core::compile::Errors<&str, ()>) -> Vec<String> {
    let mut out = Vec::new();
    for (_file, list) in errs {
        for (name, undef) in list {
            let what = match undef {
                Undefined::Filter(arity) => format!("{name}/{arity} is not defined"),
                Undefined::Var => format!("{name} is not defined"),
                Undefined::Label => format!("$*label-{} is not defined", name.trim_start_matches('$')),
                Undefined::Mod => format!("module not found: {name}"),
                _ => format!("{name} is not defined"),
            };
            out.push(locate(program, name, &what));
        }
    }
    out
}
