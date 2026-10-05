//! `git remote`: lista, `add`, `rename`, `remove`, `set-head`, `set-branches`, `get-url`,
//! `set-url`, `show`, `prune` e `update`. A configuração (`remote.<nome>.*`, `branch.<nome>.*`,
//! `url.<base>.insteadOf`) é lida da configuração do repositório e escrita no arquivo local, e as
//! refs de acompanhamento seguem os refspecs de busca. Consultar o remoto (`show`, `prune`,
//! `set-head -a`) e buscar (`add -f`, `update`) só têm o caminho de falha do transporte: não há
//! rede nem `git fetch` neste git.

use std::collections::BTreeMap;

use super::Git;
use crate::config::{self, ConfigFile, Edit, Editor, Scope};
use crate::error::{Fail, R, error, warning};
use crate::opts::{self, Spec};
use crate::os;
use crate::refs;
use crate::repo::Repo;

const USAGE_ADD: &str = include_str!("../usage/remote-add.txt");
const USAGE_RENAME: &str = include_str!("../usage/remote-rename.txt");
const USAGE_RM: &str = include_str!("../usage/remote-rm.txt");
const USAGE_SET_HEAD: &str = include_str!("../usage/remote-set-head.txt");
const USAGE_SHOW: &str = include_str!("../usage/remote-show.txt");
const USAGE_PRUNE: &str = include_str!("../usage/remote-prune.txt");
const USAGE_UPDATE: &str = include_str!("../usage/remote-update.txt");
const USAGE_SET_BRANCHES: &str = include_str!("../usage/remote-set-branches.txt");
const USAGE_GET_URL: &str = include_str!("../usage/remote-get-url.txt");
const USAGE_SET_URL: &str = include_str!("../usage/remote-set-url.txt");

const TOP_SPECS: &[Spec] = &[opts::flag(Some(b'v'), "verbose", "verbose")];
const ADD_SPECS: &[Spec] = &[
    opts::flag(Some(b'f'), "fetch", "fetch"),
    opts::flag(None, "tags", "tags"),
    opts::value(Some(b't'), "track", "track"),
    opts::value(Some(b'm'), "master", "master"),
    opts::optional(None, "mirror", "mirror"),
];
const RENAME_SPECS: &[Spec] = &[opts::flag(None, "progress", "progress")];
const SET_HEAD_SPECS: &[Spec] = &[opts::flag(Some(b'a'), "auto", "auto"), opts::flag(Some(b'd'), "delete", "delete")];
const SHOW_SPECS: &[Spec] = &[opts::short_flag(b'n', "no-query")];
const PRUNE_SPECS: &[Spec] = &[opts::flag(Some(b'n'), "dry-run", "dry-run")];
const UPDATE_SPECS: &[Spec] = &[opts::flag(Some(b'p'), "prune", "prune")];
const SET_BRANCHES_SPECS: &[Spec] = &[opts::flag(None, "add", "add")];
const GET_URL_SPECS: &[Spec] = &[opts::flag(None, "push", "push"), opts::flag(None, "all", "all")];
const SET_URL_SPECS: &[Spec] = &[opts::flag(None, "push", "push"), opts::flag(None, "add", "add"), opts::flag(None, "delete", "delete")];

const MIRROR_FETCH: u8 = 1;
const MIRROR_PUSH: u8 = 2;

// ---- o estado lido da configuração ------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Remote {
    name: String,
    url: Vec<String>,
    pushurl: Vec<String>,
    /// Refspecs de busca como escritos na configuração.
    fetch: Vec<String>,
    push: Vec<String>,
    mirror: bool,
    /// Alguma variável `remote.<nome>.*` vem do arquivo local (ou do da árvore de trabalho).
    configured_in_repo: bool,
    /// Alguma variável `remote.<nome>.*` existe em qualquer arquivo.
    configured: bool,
    skip_default_update: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
enum Rebase {
    Invalid,
    #[default]
    False,
    True,
    Merges,
    Interactive,
}

#[derive(Clone, Debug, Default)]
struct BranchInfo {
    remote_name: Option<String>,
    /// `branch.<nome>.merge` sem o `refs/heads/`.
    merge: Vec<String>,
    rebase: Rebase,
    push_remote_name: Option<String>,
}

/// `url.<base>.insteadOf` (e `pushInsteadOf`): a base e os prefixos que ela substitui.
type Rewrite = (String, Vec<String>);

struct State {
    remotes: Vec<Remote>,
    branches: BTreeMap<String, BranchInfo>,
    rewrites: Vec<Rewrite>,
    rewrites_push: Vec<Rewrite>,
}

fn abbrev_branch(name: &str) -> &str {
    name.strip_prefix("refs/heads/").unwrap_or(name)
}

fn rebase_value(v: &str) -> Rebase {
    match config::parse_bool(Some(v.as_bytes())) {
        Some(false) => Rebase::False,
        Some(true) => Rebase::True,
        None => match v {
            "merges" | "m" => Rebase::Merges,
            "interactive" | "i" => Rebase::Interactive,
            _ => Rebase::Invalid,
        },
    }
}

fn rewrite_entry<'a>(list: &'a mut Vec<Rewrite>, base: &str) -> &'a mut Rewrite {
    if let Some(i) = list.iter().position(|r| r.0 == base) {
        return &mut list[i];
    }
    list.push((base.to_string(), Vec::new()));
    let last = list.len() - 1;
    &mut list[last]
}

/// `alias_url`: a base do prefixo mais longo que casa com o começo do URL.
fn alias_url(url: &str, list: &[Rewrite]) -> Option<String> {
    let mut best: Option<(&str, usize)> = None;
    for (base, prefixes) in list {
        for p in prefixes {
            if url.starts_with(p.as_str()) && best.is_none_or(|(_, l)| l < p.len()) {
                best = Some((base.as_str(), p.len()));
            }
        }
    }
    best.map(|(base, len)| format!("{base}{}", &url[len..]))
}

fn add_url(list: &mut Vec<String>, v: &str) {
    if v.is_empty() {
        list.clear();
    } else {
        list.push(v.to_string());
    }
}

fn remote_slot<'a>(remotes: &'a mut Vec<Remote>, name: &str) -> &'a mut Remote {
    if let Some(i) = remotes.iter().position(|r| r.name == name) {
        return &mut remotes[i];
    }
    remotes.push(Remote { name: name.to_string(), ..Remote::default() });
    let last = remotes.len() - 1;
    &mut remotes[last]
}

fn load_state(repo: &Repo) -> State {
    let mut st = State { remotes: Vec::new(), branches: BTreeMap::new(), rewrites: Vec::new(), rewrites_push: Vec::new() };
    for (file, e) in repo.config.entries() {
        let value = e.value.as_deref().map(os::lossy);
        if let Some(rest) = e.key.strip_prefix("branch.") {
            let Some(dot) = rest.rfind('.') else { continue };
            let (name, var) = (&rest[..dot], &rest[dot + 1..]);
            if name.is_empty() || !matches!(var, "remote" | "merge" | "rebase" | "pushremote") {
                continue;
            }
            let Some(v) = value else { continue };
            let info = st.branches.entry(name.to_string()).or_default();
            match var {
                "remote" => {
                    if info.remote_name.is_some() {
                        warning(&format!("more than one {}", e.key));
                    }
                    info.remote_name = Some(v);
                }
                "merge" => {
                    // Vários ramos separados por espaço, cada um sem o `refs/heads/`.
                    let mut rest = v.as_str();
                    rest = abbrev_branch(rest);
                    while let Some(sp) = rest.find(' ') {
                        info.merge.push(rest[..sp].to_string());
                        rest = abbrev_branch(&rest[sp + 1..]);
                    }
                    info.merge.push(rest.to_string());
                }
                "rebase" => {
                    info.rebase = rebase_value(&v);
                    if info.rebase == Rebase::Invalid {
                        warning(&format!("unhandled branch.{name}.rebase={v}; assuming 'true'"));
                    }
                }
                _ => {
                    if info.push_remote_name.is_some() {
                        warning(&format!("more than one {}", e.key));
                    }
                    info.push_remote_name = Some(v);
                }
            }
            continue;
        }
        if let Some(rest) = e.key.strip_prefix("url.") {
            let Some(dot) = rest.rfind('.') else { continue };
            let (base, var) = (&rest[..dot], &rest[dot + 1..]);
            let Some(v) = value else { continue };
            match var {
                "insteadof" => rewrite_entry(&mut st.rewrites, base).1.push(v),
                "pushinsteadof" => rewrite_entry(&mut st.rewrites_push, base).1.push(v),
                _ => {}
            }
            continue;
        }
        let Some(rest) = e.key.strip_prefix("remote.") else { continue };
        let Some(dot) = rest.rfind('.') else { continue };
        let (name, var) = (&rest[..dot], &rest[dot + 1..]);
        if name.is_empty() || name.starts_with('/') {
            continue;
        }
        let r = remote_slot(&mut st.remotes, name);
        r.configured = true;
        if matches!(file.scope, Scope::Local | Scope::Worktree) {
            r.configured_in_repo = true;
        }
        match var {
            "mirror" => r.mirror = config::parse_bool(e.value.as_deref()).unwrap_or(false),
            "skipdefaultupdate" | "skipfetchall" => r.skip_default_update = config::parse_bool(e.value.as_deref()).unwrap_or(false),
            "url" => {
                if let Some(v) = value {
                    add_url(&mut r.url, &v);
                }
            }
            "pushurl" => {
                if let Some(v) = value {
                    add_url(&mut r.pushurl, &v);
                }
            }
            "push" => {
                if let Some(v) = value {
                    r.push.push(v);
                }
            }
            "fetch" => {
                if let Some(v) = value {
                    r.fetch.push(v);
                }
            }
            _ => {}
        }
    }
    // `alias_all_urls`.
    let (rewrites, rewrites_push) = (st.rewrites.clone(), st.rewrites_push.clone());
    for r in st.remotes.iter_mut() {
        for p in r.pushurl.iter_mut() {
            if let Some(a) = alias_url(p, &rewrites) {
                *p = a;
            }
        }
        let add_push_aliases = r.pushurl.is_empty();
        let mut derived: Vec<String> = Vec::new();
        for u in r.url.iter_mut() {
            if add_push_aliases && let Some(a) = alias_url(u, &rewrites_push) {
                derived.push(a);
            }
            if let Some(a) = alias_url(u, &rewrites) {
                *u = a;
            }
        }
        for a in derived {
            add_url(&mut r.pushurl, &a);
        }
    }
    st
}

impl State {
    /// O `remote_get` com o nome dado: o que a configuração tem, ou o próprio nome como URL.
    fn remote_get(&self, name: &str) -> Remote {
        let mut r = match self.remotes.iter().find(|r| r.name == name) {
            Some(r) => r.clone(),
            None => Remote { name: name.to_string(), ..Remote::default() },
        };
        if r.url.is_empty() {
            let u = alias_url(name, &self.rewrites).unwrap_or_else(|| name.to_string());
            r.url.push(u);
            if let Some(a) = alias_url(name, &self.rewrites_push) {
                add_url(&mut r.pushurl, &a);
            }
        }
        r
    }

    fn is_configured_in_repo(&self, name: &str) -> bool {
        self.remotes.iter().any(|r| r.name == name && r.configured_in_repo)
    }
}

fn push_urls(r: &Remote) -> &Vec<String> {
    if r.pushurl.is_empty() { &r.url } else { &r.pushurl }
}

/// `valid_remote_name`: o nome tem de servir de componente em `refs/remotes/<nome>/...`.
fn valid_remote_name(name: &str) -> bool {
    refs::check_refname_format(&format!("refs/remotes/{name}/test"), false, false)
}

// ---- refspecs de busca ------------------------------------------------------------------------

/// Ref de origem cujo destino é `dst_ref` (o `remote_find_tracking`), olhando os refspecs de busca.
fn find_tracking(r: &Remote, dst_ref: &str) -> Option<String> {
    for spec in &r.fetch {
        if spec.starts_with('^') {
            continue;
        }
        let spec = spec.strip_prefix('+').unwrap_or(spec);
        let Some((src, dst)) = spec.split_once(':') else { continue };
        if dst.is_empty() {
            continue;
        }
        if let Some(star) = dst.find('*') {
            let (dpre, dsuf) = (&dst[..star], &dst[star + 1..]);
            if dst_ref.len() >= dpre.len() + dsuf.len() && dst_ref.starts_with(dpre) && dst_ref.ends_with(dsuf) {
                let mid = &dst_ref[dpre.len()..dst_ref.len() - dsuf.len()];
                if let Some(sstar) = src.find('*') {
                    return Some(format!("{}{mid}{}", &src[..sstar], &src[sstar + 1..]));
                }
                return Some(src.to_string());
            }
        } else if dst == dst_ref {
            return Some(src.to_string());
        }
    }
    None
}

// ---- escrita da configuração local ------------------------------------------------------------

fn load_local(repo: &Repo) -> R<(Vec<u8>, ConfigFile)> {
    let path = repo.common("config");
    let file = match ConfigFile::load(&path, path.clone(), Scope::Local)? {
        Some(f) => f,
        None => ConfigFile { path: path.clone(), scope: Scope::Local, data: Vec::new(), entries: Vec::new(), sections: Vec::new(), command_line: false },
    };
    Ok((path, file))
}

fn save(path: &[u8], data: &[u8]) -> R<()> {
    os::write_locked(path, data).map_err(Fail::Fatal)
}

fn count_matches(file: &ConfigFile, canon: &str, filter: Option<config::ValueFilter<'_>>) -> usize {
    file.entries.iter().filter(|e| e.key == canon && filter.is_none_or(|f| f(e.value.as_deref()))).count()
}

/// `git_config_set_multivar(chave, valor, padrão, 0)`: um valor só. Com vários valores casando o git
/// avisa e morre.
fn cfg_set_with(repo: &Repo, key: &str, value: &str, filter: Option<config::ValueFilter<'_>>) -> R<()> {
    let (path, file) = load_local(repo)?;
    let parts = config::parse_key(key).map_err(Fail::Fatal)?;
    let canon = parts.canonical();
    if count_matches(&file, &canon, filter) > 1 {
        warning(&format!("{canon} has multiple values"));
        return Err(Fail::Fatal(format!("could not set '{key}' to '{value}'")));
    }
    match (Editor { file: &file }).set(&parts, Some(value.as_bytes()), false, false, filter) {
        Edit::Ok(data) => save(&path, &data),
        Edit::Fail(_) => Err(Fail::Fatal(format!("could not set '{key}' to '{value}'"))),
    }
}

/// `git_config_set(chave, valor)`.
fn cfg_set(repo: &Repo, key: &str, value: &str) -> R<()> {
    cfg_set_with(repo, key, value, None)
}

/// `git_config_set_multivar(chave, valor, "^$", 0)`: acrescenta mais um valor (só troca um valor vazio).
fn cfg_add(repo: &Repo, key: &str, value: &str) -> R<()> {
    let empty = |v: Option<&[u8]>| v.is_some_and(|b| b.is_empty());
    cfg_set_with(repo, key, value, Some(&empty))
}

/// `git_config_set_gently(chave, NULL)`: tira o valor. Ausente, ou com vários valores (o aviso sai),
/// não é erro.
fn cfg_unset(repo: &Repo, key: &str) -> R<()> {
    let (path, file) = load_local(repo)?;
    let parts = config::parse_key(key).map_err(Fail::Fatal)?;
    if let Edit::Ok(data) = (Editor { file: &file }).unset(&parts, false, None) {
        save(&path, &data)?;
    }
    Ok(())
}

/// `CONFIG_FLAGS_MULTI_REPLACE` com valor nulo: tira todos os valores (ou os que casam).
fn cfg_unset_all(repo: &Repo, key: &str, filter: Option<config::ValueFilter<'_>>) -> R<()> {
    let (path, file) = load_local(repo)?;
    let parts = config::parse_key(key).map_err(Fail::Fatal)?;
    if let Edit::Ok(data) = (Editor { file: &file }).unset(&parts, true, filter) {
        save(&path, &data)?;
    }
    Ok(())
}

/// `repo_config_rename_section(antigo, NULL)`: tira a seção; `false` se ela não existe.
fn cfg_remove_section(repo: &Repo, name: &str) -> R<bool> {
    let (path, file) = load_local(repo)?;
    match (Editor { file: &file }).remove_section(name) {
        Some(data) => {
            save(&path, &data)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// `repo_config_rename_section(antigo, novo)` para `remote.<antigo>` e `remote.<novo>`.
fn cfg_rename_remote_section(repo: &Repo, old: &str, new: &str) -> R<bool> {
    let (path, file) = load_local(repo)?;
    let parts = config::parse_key(&format!("remote.{new}.x")).map_err(Fail::Fatal)?;
    match (Editor { file: &file }).rename_section(&format!("remote.{old}"), &parts) {
        Some(data) => {
            save(&path, &data)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Compila o padrão (ERE) que o `set-url` usa pra achar a URL antiga.
fn old_url_regex(pattern: &str) -> R<regex::bytes::Regex> {
    crate::re::compile(pattern.as_bytes(), crate::re::Flavor::Extended, false).map_err(|_| Fail::Fatal(format!("Invalid old URL pattern: {pattern}")))
}

// ---- o transporte -----------------------------------------------------------------------------

/// O que dá pra fazer com o URL de um remoto.
enum Reach {
    /// Não alcança: o texto que o transporte deixa no stderr (já com os `fatal:`).
    Unreachable(String),
    /// Repositório local que existe (não há `git fetch` nem consulta de refs neste git).
    Local(String),
}

fn does_not_appear(path: &str) -> String {
    format!(
        "fatal: '{path}' does not appear to be a git repository\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.\n"
    )
}

/// Host (sem usuário nem porta) e porta explícita de um URL com esquema.
fn host_of(rest: &str) -> (String, Option<String>) {
    let authority = rest.split('/').next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    match authority.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()) => (h.to_string(), Some(p.to_string())),
        _ => (authority.to_string(), None),
    }
}

fn is_repo_dir(path: &str) -> bool {
    let p = path.as_bytes();
    let mut cands: Vec<Vec<u8>> = vec![os::join(p, b".git"), p.to_vec()];
    for suffix in [&b".git/.git"[..], &b".git"[..]] {
        let mut c = p.to_vec();
        c.extend_from_slice(suffix);
        cands.push(c);
    }
    cands.iter().any(|c| crate::repo::is_git_directory(c))
}

/// Tenta chegar no URL como o transporte do git: caminho local (ou `file://`), `ssh`, `git://` e
/// `http(s)`. Sem rede e sem ssh, só o caminho local que é repositório "responde".
fn reach(url: &str) -> Reach {
    let (host_part, scheme) = match url.split_once("://") {
        Some((s, r)) if !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.') => (Some(r), s),
        _ => (None, ""),
    };
    match (scheme, host_part) {
        ("file", Some(rest)) => {
            // `file:///caminho` (host vazio) ou `file://host/caminho` (o git só aceita o local).
            let path = if rest.starts_with('/') { rest.to_string() } else { format!("/{}", rest.split_once('/').map(|x| x.1).unwrap_or("")) };
            if is_repo_dir(&path) { Reach::Local(path) } else { Reach::Unreachable(does_not_appear(&path)) }
        }
        ("ssh", Some(_)) | ("git+ssh", Some(_)) | ("ssh+git", Some(_)) => Reach::Unreachable("error: cannot run ssh: No such file or directory\nfatal: unable to fork\n".to_string()),
        ("git", Some(rest)) => {
            let (host, port) = host_of(rest);
            let port = port.unwrap_or_else(|| "9418".to_string());
            Reach::Unreachable(format!("fatal: unable to look up {host} (port {port}) (Name or service not known)\n"))
        }
        ("http", Some(rest)) | ("https", Some(rest)) => {
            let (host, _) = host_of(rest);
            let shown = if url.ends_with('/') { url.to_string() } else { format!("{url}/") };
            Reach::Unreachable(format!("fatal: unable to access '{shown}': Could not resolve host: {host}\n"))
        }
        _ => {
            // `usuario@host:caminho` (sem barra antes dos dois pontos) é ssh.
            if let Some(colon) = url.find(':') {
                let before = &url[..colon];
                if !before.contains('/') && !before.is_empty() {
                    return Reach::Unreachable("error: cannot run ssh: No such file or directory\nfatal: unable to fork\n".to_string());
                }
            }
            if is_repo_dir(url) { Reach::Local(url.to_string()) } else { Reach::Unreachable(does_not_appear(url)) }
        }
    }
}

/// `git fetch <nome>` de um remoto: devolve se deu certo. A falha do transporte sai no stderr.
fn fetch_remote(r: &Remote) -> bool {
    let url = r.url.first().cloned().unwrap_or_else(|| r.name.clone());
    match reach(&url) {
        Reach::Unreachable(text) => {
            os::errs(&text);
            false
        }
        Reach::Local(path) => {
            os::errs(&format!("fatal: fetching from the local repository '{path}' is not supported by this git\n"));
            false
        }
    }
}

/// A consulta das refs do remoto (`ls-remote`) que o `show`, o `prune` e o `set-head -a` fazem: sem
/// rede ela sempre falha, e a falha do transporte vira o `fatal` do git (exit 128).
fn query_failure(r: &Remote) -> Fail {
    let url = r.url.first().cloned().unwrap_or_else(|| r.name.clone());
    match reach(&url) {
        Reach::Unreachable(text) => {
            os::errs(&text);
            Fail::Exit(128)
        }
        Reach::Local(path) => Fail::Fatal(format!("querying the local repository '{path}' is not supported by this git")),
    }
}

// ---- add --------------------------------------------------------------------------------------

fn usage_exit(usage: &str) -> Fail {
    opts::usage_to_stderr(usage);
    Fail::Exit(129)
}

fn add_branch(repo: &Repo, key: &str, branch: &str, remote: &str, mirror: bool) -> R<()> {
    let spec = if mirror { format!("+refs/{branch}:refs/{branch}") } else { format!("+refs/heads/{branch}:refs/remotes/{remote}/{branch}") };
    cfg_add(repo, key, &spec)
}

/// `refs_update_symref`: `name` passa a apontar pra `target`, com a linha de reflog que o git deixa.
fn update_symref(repo: &Repo, name: &str, target: &str, msg: &str) -> R<()> {
    if !refs::check_refname_format(target, true, false) {
        return Err(Fail::Exit(1));
    }
    repo.set_symref(name, target, Some(msg))
}

fn cmd_add(git: &Git, args: &[Vec<u8>]) -> R<i32> {
    let repo = git.repo()?;
    let p = opts::parse(ADD_SPECS, args, 0, USAGE_ADD)?;
    let mut mirror = 0u8;
    for h in p.hits.iter().filter(|h| h.id == "mirror") {
        if h.negated {
            mirror = 0;
            continue;
        }
        match h.value.as_deref() {
            None => {
                warning("--mirror is dangerous and deprecated; please\n\t use --mirror=fetch or --mirror=push instead");
                mirror = MIRROR_FETCH | MIRROR_PUSH;
            }
            Some(b"fetch") => mirror = MIRROR_FETCH,
            Some(b"push") => mirror = MIRROR_PUSH,
            Some(v) => return Err(opts::error_only(&format!("unknown --mirror argument: {}", os::lossy(v)))),
        }
    }
    // 0 = desligado (`--no-tags`), 1 = padrão, 2 = `--tags`.
    let fetch_tags = p.hits.iter().rev().find(|h| h.id == "tags").map_or(1, |h| if h.negated { 0 } else { 2 });
    let fetch = p.has("fetch");
    let track: Vec<String> = p.values("track").iter().map(|v| os::lossy(v)).collect();
    let master = p.value_str("master");
    if p.args.len() != 2 {
        return Err(usage_exit(USAGE_ADD));
    }
    if mirror != 0 && master.is_some() {
        return Err(Fail::Fatal("specifying a master branch makes no sense with --mirror".into()));
    }
    if mirror != 0 && mirror & MIRROR_FETCH == 0 && !track.is_empty() {
        return Err(Fail::Fatal("specifying branches to track makes sense only with fetch mirrors".into()));
    }
    let name = os::lossy(&p.args[0]);
    let url = os::lossy(&p.args[1]);
    let st = load_state(repo);
    if st.is_configured_in_repo(&name) {
        error(&format!("remote {name} already exists."));
        return Err(Fail::Exit(3));
    }
    if !valid_remote_name(&name) {
        return Err(Fail::Fatal(format!("'{name}' is not a valid remote name")));
    }
    cfg_set(repo, &format!("remote.{name}.url"), &url)?;
    if mirror == 0 || mirror & MIRROR_FETCH != 0 {
        let key = format!("remote.{name}.fetch");
        let tracks: Vec<String> = if track.is_empty() { vec!["*".to_string()] } else { track };
        for b in &tracks {
            add_branch(repo, &key, b, &name, mirror != 0)?;
        }
    }
    if mirror & MIRROR_PUSH != 0 {
        cfg_set(repo, &format!("remote.{name}.mirror"), "true")?;
    }
    if fetch_tags != 1 {
        cfg_set(repo, &format!("remote.{name}.tagOpt"), if fetch_tags == 2 { "--tags" } else { "--no-tags" })?;
    }
    if fetch {
        os::outs(&format!("Updating {name}\n"));
        let shown = alias_url(&url, &st.rewrites).unwrap_or(url);
        let remote = Remote { name: name.clone(), url: vec![shown], ..Remote::default() };
        if !fetch_remote(&remote) {
            error(&format!("Could not fetch {name}"));
            return Ok(1);
        }
    }
    if let Some(m) = master {
        let head = format!("refs/remotes/{name}/HEAD");
        let target = format!("refs/remotes/{name}/{m}");
        if update_symref(repo, &head, &target, "remote add").is_err() {
            error(&format!("Could not setup master '{m}'"));
            return Ok(1);
        }
    }
    Ok(0)
}

// ---- remove -----------------------------------------------------------------------------------

/// `handle_push_default`: o `remote.pushDefault` que nomeava o remoto antigo.
fn handle_push_default(repo: &Repo, old: &str, new: Option<&str>) -> R<()> {
    let mut found: Option<(Scope, usize)> = None;
    for (file, e) in repo.config.entries() {
        if e.key == "remote.pushdefault" && e.value.as_deref() == Some(old.as_bytes()) {
            let line = file.data[..e.span.0.min(file.data.len())].iter().filter(|c| **c == b'\n').count() + 1;
            found = Some((file.scope, line));
        }
    }
    let Some((scope, line)) = found else { return Ok(()) };
    if scope >= Scope::Command {
        return Ok(());
    }
    if scope >= Scope::Local {
        match new {
            Some(n) => cfg_set(repo, "remote.pushDefault", n)?,
            None => cfg_unset(repo, "remote.pushDefault")?,
        }
    } else {
        warning(&format!(
            "The {} configuration remote.pushDefault in:\n\tfile:{line}\nnow names the non-existent remote '{old}'",
            scope.name()
        ));
    }
    Ok(())
}

fn cmd_rm(git: &Git, args: &[Vec<u8>]) -> R<i32> {
    let repo = git.repo()?;
    let p = opts::parse(&[], args, 0, USAGE_RM)?;
    if p.args.len() != 1 {
        return Err(usage_exit(USAGE_RM));
    }
    let name = os::lossy(&p.args[0]);
    let st = load_state(repo);
    let remote = st.remote_get(&name);
    if !remote.configured_in_repo {
        error(&format!("No such remote: '{name}'"));
        return Err(Fail::Exit(2));
    }
    for (bname, info) in &st.branches {
        if info.remote_name.as_deref() == Some(name.as_str()) {
            cfg_unset(repo, &format!("branch.{bname}.remote"))?;
            cfg_unset(repo, &format!("branch.{bname}.merge"))?;
        }
        if info.push_remote_name.as_deref() == Some(name.as_str()) {
            cfg_unset(repo, &format!("branch.{bname}.pushremote"))?;
        }
    }
    // Refs que só este remoto acompanha: as de `refs/remotes/` somem, as de `refs/heads/` ficam e
    // ganham a dica do `git branch -d`.
    let others: Vec<&Remote> = st.remotes.iter().filter(|r| r.name != remote.name).collect();
    let mut doomed: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for (refname, _) in repo.list_refs("refs/")? {
        if find_tracking(&remote, &refname).is_none() || others.iter().any(|o| find_tracking(o, &refname).is_some()) {
            continue;
        }
        if !refname.starts_with("refs/remotes/") {
            if refname.starts_with("refs/heads/") {
                skipped.push(abbrev_branch(&refname).to_string());
            }
            continue;
        }
        doomed.push(refname);
    }
    for r in &doomed {
        repo.delete_ref(r, None)?;
    }
    if !skipped.is_empty() {
        let mut m = if skipped.len() == 1 {
            "Note: A branch outside the refs/remotes/ hierarchy was not removed;\nto delete it, use:\n".to_string()
        } else {
            "Note: Some branches outside the refs/remotes/ hierarchy were not removed;\nto delete them, use:\n".to_string()
        };
        for b in &skipped {
            m.push_str(&format!("  git branch -d {b}\n"));
        }
        os::errs(&m);
    }
    if !cfg_remove_section(repo, &format!("remote.{name}"))? {
        error(&format!("Could not remove config section 'remote.{name}'"));
        return Ok(1);
    }
    handle_push_default(repo, &name, None)?;
    Ok(0)
}

// ---- rename -----------------------------------------------------------------------------------

/// O `strbuf_splice` do rename: troca o nome do remoto logo depois de `refs/remotes/`.
fn splice_remote(name: &str, old: &str, new: &str) -> String {
    let start = "refs/remotes/".len();
    let mut s = name.to_string();
    if s.len() >= start + old.len() && s.is_char_boundary(start) && s.is_char_boundary(start + old.len()) {
        s.replace_range(start..start + old.len(), new);
    }
    s
}

/// `refs_rename_ref`: leva a ref e o reflog dela e deixa a linha de `msg`.
fn rename_ref(repo: &Repo, old: &str, new: &str, msg: &str) -> R<()> {
    let Some(oid) = repo.ref_oid(old)? else {
        return Err(Fail::Fatal(format!("renaming '{old}' failed")));
    };
    let had_log = repo.stash_reflog(old)?;
    repo.delete_ref(old, None)?;
    if had_log {
        repo.unstash_reflog(new)?;
    }
    repo.set_ref_no_log(new, oid)?;
    repo.append_reflog(new, oid, oid, msg, true)
}

fn cmd_rename(git: &Git, args: &[Vec<u8>]) -> R<i32> {
    let repo = git.repo()?;
    let p = opts::parse(RENAME_SPECS, args, 0, USAGE_RENAME)?;
    if p.args.len() != 2 {
        return Err(usage_exit(USAGE_RENAME));
    }
    let old = os::lossy(&p.args[0]);
    let new = os::lossy(&p.args[1]);
    let st = load_state(repo);
    let oldremote = st.remote_get(&old);
    if !oldremote.configured_in_repo {
        error(&format!("No such remote: '{old}'"));
        return Err(Fail::Exit(2));
    }
    if st.remote_get(&new).configured_in_repo {
        error(&format!("remote {new} already exists."));
        return Err(Fail::Exit(3));
    }
    if !valid_remote_name(&new) {
        return Err(Fail::Fatal(format!("'{new}' is not a valid remote name")));
    }
    if !cfg_rename_remote_section(repo, &old, &new)? {
        error(&format!("Could not rename config section 'remote.{old}' to 'remote.{new}'"));
        return Ok(1);
    }
    let mut refspec_updated = false;
    if !oldremote.fetch.is_empty() {
        let key = format!("remote.{new}.fetch");
        cfg_unset_all(repo, &key, None)?;
        let context = format!(":refs/remotes/{old}/");
        for raw in &oldremote.fetch {
            let mut spec = raw.clone();
            if let Some(pos) = spec.find(&context) {
                refspec_updated = true;
                let start = pos + ":refs/remotes/".len();
                spec.replace_range(start..start + old.len(), &new);
            } else {
                warning(&format!("Not updating non-default fetch refspec\n\t{spec}\n\tPlease update the configuration manually if necessary."));
            }
            cfg_add(repo, &key, &spec)?;
        }
    }
    for (bname, info) in &st.branches {
        if info.remote_name.as_deref() == Some(old.as_str()) {
            cfg_set(repo, &format!("branch.{bname}.remote"), &new)?;
        }
        if info.push_remote_name.as_deref() == Some(old.as_str()) {
            cfg_set(repo, &format!("branch.{bname}.pushRemote"), &new)?;
        }
    }
    if !refspec_updated {
        return Ok(0);
    }
    // Primeiro somem as refs simbólicas, depois as outras são renomeadas, e por fim as simbólicas
    // voltam apontando pros nomes novos.
    let prefix = format!("refs/remotes/{old}/");
    let mut found: Vec<(String, Option<String>)> = Vec::new();
    for (refname, _) in repo.list_refs(&prefix)? {
        let sym = match repo.symref_target(&refname)? {
            Some(_) => repo.resolve_ref(&refname)?.map(|(n, _)| n),
            None => None,
        };
        found.push((refname, sym));
    }
    for (refname, sym) in &found {
        if sym.is_some() && repo.delete_ref(refname, None).is_err() {
            return Err(Fail::Fatal(format!("deleting '{refname}' failed")));
        }
    }
    for (refname, sym) in &found {
        if sym.is_some() {
            continue;
        }
        let newname = splice_remote(refname, &old, &new);
        let msg = format!("remote: renamed {refname} to {newname}");
        if rename_ref(repo, refname, &newname, &msg).is_err() {
            return Err(Fail::Fatal(format!("renaming '{refname}' failed")));
        }
    }
    for (refname, sym) in &found {
        let Some(target) = sym else { continue };
        let newname = splice_remote(refname, &old, &new);
        let newtarget = splice_remote(target, &old, &new);
        let msg = format!("remote: renamed {refname} to {newname}");
        if repo.set_symref(&newname, &newtarget, Some(&msg)).is_err() {
            return Err(Fail::Fatal(format!("creating '{newname}' failed")));
        }
    }
    handle_push_default(repo, &old, Some(&new))?;
    Ok(0)
}

// ---- set-head ---------------------------------------------------------------------------------

fn cmd_set_head(git: &Git, args: &[Vec<u8>]) -> R<i32> {
    let repo = git.repo()?;
    let p = opts::parse(SET_HEAD_SPECS, args, 0, USAGE_SET_HEAD)?;
    let argc = p.args.len();
    let (opt_a, opt_d) = (p.has("auto"), p.has("delete"));
    let name = p.args.first().map(|a| os::lossy(a)).unwrap_or_default();
    let head_ref = format!("refs/remotes/{name}/HEAD");
    let mut result = 0;
    let mut head_name: Option<String> = None;
    if !opt_a && !opt_d && argc == 2 {
        head_name = Some(os::lossy(&p.args[1]));
    } else if opt_a && !opt_d && argc == 1 {
        let st = load_state(repo);
        return Err(query_failure(&st.remote_get(&name)));
    } else if opt_d && !opt_a && argc == 1 {
        if repo.delete_ref(&head_ref, None).is_err() {
            error(&format!("Could not delete {head_ref}"));
            result = 1;
        }
    } else {
        return Err(usage_exit(USAGE_SET_HEAD));
    }
    if let Some(h) = head_name {
        let target = format!("refs/remotes/{name}/{h}");
        if repo.ref_oid(&target)?.is_none() {
            error(&format!("Not a valid ref: {target}"));
            result = 1;
        } else if update_symref(repo, &head_ref, &target, "remote set-head").is_err() {
            error(&format!("Could not setup {head_ref}"));
            result = 1;
        } else if opt_a {
            os::outs(&format!("{name}/HEAD set to {h}\n"));
        }
    }
    Ok(result)
}

// ---- set-branches, get-url, set-url -----------------------------------------------------------

fn cmd_set_branches(git: &Git, args: &[Vec<u8>]) -> R<i32> {
    let repo = git.repo()?;
    let p = opts::parse(SET_BRANCHES_SPECS, args, 0, USAGE_SET_BRANCHES)?;
    if p.args.is_empty() {
        error("no remote specified");
        return Err(usage_exit(USAGE_SET_BRANCHES));
    }
    let name = os::lossy(&p.args[0]);
    let st = load_state(repo);
    let remote = st.remote_get(&name);
    if !remote.configured_in_repo {
        error(&format!("No such remote '{name}'"));
        return Err(Fail::Exit(2));
    }
    let key = format!("remote.{name}.fetch");
    if !p.has("add") {
        cfg_unset_all(repo, &key, None)?;
    }
    for b in &p.args[1..] {
        add_branch(repo, &key, &os::lossy(b), &name, remote.mirror)?;
    }
    Ok(0)
}

fn cmd_get_url(git: &Git, args: &[Vec<u8>]) -> R<i32> {
    let repo = git.repo()?;
    let p = opts::parse(GET_URL_SPECS, args, 0, USAGE_GET_URL)?;
    if p.args.len() != 1 {
        return Err(usage_exit(USAGE_GET_URL));
    }
    let name = os::lossy(&p.args[0]);
    let st = load_state(repo);
    let remote = st.remote_get(&name);
    if !remote.configured_in_repo {
        error(&format!("No such remote '{name}'"));
        return Err(Fail::Exit(2));
    }
    let urls = if p.has("push") { push_urls(&remote) } else { &remote.url };
    let mut out = String::new();
    if p.has("all") {
        for u in urls {
            out.push_str(u);
            out.push('\n');
        }
    } else if let Some(u) = urls.first() {
        out.push_str(u);
        out.push('\n');
    }
    os::outs(&out);
    Ok(0)
}

fn cmd_set_url(git: &Git, args: &[Vec<u8>]) -> R<i32> {
    let repo = git.repo()?;
    let p = opts::parse(SET_URL_SPECS, args, 0, USAGE_SET_URL)?;
    let (push, add, delete) = (p.has("push"), p.has("add"), p.has("delete"));
    if add && delete {
        return Err(Fail::Fatal("--add --delete doesn't make sense".into()));
    }
    // O `argv[0]` (o nome do subcomando) fica na conta do git.
    let argc = p.args.len() + 1;
    if !(3..=4).contains(&argc) || ((add || delete) && argc != 3) {
        return Err(usage_exit(USAGE_SET_URL));
    }
    let name = os::lossy(&p.args[0]);
    let newurl = os::lossy(&p.args[1]);
    let mut oldurl: Option<String> = p.args.get(2).map(|a| os::lossy(a));
    if delete {
        oldurl = Some(newurl.clone());
    }
    let st = load_state(repo);
    let remote = st.remote_get(&name);
    if !remote.configured_in_repo {
        error(&format!("No such remote '{name}'"));
        return Err(Fail::Exit(2));
    }
    let key = if push { format!("remote.{name}.pushurl") } else { format!("remote.{name}.url") };
    let urlset = if push { &remote.pushurl } else { &remote.url };
    if (oldurl.is_none() && !delete) || add {
        if add {
            cfg_add(repo, &key, &newurl)?;
        } else {
            cfg_set(repo, &key, &newurl)?;
        }
        return Ok(0);
    }
    let Some(oldurl) = oldurl else { return Ok(0) };
    let re = old_url_regex(&oldurl)?;
    let matches = urlset.iter().filter(|u| re.is_match(u.as_bytes())).count();
    let negative = urlset.len() - matches;
    if !delete && matches == 0 {
        return Err(Fail::Fatal(format!("No such URL found: {oldurl}")));
    }
    if delete && negative == 0 && !push {
        return Err(Fail::Fatal("Will not delete all non-push URLs".into()));
    }
    let filter = |v: Option<&[u8]>| v.is_some_and(|b| re.is_match(b));
    if delete {
        cfg_unset_all(repo, &key, Some(&filter))?;
    } else {
        cfg_set_with(repo, &key, &newurl, Some(&filter))?;
    }
    Ok(0)
}

// ---- lista e show -----------------------------------------------------------------------------

/// `printf("%-*s")`: preenche até `w` bytes.
fn pad(s: &str, w: usize) -> String {
    format!("{s}{}", " ".repeat(w.saturating_sub(s.len())))
}

/// `git remote` e `git remote -v`: um nome por remoto (ou uma linha por URL, com `-v`).
fn show_all(repo: &Repo, st: &State, verbose: bool) -> R<i32> {
    let mut list: Vec<(String, Option<String>)> = Vec::new();
    for r in &st.remotes {
        if let Some(u) = r.url.first() {
            let mut info = format!("{u} (fetch)");
            if let Some(f) = repo.config.get(&format!("remote.{}.partialclonefilter", r.name)) {
                info.push_str(&format!(" [{f}]"));
            }
            list.push((r.name.clone(), Some(info)));
        } else {
            list.push((r.name.clone(), None));
        }
        for u in push_urls(r) {
            list.push((r.name.clone(), Some(format!("{u} (push)"))));
        }
    }
    list.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::new();
    for (i, (name, info)) in list.iter().enumerate() {
        if verbose {
            out.push_str(&format!("{name}\t{}\n", info.as_deref().unwrap_or("")));
        } else {
            if i > 0 && list[i - 1].0 == *name {
                continue;
            }
            out.push_str(&format!("{name}\n"));
        }
    }
    os::outs(&out);
    Ok(0)
}

/// O que `remote.<nome>.push` manda enviar, sem consultar o remoto (`get_push_ref_states_noquery`).
struct PushItem {
    src: String,
    dest: String,
    forced: bool,
}

fn push_items(r: &Remote) -> Vec<PushItem> {
    if r.mirror {
        return Vec::new();
    }
    if r.push.is_empty() {
        return vec![PushItem { src: "(matching)".to_string(), dest: "(matching)".to_string(), forced: false }];
    }
    let mut out = Vec::new();
    for raw in &r.push {
        let (forced, spec) = match raw.strip_prefix('+') {
            Some(s) => (true, s),
            None => (false, raw.as_str()),
        };
        let (src, dst) = if spec == ":" {
            (String::new(), None)
        } else {
            match spec.split_once(':') {
                Some((s, d)) => (s.to_string(), Some(d.to_string())),
                None => (spec.to_string(), None),
            }
        };
        let item = if spec == ":" {
            "(matching)".to_string()
        } else if !src.is_empty() {
            src
        } else {
            "(delete)".to_string()
        };
        let dest = dst.unwrap_or_else(|| item.clone());
        out.push(PushItem { src: item, dest, forced });
    }
    out
}

fn show_one(repo: &Repo, st: &State, remote: &Remote) -> R<()> {
    let mut o = String::new();
    o.push_str(&format!("* remote {}\n", remote.name));
    o.push_str(&format!("  Fetch URL: {}\n", remote.url.first().map(String::as_str).unwrap_or("")));
    let pushes = push_urls(remote);
    for u in pushes {
        o.push_str(&format!("  Push  URL: {u}\n"));
    }
    if pushes.is_empty() {
        o.push_str("  Push  URL: (no URL)\n");
    }
    o.push_str("  HEAD branch: (not queried)\n");

    // Ramos do remoto que a configuração acompanha: o que `refs/` já tem.
    let mut tracked: Vec<String> = Vec::new();
    for (refname, _) in repo.list_refs("refs/")? {
        if repo.symref_target(&refname)?.is_some() {
            continue;
        }
        if let Some(src) = find_tracking(remote, &refname) {
            tracked.push(abbrev_branch(&src).to_string());
        }
    }
    tracked.sort();
    tracked.dedup();
    if !tracked.is_empty() {
        o.push_str(&format!(
            "  Remote branch{}: (status not queried)\n",
            if tracked.len() == 1 { "" } else { "es" }
        ));
        for t in &tracked {
            o.push_str(&format!("    {t}\n"));
        }
    }

    // Ramos locais que o `git pull` junta com este remoto.
    let locals: Vec<(&String, &BranchInfo)> = st
        .branches
        .iter()
        .filter(|(_, b)| !b.merge.is_empty() && b.remote_name.as_deref() == Some(remote.name.as_str()))
        .collect();
    if !locals.is_empty() {
        let width = locals.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
        let any_rebase = locals.iter().any(|(_, b)| b.rebase >= Rebase::True);
        o.push_str(&format!(
            "  Local branch{} configured for 'git pull':\n",
            if locals.len() == 1 { "" } else { "es" }
        ));
        for (name, b) in &locals {
            let mut w = width + 4;
            if b.rebase >= Rebase::True && b.merge.len() > 1 {
                error(&format!("invalid branch.{name}.merge; cannot rebase onto > 1 branch"));
                continue;
            }
            o.push_str(&format!("    {} ", pad(name, width)));
            if b.rebase >= Rebase::True {
                let msg = match b.rebase {
                    Rebase::Interactive => "rebases interactively onto remote",
                    Rebase::Merges => "rebases interactively (with merges) onto remote",
                    _ => "rebases onto remote",
                };
                o.push_str(&format!("{msg} {}\n", b.merge[0]));
                continue;
            } else if any_rebase {
                o.push_str(&format!(" merges with remote {}\n", b.merge[0]));
                w += 1;
            } else {
                o.push_str(&format!("merges with remote {}\n", b.merge[0]));
            }
            for m in &b.merge[1..] {
                o.push_str(&format!("{}    and with remote {m}\n", pad("", w)));
            }
        }
    }

    if remote.mirror {
        o.push_str("  Local refs will be mirrored by 'git push'\n");
    }

    // O que o `git push` enviaria, sem consultar o remoto.
    let mut items = push_items(remote);
    items.sort_by(|a, b| a.src.cmp(&b.src).then_with(|| a.dest.cmp(&b.dest)));
    if !items.is_empty() {
        let w1 = items.iter().map(|i| i.src.len()).max().unwrap_or(0);
        o.push_str(&format!(
            "  Local ref{} configured for 'git push' (status not queried):\n",
            if items.len() == 1 { "" } else { "s" }
        ));
        for i in &items {
            o.push_str(&format!("    {} {} {}\n", pad(&i.src, w1), if i.forced { "forces to" } else { "pushes to" }, i.dest));
        }
    }
    os::outs(&o);
    Ok(())
}

fn cmd_show(git: &Git, args: &[Vec<u8>], verbose: bool) -> R<i32> {
    let repo = git.repo()?;
    let p = opts::parse(SHOW_SPECS, args, 0, USAGE_SHOW)?;
    let st = load_state(repo);
    if p.args.is_empty() {
        return show_all(repo, &st, verbose);
    }
    let no_query = p.has("no-query");
    for a in &p.args {
        let remote = st.remote_get(&os::lossy(a));
        if !no_query {
            return Err(query_failure(&remote));
        }
        show_one(repo, &st, &remote)?;
    }
    Ok(0)
}

// ---- prune e update ---------------------------------------------------------------------------

fn cmd_prune(git: &Git, args: &[Vec<u8>]) -> R<i32> {
    let repo = git.repo()?;
    let p = opts::parse(PRUNE_SPECS, args, 0, USAGE_PRUNE)?;
    if p.args.is_empty() {
        return Err(usage_exit(USAGE_PRUNE));
    }
    let st = load_state(repo);
    let remote = st.remote_get(&os::lossy(&p.args[0]));
    Err(query_failure(&remote))
}

/// `git remote update`: o `git fetch --multiple` sobre os remotos pedidos, os do grupo
/// `remotes.<nome>` ou todos (`--all`) quando não há grupo `default`.
fn cmd_update(git: &Git, args: &[Vec<u8>]) -> R<i32> {
    let repo = git.repo()?;
    let p = opts::parse(UPDATE_SPECS, args, 0, USAGE_UPDATE)?;
    let st = load_state(repo);
    let mut names: Vec<String> = p.args.iter().map(|a| os::lossy(a)).collect();
    if names.is_empty() {
        names.push("default".to_string());
    }
    let mut all = false;
    if names.last().map(String::as_str) == Some("default") && repo.config.get_all("remotes.default").is_empty() {
        names.pop();
        all = true;
    }
    let mut list: Vec<String> = Vec::new();
    if all {
        for r in &st.remotes {
            if !r.skip_default_update {
                list.push(r.name.clone());
            }
        }
    } else {
        for n in &names {
            let groups = repo.config.get_all(&format!("remotes.{n}"));
            if !groups.is_empty() {
                for g in groups.into_iter().flatten() {
                    for word in os::lossy(g).split_whitespace() {
                        list.push(word.to_string());
                    }
                }
            } else if st.remotes.iter().any(|r| r.name == *n && r.configured) {
                list.push(n.clone());
            } else {
                os::errs(&format!("fatal: no such remote or remote group: {n}\n"));
                return Ok(1);
            }
        }
    }
    let mut result = 0;
    for n in &list {
        os::outs(&format!("Fetching {n}\n"));
        if !fetch_remote(&st.remote_get(n)) {
            error(&format!("could not fetch {n}"));
            result = 1;
        }
    }
    Ok(result)
}

// ---- entrada ----------------------------------------------------------------------------------

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(TOP_SPECS, args, opts::STOP_AT_NON_OPTION, usage)?;
    let verbose = p.has("verbose");
    let git: &Git = git;
    let Some(sub) = p.args.first() else {
        return show_all(git.repo()?, &load_state(git.repo()?), verbose);
    };
    let rest = &p.args[1..];
    let code = match sub.as_slice() {
        b"add" => cmd_add(git, rest)?,
        b"rename" => cmd_rename(git, rest)?,
        b"rm" | b"remove" => cmd_rm(git, rest)?,
        b"set-head" => cmd_set_head(git, rest)?,
        b"set-branches" => cmd_set_branches(git, rest)?,
        b"get-url" => cmd_get_url(git, rest)?,
        b"set-url" => cmd_set_url(git, rest)?,
        b"show" => cmd_show(git, rest, verbose)?,
        b"prune" => cmd_prune(git, rest)?,
        b"update" => cmd_update(git, rest)?,
        other => {
            return Err(opts::usage_error(usage, &format!("unknown subcommand: `{}'", os::lossy(other))));
        }
    };
    // O `return !!fn(...)` do git.
    Ok(i32::from(code != 0))
}


