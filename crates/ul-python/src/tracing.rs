//! `sys.settrace` e `sys.setprofile`: os eventos do CPython 3.13 para funções escritas em Python
//! (`call`, `line`, `return`, `exception`) e para as nativas (`c_call`, `c_return`, `c_exception`).
//!
//! O rastreador global recebe o `call`; o que ele devolve (se não for `None`) vira o `f_trace`
//! daquele quadro, o rastreador local que recebe os demais eventos. Quem mexe em `frame.f_trace`
//! (o `bdb` faz isso nos quadros de cima) muda o rastreador do quadro, e `f_trace_lines = False`
//! desliga os eventos `line` dele. `line`, `exception` e `return` dependem só de o quadro em
//! execução ter `f_trace` (a identidade e o `f_trace` moram no registro do `frameobj`, chaveado
//! pelo `Env` do quadro em `vm.frames`), não de ele ter sido entrado com o rastreio ligado. A
//! função de perfil recebe `call` e `return` de todo quadro Python e `c_call`, `c_return` e
//! `c_exception` das nativas, sem valor de retorno que importe. Enquanto um rastreador ou a função
//! de perfil roda, nada é rastreado nem perfilado (como no CPython). Com tudo desligado, o custo na
//! VM é a leitura de um `Cell<bool>`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::compile::Code;
use crate::frameobj::{arm_first_line, is_traced, line_changed, set_trace_of, trace_lines_of, trace_of};
use crate::modules::pysys::{current_frame, new_frame};
use crate::object::Value;
use crate::vm::{PyResult, Vm};

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static PROFILING: Cell<bool> = const { Cell::new(false) };
    static INSIDE: Cell<bool> = const { Cell::new(false) };
    static GLOBAL: RefCell<Option<Value>> = const { RefCell::new(None) };
    static PROFILE: RefCell<Option<Value>> = const { RefCell::new(None) };
}

/// Gera o par de acessores de um gancho: `$set` guarda a função (e liga a bandeira `$flag` quando há uma) e
/// `$get` a devolve, `None` se não há.
macro_rules! hook_accessors {
    ($(#[$set_meta:meta])* $set:ident, $(#[$get_meta:meta])* $get:ident, $flag:ident, $slot:ident) => {
        $(#[$set_meta])*
        pub fn $set(f: Option<Value>) {
            $flag.with(|a| a.set(f.is_some()));
            $slot.with(|g| *g.borrow_mut() = f);
        }

        $(#[$get_meta])*
        pub fn $get() -> Value {
            $slot.with(|g| g.borrow().clone()).unwrap_or(Value::None)
        }
    };
}

hook_accessors! {
    /// `sys.settrace(f)`.
    set,
    /// `sys.gettrace()`.
    get,
    ACTIVE,
    GLOBAL
}

hook_accessors! {
    /// `sys.setprofile(f)`.
    set_profile,
    /// `sys.getprofile()`.
    get_profile,
    PROFILING,
    PROFILE
}

/// Há rastreador global e nenhum rastreador ou função de perfil em execução.
#[inline]
pub fn active() -> bool {
    ACTIVE.with(Cell::get) && !INSIDE.with(Cell::get)
}

/// Há função de perfil e nenhum rastreador ou função de perfil em execução.
#[inline]
pub fn profiling() -> bool {
    PROFILING.with(Cell::get) && !INSIDE.with(Cell::get)
}

/// Algum dos dois está ligado (e fora de um callback).
#[inline]
pub fn hooked() -> bool {
    (ACTIVE.with(Cell::get) || PROFILING.with(Cell::get)) && !INSIDE.with(Cell::get)
}

/// Roda `f` com o rastreio e o perfil suspensos (o contador `tracing` do CPython).
fn inside<R>(f: impl FnOnce() -> R) -> R {
    INSIDE.with(|i| i.set(true));
    let r = f();
    INSIDE.with(|i| i.set(false));
    r
}

/// Chama o rastreador `f` com `(quadro, evento, arg)`. Como no CPython, um valor devolvido que não
/// seja `None` passa a ser o `f_trace` do quadro; `None` deixa o que lá estiver.
fn invoke(vm: &mut Vm, f: &Value, frame: &Value, event: &str, arg: Value) -> PyResult<()> {
    match inside(|| vm.call(f, vec![frame.clone(), Value::str(event), arg], Vec::new())) {
        Ok(next) => {
            if !matches!(next, Value::None) {
                set_trace_of(frame, next);
            }
            Ok(())
        }
        Err(e) => {
            // Como o CPython: rastreador que levanta é desligado.
            set(None);
            set_trace_of(frame, Value::None);
            Err(e)
        }
    }
}

/// Chama a função de perfil com `(quadro, evento, arg)`; o retorno dela não importa, e se ela
/// levantar o perfil é desligado.
fn invoke_profile(vm: &mut Vm, frame: &Value, event: &str, arg: Value) -> PyResult<()> {
    let Some(f) = PROFILE.with(|g| g.borrow().clone()) else { return Ok(()) };
    inside(|| vm.call(&f, vec![frame.clone(), Value::str(event), arg], Vec::new())).map(|_| ()).inspect_err(|_| set_profile(None))
}

/// Entrada numa função Python: dispara `call` na função de perfil e depois no rastreador global.
pub fn enter(vm: &mut Vm, code: &Rc<Code>) -> PyResult<()> {
    call_event(vm, code, None)
}

/// Retomada de um gerador, de uma corrente ou de um gerador assíncrono: cada retomada gera `call`
/// no mesmo quadro (o objeto que `gi_frame` devolve), na linha onde ele parou (`line`), ou na linha
/// da definição na primeira (`None`, que também rearma o primeiro `line`). O quadro já está em
/// `vm.frames`.
pub fn resume(vm: &mut Vm, code: &Rc<Code>, line: Option<usize>) -> PyResult<()> {
    call_event(vm, code, Some(line))
}

/// O `call` de `enter` (`resumed` é `None`: quadro novo) e de `resume` (`Some(linha)`: o quadro do
/// gerador, com a identidade que já tem).
fn call_event(vm: &mut Vm, code: &Rc<Code>, resumed: Option<Option<usize>>) -> PyResult<()> {
    // Sem rastreador nem perfil (o caso comum), nada a fazer: uma leitura só, antes de qualquer `clone`.
    if !hooked() {
        return Ok(());
    }
    let tracer = if active() { GLOBAL.with(|g| g.borrow().clone()) } else { None };
    let profile = profiling();
    if (tracer.is_none() && !profile) || crate::vm::code_is_native(code) {
        return Ok(());
    }
    let first = if code.first_line > 0 { code.first_line } else { code.lines.first().copied().unwrap_or(0) };
    let (line, arm) = match resumed {
        Some(Some(line)) => (line, false),
        _ => (first, true),
    };
    vm.cur_line.set(line);
    let frame = if resumed.is_some() { current_frame(vm)? } else { new_frame(vm)? };
    if profile {
        invoke_profile(vm, &frame, "call", Value::None)?;
    }
    // O perfil pode ter desligado o rastreio.
    if let Some(global) = tracer.filter(|_| active()) {
        if arm {
            arm_first_line(&frame);
        }
        invoke(vm, &global, &frame, "call", Value::None)?;
    }
    Ok(())
}

/// O `Env` do quadro em execução, se o topo de `vm.frames` for o de `code` (`Some(None)` no
/// `<module>`, que não tem `Env`).
fn frame_key(vm: &Vm, code: &Rc<Code>) -> Option<Option<usize>> {
    match vm.frames.borrow().last() {
        Some((top, _, env)) if Rc::ptr_eq(top, code) => Some(Some(Rc::as_ptr(env) as usize)),
        None if !code.is_function && !code.is_class => Some(None),
        _ => None,
    }
}

/// Dispara `event` no `f_trace` do quadro em execução (o de chave `env`), se ele tiver um (e, nos
/// eventos `line`, se `f_trace_lines` estiver ligado).
fn fire_local(vm: &mut Vm, env: Option<usize>, event: &str, arg: impl FnOnce(&mut Vm) -> Value) -> PyResult<()> {
    if !is_traced(env) {
        return Ok(());
    }
    let frame = current_frame(vm)?;
    if event == "line" && !trace_lines_of(&frame) {
        return Ok(());
    }
    let tracer = trace_of(&frame);
    let arg = arg(vm);
    invoke(vm, &tracer, &frame, event, arg)
}

/// Uma instrução da função `code` na linha `line`: dispara `line` quando a linha muda no quadro, e
/// só se o quadro tiver `f_trace`. Vale para qualquer quadro com `f_trace`, não só para os entrados
/// com o rastreio ligado: é assim que o chamador de `pdb.set_trace()` passa a receber `line`.
pub fn line(vm: &mut Vm, code: &Rc<Code>, line: usize) -> PyResult<()> {
    let Some(env) = frame_key(vm, code) else { return Ok(()) };
    if line_changed(env, line) {
        fire_local(vm, env, "line", |_| Value::None)?;
    }
    Ok(())
}

/// Salto para trás (volta de laço) na função `code`: o `line` dispara de novo no alvo, mesmo na
/// mesma linha.
pub fn back_edge(vm: &Vm, code: &Rc<Code>) {
    if let Some(env) = frame_key(vm, code) {
        crate::frameobj::back_edge(env);
    }
}

/// Erro numa instrução da função `code` (capturado ou não, levantado nela ou vindo de uma
/// chamada): dispara `exception` com `(tipo, valor, traceback)` no quadro dela, se tiver `f_trace`.
pub fn exception(vm: &mut Vm, code: &Rc<Code>, e: &crate::vm::PyException) -> PyResult<()> {
    let Some(env) = frame_key(vm, code) else { return Ok(()) };
    fire_local(vm, env, "exception", |vm| {
        let exc = e.to_value();
        let kind = vm.load_attr(&exc, "__class__").unwrap_or(Value::None);
        let tb = vm.load_attr(&exc, "__traceback__").unwrap_or(Value::None);
        Value::tuple(vec![kind, exc, tb])
    })
}

/// Saída da função (o quadro ainda está em `vm.frames`): `return` na função de perfil e no
/// `f_trace` do quadro, com o valor devolvido (`None` se saiu por erro).
pub fn leave(vm: &mut Vm, result: &PyResult<Value>) -> PyResult<()> {
    if !hooked() || in_native(vm) {
        return Ok(());
    }
    let value = || result.as_ref().map_or(Value::None, Clone::clone);
    if profiling() {
        let frame = current_frame(vm)?;
        invoke_profile(vm, &frame, "return", value())?;
    }
    if active() {
        let env = vm.frames.borrow().last().map(|(_, _, env)| Rc::as_ptr(env) as usize);
        fire_local(vm, env, "return", |_| value())?;
    }
    Ok(())
}

/// Chamada de uma nativa `func` (função embutida ou método de tipo embutido): `c_call` antes,
/// `c_return` depois, ou `c_exception` se `run` falhar, todos com a nativa como argumento.
pub fn c_call(vm: &mut Vm, func: &Value, run: impl FnOnce(&mut Vm) -> PyResult<Value>) -> PyResult<Value> {
    let frame = current_frame(vm)?;
    invoke_profile(vm, &frame, "c_call", func.clone())?;
    let result = run(vm);
    if profiling() {
        invoke_profile(vm, &frame, if result.is_ok() { "c_return" } else { "c_exception" }, func.clone())?;
    }
    result
}

/// `sys.setprofile(f)` trocou um perfil ativo: a chamada já rodava instrumentada, então o novo perfil
/// recebe o `c_return` dela. Ligar o perfil do zero não gera nada (o CPython 3.13 só instrumenta o
/// quadro depois que a chamada começou).
pub fn profile_started(vm: &mut Vm, func: Value) -> PyResult<()> {
    if !profiling() {
        return Ok(());
    }
    let frame = current_frame(vm)?;
    invoke_profile(vm, &frame, "c_return", func)
}

/// Nome da nativa interna que troca os ganchos da thread que começa ou termina. No CPython cada
/// thread tem o próprio estado, então a troca nunca aparece como evento no perfil da outra.
pub const SWAP_HOOKS: &str = "_swap_hooks";

/// Troca o rastreador e a função de perfil de uma vez (`None` desliga) e devolve os de antes, sem
/// gerar evento algum.
pub fn swap_hooks(trace: Option<Value>, profile: Option<Value>) -> (Value, Value) {
    let old = (get(), get_profile());
    set(trace);
    set_profile(profile);
    old
}

/// `func` é uma nativa que gera eventos `c_*` (classes, como `int`, não geram).
pub fn is_c_function(func: &Value) -> bool {
    match func {
        Value::NativeFn(n) if n.name == SWAP_HOOKS => false,
        Value::NativeFn(_) | Value::Builtin(_) => {
            // Classes de verdade no CPython (chamá-las não gera `c_call`) que aqui têm nome de nativa.
            let builtin_class = match func {
                Value::Builtin(n) => *n,
                Value::NativeFn(f) => f.name,
                _ => "",
            };
            let builtin_class = matches!(builtin_class, "type" | "super" | "property" | "staticmethod" | "classmethod");
            !builtin_class && crate::builtins::class_name(func).is_none()
        }
        Value::Bound(_) => true,
        _ => false,
    }
}

/// `func` é a função (ou o método ligado) de um módulo embutido que no CPython seria C: o perfil
/// enxerga uma chamada de C, nunca o quadro Python que a implementa aqui.
fn is_native_python_function(func: &Value) -> bool {
    match func {
        Value::Function(f) => crate::vm::code_is_native(&f.code) && !stands_for_c_class(&f.code),
        Value::BoundFn(b) => crate::vm::code_is_native(&b.1.code),
        _ => false,
    }
}

/// A função de módulo faz aqui o papel de uma classe de C (`itertools.chain`, `itertools.cycle`...):
/// chamar um tipo no CPython não gera `c_call`. As funções de verdade de `itertools` são só `tee`.
fn stands_for_c_class(code: &Code) -> bool {
    if code.qual().contains('.') {
        return false;
    }
    code.is_generator || (code.filename.ends_with("/itertools.py") && code.name != "tee")
}

/// O quadro em execução é de código que no CPython seria C: nada dele gera evento, e as nativas que
/// ele chama também não (o que ele chama de volta no código do usuário gera, porque o quadro é outro).
fn in_native(vm: &Vm) -> bool {
    vm.frames.borrow().last().is_some_and(|(code, _, _)| crate::vm::code_is_native(code))
}

/// Gera as perguntas "a chamada de `func` gera evento no perfil?": o perfil está ligado, `$is_target` reconhece
/// `func` e quem chama é código do usuário (ou Python de verdade).
macro_rules! reports_call {
    ($(#[$meta:meta])* $name:ident, $is_target:ident) => {
        $(#[$meta])*
        pub fn $name(vm: &Vm, func: &Value) -> bool {
            profiling() && $is_target(func) && !in_native(vm)
        }
    };
}

reports_call! {
    /// A chamada de `func` por quem está em `vm` gera `c_call`: `func` é uma nativa.
    reports_c_call,
    is_c_function
}

reports_call! {
    /// Como `reports_c_call`, para a função Python que faz o papel de C. Só a instrução de chamada do
    /// programa a consulta: o C do CPython que chama outro C (`print` chamando o `write` de um
    /// `StringIO`) não gera evento.
    reports_native_python_call,
    is_native_python_function
}

