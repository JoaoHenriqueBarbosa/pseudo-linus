//! Refs no backend de arquivos do git: refs soltas, `packed-refs`, refs simbólicas, reflogs,
//! travas com `.lock` e as regras de nome do `check-ref-format`.

use std::rc::Rc;

use crate::error::{Fail, R};
use crate::hash::Oid;
use crate::ident::{self, Who};
use crate::object::{Ident, format_tz};
use crate::os;
use crate::repo::Repo;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Oid(Oid),
    Sym(String),
}

/// Para onde o HEAD aponta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    /// `refs/heads/x` e o commit (se o ramo já existe).
    Branch(String, Option<Oid>),
    Detached(Oid),
}

impl Head {
    pub fn oid(&self) -> Option<Oid> {
        match self {
            Head::Branch(_, o) => *o,
            Head::Detached(o) => Some(*o),
        }
    }

    pub fn branch(&self) -> Option<&str> {
        match self {
            Head::Branch(b, _) => Some(b),
            Head::Detached(_) => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PackedRef {
    pub name: String,
    pub oid: Oid,
    pub peeled: Option<Oid>,
}

/// Uma entrada de reflog.
#[derive(Clone, Debug)]
pub struct ReflogEntry {
    pub old: Oid,
    pub new: Oid,
    /// `Nome <email>`.
    pub who: Vec<u8>,
    pub time: i64,
    pub tz: i32,
    pub message: Vec<u8>,
}

/// Refs que moram no diretório do worktree, não no comum.
fn is_per_worktree(name: &str) -> bool {
    !name.contains('/') || name.starts_with("refs/bisect/") || name.starts_with("refs/worktree/") || name.starts_with("refs/rewritten/")
}

/// Pseudo-ref no topo (`HEAD`, `ORIG_HEAD`, `MERGE_HEAD`...): só maiúsculas e `_`.
pub fn is_pseudoref_syntax(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|c| c.is_ascii_uppercase() || c == b'_' || c == b'-')
}

/// As regras do `check_refname_format`. `onelevel` permite nome sem `/`.
pub fn check_refname_format(name: &str, onelevel: bool, pattern: bool) -> bool {
    if name.is_empty() || name == "@" {
        return false;
    }
    let mut components = 0;
    let mut stars = 0;
    for comp in name.split('/') {
        components += 1;
        if comp.is_empty() {
            return false;
        }
        if comp.starts_with('.') || comp.ends_with(".lock") {
            return false;
        }
        let b = comp.as_bytes();
        for (i, &c) in b.iter().enumerate() {
            match c {
                0..=31 | 127 | b' ' | b'~' | b'^' | b':' | b'?' | b'[' | b'\\' => return false,
                b'*' => {
                    if !pattern {
                        return false;
                    }
                    stars += 1;
                    if stars > 1 {
                        return false;
                    }
                }
                b'.' if b.get(i + 1) == Some(&b'.') => return false,
                b'@' if b.get(i + 1) == Some(&b'{') => return false,
                _ => {}
            }
        }
    }
    if name.ends_with('.') || name.ends_with('/') {
        return false;
    }
    components >= 2 || onelevel
}

/// Nome de ramo válido (o `strbuf_check_branch_ref`).
pub fn valid_branch_name(name: &str) -> bool {
    !name.starts_with('-') && name != "HEAD" && check_refname_format(&format!("refs/heads/{name}"), false, false)
}

fn read_target(data: &[u8]) -> Option<Target> {
    if let Some(rest) = data.strip_prefix(b"ref:") {
        let t = crate::object::trim_ascii(rest);
        return Some(Target::Sym(String::from_utf8_lossy(t).into_owned()));
    }
    if data.len() >= 40 {
        let id = Oid::from_hex(&data[..40])?;
        if data.len() == 40 || data[40].is_ascii_whitespace() {
            return Some(Target::Oid(id));
        }
    }
    None
}

impl Repo {
    /// Arquivo de uma ref solta.
    pub fn ref_file(&self, name: &str) -> Vec<u8> {
        if is_per_worktree(name) { self.path(name) } else { self.common(name) }
    }

    fn log_file(&self, name: &str) -> Vec<u8> {
        if is_per_worktree(name) { self.path(&format!("logs/{name}")) } else { self.common(&format!("logs/{name}")) }
    }

    pub fn packed_refs(&self) -> Rc<Vec<PackedRef>> {
        if let Some(rc) = self.packed_cache.borrow().as_ref() {
            return rc.clone();
        }
        let path = self.common("packed-refs");
        let mut out = Vec::new();
        if let Ok(Some(data)) = os::read_opt(&path) {
            for line in data.split(|c| *c == b'\n') {
                if line.is_empty() || line.starts_with(b"#") {
                    continue;
                }
                if let Some(p) = line.strip_prefix(b"^") {
                    if let (Some(last), Some(id)) = (out.last_mut(), Oid::from_hex(crate::object::trim_ascii(p))) {
                        let last: &mut PackedRef = last;
                        last.peeled = Some(id);
                    }
                    continue;
                }
                if line.len() > 41
                    && let Some(id) = Oid::from_hex(&line[..40])
                {
                    out.push(PackedRef { name: String::from_utf8_lossy(&line[41..]).into_owned(), oid: id, peeled: None });
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        let rc = Rc::new(out);
        *self.packed_cache.borrow_mut() = Some(rc.clone());
        rc
    }

    fn invalidate_packed(&self) {
        *self.packed_cache.borrow_mut() = None;
    }

    /// Valor de uma ref, sem seguir a simbólica.
    pub fn read_ref(&self, name: &str) -> R<Option<Target>> {
        let path = self.ref_file(name);
        if let Ok(t) = os::readlink(&path) {
            // Ref simbólica antiga feita com symlink.
            return Ok(Some(Target::Sym(os::lossy(&t))));
        }
        match os::read_opt(&path) {
            Ok(Some(data)) => {
                if let Some(t) = read_target(&data) {
                    return Ok(Some(t));
                }
                if os::is_dir(&path) {
                    return Ok(None);
                }
                return Ok(None);
            }
            Ok(None) => {}
            Err(sysabi::Errno::EISDIR) => {}
            Err(e) => return Err(Fail::Fatal(format!("unable to read ref {name}: {}", e.message()))),
        }
        if name.starts_with("refs/") {
            let packed = self.packed_refs();
            if let Ok(i) = packed.binary_search_by(|p| p.name.as_str().cmp(name)) {
                return Ok(Some(Target::Oid(packed[i].oid)));
            }
        }
        Ok(None)
    }

    /// Segue refs simbólicas: `(nome final, id se existe)`. `None` se nem a primeira existe.
    pub fn resolve_ref(&self, name: &str) -> R<Option<(String, Option<Oid>)>> {
        let mut cur = name.to_string();
        for _ in 0..6 {
            match self.read_ref(&cur)? {
                None => return Ok(if cur == name { None } else { Some((cur, None)) }),
                Some(Target::Oid(id)) => return Ok(Some((cur, Some(id)))),
                Some(Target::Sym(t)) => cur = t,
            }
        }
        Ok(Some((cur, None)))
    }

    pub fn ref_oid(&self, name: &str) -> R<Option<Oid>> {
        Ok(self.resolve_ref(name)?.and_then(|(_, o)| o))
    }

    pub fn head(&self) -> R<Head> {
        match self.read_ref("HEAD")? {
            Some(Target::Sym(t)) => {
                let oid = self.ref_oid(&t)?;
                Ok(Head::Branch(t, oid))
            }
            Some(Target::Oid(id)) => Ok(Head::Detached(id)),
            None => Err(Fail::Fatal("unable to read HEAD".into())),
        }
    }

    pub fn head_oid(&self) -> R<Option<Oid>> {
        Ok(self.head()?.oid())
    }

    /// Ramo atual (`refs/heads/x`), ou `None` com HEAD destacado.
    pub fn current_branch(&self) -> R<Option<String>> {
        Ok(self.head()?.branch().map(str::to_string))
    }

    /// Todas as refs sob `prefix` (soltas e empacotadas), em ordem de nome, já resolvidas.
    pub fn list_refs(&self, prefix: &str) -> R<Vec<(String, Oid)>> {
        let mut map: std::collections::BTreeMap<String, Oid> = std::collections::BTreeMap::new();
        for p in self.packed_refs().iter() {
            if p.name.starts_with(prefix) {
                map.insert(p.name.clone(), p.oid);
            }
        }
        let mut loose = Vec::new();
        self.collect_loose(&self.common_dir.clone(), "refs/", &mut loose);
        if self.git_dir != self.common_dir {
            for sub in ["refs/bisect/", "refs/worktree/", "refs/rewritten/"] {
                self.collect_loose(&self.git_dir.clone(), sub, &mut loose);
            }
        }
        for name in loose {
            if !name.starts_with(prefix) {
                continue;
            }
            match self.resolve_ref(&name)? {
                Some((_, Some(id))) => {
                    map.insert(name, id);
                }
                _ => {
                    map.remove(&name);
                }
            }
        }
        Ok(map.into_iter().collect())
    }

    fn collect_loose(&self, base: &[u8], dir: &str, out: &mut Vec<String>) {
        let path = os::join(base, dir.as_bytes());
        let Ok(entries) = os::read_dir(&path) else { return };
        let mut names: Vec<Vec<u8>> = entries.into_iter().map(|e| e.name).collect();
        names.sort();
        for n in names {
            let name = format!("{dir}{}", String::from_utf8_lossy(&n));
            let full = os::join(&path, &n);
            if os::is_dir(&full) {
                self.collect_loose(base, &format!("{name}/"), out);
            } else if !name.ends_with(".lock") {
                out.push(name);
            }
        }
    }

    /// Ref simbólica? Devolve o alvo.
    pub fn symref_target(&self, name: &str) -> R<Option<String>> {
        Ok(match self.read_ref(name)? {
            Some(Target::Sym(t)) => Some(t),
            _ => None,
        })
    }

    /// Valor descascado (tag anotada -> objeto) guardado no `packed-refs`, se houver.
    pub fn packed_peeled(&self, name: &str) -> Option<Oid> {
        let packed = self.packed_refs();
        packed.binary_search_by(|p| p.name.as_str().cmp(name)).ok().and_then(|i| packed[i].peeled)
    }

    /// Erro de conflito diretório/arquivo ao criar `name`.
    fn df_conflict(&self, name: &str) -> R<()> {
        // Um prefixo de `name` existe como ref?
        let parts: Vec<&str> = name.split('/').collect();
        for i in 1..parts.len() {
            let pre = parts[..i].join("/");
            if pre == "refs" {
                continue;
            }
            if matches!(self.read_ref(&pre)?, Some(_)) && !os::is_dir(&self.ref_file(&pre)) {
                return Err(Fail::Fatal(format!("cannot lock ref '{name}': '{pre}' exists; cannot create '{name}'")));
            }
        }
        // Alguma ref existe abaixo de `name/`?
        let below = format!("{name}/");
        if let Some(r) = self.list_refs(&below)?.first() {
            return Err(Fail::Fatal(format!("cannot lock ref '{name}': '{}' exists; cannot create '{name}'", r.0)));
        }
        Ok(())
    }

    /// Atualiza uma ref (seguindo simbólicas, a menos que `no_deref`), com checagem do valor antigo e
    /// reflog. `old`: `None` = não checa; `Some(None)` = não pode existir; `Some(Some(x))` = tem que
    /// valer x.
    pub fn update_ref(&self, name: &str, new: Oid, old: Option<Option<Oid>>, msg: &str, no_deref: bool) -> R<()> {
        let target = if no_deref {
            name.to_string()
        } else {
            match self.resolve_ref(name)? {
                Some((t, _)) => t,
                None => name.to_string(),
            }
        };
        let current = match self.read_ref(&target)? {
            Some(Target::Oid(id)) => Some(id),
            _ => None,
        };
        if let Some(expect) = old
            && expect != current
        {
            return Err(Fail::Fatal(match (expect, current) {
                (None, Some(_)) => format!("cannot lock ref '{name}': reference already exists"),
                (Some(e), None) => format!("cannot lock ref '{name}': unable to resolve reference '{target}'{}", if e.is_zero() { "" } else { "" }),
                (Some(e), Some(c)) => format!("cannot lock ref '{name}': is at {c} but expected {e}"),
                (None, None) => unreachable!(),
            }));
        }
        if current.is_none() {
            self.df_conflict(&target)?;
        }
        let path = self.ref_file(&target);
        let mut lock = self.lock_ref(&target, &path)?;
        lock.write(format!("{new}\n").as_bytes()).map_err(|e| Fail::Fatal(format!("couldn't write '{}': {}", os::lossy(lock.lock_path()), e.message())))?;
        lock.commit().map_err(|e| Fail::Fatal(format!("couldn't set '{target}': {}", e.message())))?;
        let old_id = current.unwrap_or(Oid::ZERO);
        self.log_ref_update(&target, old_id, new, msg)?;
        // Quem atualiza o ramo do HEAD também deixa entrada no reflog do HEAD.
        if target != "HEAD"
            && (name == "HEAD" && !no_deref || self.symref_target("HEAD")?.as_deref() == Some(target.as_str()))
        {
            self.log_ref_update("HEAD", old_id, new, msg)?;
        }
        Ok(())
    }

    fn lock_ref(&self, name: &str, path: &[u8]) -> R<os::LockFile> {
        if let Err(e) = os::mkdir_parents(path) {
            return Err(Fail::Fatal(format!("cannot lock ref '{name}': unable to create directory for '{}': {}", os::lossy(path), e.message())));
        }
        os::LockFile::acquire(path).map_err(|e| Fail::Fatal(format!("cannot lock ref '{name}': {}", os::lock_error_message(path, e))))
    }

    /// Grava uma ref simbólica (`HEAD` -> `refs/heads/x`).
    pub fn set_symref(&self, name: &str, target: &str, msg: Option<&str>) -> R<()> {
        let old = self.ref_oid(name)?;
        let path = self.ref_file(name);
        let mut lock = self.lock_ref(name, &path)?;
        lock.write(format!("ref: {target}\n").as_bytes()).map_err(|e| Fail::Fatal(e.message().to_string()))?;
        lock.commit().map_err(|e| Fail::Fatal(format!("couldn't set '{name}': {}", e.message())))?;
        if let Some(m) = msg {
            let new = self.ref_oid(target)?;
            if let (Some(o), Some(n)) = (old, new) {
                self.log_ref_update(name, o, n, m)?;
            } else if let Some(n) = new {
                self.log_ref_update(name, Oid::ZERO, n, m)?;
            }
        }
        Ok(())
    }

    /// Grava o HEAD destacado (sem seguir a simbólica).
    pub fn detach_head(&self, id: Oid, msg: &str) -> R<()> {
        self.update_ref("HEAD", id, None, msg, true)
    }

    /// Apaga uma ref (solta e empacotada) e o reflog dela.
    pub fn delete_ref(&self, name: &str, old: Option<Oid>) -> R<()> {
        let current = self.ref_oid(name)?;
        if let Some(e) = old
            && current != Some(e)
        {
            return Err(Fail::Fatal(format!("cannot lock ref '{name}': is at {} but expected {e}", current.unwrap_or(Oid::ZERO))));
        }
        let path = self.ref_file(name);
        let _ = os::unlink(&path);
        let packed = self.packed_refs();
        if packed.iter().any(|p| p.name == name) {
            let ppath = self.common("packed-refs");
            let data = os::read(&ppath).unwrap_or_default();
            let mut out = Vec::new();
            let mut skip_peel = false;
            for line in data.split_inclusive(|c| *c == b'\n') {
                if line.starts_with(b"^") {
                    if !skip_peel {
                        out.extend_from_slice(line);
                    }
                    continue;
                }
                skip_peel = false;
                if line.len() > 41 && crate::object::trim_ascii(&line[41..]) == name.as_bytes() {
                    skip_peel = true;
                    continue;
                }
                out.extend_from_slice(line);
            }
            os::write_locked(&ppath, &out).map_err(Fail::Fatal)?;
            self.invalidate_packed();
        }
        let log = self.log_file(name);
        let _ = os::unlink(&log);
        let base = if is_per_worktree(name) { self.git_dir.clone() } else { self.common_dir.clone() };
        os::remove_empty_parents(os::dirname(&path), &os::join(&base, b"refs"));
        os::remove_empty_parents(os::dirname(&log), &os::join(&base, b"logs/refs"));
        Ok(())
    }

    /// Escreve a pseudo-ref (`ORIG_HEAD`, `MERGE_HEAD`...) direto, sem reflog.
    pub fn write_pseudoref(&self, name: &str, content: &[u8]) -> R<()> {
        os::write_locked(&self.path(name), content).map_err(Fail::Fatal)
    }

    pub fn remove_pseudoref(&self, name: &str) {
        let _ = os::unlink(&self.path(name));
    }

    // ---- reflog -------------------------------------------------------------------------------

    fn should_autocreate_reflog(&self, name: &str) -> bool {
        let mode = self.config.get("core.logallrefupdates").map(|v| v.to_ascii_lowercase());
        match mode.as_deref() {
            Some("always") => true,
            Some(v) if crate::config::parse_bool(Some(v.as_bytes())) == Some(false) => false,
            Some(_) => name == "HEAD" || name.starts_with("refs/heads/") || name.starts_with("refs/remotes/") || name.starts_with("refs/notes/"),
            None => {
                !self.bare && (name == "HEAD" || name.starts_with("refs/heads/") || name.starts_with("refs/remotes/") || name.starts_with("refs/notes/"))
            }
        }
    }

    pub fn has_reflog(&self, name: &str) -> bool {
        os::is_file(&self.log_file(name))
    }

    /// Acrescenta uma linha no reflog (se o reflog existe ou deve ser criado).
    pub fn log_ref_update(&self, name: &str, old: Oid, new: Oid, msg: &str) -> R<()> {
        let path = self.log_file(name);
        if !os::exists(&path) && !self.should_autocreate_reflog(name) {
            return Ok(());
        }
        self.append_reflog(name, old, new, msg, false)
    }

    /// Escreve no reflog mesmo sem a regra de criação automática (o `refs/stash` sempre tem).
    pub fn append_reflog(&self, name: &str, old: Oid, new: Oid, msg: &str, _force: bool) -> R<()> {
        let path = self.log_file(name);
        let who = ident::ident(&self.config, Who::Committer, false)?;
        let line = reflog_line(old, new, &who, msg);
        if let Err(e) = os::mkdir_parents(&path) {
            return Err(Fail::Fatal(format!("unable to create directory for {}: {}", os::lossy(&path), e.message())));
        }
        os::append(&path, &line, 0o666).map_err(|e| Fail::Fatal(format!("unable to append to '{}': {}", os::lossy(&path), e.message())))
    }

    pub fn read_reflog(&self, name: &str) -> Vec<ReflogEntry> {
        let Ok(Some(data)) = os::read_opt(&self.log_file(name)) else { return Vec::new() };
        let mut out = Vec::new();
        for line in data.split(|c| *c == b'\n') {
            if line.len() < 83 {
                continue;
            }
            let (Some(old), Some(new)) = (Oid::from_hex(&line[..40]), Oid::from_hex(&line[41..81])) else { continue };
            let rest = &line[82..];
            let (idpart, msg) = match rest.iter().position(|c| *c == b'\t') {
                Some(t) => (&rest[..t], rest[t + 1..].to_vec()),
                None => (rest, Vec::new()),
            };
            let id = crate::object::parse_ident(idpart).unwrap_or_default();
            out.push(ReflogEntry { old, new, who: id.name_email(), time: id.date.unwrap_or(0), tz: id.tz, message: msg });
        }
        out
    }

    /// Reescreve o reflog inteiro (usado por `stash drop` e `reflog delete`).
    pub fn write_reflog(&self, name: &str, entries: &[ReflogEntry]) -> R<()> {
        let path = self.log_file(name);
        let mut data = Vec::new();
        for e in entries {
            data.extend_from_slice(format!("{} {} ", e.old, e.new).as_bytes());
            data.extend_from_slice(&e.who);
            data.extend_from_slice(format!(" {} {}", e.time, format_tz(e.tz)).as_bytes());
            data.push(b'\t');
            data.extend_from_slice(&e.message);
            data.push(b'\n');
        }
        os::write_locked(&path, &data).map_err(Fail::Fatal)
    }

    /// Renomeia o arquivo de reflog (branch -m).
    pub fn rename_reflog(&self, old: &str, new: &str) -> R<()> {
        let a = self.log_file(old);
        let b = self.log_file(new);
        if os::exists(&a) {
            let _ = os::mkdir_parents(&b);
            os::rename(&a, &b).map_err(|e| Fail::Fatal(format!("unable to move logfile {} to {}: {}", os::lossy(&a), os::lossy(&b), e.message())))?;
            let base = if is_per_worktree(old) { self.git_dir.clone() } else { self.common_dir.clone() };
            os::remove_empty_parents(os::dirname(&a), &os::join(&base, b"logs/refs"));
        }
        Ok(())
    }
}

/// O `copy_reflog_msg`: espaços em sequência viram um, sem espaço nas pontas.
pub fn clean_reflog_msg(msg: &str) -> String {
    let mut out = String::new();
    let mut was_space = true;
    for c in msg.chars() {
        if c.is_ascii_whitespace() {
            if was_space {
                continue;
            }
            was_space = true;
            out.push(' ');
        } else {
            was_space = false;
            out.push(c);
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

pub fn reflog_line(old: Oid, new: Oid, who: &Ident, msg: &str) -> Vec<u8> {
    let mut line = format!("{old} {new} ").into_bytes();
    line.extend_from_slice(&who.to_bytes());
    line.push(b'\t');
    line.extend_from_slice(clean_reflog_msg(msg).as_bytes());
    line.push(b'\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refname_rules() {
        assert!(check_refname_format("refs/heads/main", false, false));
        assert!(!check_refname_format("refs/heads/a..b", false, false));
        assert!(!check_refname_format("refs/heads/a.lock", false, false));
        assert!(!check_refname_format("refs/heads/a b", false, false));
        assert!(!check_refname_format("refs/heads/.a", false, false));
        assert!(!check_refname_format("refs/heads/a@{1}", false, false));
        assert!(!check_refname_format("main", false, false));
        assert!(check_refname_format("main", true, false));
        assert!(valid_branch_name("feature/x"));
        assert!(!valid_branch_name("-x"));
        assert_eq!(clean_reflog_msg("  commit:  a\n b  "), "commit: a b");
    }
}
