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
use crate::runtime::js_value::JSValue;
use crate::runtime::native_class_support::install_global_function;
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

    /// Roda o laço de eventos (timers, immediates, microtarefas) no relógio virtual do motor até esvaziar ou até
    /// passarem `budget_ms` de tempo virtual (o `--virtual-time-budget` do Chromium); o que sobrar de timers fica sem
    /// disparar. Só deve ser chamado uma vez por página: o laço parado pelo orçamento não é retomável.
    pub fn run_event_loop_for(&self, budget_ms: f64) {
        crate::runtime::timers::schedule_native(&self.global_object, budget_ms, Rc::new(|_| crate::runtime::timers::halt_event_loop()));
        crate::runtime::timers::run_event_loop_until_held(&self.global_object);
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
        install_global_function(&global_object, "__pl_eval", 2, pl_eval);
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
