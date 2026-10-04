//! `[[ ... ]]` e os testes de arquivo/string compartilhados com o `test`/`[`.

use sysabi::{AccessMode, AtFlags, Fd, FileType, Stat, mode};

use crate::ast::{CondBinOp, CondExpr, Word};
use crate::shell::{Exec, Flow, Shell, sys};
use crate::vars::{Attrs, Value};

fn stat(p: &[u8]) -> Option<Stat> {
    if p.is_empty() {
        return None;
    }
    sys().fstatat(Fd::CWD, p, AtFlags::empty()).ok()
}

fn lstat(p: &[u8]) -> Option<Stat> {
    if p.is_empty() {
        return None;
    }
    sys().fstatat(Fd::CWD, p, AtFlags::SYMLINK_NOFOLLOW).ok()
}

fn mtime(st: &Stat) -> (i64, u32) {
    (st.mtime.sec, st.mtime.nsec)
}

/// Operadores unários de arquivo e string (sem o `-`). Devolve `None` se `op` não é unário.
pub fn unary_test(sh: &mut Shell, op: &str, arg: &[u8]) -> Option<bool> {
    let s = sys();
    Some(match op {
        "a" | "e" => stat(arg).is_some(),
        "f" => stat(arg).is_some_and(|st| st.file_type() == FileType::Regular),
        "d" => stat(arg).is_some_and(|st| st.file_type() == FileType::Directory),
        "b" => stat(arg).is_some_and(|st| st.file_type() == FileType::BlockDevice),
        "c" => stat(arg).is_some_and(|st| st.file_type() == FileType::CharDevice),
        "p" => stat(arg).is_some_and(|st| st.file_type() == FileType::Fifo),
        "S" => stat(arg).is_some_and(|st| st.file_type() == FileType::Socket),
        "h" | "L" => lstat(arg).is_some_and(|st| st.file_type() == FileType::Symlink),
        "s" => stat(arg).is_some_and(|st| st.size > 0),
        "g" => stat(arg).is_some_and(|st| st.mode & mode::S_ISGID != 0),
        "u" => stat(arg).is_some_and(|st| st.mode & mode::S_ISUID != 0),
        "k" => stat(arg).is_some_and(|st| st.mode & mode::S_ISVTX != 0),
        "r" => !arg.is_empty() && s.faccessat(Fd::CWD, arg, AccessMode::R_OK, AtFlags::REMOVEDIR).is_ok(),
        "w" => !arg.is_empty() && s.faccessat(Fd::CWD, arg, AccessMode::W_OK, AtFlags::REMOVEDIR).is_ok(),
        "x" => !arg.is_empty() && s.faccessat(Fd::CWD, arg, AccessMode::X_OK, AtFlags::REMOVEDIR).is_ok(),
        "O" => stat(arg).is_some_and(|st| st.uid == s.geteuid()),
        "G" => stat(arg).is_some_and(|st| st.gid == s.getegid()),
        "N" => stat(arg).is_some_and(|st| mtime(&st) > (st.atime.sec, st.atime.nsec)),
        "t" => match crate::builtins::parse_int(arg) {
            Some(n) if n >= 0 && n <= i32::MAX as i64 => s.isatty(Fd(n as i32)),
            _ => false,
        },
        "z" => arg.is_empty(),
        "n" => !arg.is_empty(),
        "o" => {
            let name = String::from_utf8_lossy(arg);
            sh.opts.get(&name)
        }
        "v" => var_is_set(sh, arg),
        "R" => {
            let name = String::from_utf8_lossy(arg);
            sh.vars.get(&name).is_some_and(|v| v.attrs.has(Attrs::NAMEREF) && v.is_set())
        }
        _ => return None,
    })
}

/// `-v nome` / `-v nome[i]`.
fn var_is_set(sh: &mut Shell, arg: &[u8]) -> bool {
    let s = String::from_utf8_lossy(arg).into_owned();
    if let Some(b) = s.find('[') {
        if !s.ends_with(']') {
            return false;
        }
        let name = &s[..b];
        let key = &s[b + 1..s.len() - 1];
        let real = sh.resolve_nameref(name);
        let Some(var) = sh.vars.get(&real).cloned() else { return false };
        if key == "@" || key == "*" {
            return match &var.value {
                Value::Indexed(m) => !m.is_empty(),
                Value::Assoc(a) => !a.is_empty(),
                Value::Scalar(_) => true,
                Value::Unset => false,
            };
        }
        return match &var.value {
            Value::Assoc(a) => {
                let k = crate::word::make_word(key, crate::word::WordOpts::mode(crate::word::Mode::Subscript, sh.lineno))
                    .ok()
                    .and_then(|w| sh.expand_word_string(&w).ok())
                    .unwrap_or_default();
                a.contains(&k)
            }
            Value::Indexed(m) => match sh.arith_eval(key.as_bytes()) {
                Ok(i) => sh.resolve_index(&real, i).is_some_and(|i| m.contains_key(&i)),
                Err(_) => false,
            },
            Value::Scalar(_) => matches!(sh.arith_eval(key.as_bytes()), Ok(0)),
            Value::Unset => false,
        };
    }
    match sh.lookup(&s) {
        Some(v) => match &v.value {
            Value::Indexed(m) => m.contains_key(&0),
            Value::Assoc(a) => a.contains(b"0"),
            Value::Scalar(_) => true,
            Value::Unset => false,
        },
        None => false,
    }
}

/// `-nt`, `-ot`, `-ef`.
pub fn file_compare(op: &str, a: &[u8], b: &[u8]) -> Option<bool> {
    let (sa, sb) = (stat(a), stat(b));
    Some(match op {
        "nt" => match (sa, sb) {
            (Some(x), Some(y)) => mtime(&x) > mtime(&y),
            (Some(_), None) => true,
            _ => false,
        },
        "ot" => match (sa, sb) {
            (Some(x), Some(y)) => mtime(&x) < mtime(&y),
            (None, Some(_)) => true,
            _ => false,
        },
        "ef" => match (sa, sb) {
            (Some(x), Some(y)) => x.dev == y.dev && x.ino == y.ino,
            _ => false,
        },
        _ => return None,
    })
}

/// Executa `[[ expr ]]`: 0 verdadeiro, 1 falso, 2 erro.
pub fn eval_cond_command(sh: &mut Shell, e: &CondExpr) -> Exec {
    match eval(sh, e) {
        Ok(true) => Ok(0),
        Ok(false) => Ok(1),
        Err(CondErr::Status(n)) => Ok(n),
        Err(CondErr::Flow(f)) => Err(f),
    }
}

enum CondErr {
    Status(i32),
    Flow(Flow),
}

impl From<Flow> for CondErr {
    fn from(f: Flow) -> Self {
        CondErr::Flow(f)
    }
}

fn trace(sh: &mut Shell, parts: &[Vec<u8>]) {
    if sh.opts.get("xtrace") {
        let mut words = vec![b"[[".to_vec()];
        words.extend(parts.iter().cloned());
        words.push(b"]]".to_vec());
        sh.xtrace_line(&words);
    }
}

fn eval(sh: &mut Shell, e: &CondExpr) -> Result<bool, CondErr> {
    match e {
        CondExpr::And(a, b) => Ok(eval(sh, a)? && eval(sh, b)?),
        CondExpr::Or(a, b) => Ok(eval(sh, a)? || eval(sh, b)?),
        CondExpr::Not(a) => Ok(!eval(sh, a)?),
        CondExpr::Group(a) => eval(sh, a),
        CondExpr::Word(w) => {
            let v = sh.expand_word_string(w)?;
            trace(sh, &[crate::quote::xtrace_word(&v)]);
            Ok(!v.is_empty())
        }
        CondExpr::Unary(op, w) => {
            let v = sh.expand_word_string(w)?;
            trace(sh, &[format!("-{op}").into_bytes(), crate::quote::xtrace_word(&v)]);
            Ok(unary_test(sh, op, &v).unwrap_or(false))
        }
        CondExpr::Binary(op, l, r) => binary(sh, *op, l, r),
    }
}

fn op_text(op: CondBinOp) -> &'static str {
    match op {
        CondBinOp::Match => "==",
        CondBinOp::NoMatch => "!=",
        CondBinOp::Regex => "=~",
        CondBinOp::Less => "<",
        CondBinOp::Greater => ">",
        CondBinOp::Eq => "-eq",
        CondBinOp::Ne => "-ne",
        CondBinOp::Lt => "-lt",
        CondBinOp::Le => "-le",
        CondBinOp::Gt => "-gt",
        CondBinOp::Ge => "-ge",
        CondBinOp::Newer => "-nt",
        CondBinOp::Older => "-ot",
        CondBinOp::SameFile => "-ef",
    }
}

fn binary(sh: &mut Shell, op: CondBinOp, l: &Word, r: &Word) -> Result<bool, CondErr> {
    let lv = sh.expand_word_string(l)?;
    match op {
        CondBinOp::Match | CondBinOp::NoMatch => {
            let pat = sh.expand_word_pattern(r)?;
            trace(sh, &[crate::quote::xtrace_word(&lv), op_text(op).as_bytes().to_vec(), pat.clone()]);
            let mut opts = sh.match_opts(false);
            // Dentro de [[ ]] o casamento usa extglob sempre.
            opts.extglob = true;
            let m = crate::pattern::Pattern::new(&pat, opts).matches(&lv);
            Ok(if op == CondBinOp::Match { m } else { !m })
        }
        CondBinOp::Regex => {
            let re = sh.expand_word_regex(r)?;
            trace(sh, &[crate::quote::xtrace_word(&lv), b"=~".to_vec(), re.clone()]);
            regex_match(sh, &lv, &re)
        }
        _ => {
            let rv = sh.expand_word_string(r)?;
            trace(sh, &[crate::quote::xtrace_word(&lv), op_text(op).as_bytes().to_vec(), crate::quote::xtrace_word(&rv)]);
            match op {
                CondBinOp::Less => Ok(lv < rv),
                CondBinOp::Greater => Ok(lv > rv),
                CondBinOp::Newer => Ok(file_compare("nt", &lv, &rv).unwrap_or(false)),
                CondBinOp::Older => Ok(file_compare("ot", &lv, &rv).unwrap_or(false)),
                CondBinOp::SameFile => Ok(file_compare("ef", &lv, &rv).unwrap_or(false)),
                _ => {
                    let a = sh.arith_eval(&lv)?;
                    let b = sh.arith_eval(&rv)?;
                    Ok(match op {
                        CondBinOp::Eq => a == b,
                        CondBinOp::Ne => a != b,
                        CondBinOp::Lt => a < b,
                        CondBinOp::Le => a <= b,
                        CondBinOp::Gt => a > b,
                        _ => a >= b,
                    })
                }
            }
        }
    }
}

/// `=~` com ERE (regcomp REG_EXTENDED) e `BASH_REMATCH`.
fn regex_match(sh: &mut Shell, text: &[u8], re: &[u8]) -> Result<bool, CondErr> {
    let builder = regex_posix::RegexBuilder::new(regex_posix::Syntax::POSIX_EXTENDED).icase(sh.opts.shopt("nocasematch"));
    let regex = match builder.build(re) {
        Ok(r) => r,
        Err(_) => return Err(CondErr::Status(2)),
    };
    let caps = regex.captures(text);
    let items: Vec<(Option<Vec<u8>>, bool, Vec<u8>)> = match &caps {
        Some(c) => c.iter().map(|m| (None, false, m.map(|m| text[m.range()].to_vec()).unwrap_or_default())).collect(),
        None => Vec::new(),
    };
    // BASH_REMATCH é reescrito a cada `=~`.
    let v = sh.vars.global_entry("BASH_REMATCH");
    v.value = Value::Indexed(Default::default());
    v.attrs = Attrs::INDEXED;
    if !items.is_empty() {
        sh.assign_array("BASH_REMATCH", &items, false)?;
    }
    Ok(caps.is_some())
}
