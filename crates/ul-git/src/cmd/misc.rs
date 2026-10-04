//! `git version`, `git var`, `git help`, `git reflog` e aliases de shell.

use super::Git;
use crate::error::{Fail, R};
use crate::ident::{self, Who};
use crate::os;
use crate::repo::Repo;

pub const VERSION: &str = "2.47.3";

pub fn print_version(args: &[Vec<u8>]) -> R<i32> {
    let mut out = format!("git version {VERSION}\n");
    if args.iter().any(|a| a == b"--build-options") {
        out.push_str("cpu: x86_64\nno commit associated with this build\nsizeof-long: 8\nsizeof-size_t: 8\nshell-path: /bin/sh\nlibcurl: 8.14.1\nzlib: 1.3.1\nSHA-1: SHA1_DC\nSHA-256: SHA256_BLK\n");
    }
    os::outs(&out);
    Ok(0)
}

pub fn version(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    for a in args {
        if a != b"--build-options" {
            return Err(crate::opts::usage_error(usage, &format!("unknown option `{}'", os::lossy(a).trim_start_matches('-'))));
        }
    }
    print_version(args)
}

/// Editor como o git escolhe: `GIT_EDITOR`, `core.editor`, `VISUAL`, `EDITOR`, `vi`.
pub fn editor(cfg: &crate::config::Config) -> Option<Vec<u8>> {
    if let Some(e) = os::getenv("GIT_EDITOR") {
        return Some(e);
    }
    if let Some(e) = cfg.get_bytes("core.editor") {
        return Some(e);
    }
    let term = os::getenv("TERM");
    let dumb = term.as_deref().is_none_or(|t| t == b"dumb");
    if !dumb && let Some(v) = os::getenv("VISUAL") {
        return Some(v);
    }
    if let Some(e) = os::getenv("EDITOR") {
        return Some(e);
    }
    if dumb {
        return None;
    }
    Some(b"vi".to_vec())
}

pub fn var(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let cfg = git.config().clone();
    if args.len() != 1 {
        return Err(crate::opts::usage_help(usage));
    }
    let a = os::lossy(&args[0]);
    let value = |name: &str| -> R<Option<String>> {
        Ok(match name {
            "GIT_AUTHOR_IDENT" => Some(os::lossy(&ident::ident(&cfg, Who::Author, true)?.to_bytes())),
            "GIT_COMMITTER_IDENT" => Some(os::lossy(&ident::ident(&cfg, Who::Committer, true)?.to_bytes())),
            "GIT_EDITOR" | "GIT_SEQUENCE_EDITOR" => editor(&cfg).map(|e| os::lossy(&e)),
            "GIT_PAGER" => Some(os::getenv_str("GIT_PAGER").or_else(|| cfg.get("core.pager")).or_else(|| os::getenv_str("PAGER")).unwrap_or_else(|| "less".into())),
            "GIT_DEFAULT_BRANCH" => Some(cfg.get("init.defaultbranch").unwrap_or_else(|| "master".into())),
            "GIT_SHELL_PATH" => Some("/bin/sh".into()),
            "GIT_ATTR_SYSTEM" => Some("/etc/gitattributes".into()),
            "GIT_ATTR_GLOBAL" => os::getenv_str("HOME").map(|h| format!("{h}/.config/git/attributes")),
            "GIT_CONFIG_SYSTEM" => Some("/etc/gitconfig".into()),
            "GIT_CONFIG_GLOBAL" => os::getenv_str("HOME").map(|h| format!("{h}/.gitconfig")),
            _ => return Err(crate::opts::usage_help(usage)),
        })
    };
    if a == "-l" {
        let mut out = String::new();
        for (_, e) in cfg.entries() {
            match &e.value {
                Some(v) => out.push_str(&format!("{}={}\n", e.key, os::lossy(v))),
                None => out.push_str(&format!("{}\n", e.key)),
            }
        }
        for n in ["GIT_COMMITTER_IDENT", "GIT_AUTHOR_IDENT", "GIT_EDITOR", "GIT_SEQUENCE_EDITOR", "GIT_PAGER", "GIT_DEFAULT_BRANCH", "GIT_SHELL_PATH"] {
            if let Ok(Some(v)) = value(n) {
                out.push_str(&format!("{n}={v}\n"));
            }
        }
        os::outs(&out);
        return Ok(0);
    }
    match value(&a)? {
        Some(v) => {
            os::outs(&format!("{v}\n"));
            Ok(0)
        }
        None => Ok(1),
    }
}

pub fn help(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let _ = git;
    let first = args.iter().find(|a| !a.starts_with(b"-"));
    match first {
        None => {
            if args.iter().any(|a| a == b"-a" || a == b"--all") {
                os::outs("See 'git help <command>' to read about a specific subcommand\n");
                return Ok(0);
            }
            os::outs(crate::usage::GIT);
            Ok(0)
        }
        Some(cmd) => {
            // Sem man no sandbox: o git real cai neste erro quando não acha visualizador.
            let c = os::lossy(cmd);
            os::errs("warning: failed to exec 'man': No such file or directory\n");
            Err(Fail::Fatal(format!("no man viewer handled the request for 'git-{c}'")))
        }
    }
}

/// Alias com `!`: roda `sh -c '<comando> "$@"' <comando> args...` na raiz da árvore de trabalho.
pub fn run_shell_alias(name: &str, cmd: &[u8], args: &[Vec<u8>], repo: Option<&Repo>) -> R<i32> {
    let mut script = cmd.to_vec();
    if !args.is_empty() {
        script.extend_from_slice(b" \"$@\"");
    }
    let mut c = sysio::process::Command::new("/bin/sh");
    use std::os::unix::ffi::OsStrExt;
    c.arg("-c").arg(std::ffi::OsStr::from_bytes(&script)).arg(std::ffi::OsStr::from_bytes(cmd));
    for a in args {
        c.arg(std::ffi::OsStr::from_bytes(a));
    }
    if let Some(r) = repo
        && let Some(wt) = &r.work_tree
    {
        c.current_dir(std::ffi::OsStr::from_bytes(wt));
        c.env("GIT_PREFIX", std::ffi::OsStr::from_bytes(&r.prefix));
    }
    os::flush_out();
    match c.status() {
        Ok(st) => Ok(st.code().unwrap_or(128)),
        Err(e) => Err(Fail::Fatal(format!("cannot run {name}: {e}"))),
    }
}

// ---- reflog -----------------------------------------------------------------------------------

pub fn reflog(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let repo = git.repo()?;
    let sub = args.first().map(|a| os::lossy(a)).unwrap_or_default();
    match sub.as_str() {
        "exists" => {
            let name = args.get(1).map(|a| os::lossy(a)).unwrap_or_default();
            let full = repo.dwim_ref_name(&name)?.unwrap_or(name);
            Ok(if repo.has_reflog(&full) { 0 } else { 1 })
        }
        "delete" => {
            let mut code = 0;
            for spec in &args[1..] {
                let s = os::lossy(spec);
                if s.starts_with('-') {
                    continue;
                }
                let Some((base, n)) = s.strip_suffix('}').and_then(|x| x.rsplit_once("@{")) else {
                    crate::error::error(&format!("not a reflog: {s}"));
                    code = 1;
                    continue;
                };
                let full = if base.is_empty() || base == "HEAD" { "HEAD".to_string() } else { repo.dwim_ref_name(base)?.unwrap_or(base.to_string()) };
                let mut log = repo.read_reflog(&full);
                let Ok(n) = n.parse::<usize>() else { continue };
                if n >= log.len() {
                    crate::error::error(&format!("no reflog for '{s}'"));
                    code = 1;
                    continue;
                }
                let idx = log.len() - 1 - n;
                log.remove(idx);
                repo.write_reflog(&full, &log)?;
            }
            Ok(code)
        }
        "expire" => Ok(0),
        _ => {
            // `show` (padrão) é `log -g --abbrev-commit --pretty=oneline`.
            let rest: &[Vec<u8>] = if sub == "show" || sub == "list" { &args[1..] } else { args };
            if sub == "list" {
                let mut out = String::new();
                let mut names = vec![];
                if repo.has_reflog("HEAD") {
                    names.push("HEAD".to_string());
                }
                for (n, _) in repo.list_refs("refs/")? {
                    if repo.has_reflog(&n) {
                        names.push(n);
                    }
                }
                names.sort();
                for n in names {
                    out.push_str(&n);
                    out.push('\n');
                }
                os::outs(&out);
                return Ok(0);
            }
            let mut largs: Vec<Vec<u8>> = vec![b"-g".to_vec(), b"--abbrev-commit".to_vec(), b"--pretty=oneline".to_vec()];
            largs.extend(rest.iter().cloned());
            super::log::run_log(git, &largs)
        }
    }
}
