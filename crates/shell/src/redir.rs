//! Redireções. Executadas no próprio processo do shell, na ordem do texto; o que muda fica anotado
//! pra desfazer depois do comando (builtins, funções e compostos), como o bash faz. Comandos
//! externos herdam os fds já arrumados no `spawn` (as cópias de segurança são CLOEXEC).

use sysabi::{AtFlags, Errno, Fd, FileType, Mode, OFlags};

use crate::ast::*;
use crate::shell::{Flow, Shell, sys, write_fd};

/// Fds de cópia de segurança ficam a partir daqui (como o bash, que usa 10+).
const SAVE_MIN: i32 = 10;

/// O que desfazer depois do comando.
#[derive(Debug, Default)]
pub struct Undo {
    /// (fd alvo, cópia salva) na ordem em que foram mexidos.
    saved: Vec<(Fd, Option<Fd>)>,
}

impl Undo {
    pub fn is_empty(&self) -> bool {
        self.saved.is_empty()
    }
}

/// Erro de redireção: a mensagem já foi impressa.
pub struct RedirFailed;

impl Shell {
    /// Aplica as redireções. `undo = None` torna permanente (`exec` sem comando).
    pub fn apply_redirects(&mut self, redirs: &[Redirect], mut undo: Option<&mut Undo>) -> Result<Result<(), RedirFailed>, Flow> {
        for r in redirs {
            match self.apply_one(r, undo.as_deref_mut())? {
                Ok(()) => {}
                Err(RedirFailed) => {
                    if let Some(u) = undo {
                        self.undo_redirects(std::mem::take(u));
                    }
                    return Ok(Err(RedirFailed));
                }
            }
        }
        Ok(Ok(()))
    }

    /// Desfaz, na ordem inversa.
    pub fn undo_redirects(&mut self, undo: Undo) {
        let s = sys();
        for (target, saved) in undo.saved.into_iter().rev() {
            match saved {
                Some(copy) => {
                    let _ = s.dup3(copy, target, false);
                    let _ = s.close(copy);
                    self.saved_fds.retain(|f| *f != copy);
                }
                None => {
                    let _ = s.close(target);
                }
            }
        }
    }

    /// Guarda o estado de `fd` antes de mexer.
    fn save_fd(&mut self, fd: Fd, undo: Option<&mut Undo>) {
        let Some(u) = undo else { return };
        if u.saved.iter().any(|(t, _)| *t == fd) {
            return;
        }
        let s = sys();
        match s.dup_min(fd, Fd(SAVE_MIN), true) {
            Ok(copy) => {
                self.saved_fds.push(copy);
                u.saved.push((fd, Some(copy)));
            }
            Err(_) => u.saved.push((fd, None)),
        }
    }

    fn redir_error(&self, what: &[u8], e: Errno) -> RedirFailed {
        let mut msg = what.to_vec();
        msg.extend_from_slice(b": ");
        msg.extend_from_slice(e.message().as_bytes());
        self.error_bytes(&msg);
        RedirFailed
    }

    /// Fd alvo padrão do operador.
    fn default_fd(op: RedirOp) -> i32 {
        match op {
            RedirOp::Read | RedirOp::ReadWrite | RedirOp::DupIn | RedirOp::HereDoc | RedirOp::HereString => 0,
            _ => 1,
        }
    }

    fn target_word(&mut self, r: &Redirect) -> Result<Result<Vec<u8>, RedirFailed>, Flow> {
        match &r.target {
            RedirTarget::Word(w) => match self.expand_redirect_target(w)? {
                Some(v) => Ok(Ok(v)),
                None => {
                    self.error(format!("{}: ambiguous redirect", w.raw));
                    Ok(Err(RedirFailed))
                }
            },
            RedirTarget::ProcSub(ps) => Ok(Ok(self.process_subst(ps)?)),
            RedirTarget::HereDoc(_) => Ok(Ok(Vec::new())),
        }
    }

    /// Instala `newfd` (recém-aberto) no lugar de `target`.
    fn install(&mut self, newfd: Fd, target: Fd, undo: Option<&mut Undo>) -> Result<(), RedirFailed> {
        let s = sys();
        if newfd == target {
            // O open já devolveu o próprio alvo (estava fechado): desfazer é fechar.
            if let Some(u) = undo {
                if !u.saved.iter().any(|(t, _)| *t == target) {
                    u.saved.push((target, None));
                }
            }
            let _ = s.set_cloexec(target, false);
            return Ok(());
        }
        self.save_fd(target, undo);
        let r = s.dup3(newfd, target, false);
        let _ = s.close(newfd);
        r.map(|_| ()).map_err(|e| self.redir_error(target.0.to_string().as_bytes(), e))
    }

    fn open_for(&mut self, path: &[u8], flags: OFlags, mode: Mode) -> Result<Fd, Errno> {
        sys().openat(Fd::CWD, path, flags | OFlags::CLOEXEC, mode)
    }

    fn apply_one(&mut self, r: &Redirect, mut undo: Option<&mut Undo>) -> Result<Result<(), RedirFailed>, Flow> {
        let s = sys();
        // `{nome}>...`: aloca um fd >= 10 e guarda o número na variável (não é desfeito).
        if let RedirFd::Var(name) = &r.fd {
            return self.apply_named(name, r);
        }
        let target = Fd(match r.fd {
            RedirFd::Num(n) => n,
            _ => Self::default_fd(r.op),
        });
        match r.op {
            RedirOp::Read | RedirOp::Write | RedirOp::Append | RedirOp::ReadWrite | RedirOp::Clobber => {
                let path = match self.target_word(r)? {
                    Ok(p) => p,
                    Err(e) => return Ok(Err(e)),
                };
                let fd = match self.open_redirect_file(&path, r.op) {
                    Ok(fd) => fd,
                    Err(e) => return Ok(Err(e)),
                };
                Ok(self.install(fd, target, undo))
            }
            RedirOp::OutErr { append } => {
                let path = match self.target_word(r)? {
                    Ok(p) => p,
                    Err(e) => return Ok(Err(e)),
                };
                let op = if append { RedirOp::Append } else { RedirOp::Write };
                let fd = match self.open_redirect_file(&path, op) {
                    Ok(fd) => fd,
                    Err(e) => return Ok(Err(e)),
                };
                let dup = s.dup_min(fd, Fd(0), true);
                if let Err(e) = self.install(fd, Fd(1), undo.as_deref_mut()) {
                    return Ok(Err(e));
                }
                match dup {
                    Ok(d) => Ok(self.install(d, Fd(2), undo)),
                    Err(e) => Ok(Err(self.redir_error(&path, e))),
                }
            }
            RedirOp::DupIn | RedirOp::DupOut => {
                let word = match self.target_word(r)? {
                    Ok(p) => p,
                    Err(e) => return Ok(Err(e)),
                };
                if word == b"-" {
                    self.save_fd(target, undo);
                    let _ = s.close(target);
                    return Ok(Ok(()));
                }
                let (num, mv) = match word.strip_suffix(b"-") {
                    Some(n) => (n, true),
                    None => (word.as_slice(), false),
                };
                if !num.is_empty() && num.iter().all(|c| c.is_ascii_digit()) {
                    let src: i32 = std::str::from_utf8(num).ok().and_then(|x| x.parse().ok()).unwrap_or(-1);
                    if src < 0 || s.get_cloexec(Fd(src)).is_err() || (self.saved_fds.contains(&Fd(src))) {
                        self.error(format!("{src}: Bad file descriptor"));
                        return Ok(Err(RedirFailed));
                    }
                    if src == target.0 {
                        return Ok(Ok(()));
                    }
                    self.save_fd(target, undo);
                    if let Err(e) = s.dup3(Fd(src), target, false) {
                        return Ok(Err(self.redir_error(src.to_string().as_bytes(), e)));
                    }
                    if mv {
                        let _ = s.close(Fd(src));
                    }
                    return Ok(Ok(()));
                }
                // `>&arquivo` sem número de fd é `&>arquivo`.
                if r.op == RedirOp::DupOut && matches!(r.fd, RedirFd::Default) {
                    let fd = match self.open_redirect_file(&word, RedirOp::Write) {
                        Ok(fd) => fd,
                        Err(e) => return Ok(Err(e)),
                    };
                    let dup = s.dup_min(fd, Fd(0), true);
                    if let Err(e) = self.install(fd, Fd(1), undo.as_deref_mut()) {
                        return Ok(Err(e));
                    }
                    return match dup {
                        Ok(d) => Ok(self.install(d, Fd(2), undo)),
                        Err(e) => Ok(Err(self.redir_error(&word, e))),
                    };
                }
                let raw = match &r.target {
                    RedirTarget::Word(w) => w.raw.to_string(),
                    _ => String::from_utf8_lossy(&word).into_owned(),
                };
                self.error(format!("{raw}: ambiguous redirect"));
                Ok(Err(RedirFailed))
            }
            RedirOp::HereDoc | RedirOp::HereString => {
                let content = match &r.target {
                    RedirTarget::HereDoc(h) => {
                        if h.expand {
                            self.expand_parts_string(&h.parts)?
                        } else {
                            h.body.clone()
                        }
                    }
                    RedirTarget::Word(w) => {
                        let mut v = self.expand_word_string(w)?;
                        v.push(b'\n');
                        v
                    }
                    RedirTarget::ProcSub(_) => Vec::new(),
                };
                match self.here_fd(&content) {
                    Ok(fd) => Ok(self.install(fd, target, undo)),
                    Err(e) => {
                        self.error(format!("cannot create temp file for here-document: {}", e.message()));
                        Ok(Err(RedirFailed))
                    }
                }
            }
        }
    }

    fn open_redirect_file(&mut self, path: &[u8], op: RedirOp) -> Result<Fd, RedirFailed> {
        let s = sys();
        let flags = match op {
            RedirOp::Read => OFlags::RDONLY,
            RedirOp::Write | RedirOp::Clobber => OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC,
            RedirOp::Append => OFlags::WRONLY | OFlags::CREAT | OFlags::APPEND,
            RedirOp::ReadWrite => OFlags::RDWR | OFlags::CREAT,
            _ => OFlags::RDONLY,
        };
        if op == RedirOp::Write && self.opts.get("noclobber") {
            if let Ok(st) = s.fstatat(Fd::CWD, path, AtFlags::empty()) {
                if st.file_type() == FileType::Regular {
                    let mut msg = path.to_vec();
                    if self.dash_style() {
                        // O dash: `sh: N: cannot create ARQ: File exists`.
                        msg.splice(0..0, b"cannot create ".iter().copied());
                        msg.extend_from_slice(b": File exists");
                    } else {
                        msg.extend_from_slice(b": cannot overwrite existing file");
                    }
                    self.error_bytes(&msg);
                    return Err(RedirFailed);
                }
            }
        }
        if path.is_empty() {
            return Err(self.redir_error(b"", Errno::ENOENT));
        }
        match self.open_for(path, flags, 0o666) {
            Ok(fd) => Ok(fd),
            Err(e) => Err(self.redir_error(path, e)),
        }
    }

    /// Conteúdo de here-doc/here-string num fd de leitura: pipe quando cabe, senão arquivo
    /// temporário já apagado.
    fn here_fd(&mut self, content: &[u8]) -> Result<Fd, Errno> {
        let s = sys();
        if content.len() < 65536 {
            let (r, w) = s.pipe2(OFlags::CLOEXEC)?;
            let res = write_fd(w, content);
            let _ = s.close(w);
            res?;
            return Ok(r);
        }
        let mut name = format!("/tmp/sh-thd.{}", s.getpid()).into_bytes();
        let mut rnd = [0u8; 6];
        let _ = s.getrandom(&mut rnd);
        for b in rnd {
            name.push(b"abcdefghijklmnopqrstuvwxyz0123456789"[(b % 36) as usize]);
        }
        let fd = s.openat(Fd::CWD, &name, OFlags::RDWR | OFlags::CREAT | OFlags::EXCL | OFlags::CLOEXEC, 0o600)?;
        let _ = s.unlinkat(Fd::CWD, &name, AtFlags::empty());
        write_fd(fd, content)?;
        s.lseek(fd, 0, sysabi::Whence::Set)?;
        Ok(fd)
    }

    /// `{nome}>arq`, `{nome}<&-`...
    fn apply_named(&mut self, name: &str, r: &Redirect) -> Result<Result<(), RedirFailed>, Flow> {
        let s = sys();
        let base = name.split('[').next().unwrap_or(name);
        if !crate::word::is_name(base.as_bytes()) {
            self.error(format!("{{{name}}}: invalid variable name for redirection"));
            return Ok(Err(RedirFailed));
        }
        let is_close = matches!(r.op, RedirOp::DupIn | RedirOp::DupOut)
            && matches!(&r.target, RedirTarget::Word(w) if &*w.raw == "-");
        if is_close {
            let v = self.var_value_for_redirect(name);
            let n: i32 = v.as_deref().and_then(|x| std::str::from_utf8(x).ok()).and_then(|x| x.trim().parse().ok()).unwrap_or(-1);
            if n < 0 || s.close(Fd(n)).is_err() {
                self.error(format!("{}: Bad file descriptor", String::from_utf8_lossy(&v.unwrap_or_default())));
                return Ok(Err(RedirFailed));
            }
            return Ok(Ok(()));
        }
        // Abre no fd padrão do operador num processo "virtual" e depois move pra >= 10.
        let tmp_target = Fd(Self::default_fd(r.op));
        let mut u = Undo::default();
        let single = Redirect { fd: RedirFd::Num(tmp_target.0), op: r.op, target: r.target.clone() };
        match self.apply_one(&single, Some(&mut u))? {
            Ok(()) => {}
            Err(e) => {
                self.undo_redirects(u);
                return Ok(Err(e));
            }
        }
        let newfd = match s.dup_min(tmp_target, Fd(SAVE_MIN), false) {
            Ok(fd) => fd,
            Err(e) => {
                self.undo_redirects(u);
                return Ok(Err(self.redir_error(name.as_bytes(), e)));
            }
        };
        self.undo_redirects(u);
        let value = newfd.0.to_string().into_bytes();
        let ok = match name.find('[') {
            Some(b) if name.ends_with(']') => {
                let key = name[b + 1..name.len() - 1].as_bytes().to_vec();
                self.assign_element(&name[..b], &key, value, false)?
            }
            _ => self.assign_scalar(name, value, false)?,
        };
        if !ok {
            let _ = s.close(newfd);
            return Ok(Err(RedirFailed));
        }
        Ok(Ok(()))
    }

    fn var_value_for_redirect(&mut self, name: &str) -> Option<Vec<u8>> {
        match name.find('[') {
            Some(b) if name.ends_with(']') => {
                let base = &name[..b];
                let key = &name[b + 1..name.len() - 1];
                let w = crate::word::literal_word(key);
                let pe = ParamExp {
                    name: ParamName::Var(base.to_string()),
                    index: Some(Index::Expr(w)),
                    indirect: false,
                    op: ParamOp::None,
                    braced: true,
                    raw: std::sync::Arc::from(name),
                };
                self.expand_parts_string(&[Part::Param(Box::new(pe))]).ok()
            }
            _ => self.get_scalar(name),
        }
    }
}
