//! Subcomandos do `git` e o que eles compartilham: a sessão (repositório e configuração), as
//! opções globais, aliases e a sugestão de comando parecido.

pub mod add;
pub mod branch;
pub mod cat_file;
pub mod checkout;
pub mod commit;
pub mod config_cmd;
pub mod diff_cmd;
pub mod for_each_ref;
pub mod init;
pub mod log;
pub mod ls;
pub mod misc;
pub mod mv;
pub mod plumbing;
pub mod reffmt;
pub mod reset;
pub mod restore;
pub mod rev_parse;
pub mod rm;
pub mod status;
pub mod tag;
pub mod unpack;

use crate::config::Config;
use crate::error::{Fail, R};
use crate::os;
use crate::pathspec::{self, Pathspec};
use crate::repo::{self, Globals, Repo};
use crate::usage;

/// O que um comando precisa do ambiente.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Setup {
    /// Nada (o comando cuida: init, clone, version).
    None,
    /// Descobre o repositório se houver (config, hash-object).
    Gently,
    /// Exige repositório.
    Repo,
    /// Exige repositório com árvore de trabalho.
    WorkTree,
}

pub struct Git {
    pub globals: Globals,
    pub repo: Option<Repo>,
    /// Configuração sem repositório (quando não há).
    pub base_config: Config,
    pub cmd: String,
}

impl Git {
    pub fn repo(&self) -> R<&Repo> {
        self.repo.as_ref().ok_or_else(repo::not_a_repository)
    }

    pub fn config(&self) -> &Config {
        match &self.repo {
            Some(r) => &r.config,
            None => &self.base_config,
        }
    }

    pub fn usage(&self) -> &'static str {
        usage::of(&self.cmd)
    }

    /// Pathspec a partir de argumentos do usuário.
    pub fn pathspec(&self, args: &[Vec<u8>]) -> R<Pathspec> {
        let r = self.repo()?;
        let top = r.work_tree.clone().unwrap_or_else(|| r.git_dir.clone());
        let opts = pathspec::Opts {
            literal: self.globals.literal_pathspecs || os::getenv("GIT_LITERAL_PATHSPECS").is_some_and(|v| v == b"1"),
            glob: self.globals.glob_pathspecs || os::getenv("GIT_GLOB_PATHSPECS").is_some_and(|v| v == b"1"),
            noglob: self.globals.noglob_pathspecs || os::getenv("GIT_NOGLOB_PATHSPECS").is_some_and(|v| v == b"1"),
            icase: self.globals.icase_pathspecs || os::getenv("GIT_ICASE_PATHSPECS").is_some_and(|v| v == b"1"),
        };
        Pathspec::parse(args, &r.prefix, &top, &opts)
    }
}

type CmdFn = fn(&mut Git, &[Vec<u8>]) -> R<i32>;

/// Tabela de comandos: nome, preparo e função.
fn lookup(name: &str) -> Option<(Setup, CmdFn)> {
    Some(match name {
        "init" | "init-db" => (Setup::None, init::run),
        "add" | "stage" => (Setup::WorkTree, add::run),
        "commit" => (Setup::WorkTree, commit::run),
        "status" => (Setup::WorkTree, status::run),
        "rm" => (Setup::WorkTree, rm::run),
        "mv" => (Setup::WorkTree, mv::run),
        "for-each-ref" => (Setup::Repo, for_each_ref::run),
        "tag" => (Setup::Repo, tag::run),
        "branch" => (Setup::Repo, branch::run),
        "restore" => (Setup::WorkTree, restore::run),
        "reset" => (Setup::Repo, reset::run),
        "checkout" => (Setup::WorkTree, checkout::run_checkout),
        "switch" => (Setup::WorkTree, checkout::run_switch),
        "log" => (Setup::Repo, log::run_log),
        "show" => (Setup::Repo, log::run_show),
        "whatchanged" => (Setup::Repo, log::run_whatchanged),
        "rev-list" => (Setup::Repo, log::run_rev_list),
        "shortlog" => (Setup::Gently, log::run_shortlog),
        "diff" => (Setup::Gently, diff_cmd::run_diff),
        "diff-index" => (Setup::Repo, diff_cmd::run_diff_index),
        "diff-files" => (Setup::WorkTree, diff_cmd::run_diff_files),
        "diff-tree" => (Setup::Repo, diff_cmd::run_diff_tree),
        "rev-parse" => (Setup::Gently, rev_parse::run),
        "cat-file" => (Setup::Repo, cat_file::run),
        "hash-object" => (Setup::Gently, plumbing::hash_object),
        "write-tree" => (Setup::Repo, plumbing::write_tree),
        "commit-tree" => (Setup::Repo, plumbing::commit_tree),
        "update-ref" => (Setup::Repo, plumbing::update_ref),
        "symbolic-ref" => (Setup::Repo, plumbing::symbolic_ref),
        "show-ref" => (Setup::Repo, plumbing::show_ref),
        "update-index" => (Setup::WorkTree, plumbing::update_index),
        "read-tree" => (Setup::Repo, plumbing::read_tree),
        "merge-base" => (Setup::Repo, plumbing::merge_base),
        "check-ref-format" => (Setup::Gently, plumbing::check_ref_format),
        "check-ignore" => (Setup::WorkTree, plumbing::check_ignore),
        "count-objects" => (Setup::Repo, plumbing::count_objects),
        "ls-files" => (Setup::Repo, ls::ls_files),
        "ls-tree" => (Setup::Repo, ls::ls_tree),
        "config" => (Setup::Gently, config_cmd::run),
        "version" => (Setup::None, misc::version),
        "var" => (Setup::Gently, misc::var),
        "help" => (Setup::None, misc::help),
        "reflog" => (Setup::Repo, misc::reflog),
        _ => return None,
    })
}

/// Todos os comandos do git 2.47.3 do Debian (pra sugestão de nome parecido).
const ALL_COMMANDS: &[&str] = &[
    "add", "am", "annotate", "apply", "archive", "bisect", "blame", "branch", "bugreport", "bundle", "cat-file", "check-attr",
    "check-ignore", "check-mailmap", "check-ref-format", "checkout", "checkout-index", "cherry", "cherry-pick", "clean", "clone",
    "column", "commit", "commit-graph", "commit-tree", "config", "count-objects", "credential", "credential-cache",
    "credential-store", "daemon", "describe", "diagnose", "diff", "diff-files", "diff-index", "diff-tree", "difftool",
    "fast-export", "fast-import", "fetch", "fetch-pack", "filter-branch", "fmt-merge-msg", "for-each-ref", "for-each-repo",
    "format-patch", "fsck", "fsck-objects", "gc", "get-tar-commit-id", "grep", "hash-object", "help", "hook", "http-backend",
    "http-fetch", "http-push", "imap-send", "index-pack", "init", "init-db", "instaweb", "interpret-trailers", "log", "ls-files",
    "ls-remote", "ls-tree", "mailinfo", "mailsplit", "maintenance", "merge", "merge-base", "merge-file", "merge-index",
    "merge-octopus", "merge-one-file", "merge-ours", "merge-recursive", "merge-recursive-ours", "merge-recursive-theirs",
    "merge-resolve", "merge-subtree", "merge-tree", "mergetool", "mktag", "mktree", "multi-pack-index", "mv", "name-rev", "notes",
    "pack-objects", "pack-redundant", "pack-refs", "patch-id", "pickaxe", "prune", "prune-packed", "pull", "push", "quiltimport",
    "range-diff", "read-tree", "rebase", "receive-pack", "reflog", "refs", "remote", "remote-ext", "remote-fd", "remote-ftp",
    "remote-ftps", "remote-http", "remote-https", "repack", "replace", "replay", "request-pull", "rerere", "reset", "restore",
    "rev-list", "rev-parse", "revert", "rm", "send-pack", "shell", "shortlog", "show", "show-branch", "show-index", "show-ref",
    "sparse-checkout", "stage", "stash", "status", "stripspace", "submodule", "subtree", "switch", "symbolic-ref", "tag",
    "unpack-file", "unpack-objects", "update-index", "update-ref", "update-server-info", "upload-archive", "upload-pack", "var",
    "verify-commit", "verify-pack", "verify-tag", "version", "whatchanged", "worktree", "write-tree",
];

const COMMON_COMMANDS: &[&str] = &[
    "add", "bisect", "branch", "clone", "commit", "diff", "fetch", "grep", "init", "log", "merge", "mv", "pull", "push", "rebase",
    "reset", "restore", "rm", "show", "status", "switch", "tag",
];

/// O `levenshtein` do git com pesos (troca, substituição, inserção, remoção).
fn levenshtein(s1: &[u8], s2: &[u8], w: i32, s: i32, a: i32, d: i32) -> i32 {
    let len2 = s2.len();
    let mut row0 = vec![0i32; len2 + 1];
    let mut row1: Vec<i32> = (0..=len2 as i32).map(|j| j * a).collect();
    let mut row2 = vec![0i32; len2 + 1];
    for i in 0..s1.len() {
        row2[0] = (i as i32 + 1) * d;
        for j in 0..len2 {
            row2[j + 1] = row1[j] + s * i32::from(s1[i] != s2[j]);
            if i > 0 && j > 0 && s1[i - 1] == s2[j] && s1[i] == s2[j - 1] && row2[j + 1] > row0[j - 1] + w {
                row2[j + 1] = row0[j - 1] + w;
            }
            if row2[j + 1] > row1[j + 1] + d {
                row2[j + 1] = row1[j + 1] + d;
            }
            if row2[j + 1] > row2[j] + a {
                row2[j + 1] = row2[j] + a;
            }
        }
        std::mem::swap(&mut row0, &mut row1);
        std::mem::swap(&mut row1, &mut row2);
    }
    row1[len2]
}

/// `git: 'x' is not a git command` com a sugestão do `help_unknown_cmd`.
fn unknown_command(cmd: &str, aliases: &[String]) -> i32 {
    let mut names: Vec<String> = ALL_COMMANDS.iter().map(|s| s.to_string()).collect();
    names.extend(aliases.iter().cloned());
    names.sort();
    names.dedup();
    let mut scored: Vec<(i32, String)> = names
        .into_iter()
        .map(|c| {
            if COMMON_COMMANDS.contains(&c.as_str()) && c.starts_with(cmd) {
                (0, c)
            } else {
                (levenshtein(cmd.as_bytes(), c.as_bytes(), 0, 2, 1, 3) + 1, c)
            }
        })
        .collect();
    scored.sort();
    let mut n = scored.iter().take_while(|(l, _)| *l == 0).count();
    let best = if n >= scored.len() {
        8
    } else {
        let b = scored[n].0;
        n += 1;
        while n < scored.len() && scored[n].0 == b {
            n += 1;
        }
        b
    };
    let mut msg = format!("git: '{cmd}' is not a git command. See 'git --help'.\n");
    if best < 7 {
        msg.push_str(if n == 1 { "\nThe most similar command is\n" } else { "\nThe most similar commands are\n" });
        for (_, c) in &scored[..n] {
            msg.push('\t');
            msg.push_str(c);
            msg.push('\n');
        }
    }
    os::errs(&msg);
    1
}

/// Divide o valor de um alias em palavras (o `split_cmdline`: aspas simples e duplas, `\`).
fn split_cmdline(s: &[u8]) -> Result<Vec<Vec<u8>>, &'static str> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    let mut quote: u8 = 0;
    let mut have = false;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        i += 1;
        if quote == 0 && c.is_ascii_whitespace() {
            if have {
                out.push(std::mem::take(&mut cur));
                have = false;
            }
            continue;
        }
        have = true;
        if c == b'\\' && quote != b'\'' {
            if i < s.len() {
                cur.push(s[i]);
                i += 1;
            }
            continue;
        }
        if quote == 0 && (c == b'"' || c == b'\'') {
            quote = c;
            continue;
        }
        if quote != 0 && c == quote {
            quote = 0;
            continue;
        }
        cur.push(c);
    }
    if quote != 0 {
        return Err("unclosed quote");
    }
    if have {
        out.push(cur);
    }
    Ok(out)
}

/// Ponto de entrada (`argv[0]` incluído).
pub fn main(argv: &[Vec<u8>]) -> i32 {
    match run(argv) {
        Ok(c) => c,
        Err(f) => f.report(),
    }
}

fn run(argv: &[Vec<u8>]) -> R<i32> {
    let mut g = Globals::default();
    let mut i = 1;
    while i < argv.len() {
        let a = argv[i].clone();
        if a.first() != Some(&b'-') {
            break;
        }
        let s = os::lossy(&a);
        let take_value = |i: &mut usize, name: &str| -> R<Vec<u8>> {
            *i += 1;
            argv.get(*i).cloned().ok_or_else(|| {
                os::errs(&format!("no directory given for '{name}' option\n \n"));
                os::errs(usage::git_short());
                Fail::Exit(129)
            })
        };
        match s.as_str() {
            "-C" => {
                let p = take_value(&mut i, "-C")?;
                if !p.is_empty()
                    && let Err(e) = os::chdir(&p)
                {
                    return Err(Fail::Fatal(format!("cannot change to '{}': {}", os::lossy(&p), e.message())));
                }
            }
            "-c" => {
                i += 1;
                let Some(kv) = argv.get(i) else {
                    os::errs("error: -c expects a configuration string\n");
                    os::errs(usage::git_short());
                    return Err(Fail::Exit(129));
                };
                let (k, v) = match kv.iter().position(|c| *c == b'=') {
                    Some(eq) => (os::lossy(&kv[..eq]), Some(kv[eq + 1..].to_vec())),
                    None => (os::lossy(kv), None),
                };
                if let Err(e) = crate::config::parse_key(&k) {
                    crate::error::error(&e);
                    return Err(Fail::Fatal("unable to parse command-line config".into()));
                }
                g.config.push((k, v));
            }
            "--no-pager" | "-P" | "-p" | "--paginate" | "--no-replace-objects" | "--no-optional-locks" | "--no-advice" | "--no-lazy-fetch" => {}
            "--bare" => g.bare = true,
            "--literal-pathspecs" => g.literal_pathspecs = true,
            "--glob-pathspecs" => g.glob_pathspecs = true,
            "--noglob-pathspecs" => g.noglob_pathspecs = true,
            "--icase-pathspecs" => g.icase_pathspecs = true,
            "--git-dir" => g.git_dir = Some(take_value(&mut i, "--git-dir")?),
            "--work-tree" => g.work_tree = Some(take_value(&mut i, "--work-tree")?),
            "--namespace" => {
                take_value(&mut i, "--namespace")?;
            }
            "-v" | "--version" => return misc::print_version(&[]),
            "-h" | "--help" => {
                os::outs(usage::GIT);
                return Ok(0);
            }
            "--exec-path" => {
                os::outs("/usr/lib/git-core\n");
                return Ok(0);
            }
            "--html-path" => {
                os::outs("/usr/share/doc/git/html\n");
                return Ok(0);
            }
            "--man-path" => {
                os::outs("/usr/share/man\n");
                return Ok(0);
            }
            "--info-path" => {
                os::outs("/usr/share/info\n");
                return Ok(0);
            }
            _ => {
                if let Some(v) = s.strip_prefix("--git-dir=") {
                    g.git_dir = Some(v.as_bytes().to_vec());
                } else if let Some(v) = s.strip_prefix("--work-tree=") {
                    g.work_tree = Some(v.as_bytes().to_vec());
                } else if s.starts_with("--namespace=") || s.starts_with("--exec-path=") || s.starts_with("--config-env=") {
                } else {
                    os::errs(&format!("unknown option: {s}\n"));
                    os::errs(usage::git_short());
                    return Err(Fail::Exit(129));
                }
            }
        }
        i += 1;
    }
    if i >= argv.len() {
        os::outs(usage::GIT);
        return Ok(1);
    }
    let mut cmd = os::lossy(&argv[i]);
    let mut args: Vec<Vec<u8>> = argv[i + 1..].to_vec();
    // Aliases (com detecção de laço).
    let mut seen: Vec<String> = Vec::new();
    loop {
        if lookup(&cmd).is_some() {
            break;
        }
        let repo = repo::discover(&g).ok().flatten();
        let cfg = match &repo {
            Some(r) => r.config.clone(),
            None => repo::full_config(&g, None, None)?,
        };
        let alias = cfg.get_bytes(&format!("alias.{cmd}"));
        let Some(val) = alias else {
            let aliases: Vec<String> = cfg
                .entries()
                .filter_map(|(_, e)| e.key.strip_prefix("alias.").map(str::to_string))
                .collect();
            return Ok(unknown_command(&cmd, &aliases));
        };
        if let Some(shell) = val.strip_prefix(b"!") {
            return misc::run_shell_alias(&cmd, shell, &args, repo.as_ref());
        }
        if seen.contains(&cmd) {
            seen.push(cmd.clone());
            return Err(Fail::Fatal(format!("alias loop detected: expansion of '{}' does not terminate:{}", seen[0], seen.iter().map(|s| format!("\n  {s}")).collect::<String>())));
        }
        seen.push(cmd.clone());
        let words = split_cmdline(&val).map_err(|e| Fail::Fatal(format!("bad alias.{cmd} string: {e}")))?;
        if words.is_empty() {
            return Err(Fail::Fatal(format!("empty alias for {cmd}")));
        }
        // Opções globais dentro do alias não são tratadas (o git trata algumas).
        cmd = os::lossy(&words[0]);
        let mut na: Vec<Vec<u8>> = words[1..].to_vec();
        na.extend(args);
        args = na;
    }
    let (setup, f) = lookup(&cmd).expect("comando conhecido");
    let mut git = Git { globals: g.clone(), repo: None, base_config: Config::default(), cmd: cmd.clone() };
    match setup {
        Setup::None => {}
        Setup::Gently => {
            git.repo = repo::discover(&g)?;
            if git.repo.is_none() {
                git.base_config = repo::full_config(&g, None, None)?;
            }
        }
        Setup::Repo | Setup::WorkTree => {
            git.repo = repo::discover(&g)?;
            let Some(r) = &git.repo else {
                return Err(repo::not_a_repository());
            };
            if setup == Setup::WorkTree && r.work_tree.is_none() {
                return Err(Fail::Fatal("this operation must be run in a work tree".into()));
            }
        }
    }
    if args.len() == 1 && args[0] == b"-h" && !usage::of(&cmd).is_empty() {
        os::outs(usage::of(&cmd));
        return Ok(129);
    }
    f(&mut git, &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_weights() {
        assert_eq!(levenshtein(b"comit", b"commit", 0, 2, 1, 3), 1);
        assert_eq!(levenshtein(b"stats", b"status", 0, 2, 1, 3), 1);
    }

    /// Escolha de sugestões igual à do oráculo (saída de `git <erro>` medida no git 2.47.3).
    fn suggestions(cmd: &str) -> Vec<String> {
        let mut names: Vec<String> = ALL_COMMANDS.iter().map(|s| s.to_string()).collect();
        names.sort();
        let mut scored: Vec<(i32, String)> = names
            .into_iter()
            .map(|c| {
                if COMMON_COMMANDS.contains(&c.as_str()) && c.starts_with(cmd) {
                    (0, c)
                } else {
                    (levenshtein(cmd.as_bytes(), c.as_bytes(), 0, 2, 1, 3) + 1, c)
                }
            })
            .collect();
        scored.sort();
        let mut n = scored.iter().take_while(|(l, _)| *l == 0).count();
        if n >= scored.len() {
            return Vec::new();
        }
        let best = scored[n].0;
        n += 1;
        while n < scored.len() && scored[n].0 == best {
            n += 1;
        }
        if best >= 7 {
            return Vec::new();
        }
        scored[..n].iter().map(|(_, c)| c.clone()).collect()
    }

    #[test]
    fn typo_suggestions_match_oracle() {
        let table: &[(&str, &[&str])] = &[
            ("stat", &["status", "stage", "stash"]),
            ("sta", &["status", "stage", "stash"]),
            ("stats", &["status"]),
            ("comit", &["commit"]),
            ("cmomit", &["commit"]),
            ("chekcout", &["checkout"]),
            ("chckout", &["checkout"]),
            ("brnach", &["branch"]),
            ("brach", &["branch"]),
            ("lgo", &["log"]),
            ("lo", &["log", "clone"]),
            ("st", &["status", "reset", "stage", "stash"]),
            ("co", &["commit", "clone", "log"]),
            ("ci", &["am", "commit", "config", "diff", "fsck", "gc", "init", "mv", "rm"]),
            ("psuh", &["push"]),
            ("pul", &["pull", "push"]),
            ("fetchh", &["fetch"]),
            ("rebse", &["rebase"]),
            ("rset", &["reset"]),
            ("stsh", &["stash"]),
            ("tg", &["tag"]),
            ("dif", &["diff", "config", "difftool", "init", "refs"]),
            ("difff", &["diff"]),
            ("shwo", &["show"]),
            ("blme", &["blame"]),
            ("mrege", &["merge"]),
            ("merg", &["merge", "grep", "mktree"]),
            ("cherrypick", &["cherry-pick"]),
            ("cherry-pik", &["cherry-pick"]),
            ("wrktree", &["worktree"]),
            ("remot", &["remote"]),
            ("cloen", &["clone"]),
            ("ini", &["init", "init-db"]),
            ("xyzzy", &[]),
            ("reflg", &["reflog"]),
            ("describ", &["describe"]),
            ("cleam", &["clean"]),
            ("tagg", &["stage", "tag"]),
            ("statu", &["status", "stage", "stash"]),
        ];
        for (typo, want) in table {
            let got = suggestions(typo);
            let want: Vec<String> = want.iter().map(|s| s.to_string()).collect();
            assert_eq!(got, want, "{typo}");
        }
    }

    #[test]
    fn alias_words() {
        let w = split_cmdline(b"log --format='%h %s' -n 3").ok().unwrap();
        assert_eq!(w, vec![b"log".to_vec(), b"--format=%h %s".to_vec(), b"-n".to_vec(), b"3".to_vec()]);
    }
}
