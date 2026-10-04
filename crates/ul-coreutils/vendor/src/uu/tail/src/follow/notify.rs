// Porte pseudo-linus: substitui o crate `notify` (inotify do host, e um PollWatcher que faz stat
// no FS do host numa thread). O contrato do pseudo-kernel não tem inotify, então o tail segue por
// polling, como o GNU com `---disable-inotify`. Este watcher é síncrono: não tem thread; a espera
// (`recv_timeout`) dorme pelo nanosleep do pseudo-kernel (o processo libera a CPU e um sinal o
// acorda) e então compara um retrato dos caminhos vigiados com o anterior, gerando os mesmos tipos
// de evento que o PollWatcher do notify gera: `Create(Any)`, `Remove(Any)`,
// `Modify(Metadata(WriteTime))` e, com `compare_contents`, `Modify(Data(Any))`.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, TryRecvError};
use std::time::Duration;

use sysio::os::unix::fs::MetadataExt;
use sysio::path::PathExt;

/// Tipos de evento, com os nomes do `notify::event`. O tail casa variantes que só o inotify gera;
/// elas ficam pra que o código dele continue igual ao do upstream.
#[allow(dead_code)]
pub mod event {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum CreateKind {
        Any,
        File,
        Folder,
        Other,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum DataChange {
        Any,
        Size,
        Content,
        Other,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum MetadataKind {
        Any,
        AccessTime,
        WriteTime,
        Permissions,
        Ownership,
        Extended,
        Other,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum RenameMode {
        Any,
        To,
        From,
        Both,
        Other,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ModifyKind {
        Any,
        Data(DataChange),
        Metadata(MetadataKind),
        Name(RenameMode),
        Other,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum RemoveKind {
        Any,
        File,
        Folder,
        Other,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum EventKind {
        Any,
        Access,
        Create(CreateKind),
        Modify(ModifyKind),
        Remove(RemoveKind),
        Other,
    }
}

use event::{CreateKind, DataChange, EventKind, MetadataKind, ModifyKind, RemoveKind};

/// Um evento sobre um ou mais caminhos.
#[derive(Clone, Debug)]
pub struct Event {
    pub kind: EventKind,
    pub paths: Vec<PathBuf>,
}

/// Os erros do `notify` que o tail trata; este watcher só gera os de caminho.
#[allow(dead_code)]
#[derive(Debug)]
pub enum ErrorKind {
    Generic(String),
    Io(sysio::io::Error),
    PathNotFound,
    WatchNotFound,
    MaxFilesWatch,
}

#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub paths: Vec<PathBuf>,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ErrorKind::Generic(s) => f.write_str(s),
            ErrorKind::Io(e) => f.write_str(&uucore::error::strip_errno(e)),
            ErrorKind::PathNotFound => f.write_str("No path was found."),
            ErrorKind::WatchNotFound => f.write_str("No watch was found."),
            ErrorKind::MaxFilesWatch => f.write_str("OS file watch limit reached."),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecursiveMode {
    Recursive,
    NonRecursive,
}

/// Configuração do polling.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    poll_interval: Duration,
    compare_contents: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { poll_interval: Duration::from_secs(30), compare_contents: false }
    }
}

impl Config {
    pub fn with_poll_interval(mut self, d: Duration) -> Self {
        self.poll_interval = d;
        self
    }

    pub fn with_compare_contents(mut self, yes: bool) -> Self {
        self.compare_contents = yes;
        self
    }
}

/// Retrato de um caminho num instante.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Snap {
    dev: u64,
    ino: u64,
    size: u64,
    mtime: (i64, i64),
    content: Option<u64>,
}

/// O watcher de polling.
pub struct PollWatcher {
    config: Config,
    roots: BTreeMap<PathBuf, RecursiveMode>,
    snaps: BTreeMap<PathBuf, Snap>,
    queue: VecDeque<Result<Event, Error>>,
}

impl PollWatcher {
    pub fn new(config: Config) -> Self {
        Self { config, roots: BTreeMap::new(), snaps: BTreeMap::new(), queue: VecDeque::new() }
    }

    pub fn watch(&mut self, path: &Path, mode: RecursiveMode) -> Result<(), Error> {
        if !path.sys_exists() {
            return Err(Error { kind: ErrorKind::PathNotFound, paths: vec![path.to_owned()] });
        }
        self.roots.insert(path.to_owned(), mode);
        // O retrato inicial não gera eventos.
        let current = self.scan();
        for (p, s) in current {
            self.snaps.entry(p).or_insert(s);
        }
        Ok(())
    }

    pub fn unwatch(&mut self, path: &Path) -> Result<(), Error> {
        if self.roots.remove(path).is_none() {
            return Err(Error { kind: ErrorKind::WatchNotFound, paths: vec![path.to_owned()] });
        }
        let current = self.scan();
        self.snaps.retain(|p, _| current.contains_key(p));
        Ok(())
    }

    /// Próximo evento, esperando no máximo `timeout` (dorme no pseudo-kernel e varre ao acordar).
    pub fn recv_timeout(&mut self, timeout: Duration) -> Result<Result<Event, Error>, RecvTimeoutError> {
        if let Some(ev) = self.queue.pop_front() {
            return Ok(ev);
        }
        // O tail passa o `--sleep-interval` como prazo e como intervalo de polling.
        let _ = sysio::time::sleep_interruptible(timeout.min(self.config.poll_interval), true);
        self.poll();
        self.queue.pop_front().ok_or(RecvTimeoutError::Timeout)
    }

    /// Evento já enfileirado, sem esperar.
    pub fn try_recv(&mut self) -> Result<Result<Event, Error>, TryRecvError> {
        self.queue.pop_front().ok_or(TryRecvError::Empty)
    }

    /// Caminhos vigiados agora: cada raiz e, se for diretório, as entradas dele (recursivo só
    /// quando pedido).
    fn scan(&self) -> BTreeMap<PathBuf, Snap> {
        let mut out = BTreeMap::new();
        for (root, mode) in &self.roots {
            self.snap_into(root, &mut out);
            if root.sys_is_dir() {
                self.scan_dir(root, *mode == RecursiveMode::Recursive, &mut out);
            }
        }
        out
    }

    fn scan_dir(&self, dir: &Path, recursive: bool, out: &mut BTreeMap<PathBuf, Snap>) {
        let Ok(entries) = sysio::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let p = entry.path();
            self.snap_into(&p, out);
            if recursive && p.sys_is_dir() {
                self.scan_dir(&p, true, out);
            }
        }
    }

    fn snap_into(&self, path: &Path, out: &mut BTreeMap<PathBuf, Snap>) {
        // O notify usa o stat que segue links (como o tail vê o arquivo).
        let Ok(md) = path.sys_metadata() else { return };
        let content = if self.config.compare_contents && md.is_file() {
            sysio::fs::read(path).ok().map(|data| {
                let mut h = DefaultHasher::new();
                data.hash(&mut h);
                h.finish()
            })
        } else {
            None
        };
        out.insert(
            path.to_owned(),
            Snap { dev: md.dev(), ino: md.ino(), size: md.size(), mtime: (md.mtime(), md.mtime_nsec()), content },
        );
    }

    /// Compara o retrato atual com o anterior e enfileira os eventos.
    fn poll(&mut self) {
        let current = self.scan();
        for (p, old) in &self.snaps {
            match current.get(p) {
                None => self.queue.push_back(Ok(Event { kind: EventKind::Remove(RemoveKind::Any), paths: vec![p.clone()] })),
                Some(new) => {
                    if new.mtime != old.mtime {
                        self.queue.push_back(Ok(Event {
                            kind: EventKind::Modify(ModifyKind::Metadata(MetadataKind::WriteTime)),
                            paths: vec![p.clone()],
                        }));
                    }
                    if new.content != old.content || new.size != old.size || new.ino != old.ino || new.dev != old.dev {
                        self.queue.push_back(Ok(Event {
                            kind: EventKind::Modify(ModifyKind::Data(DataChange::Any)),
                            paths: vec![p.clone()],
                        }));
                    }
                }
            }
        }
        for p in current.keys() {
            if !self.snaps.contains_key(p) {
                self.queue.push_back(Ok(Event { kind: EventKind::Create(CreateKind::Any), paths: vec![p.clone()] }));
            }
        }
        self.snaps = current;
    }
}
