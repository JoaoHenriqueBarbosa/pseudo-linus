//! `git config`: a forma antiga (`--get`, `--list`, `--unset`...) e os subcomandos novos (`get`,
//! `set`, `unset`, `list`, `rename-section`, `remove-section`).

use super::Git;
use crate::config::{self, ConfigFile, Edit, Editor, Scope};
use crate::error::{Fail, R, error};
use crate::opts::{self, Spec};
use crate::os;
use crate::repo;

const SPECS: &[Spec] = &[
    opts::flag(None, "global", "global"),
    opts::flag(None, "system", "system"),
    opts::flag(None, "local", "local"),
    opts::flag(None, "worktree", "worktree"),
    opts::value(Some(b'f'), "file", "file"),
    opts::value(None, "blob", "blob"),
    opts::flag(None, "get", "get"),
    opts::flag(None, "get-all", "get-all"),
    opts::flag(None, "get-regexp", "get-regexp"),
    opts::flag(None, "get-urlmatch", "get-urlmatch"),
    opts::flag(None, "replace-all", "replace-all"),
    opts::flag(None, "add", "add"),
    opts::flag(None, "unset", "unset"),
    opts::flag(None, "unset-all", "unset-all"),
    opts::flag(None, "rename-section", "rename-section"),
    opts::flag(None, "remove-section", "remove-section"),
    opts::flag(Some(b'l'), "list", "list"),
    opts::flag(Some(b'e'), "edit", "edit"),
    opts::flag(None, "get-color", "get-color"),
    opts::flag(None, "get-colorbool", "get-colorbool"),
    opts::value(Some(b't'), "type", "type"),
    opts::flag(None, "bool", "bool"),
    opts::flag(None, "int", "int"),
    opts::flag(None, "bool-or-int", "bool-or-int"),
    opts::flag(None, "bool-or-str", "bool-or-str"),
    opts::flag(None, "path", "path"),
    opts::flag(None, "expiry-date", "expiry-date"),
    opts::flag(None, "no-type", "no-type"),
    opts::flag(Some(b'z'), "null", "null"),
    opts::flag(None, "name-only", "name-only"),
    opts::flag(None, "includes", "includes"),
    opts::flag(None, "show-origin", "show-origin"),
    opts::flag(None, "show-scope", "show-scope"),
    opts::value(None, "default", "default"),
    opts::flag(None, "all", "all"),
    opts::flag(None, "regexp", "regexp"),
    opts::value(None, "value", "value"),
    opts::flag(None, "fixed-value", "fixed-value"),
    opts::flag(None, "append", "append"),
    opts::value(None, "comment", "comment"),
    opts::value(None, "url", "url"),
];

#[derive(Copy, Clone, PartialEq, Eq)]
enum Type {
    None,
    Bool,
    Int,
    BoolOrInt,
    BoolOrStr,
    Path,
}

fn canonical_value(key: &str, v: Option<&[u8]>, t: Type) -> R<Vec<u8>> {
    let bad = |kind: &str| {
        let shown = os::lossy(v.unwrap_or_default());
        match kind {
            "bool" => Fail::Fatal(format!("bad boolean config value '{shown}' for '{key}'")),
            _ => Fail::Fatal(format!("bad numeric config value '{shown}' for '{key}': invalid unit")),
        }
    };
    Ok(match t {
        Type::None => v.unwrap_or_default().to_vec(),
        Type::Bool => match config::parse_bool(v) {
            Some(b) => (if b { "true" } else { "false" }).as_bytes().to_vec(),
            None => return Err(bad("bool")),
        },
        Type::Int => match v.and_then(config::parse_int) {
            Some(n) => n.to_string().into_bytes(),
            None => return Err(bad("int")),
        },
        Type::BoolOrInt => match v.and_then(config::parse_int) {
            Some(n) => n.to_string().into_bytes(),
            None => match config::parse_bool(v) {
                Some(b) => (if b { "true" } else { "false" }).as_bytes().to_vec(),
                None => return Err(bad("bool")),
            },
        },
        Type::BoolOrStr => match config::parse_bool(v) {
            Some(b) if v.is_none_or(|x| config::parse_int(x).is_none()) => (if b { "true" } else { "false" }).as_bytes().to_vec(),
            _ => v.unwrap_or_default().to_vec(),
        },
        Type::Path => crate::ignore::expand_user(v.unwrap_or_default()),
    })
}

/// Arquivo que uma escrita afeta e como ele aparece em mensagens.
fn target_file(git: &Git, p: &opts::Parsed) -> R<(Vec<u8>, Scope)> {
    if let Some(f) = p.value("file") {
        let path = if f.starts_with(b"/") || git.repo.is_none() { f.to_vec() } else {
            let r = git.repo()?;
            if r.prefix.is_empty() { f.to_vec() } else { os::join(&r.prefix, f) }
        };
        return Ok((path, Scope::Local));
    }
    if p.has("global") {
        let path = repo::global_write_path().ok_or_else(|| Fail::Fatal("$HOME not set".into()))?;
        return Ok((path, Scope::Global));
    }
    if p.has("system") {
        let path = os::getenv("GIT_CONFIG_SYSTEM").unwrap_or_else(|| b"/etc/gitconfig".to_vec());
        return Ok((path, Scope::System));
    }
    let Some(r) = &git.repo else {
        if p.has("local") || p.has("worktree") {
            return Err(Fail::Fatal(format!("--{} can only be used inside a git repository", if p.has("local") { "local" } else { "worktree" })));
        }
        return Err(Fail::Fatal("not in a git directory".into()));
    };
    if p.has("worktree") {
        return Ok((r.path("config.worktree"), Scope::Worktree));
    }
    Ok((r.common("config"), Scope::Local))
}

fn load_target(path: &[u8], scope: Scope) -> R<ConfigFile> {
    match ConfigFile::load(path, path.to_vec(), scope)? {
        Some(f) => Ok(f),
        None => Ok(ConfigFile { path: path.to_vec(), scope, data: Vec::new(), entries: Vec::new(), sections: Vec::new(), command_line: false }),
    }
}

fn save(path: &[u8], data: &[u8]) -> R<()> {
    if let Some(parent) = path.iter().rposition(|c| *c == b'/').map(|i| &path[..i])
        && !parent.is_empty()
    {
        let _ = os::mkdir_p(parent, 0o777);
    }
    match os::write_locked(path, data) {
        Ok(()) => Ok(()),
        Err(e) => {
            error(&e);
            Err(Fail::Exit(4))
        }
    }
}

/// Os arquivos que uma leitura enxerga (com `--global`/`--local`/`--file` restringindo).
fn read_set(git: &Git, p: &opts::Parsed) -> R<Vec<ConfigFile>> {
    if p.value("file").is_some() || p.has("global") || p.has("system") || p.has("local") || p.has("worktree") {
        let (path, scope) = target_file(git, p)?;
        let shown = if scope == Scope::Local && p.value("file").is_none() {
            git.repo.as_ref().map(|r| os::join(&r.git_dir_display, b"config")).unwrap_or(path.clone())
        } else {
            path.clone()
        };
        return Ok(match ConfigFile::load(&path, shown, scope)? {
            Some(f) => vec![f],
            None => {
                if p.value("file").is_some() || p.has("global") {
                    // Arquivo explícito ausente: listar dá erro, ler dá "não achou".
                    Vec::new()
                } else {
                    Vec::new()
                }
            }
        });
    }
    Ok(git.config().files.clone())
}

fn origin(f: &ConfigFile) -> String {
    if f.command_line {
        "command line:".to_string()
    } else {
        format!("file:{}", os::lossy(&f.path))
    }
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    // Subcomandos novos.
    let first = args.first().map(|a| os::lossy(a)).unwrap_or_default();
    let (sub, rest): (Option<&str>, &[Vec<u8>]) = match first.as_str() {
        "get" | "set" | "unset" | "list" | "rename-section" | "remove-section" | "edit" => (Some(match first.as_str() {
            "get" => "get",
            "set" => "set",
            "unset" => "unset",
            "list" => "list",
            "rename-section" => "rename-section",
            "remove-section" => "remove-section",
            _ => "edit",
        }), &args[1..]),
        _ => (None, args),
    };
    let p = opts::parse(SPECS, rest, 0, usage)?;
    let mut t = Type::None;
    if let Some(v) = p.value_str("type") {
        t = match v.as_str() {
            "bool" => Type::Bool,
            "int" => Type::Int,
            "bool-or-int" => Type::BoolOrInt,
            "bool-or-str" => Type::BoolOrStr,
            "path" => Type::Path,
            "expiry-date" | "color" => Type::None,
            other => return Err(Fail::Fatal(format!("unrecognized --type argument, {other}"))),
        };
    }
    for (flag, ty) in [("bool", Type::Bool), ("int", Type::Int), ("bool-or-int", Type::BoolOrInt), ("bool-or-str", Type::BoolOrStr), ("path", Type::Path)] {
        if p.has(flag) {
            t = ty;
        }
    }
    if p.has("no-type") {
        t = Type::None;
    }
    let z = p.has("null");
    let a = &p.args;
    let action = match sub {
        Some("get") => {
            if p.has("all") {
                "get-all"
            } else if p.has("regexp") {
                "get-regexp"
            } else {
                "get"
            }
        }
        Some("set") => {
            if p.has("append") {
                "add"
            } else if p.has("all") {
                "replace-all"
            } else {
                "set"
            }
        }
        Some("unset") => {
            if p.has("all") {
                "unset-all"
            } else {
                "unset"
            }
        }
        Some(s) => s,
        None => {
            let explicit = ["get", "get-all", "get-regexp", "replace-all", "add", "unset", "unset-all", "rename-section", "remove-section", "list", "edit"];
            match explicit.iter().find(|x| p.has(x)) {
                Some(x) => x,
                None => match a.len() {
                    0 => {
                        error("no action specified");
                        return Ok(129);
                    }
                    1 => "get",
                    _ => "set",
                },
            }
        }
    };
    match action {
        "list" => {
            let files = read_set(git, &p)?;
            if files.is_empty() && (p.value("file").is_some() || p.has("global")) {
                let (path, _) = target_file(git, &p)?;
                return Err(Fail::Fatal(format!("unable to read config file '{}': No such file or directory", os::lossy(&path))));
            }
            let mut out = Vec::new();
            for f in &files {
                for e in &f.entries {
                    if p.has("show-scope") {
                        out.extend_from_slice(f.scope.name().as_bytes());
                        out.push(b'\t');
                    }
                    if p.has("show-origin") {
                        out.extend_from_slice(origin(f).as_bytes());
                        out.push(if z { 0 } else { b'\t' });
                    }
                    out.extend_from_slice(e.key.as_bytes());
                    if !p.has("name-only") {
                        if let Some(v) = &e.value {
                            out.push(if z { b'\n' } else { b'=' });
                            out.extend_from_slice(&canonical_value(&e.key, Some(v), t)?);
                        }
                    }
                    out.push(if z { 0 } else { b'\n' });
                }
            }
            os::out(&out);
            Ok(0)
        }
        "get" | "get-all" | "get-regexp" => {
            if a.is_empty() || a.len() > 2 {
                return Err(opts::usage_error(usage, "wrong number of arguments, should be from 1 to 2"));
            }
            let key_arg = os::lossy(&a[0]);
            let value_re = match a.get(1).or(p.value("value").map(|v| v).map(|_| &a[0]).filter(|_| false)) {
                Some(v) => Some(regex::bytes::Regex::new(&os::lossy(v)).map_err(|_| Fail::Exit(6))?),
                None => p.value("value").map(|v| regex::bytes::Regex::new(&os::lossy(v))).transpose().map_err(|_| Fail::Exit(6))?,
            };
            let files = read_set(git, &p)?;
            let mut hits: Vec<(String, Option<Vec<u8>>, &ConfigFile)> = Vec::new();
            if action == "get-regexp" {
                let re = regex::Regex::new(&key_arg).map_err(|e| {
                    error(&format!("invalid key pattern: {key_arg}"));
                    let _ = e;
                    Fail::Exit(6)
                })?;
                for f in &files {
                    for e in &f.entries {
                        if re.is_match(&e.key) && value_re.as_ref().is_none_or(|r| r.is_match(e.value.as_deref().unwrap_or_default())) {
                            hits.push((e.key.clone(), e.value.clone(), f));
                        }
                    }
                }
            } else {
                let parts = match config::parse_key(&key_arg) {
                    Ok(k) => k,
                    Err(e) => {
                        error(&e);
                        return Ok(if e.contains("section") || e.contains("variable name") { 1 } else { 1 });
                    }
                };
                let canon = parts.canonical();
                for f in &files {
                    for e in &f.entries {
                        if e.key == canon && value_re.as_ref().is_none_or(|r| r.is_match(e.value.as_deref().unwrap_or_default())) {
                            hits.push((e.key.clone(), e.value.clone(), f));
                        }
                    }
                }
            }
            if hits.is_empty() {
                if let Some(d) = p.value("default") {
                    let v = canonical_value(&key_arg, Some(d), t)?;
                    let mut out = v;
                    out.push(if z { 0 } else { b'\n' });
                    os::out(&out);
                    return Ok(0);
                }
                return Ok(1);
            }
            let selected: Vec<&(String, Option<Vec<u8>>, &ConfigFile)> = if action == "get" { vec![hits.last().expect("não vazio")] } else { hits.iter().collect() };
            let mut out = Vec::new();
            for (k, v, f) in selected {
                if p.has("show-scope") {
                    out.extend_from_slice(f.scope.name().as_bytes());
                    out.push(b'\t');
                }
                if p.has("show-origin") {
                    out.extend_from_slice(origin(f).as_bytes());
                    out.push(if z { 0 } else { b'\t' });
                }
                if action == "get-regexp" {
                    out.extend_from_slice(k.as_bytes());
                    if p.has("name-only") {
                        out.push(if z { 0 } else { b'\n' });
                        continue;
                    }
                    if v.is_some() {
                        out.push(if z { b'\n' } else { b' ' });
                    }
                }
                if action != "get-regexp" || v.is_some() {
                    out.extend_from_slice(&canonical_value(k, v.as_deref(), t)?);
                }
                out.push(if z { 0 } else { b'\n' });
            }
            os::out(&out);
            Ok(0)
        }
        "set" | "add" | "replace-all" => {
            if a.len() < 2 || a.len() > 3 {
                if a.len() == 1 && action != "set" {
                    return Err(opts::usage_error(usage, "wrong number of arguments, should be 2"));
                }
                return Err(opts::usage_error(usage, "wrong number of arguments, should be from 2 to 3"));
            }
            let key_arg = os::lossy(&a[0]);
            let parts = match config::parse_key(&key_arg) {
                Ok(k) => k,
                Err(e) => {
                    error(&e);
                    return Ok(if e.starts_with("key does not contain") { 2 } else { 1 });
                }
            };
            let value = if t == Type::None { a[1].clone() } else { canonical_value(&key_arg, Some(&a[1]), t)? };
            let (path, scope) = target_file(git, &p)?;
            let file = load_target(&path, scope)?;
            let filter_re = a.get(2).map(|v| regex::bytes::Regex::new(&os::lossy(v))).transpose().map_err(|_| Fail::Exit(6))?;
            let fixed = p.has("fixed-value");
            let pat = a.get(2).cloned();
            let filter = |v: Option<&[u8]>| -> bool {
                match (&filter_re, &pat) {
                    (Some(_), Some(raw)) if fixed => v.unwrap_or_default() == raw.as_slice(),
                    (Some(r), _) => r.is_match(v.unwrap_or_default()),
                    _ => true,
                }
            };
            let ed = Editor { file: &file };
            let res = ed.set(&parts, Some(&value), action == "add", action == "replace-all", if a.len() == 3 { Some(&filter) } else { None });
            match res {
                Edit::Ok(data) => {
                    save(&path, &data)?;
                    Ok(0)
                }
                Edit::Fail(c) => Ok(c),
            }
        }
        "unset" | "unset-all" => {
            if a.is_empty() || a.len() > 2 {
                return Err(opts::usage_error(usage, "wrong number of arguments, should be from 1 to 2"));
            }
            let key_arg = os::lossy(&a[0]);
            let parts = match config::parse_key(&key_arg) {
                Ok(k) => k,
                Err(e) => {
                    error(&e);
                    return Ok(2);
                }
            };
            let (path, scope) = target_file(git, &p)?;
            let file = load_target(&path, scope)?;
            let filter_re = a.get(1).map(|v| regex::bytes::Regex::new(&os::lossy(v))).transpose().map_err(|_| Fail::Exit(6))?;
            let filter = |v: Option<&[u8]>| filter_re.as_ref().is_none_or(|r| r.is_match(v.unwrap_or_default()));
            match (Editor { file: &file }).unset(&parts, action == "unset-all", if a.len() == 2 { Some(&filter) } else { None }) {
                Edit::Ok(data) => {
                    save(&path, &data)?;
                    Ok(0)
                }
                Edit::Fail(c) => Ok(c),
            }
        }
        "remove-section" | "rename-section" => {
            let need = if action == "remove-section" { 1 } else { 2 };
            if a.len() != need {
                return Err(opts::usage_error(usage, &format!("wrong number of arguments, should be {need}")));
            }
            let (path, scope) = target_file(git, &p)?;
            let file = load_target(&path, scope)?;
            let old = os::lossy(&a[0]);
            let canon_old = match old.split_once('.') {
                Some((s, sub)) => format!("{}.{sub}", s.to_ascii_lowercase()),
                None => old.to_ascii_lowercase(),
            };
            let ed = Editor { file: &file };
            let res = if action == "remove-section" {
                ed.remove_section(&canon_old)
            } else {
                let new = os::lossy(&a[1]);
                let fake_key = format!("{new}.x");
                let parts = config::parse_key(&fake_key).map_err(|_| Fail::Fatal(format!("invalid section name: {new}")))?;
                ed.rename_section(&canon_old, &parts)
            };
            match res {
                Some(data) => {
                    save(&path, &data)?;
                    Ok(0)
                }
                None => Err(Fail::Fatal(format!("no such section: {old}"))),
            }
        }
        "edit" => {
            let (path, _) = target_file(git, &p)?;
            if !os::exists(&path) {
                save(&path, b"")?;
            }
            let cfg = git.config().clone();
            crate::editor::edit_file(&cfg, &path)?;
            Ok(0)
        }
        _ => Err(opts::usage_help(usage)),
    }
}
