//! Descoberta do repositório (o `setup_git_directory` do git), configuração em camadas e caminhos.

use std::cell::RefCell;
use std::rc::Rc;

use crate::config::{self, Config, ConfigFile, Scope};
use crate::error::{Fail, R};
use crate::odb::Odb;
use crate::os;
use crate::refs::PackedRef;

/// Opções globais do `git` (antes do subcomando).
#[derive(Clone, Debug, Default)]
pub struct Globals {
    pub git_dir: Option<Vec<u8>>,
    pub work_tree: Option<Vec<u8>>,
    pub bare: bool,
    /// `-c chave=valor`, na ordem.
    pub config: Vec<(String, Option<Vec<u8>>)>,
    pub literal_pathspecs: bool,
    pub glob_pathspecs: bool,
    pub noglob_pathspecs: bool,
    pub icase_pathspecs: bool,
}

pub struct Repo {
    /// Como o git mostraria o diretório (`.git`, `/abs/.git`, ou o `GIT_DIR` dado).
    pub git_dir_display: Vec<u8>,
    /// Diretório do repositório (absoluto).
    pub git_dir: Vec<u8>,
    /// Diretório comum (o mesmo do `git_dir` fora de worktrees adicionais).
    pub common_dir: Vec<u8>,
    /// Topo da árvore de trabalho (absoluto); `None` em repositório bare ou dentro do `.git`.
    pub work_tree: Option<Vec<u8>>,
    /// Caminho do cwd original relativo ao topo, com `/` no fim (vazio no topo).
    pub prefix: Vec<u8>,
    pub bare: bool,
    pub inside_git_dir: bool,
    pub odb: Odb,
    pub config: Config,
    /// `packed-refs` já lido (invalidado quando o próprio processo o reescreve).
    pub packed_cache: RefCell<Option<Rc<Vec<PackedRef>>>>,
}

impl Repo {
    /// `<git_dir>/<rel>` (arquivos por worktree: HEAD, index, logs/HEAD...).
    pub fn path(&self, rel: &str) -> Vec<u8> {
        os::join(&self.git_dir, rel.as_bytes())
    }

    /// `<common_dir>/<rel>` (objects, refs, config, packed-refs).
    pub fn common(&self, rel: &str) -> Vec<u8> {
        os::join(&self.common_dir, rel.as_bytes())
    }

    pub fn index_path(&self) -> Vec<u8> {
        match os::getenv("GIT_INDEX_FILE") {
            Some(p) if !p.is_empty() => {
                if p.starts_with(b"/") {
                    p
                } else {
                    os::absolute(&p)
                }
            }
            _ => self.path("index"),
        }
    }

    /// Topo da árvore de trabalho ou o erro do git.
    pub fn work_tree(&self) -> R<&[u8]> {
        self.work_tree.as_deref().ok_or_else(|| Fail::Fatal("this operation must be run in a work tree".into()))
    }

    /// Caminho dado pelo usuário (relativo ao cwd original) como caminho relativo ao topo.
    pub fn rel_from_prefix(&self, p: &[u8]) -> Option<Vec<u8>> {
        let top = self.work_tree.as_deref()?;
        let abs = if p.starts_with(b"/") { os::normalize_abs(p) } else { os::normalize_abs(&os::join(&os::join(top, &self.prefix), p)) };
        if abs == top {
            return Some(Vec::new());
        }
        let mut t = top.to_vec();
        if !t.ends_with(b"/") {
            t.push(b'/');
        }
        abs.strip_prefix(t.as_slice()).map(|r| r.to_vec())
    }

    /// Caminho relativo ao topo mostrado relativo ao cwd original (`../a`, `b`), como o status.
    pub fn display_path(&self, path: &[u8]) -> Vec<u8> {
        relative_to(path, &self.prefix)
    }

    pub fn abbrev_len(&self) -> usize {
        match self.config.get("core.abbrev") {
            Some(v) if v == "no" => 40,
            Some(v) => match v.parse::<usize>() {
                Ok(n) => n.clamp(4, 40),
                Err(_) => self.default_abbrev(),
            },
            None => self.default_abbrev(),
        }
    }

    fn default_abbrev(&self) -> usize {
        let count = self.odb.approximate_count();
        if count == 0 {
            return 7;
        }
        let bits = usize::BITS as usize - count.leading_zeros() as usize;
        bits.div_ceil(2).max(7)
    }
}

/// `path` (relativo ao topo) visto de `prefix` (diretório relativo ao topo, com `/` no fim).
pub fn relative_to(path: &[u8], prefix: &[u8]) -> Vec<u8> {
    if prefix.is_empty() {
        return path.to_vec();
    }
    // Parte comum, por componente.
    let mut common = 0;
    let mut i = 0;
    while i < prefix.len() && i < path.len() && prefix[i] == path[i] {
        if prefix[i] == b'/' {
            common = i + 1;
        }
        i += 1;
    }
    if i == prefix.len() && prefix.ends_with(b"/") {
        common = i;
    }
    let ups = prefix[common..].iter().filter(|c| **c == b'/').count();
    let mut out = Vec::new();
    for _ in 0..ups {
        out.extend_from_slice(b"../");
    }
    out.extend_from_slice(&path[common..]);
    if out.is_empty() {
        out.extend_from_slice(b"./");
    }
    out
}

/// O `is_git_directory` do git.
pub fn is_git_directory(dir: &[u8]) -> bool {
    let common = match os::read_opt(&os::join(dir, b"commondir")) {
        Ok(Some(c)) => {
            let c = crate::object::trim_ascii(&c).to_vec();
            if c.starts_with(b"/") { c } else { os::normalize_abs(&os::join(&os::absolute(dir), &c)) }
        }
        _ => dir.to_vec(),
    };
    if os::getenv("GIT_OBJECT_DIRECTORY").is_none() && !os::is_dir(&os::join(&common, b"objects")) {
        return false;
    }
    if !os::is_dir(&os::join(&common, b"refs")) {
        return false;
    }
    validate_headref(&os::join(dir, b"HEAD"))
}

fn validate_headref(path: &[u8]) -> bool {
    if let Ok(target) = os::readlink(path) {
        return target.starts_with(b"refs/");
    }
    let Ok(Some(data)) = os::read_opt(path) else { return false };
    if let Some(rest) = data.strip_prefix(b"ref:") {
        let rest = crate::object::trim_ascii(rest);
        return rest.starts_with(b"refs/");
    }
    data.len() >= 40 && crate::hash::is_hex(&data[..40])
}

/// Lê um gitfile (`gitdir: caminho`), resolvendo relativo ao diretório dele.
fn read_gitfile(path: &[u8]) -> Option<Vec<u8>> {
    let data = os::read_opt(path).ok()??;
    let rest = data.strip_prefix(b"gitdir: ")?;
    let rest = crate::object::trim_ascii(rest);
    let abs = if rest.starts_with(b"/") { rest.to_vec() } else { os::join(os::dirname(&os::absolute(path)), rest) };
    let abs = os::normalize_abs(&abs);
    is_git_directory(&abs).then_some(abs)
}

/// Carrega sistema, global e linha de comando (sem o repositório).
pub fn base_config(g: &Globals) -> R<Vec<ConfigFile>> {
    let mut files = Vec::new();
    let nosystem = os::getenv("GIT_CONFIG_NOSYSTEM").map(|v| config::parse_bool(Some(&v)).unwrap_or(false)).unwrap_or(false);
    if !nosystem {
        let path = os::getenv("GIT_CONFIG_SYSTEM").unwrap_or_else(|| b"/etc/gitconfig".to_vec());
        if let Some(f) = ConfigFile::load(&path, path.clone(), Scope::System)? {
            files.push(f);
        }
    }
    for path in global_paths() {
        if let Some(f) = ConfigFile::load(&path, path.clone(), Scope::Global)? {
            files.push(f);
        }
    }
    let _ = g;
    Ok(files)
}

/// Arquivos globais na ordem de leitura (XDG primeiro, depois `~/.gitconfig`).
pub fn global_paths() -> Vec<Vec<u8>> {
    if let Some(p) = os::getenv("GIT_CONFIG_GLOBAL") {
        return if p.is_empty() { Vec::new() } else { vec![p] };
    }
    let mut out = Vec::new();
    let home = os::getenv("HOME");
    match os::getenv("XDG_CONFIG_HOME") {
        Some(x) if !x.is_empty() => out.push(os::join(&x, b"git/config")),
        _ => {
            if let Some(h) = &home {
                out.push(os::join(h, b".config/git/config"));
            }
        }
    }
    if let Some(h) = &home {
        out.push(os::join(h, b".gitconfig"));
    }
    out
}

/// Onde o `git config --global` escreve.
pub fn global_write_path() -> Option<Vec<u8>> {
    if let Some(p) = os::getenv("GIT_CONFIG_GLOBAL") {
        return Some(p);
    }
    let paths = global_paths();
    let user = paths.last()?.clone();
    if !os::exists(&user)
        && let Some(xdg) = paths.first()
        && xdg != &user
        && os::exists(xdg)
    {
        return Some(xdg.clone());
    }
    Some(user)
}

/// Entradas de `-c`, de `GIT_CONFIG_PARAMETERS` e de `GIT_CONFIG_COUNT`.
pub fn command_config(g: &Globals) -> R<ConfigFile> {
    let mut items: Vec<(String, Option<Vec<u8>>)> = Vec::new();
    if let Some(p) = os::getenv("GIT_CONFIG_PARAMETERS") {
        items.extend(config::parse_config_parameters(&p));
    }
    if let Some(n) = os::getenv_str("GIT_CONFIG_COUNT") {
        let n: usize = n.trim().parse().map_err(|_| Fail::Fatal("bogus count in GIT_CONFIG_COUNT".to_string()))?;
        for i in 0..n {
            let k = os::getenv_str(&format!("GIT_CONFIG_KEY_{i}")).ok_or_else(|| Fail::Fatal(format!("missing config key GIT_CONFIG_KEY_{i}")))?;
            let v = os::getenv(&format!("GIT_CONFIG_VALUE_{i}")).ok_or_else(|| Fail::Fatal(format!("missing config value GIT_CONFIG_VALUE_{i}")))?;
            items.push((k, Some(v)));
        }
    }
    items.extend(g.config.iter().cloned());
    Ok(config::command_line_file(&items))
}

/// Configuração completa: base, repositório (se houver) e linha de comando.
pub fn full_config(g: &Globals, git_dir: Option<&[u8]>, display: Option<&[u8]>) -> R<Config> {
    let mut files = base_config(g)?;
    if let Some(dir) = git_dir {
        let common = common_dir_of(dir);
        let disp = match display {
            Some(d) => os::join(&common_display(d, dir, &common), b"config"),
            None => os::join(&common, b"config"),
        };
        if let Some(f) = ConfigFile::load(&os::join(&common, b"config"), disp.clone(), Scope::Local)? {
            let wt = f.entries.iter().any(|e| e.key == "extensions.worktreeconfig" && config::parse_bool(e.value.as_deref()) == Some(true));
            files.push(f);
            if wt {
                let p = os::join(dir, b"config.worktree");
                let mut d = disp.clone();
                d.extend_from_slice(b".worktree");
                if let Some(f) = ConfigFile::load(&p, d, Scope::Worktree)? {
                    files.push(f);
                }
            }
        }
    }
    files.push(command_config(g)?);
    Ok(Config { files })
}

fn common_display(display: &[u8], dir: &[u8], common: &[u8]) -> Vec<u8> {
    if common == dir { display.to_vec() } else { common.to_vec() }
}

pub fn common_dir_of(dir: &[u8]) -> Vec<u8> {
    if let Some(c) = os::getenv("GIT_COMMON_DIR") {
        return os::absolute(&c);
    }
    match os::read_opt(&os::join(dir, b"commondir")) {
        Ok(Some(c)) => {
            let c = crate::object::trim_ascii(&c).to_vec();
            if c.starts_with(b"/") { os::normalize_abs(&c) } else { os::normalize_abs(&os::join(dir, &c)) }
        }
        _ => dir.to_vec(),
    }
}

fn ceiling_dirs() -> Vec<Vec<u8>> {
    os::getenv("GIT_CEILING_DIRECTORIES")
        .map(|v| v.split(|c| *c == b':').filter(|p| p.starts_with(b"/")).map(os::normalize_abs).collect())
        .unwrap_or_default()
}

/// A falha de quem precisa de repositório e não achou nenhum: a mensagem depende de onde a busca
/// parou. Subindo a partir do cwd, a primeira vez que o pai está em outro dispositivo (outro
/// ponto de montagem) o git desiste e nomeia o pai; chegando na raiz, ou num diretório do
/// `GIT_CEILING_DIRECTORIES`, a mensagem é a curta. `GIT_DISCOVERY_ACROSS_FILESYSTEM=1` desliga a
/// checagem de dispositivo.
pub fn not_a_repository() -> Fail {
    Fail::Fatal(not_a_repository_message())
}

fn not_a_repository_message() -> String {
    const SHORT: &str = "not a git repository (or any of the parent directories): .git";
    let across = os::getenv("GIT_DISCOVERY_ACROSS_FILESYSTEM").is_some_and(|v| config::parse_bool(Some(v.as_slice())) == Some(true));
    let ceilings = ceiling_dirs();
    let Ok(mut dir) = os::getcwd() else { return SHORT.to_string() };
    loop {
        if dir == b"/" || dir.is_empty() || ceilings.contains(&dir) {
            return SHORT.to_string();
        }
        let parent = os::dirname(&dir).to_vec();
        if !across
            && let (Ok(a), Ok(b)) = (os::stat(&dir), os::stat(&parent))
            && a.dev != b.dev
        {
            return format!(
                "not a git repository (or any parent up to mount point {})\nStopping at filesystem boundary (GIT_DISCOVERY_ACROSS_FILESYSTEM not set).",
                os::lossy(&parent)
            );
        }
        dir = parent;
    }
}

/// Encontra o repositório a partir do cwd. `Ok(None)` fora de um repositório.
pub fn discover(g: &Globals) -> R<Option<Repo>> {
    let cwd = os::getcwd().map_err(|e| Fail::Fatal(format!("unable to get current working directory: {}", e.message())))?;
    let env_git_dir = g.git_dir.clone().or_else(|| os::getenv("GIT_DIR"));
    let env_work_tree = g.work_tree.clone().or_else(|| os::getenv("GIT_WORK_TREE"));
    if let Some(gd) = env_git_dir {
        let abs = os::absolute(&gd);
        let abs = match read_gitfile(&abs) {
            Some(r) => r,
            None => abs,
        };
        if !is_git_directory(&abs) {
            return Err(Fail::Fatal(format!("not a git repository: '{}'", os::lossy(&gd))));
        }
        let config = full_config(g, Some(&abs), Some(&gd))?;
        let bare_cfg = config.get_bool("core.bare")?.unwrap_or(false) || g.bare;
        let wt = match env_work_tree {
            Some(w) => Some(os::absolute(&w)),
            None => match config.get_bytes("core.worktree") {
                Some(w) => Some(if w.starts_with(b"/") { os::normalize_abs(&w) } else { os::normalize_abs(&os::join(&abs, &w)) }),
                None => (!bare_cfg).then(|| cwd.clone()),
            },
        };
        return finish(g, gd, abs, wt, &cwd, config, false).map(Some);
    }
    let ceilings = ceiling_dirs();
    let mut dir = cwd.clone();
    loop {
        let dotgit = os::join(&dir, b".git");
        if os::is_dir(&dotgit) && is_git_directory(&dotgit) {
            let display = if dir == cwd { b".git".to_vec() } else { dotgit.clone() };
            let config = full_config(g, Some(&dotgit), Some(&display))?;
            let bare = config.get_bool("core.bare")?.unwrap_or(false);
            let wt = match env_work_tree {
                Some(w) => Some(os::absolute(&w)),
                None => match config.get_bytes("core.worktree") {
                    Some(w) => Some(if w.starts_with(b"/") { os::normalize_abs(&w) } else { os::normalize_abs(&os::join(&dotgit, &w)) }),
                    None => (!bare).then(|| dir.clone()),
                },
            };
            return finish(g, display, dotgit, wt, &cwd, config, false).map(Some);
        }
        if os::is_file(&dotgit)
            && let Some(gd) = read_gitfile(&dotgit) {
                let config = full_config(g, Some(&gd), Some(&gd))?;
                let wt = match env_work_tree {
                    Some(w) => Some(os::absolute(&w)),
                    None => Some(dir.clone()),
                };
                return finish(g, gd.clone(), gd, wt, &cwd, config, false).map(Some);
            }
        if is_git_directory(&dir) {
            // Dentro de um diretório git (bare, ou o próprio `.git`).
            let config = full_config(g, Some(&dir), Some(b"."))?;
            let display = if dir == cwd { b".".to_vec() } else { dir.clone() };
            let bare = config.get_bool("core.bare")?.unwrap_or(false) || os::basename(&dir) != b".git";
            let inside_dotgit = os::basename(&dir) == b".git" && !bare;
            let wt = env_work_tree.map(|w| os::absolute(&w));
            let mut r = finish(g, display, dir.clone(), wt, &cwd, config, true)?;
            r.bare = bare && r.work_tree.is_none();
            r.inside_git_dir = true;
            let _ = inside_dotgit;
            return Ok(Some(r));
        }
        if dir == b"/" || ceilings.contains(&dir) {
            return Ok(None);
        }
        dir = os::dirname(&dir).to_vec();
    }
}

fn finish(g: &Globals, display: Vec<u8>, git_dir: Vec<u8>, work_tree: Option<Vec<u8>>, cwd: &[u8], config: Config, inside: bool) -> R<Repo> {
    let _ = g;
    let version = config.get_int("core.repositoryformatversion")?.unwrap_or(0);
    if version > 1 {
        return Err(Fail::Fatal(format!("Expected git repo version <= 1, found {version}")));
    }
    if version == 1 {
        for (_, e) in config.entries() {
            if let Some(ext) = e.key.strip_prefix("extensions.") {
                let ok = match ext {
                    "objectformat" => e.value.as_deref().map(|v| v.eq_ignore_ascii_case(b"sha1")).unwrap_or(false),
                    "worktreeconfig" | "noop" | "preciousobjects" | "partialclone" => true,
                    _ => false,
                };
                if !ok {
                    return Err(Fail::Fatal(format!("unknown repository extension found:\n\t{ext}")));
                }
            }
        }
    }
    let common_dir = common_dir_of(&git_dir);
    let objects = match os::getenv("GIT_OBJECT_DIRECTORY") {
        Some(o) => os::absolute(&o),
        None => os::join(&common_dir, b"objects"),
    };
    let bare = work_tree.is_none() && !inside;
    let mut prefix = Vec::new();
    let mut work_tree = work_tree;
    if let Some(wt) = &work_tree {
        let mut t = wt.clone();
        if !t.ends_with(b"/") {
            t.push(b'/');
        }
        let mut c = cwd.to_vec();
        if !c.ends_with(b"/") {
            c.push(b'/');
        }
        if c == t {
            prefix.clear();
        } else if let Some(rest) = c.strip_prefix(t.as_slice()) {
            prefix = rest.to_vec();
        } else if wt != b"/" {
            // cwd fora da árvore de trabalho: o git trata como se não houvesse worktree aqui.
            prefix.clear();
        }
        // Dentro do `.git` não há árvore de trabalho.
        let mut gd = git_dir.clone();
        gd.push(b'/');
        if c.starts_with(&gd) {
            work_tree = None;
            prefix.clear();
        }
    }
    if let Some(wt) = &work_tree
        && os::chdir(wt).is_err()
    {
        return Err(Fail::Fatal(format!("cannot chdir to '{}'", os::lossy(wt))));
    }
    Ok(Repo {
        git_dir_display: display,
        git_dir,
        common_dir,
        work_tree,
        prefix,
        bare,
        inside_git_dir: inside,
        odb: Odb::new(objects),
        config,
        packed_cache: RefCell::new(None),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(relative_to(b"a.txt", b""), b"a.txt");
        assert_eq!(relative_to(b"a.txt", b"sub/"), b"../a.txt");
        assert_eq!(relative_to(b"sub/x", b"sub/"), b"x");
        assert_eq!(relative_to(b"sub/y/x", b"sub/z/"), b"../y/x");
        assert_eq!(relative_to(b"sub/", b"sub/"), b"./");
    }
}
