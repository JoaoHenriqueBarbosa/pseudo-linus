//! Finalização de instâncias: `__del__` como no CPython (PEP 442).
//!
//! O Rust não deixa o `Drop` chamar Python (a VM está no meio de uma instrução, com empréstimos
//! abertos), então o caminho tem três passos:
//!
//! 1. Quando a última referência `Rc` de uma instância cuja classe tem `__del__` cai, o `Drop` de
//!    `InstanceObj` guarda os dados numa instância nova (ressuscitada, já marcada como finalizada) e a
//!    põe na fila da thread (`enqueue`).
//! 2. O laço de instruções olha a bandeira `pending` antes de cada instrução e, se ela estiver acesa,
//!    chama `Vm::run_finalizers`: o ponto seguro logo depois da instrução que soltou a referência.
//! 3. `run_finalizers` chama o `__del__` de cada objeto da fila, na ordem em que as referências caíram.
//!    Uma exceção dentro dele nunca sobe: vira `sys.unraisablehook`, como o `PyErr_WriteUnraisable`.
//!
//! Cada objeto é finalizado no máximo uma vez (`InstanceObj::finalized`): o `Drop` da instância
//! ressuscitada, e o de uma que o `__del__` guardou em outro lugar, não enfileiram de novo.
//!
//! O gerador (e a corrente) suspenso segue o mesmo caminho: o `Drop` de `GenCore` move o quadro para um
//! núcleo ressuscitado e o enfileira (`enqueue_generator`); `run_finalizers` o fecha (`GenCore::reap`, o
//! `close()` do CPython no descarte), e os `finally`, `with` e `except GeneratorExit` abertos rodam, inclusive
//! os de `yield from` aninhado. Quadro sem o que desfazer não entra na fila.
//!
//! Objetos em ciclo de referências não têm a última referência caindo, e a VM não tem coletor de ciclos:
//! ficam sem finalizar até a saída do interpretador (`finalize_at_exit`), como no CPython antes de um
//! `gc.collect()`. A saída finaliza tudo o que ainda vive, ciclos inclusive, na ordem de criação, que é
//! a ordem em que o coletor de lixo do CPython finaliza o lixo do desligamento (módulos com funções
//! formam ciclo com as próprias globais, então é esse o caminho de quase todo script).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use crate::generator::GenCore;
use crate::object::{InstanceObj, Value};
use crate::vm::{PyException, Vm};

/// Teto de rodadas da finalização na saída: um `__del__` que cria sempre mais objetos finalizáveis não
/// prende o desligamento para sempre.
const EXIT_ROUNDS: usize = 32;

/// Um objeto a finalizar: a instância com `__del__` (chama o `__del__`) ou o gerador suspenso (chama o
/// `close()`, que roda os `finally` abertos).
enum Doomed {
    Instance(Rc<InstanceObj>),
    Generator(Rc<GenCore>),
}

/// A referência fraca de um objeto finalizável vivo.
enum Live {
    Instance(Weak<InstanceObj>),
    Generator(Weak<GenCore>),
}

impl Live {
    fn alive(&self) -> bool {
        match self {
            Live::Instance(w) => w.strong_count() > 0,
            Live::Generator(w) => w.strong_count() > 0,
        }
    }

    fn upgrade(&self) -> Option<Doomed> {
        match self {
            Live::Instance(w) => w.upgrade().map(Doomed::Instance),
            Live::Generator(w) => w.upgrade().map(Doomed::Generator),
        }
    }
}

/// Os objetos vivos finalizáveis (instâncias de classes com `__del__` e geradores), em ordem de criação,
/// para a finalização da saída. `limit` é o tamanho em que a lista descarta os mortos (dobra a cada poda),
/// para que o registro de um programa que cria milhões de objetos curtos não cresça sem fim.
struct Registry {
    items: Vec<Live>,
    limit: usize,
}

thread_local! {
    static PENDING: Cell<bool> = const { Cell::new(false) };
    static QUEUE: RefCell<Vec<Doomed>> = const { RefCell::new(Vec::new()) };
    static LIVE: RefCell<Registry> = const { RefCell::new(Registry { items: Vec::new(), limit: 1024 }) };
}

/// Há objetos na fila de finalização desta thread? Lido a cada instrução.
#[inline]
pub(crate) fn pending() -> bool {
    PENDING.with(Cell::get)
}

/// Entrega à fila da thread um objeto ressuscitado pelo `Drop`. Com a fila fora de uso (thread em
/// desmontagem), o objeto cai junto e fica sem finalizar.
pub(crate) fn enqueue(obj: Rc<InstanceObj>) {
    push(Doomed::Instance(obj));
}

/// O mesmo para o gerador suspenso que perdeu a última referência (`Drop` de `GenCore`).
pub(crate) fn enqueue_generator(core: Rc<GenCore>) {
    push(Doomed::Generator(core));
}

fn push(obj: Doomed) {
    let _ = QUEUE.try_with(|q| {
        if let Ok(mut q) = q.try_borrow_mut() {
            q.push(obj);
        }
    });
    let _ = PENDING.try_with(|p| p.set(true));
}

/// Inclui uma instância recém-criada no registro da finalização da saída.
pub(crate) fn register(inst: &Rc<InstanceObj>) {
    remember(Live::Instance(Rc::downgrade(inst)));
}

/// Inclui um gerador recém-criado no registro da finalização da saída.
pub(crate) fn register_generator(core: &Rc<GenCore>) {
    remember(Live::Generator(Rc::downgrade(core)));
}

fn remember(live: Live) {
    let _ = LIVE.try_with(|r| {
        let Ok(mut r) = r.try_borrow_mut() else { return };
        if r.items.len() >= r.limit {
            r.items.retain(Live::alive);
            r.limit = (r.items.len() * 2).max(1024);
        }
        r.items.push(live);
    });
}

impl Vm {
    /// Esvazia a fila de finalização: chama o `__del__` de cada objeto, inclusive dos que a própria
    /// finalização enfileirar. É o ponto seguro: só roda entre instruções.
    pub(crate) fn run_finalizers(&mut self) {
        loop {
            PENDING.with(|p| p.set(false));
            let batch = QUEUE
                .try_with(|q| q.try_borrow_mut().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default())
                .unwrap_or_default();
            if batch.is_empty() {
                return;
            }
            for obj in &batch {
                self.finalize_doomed(obj);
            }
        }
    }

    /// A finalização do desligamento: depois do `atexit`, o `__del__` de tudo o que ainda vive, na ordem
    /// de criação, repetindo enquanto a finalização criar objetos novos (até `EXIT_ROUNDS`).
    pub(crate) fn finalize_at_exit(&mut self) {
        for _ in 0..EXIT_ROUNDS {
            self.run_finalizers();
            let items = LIVE
                .try_with(|r| r.try_borrow_mut().map(|mut r| std::mem::take(&mut r.items)).unwrap_or_default())
                .unwrap_or_default();
            let live: Vec<Doomed> = items.iter().filter_map(Live::upgrade).collect();
            drop(items);
            if live.is_empty() {
                return;
            }
            for obj in live.iter().filter(|o| !matches!(o, Doomed::Instance(i) if i.finalized.get())) {
                self.finalize_doomed(obj);
            }
        }
        self.run_finalizers();
    }

    fn finalize_doomed(&mut self, obj: &Doomed) {
        match obj {
            Doomed::Instance(inst) => self.finalize(inst),
            Doomed::Generator(core) => core.reap(self),
        }
    }

    /// Chama o `__del__` de `obj` (marcando-o antes como finalizado). O estado de exceções tratadas fica
    /// como estava, e o erro do `__del__` vai para o `unraisablehook`.
    fn finalize(&mut self, obj: &Rc<InstanceObj>) {
        obj.finalized.set(true);
        let Some(del) = obj.class().finalizer() else { return };
        let handled = self.handled.borrow().len();
        let this = Value::Instance(obj.clone());
        let outcome = self.call_dunder(&this, "__del__", Vec::new());
        self.handled.borrow_mut().truncate(handled);
        if let Some(Err(e)) = outcome {
            self.write_unraisable(&del, e);
        }
    }

    /// `PyErr_WriteUnraisable(object)`: entrega a exceção que não pode subir ao `sys.unraisablehook`. Se o
    /// próprio gancho falhar, o erro dele vai ao gancho padrão com a mensagem do CPython.
    pub(crate) fn write_unraisable(&mut self, object: &Value, e: PyException) {
        let Ok(sys) = crate::modules::import_checked(self, "sys") else { return };
        let sys = Value::Module(sys);
        let (Ok(hook), Ok(default)) = (self.load_attr(&sys, "unraisablehook"), self.load_attr(&sys, "__unraisablehook__"))
        else {
            return;
        };
        let Err(failed) = self.call_unraisable(&hook, Value::None, object, e) else { return };
        let msg = Value::str("Exception ignored in sys.unraisablehook");
        let _ = self.call_unraisable(&default, msg, &hook, failed);
    }

    /// Monta o `sys.UnraisableHookArgs` de `e` e chama `hook` com ele. `Err` é a exceção do gancho.
    fn call_unraisable(
        &mut self,
        hook: &Value,
        err_msg: Value,
        object: &Value,
        mut e: PyException,
    ) -> Result<(), PyException> {
        let value = e.to_value();
        e.take_reraise_mark();
        let entries = e.tb.iter().rev().cloned().collect();
        let tb = crate::tbobj::TracebackObj::make(entries, &self.script_name());
        crate::vm::attach_traceback(&value, tb.clone());
        let exc_type = self.call(&Value::Builtin("type"), vec![value.clone()], Vec::new())?;
        let support = Value::Module(crate::modules::import_checked(self, "_unraisable")?);
        let args_type = self.load_attr(&support, "UnraisableHookArgs")?;
        let args = self.call(&args_type, vec![Value::tuple(vec![exc_type, value, tb, err_msg, object.clone()])], Vec::new())?;
        self.call(hook, vec![args], Vec::new()).map(|_| ())
    }
}
