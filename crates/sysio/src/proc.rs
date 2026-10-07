//! Estado de userland do pseudo-processo corrente: o que num programa C mora na libc do processo
//! (buffer do stdout, buffer do stdin, código de saída do uucore, "statics" do programa).
//!
//! O kernel instala o processo corrente da thread em `sysabi::sys` (modelo A: uma thread do SO por
//! pseudo-processo). Este módulo guarda, por thread, uma pilha de [`Frame`]s, cada um ligado a um
//! processo. A pilha existe por causa do kernel de teste, que roda o filho de `spawn` na mesma
//! thread do pai: o filho ganha o próprio frame em cima do do pai.
//!
//! - [`run`] abre um frame novo pra um programa e fecha no fim, descarregando o stdout. É o ponto
//!   de entrada de todo programa portado (`Program::main` chama `sysio::run`).
//! - Fora de um `run` (código que roda num processo sem passar pela entrada de um programa, como o
//!   corpo de um `spawn_fn`), o primeiro acesso cria um frame implícito, sem buffer no stdout, que
//!   some quando o processo dele termina.

use std::any::Any;
use std::cell::RefCell;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use sysabi::Syscalls;

use crate::io::{InBuf, OutBuf};

/// Estado de userland de um pseudo-processo.
pub struct Frame {
    /// Identidade única (nunca reaproveitada) do frame.
    id: u64,
    /// O processo dono (pra saber, na pilha da thread, se o processo corrente ainda é este).
    sys: Weak<dyn Syscalls>,
    pid: sysabi::Pid,
    /// Frames de [`run`] têm stdout com buffer (descarregado no fim); implícitos escrevem direto.
    pub(crate) buffered: bool,
    exit_code: AtomicI32,
    locals: Mutex<Vec<Arc<dyn Any + Send + Sync>>>,
    pub(crate) stdout: Mutex<OutBuf>,
    /// `None` enquanto um `StdinLock` está com o buffer.
    pub(crate) stdin: Mutex<Option<InBuf>>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame").field("pid", &self.pid).field("buffered", &self.buffered).finish_non_exhaustive()
    }
}

impl Frame {
    fn new(sys: &Arc<dyn Syscalls>, buffered: bool) -> Frame {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Frame {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            sys: Arc::downgrade(sys),
            pid: sys.getpid(),
            buffered,
            exit_code: AtomicI32::new(0),
            locals: Mutex::new(Vec::new()),
            stdout: Mutex::new(OutBuf::default()),
            stdin: Mutex::new(Some(InBuf::for_program())),
        }
    }

    fn alive(&self) -> bool {
        self.sys.strong_count() > 0
    }

    fn owns(&self, sys: &Arc<dyn Syscalls>) -> bool {
        // Ponteiro igual basta: enquanto o `Weak` existe, a alocação não é reaproveitada. Se o kernel
        // trocar o objeto do mesmo processo (exec), o pid desempata.
        std::ptr::addr_eq(self.sys.as_ptr(), Arc::as_ptr(sys)) || (self.alive() && self.pid == sys.getpid())
    }
}

thread_local! {
    static STACK: RefCell<Vec<Arc<Frame>>> = const { RefCell::new(Vec::new()) };
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// O processo corrente (o objeto de syscalls que o kernel instalou na thread).
pub use sysabi::sys::current as sys;

/// Frame do processo corrente, criado (implícito, sem buffer) se ainda não existe.
pub fn frame() -> Arc<Frame> {
    let sys = sys();
    STACK.with(|s| {
        let mut stack = s.borrow_mut();
        // Frames implícitos de processos que já terminaram (filhos síncronos do kernel de teste).
        while stack.last().is_some_and(|f| !f.alive()) {
            stack.pop();
        }
        if let Some(top) = stack.last()
            && top.owns(&sys)
        {
            return Arc::clone(top);
        }
        let f = Arc::new(Frame::new(&sys, false));
        stack.push(Arc::clone(&f));
        f
    })
}

/// Identidade do frame (estado de userland) do processo corrente. Serve pra quem precisa guardar
/// estado por processo que não é `Send + Sync` (e por isso não cabe em [`proc_local`]) num
/// `thread_local`, indexado por este número.
pub fn frame_id() -> u64 {
    frame().id
}

/// Identidades dos frames vivos nesta thread (pra podar caches indexados por [`frame_id`]).
pub fn live_frame_ids() -> Vec<u64> {
    STACK.with(|s| s.borrow().iter().filter(|f| f.alive()).map(|f| f.id).collect())
}

/// Pedido de término (mantido pela API do shim do F06): o `exit` de verdade é
/// [`crate::process::exit`], que desenrola com o `ExitUnwind` do sysabi.
#[derive(Debug, Clone, Copy)]
pub struct ExitRequest(pub i32);

/// Roda o `main` de um programa num frame novo: stdout com buffer (como o stdio da glibc: linha
/// se for terminal, bloco se não for), descarregado no fim. Devolve o código de saída.
///
/// - `exit` no meio (unwind com `ExitUnwind`) também descarrega o stdout, como o `exit(3)`.
/// - Morte por sinal (`KillUnwind`) e `execve` (`ExecUnwind`) não descarregam: no Linux o buffer
///   do stdio some junto com a imagem do processo.
/// - Se o descarregamento final falhar (stdout fechado, disco cheio), o programa escreve
///   `prog: write error: ...` e sai com 1, como o `close_stdout` do gnulib; EPIPE mata por SIGPIPE
///   antes disso, no próprio `write`.
pub fn run(main: impl FnOnce() -> i32) -> i32 {
    let sys = sys();
    let frame = Arc::new(Frame::new(&sys, true));
    STACK.with(|s| s.borrow_mut().push(Arc::clone(&frame)));
    struct Pop(Arc<Frame>);
    impl Drop for Pop {
        fn drop(&mut self) {
            STACK.with(|s| {
                let mut stack = s.borrow_mut();
                if let Some(i) = stack.iter().rposition(|f| Arc::ptr_eq(f, &self.0)) {
                    stack.truncate(i);
                }
            });
        }
    }
    let _pop = Pop(Arc::clone(&frame));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(main));
    match result {
        Ok(code) => finish(&frame, code),
        Err(payload) => {
            if let Some(exit) = payload.downcast_ref::<sysabi::ExitUnwind>() {
                let code = finish(&frame, exit.0);
                sysabi::sys::exit(code);
            }
            if let Some(ExitRequest(code)) = payload.downcast_ref::<ExitRequest>() {
                return finish(&frame, *code);
            }
            std::panic::resume_unwind(payload)
        }
    }
}

/// Descarrega o stdout do frame no fim do programa.
fn finish(frame: &Frame, code: i32) -> i32 {
    match crate::io::flush_frame_stdout(frame) {
        Ok(()) => code,
        Err(e) => {
            let msg = format!("{}: write error: {}\n", program_name(), crate::errno::strerror(&e));
            let _ = sysabi::sys::write_all(sysabi::Fd::STDERR, msg.as_bytes());
            if code == 0 { 1 } else { code }
        }
    }
}

/// `argv[0]` como o programa foi chamado (o `program_name` do gnulib, que as mensagens de erro do
/// GNU usam sem tirar o diretório).
pub fn program_name() -> String {
    let argv = sys().argv();
    argv.first().map(|a| String::from_utf8_lossy(a).into_owned()).unwrap_or_default()
}

/// Valor de tipo `T` do processo corrente, criado com `init` na primeira chamada: o substituto,
/// por pseudo-processo, de um `static X: OnceLock<T>` (que seria um só pro host inteiro).
pub fn proc_local<T: Any + Send + Sync>(init: impl FnOnce() -> T) -> Arc<T> {
    let f = frame();
    let mut locals = lock(&f.locals);
    for item in locals.iter() {
        if let Ok(found) = Arc::clone(item).downcast::<T>() {
            return found;
        }
    }
    let value = Arc::new(init());
    locals.push(Arc::clone(&value) as Arc<dyn Any + Send + Sync>);
    value
}

/// Código de saída guardado pelo programa (o `EXIT_CODE` do uucore, que era global do host).
pub fn exit_code() -> i32 {
    frame().exit_code.load(Ordering::SeqCst)
}

pub fn set_exit_code(code: i32) {
    frame().exit_code.store(code, Ordering::SeqCst);
}

/// `RLIMIT_NOFILE` (soft) do processo corrente.
pub fn rlimit_nofile() -> usize {
    match sys().getrlimit(sysabi::Resource::Nofile) {
        Ok(l) if l.cur != sysabi::RLIM_INFINITY => l.cur as usize,
        Ok(_) => usize::MAX,
        Err(_) => RLIMIT_NOFILE_SOFT,
    }
}

/// Padrão do Linux, pra quem precisa de uma constante.
pub const RLIMIT_NOFILE_SOFT: usize = 1024;

/// Liga uma thread nova ao frame do processo corrente (usado por [`crate::thread::spawn`]); o
/// guarda desliga no fim da thread.
pub(crate) fn adopt(frame: Arc<Frame>) -> Adopted {
    STACK.with(|s| s.borrow_mut().push(Arc::clone(&frame)));
    Adopted(frame)
}

pub(crate) struct Adopted(Arc<Frame>);

impl Drop for Adopted {
    fn drop(&mut self) {
        STACK.with(|s| {
            let mut stack = s.borrow_mut();
            if let Some(i) = stack.iter().rposition(|f| Arc::ptr_eq(f, &self.0)) {
                stack.remove(i);
            }
        });
    }
}
