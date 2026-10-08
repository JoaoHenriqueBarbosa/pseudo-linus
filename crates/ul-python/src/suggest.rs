//! Sugestões "Did you mean" de `AttributeError`, `NameError` e `ImportError`, com o algoritmo de
//! `Lib/traceback.py` (porta de `Python/suggestions.c`): distância de Levenshtein com custo 2 por
//! inserção, remoção ou troca, e 1 por troca só de caixa.

use std::rc::Rc;

use crate::object::{ExcObj, Value};

const MAX_CANDIDATE_ITEMS: usize = 750;

/// Métodos públicos dos tipos embutidos (o `dir(tipo)` do CPython 3.13 sem os nomes com `_`), porque o
/// interpretador não enumera métodos de tipos primitivos.
const BUILTIN_METHODS: &[(&str, &[&str])] = &[
    ("str", &["capitalize", "casefold", "center", "count", "encode", "endswith", "expandtabs", "find", "format", "format_map", "index", "isalnum", "isalpha", "isascii", "isdecimal", "isdigit", "isidentifier", "islower", "isnumeric", "isprintable", "isspace", "istitle", "isupper", "join", "ljust", "lower", "lstrip", "maketrans", "partition", "removeprefix", "removesuffix", "replace", "rfind", "rindex", "rjust", "rpartition", "rsplit", "rstrip", "split", "splitlines", "startswith", "strip", "swapcase", "title", "translate", "upper", "zfill"]),
    ("list", &["append", "clear", "copy", "count", "extend", "index", "insert", "pop", "remove", "reverse", "sort"]),
    ("dict", &["clear", "copy", "fromkeys", "get", "items", "keys", "pop", "popitem", "setdefault", "update", "values"]),
    ("set", &["add", "clear", "copy", "difference", "difference_update", "discard", "intersection", "intersection_update", "isdisjoint", "issubset", "issuperset", "pop", "remove", "symmetric_difference", "symmetric_difference_update", "union", "update"]),
    ("frozenset", &["copy", "difference", "intersection", "isdisjoint", "issubset", "issuperset", "symmetric_difference", "union"]),
    ("tuple", &["count", "index"]),
    ("int", &["as_integer_ratio", "bit_count", "bit_length", "conjugate", "denominator", "from_bytes", "imag", "is_integer", "numerator", "real", "to_bytes"]),
    ("bool", &["as_integer_ratio", "bit_count", "bit_length", "conjugate", "denominator", "from_bytes", "imag", "is_integer", "numerator", "real", "to_bytes"]),
    ("float", &["as_integer_ratio", "conjugate", "fromhex", "hex", "imag", "is_integer", "real"]),
    ("bytes", &["capitalize", "center", "count", "decode", "endswith", "expandtabs", "find", "fromhex", "hex", "index", "isalnum", "isalpha", "isascii", "isdigit", "islower", "isspace", "istitle", "isupper", "join", "ljust", "lower", "lstrip", "maketrans", "partition", "removeprefix", "removesuffix", "replace", "rfind", "rindex", "rjust", "rpartition", "rsplit", "rstrip", "split", "splitlines", "startswith", "strip", "swapcase", "title", "translate", "upper", "zfill"]),
    ("bytearray", &["append", "capitalize", "center", "clear", "copy", "count", "decode", "endswith", "expandtabs", "extend", "find", "fromhex", "hex", "index", "insert", "isalnum", "isalpha", "isascii", "isdigit", "islower", "isspace", "istitle", "isupper", "join", "ljust", "lower", "lstrip", "maketrans", "partition", "pop", "remove", "removeprefix", "removesuffix", "replace", "reverse", "rfind", "rindex", "rjust", "rpartition", "rsplit", "rstrip", "split", "splitlines", "startswith", "strip", "swapcase", "title", "translate", "upper", "zfill"]),
    ("complex", &["conjugate", "imag", "real"]),
];

/// Métodos públicos de um valor de tipo embutido primitivo (`"x"`, `[]`, `{}`...).
pub fn builtin_methods(type_name: &str) -> &'static [&'static str] {
    BUILTIN_METHODS.iter().find(|(n, _)| *n == type_name).map_or(&[], |(_, m)| m)
}
const MAX_STRING_SIZE: usize = 40;
const MOVE_COST: usize = 2;
const CASE_COST: usize = 1;

fn substitution_cost(a: char, b: char) -> usize {
    if a == b {
        0
    } else if a.to_lowercase().eq(b.to_lowercase()) {
        CASE_COST
    } else {
        MOVE_COST
    }
}

fn levenshtein(a: &[char], b: &[char], max_cost: usize) -> usize {
    if a == b {
        return 0;
    }
    let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[pre..], &b[pre..]);
    let post = a.iter().rev().zip(b.iter().rev()).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[..a.len() - post], &b[..b.len() - post]);
    if a.is_empty() || b.is_empty() {
        return MOVE_COST * (a.len() + b.len());
    }
    if a.len() > MAX_STRING_SIZE || b.len() > MAX_STRING_SIZE {
        return max_cost + 1;
    }
    let (a, b) = if b.len() < a.len() { (b, a) } else { (a, b) };
    if (b.len() - a.len()) * MOVE_COST > max_cost {
        return max_cost + 1;
    }
    let mut row: Vec<usize> = (1..=a.len()).map(|i| i * MOVE_COST).collect();
    let mut result = 0;
    for (bindex, &bchar) in b.iter().enumerate() {
        let mut distance = bindex * MOVE_COST;
        result = distance;
        let mut minimum = usize::MAX;
        for (index, &achar) in a.iter().enumerate() {
            let substitute = distance + substitution_cost(bchar, achar);
            distance = row[index];
            let insert_delete = result.min(distance) + MOVE_COST;
            result = insert_delete.min(substitute);
            row[index] = result;
            minimum = minimum.min(result);
        }
        if minimum > max_cost {
            return max_cost + 1;
        }
    }
    result
}

/// O candidato mais próximo de `wrong` entre `candidates`, ou `None` se nenhum chega perto.
pub fn closest<'a>(wrong: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let candidates: Vec<&str> = candidates.into_iter().collect();
    if candidates.len() > MAX_CANDIDATE_ITEMS {
        return None;
    }
    let wrong_chars: Vec<char> = wrong.chars().collect();
    if wrong_chars.len() > MAX_STRING_SIZE {
        return None;
    }
    let mut best_distance = wrong_chars.len();
    let mut suggestion: Option<&str> = None;
    for name in candidates {
        if name == wrong {
            continue;
        }
        let chars: Vec<char> = name.chars().collect();
        let max_distance = ((chars.len() + wrong_chars.len() + 3) * MOVE_COST / 6).min(best_distance.saturating_sub(1));
        let distance = levenshtein(&wrong_chars, &chars, max_distance);
        if distance > max_distance {
            continue;
        }
        if suggestion.is_none() || distance < best_distance {
            suggestion = Some(name);
            best_distance = distance;
        }
    }
    suggestion.map(str::to_string)
}

/// Nomes de módulos da biblioteca padrão que o CPython sugere importar num `NameError`
/// (a lista de `sys.stdlib_module_names`, lida do `sys.py` embutido).
fn is_stdlib_module(name: &str) -> bool {
    static NAMES: std::sync::OnceLock<std::collections::HashSet<&'static str>> = std::sync::OnceLock::new();
    NAMES
        .get_or_init(|| {
            const MARKER: &str = "stdlib_module_names=frozenset('''";
            let src: &'static str = include_str!("modules/py/sys.py");
            let Some(start) = src.find(MARKER).map(|i| i + MARKER.len()) else { return Default::default() };
            let end = src[start..].find("'''").map_or(src.len(), |i| start + i);
            src[start..end].split_whitespace().collect()
        })
        .contains(name)
}

/// Os candidatos de `AttributeError` e `ImportError`: o `dir()` ordenado e sem repetição, sem os nomes com `_`
/// quando o nome errado não começa com `_` (o `_compute_suggestion_error` de `traceback.py`). O limite de
/// candidatos vale depois do filtro.
fn candidate_names(mut names: Vec<String>, wrong: &str) -> Vec<String> {
    names.sort();
    names.dedup();
    if !wrong.starts_with('_') {
        names.retain(|n| !n.starts_with('_'));
    }
    names
}

/// Sufixo da mensagem de um `NameError`/`AttributeError`/`ImportError` (`. Did you mean: 'x'?`), vazio
/// quando não há sugestão. `attr_names` lista os atributos do objeto (ou do módulo); `scope_names` os
/// nomes visíveis na hora do `NameError`.
pub fn hint_for(e: &ExcObj, attr_names: impl FnOnce(&Value) -> Vec<String>) -> String {
    let wrong_key = if e.kind == "ImportError" || e.kind == "ModuleNotFoundError" { "name_from" } else { "name" };
    let name = match e.extra_get(wrong_key) {
        Some(Value::Str(s)) => s.as_str().to_string(),
        _ => return String::new(),
    };
    let mut hint = String::new();
    let suggestion = match e.kind {
        "AttributeError" => e.extra_get("obj").and_then(|obj| {
            let names = candidate_names(attr_names(&obj), &name);
            closest(&name, names.iter().map(String::as_str))
        }),
        "ImportError" | "ModuleNotFoundError" => match e.extra_get("module") {
            Some(m) => {
                let names = candidate_names(attr_names(&m), &name);
                closest(&name, names.iter().map(String::as_str))
            }
            None => None,
        },
        "NameError" => match e.extra_get("scope") {
            Some(Value::List(l)) => {
                let items = l.borrow();
                let names: Vec<String> = items.iter().map(crate::object::to_str).collect();
                if let Some(Value::Bool(true)) = e.extra_get("self_has") {
                    Some(format!("self.{name}"))
                } else {
                    closest(&name, names.iter().map(String::as_str))
                }
            }
            _ => None,
        },
        _ => None,
    };
    if let Some(s) = &suggestion {
        hint.push_str(&format!(". Did you mean: '{s}'?"));
    }
    if e.kind == "NameError" && is_stdlib_module(&name) {
        if suggestion.is_some() {
            hint.push_str(&format!(" Or did you forget to import '{name}'?"));
        } else {
            hint.push_str(&format!(". Did you forget to import '{name}'?"));
        }
    }
    hint
}

/// Anexa `name` (e `obj`) a um `AttributeError`.
pub fn attribute_error(msg: String, obj: &Value, name: &str) -> Value {
    let e = ExcObj::new("AttributeError", vec![Value::str(msg)]);
    e.extra.borrow_mut().push(("name", Value::str(name.to_string())));
    e.extra.borrow_mut().push(("obj", obj.clone()));
    Value::Exception(Rc::new(e))
}
