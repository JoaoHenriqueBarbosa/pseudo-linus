//! Contexto do pseudo-processo corrente.
//!
//! No pseudo-linus todo I/O passa pelo `Ctx`. Aqui o `Ctx` fica numa thread-local (`CURRENT`), o que
//! só é correto no modelo A do design (uma thread do SO por pseudo-processo): é o jeito de portar
//! código que chama `std::fs::File::open(p)` sem mudar a assinatura de cada função pra receber
//! `&mut Ctx`. Threads criadas pelo processo herdam o contexto via [`crate::thread::spawn`].

use std::any::Any;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use crate::vfs::Vfs;

/// Quem sabe executar um comando a partir de argv (a tabela de programas do pseudo-kernel).
/// A saída do filho vai pros mesmos stdout/stderr do pai (herdados), a entrada é a dada.
pub trait Executor: Send + Sync {
    /// Devolve o código de saída, ou erro de `exec` (ENOENT quando o programa não existe). `cwd`
    /// troca o diretório de trabalho do filho (o `chdir` entre o fork e o exec).
    fn exec(&self, argv: &[OsString], stdin: Vec<u8>, cwd: Option<&std::path::Path>) -> std::io::Result<i32>;
}

/// Entrada padrão: bytes imutáveis + posição compartilhada.
#[derive(Debug, Default)]
pub struct Input {
    pub data: Arc<Vec<u8>>,
    pub pos: usize,
}

pub struct ProcInner {
    pub vfs: Arc<Mutex<Vfs>>,
    pub cwd: Mutex<PathBuf>,
    pub args: Vec<OsString>,
    pub env: Mutex<BTreeMap<OsString, OsString>>,
    pub stdin: Arc<Mutex<Input>>,
    pub stdout: Arc<Mutex<Vec<u8>>>,
    pub stderr: Arc<Mutex<Vec<u8>>>,
    pub now: SystemTime,
    pub umask: u32,
    /// O `EXIT_CODE` global do uucore vira estado do processo.
    pub exit_code: AtomicI32,
    pub executor: Option<Arc<dyn Executor>>,
    /// Armazenamento local do processo: o substituto de `static X: OnceLock<T>` nos portes.
    pub locals: Mutex<Vec<Arc<dyn Any + Send + Sync>>>,
}

/// Valor de tipo `T` do processo corrente, criado com `init` na primeira chamada. É o
/// equivalente "por pseudo-processo" de um `static OnceLock<T>` (que seria um só pro host).
pub fn proc_local<T: Any + Send + Sync>(init: impl FnOnce() -> T) -> Arc<T> {
    let p = current();
    let mut locals = lock(&p.locals);
    for item in locals.iter() {
        if let Ok(found) = Arc::clone(item).downcast::<T>() {
            return found;
        }
    }
    let value = Arc::new(init());
    locals.push(Arc::clone(&value) as Arc<dyn Any + Send + Sync>);
    value
}

/// Handle clonável do processo corrente.
pub type Proc = Arc<ProcInner>;

/// `RLIMIT_NOFILE` (soft) de todo pseudo-processo: o padrão do Linux.
pub const RLIMIT_NOFILE_SOFT: usize = 1024;

thread_local! {
    static CURRENT: RefCell<Option<Proc>> = const { RefCell::new(None) };
}

/// Especificação de um processo novo.
pub struct Spawn {
    pub vfs: Arc<Mutex<Vfs>>,
    pub cwd: PathBuf,
    pub args: Vec<OsString>,
    pub env: BTreeMap<OsString, OsString>,
    pub stdin: Vec<u8>,
    pub stdout: Arc<Mutex<Vec<u8>>>,
    pub stderr: Arc<Mutex<Vec<u8>>>,
    pub now: SystemTime,
    pub executor: Option<Arc<dyn Executor>>,
}

impl Spawn {
    pub fn build(self) -> Proc {
        Arc::new(ProcInner {
            vfs: self.vfs,
            cwd: Mutex::new(self.cwd),
            args: self.args,
            env: Mutex::new(self.env),
            stdin: Arc::new(Mutex::new(Input { data: Arc::new(self.stdin), pos: 0 })),
            stdout: self.stdout,
            stderr: self.stderr,
            now: self.now,
            umask: 0o022,
            exit_code: AtomicI32::new(0),
            executor: self.executor,
            locals: Mutex::new(Vec::new()),
        })
    }
}

/// Processo corrente. Chamar fora de um processo é bug do porte (I/O que escapou do shim).
pub fn current() -> Proc {
    try_current().expect("sysio: I/O fora de um pseudo-processo (thread sem contexto)")
}

pub fn try_current() -> Option<Proc> {
    CURRENT.with(|c| c.borrow().clone())
}

/// Roda `f` com `proc` como processo corrente desta thread, restaurando o anterior no fim (mesmo
/// com panic), o que permite um processo "executar" um filho na mesma thread.
pub fn enter<R>(proc: Proc, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Proc>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let prev = self.0.take();
            CURRENT.with(|c| *c.borrow_mut() = prev);
        }
    }
    let prev = CURRENT.with(|c| c.borrow_mut().replace(proc));
    let _restore = Restore(prev);
    f()
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Pedido de término do processo (o `exit(2)` do pseudo-processo): sobe como unwind até quem
/// executou o programa, que devolve o código. `std::process::exit` mataria o host inteiro.
#[derive(Debug, Clone, Copy)]
pub struct ExitRequest(pub i32);

pub fn exit_code() -> i32 {
    current().exit_code.load(Ordering::SeqCst)
}

pub fn set_exit_code(code: i32) {
    current().exit_code.store(code, Ordering::SeqCst);
}
