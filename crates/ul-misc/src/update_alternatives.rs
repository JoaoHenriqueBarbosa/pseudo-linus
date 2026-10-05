//! `update-alternatives` do dpkg 1.22 (Debian 13): mantém os grupos de alternativas em
//! `/var/lib/dpkg/alternatives` (arquivos de administração) e os links em `/etc/alternatives`.
//!
//! Comandos: `--install` (com `--slave`), `--remove`, `--remove-all`, `--auto`, `--set`, `--display`,
//! `--query`, `--list`, `--get-selections`, `--set-selections`, `--config` e `--all`. Opções:
//! `--altdir`, `--admindir`, `--instdir`, `--root`, `--log` (aceita e ignorada: o log de alterações
//! não é gravado), `--force`, `--skip-auto`, `--quiet`, `--verbose`, `--debug`, `--help` e `--version`.
//!
//! Formato do arquivo de administração: linha de estado (`auto` ou `manual`), link mestre, pares
//! nome/link de cada escravo, linha vazia, e então cada alternativa (caminho, prioridade e um
//! caminho por escravo, vazio quando falta), terminando em linha vazia.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{AtFlags, Ctx, Fd, FileType, OFlags, RenameFlags, sys};

use crate::util::io;

const PROG: &str = "update-alternatives";

const USAGE: &str = "Usage: update-alternatives [<option> ...] <command>

Commands:
  --install <link> <name> <path> <priority>
    [--slave <link> <name> <path>] ...
                           add a group of alternatives to the system.
  --remove <name> <path>   remove <path> from the <name> group alternative.
  --remove-all <name>      remove <name> group from the alternatives system.
  --auto <name>            switch the master link <name> to automatic mode.
  --display <name>         display information about the <name> group.
  --query <name>           machine parseable version of --display <name>.
  --list <name>            display all targets of the <name> group.
  --get-selections         list master alternative names and their status.
  --set-selections         read alternative status from standard input.
  --config <name>          show alternatives for the <name> group and ask the
                           user to select which one to use.
  --set <name> <path>      set <path> as alternative for <name>.
  --all                    call --config on all alternatives.

<link> is the symlink pointing to /etc/alternatives/<name>.
  (e.g. /usr/bin/pager)
<name> is the master name for this link group.
  (e.g. pager)
<path> is the location of one of the alternative target files.
  (e.g. /usr/bin/less)
<priority> is an integer; options with higher numbers have higher priority in
  automatic mode.

Options:
  --altdir <directory>     change the alternatives directory
                             (default is /etc/alternatives).
  --admindir <directory>   change the administrative directory
                             (default is /var/lib/dpkg/alternatives).
  --instdir <directory>    change the installation directory.
  --root <directory>       change the filesystem root directory.
  --log <file>             change the log file.
  --force                  allow replacing files with alternative links.
  --skip-auto              skip prompt for alternatives correctly configured
                           in automatic mode (relevant for --config only)
  --quiet                  quiet operation, minimal output.
  --verbose                verbose operation, more output.
  --debug                  debug output, way more output.
  --help                   show this help message.
  --version                show the version.
";

const VERSION: &str = "update-alternatives version 1.22.22.

This is free software; see the GNU General Public License version 2 or
later for copying conditions. There is NO warranty.
";

#[derive(Clone)]
struct Slave {
    name: String,
    link: String,
}

#[derive(Clone)]
struct Choice {
    path: String,
    prio: i64,
    /// Um caminho por escravo do grupo, na mesma ordem; vazio quando a alternativa não o tem.
    slaves: Vec<String>,
}

#[derive(Clone)]
struct Alt {
    name: String,
    link: String,
    auto: bool,
    slaves: Vec<Slave>,
    choices: Vec<Choice>,
}

impl Alt {
    fn best(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, c) in self.choices.iter().enumerate() {
            if best.is_none_or(|b| c.prio > self.choices[b].prio) {
                best = Some(i);
            }
        }
        best
    }
}

struct State {
    altdir: String,
    admindir: String,
    root: String,
    inst: String,
    quiet: bool,
    force: bool,
    skip_auto: bool,
}

type R<T> = Result<T, i32>;

fn out(s: &str) {
    let mut o = io::stdout();
    let _ = o.write_all(s.as_bytes());
}

fn error(msg: &str) -> i32 {
    io::eprint(format!("{PROG}: error: {msg}\n"));
    2
}

fn warning(msg: &str) {
    io::eprint(format!("{PROG}: warning: {msg}\n"));
}

fn badusage(msg: &str) -> i32 {
    io::eprint(format!(
        "{PROG}: {msg}\n\nUse '{PROG} --help' for program usage information.\n"
    ));
    2
}

fn ltype(path: &str) -> Option<FileType> {
    sys::lstat(path.as_bytes()).ok().map(|s| s.file_type())
}

fn readlink(path: &str) -> Option<String> {
    sys::current()
        .readlinkat(Fd::CWD, path.as_bytes())
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn rm_if_symlink(path: &str) {
    if ltype(path) == Some(FileType::Symlink) {
        let _ = sys::current().unlinkat(Fd::CWD, path.as_bytes(), AtFlags::empty());
    }
}

/// Cria `path` como link simbólico para `target` de forma atômica (via `.dpkg-tmp`).
fn atomic_symlink(target: &str, path: &str) -> R<()> {
    let tmp = format!("{path}.dpkg-tmp");
    let s = sys::current();
    let _ = s.unlinkat(Fd::CWD, tmp.as_bytes(), AtFlags::empty());
    if let Err(e) = s.symlinkat(target.as_bytes(), Fd::CWD, tmp.as_bytes()) {
        return Err(error(&format!(
            "error creating symbolic link '{tmp}': {}",
            e.message()
        )));
    }
    if let Err(e) = s.renameat2(Fd::CWD, tmp.as_bytes(), Fd::CWD, path.as_bytes(), RenameFlags::empty()) {
        let _ = s.unlinkat(Fd::CWD, tmp.as_bytes(), AtFlags::empty());
        return Err(error(&format!(
            "unable to install '{tmp}' as '{path}': {}",
            e.message()
        )));
    }
    Ok(())
}

impl State {
    fn adm(&self, p: &str) -> String {
        format!("{}{}", self.root, p)
    }

    fn ins(&self, p: &str) -> String {
        format!("{}{}", self.inst, p)
    }

    fn admin_file(&self, name: &str) -> String {
        self.adm(&format!("{}/{}", self.admindir, name))
    }

    fn alt_link(&self, name: &str) -> String {
        format!("{}/{}", self.altdir, name)
    }

    fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = match sys::read_dir(self.adm(&self.admindir).as_bytes()) {
            Ok(es) => es
                .into_iter()
                .map(|e| String::from_utf8_lossy(&e.name).into_owned())
                .filter(|n| !n.ends_with(".dpkg-tmp"))
                .collect(),
            Err(_) => Vec::new(),
        };
        v.sort();
        v
    }

    /// `Ok(None)` quando o grupo não existe.
    fn load(&self, name: &str) -> R<Option<Alt>> {
        let file = self.admin_file(name);
        let data = match sys::read_file(file.as_bytes()) {
            Ok(d) => d,
            Err(sysabi::Errno::ENOENT) => return Ok(None),
            Err(e) => {
                return Err(error(&format!("unable to read '{file}': {}", e.message())));
            }
        };
        let text = String::from_utf8_lossy(&data).into_owned();
        let mut lines: Vec<&str> = text.split('\n').collect();
        if lines.last() == Some(&"") {
            lines.pop();
        }
        let corrupt = |why: &str| Err(error(&format!("{file} corrupt: {why}")));
        let mut it = lines.into_iter();
        let auto = match it.next() {
            Some("auto") => true,
            Some("manual") => false,
            Some(_) => return corrupt("invalid status"),
            None => return corrupt("unexpected end of file"),
        };
        let link = match it.next() {
            Some(l) if !l.is_empty() => l.to_string(),
            _ => return corrupt("missing master link"),
        };
        let mut slaves = Vec::new();
        loop {
            match it.next() {
                None => return corrupt("unexpected end of file"),
                Some("") => break,
                Some(n) => match it.next() {
                    Some(l) if !l.is_empty() => slaves.push(Slave {
                        name: n.to_string(),
                        link: l.to_string(),
                    }),
                    _ => return corrupt("missing slave link"),
                },
            }
        }
        let mut choices = Vec::new();
        loop {
            let path = match it.next() {
                None | Some("") => break,
                Some(p) => p.to_string(),
            };
            let prio = match it.next().map(|p| p.parse::<i64>()) {
                Some(Ok(p)) => p,
                _ => return corrupt("invalid priority"),
            };
            let mut sl = Vec::new();
            for _ in 0..slaves.len() {
                match it.next() {
                    Some(s) => sl.push(s.to_string()),
                    None => return corrupt("unexpected end of file"),
                }
            }
            choices.push(Choice { path, prio, slaves: sl });
        }
        Ok(Some(Alt {
            name: name.to_string(),
            link,
            auto,
            slaves,
            choices,
        }))
    }

    fn save(&self, a: &Alt) -> R<()> {
        let mut s = String::new();
        s.push_str(if a.auto { "auto\n" } else { "manual\n" });
        s.push_str(&a.link);
        s.push('\n');
        for sl in &a.slaves {
            s.push_str(&format!("{}\n{}\n", sl.name, sl.link));
        }
        s.push('\n');
        for c in &a.choices {
            s.push_str(&format!("{}\n{}\n", c.path, c.prio));
            for p in &c.slaves {
                s.push_str(p);
                s.push('\n');
            }
        }
        s.push('\n');
        let file = self.admin_file(&a.name);
        let tmp = format!("{file}.dpkg-tmp");
        let fd = sys::open(
            tmp.as_bytes(),
            OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC,
            0o644,
        )
        .map_err(|e| error(&format!("unable to create '{tmp}': {}", e.message())))?;
        let w = sys::write_all(fd, s.as_bytes());
        let _ = sys::close(fd);
        if let Err(e) = w {
            return Err(error(&format!("unable to write '{tmp}': {}", e.message())));
        }
        sys::current()
            .renameat2(Fd::CWD, tmp.as_bytes(), Fd::CWD, file.as_bytes(), RenameFlags::empty())
            .map_err(|e| error(&format!("unable to install '{tmp}' as '{file}': {}", e.message())))
    }

    fn current(&self, a: &Alt) -> Option<String> {
        readlink(&self.adm(&self.alt_link(&a.name)))
    }

    fn can_replace(&self, path: &str) -> bool {
        match ltype(path) {
            None | Some(FileType::Symlink) => true,
            _ => self.force,
        }
    }

    /// O grupo está de acordo com o que a alternativa `c` exige?
    fn is_broken(&self, a: &Alt, c: &Choice) -> bool {
        let alt = self.alt_link(&a.name);
        if readlink(&self.adm(&alt)).as_deref() != Some(c.path.as_str()) {
            return true;
        }
        if readlink(&self.ins(&a.link)).as_deref() != Some(alt.as_str()) {
            return true;
        }
        for (i, sl) in a.slaves.iter().enumerate() {
            let target = &c.slaves[i];
            let salt = self.alt_link(&sl.name);
            if target.is_empty() || ltype(&self.ins(target)).is_none() {
                if ltype(&self.ins(&sl.link)).is_some_and(|t| t == FileType::Symlink) {
                    return true;
                }
                continue;
            }
            if readlink(&self.adm(&salt)).as_deref() != Some(target.as_str())
                || readlink(&self.ins(&sl.link)).as_deref() != Some(salt.as_str())
            {
                return true;
            }
        }
        false
    }

    /// Aponta o grupo para a alternativa `idx`: atualiza `altdir`, o link mestre e os escravos.
    fn select(&self, a: &Alt, idx: usize, announce: bool) -> R<()> {
        let c = &a.choices[idx];
        let alt = self.alt_link(&a.name);
        atomic_symlink(&c.path, &self.adm(&alt))?;
        if announce && !self.quiet {
            out(&format!(
                "{PROG}: using {} to provide {} ({}) in {} mode\n",
                c.path,
                a.link,
                a.name,
                if a.auto { "auto" } else { "manual" }
            ));
        }
        let master = self.ins(&a.link);
        if self.can_replace(&master) {
            atomic_symlink(&alt, &master)?;
        } else {
            warning(&format!("not replacing {} with a link", a.link));
        }
        for (i, sl) in a.slaves.iter().enumerate() {
            let target = &c.slaves[i];
            let salt = self.alt_link(&sl.name);
            let slink = self.ins(&sl.link);
            if target.is_empty() {
                rm_if_symlink(&self.adm(&salt));
                rm_if_symlink(&slink);
                continue;
            }
            if ltype(&self.ins(target)).is_none() && sys::stat(self.ins(target).as_bytes()).is_err() {
                warning(&format!(
                    "skip creation of {} because associated file {} (of link group {}) doesn't exist",
                    sl.link, target, a.name
                ));
                rm_if_symlink(&self.adm(&salt));
                rm_if_symlink(&slink);
                continue;
            }
            atomic_symlink(target, &self.adm(&salt))?;
            if self.can_replace(&slink) {
                atomic_symlink(&salt, &slink)?;
            } else {
                warning(&format!("not replacing {} with a link", sl.link));
            }
        }
        Ok(())
    }

    fn remove_links(&self, a: &Alt) {
        rm_if_symlink(&self.ins(&a.link));
        rm_if_symlink(&self.adm(&self.alt_link(&a.name)));
        for sl in &a.slaves {
            rm_if_symlink(&self.ins(&sl.link));
            rm_if_symlink(&self.adm(&self.alt_link(&sl.name)));
        }
    }

    fn delete_group(&self, a: &Alt) {
        self.remove_links(a);
        let _ = sys::current().unlinkat(Fd::CWD, self.admin_file(&a.name).as_bytes(), AtFlags::empty());
    }

    fn need(&self, name: &str) -> R<Alt> {
        match self.load(name)? {
            Some(a) => Ok(a),
            None => Err(error(&format!("no alternatives for {name}"))),
        }
    }

    fn display(&self, name: &str) -> R<()> {
        let a = self.need(name)?;
        let mut s = format!("{} - {} mode\n", a.name, if a.auto { "auto" } else { "manual" });
        match a.best() {
            Some(b) => s.push_str(&format!("  link best version is {}\n", a.choices[b].path)),
            None => s.push_str("  link best version not available\n"),
        }
        match self.current(&a) {
            Some(c) => s.push_str(&format!("  link currently points to {c}\n")),
            None => s.push_str("  link currently absent\n"),
        }
        s.push_str(&format!("  link {} is {}\n", a.name, a.link));
        for sl in &a.slaves {
            s.push_str(&format!("  slave {} is {}\n", sl.name, sl.link));
        }
        for c in &a.choices {
            s.push_str(&format!("{} - priority {}\n", c.path, c.prio));
            for (i, sl) in a.slaves.iter().enumerate() {
                if !c.slaves[i].is_empty() {
                    s.push_str(&format!("  slave {}: {}\n", sl.name, c.slaves[i]));
                }
            }
        }
        out(&s);
        Ok(())
    }

    fn query(&self, name: &str) -> R<()> {
        let a = self.need(name)?;
        let mut s = format!("Name: {}\nLink: {}\n", a.name, a.link);
        if !a.slaves.is_empty() {
            s.push_str("Slaves:\n");
            for sl in &a.slaves {
                s.push_str(&format!(" {} {}\n", sl.name, sl.link));
            }
        }
        s.push_str(&format!("Status: {}\n", if a.auto { "auto" } else { "manual" }));
        if let Some(b) = a.best() {
            s.push_str(&format!("Best: {}\n", a.choices[b].path));
        }
        match self.current(&a) {
            Some(c) => s.push_str(&format!("Value: {c}\n")),
            None => s.push_str("Value: none\n"),
        }
        for c in &a.choices {
            s.push_str(&format!("\nAlternative: {}\nPriority: {}\n", c.path, c.prio));
            if !a.slaves.is_empty() {
                s.push_str("Slaves:\n");
                for (i, sl) in a.slaves.iter().enumerate() {
                    if !c.slaves[i].is_empty() {
                        s.push_str(&format!(" {} {}\n", sl.name, c.slaves[i]));
                    }
                }
            }
        }
        out(&s);
        Ok(())
    }

    fn list(&self, name: &str) -> R<()> {
        let a = self.need(name)?;
        let mut s = String::new();
        for c in &a.choices {
            s.push_str(&c.path);
            s.push('\n');
        }
        out(&s);
        Ok(())
    }

    fn get_selections(&self) -> R<()> {
        let mut s = String::new();
        for n in self.names() {
            if let Some(a) = self.load(&n)? {
                let cur = self.current(&a).unwrap_or_default();
                s.push_str(&format!(
                    "{:<30} {:<8} {}\n",
                    a.name,
                    if a.auto { "auto" } else { "manual" },
                    cur
                ));
            }
        }
        out(&s);
        Ok(())
    }

    fn set_auto(&self, name: &str) -> R<()> {
        let mut a = self.need(name)?;
        a.auto = true;
        self.save(&a)?;
        if let Some(b) = a.best() {
            self.select(&a, b, true)?;
        }
        Ok(())
    }

    fn set_manual(&self, name: &str, path: &str) -> R<()> {
        let mut a = self.need(name)?;
        let Some(i) = a.choices.iter().position(|c| c.path == path) else {
            return Err(error(&format!(
                "alternative {path} for {name} not registered; not setting"
            )));
        };
        a.auto = false;
        self.save(&a)?;
        self.select(&a, i, true)
    }

    fn set_selections(&self) -> R<()> {
        let data = io::read_stdin().unwrap_or_default();
        let text = String::from_utf8_lossy(&data).into_owned();
        for line in text.lines() {
            let mut t = line.split_whitespace();
            let (Some(name), Some(status)) = (t.next(), t.next()) else {
                continue;
            };
            if self.load(name)?.is_none() {
                warning(&format!("skip updating {name} as it doesn't exist"));
                continue;
            }
            if status == "auto" {
                self.set_auto(name)?;
            } else if status == "manual" {
                let path: Vec<&str> = t.collect();
                let path = path.join(" ");
                self.set_manual(name, &path)?;
            }
        }
        Ok(())
    }

    fn remove(&self, name: &str, path: &str) -> R<()> {
        let Some(mut a) = self.load(name)? else {
            return Ok(());
        };
        let Some(i) = a.choices.iter().position(|c| c.path == path) else {
            return Ok(());
        };
        let cur = self.current(&a);
        a.choices.remove(i);
        if a.choices.is_empty() {
            self.delete_group(&a);
            return Ok(());
        }
        if cur.as_deref() == Some(path) {
            a.auto = true;
            self.save(&a)?;
            if let Some(b) = a.best() {
                self.select(&a, b, true)?;
            }
        } else {
            self.save(&a)?;
        }
        Ok(())
    }

    fn remove_all(&self, name: &str) -> R<()> {
        if let Some(a) = self.load(name)? {
            self.delete_group(&a);
        }
        Ok(())
    }

    fn check_name(&self, n: &str) -> R<()> {
        if n.contains('/') || n.contains(' ') {
            return Err(error(&format!(
                "alternative name ({n}) must not contain '/' and spaces"
            )));
        }
        Ok(())
    }

    fn check_abs(&self, what: &str, p: &str) -> R<()> {
        if !p.starts_with('/') {
            return Err(error(&format!(
                "alternative {what} is not absolute as it should be: {p}"
            )));
        }
        Ok(())
    }

    fn install(&self, link: &str, name: &str, path: &str, prio: i64, slaves: &[(String, String, String)]) -> R<()> {
        self.check_name(name)?;
        self.check_abs("link", link)?;
        self.check_abs("path", path)?;
        if sys::stat(self.ins(path).as_bytes()).is_err() {
            return Err(error(&format!("alternative path {path} doesn't exist")));
        }
        for (sl, sn, sp) in slaves {
            self.check_name(sn)?;
            self.check_abs("link", sl)?;
            self.check_abs("path", sp)?;
        }
        // Conflitos com outros grupos.
        for other in self.names() {
            if other == name {
                continue;
            }
            let Some(o) = self.load(&other)? else { continue };
            if o.link == link {
                return Err(error(&format!(
                    "alternative link {link} is already managed by {}",
                    o.name
                )));
            }
            if o.slaves.iter().any(|s| s.link == link) {
                return Err(error(&format!(
                    "alternative link {link} is already managed by {} (slave of {}).",
                    o.slaves.iter().find(|s| s.link == link).map(|s| s.name.as_str()).unwrap_or(""),
                    o.name
                )));
            }
            if o.slaves.iter().any(|s| s.name == name) {
                return Err(error(&format!(
                    "alternative {name} can't be master: it is a slave of {}",
                    o.name
                )));
            }
            for (sl, sn, _) in slaves {
                if o.name == *sn {
                    return Err(error(&format!(
                        "alternative {sn} can't be slave of {name}: it is a master alternative."
                    )));
                }
                if o.link == *sl {
                    return Err(error(&format!(
                        "alternative link {sl} is already managed by {}.",
                        o.name
                    )));
                }
                if let Some(x) = o.slaves.iter().find(|x| x.link == *sl) {
                    return Err(error(&format!(
                        "alternative link {sl} is already managed by {} (slave of {}).",
                        x.name, o.name
                    )));
                }
            }
        }

        let existing = self.load(name)?;
        let is_new = existing.is_none();
        let mut a = existing.unwrap_or(Alt {
            name: name.to_string(),
            link: link.to_string(),
            auto: true,
            slaves: Vec::new(),
            choices: Vec::new(),
        });
        if a.link != link {
            // O link mestre mudou: o antigo deixa de ser gerenciado.
            rm_if_symlink(&self.ins(&a.link));
            a.link = link.to_string();
        }
        for (sl, sn, _) in slaves {
            match a.slaves.iter_mut().find(|s| s.name == *sn) {
                Some(s) => {
                    if s.link != *sl {
                        rm_if_symlink(&self.ins(&s.link));
                        s.link = sl.clone();
                    }
                }
                None => {
                    a.slaves.push(Slave {
                        name: sn.clone(),
                        link: sl.clone(),
                    });
                    for c in &mut a.choices {
                        c.slaves.push(String::new());
                    }
                }
            }
        }
        let mut sl_paths: Vec<String> = Vec::new();
        for s in &a.slaves {
            let p = slaves
                .iter()
                .find(|(_, sn, _)| *sn == s.name)
                .map(|(_, _, sp)| sp.clone())
                .unwrap_or_default();
            sl_paths.push(p);
        }
        let idx = match a.choices.iter().position(|c| c.path == path) {
            Some(i) => {
                a.choices[i].prio = prio;
                a.choices[i].slaves = sl_paths;
                i
            }
            None => {
                a.choices.push(Choice {
                    path: path.to_string(),
                    prio,
                    slaves: sl_paths,
                });
                a.choices.len() - 1
            }
        };
        let _ = idx;

        let cur = self.current(&a);
        if a.auto {
            let b = a.best().unwrap_or(0);
            let bc = &a.choices[b];
            let changed = cur.as_deref() != Some(bc.path.as_str());
            let broken = !changed && self.is_broken(&a, bc);
            if broken && !is_new {
                warning(&format!(
                    "forcing reinstallation of alternative {} because link group {} is broken",
                    bc.path, a.name
                ));
            }
            self.save(&a)?;
            self.select(&a, b, changed || broken)?;
        } else {
            let known = cur
                .as_deref()
                .and_then(|c| a.choices.iter().position(|x| x.path == c));
            match known {
                Some(i) => {
                    self.save(&a)?;
                    self.select(&a, i, false)?;
                }
                None => {
                    let b = a.best().unwrap_or(0);
                    warning(&format!(
                        "current alternative {} is unknown, switching to {} for link group {}",
                        cur.as_deref().unwrap_or(""),
                        a.choices[b].path,
                        a.name
                    ));
                    a.auto = true;
                    self.save(&a)?;
                    self.select(&a, b, true)?;
                }
            }
        }
        Ok(())
    }

    fn config(&self, name: &str, input: &mut Input) -> R<()> {
        let mut a = self.need(name)?;
        let n = a.choices.len();
        if n == 0 {
            out(&format!("No alternatives for {name}.\n"));
            return Ok(());
        }
        let cur = self.current(&a);
        let best = a.best().unwrap_or(0);
        let width = a.choices.iter().map(|c| c.path.len()).max().unwrap_or(0).max(4);
        let mut s = format!(
            "There {} {n} choice{} for the alternative {} (providing {}).\n\n",
            if n == 1 { "is" } else { "are" },
            if n == 1 { "" } else { "s" },
            a.name,
            a.link
        );
        s.push_str(&format!(
            "  {:<12.12} {:<w1$.w1$} {:<10.10} {}\n",
            "Selection", "Path", "Priority", "Status",
            w1 = width + 1
        ));
        s.push_str("------------------------------------------------------------\n");
        let w2 = width + 2;
        let mark0 = if a.auto { '*' } else { ' ' };
        s.push_str(&format!(
            "{mark0} {:<12} {:<w2$} {:<9} auto mode\n",
            0, a.choices[best].path, a.choices[best].prio
        ));
        for (i, c) in a.choices.iter().enumerate() {
            let mark = if !a.auto && cur.as_deref() == Some(c.path.as_str()) { '*' } else { ' ' };
            s.push_str(&format!(
                "{mark} {:<12} {:<w2$} {:<9} manual mode\n",
                i + 1,
                c.path,
                c.prio
            ));
        }
        s.push('\n');
        out(&s);
        loop {
            out("Press <enter> to keep the current choice[*], or type selection number: ");
            let Some(line) = input.line() else { return Ok(()) };
            let line = line.trim();
            if line.is_empty() {
                return Ok(());
            }
            match line.parse::<usize>() {
                Ok(0) => {
                    a.auto = true;
                    self.save(&a)?;
                    return self.select(&a, best, true);
                }
                Ok(k) if k <= n => {
                    a.auto = false;
                    self.save(&a)?;
                    return self.select(&a, k - 1, true);
                }
                _ => {}
            }
        }
    }

    fn config_all(&self, input: &mut Input) -> R<()> {
        for n in self.names() {
            if let Some(a) = self.load(&n)? {
                if self.skip_auto && a.auto && a.choices.len() > 0 {
                    let cur = self.current(&a);
                    if let Some(b) = a.best() {
                        if cur.as_deref() == Some(a.choices[b].path.as_str()) {
                            continue;
                        }
                    }
                }
                self.config(&n, input)?;
            }
        }
        Ok(())
    }
}

/// Leitor de linhas do stdin (lido por inteiro na primeira chamada).
struct Input {
    data: Option<Vec<u8>>,
    pos: usize,
}

impl Input {
    fn line(&mut self) -> Option<String> {
        if self.data.is_none() {
            self.data = Some(io::read_stdin().unwrap_or_default());
        }
        let d = self.data.as_ref()?;
        if self.pos >= d.len() {
            return None;
        }
        let rest = &d[self.pos..];
        let end = rest.iter().position(|&b| b == b'\n');
        let (l, adv) = match end {
            Some(e) => (&rest[..e], e + 1),
            None => (rest, rest.len()),
        };
        let s = String::from_utf8_lossy(l).into_owned();
        self.pos += adv;
        Some(s)
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    match run_inner(args) {
        Ok(()) => 0,
        Err(c) => c,
    }
}

fn run_inner(args: &[OsString]) -> R<()> {
    let argv = io::args_bytes(args);
    let argv: Vec<String> = argv
        .iter()
        .skip(1)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect();

    let mut altdir = "/etc/alternatives".to_string();
    let mut admindir = "/var/lib/dpkg/alternatives".to_string();
    let mut root = String::new();
    let mut inst = String::new();
    let mut st_quiet = false;
    let mut force = false;
    let mut skip_auto = false;

    let mut action: Option<String> = None;
    let mut aargs: Vec<String> = Vec::new();
    let mut inst_prio = 0i64;
    let mut slaves: Vec<(String, String, String)> = Vec::new();

    let mut i = 0;
    while i < argv.len() {
        let a = argv[i].clone();
        i += 1;
        let name = match a.strip_prefix("--") {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => return Err(badusage(&format!("unknown argument `{a}'"))),
        };
        let mut take = |what: &str| -> R<String> {
            if i < argv.len() {
                i += 1;
                Ok(argv[i - 1].clone())
            } else {
                Err(badusage(&format!("--{name} needs a <{what}> argument")))
            }
        };
        match name.as_str() {
            "help" => {
                out(USAGE);
                return Ok(());
            }
            "version" => {
                out(VERSION);
                return Ok(());
            }
            "altdir" => altdir = take("directory")?,
            "admindir" => admindir = take("directory")?,
            "instdir" => inst = take("directory")?,
            "root" => {
                root = take("directory")?;
                inst = root.clone();
            }
            "log" => {
                let _ = take("file")?;
            }
            "force" => force = true,
            "skip-auto" => skip_auto = true,
            "quiet" => st_quiet = true,
            "verbose" | "debug" => {}
            "install" | "remove" | "remove-all" | "auto" | "display" | "query" | "list" | "set"
            | "config" | "get-selections" | "set-selections" | "all" => {
                if let Some(prev) = &action {
                    return Err(badusage(&format!(
                        "two commands specified: --{prev} and --{name}"
                    )));
                }
                let need = match name.as_str() {
                    "install" => 4,
                    "remove" | "set" => 2,
                    "remove-all" | "auto" | "display" | "query" | "list" | "config" => 1,
                    _ => 0,
                };
                if i + need > argv.len() {
                    let usage = match name.as_str() {
                        "install" => "<link> <name> <path> <priority>",
                        "remove" | "set" => "<name> <path>",
                        _ => "<name>",
                    };
                    return Err(badusage(&format!("--{name} needs {usage}")));
                }
                aargs = argv[i..i + need].to_vec();
                i += need;
                action = Some(name.clone());
            }
            "slave" => {
                if action.as_deref() != Some("install") {
                    return Err(badusage("--slave only allowed with --install"));
                }
                if i + 3 > argv.len() {
                    return Err(badusage("--slave needs <link> <name> <path>"));
                }
                slaves.push((argv[i].clone(), argv[i + 1].clone(), argv[i + 2].clone()));
                i += 3;
            }
            _ => return Err(badusage(&format!("unknown option '{a}'"))),
        }
    }
    let Some(action) = action else {
        return Err(badusage(
            "need --display, --query, --list, --get-selections, --config, --set, --set-selections, --install, --remove, --all, --remove-all or --auto",
        ));
    };
    if action == "install" {
        match aargs[3].parse::<i64>() {
            Ok(p) => inst_prio = p,
            Err(_) => return Err(badusage("priority must be an integer")),
        }
    }

    let st = State {
        altdir,
        admindir,
        root,
        inst,
        quiet: st_quiet,
        force,
        skip_auto,
    };
    let mut input = Input { data: None, pos: 0 };
    match action.as_str() {
        "install" => st.install(&aargs[0], &aargs[1], &aargs[2], inst_prio, &slaves),
        "remove" => st.remove(&aargs[0], &aargs[1]),
        "remove-all" => st.remove_all(&aargs[0]),
        "auto" => st.set_auto(&aargs[0]),
        "display" => st.display(&aargs[0]),
        "query" => st.query(&aargs[0]),
        "list" => st.list(&aargs[0]),
        "set" => st.set_manual(&aargs[0], &aargs[1]),
        "config" => st.config(&aargs[0], &mut input),
        "all" => st.config_all(&mut input),
        "get-selections" => st.get_selections(),
        "set-selections" => st.set_selections(),
        _ => Ok(()),
    }
}
