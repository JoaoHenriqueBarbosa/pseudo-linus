//! Um VFS de brinquedo com o namespace do sandbox (`/etc/passwd` próprio, `/tmp`, e o hostfs montado em
//! `/work`) e quatro formas de implementar o hostfs:
//!
//! - **cap-std**: delega o resto do caminho pro `cap_std::fs::Dir::open`;
//! - **openat2 BENEATH** e **openat2 IN_ROOT**: delega pro kernel com `RESOLVE_BENEATH` ou
//!   `RESOLVE_IN_ROOT`, sempre com `RESOLVE_NO_MAGICLINKS`;
//! - **namei manual**: o nosso VFS resolve componente por componente; o hostfs só faz lookup de um nome
//!   num diretório (`openat` com `O_PATH | O_NOFOLLOW`) e devolve symlink como symlink, pra que o alvo seja
//!   resolvido no namespace do sandbox.
//!
//! Nos três primeiros, quem resolve symlink e `..` dentro da montagem é o kernel do host, com a semântica
//! dele. No manual, é o nosso namei, com a semântica do sandbox.

use std::collections::VecDeque;
use std::io::Read;
use std::os::fd::OwnedFd;
use std::rc::Rc;

use rustix::fs::{FileType, Mode, OFlags, ResolveFlags};
use rustix::io::Errno;

pub const PATH_MAX: usize = 4096;
pub const NAME_MAX: usize = 255;
pub const MAXSYMLINKS: usize = 40;

/// Resultado de "abrir e ler um arquivo" por um caminho visto de dentro do sandbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Content(Vec<u8>),
    Errno(i32),
    /// Erro sem errno do sistema (ex.: o erro próprio do cap-std pra tentativa de fuga).
    Opaque(String),
}

impl Outcome {
    pub fn label(&self) -> String {
        match self {
            Outcome::Content(c) => format!("conteúdo {:?}", String::from_utf8_lossy(c).trim_end()),
            Outcome::Errno(e) => errno_name(*e).to_string(),
            Outcome::Opaque(s) => format!("erro sem errno: {s}"),
        }
    }
}

pub fn errno_name(e: i32) -> &'static str {
    match e {
        1 => "EPERM",
        2 => "ENOENT",
        13 => "EACCES",
        18 => "EXDEV",
        20 => "ENOTDIR",
        21 => "EISDIR",
        22 => "EINVAL",
        36 => "ENAMETOOLONG",
        40 => "ELOOP",
        11 => "EAGAIN",
        _ => "E?",
    }
}

fn from_errno(e: Errno) -> Outcome {
    Outcome::Errno(e.raw_os_error())
}

fn from_io(e: std::io::Error) -> Outcome {
    match e.raw_os_error() {
        Some(n) => Outcome::Errno(n),
        None => Outcome::Opaque(e.to_string()),
    }
}

fn read_fd(fd: OwnedFd) -> Outcome {
    let mut f = std::fs::File::from(fd);
    let mut buf = Vec::new();
    match f.read_to_end(&mut buf) {
        Ok(_) => Outcome::Content(buf),
        Err(e) => from_io(e),
    }
}

fn join_rest(first: &str, queue: &VecDeque<String>, trailing_slash: bool) -> String {
    let mut joined = first.to_string();
    for c in queue {
        joined.push('/');
        joined.push_str(c);
    }
    if trailing_slash {
        joined.push('/');
    }
    joined
}

fn components(path: &str) -> VecDeque<String> {
    path.split('/').filter(|c| !c.is_empty()).map(str::to_string).collect()
}

/// Como o hostfs é implementado.
pub enum HostFs {
    CapStd(cap_std::fs::Dir),
    Openat2 { root: OwnedFd, resolve: ResolveFlags },
    Manual { root: OwnedFd },
    /// Manual com caminho rápido: tenta o resto do caminho numa syscall com
    /// `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS`; se o kernel recusar por achar
    /// symlink (ELOOP) ou `..` acima do ponto de partida (EXDEV), segue componente a componente.
    Hybrid { root: OwnedFd },
}

impl HostFs {
    pub fn name(&self) -> String {
        match self {
            HostFs::CapStd(_) => "cap-std".into(),
            HostFs::Openat2 { resolve, .. } if resolve.contains(ResolveFlags::IN_ROOT) => "openat2 IN_ROOT".into(),
            HostFs::Openat2 { .. } => "openat2 BENEATH".into(),
            HostFs::Manual { .. } => "namei manual (O_PATH|O_NOFOLLOW)".into(),
            HostFs::Hybrid { .. } => "namei híbrido (openat2 NO_SYMLINKS + manual)".into(),
        }
    }

    /// O nosso namei resolve symlink e `..` (manual e híbrido); os outros delegam ao kernel do host.
    fn own_namei(&self) -> bool {
        matches!(self, HostFs::Manual { .. } | HostFs::Hybrid { .. })
    }

    /// Delegação: o resto do caminho, a partir da raiz da montagem, vai inteiro pro candidato.
    fn delegate(&self, rest: &str) -> Outcome {
        match self {
            HostFs::CapStd(dir) => match dir.open(rest) {
                Ok(f) => {
                    let mut f = f.into_std();
                    let mut buf = Vec::new();
                    match f.read_to_end(&mut buf) {
                        Ok(_) => Outcome::Content(buf),
                        Err(e) => from_io(e),
                    }
                }
                Err(e) => from_io(e),
            },
            HostFs::Openat2 { root, resolve } => {
                match rustix::fs::openat2(root, rest, OFlags::RDONLY | OFlags::CLOEXEC, Mode::empty(), *resolve) {
                    Ok(fd) => read_fd(fd),
                    Err(e) => from_errno(e),
                }
            }
            HostFs::Manual { .. } | HostFs::Hybrid { .. } => unreachable!("namei próprio não delega"),
        }
    }
}

/// Posição corrente da resolução.
enum Pos {
    /// No tmpfs do sandbox (`etc`, `tmp`), caminho canônico.
    Guest(Vec<String>),
    /// Dentro da montagem: pilha de fds `O_PATH` (o primeiro é a raiz da montagem), um por nível. `..` só
    /// desempilha: é o diretório pelo qual a resolução realmente passou, como o dentry pai do Linux.
    Host(Vec<Rc<OwnedFd>>),
}

enum Looked {
    Dir(Pos),
    File(Box<dyn FnOnce() -> Outcome>),
    Symlink(String),
}

/// O sandbox: tmpfs mínimo + hostfs montado em /work. cwd = /work.
pub struct Sandbox {
    pub host: HostFs,
    passwd: Vec<u8>,
    /// fd da raiz da montagem, compartilhado por todas as resoluções (sem dup por chamada).
    mount_fd: Rc<OwnedFd>,
}

impl Sandbox {
    pub fn new(host: HostFs, passwd: &[u8]) -> Sandbox {
        let mount_fd = match &host {
            HostFs::Manual { root } | HostFs::Hybrid { root } | HostFs::Openat2 { root, .. } => {
                root.try_clone().expect("dup da raiz")
            }
            HostFs::CapStd(dir) => dir.try_clone().expect("dup").into_std_file().into(),
        };
        Sandbox { host, passwd: passwd.to_vec(), mount_fd: Rc::new(mount_fd) }
    }

    /// Abre e lê `path` como um processo dentro do sandbox faria (`open(O_RDONLY)` seguindo symlinks + read).
    pub fn read(&self, path: &str) -> Outcome {
        if path.is_empty() {
            return Outcome::Errno(2);
        }
        if path.len() >= PATH_MAX {
            return Outcome::Errno(36);
        }
        let trailing_slash = path.ends_with('/');
        let mut queue = components(path);
        let mut pos = if path.starts_with('/') { Pos::Guest(Vec::new()) } else { self.mount_root() };
        let mut links = 0;
        // O caminho rápido do híbrido é tentado uma vez por trecho: no começo e depois de cada symlink.
        let mut fast_allowed = true;
        loop {
            let Some(c) = queue.pop_front() else {
                // Caminho terminou num diretório: ler um diretório dá EISDIR.
                return Outcome::Errno(21);
            };
            if c.len() > NAME_MAX {
                return Outcome::Errno(36);
            }
            if c == "." {
                continue;
            }
            if c == ".." {
                pos = self.parent(pos);
                continue;
            }
            // Delegação: ao entrar na montagem, o resto vai inteiro pro candidato.
            if let (Pos::Host(_), false) = (&pos, self.host.own_namei()) {
                return self.host.delegate(&join_rest(&c, &queue, trailing_slash));
            }
            // Caminho rápido do híbrido: uma syscall quando não há symlink nem `..` pra cima no resto.
            if let (Pos::Host(stack), HostFs::Hybrid { .. }, true) = (&pos, &self.host, fast_allowed) {
                fast_allowed = false;
                let rest = join_rest(&c, &queue, trailing_slash);
                let fast = rustix::fs::openat2(
                    stack.last().expect("pilha não vazia").as_ref(),
                    rest.as_str(),
                    OFlags::RDONLY | OFlags::CLOEXEC,
                    Mode::empty(),
                    ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
                );
                match fast {
                    Ok(fd) => return read_fd(fd),
                    Err(e) if e == Errno::LOOP || e == Errno::XDEV => {}
                    Err(e) => return from_errno(e),
                }
            }
            let last = queue.is_empty();
            let looked = match self.lookup(&pos, &c) {
                Ok(l) => l,
                Err(o) => return o,
            };
            match looked {
                Looked::Symlink(target) => {
                    links += 1;
                    if links > MAXSYMLINKS {
                        return Outcome::Errno(40);
                    }
                    if target.is_empty() {
                        return Outcome::Errno(2);
                    }
                    if target.starts_with('/') {
                        pos = Pos::Guest(Vec::new());
                    }
                    let mut next = components(&target);
                    next.extend(queue.drain(..));
                    queue = next;
                    fast_allowed = true;
                }
                Looked::Dir(p) => pos = p,
                Looked::File(read) => {
                    if !last || trailing_slash {
                        return Outcome::Errno(20);
                    }
                    return read();
                }
            }
        }
    }

    fn mount_root(&self) -> Pos {
        Pos::Host(vec![self.mount_fd.clone()])
    }

    fn parent(&self, pos: Pos) -> Pos {
        match pos {
            Pos::Guest(mut p) => {
                p.pop();
                Pos::Guest(p)
            }
            Pos::Host(mut stack) => {
                if stack.len() <= 1 {
                    // `..` da raiz da montagem sobe pro diretório pai no sandbox: /.
                    Pos::Guest(Vec::new())
                } else {
                    stack.pop();
                    Pos::Host(stack)
                }
            }
        }
    }

    fn lookup(&self, pos: &Pos, name: &str) -> Result<Looked, Outcome> {
        match pos {
            Pos::Guest(path) => {
                let p: Vec<&str> = path.iter().map(String::as_str).collect();
                match (p.as_slice(), name) {
                    ([], "work") => Ok(Looked::Dir(self.mount_root())),
                    ([], "etc") => Ok(Looked::Dir(Pos::Guest(vec!["etc".into()]))),
                    ([], "tmp") => Ok(Looked::Dir(Pos::Guest(vec!["tmp".into()]))),
                    (["etc"], "passwd") => {
                        let data = self.passwd.clone();
                        Ok(Looked::File(Box::new(move || Outcome::Content(data))))
                    }
                    _ => Err(Outcome::Errno(2)),
                }
            }
            Pos::Host(stack) => {
                let dir = stack.last().expect("pilha não vazia");
                let fd = rustix::fs::openat(dir, name, OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty())
                    .map_err(from_errno)?;
                let st = rustix::fs::fstat(&fd).map_err(from_errno)?;
                match FileType::from_raw_mode(st.st_mode) {
                    FileType::Symlink => {
                        // readlink do próprio fd O_PATH: lê o link que foi aberto, sem corrida com trocas.
                        let target = rustix::fs::readlinkat(&fd, "", Vec::new()).map_err(from_errno)?;
                        Ok(Looked::Symlink(target.to_string_lossy().into_owned()))
                    }
                    FileType::Directory => {
                        let mut next = stack.clone();
                        next.push(Rc::new(fd));
                        Ok(Looked::Dir(Pos::Host(next)))
                    }
                    _ => {
                        let parent = dir.clone();
                        let name = name.to_string();
                        let (dev, ino) = (st.st_dev, st.st_ino);
                        Ok(Looked::File(Box::new(move || {
                            // Reabre pra leitura sem seguir symlink e confere que é o mesmo inode.
                            match rustix::fs::openat(&parent, &name, OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty()) {
                                Ok(rfd) => match rustix::fs::fstat(&rfd) {
                                    Ok(s) if s.st_dev == dev && s.st_ino == ino => read_fd(rfd),
                                    Ok(_) => Outcome::Errno(11),
                                    Err(e) => from_errno(e),
                                },
                                Err(e) => from_errno(e),
                            }
                        })))
                    }
                }
            }
        }
    }
}

/// A resposta do Linux de verdade: o namespace do sandbox materializado em `refroot`, resolvido pelo
/// kernel com `RESOLVE_IN_ROOT` (semântica de chroot), cwd = /work.
pub fn reference(refroot: &OwnedFd, guest_path: &str) -> Outcome {
    let path = if guest_path.starts_with('/') { guest_path.to_string() } else { format!("work/{guest_path}") };
    // O prefixo "work/" não pode mudar o resultado de ENAMETOOLONG.
    if guest_path.len() >= PATH_MAX {
        return Outcome::Errno(36);
    }
    match rustix::fs::openat2(refroot, path.as_str(), OFlags::RDONLY | OFlags::CLOEXEC, Mode::empty(), ResolveFlags::IN_ROOT) {
        Ok(fd) => read_fd(fd),
        Err(e) => from_errno(e),
    }
}
