//! Threads verdes: as threads de `threading` e `_thread` como pilhas de quadros que o laço de instruções
//! troca entre si (desenho em `wip/notes/python-threads.md`).
//!
//! ## O modelo
//! Uma thread Python é um segmento da pilha explícita da VM: o quadro mais externo (`outer`), os quadros que
//! esperam em `Vm::frames_stack` e o que está em execução (`child`), mais o estado que a VM guarda por
//! thread (`handled`, `frames`, `depth`, `cur_line`, `globals`). Trocar de thread é trocar esse segmento
//! dentro do próprio `Vm::run_frames`: nada de recursão Rust nem de thread do host, então nada precisa ser
//! `Send` e o modelo "uma thread do host por processo" do kernel não muda.
//!
//! ## A API (módulo `_sys`, só o `threading` embutido a usa)
//! - `_gt_spawn(callable) -> tid`: cria a thread verde, ainda sem rodar; o `callable` não recebe argumentos.
//! - `_gt_switch(tid, valor=None) -> valor`: suspende a thread atual e retoma `tid` (ou a inicia, se nova).
//!   Devolve o valor que quem a retomar passar. Como o `greenlet.switch`, é simétrico: a política (quem
//!   roda a seguir) fica toda no escalonador em Python.
//! - `_gt_finish(tid)`: encerra a thread atual (a pilha dela é descartada) e retoma `tid`; `-1` encerra o
//!   laço do programa (o filho de um `os.fork` feito numa thread, que não tem para quem passar o controle).
//! - `_gt_current() -> tid` (a principal é 0) e `_gt_can_switch() -> bool`.
//!
//! ## Só no laço mais externo
//! Como no `os.fork` (`crate::fork`), a troca só vale com `rust_nest == 1`: uma chamada Rust ativa (callback
//! de `sorted(key=)`, retomada de gerador, `import`...) guarda estado que não é dado e que a troca não
//! alcança. `_gt_can_switch()` diz quando é o caso; o `threading` cai então no escalonador aninhado antigo.
//! Quando as fatias G1 a G5 de `wip/notes/python-fork.md` fecharem a recursão, a restrição some dos dois.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::compile::Code;
use crate::fork::{suspend, SuspendRequest};
use crate::modules::ModuleBuilder;
use crate::native_util::{no_kwargs, want_int};
use crate::object::{Env, Kw, Value, VarMap};
use crate::vm::{exc, Callee, Entered, Frame, PyResult, Vm};

/// O pedido de troca que a nativa deixa para o laço de instruções completar.
pub(crate) enum GreenRequest {
    /// Suspende a thread atual e retoma `target`, entregando `value` ao `_gt_switch` dela.
    Switch { target: i64, value: Value },
    /// Descarta a thread atual e retoma `target` (`-1`: acaba o laço do programa).
    Finish { target: i64 },
}

/// O que o laço faz com o quadro que passou a executar.
pub(crate) enum Landing {
    /// Uma thread suspensa foi retomada: o valor entra na pilha dela, como o resultado do `_gt_switch`.
    Resume(Value),
    /// Uma thread nova começou: o primeiro quadro dela já é o que executa, não há resultado a entregar.
    Started,
    /// Não há mais thread: o laço do programa termina.
    Exit,
}

/// O estado de uma thread que não está rodando.
struct Parked {
    outer: Frame,
    suspended: Vec<Callee>,
    child: Option<Callee>,
    handled: Vec<Value>,
    frames: Vec<(Rc<Code>, usize, Rc<Env>)>,
    depth: usize,
    cur_line: usize,
    globals: Rc<RefCell<VarMap>>,
}

enum Entry {
    Parked(Box<Parked>),
    /// Criada por `_gt_spawn` e ainda sem rodar.
    Fresh(Value),
}

struct Sched {
    next: i64,
    current: i64,
    entries: HashMap<i64, Entry>,
}

thread_local! {
    /// O escalonador do processo (cada processo tem a sua thread do interpretador). A principal é a 0 e só
    /// ganha entrada quando suspende.
    static SCHED: RefCell<Sched> = RefCell::new(Sched { next: 1, current: 0, entries: HashMap::new() });
    /// Espelho de `!SCHED.entries.is_empty()`: há outra thread verde (nova ou suspensa). O laço de instruções
    /// o lê a cada instrução, então o caso comum (nenhuma outra thread) custa uma leitura de `Cell`.
    static OTHERS: Cell<bool> = const { Cell::new(false) };
}

/// Reflete em `OTHERS` o estado de `entries`; toda mutação de `Sched::entries` termina chamando isto.
fn sync_others(s: &Sched) {
    OTHERS.with(|o| o.set(!s.entries.is_empty()));
}

/// A thread que roda não é a principal: o quadro mais externo dela é um marcador, nunca o do programa.
pub(crate) fn in_secondary() -> bool {
    SCHED.with(|s| s.borrow().current != 0)
}

fn nested_error() -> crate::vm::PyException {
    exc("RuntimeError", "cannot switch threads inside a callback of the interpreter")
}

impl Vm {
    /// Troca de thread (ver o cabeçalho do módulo). `frame` é o quadro mais externo do laço, `child` o que
    /// executa e `mark` o início do segmento em `frames_stack`. Antes de qualquer troca o pedido é
    /// validado: um erro aqui deixa a thread atual exatamente como estava.
    pub(crate) fn green_switch(
        &mut self,
        request: GreenRequest,
        frame: &mut Frame,
        child: &mut Option<Callee>,
        mark: usize,
    ) -> PyResult<Landing> {
        if mark != 0 || self.rust_nest.get() != 1 {
            return Err(nested_error());
        }
        let (target, value, keep) = match request {
            GreenRequest::Switch { target, value } => (target, value, true),
            GreenRequest::Finish { target } => {
                if target == -1 {
                    return Ok(Landing::Exit);
                }
                (target, Value::None, false)
            }
        };
        let (me, entry) = SCHED.with(|s| {
            let mut s = s.borrow_mut();
            let me = s.current;
            if target == me {
                return Err(exc("RuntimeError", "cannot switch to the running thread"));
            }
            let removed = s.entries.remove(&target);
            sync_others(&s);
            match removed {
                Some(entry) => Ok((me, entry)),
                None => Err(exc("RuntimeError", "cannot switch to a thread that is not suspended")),
            }
        })?;
        let mine = self.take_thread(frame, child);
        match entry {
            Entry::Parked(theirs) => {
                self.install_thread(*theirs, frame, child);
                self.park_and_switch(me, target, keep.then_some(mine));
                Ok(Landing::Resume(value))
            }
            Entry::Fresh(callable) => match self.call_or_enter(&callable, Vec::new(), Vec::new()) {
                Ok(Entered::Frame(callee)) => {
                    *child = Some(callee);
                    self.park_and_switch(me, target, keep.then_some(mine));
                    Ok(Landing::Started)
                }
                other => {
                    // A thread nova não pôde começar: a atual volta como estava e a nova continua nova.
                    self.install_thread(mine, frame, child);
                    SCHED.with(|s| {
                        let mut s = s.borrow_mut();
                        s.entries.insert(target, Entry::Fresh(callable));
                        sync_others(&s);
                    });
                    match other {
                        Err(e) => Err(e),
                        Ok(_) => Err(exc("SystemError", "thread entry point is not a Python function")),
                    }
                }
            },
        }
    }

    /// Guarda a thread que saiu (se ela ainda vive) e marca `target` como a que roda.
    fn park_and_switch(&mut self, me: i64, target: i64, mine: Option<Parked>) {
        SCHED.with(|s| {
            let mut s = s.borrow_mut();
            if let Some(parked) = mine {
                s.entries.insert(me, Entry::Parked(Box::new(parked)));
            }
            s.current = target;
            sync_others(&s);
        });
        restart_slice();
    }

    /// Tira da VM o estado da thread que roda e a deixa como uma thread recém-criada: o quadro mais externo
    /// vira um marcador (o código do programa com o `pc` no fim, que `run_frames` reconhece), sem
    /// quadros, sem exceções em tratamento e com profundidade zero.
    fn take_thread(&mut self, frame: &mut Frame, child: &mut Option<Callee>) -> Parked {
        let marker = Frame {
            code: frame.code.clone(),
            env: frame.env.clone(),
            stack: Vec::new(),
            blocks: Vec::new(),
            pc: frame.code.ops.len(),
            handled: Vec::new(),
            handled_base: 0,
        };
        Parked {
            outer: std::mem::replace(frame, marker),
            suspended: self.frames_stack.borrow_mut().split_off(0),
            child: child.take(),
            handled: std::mem::take(&mut *self.handled.borrow_mut()),
            frames: std::mem::take(&mut *self.frames.borrow_mut()),
            depth: self.depth.replace(0),
            cur_line: self.cur_line.replace(0),
            globals: self.globals.clone(),
        }
    }

    /// O inverso de `take_thread`: a VM passa a executar o estado de `thread`.
    fn install_thread(&mut self, thread: Parked, frame: &mut Frame, child: &mut Option<Callee>) {
        *frame = thread.outer;
        *child = thread.child;
        *self.frames_stack.borrow_mut() = thread.suspended;
        *self.handled.borrow_mut() = thread.handled;
        *self.frames.borrow_mut() = thread.frames;
        self.depth.set(thread.depth);
        self.cur_line.set(thread.cur_line);
        self.globals = thread.globals;
    }
}

fn spawn(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("_gt_spawn", &kw)?;
    let [callable] = <[Value; 1]>::try_from(args).map_err(|a| {
        crate::vm::type_error(format!("_gt_spawn() takes exactly one argument ({} given)", a.len()))
    })?;
    let tid = SCHED.with(|s| {
        let mut s = s.borrow_mut();
        let tid = s.next;
        s.next += 1;
        s.entries.insert(tid, Entry::Fresh(callable));
        sync_others(&s);
        tid
    });
    Ok(Value::Int(tid))
}

/// Valida que a troca para `target` é possível agora (o mesmo que `green_switch` confere, antes de
/// suspender, para o erro sair na chamada).
fn check_target(vm: &Vm, target: i64) -> PyResult<()> {
    if vm.rust_nest.get() != 1 {
        return Err(nested_error());
    }
    let known = SCHED.with(|s| {
        let s = s.borrow();
        target != s.current && s.entries.contains_key(&target)
    });
    if known || target == -1 {
        Ok(())
    } else {
        Err(exc("RuntimeError", "cannot switch to a thread that is not suspended"))
    }
}

fn switch(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("_gt_switch", &kw)?;
    let mut args = args.into_iter();
    let target = want_int(&args.next().ok_or_else(|| crate::vm::type_error("_gt_switch() missing the thread"))?)?;
    let value = args.next().unwrap_or(Value::None);
    check_target(vm, target)?;
    Err(suspend(SuspendRequest::Green(GreenRequest::Switch { target, value })))
}

fn finish(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("_gt_finish", &kw)?;
    let target = want_int(args.first().ok_or_else(|| crate::vm::type_error("_gt_finish() missing the thread"))?)?;
    check_target(vm, target)?;
    Err(suspend(SuspendRequest::Green(GreenRequest::Finish { target })))
}

fn current(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(Value::Int(SCHED.with(|s| s.borrow().current)))
}

fn can_switch(vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(Value::Bool(vm.rust_nest.get() == 1))
}

/// A fatia de tempo das threads verdes, o `sys.getswitchinterval()` da GIL: uma thread que roda sem bloquear
/// passa a vez quando a fatia vence, como o pedido de troca (`eval_breaker`) do CPython.
struct Slice {
    /// Quando a thread que roda ganhou a vez.
    start: Instant,
    interval: Duration,
    /// Instruções desde a última olhada no relógio.
    ticks: u32,
}

/// De quantas em quantas instruções o laço olha o relógio (só com outra thread viva).
const SLICE_CHECK_EVERY: u32 = 64;

thread_local! {
    static SLICE: RefCell<Slice> = RefCell::new(Slice {
        start: Instant::now(),
        interval: Duration::from_micros(5000),
        ticks: 0,
    });
}

/// Começa a fatia da thread que acabou de ganhar a vez.
fn restart_slice() {
    SLICE.with(|s| {
        let mut s = s.borrow_mut();
        s.start = Instant::now();
        s.ticks = 0;
    });
}

/// Entre instruções: a fatia da thread que roda venceu e há outra thread para quem passar a vez. Quando vence,
/// a fatia recomeça (sem ninguém pronto, a thread segue e só volta a perguntar no fim da próxima).
pub(crate) fn slice_expired() -> bool {
    if !OTHERS.with(Cell::get) {
        return false;
    }
    SLICE.with(|s| {
        let mut s = s.borrow_mut();
        s.ticks += 1;
        if s.ticks < SLICE_CHECK_EVERY {
            return false;
        }
        s.ticks = 0;
        let now = Instant::now();
        if now.saturating_duration_since(s.start) < s.interval {
            return false;
        }
        s.start = now;
        true
    })
}

/// `_gt_interval(segundos)`: o `sys.setswitchinterval` muda a fatia.
fn interval(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("_gt_interval", &kw)?;
    let secs = match args.first() {
        Some(Value::Float(f)) => *f,
        Some(Value::Int(i)) => *i as f64,
        _ => return Err(crate::vm::type_error("_gt_interval() takes a number of seconds")),
    };
    let interval = Duration::try_from_secs_f64(secs).map_err(|_| exc("ValueError", "switch interval must be strictly positive"))?;
    SLICE.with(|s| s.borrow_mut().interval = interval);
    Ok(Value::None)
}

/// Registra as nativas no módulo `_sys`.
pub(crate) fn register(builder: ModuleBuilder) -> ModuleBuilder {
    builder
        .func("_gt_spawn", spawn)
        .func("_gt_switch", switch)
        .func("_gt_finish", finish)
        .func("_gt_current", current)
        .func("_gt_can_switch", can_switch)
        .func("_gt_interval", interval)
}
