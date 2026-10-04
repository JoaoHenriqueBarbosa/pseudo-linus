//! Porte pseudo-linus: regex do jq 1.7.1 com o Oniguruma em Rust (`ferroni`), no lugar do módulo
//! `regex` do jaq (que usava o `regex-bites`, sem lookaround nem as classes e capturas do Oniguruma).
//!
//! [`f_match`] é o `f_match` de `src/builtin.c`: mesma sintaxe (`ONIG_SYNTAX_PERL_NT`), mesmas
//! opções por modificador, mesmo laço de busca global (inclusive o avanço de um byte depois de casamento
//! vazio), offsets e comprimentos em codepoints, grupos que não casaram com `offset: -1`, nomes de
//! grupo por `onig_foreach_name`, e as mensagens de erro do jq.

use crate::ValT;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use ferroni::oniguruma::{
    OnigOptionType, OnigRegion, ONIG_MISMATCH, ONIG_OPTION_CAPTURE_GROUP, ONIG_OPTION_EXTEND, ONIG_OPTION_FIND_LONGEST,
    ONIG_OPTION_FIND_NOT_EMPTY, ONIG_OPTION_IGNORECASE, ONIG_OPTION_MULTILINE, ONIG_OPTION_NONE, ONIG_OPTION_SINGLELINE,
};
use jaq_core::{Error, ValR};

fn err<V: ValT>(msg: String) -> Error<V> {
    Error::new(V::from(msg))
}

fn type_error<V: ValT>(v: &V, msg: &str) -> Error<V> {
    err(format!("{} ({}) {msg}", v.kind_name(), v.dump_trunc(15)))
}

/// Comprimento de uma sequência UTF-8 pelo primeiro byte (`jvp_utf8_decode_length`).
fn utf8_len(b: u8) -> usize {
    match b {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}

/// Número de codepoints antes do byte `pos` (o laço de `fr += jvp_utf8_decode_length(*fr)`).
fn cp_index(s: &[u8], pos: usize) -> usize {
    let (mut i, mut n) = (0, 0);
    while i < pos && i < s.len() {
        i += utf8_len(s[i]);
        n += 1;
    }
    n
}

fn obj<V: ValT>(pairs: Vec<(&str, V)>) -> V {
    V::from_map(pairs.into_iter().map(|(k, v)| (V::from(k.to_string()), v))).unwrap_or_else(|_| V::null())
}

fn num<V: ValT>(n: usize) -> V {
    V::from(n as isize)
}

/// `f_match(input, regex, modifiers, testmode)` do jq.
pub fn f_match<V: ValT>(input: &V, regex: &V, modifiers: &V, test: bool) -> ValR<V> {
    let Some(subject) = input.as_utf8_bytes() else {
        return Err(type_error(input, "cannot be matched, as it is not a string"));
    };
    let Some(pattern) = regex.as_utf8_bytes() else {
        return Err(type_error(regex, "is not a string"));
    };
    let mut options: OnigOptionType = ONIG_OPTION_CAPTURE_GROUP;
    let mut global = false;
    if let Some(m) = modifiers.as_utf8_bytes() {
        let text = String::from_utf8_lossy(m);
        for c in text.chars() {
            match c {
                'g' => global = true,
                'i' => options |= ONIG_OPTION_IGNORECASE,
                'x' => options |= ONIG_OPTION_EXTEND,
                'm' => options |= ONIG_OPTION_MULTILINE,
                's' => options |= ONIG_OPTION_SINGLELINE,
                'p' => options |= ONIG_OPTION_MULTILINE | ONIG_OPTION_SINGLELINE,
                'l' => options |= ONIG_OPTION_FIND_LONGEST,
                'n' => options |= ONIG_OPTION_FIND_NOT_EMPTY,
                _ => return Err(err(format!("{text} is not a valid modifier string"))),
            }
        }
    } else if *modifiers != V::null() {
        return Err(type_error(modifiers, "is not a string"));
    }

    let reg = match ferroni::regcomp::onig_new(
        pattern,
        options,
        &ferroni::encodings::utf8::ONIG_ENCODING_UTF8,
        &ferroni::regsyntax::OnigSyntaxPerl_NG,
    ) {
        Ok(r) => r,
        Err(e) => return Err(err(format!("Regex failure: {}", onig_message(&e)))),
    };

    // Nomes de grupo: grupo -> nome.
    let mut names: Vec<(i32, String)> = Vec::new();
    ferroni::regexec::onig_foreach_name(&reg, |name, groups| {
        for g in groups {
            names.push((*g, String::from_utf8_lossy(name).into_owned()));
        }
        0
    });
    let name_of = |group: usize| -> V {
        names
            .iter()
            .find(|(g, _)| *g as usize == group)
            .map(|(_, n)| V::from(n.clone()))
            .unwrap_or_else(V::null)
    };

    let end = subject.len();
    let mut start = 0usize;
    let mut results: Vec<V> = Vec::new();
    loop {
        let (r, region) = ferroni::regexec::onig_search(&reg, subject, end, start, end, Some(OnigRegion::new()), ONIG_OPTION_NONE);
        if r >= 0 {
            if test {
                return Ok(V::from(true));
            }
            let Some(region) = region else { break };
            let beg0 = region.beg[0] as usize;
            let end0 = region.end[0] as usize;
            let nregs = region.num_regs as usize;
            if end0 == beg0 {
                // Casamento vazio.
                let idx = cp_index(subject, beg0);
                let mut captures = Vec::new();
                for i in 1..nregs {
                    captures.push(obj(alloc::vec![
                        ("offset", num::<V>(idx)),
                        ("string", V::from(String::new())),
                        ("length", num::<V>(0)),
                        ("name", name_of(i)),
                    ]));
                }
                results.push(obj(alloc::vec![
                    ("offset", num::<V>(idx)),
                    ("length", num::<V>(0)),
                    ("string", V::from(String::new())),
                    ("captures", captures.into_iter().collect()),
                ]));
                // Garante que '"qux" | match("(?=u)"; "g")' case uma vez só.
                start = end0 + 1;
            } else {
                let idx = cp_index(subject, beg0);
                let len = cp_index(subject, end0) - idx;
                let text = String::from_utf8_lossy(&subject[beg0..end0]).into_owned();
                let mut captures = Vec::new();
                for i in 1..nregs {
                    let (b, e) = (region.beg[i], region.end[i]);
                    let cap = if b == e {
                        if b == -1 {
                            obj(alloc::vec![
                                ("offset", V::from(-1isize)),
                                ("string", V::null()),
                                ("length", num::<V>(0)),
                                ("name", name_of(i)),
                            ])
                        } else {
                            obj(alloc::vec![
                                ("offset", num::<V>(cp_index(subject, b as usize))),
                                ("string", V::from(String::new())),
                                ("length", num::<V>(0)),
                                ("name", name_of(i)),
                            ])
                        }
                    } else {
                        let ci = cp_index(subject, b as usize);
                        let cl = cp_index(subject, e as usize) - ci;
                        obj(alloc::vec![
                            ("offset", num::<V>(ci)),
                            ("length", num::<V>(cl)),
                            ("string", V::from(String::from_utf8_lossy(&subject[b as usize..e as usize]).into_owned())),
                            ("name", name_of(i)),
                        ])
                    };
                    captures.push(cap);
                }
                results.push(obj(alloc::vec![
                    ("offset", num::<V>(idx)),
                    ("length", num::<V>(len)),
                    ("string", V::from(text)),
                    ("captures", captures.into_iter().collect()),
                ]));
                start = end0;
            }
        } else if r == ONIG_MISMATCH {
            break;
        } else {
            return Err(err(format!("Regex failure: {}", ferroni::regerror::onig_error_code_to_str(r, None))));
        }
        if !(global && start <= end) {
            break;
        }
    }
    if test {
        return Ok(V::from(false));
    }
    Ok(results.into_iter().collect())
}

/// Texto do `onig_error_code_to_str` para um erro de compilação.
fn onig_message(e: &ferroni::error::RegexError) -> String {
    use ferroni::error::RegexError::*;
    match e {
        Syntax { message, .. } | InternalBug { message, .. } | Encoding { message, .. } => message.clone(),
        other => ferroni::regerror::onig_error_code_to_str(other.code(), None),
    }
}
