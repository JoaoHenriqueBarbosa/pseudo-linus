//! `declare`, `typeset`, `local`, `export`, `readonly` e `unset`.

use super::{Arg, AssignArg, AssignedValue, out, parse_opts};
use crate::shell::{Exec, Shell};
use crate::vars::{Assoc, Attrs, Value, Var};
use crate::word::is_name;

/// Letras de atributo na ordem em que o bash mostra (`declare -ir`, `declare -ax`...).
const ATTR_ORDER: &[(u8, Attrs)] = &[
    (b'a', Attrs::INDEXED),
    (b'A', Attrs::ASSOC),
    (b'i', Attrs::INTEGER),
    (b'l', Attrs::LOWER),
    (b'n', Attrs::NAMEREF),
    (b'r', Attrs::READONLY),
    (b't', Attrs::TRACE),
    (b'u', Attrs::UPPER),
    (b'x', Attrs::EXPORT),
];

pub fn attr_letters(v: &Var) -> String {
    let mut s = String::new();
    let indexed = v.attrs.has(Attrs::INDEXED) || matches!(v.value, Value::Indexed(_));
    let assoc = v.attrs.has(Attrs::ASSOC) || matches!(v.value, Value::Assoc(_));
    for (c, a) in ATTR_ORDER {
        let on = match *c {
            b'a' => indexed && !assoc,
            b'A' => assoc,
            _ => v.attrs.has(*a),
        };
        if on {
            s.push(*c as char);
        }
    }
    s
}

/// Chave de associativo no `declare -p`: entre aspas só quando precisa.
fn assoc_key(k: &[u8]) -> Vec<u8> {
    let plain = !k.is_empty() && k.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-' | b'/' | b':' | b'+' | b'@' | b'%' | b',') || *c >= 0x80);
    if plain { k.to_vec() } else { crate::quote::double_quote_value(k) }
}

/// Uma linha do `declare -p` (sem o newline).
pub fn declare_line(name: &str, v: &Var) -> String {
    let letters = attr_letters(v);
    let flags = if letters.is_empty() { "--".to_string() } else { format!("-{letters}") };
    let mut out = format!("declare {flags} {name}").into_bytes();
    match &v.value {
        Value::Unset => {
            if v.attrs.has(Attrs::INDEXED) || v.attrs.has(Attrs::ASSOC) {
                // `declare -a x` sem valor ainda mostra `=()` no bash 5.2? Não: mostra só o nome.
            }
        }
        Value::Scalar(s) => {
            out.push(b'=');
            out.extend(crate::quote::double_quote_value(s));
        }
        Value::Indexed(m) => {
            out.extend_from_slice(b"=(");
            let mut first = true;
            for (k, val) in m {
                if !first {
                    out.push(b' ');
                }
                first = false;
                out.extend_from_slice(format!("[{k}]=").as_bytes());
                out.extend(crate::quote::double_quote_value(val));
            }
            out.push(b')');
        }
        Value::Assoc(a) => {
            out.extend_from_slice(b"=(");
            for (k, val) in a.iter() {
                out.push(b'[');
                out.extend(assoc_key(k));
                out.extend_from_slice(b"]=");
                out.extend(crate::quote::double_quote_value(val));
                out.push(b' ');
            }
            out.push(b')');
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Formato do `set`/`declare` sem opções: `nome=valor` com aspas simples quando precisa.
pub fn set_line(name: &str, v: &Var) -> Option<Vec<u8>> {
    let mut out = name.as_bytes().to_vec();
    match &v.value {
        Value::Unset => return None,
        Value::Scalar(s) => {
            out.push(b'=');
            out.extend(crate::quote::set_value(s));
        }
        Value::Indexed(m) => {
            out.extend_from_slice(b"=(");
            let mut first = true;
            for (k, val) in m {
                if !first {
                    out.push(b' ');
                }
                first = false;
                out.extend_from_slice(format!("[{k}]=").as_bytes());
                out.extend(crate::quote::double_quote_value(val));
            }
            out.push(b')');
        }
        Value::Assoc(a) => {
            out.extend_from_slice(b"=(");
            for (k, val) in a.iter() {
                out.push(b'[');
                out.extend(assoc_key(k));
                out.extend_from_slice(b"]=");
                out.extend(crate::quote::double_quote_value(val));
                out.push(b' ');
            }
            out.push(b')');
        }
    }
    Some(out)
}

fn usage(name: &str) -> &'static str {
    match name {
        "local" => "local [option] name[=value] ...",
        "export" => "export [-fn] [name[=value] ...] or export -p",
        "readonly" => "readonly [-aAf] [name[=value] ...] or readonly -p",
        "typeset" => "typeset [-aAfFgiIlnrtux] name[=value] ... or typeset -p [-aAfFilnrtux] [name ...]",
        _ => "declare [-aAfFgiIlnrtux] name[=value] ... or declare -p [-aAfFilnrtux] [name ...]",
    }
}

/// Separa as opções (palavras que começam com `-` ou `+`) dos operandos.
fn split_options(args: &[Arg]) -> (Vec<Vec<u8>>, usize) {
    let mut opts = vec![Vec::new()];
    let mut i = 1;
    while i < args.len() {
        match &args[i] {
            Arg::Word(w) if w == b"--" => {
                i += 1;
                break;
            }
            Arg::Word(w) if w.len() > 1 && (w[0] == b'-' || w[0] == b'+') => {
                opts.push(w.clone());
                i += 1;
            }
            _ => break,
        }
    }
    (opts, i)
}

pub fn run(sh: &mut Shell, name: &str, args: &[Arg]) -> Exec {
    let (optwords, first) = split_options(args);
    let spec = match name {
        "export" => "fnp",
        "readonly" => "aAfp",
        "local" => "aAfFgiIlnrtux",
        _ => "aAfFgiIlnprtux",
    };
    let opts = match parse_opts(&optwords, spec, true) {
        Ok(o) => o,
        Err(e) => return Ok(super::opt_error(sh, name, e, usage(name))),
    };
    if name == "local" && !sh.in_function() {
        sh.builtin_error("local", "can only be used in a function");
        return Ok(1);
    }
    let operands = &args[first..];
    let mut set_attrs = Attrs::default();
    let mut clear_attrs = Attrs::default();
    let letter = |c: u8| -> Option<Attrs> {
        Some(match c {
            b'a' => Attrs::INDEXED,
            b'A' => Attrs::ASSOC,
            b'i' => Attrs::INTEGER,
            b'l' => Attrs::LOWER,
            b'n' => Attrs::NAMEREF,
            b'r' => Attrs::READONLY,
            b't' => Attrs::TRACE,
            b'u' => Attrs::UPPER,
            b'x' => Attrs::EXPORT,
            _ => return None,
        })
    };
    for (c, _) in &opts.flags {
        if let Some(a) = letter(*c) {
            set_attrs.set(a);
        }
    }
    for c in &opts.plus {
        if let Some(a) = letter(*c) {
            clear_attrs.set(a);
        }
    }
    match name {
        "export" if !opts.has(b'n') => set_attrs.set(Attrs::EXPORT),
        "export" => clear_attrs.set(Attrs::EXPORT),
        "readonly" => set_attrs.set(Attrs::READONLY),
        _ => {}
    }
    let functions = opts.has(b'f') || opts.has(b'F');
    let print = opts.has(b'p') || (operands.is_empty() && !matches!(name, "local"));

    // Funções.
    if functions {
        return functions_mode(sh, name, &opts, operands);
    }

    if print && operands.is_empty() {
        return Ok(list_vars(sh, name, set_attrs, opts.has(b'p') || name != "declare" && name != "typeset" || set_attrs.0 != 0));
    }
    if opts.has(b'p') {
        let mut status = 0;
        let mut text = String::new();
        for op in operands {
            let n = String::from_utf8_lossy(op.as_bytes()).into_owned();
            match sh.vars.get(&n) {
                Some(v) => text.push_str(&format!("{}\n", declare_line(&n, v))),
                None => {
                    sh.builtin_error(name, format!("{n}: not found"));
                    status = 1;
                }
            }
        }
        out(sh, name, text.as_bytes());
        return Ok(status);
    }

    let global = opts.has(b'g');
    let make_local = name == "local" || (sh.in_function() && !global && matches!(name, "declare" | "typeset"));
    let mut status = 0;
    for op in operands {
        let assign: AssignArg = match op {
            Arg::Assign(a) => a.clone(),
            Arg::Word(w) => match parse_word_assignment(w) {
                Some(a) => a,
                None => {
                    // Só o nome.
                    let n = String::from_utf8_lossy(w).into_owned();
                    AssignArg { name: n.clone(), index: None, append: false, value: AssignedValue::Scalar(Vec::new()), raw: n }
                }
            },
        };
        let has_value = match op {
            Arg::Assign(_) => true,
            Arg::Word(w) => w.contains(&b'='),
        };
        let vname = assign.name.clone();
        let base_ok = is_name(vname.as_bytes());
        if !base_ok {
            sh.builtin_error(name, format!("`{}': not a valid identifier", String::from_utf8_lossy(op.as_bytes())));
            status = 1;
            continue;
        }
        // Readonly não pode mudar.
        if let Some(v) = sh.vars.get(&vname)
            && v.attrs.has(Attrs::READONLY) && (has_value || clear_attrs.has(Attrs::READONLY)) && !(make_local && sh.vars.current_function_scope().is_some_and(|i| sh.vars.get_in(i, &vname).is_none())) {
                sh.builtin_error(name, format!("{vname}: readonly variable"));
                status = 1;
                continue;
            }
        // Escolhe o escopo e prepara a entrada com os atributos.
        let existed;
        {
            let entry: &mut Var = if make_local {
                let (e, ex) = sh.vars.local_entry(&vname).expect("dentro de função");
                existed = ex;
                e
            } else if global {
                existed = sh.vars.get_in(0, &vname).is_some();
                sh.vars.global_entry(&vname)
            } else {
                existed = sh.vars.get(&vname).is_some();
                sh.vars.entry(&vname)
            };
            // Conversões de tipo de array.
            if set_attrs.has(Attrs::ASSOC) && matches!(entry.value, Value::Indexed(_)) {
                sh.builtin_error(name, format!("{vname}: cannot convert indexed to associative array"));
                status = 1;
                continue;
            }
            if set_attrs.has(Attrs::INDEXED) && matches!(entry.value, Value::Assoc(_)) {
                sh.builtin_error(name, format!("{vname}: cannot convert associative to indexed array"));
                status = 1;
                continue;
            }
            let _ = existed;
            if set_attrs.has(Attrs::INDEXED) {
                if let Value::Scalar(s) = &entry.value {
                    let s = s.clone();
                    entry.value = Value::Indexed([(0, s)].into_iter().collect());
                } else if matches!(entry.value, Value::Unset) && !has_value {
                    entry.value = Value::Indexed(Default::default());
                }
            }
            if set_attrs.has(Attrs::ASSOC) {
                match &entry.value {
                    Value::Scalar(s) => {
                        let s = s.clone();
                        let mut a = Assoc::default();
                        a.insert(b"0".to_vec(), s);
                        entry.value = Value::Assoc(a);
                    }
                    Value::Unset if !has_value => entry.value = Value::Assoc(Assoc::default()),
                    _ => {}
                }
            }
            let mut a = entry.attrs;
            let ro_later = set_attrs.has(Attrs::READONLY);
            let mut add = set_attrs;
            add.clear(Attrs::READONLY);
            a.set(add);
            a.0 &= !clear_attrs.0;
            // -l e -u se excluem: vale a última.
            if set_attrs.has(Attrs::LOWER) {
                a.clear(Attrs::UPPER);
            }
            if set_attrs.has(Attrs::UPPER) {
                a.clear(Attrs::LOWER);
            }
            entry.attrs = a;
            let _ = ro_later;
        }
        if has_value {
            let r = if make_local || global {
                assign_in_place(sh, &assign, make_local, global)?
            } else {
                sh.apply_assign_arg(&assign)?
            };
            if !r {
                status = 1;
            }
        } else if set_attrs.has(Attrs::INTEGER) || set_attrs.has(Attrs::LOWER) || set_attrs.has(Attrs::UPPER) {
            // Atributo novo num valor existente: o bash não reconverte o valor atual.
        }
        if set_attrs.has(Attrs::READONLY)
            && let Some(v) = target_var(sh, &vname, make_local, global) {
                v.attrs.set(Attrs::READONLY);
            }
        if set_attrs.has(Attrs::EXPORT) && name == "export" && !has_value {
            // `export x` de variável inexistente: fica declarada e exportada, sem valor.
        }
    }
    Ok(status)
}

fn target_var<'a>(sh: &'a mut Shell, name: &str, local: bool, global: bool) -> Option<&'a mut Var> {
    if local {
        let i = sh.vars.current_function_scope()?;
        let _ = i;
        return sh.vars.local_entry(name).map(|(v, _)| v);
    }
    if global {
        return Some(sh.vars.global_entry(name));
    }
    sh.vars.get_mut(name)
}

/// Atribuição feita no escopo escolhido pelo `local`/`declare -g` (sem subir pros escopos de fora).
fn assign_in_place(sh: &mut Shell, a: &AssignArg, local: bool, global: bool) -> Result<bool, crate::shell::Flow> {
    // Isola: temporariamente o nome resolve pro escopo certo porque a entrada já foi criada lá e é a
    // mais interna (local) ou, no -g, gravamos direto no global.
    if global && sh.vars.find_scope(&a.name) != Some(0) {
        // Valor vai pro global mesmo havendo local com o mesmo nome.
        let attrs = sh.vars.get_in(0, &a.name).map(|v| v.attrs).unwrap_or_default();
        let old = sh.vars.get_in(0, &a.name).and_then(|v| v.scalar_value().map(|x| x.to_vec()));
        match &a.value {
            AssignedValue::Scalar(v) => {
                let v = sh.convert_value(&a.name, attrs, old.as_deref(), v.clone(), a.append)?;
                sh.vars.global_entry(&a.name).value = Value::Scalar(v);
            }
            AssignedValue::Array(_) => {
                return sh.apply_assign_arg(a);
            }
        }
        return Ok(true);
    }
    let _ = local;
    sh.apply_assign_arg(a)
}

/// `nome=valor` vindo como palavra (quando o builtin não foi reconhecido no parse).
fn parse_word_assignment(w: &[u8]) -> Option<AssignArg> {
    let eq = w.iter().position(|c| *c == b'=')?;
    let mut lhs = &w[..eq];
    let append = lhs.ends_with(b"+");
    if append {
        lhs = &lhs[..lhs.len() - 1];
    }
    let (name, index) = match lhs.iter().position(|c| *c == b'[') {
        Some(b) if lhs.ends_with(b"]") => (&lhs[..b], Some(lhs[b + 1..lhs.len() - 1].to_vec())),
        _ => (lhs, None),
    };
    let value = w[eq + 1..].to_vec();
    let raw = String::from_utf8_lossy(w).into_owned();
    // `declare -a x='(a b)'` vale como atribuição composta.
    let value = if value.starts_with(b"(") && value.ends_with(b")") && index.is_none() {
        let inner = String::from_utf8_lossy(&value[1..value.len() - 1]).into_owned();
        let items: Vec<(Option<Vec<u8>>, bool, Vec<u8>)> = inner
            .split_whitespace()
            .map(|it| {
                if it.starts_with('[')
                    && let Some(close) = it.find("]=") {
                        return (Some(it.as_bytes()[1..close].to_vec()), false, it.as_bytes()[close + 2..].to_vec());
                    }
                (None, false, it.as_bytes().to_vec())
            })
            .collect();
        AssignedValue::Array(items)
    } else {
        AssignedValue::Scalar(value)
    };
    Some(AssignArg { name: String::from_utf8_lossy(name).into_owned(), index, append, value, raw })
}

fn list_vars(sh: &mut Shell, name: &str, filter: Attrs, declare_form: bool) -> i32 {
    let mut text = Vec::new();
    for (n, v) in sh.vars.visible() {
        if filter.0 != 0 && v.attrs.0 & filter.0 != filter.0 {
            // Filtro por atributo (export -p só exportadas etc.).
            let arr_ok = (filter.has(Attrs::INDEXED) && matches!(v.value, Value::Indexed(_))) || (filter.has(Attrs::ASSOC) && matches!(v.value, Value::Assoc(_)));
            if !arr_ok {
                continue;
            }
        }
        if name == "export" && !v.attrs.has(Attrs::EXPORT) {
            continue;
        }
        if name == "readonly" && !v.attrs.has(Attrs::READONLY) {
            continue;
        }
        if declare_form {
            text.extend(declare_line(&n, v).into_bytes());
            text.push(b'\n');
        } else if let Some(l) = set_line(&n, v) {
            text.extend(l);
            text.push(b'\n');
        }
    }
    if !declare_form && name == "declare" {
        let mut names: Vec<&String> = sh.funcs.keys().collect();
        names.sort();
        for f in names {
            text.extend(crate::print::function_text(&sh.funcs[f]).into_bytes());
            text.push(b'\n');
        }
    }
    let cmd = name.to_string();
    if out(sh, &cmd, &text) { 0 } else { 1 }
}

fn functions_mode(sh: &mut Shell, name: &str, opts: &super::Opts, operands: &[Arg]) -> Exec {
    let names_only = opts.has(b'F');
    let mut text = String::new();
    let mut status = 0;
    if operands.is_empty() {
        let mut names: Vec<String> = sh.funcs.keys().cloned().collect();
        names.sort();
        for n in names {
            if names_only {
                text.push_str(&format!("declare -f {n}\n"));
            } else {
                text.push_str(&crate::print::function_text(&sh.funcs[&n]));
                text.push('\n');
            }
        }
    } else {
        for op in operands {
            let n = String::from_utf8_lossy(op.as_bytes()).into_owned();
            match sh.funcs.get(&n) {
                Some(f) => {
                    if opts.has(b'x') || opts.has(b'r') || opts.has(b't') || name == "export" || name == "readonly" {
                        continue;
                    }
                    if names_only {
                        text.push_str(&format!("{n}\n"));
                    } else {
                        text.push_str(&crate::print::function_text(f));
                        text.push('\n');
                    }
                }
                None => {
                    if name == "export" || name == "readonly" {
                        sh.builtin_error(name, format!("{n}: not a function"));
                    }
                    status = 1;
                }
            }
        }
    }
    out(sh, name, text.as_bytes());
    Ok(status)
}

/// `unset [-fvn] nome...`.
pub fn unset(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let opts = match parse_opts(argv, "fvn", false) {
        Ok(o) => o,
        Err(e) => return Ok(super::opt_error(sh, "unset", e, "unset [-f] [-v] [-n] [name ...]")),
    };
    let mut status = 0;
    for a in &argv[opts.rest..] {
        let s = String::from_utf8_lossy(a).into_owned();
        if opts.has(b'f') {
            sh.funcs.remove(&s);
            continue;
        }
        // Elemento: nome[índice].
        if let Some(b) = s.find('[') {
            let base = &s[..b];
            if !s.ends_with(']') || !is_name(base.as_bytes()) || s.len() < b + 2 {
                sh.builtin_error("unset", format!("`{s}': not a valid identifier"));
                status = 1;
                continue;
            }
            let key = &s[b + 1..s.len() - 1];
            if key == "@" || key == "*" {
                sh.unset_var(base);
                continue;
            }
            if !unset_element(sh, base, key.as_bytes())? {
                status = 1;
            }
            continue;
        }
        if !is_name(a) {
            sh.builtin_error("unset", format!("`{s}': not a valid identifier"));
            status = 1;
            continue;
        }
        if opts.has(b'n') {
            if let Some(v) = sh.vars.get(&s)
                && v.attrs.has(Attrs::READONLY) {
                    sh.builtin_error("unset", format!("{s}: cannot unset: readonly variable"));
                    status = 1;
                    continue;
                }
            sh.vars.unset(&s);
            continue;
        }
        let is_var = sh.vars.get(&s).is_some();
        if !is_var && !opts.has(b'v') && sh.funcs.contains_key(&s) {
            sh.funcs.remove(&s);
            continue;
        }
        if !sh.unset_var(&s) {
            status = 1;
        }
    }
    Ok(status)
}

fn unset_element(sh: &mut Shell, name: &str, key: &[u8]) -> Result<bool, crate::shell::Flow> {
    let real = sh.resolve_nameref(name);
    if let Some(v) = sh.vars.get(&real)
        && v.attrs.has(Attrs::READONLY) {
            sh.builtin_error("unset", format!("{real}: cannot unset: readonly variable"));
            return Ok(false);
        }
    let is_assoc = sh.vars.get(&real).is_some_and(|v| matches!(v.value, Value::Assoc(_)));
    if is_assoc {
        let w = crate::word::make_word(&String::from_utf8_lossy(key), crate::word::WordOpts::mode(crate::word::Mode::Subscript, sh.lineno))
            .map_err(|_| crate::shell::Flow::Discard)?;
        let k = sh.expand_word_string(&w)?;
        if let Some(Var { value: Value::Assoc(a), .. }) = sh.vars.get_mut(&real) {
            a.remove(&k);
        }
        return Ok(true);
    }
    let idx = sh.arith_eval(key)?;
    let Some(idx) = sh.resolve_index(&real, idx) else {
        sh.builtin_error("unset", format!("{real}[{}]: bad array subscript", String::from_utf8_lossy(key)));
        return Ok(false);
    };
    match sh.vars.get_mut(&real) {
        Some(Var { value: Value::Indexed(m), .. }) => {
            m.remove(&idx);
        }
        Some(v @ Var { value: Value::Scalar(_), .. }) if idx == 0 => {
            v.value = Value::Unset;
        }
        _ => {}
    }
    Ok(true)
}
