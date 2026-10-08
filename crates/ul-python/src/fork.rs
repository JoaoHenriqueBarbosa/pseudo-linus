//! `os.fork` (fatias F1 a F3 de `wip/notes/python-fork.md`): suspender o quadro em execução, copiar o
//! estado do interpretador numa [`crate::heapimage::VmImage`] e retomar a cópia, num processo novo do
//! kernel, na instrução seguinte à chamada.
//!
//! ## Como o pedido chega ao laço
//! O `_os.fork()` nativo não cria processo: devolve a exceção de marca [`SUSPEND_KIND`] e deixa o pedido
//! numa variável da thread ([`suspend`]). O `PyException` é uma estrutura (não um `enum`), então a
//! "variante `Suspend`" do desenho é esta marca. Só o ramo de erro das instruções de chamada de
//! `Vm::run_frames` a reconhece (custo zero no caminho comum), e ali, com o quadro em execução inteiro
//! em dados, chama [`Vm::complete_suspend`]. `pc` já aponta a instrução seguinte à chamada e a nativa
//! já consumiu os argumentos, então o resultado entra na pilha como se a nativa o tivesse devolvido.
//!
//! ## O que o filho retoma
//! `Vm::run_resumed` recebe o quadro do programa, os chamados de `frames_stack` e o que estava em
//! execução, refeitos por `VmImage::restore_fork`, e continua com `0` no lugar do resultado. Ao terminar
//! o programa, o filho faz o que o pai faria (`lib.rs::conclude_run` e `conclude`): ganchos de saída,
//! descarregar stdout, traceback no stderr e código de saída.
//!
//! ## Só no laço mais externo
//! O estado de uma chamada Rust ativa (um `run_loop` aninhado de callback de `sorted(key=)`, retomada de
//! gerador, `import`...) não é dado e a imagem não o alcança. Com `rust_nest > 1`, ou fora do `Vm::run` do
//! programa, o pedido falha com `RuntimeError`: é a lacuna que as fatias G1 a G5 do desenho fecham.

use std::cell::{Cell, RefCell};
use std::sync::{Arc, Condvar, Mutex};

use sysabi::{ProcAttrs, ProcessFn};

use crate::heapimage::{ImageError, Resume, VmImage};
use crate::object::Value;
use crate::vm::{exc, Callee, Exit, Frame, PyException, PyResult, Vm};
use crate::Finish;

/// A marca da exceção que pede a suspensão (nunca chega ao programa).
pub(crate) const SUSPEND_KIND: &str = "<suspend>";

/// O que uma nativa pede ao laço de instruções para completar com o quadro em mãos.
pub(crate) enum SuspendRequest {
    /// Devolve o valor sem criar processo (a prova de que a retomada na instrução funciona).
    Echo(Value),
    /// `fork(2)`: copia o estado e cria o processo filho.
    Fork,
    /// Troca de thread verde (`crate::gthread`): o laço de instruções a completa por conta própria, porque
    /// ela troca o quadro em execução em vez de devolver um valor.
    Green(crate::gthread::GreenRequest),
    /// Espera o descritor (o stdin, que outra thread alimenta) ter o que ler e repete a instrução que parou por
    /// falta de entrada: o laço a completa porque a espera roda as outras threads, o que só vale fora de uma
    /// chamada Rust (`crate::stdin`).
    Wait(i64),
}

/// A contabilidade que `Vm::run` tinha na entrada: o `run_loop` do filho a restaura na saída.
#[derive(Clone, Copy)]
pub(crate) struct Entry {
    pub(crate) depth: usize,
    pub(crate) frames: usize,
    pub(crate) line: usize,
}

/// O que falta para terminar o programa como o pai terminaria: o nome e o fonte que o traceback mostra,
/// e como o resultado vira saída (`-c`, `-m` e arquivo diferem).
#[derive(Clone)]
pub(crate) struct RunTail {
    pub(crate) name: String,
    pub(crate) shown_src: Option<String>,
    pub(crate) finish_mode: Finish,
    /// Preenchido quando o programa já acabou e a saída roda as funções de `atexit` (`enter_exit_phase`): o
    /// filho de um `fork` feito ali termina a saída com o desfecho que o pai já tinha.
    pub(crate) verdict: Option<Verdict>,
}

/// O desfecho do programa, calculado antes do `atexit` como o CPython imprime o erro antes de finalizar: o
/// código de saída e o texto do stderr (traceback de exceção não tratada, mensagem de `SystemExit`).
#[derive(Clone)]
pub(crate) struct Verdict {
    pub(crate) status: i32,
    pub(crate) stderr: String,
}

thread_local! {
    static REQUEST: RefCell<Option<SuspendRequest>> = const { RefCell::new(None) };
    static MAIN: Cell<Option<Entry>> = const { Cell::new(None) };
    static TAIL: RefCell<Option<RunTail>> = const { RefCell::new(None) };
    static PENDING: RefCell<Option<Arc<Gate>>> = const { RefCell::new(None) };
}

/// A exceção que pede ao laço de instruções para completar `request` (ver o cabeçalho do módulo).
pub(crate) fn suspend(request: SuspendRequest) -> PyException {
    REQUEST.with(|r| *r.borrow_mut() = Some(request));
    exc(SUSPEND_KIND, String::new())
}

pub(crate) fn is_suspend(e: &PyException) -> bool {
    e.kind == SUSPEND_KIND
}

/// O pedido que [`suspend`] deixou. Só é chamado depois de `is_suspend`, então ele existe.
pub(crate) fn take_request() -> SuspendRequest {
    REQUEST.with(|r| r.borrow_mut().take()).unwrap_or(SuspendRequest::Echo(Value::None))
}

/// Marca o laço mais externo do programa (`Vm::run`): só nele o `os.fork` pode copiar o estado.
pub(crate) struct MainGuard(Option<Entry>);

impl MainGuard {
    pub(crate) fn enter(entry: Entry) -> MainGuard {
        MainGuard(MAIN.with(|m| m.replace(Some(entry))))
    }
}

impl Drop for MainGuard {
    fn drop(&mut self) {
        MAIN.with(|m| m.set(self.0));
    }
}

pub(crate) fn set_tail(tail: RunTail) {
    TAIL.with(|t| *t.borrow_mut() = Some(tail));
}

/// O programa acabou com `verdict` e a saída vai rodar o `atexit`: um `os.fork` dali em diante o leva ao filho.
pub(crate) fn enter_exit_phase(verdict: &Verdict) {
    TAIL.with(|t| {
        if let Some(tail) = t.borrow_mut().as_mut() {
            tail.verdict = Some(verdict.clone());
        }
    });
}

/// A lacuna: o estado deste ponto da execução não é dado, e copiá-lo errado mostraria uma máquina diferente
/// do Debian. Prioridade máxima (ver `CLAUDE.md`).
fn gap(what: &str) -> PyException {
    exc("RuntimeError", format!("fork() is not supported {what}"))
}

impl Vm {
    /// Completa o pedido de uma nativa com o quadro em mãos. `outer` é o quadro do programa e `child` o
    /// chamado em execução (`None`: o próprio `outer`); os que esperam estão em `frames_stack`.
    pub(crate) fn complete_suspend(&mut self, request: SuspendRequest, outer: &Frame, child: Option<&Callee>) -> PyResult<Value> {
        match request {
            SuspendRequest::Echo(value) => Ok(value),
            SuspendRequest::Fork => self.fork_process(outer, child),
            SuspendRequest::Green(_) | SuspendRequest::Wait(_) => Err(exc("SystemError", "thread switch outside the instruction loop")),
        }
    }

    fn fork_process(&mut self, outer: &Frame, child: Option<&Callee>) -> PyResult<Value> {
        let Some(entry) = MAIN.with(Cell::get).filter(|_| self.rust_nest.get() == 1) else {
            return Err(gap("inside a callback of the interpreter"));
        };
        let Some(tail) = TAIL.with(|t| t.borrow().clone()) else {
            return Err(gap("outside a program run"));
        };
        let Some(process) = sysabi::sys::try_current() else {
            return Err(gap("outside a process"));
        };
        let image = VmImage::capture_fork(self, outer, child, entry).map_err(|e| match e {
            ImageError::Unsupported(kind) => gap(&format!("with a live '{kind}' object")),
            ImageError::Malformed(_) => gap("with this process state"),
        })?;
        // O filho só existe de fato quando termina de refazer o interpretador. No Linux o pai segue na frente
        // do filho (os ganchos `after_in_parent`, o `write` no mestre do `pty.fork` chegam antes da primeira
        // linha dele), salvo quando o pai faz algo pesado: o aviso de threads, que lê o arquivo do fonte,
        // chega depois da primeira linha do filho. O portão reproduz só esse caso: o pai o espera
        // ([`settle_fork`], chamada por `os._fork_with_hooks` antes do aviso). É um `Arc` na memória do
        // interpretador, sem fd: nada dele aparece em `/proc/self/fd`.
        let gate = Arc::new(Gate::default());
        PENDING.with(|p| *p.borrow_mut() = Some(gate.clone()));
        let release = Release(gate);
        let body: ProcessFn = Box::new(move || run_child(image, tail, release));
        match process.spawn_fn(ProcAttrs::default(), Vec::new(), body) {
            Ok(pid) => Ok(Value::Int(i64::from(pid))),
            Err(e) => {
                PENDING.with(|p| p.borrow_mut().take());
                Err(crate::modules::osnative::os_error(e, None))
            }
        }
    }
}

/// O portão do `fork`: `true` quando o filho terminou de refazer o interpretador (ou morreu).
#[derive(Default)]
struct Gate {
    ready: Mutex<bool>,
    cv: Condvar,
}

/// A ponta do filho: soltá-la (o filho pronto, morto ou nunca iniciado) abre o portão.
struct Release(Arc<Gate>);

impl Drop for Release {
    fn drop(&mut self) {
        *self.0.ready.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.0.cv.notify_all();
    }
}

/// Espera o filho do último `fork` ficar de pé (sem efeito se não há `fork` pendente).
pub(crate) fn settle_fork() {
    let Some(gate) = PENDING.with(|p| p.borrow_mut().take()) else { return };
    let mut ready = gate.ready.lock().unwrap_or_else(|e| e.into_inner());
    while !*ready {
        ready = gate.cv.wait(ready).unwrap_or_else(|e| e.into_inner());
    }
}

/// O corpo do processo filho: refaz o interpretador numa thread nova, retoma o programa e termina como o
/// pai terminaria.
fn run_child(image: VmImage, tail: RunTail, gate: Release) -> i32 {
    let mode = tail.finish_mode;
    let outcome = crate::on_interpreter_thread(move || child_main(&image, &tail, gate)).unwrap_or_else(|| crate::Outcome {
        stdout: Vec::new(),
        stderr: "Fatal Python error: could not run the interpreter thread\n".into(),
        status: 1,
    });
    crate::conclude(outcome, mode)
}

fn child_main(image: &VmImage, tail: &RunTail, gate: Release) -> crate::Outcome {
    let restored = image.restore_fork();
    // O filho está de pé (ou não vai estar): o pai pode seguir.
    drop(gate);
    let (mut vm, resume) = match restored {
        Ok(restored) => restored,
        Err(e) => {
            return crate::Outcome {
                stdout: Vec::new(),
                stderr: format!("Fatal Python error: could not restore the process image ({e:?})\n"),
                status: 1,
            }
        }
    };
    set_tail(tail.clone());
    let Resume { mut outer, suspended, child, entry } = resume;
    let result = {
        let _main = MainGuard::enter(entry);
        match vm.run_resumed(&mut outer, suspended, child, entry) {
            Ok(Exit::Return(value)) => Ok(value),
            Ok(Exit::Yield(_)) => Err(crate::vm::internal("yield outside generator")),
            Err(e) => Err(e),
        }
    };
    // Fork feito numa função de `atexit`: o programa já tinha o desfecho, e o resto da saída é o mesmo do pai
    // (o resultado das funções de `atexit` nunca muda o código de saída).
    if let Some(verdict) = tail.verdict.clone() {
        return crate::finish_exit(&mut vm, verdict);
    }
    crate::conclude_run(&mut vm, crate::vm::run_outcome(result), &tail.name, tail.shown_src.as_deref().unwrap_or(""))
}
