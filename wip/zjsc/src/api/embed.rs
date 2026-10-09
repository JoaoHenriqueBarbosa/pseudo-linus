//! Ponto de entrada do hospedeiro de páginas (o navegador headless do sandbox): um programa JavaScript de vida longa
//! sobre um global novo, com o console e o DOM do hospedeiro instalados, onde o hospedeiro avalia scripts em ordem,
//! dispara os eventos do documento e roda o laço de eventos.

use std::cell::RefCell;
use std::rc::Rc;

use crate::api::browser_modules::{evaluate_page_module, install_url_module_loader, ModuleFetch, UrlModuleHost};
use crate::api::eval::{describe_exception, evaluate, named_program_source, new_global_object, VmFinalizer};
use crate::host_function;
use crate::runtime::cell_registry::run_program;
use crate::runtime::console_host::ConsoleHost;
use crate::runtime::dom_bridge::{self, DomHost};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_promise::Status;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::object_to_primitive::call_function;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::native_class_support::install_global_function_with_visibility;
use crate::runtime::program_executable::ProgramExecutable;
use crate::runtime::vm::VM;

thread_local! {
    /// O global da página, para o `__pl_eval` (que roda no escopo global, com `let` e `const` de topo visíveis aos
    /// scripts seguintes, como um `<script>` clássico).
    static PAGE_GLOBAL: RefCell<Option<JSGlobalObjectRef>> = const { RefCell::new(None) };
}

/// `__pl_eval(fonte, url)`: avalia `fonte` como script clássico no global da página. A exceção do script segue
/// pendente para o chamador JavaScript, que a relata como erro de página.
fn pl_eval_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let source = rust_string(&call.argument(0).to_wtf_string());
    let url = rust_string(&call.argument(1).to_wtf_string());
    let Some(global) = PAGE_GLOBAL.with(|slot| slot.borrow().clone()) else { return Ok(JSValue::undefined()) };
    let executable = ProgramExecutable::create(&global, &named_program_source(&source, &url));
    let vm = global_object.vm();
    vm.interpreter().execute_program(&executable, &global);
    if vm.exception().is_some() {
        return Err(Thrown::Pending);
    }
    Ok(JSValue::undefined())
}
host_function!(pl_eval, pl_eval_body);

/// Um valor do programa escondido que o hospedeiro guarda para chamar depois ([`PageRunner::call_hidden`]).
pub struct HostHandle(JSValue);

/// Argumento de [`PageRunner::call_hidden`].
#[derive(Clone, Copy)]
pub enum HostArg<'a> {
    Str(&'a str),
    Num(f64),
    Null,
}

/// O programa em curso: avalia scripts no global da página.
pub struct PageRunner {
    vm: Rc<VM>,
    global_object: JSGlobalObjectRef,
    modules: Rc<UrlModuleHost>,
}

impl PageRunner {
    /// Avalia um `<script type=module>`: inline (`source`, com `url` `None`) ou externo (`url` absoluta). Esvazia as
    /// microtarefas; `Err` é a rejeição da avaliação já conhecida (`Nome: mensagem`). Um módulo que ainda espera
    /// (`await` de topo, `import` pela rede) segue no laço de eventos.
    pub fn eval_module(&self, source: Option<&str>, url: Option<&str>) -> Result<(), String> {
        let promise = evaluate_page_module(&self.global_object, &self.modules, source, url);
        self.vm.drain_microtasks();
        match promise.status() {
            Status::Rejected => Err(describe_exception(&promise.result())),
            _ => Ok(()),
        }
    }

    /// Avalia `source` como script clássico de nome `url` e esvazia as microtarefas. `Err` é a exceção não capturada
    /// (`Nome: mensagem`).
    pub fn eval(&self, source: &str, url: &str) -> Result<(), String> {
        let result = evaluate(&self.global_object, &named_program_source(source, url));
        self.vm.drain_microtasks();
        result.map(|_| ()).map_err(|exception| describe_exception(&exception))
    }

    /// Como [`PageRunner::eval`], para o código do hospedeiro (o DOM em JavaScript e as chamadas ao gancho dele):
    /// as funções criadas são builtins para a página, sem nome de arquivo nem frame no `Error.stack`.
    pub fn eval_hidden(&self, source: &str) -> Result<(), String> {
        self.eval_hidden_value(source).map(|_| ())
    }

    /// Como [`PageRunner::eval_hidden`], guardando o valor de completude do programa (a função gancho do DOM, que
    /// assim não precisa de binding global que a página enxergue). Sem coletor no porte, o valor vive até o fim.
    pub fn eval_hidden_value(&self, source: &str) -> Result<HostHandle, String> {
        let result = crate::api::eval::evaluate_hidden_program(&self.global_object, source);
        self.vm.drain_microtasks();
        result.map(HostHandle).map_err(|exception| describe_exception(&exception))
    }

    /// Chama a função de `handle` com `args` a partir do hospedeiro (sem frame da página) e esvazia as microtarefas.
    pub fn call_hidden(&self, handle: &HostHandle, args: &[HostArg]) -> Result<(), String> {
        let args: Vec<JSValue> = args
            .iter()
            .map(|arg| match arg {
                HostArg::Str(text) => JSValue::from_js_string(js_string(&self.vm, &WtfString::from_utf8(text.as_bytes()))),
                HostArg::Num(number) => js_number(*number),
                HostArg::Null => JSValue::null(),
            })
            .collect();
        let result = call_function(&self.global_object, handle.0, JSValue::undefined(), &args).ok_or_else(|| {
            let exception = self.vm.exception().map(|exception| describe_exception(&exception.value())).unwrap_or_default();
            self.vm.clear_exception();
            exception
        });
        self.vm.drain_microtasks();
        result.map(|_| ())
    }

    /// Roda o laço de eventos (timers, immediates, microtarefas) no relógio virtual do motor até esvaziar ou até
    /// passarem `budget_ms` de tempo virtual (o `--virtual-time-budget` do Chromium). Pode ser chamado de novo: o
    /// relógio virtual continua de onde parou.
    pub fn run_event_loop_for(&self, budget_ms: f64) {
        crate::runtime::timers::run_event_loop_for(&self.global_object, budget_ms);
    }

    /// A latência de rede do `import()` dinâmico de módulo ainda não carregado.
    pub fn set_network_latency_ms(&self, milliseconds: f64) {
        self.modules.set_network_latency_ms(milliseconds);
    }

    /// Entrega ao hospedeiro as rejeições sem tratador: `handle` recebe `("unhandled", promise, reason)` ao fim de
    /// cada checkpoint e `("handled", promise)` quando o tratador chega depois. Tira o tratamento padrão do bun.
    pub fn set_rejection_hook(&self, handle: &HostHandle) {
        let function = handle.0;
        let notify = move |global_object: &JSGlobalObject, args: &[JSValue]| {
            let vm = global_object.vm();
            if call_function(global_object, function, JSValue::undefined(), args).is_none() {
                vm.clear_exception();
            }
        };
        let notify = Rc::new(notify);
        let unhandled = notify.clone();
        self.vm.set_unhandled_rejection_reporter(Some(Rc::new(move |global_object: &JSGlobalObject, promise, reason| {
            let kind = JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(b"unhandled")));
            unhandled(global_object, &[kind, promise, reason]);
        })));
        self.vm.set_rejection_handled_reporter(Some(Rc::new(move |global_object: &JSGlobalObject, promise| {
            let kind = JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(b"handled")));
            notify(global_object, &[kind, promise]);
        })));
    }

    /// Só as microtarefas pendentes.
    pub fn drain_microtasks(&self) {
        self.vm.drain_microtasks();
    }
}

/// Cria o programa da página e entrega o [`PageRunner`] a `body`. `document_url` e `fetch` alimentam os módulos ES
/// (`<script type=module>` e `import()`): o texto de cada URL absoluta, ou `None` para erro de rede.
pub fn run_page<T>(console: Rc<dyn ConsoleHost>, dom: Rc<dyn DomHost>, document_url: &str, fetch: ModuleFetch, body: impl FnOnce(&PageRunner) -> T) -> T {
    run_program(|| {
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        global_object.set_console_host(console);
        dom_bridge::install(&global_object, dom);
        install_global_function_with_visibility(&global_object, "__pl_eval", 2, pl_eval, ImplementationVisibility::Private);
        let modules = Rc::new(UrlModuleHost::new(document_url, fetch));
        install_url_module_loader(&global_object, modules.clone());
        PAGE_GLOBAL.with(|slot| *slot.borrow_mut() = Some(global_object.clone()));
        crate::runtime::timers::enable_event_loop();
        let runner = PageRunner { vm, global_object, modules };
        let result = body(&runner);
        PAGE_GLOBAL.with(|slot| slot.borrow_mut().take());
        result
    })
}
