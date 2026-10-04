//! Opções no estilo do `parse-options` do git: curtas agrupadas (`-qm msg`, `-am`), longas com `=`
//! ou no argumento seguinte, abreviação única de opção longa, `--no-<opção>`, `--` e as mensagens
//! de erro com o uso do comando (exit 129).

use crate::error::{Fail, R};
use crate::os;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Arg {
    /// Flag (booleana); aceita `--no-`.
    None,
    /// Exige valor.
    Required,
    /// Valor só se colado (`--opt=v`, `-ov`).
    Optional,
}

#[derive(Copy, Clone, Debug)]
pub struct Spec {
    pub short: Option<u8>,
    pub long: Option<&'static str>,
    pub arg: Arg,
    /// Aceita `--no-<long>`.
    pub negatable: bool,
    /// Nome interno (o que o comando consulta).
    pub id: &'static str,
}

pub const fn flag(short: Option<u8>, long: &'static str, id: &'static str) -> Spec {
    Spec { short, long: Some(long), arg: Arg::None, negatable: true, id }
}

pub const fn value(short: Option<u8>, long: &'static str, id: &'static str) -> Spec {
    Spec { short, long: Some(long), arg: Arg::Required, negatable: true, id }
}

pub const fn optional(short: Option<u8>, long: &'static str, id: &'static str) -> Spec {
    Spec { short, long: Some(long), arg: Arg::Optional, negatable: true, id }
}

pub const fn short_flag(c: u8, id: &'static str) -> Spec {
    Spec { short: Some(c), long: None, arg: Arg::None, negatable: false, id }
}

pub const fn short_value(c: u8, id: &'static str) -> Spec {
    Spec { short: Some(c), long: None, arg: Arg::Required, negatable: false, id }
}

pub const fn noneg(mut s: Spec) -> Spec {
    s.negatable = false;
    s
}

/// Uma ocorrência de opção.
#[derive(Clone, Debug)]
pub struct Hit {
    pub id: &'static str,
    pub value: Option<Vec<u8>>,
    pub negated: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Parsed {
    pub hits: Vec<Hit>,
    pub args: Vec<Vec<u8>>,
    /// Posição em `args` onde estava o `--` (se havia).
    pub dashdash: Option<usize>,
    /// Opções desconhecidas, na ordem (com `KEEP_UNKNOWN`).
    pub unknown: Vec<Vec<u8>>,
}

impl Parsed {
    pub fn has(&self, id: &str) -> bool {
        self.flag(id).unwrap_or(false)
    }

    /// Último estado de uma flag (`Some(false)` se a última foi `--no-`).
    pub fn flag(&self, id: &str) -> Option<bool> {
        self.hits.iter().rev().find(|h| h.id == id).map(|h| !h.negated)
    }

    pub fn count(&self, id: &str) -> usize {
        let mut n = 0;
        for h in &self.hits {
            if h.id == id {
                if h.negated {
                    n = 0;
                } else {
                    n += 1;
                }
            }
        }
        n
    }

    /// Último valor dado (ou `None` se ausente ou negado por último).
    pub fn value(&self, id: &str) -> Option<&[u8]> {
        let h = self.hits.iter().rev().find(|h| h.id == id)?;
        if h.negated { None } else { h.value.as_deref() }
    }

    pub fn value_str(&self, id: &str) -> Option<String> {
        self.value(id).map(os::lossy)
    }

    /// Todos os valores, na ordem (um `--no-` zera a lista).
    pub fn values(&self, id: &str) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for h in &self.hits {
            if h.id == id {
                if h.negated {
                    out.clear();
                } else if let Some(v) = &h.value {
                    out.push(v.clone());
                }
            }
        }
        out
    }

    pub fn present(&self, id: &str) -> bool {
        self.hits.iter().any(|h| h.id == id)
    }

    /// Argumentos antes do `--` e depois.
    pub fn split_dashdash(&self) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
        match self.dashdash {
            Some(i) => (self.args[..i].to_vec(), self.args[i..].to_vec()),
            None => (self.args.clone(), Vec::new()),
        }
    }
}

pub const STOP_AT_NON_OPTION: u32 = 1;
pub const KEEP_DASHDASH: u32 = 2;
pub const KEEP_UNKNOWN: u32 = 4;
/// `-NUM` vira a opção `id` "number" (como `git log -5`).
pub const NUMBER: u32 = 8;

/// Erro de opção: `error: <msg>` e o uso, exit 129.
pub fn usage_error(usage: &str, msg: &str) -> Fail {
    os::err_line("error: ", msg);
    usage_to_stderr(usage);
    Fail::Exit(129)
}

/// Erro sem a palavra `error:` (o `usage_msg_opt`): `fatal: <msg>` e o uso.
pub fn usage_fatal(usage: &str, msg: &str) -> Fail {
    os::err_line("fatal: ", msg);
    usage_to_stderr(usage);
    Fail::Exit(129)
}

pub fn usage_to_stderr(usage: &str) {
    if usage.is_empty() {
        return;
    }
    let mut u = usage.to_string();
    if !u.ends_with('\n') {
        u.push('\n');
    }
    os::errs(&u);
}

/// `git <cmd> -h`: o uso no stdout, exit 129.
pub fn usage_help(usage: &str) -> Fail {
    let mut u = usage.to_string();
    if !u.ends_with('\n') {
        u.push('\n');
    }
    os::outs(&u);
    Fail::Exit(129)
}

fn find_long<'a>(specs: &'a [Spec], name: &str, usage: &str, arg_text: &str) -> R<Option<(&'a Spec, bool)>> {
    // Exata primeiro (inclusive a forma negada).
    for s in specs {
        if s.long == Some(name) {
            return Ok(Some((s, false)));
        }
    }
    if let Some(rest) = name.strip_prefix("no-") {
        for s in specs {
            if s.long == Some(rest) && s.negatable {
                return Ok(Some((s, true)));
            }
        }
    }
    // `--edit` quando a opção declarada é `no-edit`.
    for s in specs {
        if let Some(l) = s.long
            && l.strip_prefix("no-") == Some(name)
            && s.negatable
        {
            return Ok(Some((s, true)));
        }
    }
    // Abreviação única.
    let mut cands: Vec<(&Spec, bool, String)> = Vec::new();
    for s in specs {
        let Some(l) = s.long else { continue };
        if l.starts_with(name) {
            cands.push((s, false, l.to_string()));
        } else if s.negatable
            && let Some(rest) = name.strip_prefix("no-")
            && !rest.is_empty()
            && l.starts_with(rest)
        {
            cands.push((s, true, format!("no-{l}")));
        }
    }
    cands.dedup_by(|a, b| a.0.id == b.0.id && a.1 == b.1);
    match cands.len() {
        0 => Ok(None),
        1 => Ok(Some((cands[0].0, cands[0].1))),
        _ => {
            let a = &cands[0].2;
            let b = &cands[1].2;
            Err(usage_error(usage, &format!("ambiguous option: {arg_text} (could be --{a} or --{b})")))
        }
    }
}

/// Analisa `args` (sem o nome do subcomando).
pub fn parse(specs: &[Spec], args: &[Vec<u8>], flags: u32, usage: &str) -> R<Parsed> {
    let mut p = Parsed::default();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if a == b"--" {
            // Com KEEP_DASHDASH o `--` fica nos argumentos e `dashdash` aponta pra ele; sem, aponta
            // pro primeiro argumento depois dele.
            p.dashdash = Some(p.args.len());
            if flags & KEEP_DASHDASH != 0 {
                p.args.push(a.clone());
            }
            p.args.extend(args[i..].iter().cloned());
            return Ok(p);
        }
        if a.len() < 2 || a[0] != b'-' {
            p.args.push(a.clone());
            if flags & STOP_AT_NON_OPTION != 0 {
                p.args.extend(args[i..].iter().cloned());
                return Ok(p);
            }
            continue;
        }
        if a == b"-h" && !specs.iter().any(|s| s.short == Some(b'h')) {
            return Err(usage_help(usage));
        }
        if a.starts_with(b"--") {
            let body = String::from_utf8_lossy(&a[2..]).into_owned();
            if body == "help-all" || (body == "help" && !specs.iter().any(|s| s.long == Some("help"))) {
                return Err(usage_help(usage));
            }
            let (name, val) = match body.find('=') {
                Some(eq) => (body[..eq].to_string(), Some(a[2 + eq + 1..].to_vec())),
                None => (body.clone(), None),
            };
            let Some((spec, negated)) = find_long(specs, &name, usage, &name)? else {
                if flags & KEEP_UNKNOWN != 0 {
                    p.unknown.push(a.clone());
                    continue;
                }
                return Err(usage_error(usage, &format!("unknown option `{name}'")));
            };
            let lname = spec.long.unwrap_or("");
            if negated {
                if val.is_some() {
                    return Err(usage_error(usage, &format!("option `no-{lname}' takes no value")));
                }
                p.hits.push(Hit { id: spec.id, value: None, negated: true });
                continue;
            }
            match spec.arg {
                Arg::None => {
                    if val.is_some() {
                        return Err(usage_error(usage, &format!("option `{lname}' takes no value")));
                    }
                    p.hits.push(Hit { id: spec.id, value: None, negated: false });
                }
                Arg::Optional => p.hits.push(Hit { id: spec.id, value: val, negated: false }),
                Arg::Required => {
                    let v = match val {
                        Some(v) => v,
                        None => {
                            if i >= args.len() {
                                return Err(usage_error(usage, &format!("option `{lname}' requires a value")));
                            }
                            i += 1;
                            args[i - 1].clone()
                        }
                    };
                    p.hits.push(Hit { id: spec.id, value: Some(v), negated: false });
                }
            }
            continue;
        }
        // Curtas agrupadas.
        if flags & NUMBER != 0 && a[1..].iter().all(|c| c.is_ascii_digit()) {
            p.hits.push(Hit { id: "number", value: Some(a[1..].to_vec()), negated: false });
            continue;
        }
        let mut k = 1;
        while k < a.len() {
            let c = a[k];
            k += 1;
            let Some(spec) = specs.iter().find(|s| s.short == Some(c)) else {
                if flags & NUMBER != 0 && c.is_ascii_digit() {
                    let start = k - 1;
                    while k < a.len() && a[k].is_ascii_digit() {
                        k += 1;
                    }
                    p.hits.push(Hit { id: "number", value: Some(a[start..k].to_vec()), negated: false });
                    continue;
                }
                if flags & KEEP_UNKNOWN != 0 {
                    p.unknown.push(a.clone());
                    break;
                }
                return Err(usage_error(usage, &format!("unknown switch `{}'", c as char)));
            };
            match spec.arg {
                Arg::None => p.hits.push(Hit { id: spec.id, value: None, negated: false }),
                Arg::Optional => {
                    let v = if k < a.len() { Some(a[k..].to_vec()) } else { None };
                    p.hits.push(Hit { id: spec.id, value: v, negated: false });
                    break;
                }
                Arg::Required => {
                    let v = if k < a.len() {
                        a[k..].to_vec()
                    } else {
                        if i >= args.len() {
                            return Err(usage_error(usage, &format!("switch `{}' requires a value", c as char)));
                        }
                        i += 1;
                        args[i - 1].clone()
                    };
                    p.hits.push(Hit { id: spec.id, value: Some(v), negated: false });
                    break;
                }
            }
        }
    }
    Ok(p)
}

/// Valor inteiro de opção, com o erro do git.
pub fn int_value(p: &Parsed, id: &str, name: &str, usage: &str) -> R<Option<i64>> {
    match p.value(id) {
        None => Ok(None),
        Some(v) => match std::str::from_utf8(v).ok().and_then(|s| s.parse::<i64>().ok()) {
            Some(n) => Ok(Some(n)),
            None => Err(usage_error(usage, &format!("{name} expects a numerical value"))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPECS: &[Spec] = &[
        short_flag(b'a', "all"),
        flag(Some(b'q'), "quiet", "quiet"),
        value(Some(b'm'), "message", "message"),
        flag(None, "amend", "amend"),
        flag(None, "allow-empty", "allow-empty"),
        flag(None, "no-verify", "no-verify"),
    ];

    fn v(s: &[&str]) -> Vec<Vec<u8>> {
        s.iter().map(|x| x.as_bytes().to_vec()).collect()
    }

    #[test]
    fn bundles_and_longs() {
        let p = parse(SPECS, &v(&["-qam", "msg", "x", "--amen", "--", "-q"]), 0, "").ok().unwrap();
        assert!(p.has("quiet") && p.has("all") && p.has("amend"));
        assert_eq!(p.value("message"), Some(&b"msg"[..]));
        assert_eq!(p.args, v(&["x", "-q"]));
        assert_eq!(p.dashdash, Some(1));
        let p = parse(SPECS, &v(&["--message=a b", "--no-quiet", "--verify"]), 0, "").ok().unwrap();
        assert_eq!(p.value("message"), Some(&b"a b"[..]));
        assert_eq!(p.flag("quiet"), Some(false));
        assert_eq!(p.flag("no-verify"), Some(false));
    }
}
