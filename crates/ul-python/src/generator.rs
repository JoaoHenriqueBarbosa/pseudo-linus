//! Geradores, correntes (coroutines) e geradores assíncronos: uma função com `yield` ou `async def`
//! vira um objeto que guarda o quadro (pilha, blocos protegidos e `pc`) entre uma retomada e a seguinte.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::compile::{Code, Op};
use crate::frameobj::FrameLink;
use crate::object::{Env, ExcObj, ExtImage, ExtObject, Kw, Value};
use crate::vm::{exc, type_error, Exit, Frame, PyException, PyResult, Slot, Vm};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Generator,
    Coroutine,
    AsyncGenerator,
}

struct GenState {
    /// O quadro suspenso: pilha, blocos, `pc` e as exceções em tratamento (o CPython guarda o
    /// `exc_info` no próprio gerador).
    frame: Frame,
    started: bool,
    done: bool,
    running: bool,
    /// O valor do `return` que terminou o gerador, guardado só com o rastreio ligado, para o `for`
    /// que o consumiu gerar o evento `exception` do `StopIteration(valor)` (ver [`take_stop_value`]).
    returned: Option<Value>,
}

/// O que uma retomada produziu: um valor entregue (`yield`/suspensão) ou o fim com o valor de retorno.
pub enum Resumed {
    Yield(Value),
    Return(Value),
}

/// Os sinalizadores de um gerador: se já começou, se terminou e se está em execução.
#[derive(Clone, Copy)]
pub struct GenFlags {
    pub started: bool,
    pub done: bool,
    pub running: bool,
}

/// O estado compartilhado de um gerador, de uma corrente ou de um gerador assíncrono: o gerador, o
/// `coroutine_wrapper` e os aguardáveis de `__anext__` enxergam o mesmo.
pub struct GenCore {
    vm: Vm,
    kind: Kind,
    state: RefCell<GenState>,
    /// Este núcleo é o que o `Drop` ressuscitou para a finalização (ver `crate::finalize`): não volta à fila.
    resurrected: bool,
}

/// O quadro suspenso que `close()` não retomaria (nada a desfazer): sem bloco protegido, sem exceção em
/// tratamento e sem delegação pendente. Terminado ou ainda não iniciado também conta.
fn is_plain(st: &GenState) -> bool {
    st.done
        || !st.started
        || (!st.running
            && st.frame.blocks.is_empty()
            && st.frame.handled.is_empty()
            && !matches!(st.frame.code.ops.get(st.frame.pc), Some(Op::DelegateNext(_))))
}

/// A última referência a um gerador (ou corrente) suspenso caiu: o CPython chama `close()` na hora, e os
/// `finally`, `with` e `except GeneratorExit` abertos rodam. O `Drop` não alcança a `Vm`: move o quadro
/// para um núcleo novo e o põe na fila de finalização, que a `Vm` esvazia entre instruções.
impl Drop for GenCore {
    fn drop(&mut self) {
        if self.resurrected || self.kind == Kind::AsyncGenerator || is_plain(self.state.get_mut()) {
            return;
        }
        let st = self.state.get_mut();
        let placeholder = Frame::new(st.frame.code.clone(), st.frame.env.clone());
        let frame = std::mem::replace(&mut st.frame, placeholder);
        crate::finalize::enqueue_generator(Rc::new(GenCore {
            vm: self.vm.clone(),
            kind: self.kind,
            state: RefCell::new(GenState { frame, started: true, done: false, running: false, returned: None }),
            resurrected: true,
        }));
    }
}

impl GenCore {
    /// O `close()` da finalização (descarte da última referência ou saída do interpretador): um erro do
    /// fechamento vai ao `sys.unraisablehook`, e as exceções tratadas de quem está rodando ficam como estavam.
    pub(crate) fn reap(self: &Rc<Self>, vm: &mut Vm) {
        if self.state.borrow().running {
            return;
        }
        let handled = vm.handled.borrow().len();
        let outcome = self.close_with_exit();
        vm.handled.borrow_mut().truncate(handled);
        if let Err(e) = outcome {
            self.close();
            vm.write_unraisable(&wrap_core(self.clone(), GenRole::Object), e);
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// As globais da `Vm` com que o gerador foi criado (a do módulo da função).
    pub fn globals(&self) -> Rc<RefCell<crate::object::VarMap>> {
        self.vm.globals.clone()
    }

    /// Lê o quadro suspenso, os sinalizadores e o valor de retorno guardado.
    pub(crate) fn inspect<R>(&self, f: impl FnOnce(&Frame, GenFlags, Option<&Value>) -> R) -> R {
        let st = self.state.borrow();
        f(&st.frame, GenFlags { started: st.started, done: st.done, running: st.running }, st.returned.as_ref())
    }

    /// Altera o quadro suspenso e o valor de retorno guardado (a reconstrução da imagem do heap).
    pub(crate) fn edit<R>(&self, f: impl FnOnce(&mut Frame, &mut Option<Value>) -> R) -> R {
        let mut st = self.state.borrow_mut();
        let GenState { frame, returned, .. } = &mut *st;
        f(frame, returned)
    }

    /// Um gerador no estado `flags`, com o quadro `frame`, sobre uma cópia de `shell` com `globals`.
    pub(crate) fn rebuilt(
        shell: &Vm,
        kind: Kind,
        globals: Rc<RefCell<crate::object::VarMap>>,
        frame: Frame,
        flags: GenFlags,
    ) -> Rc<GenCore> {
        let mut vm = shell.clone();
        vm.globals = globals;
        Rc::new(GenCore {
            vm,
            kind,
            state: RefCell::new(GenState {
                frame,
                started: flags.started,
                done: flags.done,
                running: flags.running,
                returned: None,
            }),
            resurrected: false,
        })
    }
}

/// Qual objeto Python enxerga um [`GenCore`].
pub enum GenRole {
    /// O gerador, a corrente ou o gerador assíncrono em si.
    Object,
    /// O iterador de `coroutine.__await__()`.
    CoroWrapper,
    /// O aguardável de `agen.__anext__()`, `asend`, `athrow` ou `aclose`.
    Await { mode: Option<AwaitMode>, started: bool, done: bool, closing: bool },
}

/// O que o aguardável de um gerador assíncrono vai fazer na retomada (a exceção de `athrow` como valor).
pub enum AwaitMode {
    Send(Value),
    Throw(Value),
    Close,
}

/// O objeto Python que enxerga `core` no papel `role`.
pub(crate) fn wrap_core(core: Rc<GenCore>, role: GenRole) -> Value {
    match role {
        GenRole::Object => Value::Ext(Rc::new(GenObj { core })),
        GenRole::CoroWrapper => Value::Ext(Rc::new(CoroWrapper { core })),
        GenRole::Await { mode, started, done, closing } => {
            let mode = mode.map(|m| match m {
                AwaitMode::Send(v) => AGMode::Send(v),
                AwaitMode::Throw(v) => AGMode::Throw(PyException::from_value(&v)),
                AwaitMode::Close => AGMode::Close,
            });
            Value::Ext(Rc::new(AGAwait {
                core,
                mode: RefCell::new(mode),
                started: Cell::new(started),
                done: Cell::new(done),
                closing: Cell::new(closing),
            }))
        }
    }
}

pub struct GenObj {
    core: Rc<GenCore>,
}

/// Cria o objeto de uma chamada de função com `yield` ou `async def`; o corpo só roda na primeira
/// retomada.
pub fn new_generator(vm: Vm, code: Rc<Code>, env: Rc<Env>) -> Value {
    let kind = match (code.is_async, code.is_generator) {
        (true, true) => Kind::AsyncGenerator,
        (true, false) => Kind::Coroutine,
        _ => Kind::Generator,
    };
    let core = Rc::new(GenCore {
        vm,
        kind,
        state: RefCell::new(GenState { frame: Frame::new(code, env), started: false, done: false, running: false, returned: None }),
        resurrected: false,
    });
    if kind != Kind::AsyncGenerator {
        crate::finalize::register_generator(&core);
    }
    Value::Ext(Rc::new(GenObj { core }))
}

/// `StopIteration(valor)`: o `return valor` de um gerador ou corrente.
pub fn stop_iteration(value: Value) -> PyException {
    let args = if matches!(value, Value::None) { Vec::new() } else { vec![value] };
    PyException::from_value(&Value::Exception(Rc::new(ExcObj::new("StopIteration", args))))
}

/// O valor carregado por um `StopIteration` (`None` se não houver).
pub fn stop_value(e: &PyException) -> Value {
    match &e.value {
        Some(Value::Exception(x)) => x.args.first().cloned().unwrap_or(Value::None),
        Some(Value::Instance(i)) => match i.dict.borrow().get("args") {
            Some(Value::Tuple(t)) => t.first().cloned().unwrap_or(Value::None),
            _ => Value::None,
        },
        _ => Value::None,
    }
}

/// Valor entregue por um `yield` de gerador assíncrono, para o distinguir de um `await` suspenso.
struct AsyncGenWrapped(Value);

impl ExtObject for AsyncGenWrapped {
    fn type_name(&self) -> &'static str {
        "async_generator_wrapped_value"
    }
    fn image(&self) -> Option<ExtImage> {
        Some(ExtImage::AsyncGenWrapped(self.0.clone()))
    }
    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        (name == "value").then(|| Ok(self.0.clone()))
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        Err(exc("AttributeError", format!("object has no attribute '{name}'")))
    }
}

pub fn wrap_async_value(v: Value) -> Value {
    Value::Ext(Rc::new(AsyncGenWrapped(v)))
}

fn unwrap_async_value(vm: &mut Vm, v: &Value) -> Option<Value> {
    match v {
        Value::Ext(e) if e.type_name() == "async_generator_wrapped_value" => e.getattr(vm, "value").and_then(Result::ok),
        _ => None,
    }
}

/// A linha em que o quadro suspenso parou: a do `yield` que acabou de entregar, a da delegação
/// (`await`, `yield from`) que o segura, ou a da definição se ainda não começou.
fn suspended_line(frame: &Frame) -> usize {
    let code = &frame.code;
    let at = match frame.pc.checked_sub(1).map(|p| (p, code.ops.get(p))) {
        Some((p, Some(Op::Yield | Op::AsyncGenYield))) => p,
        _ => frame.pc,
    };
    code.lines.get(at).copied().unwrap_or(code.first_line)
}

/// O valor de retorno do gerador que o `for` acabou de esgotar, se ele devolveu algo além de `None`:
/// o CPython monta `StopIteration(valor)` e o `FOR_ITER` o engole depois de gerar o evento
/// `exception`. Só há valor guardado com o rastreio ligado.
pub(crate) fn take_stop_value(it: &crate::vm::PyIter) -> Option<Value> {
    let crate::vm::PyIter::Ext(x) = it else { return None };
    let generator = x.as_any()?.downcast_ref::<GenObj>()?;
    generator.core.take_returned()
}

/// O núcleo de gerador ou de corrente que o objeto `x` enxerga (o gerador ou o iterador de
/// `coroutine.__await__()`): é o que o laço de quadros retoma sem recursar. O gerador assíncrono fica
/// de fora (o `await` dele passa pelo aguardável de `__anext__`).
pub(crate) fn core_of(x: &Rc<dyn ExtObject>) -> Option<Rc<GenCore>> {
    let any = x.as_any()?;
    let core = match any.downcast_ref::<GenObj>() {
        Some(generator) => &generator.core,
        None => &any.downcast_ref::<CoroWrapper>()?.core,
    };
    (core.kind != Kind::AsyncGenerator).then(|| core.clone())
}

/// O que o laço de quadros faz com o desfecho de uma retomada que ele mesmo executou.
pub(crate) enum ResumeUse {
    /// `for` sobre o gerador: o valor entregue vira o próximo item; o fim descarta o iterador e salta
    /// para `exit`.
    ForIter { exit: usize },
    /// `next(g)`, `g.send(v)` e `g.__next__()`: o valor entregue é o resultado; o fim vira
    /// `StopIteration(retorno)`, ou `default`, quando `next` recebeu um.
    Next { default: Option<Value> },
    /// `yield from` e `await` sobre o gerador no topo da pilha: o valor entregue é reentregue pelo
    /// gerador de fora; o fim troca o sub-iterador pelo valor de retorno e salta para `end`.
    Delegate { end: usize },
    /// `throw` num gerador de fora parado em `yield from`/`await` sobre um gerador: a exceção entra no quadro do
    /// sub-gerador (`_gen_throw` do CPython) e o desfecho volta ao de fora. Um valor entregue é reentregue pelo
    /// de fora (que volta ao `Yield` da delegação); o fim troca o sub-iterador pelo valor de retorno e salta para
    /// `end`; uma exceção sobe ao de fora, no ponto da suspensão.
    DelegateThrow { end: usize },
    /// `close()` (ou `throw(GeneratorExit)`) num gerador de fora parado numa delegação: o sub-gerador é fechado
    /// primeiro (`gen_close_iter`); sem erro, `exit` é levantada no de fora, e com erro (inclusive o
    /// `RuntimeError` de "ignored GeneratorExit") sobe o erro do fechamento no lugar dela. O desfecho passa por
    /// `GenCore::settle_close`.
    DelegateExit(PyException),
    /// `g.close()`: o desfecho passa por `GenCore::settle_close` e o resultado é sempre `None`.
    Close,
    /// `list(g)`, `tuple(g)`, `sorted(g, ...)`, `s.join(g)` e `*g` (em chamada, lista ou tupla): o gerador é
    /// esgotado quadro a quadro numa lista e só então a função é chamada com ela, ou a lista de destino é
    /// estendida (o CPython faz o mesmo: esgota antes de usar). Com `Collect::fold` (`sum`, `set`, `dict`,
    /// `min`, `max`, `any`, `all`, `list.extend`) cada item entra na conta na hora, como o laço em C do
    /// CPython, e a conta pode parar antes do fim. Só aparece dentro de um [`Pull`].
    Collect(Box<Collect>),
    /// O quadro de um gerador que alimenta um consumidor por item (`Collect`, `for` ou `next`) quando há
    /// camadas preguiçosas no meio (`enumerate`, `zip`, `map`, `filter`) ou o consumidor é uma coleta: o
    /// desfecho volta pelas camadas e só então chega ao consumidor.
    Pull(Box<Pull>),
}

/// O que o laço acumula ao esgotar um gerador para uma função nativa.
pub(crate) struct Collect {
    /// Os valores entregues até agora (a coleta sem `fold`).
    pub(crate) items: Vec<Value>,
    /// A função chamada com a lista no fim (`list`, `tuple`, `sorted`, `str.join`); ou, no `*g` de uma
    /// lista em construção (`ListExtend`), a própria lista a estender com o que o gerador entregou.
    pub(crate) call: Value,
    /// Os argumentos nomeados da chamada original (`sorted(g, key=..., reverse=...)`).
    pub(crate) kwargs: Vec<(String, Value)>,
    /// A conta por item de um consumidor que não junta tudo antes (`call` e `items` ficam sem uso).
    pub(crate) fold: Option<crate::fold::Fold>,
}

/// A retomada de uma fonte (`root`: um gerador ou uma cadeia preguiçosa sobre geradores) para o consumidor
/// `then`. `layers` são as camadas da cadeia já descidas, da mais externa para a mais interna: o que o
/// gerador entregar sobe por elas antes de chegar a `then`.
pub(crate) struct Pull {
    pub(crate) root: Value,
    pub(crate) layers: Vec<Layer>,
    pub(crate) then: ResumeUse,
}

/// Uma camada preguiçosa a meio caminho: o iterador (`enumerate`, `filter`, `map` ou `zip`) e o que ela já
/// juntou das fontes antes da que está sendo puxada.
pub(crate) enum Layer {
    Enumerate(Rc<dyn ExtObject>),
    Filter(Rc<dyn ExtObject>),
    /// `map` e `zip`: a fonte `at` está sendo puxada, e `got` são os itens que as anteriores entregaram.
    Many { it: Rc<dyn ExtObject>, at: usize, got: Vec<Value> },
    /// `zip(strict=True)` cuja primeira fonte acabou: a fonte `at` está sendo puxada para conferir que
    /// também acabou (um item a mais é o `ValueError` de "longer").
    Check { it: Rc<dyn ExtObject>, at: usize },
}

/// Um callback Python que a nativa chamaria recursivamente e que o laço executa em quadro: o consumidor em
/// andamento (`pull`, parado no ponto em que o callback foi pedido) e o que fazer com o valor que o quadro
/// devolver. Vive no `CallLink::then` do quadro do callback (`Dunder::Callback`).
pub(crate) struct Callback {
    pub(crate) pull: Pull,
    pub(crate) what: CallbackKind,
}

pub(crate) enum CallbackKind {
    /// A função do `map`: o valor devolvido é o item que sobe pelas camadas.
    Mapped,
    /// O predicado do `filter` `it` sobre `item`: com valor verdadeiro o item sobe, senão a camada pede o
    /// seguinte à fonte.
    Kept { it: Rc<dyn ExtObject>, item: Value },
    /// A chave de `item` para a conta (`min`, `max`, `sorted`): `pull.then` é a coleta.
    Keyed { item: Value },
    /// O `__next__` ou o `__getitem__` em Python da fonte folha `leaf`: o valor devolvido é o item; o
    /// `StopIteration` (no protocolo antigo também `IndexError`) é o fim da fonte.
    Advance { leaf: Rc<dyn ExtObject> },
    /// O `__iter__` em Python da instância que é a fonte da coleta: o valor devolvido é o iterador que passa
    /// a ser a fonte.
    Start,
}

impl ResumeUse {
    /// O desfecho é o de um `close()`: passa por `GenCore::settle_close`.
    pub(crate) fn closes(&self) -> bool {
        matches!(self, ResumeUse::Close | ResumeUse::DelegateExit(_))
    }
}

/// A retomada em curso, guardada no `CallLink` do quadro do gerador enquanto ele roda no laço.
pub(crate) struct Resuming {
    pub(crate) tail: Tail,
    pub(crate) use_: ResumeUse,
    /// A exceção de `throw`/`close` a levantar no ponto da suspensão; o laço a toma ao empilhar o quadro.
    pub(crate) inject: Option<PyException>,
}

/// O que `GenCore::begin` deixa para `GenCore::end` fechar a retomada.
pub(crate) struct Tail {
    pub(crate) core: Rc<GenCore>,
    /// A altura de `Vm::handled` antes de o gerador devolver as exceções que ele tratava.
    pub(crate) base: usize,
    /// A linha do chamador, devolvida a `cur_line` no fim.
    pub(crate) caller_line: usize,
    /// As globais do chamador, quando a retomada trocou `Vm::globals` pelas do gerador.
    pub(crate) caller_globals: Option<Rc<RefCell<crate::object::VarMap>>>,
}

/// O desfecho de `GenCore::prepare`.
enum Prepared {
    Ready(Resumed),
    Run { frame: Frame, first: bool, line: usize, inject: Option<PyException> },
}

/// O desfecho de `GenCore::begin`: pronto sem rodar, ou o quadro a executar com o que `end` precisa.
pub(crate) enum Begun {
    Ready(Resumed),
    Run(Frame, Tail, Option<PyException>),
}

impl GenCore {
    /// O valor do `return` que terminou o gerador, guardado só com o rastreio ligado (ver `returned`).
    pub(crate) fn take_returned(&self) -> Option<Value> {
        self.state.borrow_mut().returned.take()
    }

    fn what(&self) -> &'static str {
        match self.kind {
            Kind::Generator => "generator",
            Kind::Coroutine => "coroutine",
            Kind::AsyncGenerator => "async generator",
        }
    }

    /// `close()` de gerador e corrente: lança `GeneratorExit` no ponto da suspensão, para os `finally`
    /// e `with` abertos rodarem. Sem bloco protegido nem exceção em tratamento (e sem delegação) o
    /// CPython 3.13 só marca o quadro como terminado, sem retomá-lo.
    fn close_with_exit(self: &Rc<Self>) -> PyResult<Value> {
        if self.close_if_plain() {
            return Ok(Value::None);
        }
        self.settle_close(self.resume(None, Some(exc("GeneratorExit", "")))).map(|_| Value::None)
    }

    /// O primeiro passo de `close()`: o quadro que não tem o que desfazer só é marcado como terminado.
    /// Devolve se foi esse o caso (nada a retomar).
    pub(crate) fn close_if_plain(&self) -> bool {
        let plain = is_plain(&self.state.borrow());
        if plain {
            self.close();
        }
        plain
    }

    /// O desfecho da retomada com `GeneratorExit` de `close()`: terminar (ou levantar o próprio
    /// `GeneratorExit`/`StopIteration`) é fechar; um `yield` é erro.
    pub(crate) fn settle_close(&self, r: PyResult<Resumed>) -> PyResult<Resumed> {
        match r {
            Ok(Resumed::Yield(_)) => Err(exc("RuntimeError", format!("{} ignored GeneratorExit", self.what()))),
            Ok(Resumed::Return(_)) => Ok(Resumed::Return(Value::None)),
            Err(e) if matches!(e.kind, "GeneratorExit" | "StopIteration") => Ok(Resumed::Return(Value::None)),
            Err(e) => Err(e),
        }
    }

    /// Retoma o quadro; com `inject`, a exceção é levantada no ponto em que ele parou. Aqui o quadro roda
    /// num `run_loop` aninhado (os chamadores nativos); o laço de quadros retoma sem recursar por
    /// `Vm::enter_resume`, que usa as mesmas `begin` e `end`.
    fn resume(self: &Rc<Self>, sent: Option<Value>, inject: Option<PyException>) -> PyResult<Resumed> {
        let mut vm = self.vm.clone();
        match GenCore::begin(self, &mut vm, sent, inject, None)? {
            Begun::Ready(done) => Ok(done),
            Begun::Run(mut frame, tail, inject) => {
                let result = vm.run_loop(&mut frame, inject);
                self.end(&mut vm, tail, frame, result)
            }
        }
    }

    /// Confere o estado do gerador e o põe em execução: o quadro de fora, a linha em que parou e o que
    /// `throw` injetou. Um desfecho que não precisa de execução (gerador terminado) sai como `Ready`.
    fn prepare(&self, sent: Option<Value>, inject: Option<PyException>) -> PyResult<Prepared> {
        let what = self.what();
        let mut st = self.state.borrow_mut();
        if st.running {
            return Err(exc("ValueError", format!("{what} already executing")));
        }
        if st.done {
            if self.kind == Kind::Coroutine && inject.is_none() {
                return Err(exc("RuntimeError", "cannot reuse already awaited coroutine"));
            }
            return match inject {
                Some(e) => Err(e),
                None => Ok(Prepared::Ready(Resumed::Return(Value::None))),
            };
        }
        if !st.started {
            if let Some(e) = inject {
                st.done = true;
                return Err(e);
            }
            if sent.is_some_and(|v| !matches!(v, Value::None)) {
                return Err(type_error(format!("can't send non-None value to a just-started {what}")));
            }
        } else if inject.is_none() {
            st.frame.stack.push(Slot::Val(sent.unwrap_or(Value::None)));
        }
        let first = !st.started;
        let line = suspended_line(&st.frame);
        st.started = true;
        st.running = true;
        // Os blocos guardam a altura absoluta da pilha: retomado sob outra altura, rebaseia.
        let base = self.vm.handled_len();
        let old = st.frame.handled_base;
        if base != old {
            for b in st.frame.blocks.iter_mut() {
                b.handled = (b.handled + base).saturating_sub(old);
            }
        }
        st.frame.handled_base = base;
        self.vm.handled_extend(std::mem::take(&mut st.frame.handled));
        // O quadro de fora fica com o código e o ambiente (o `repr` os lê durante a execução).
        let frame = Frame {
            code: st.frame.code.clone(),
            env: st.frame.env.clone(),
            stack: std::mem::take(&mut st.frame.stack),
            blocks: std::mem::take(&mut st.frame.blocks),
            pc: st.frame.pc,
            handled: Vec::new(),
            handled_base: base,
        };
        Ok(Prepared::Run { frame, first, line, inject })
    }

    /// Começa uma retomada sobre `vm`: o quadro do gerador entra na pilha de quadros (`sys._getframe`,
    /// `f_back`, eventos do `sys.settrace`) e o evento `call` é gerado. `caller_globals` são as globais que
    /// `vm` tinha antes de trocá-las pelas do gerador; voltam em `end` (ou aqui, se nada chega a rodar).
    pub(crate) fn begin(
        core: &Rc<GenCore>,
        vm: &mut Vm,
        sent: Option<Value>,
        inject: Option<PyException>,
        caller_globals: Option<Rc<RefCell<crate::object::VarMap>>>,
    ) -> PyResult<Begun> {
        let restore = |vm: &mut Vm, globals: Option<Rc<RefCell<crate::object::VarMap>>>| {
            if let Some(g) = globals {
                vm.globals = g;
            }
        };
        let (frame, first, line, inject) = match core.prepare(sent, inject) {
            Ok(Prepared::Run { frame, first, line, inject }) => (frame, first, line, inject),
            Ok(Prepared::Ready(done)) => {
                restore(vm, caller_globals);
                return Ok(Begun::Ready(done));
            }
            Err(e) => {
                restore(vm, caller_globals);
                return Err(e);
            }
        };
        let base = vm.handled_len().min(core.state.borrow().frame.handled_base);
        let caller_line = vm.cur_line.get();
        vm.frames.borrow_mut().push((frame.code.clone(), caller_line, frame.env.clone()));
        let tail = Tail { core: core.clone(), base, caller_line, caller_globals };
        match crate::tracing::resume(vm, &frame.code, (!first).then_some(line)) {
            Ok(()) => Ok(Begun::Run(frame, tail, inject)),
            Err(e) => core.end(vm, tail, frame, Err(e)).map(Begun::Ready),
        }
    }

    /// Termina uma retomada: `frame` é o quadro que rodou e `result` o que o laço devolveu. Guarda o quadro
    /// de volta no gerador (suspenso) ou o marca terminado.
    pub(crate) fn end(&self, vm: &mut Vm, tail: Tail, frame: Frame, result: PyResult<Exit>) -> PyResult<Resumed> {
        let Tail { base, caller_line, caller_globals, .. } = tail;
        let mut result = result;
        // `return` do rastreio e do perfil: ao suspender com o valor entregue, ao terminar com o de retorno.
        let hook_error = if crate::tracing::hooked() {
            let seen = match &result {
                Ok(Exit::Yield(v)) => Ok(unwrap_async_value(vm, v).unwrap_or_else(|| v.clone())),
                Ok(Exit::Return(v)) => Ok(v.clone()),
                Err(e) => Err(e.clone()),
            };
            crate::tracing::leave(vm, &seen).err()
        } else {
            None
        };
        vm.frames.borrow_mut().pop();
        vm.cur_line.set(caller_line);
        crate::frameobj::suspend_frame(&frame.env);
        let inner = vm.handled_split(base);
        if let Some(globals) = caller_globals {
            vm.globals = globals;
        }
        let mut st = self.state.borrow_mut();
        st.running = false;
        if let (Some(e), Ok(Exit::Return(_))) = (&hook_error, &result) {
            result = Err(e.clone());
        }
        let outcome = match result {
            Ok(Exit::Yield(v)) => {
                st.frame.stack = frame.stack;
                st.frame.blocks = frame.blocks;
                st.frame.pc = frame.pc;
                st.frame.handled = inner;
                Ok(Resumed::Yield(v))
            }
            Ok(Exit::Return(v)) => {
                st.done = true;
                if crate::tracing::active() && !matches!(v, Value::None) {
                    st.returned = Some(v.clone());
                }
                Ok(Resumed::Return(v))
            }
            Err(e) => {
                st.done = true;
                Err(e)
            }
        };
        match hook_error {
            Some(e) if outcome.is_ok() => Err(e),
            _ => outcome,
        }
    }

    fn close(&self) {
        let mut st = self.state.borrow_mut();
        st.done = true;
        st.frame.stack.clear();
        st.frame.blocks.clear();
    }
}

impl GenObj {
    fn type_str(&self) -> &'static str {
        match self.core.kind {
            Kind::Generator => "generator",
            Kind::Coroutine => "coroutine",
            Kind::AsyncGenerator => "async_generator",
        }
    }

    /// `send`/`throw` de gerador e corrente: o valor entregue, ou `StopIteration(retorno)`.
    fn step(&self, sent: Option<Value>, inject: Option<PyException>) -> PyResult<Value> {
        match self.core.resume(sent, inject)? {
            Resumed::Yield(v) => Ok(v),
            Resumed::Return(v) => Err(stop_iteration(v)),
        }
    }

    fn awaitable(&self, mode: AGMode) -> Value {
        Value::Ext(Rc::new(AGAwait { core: self.core.clone(), mode: RefCell::new(Some(mode)), started: Cell::new(false), done: Cell::new(false), closing: Cell::new(false) }))
    }
}

/// O sub-iterador (ou aguardável) em que o gerador está parado numa delegação (`yield from`, `await`): o
/// topo da pilha do quadro suspenso na instrução `DelegateNext`; `None` fora disso.
fn delegated_target(st: &GenState) -> Value {
    let parked = st.started && !st.done && !st.running;
    if !parked || !matches!(st.frame.code.ops.get(st.frame.pc), Some(Op::DelegateNext(_))) {
        return Value::None;
    }
    match st.frame.stack.last() {
        Some(Slot::Val(v)) => v.clone(),
        _ => Value::None,
    }
}

impl ExtObject for GenObj {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn image(&self) -> Option<ExtImage> {
        Some(ExtImage::Generator { core: self.core.clone(), role: GenRole::Object })
    }
    fn type_name(&self) -> &'static str {
        self.type_str()
    }
    fn repr(&self) -> String {
        format!(
            "<{} object {} at {:#x}>",
            self.type_str().replace('_', " "),
            self.core.state.borrow().frame.code.name,
            crate::object::py_addr(self as *const GenObj as usize)
        )
    }
    fn methods(&self) -> &'static [&'static str] {
        match self.core.kind {
            Kind::Generator => &["send", "throw", "close", "__next__"],
            Kind::Coroutine => &["send", "throw", "close", "__await__"],
            Kind::AsyncGenerator => &["__aiter__", "__anext__", "asend", "athrow", "aclose"],
        }
    }
    fn is_iterable(&self) -> bool {
        self.core.kind == Kind::Generator
    }
    fn iter_next(&self) -> PyResult<Option<Value>> {
        match self.core.resume(None, None)? {
            Resumed::Yield(v) => Ok(Some(v)),
            Resumed::Return(_) => Ok(None),
        }
    }
    fn getattr(&self, vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        let prefix = match self.core.kind {
            Kind::Generator => "gi_",
            Kind::Coroutine => "cr_",
            Kind::AsyncGenerator => "ag_",
        };
        let st = self.core.state.borrow();
        match name {
            "__name__" => return Some(Ok(Value::str(st.frame.code.name.clone()))),
            "__qualname__" => return Some(Ok(Value::str(st.frame.code.qual()))),
            _ => {}
        }
        Some(Ok(match name.strip_prefix(prefix)? {
            "running" => Value::Bool(st.running),
            // Parado num `yield` (ou `await`): já começou, não acabou e não está rodando.
            "suspended" => Value::Bool(st.started && !st.done && !st.running),
            "code" => {
                let code = st.frame.code.clone();
                let file = if code.filename.is_empty() { vm.script_name() } else { code.filename.clone() };
                crate::tbobj::function_code(&code, &file)
            }
            // O quadro acaba quando o corpo termina: depois disso é `None`.
            "frame" if st.done => Value::None,
            "frame" => {
                let code = st.frame.code.clone();
                let line = if st.started { suspended_line(&st.frame) } else { code.first_line };
                let file: Rc<str> = if code.filename.is_empty() { vm.script_name().into() } else { code.filename.as_str().into() };
                let link = FrameLink { line, name: code.name.clone(), file, code: Some(code), env: Some(st.frame.env.clone()), caller_line: 0 };
                crate::frameobj::generator_frame(&link, st.running)
            }
            // O sub-iterador de um `yield from` (ou o aguardável de um `await`) em que o gerador está parado.
            "yieldfrom" if self.core.kind == Kind::Generator => delegated_target(&st),
            "await" if self.core.kind != Kind::Generator => delegated_target(&st),
            "origin" if self.core.kind == Kind::Coroutine => Value::None,
            _ => return None,
        }))
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "send" => {
                let [v] = <[Value; 1]>::try_from(args)
                    .map_err(|a| type_error(format!("send() takes exactly one argument ({} given)", a.len())))?;
                self.step(Some(v), None)
            }
            "throw" => {
                let Some(first) = args.into_iter().next() else {
                    return Err(type_error("throw expected at least 1 argument, got 0"));
                };
                let e = raise_for_throw(vm, first)?;
                self.step(None, Some(e))
            }
            "__next__" => self.step(None, None),
            "__await__" => Ok(Value::Ext(Rc::new(CoroWrapper { core: self.core.clone() }))),
            "close" => self.core.close_with_exit(),
            "__aiter__" => Err(type_error("__aiter__ returns self")),
            "__anext__" => Ok(self.awaitable(AGMode::Send(Value::None))),
            "asend" => {
                let [v] = <[Value; 1]>::try_from(args)
                    .map_err(|a| type_error(format!("asend() takes exactly one argument ({} given)", a.len())))?;
                Ok(self.awaitable(AGMode::Send(v)))
            }
            "athrow" => {
                let Some(first) = args.into_iter().next() else {
                    return Err(type_error("athrow expected at least 1 argument, got 0"));
                };
                let e = raise_for_throw(vm, first)?;
                Ok(self.awaitable(AGMode::Throw(e)))
            }
            "aclose" => Ok(self.awaitable(AGMode::Close)),
            _ => Err(crate::object::no_attribute(self.type_str(), name)),
        }
    }
}

/// O iterador devolvido por `coroutine.__await__()`: repassa `send`/`throw` à corrente.
struct CoroWrapper {
    core: Rc<GenCore>,
}

impl ExtObject for CoroWrapper {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        "coroutine_wrapper"
    }
    fn image(&self) -> Option<ExtImage> {
        Some(ExtImage::Generator { core: self.core.clone(), role: GenRole::CoroWrapper })
    }
    fn methods(&self) -> &'static [&'static str] {
        &["send", "throw", "close", "__next__"]
    }
    fn is_iterable(&self) -> bool {
        true
    }
    fn iter_next(&self) -> PyResult<Option<Value>> {
        match self.core.resume(None, None)? {
            Resumed::Yield(v) => Ok(Some(v)),
            Resumed::Return(_) => Ok(None),
        }
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let step = |sent, inject| match self.core.resume(sent, inject)? {
            Resumed::Yield(v) => Ok(v),
            Resumed::Return(v) => Err(stop_iteration(v)),
        };
        match name {
            "send" => step(args.into_iter().next(), None),
            "__next__" => step(None, None),
            "throw" => {
                let first = args.into_iter().next().ok_or_else(|| type_error("throw expected at least 1 argument, got 0"))?;
                let e = raise_for_throw(vm, first)?;
                step(None, Some(e))
            }
            "close" => self.core.close_with_exit(),
            _ => Err(crate::object::no_attribute("coroutine_wrapper", name)),
        }
    }
}

/// `raise_any` para o `throw`: a exceção injetada carrega o traceback que já tinha (`__traceback__`),
/// para os quadros de quem a levantou continuarem aparecendo depois de atravessar o gerador.
pub(crate) fn raise_for_throw(vm: &mut Vm, v: Value) -> PyResult<PyException> {
    let mut e = vm.raise_any(v)?;
    e.seed_traceback();
    Ok(e)
}

enum AGMode {
    Send(Value),
    Throw(PyException),
    Close,
}

/// O aguardável de `agen.__anext__()`/`asend`/`athrow`/`aclose`: o `await` dele retoma o gerador
/// assíncrono; um `yield` do corpo termina o `await` com esse valor, um `await` do corpo suspende.
struct AGAwait {
    core: Rc<GenCore>,
    mode: RefCell<Option<AGMode>>,
    started: Cell<bool>,
    done: Cell<bool>,
    closing: Cell<bool>,
}

impl AGAwait {
    fn finish(&self, r: PyResult<Resumed>, vm: &mut Vm, closing: bool) -> PyResult<Value> {
        match r {
            Ok(Resumed::Yield(v)) => match unwrap_async_value(vm, &v) {
                Some(_) if closing => {
                    self.done.set(true);
                    Err(exc("RuntimeError", "async generator ignored GeneratorExit"))
                }
                Some(inner) => {
                    self.done.set(true);
                    Err(stop_iteration(inner))
                }
                None => Ok(v),
            },
            Ok(Resumed::Return(_)) => {
                self.done.set(true);
                Err(if closing { stop_iteration(Value::None) } else { exc("StopAsyncIteration", "") })
            }
            Err(e) if closing && matches!(e.kind, "GeneratorExit" | "StopAsyncIteration") => {
                self.done.set(true);
                Err(stop_iteration(Value::None))
            }
            Err(e) => {
                self.done.set(true);
                Err(e)
            }
        }
    }

    fn advance(&self, vm: &mut Vm, arg: Option<Value>, inject: Option<PyException>) -> PyResult<Value> {
        if self.done.get() {
            return Err(exc("StopIteration", ""));
        }
        let first = !self.started.replace(true);
        let mode = if first { self.mode.borrow_mut().take() } else { None };
        if first {
            self.closing.set(matches!(mode, Some(AGMode::Close)));
        }
        let closing = self.closing.get();
        let r = match (mode, inject) {
            (_, Some(e)) => self.core.resume(None, Some(e)),
            (Some(AGMode::Send(v)), None) => self.core.resume(Some(v), None),
            (Some(AGMode::Throw(e)), None) => self.core.resume(None, Some(e)),
            (Some(AGMode::Close), None) => {
                let st = self.core.state.borrow();
                if st.done || !st.started {
                    drop(st);
                    self.core.close();
                    self.done.set(true);
                    return Err(stop_iteration(Value::None));
                }
                drop(st);
                self.core.resume(None, Some(exc("GeneratorExit", "")))
            }
            (None, None) => self.core.resume(arg, None),
        };
        self.finish(r, vm, closing)
    }
}

impl ExtObject for AGAwait {
    fn type_name(&self) -> &'static str {
        "async_generator_asend"
    }
    fn image(&self) -> Option<ExtImage> {
        let mode = self.mode.borrow().as_ref().map(|m| match m {
            AGMode::Send(v) => AwaitMode::Send(v.clone()),
            AGMode::Throw(e) => AwaitMode::Throw(e.to_value()),
            AGMode::Close => AwaitMode::Close,
        });
        Some(ExtImage::Generator {
            core: self.core.clone(),
            role: GenRole::Await { mode, started: self.started.get(), done: self.done.get(), closing: self.closing.get() },
        })
    }
    fn methods(&self) -> &'static [&'static str] {
        &["send", "throw", "close", "__next__", "__await__"]
    }
    fn is_iterable(&self) -> bool {
        true
    }
    fn iter_next(&self) -> PyResult<Option<Value>> {
        let mut vm = crate::vm::current().ok_or_else(|| exc("RuntimeError", "no running interpreter"))?;
        match self.advance(&mut vm, None, None) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.kind == "StopIteration" => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "send" => self.advance(vm, args.into_iter().next(), None),
            "__next__" => self.advance(vm, None, None),
            "throw" => {
                let first = args.into_iter().next().ok_or_else(|| type_error("throw expected at least 1 argument, got 0"))?;
                let e = raise_for_throw(vm, first)?;
                self.advance(vm, None, Some(e))
            }
            "close" => {
                self.done.set(true);
                Ok(Value::None)
            }
            "__await__" => Err(type_error("__await__ returns self")),
            _ => Err(crate::object::no_attribute("async_generator_asend", name)),
        }
    }
}
