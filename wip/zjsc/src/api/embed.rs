//! Ponto de entrada do hospedeiro de páginas (o navegador headless do sandbox): um programa JavaScript de vida longa
//! sobre um global novo, com o console e o DOM do hospedeiro instalados, onde o hospedeiro avalia scripts em ordem,
//! dispara os eventos do documento e roda o laço de eventos.

use std::rc::Rc;

use crate::api::eval::{describe_exception, evaluate, named_program_source, new_global_object, VmFinalizer};
use crate::runtime::cell_registry::run_program;
use crate::runtime::console_host::ConsoleHost;
use crate::runtime::dom_bridge::{self, DomHost};
use crate::runtime::js_global_object::JSGlobalObjectRef;
use crate::runtime::vm::VM;

/// O programa em curso: avalia scripts no global da página.
pub struct PageRunner {
    vm: Rc<VM>,
    global_object: JSGlobalObjectRef,
}

impl PageRunner {
    /// Avalia `source` como script clássico de nome `url` e esvazia as microtarefas. `Err` é a exceção não capturada
    /// (`Nome: mensagem`).
    pub fn eval(&self, source: &str, url: &str) -> Result<(), String> {
        let result = evaluate(&self.global_object, &named_program_source(source, url));
        self.vm.drain_microtasks();
        result.map(|_| ()).map_err(|exception| describe_exception(&exception))
    }

    /// Roda o laço de eventos (timers, immediates, microtarefas) até esvaziar, no relógio virtual do motor.
    pub fn run_event_loop(&self) {
        crate::runtime::timers::run_event_loop(&self.global_object);
    }

    /// Só as microtarefas pendentes.
    pub fn drain_microtasks(&self) {
        self.vm.drain_microtasks();
    }
}

/// Cria o programa da página e entrega o [`PageRunner`] a `body`.
pub fn run_page<T>(console: Rc<dyn ConsoleHost>, dom: Rc<dyn DomHost>, body: impl FnOnce(&PageRunner) -> T) -> T {
    run_program(|| {
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        global_object.set_console_host(console);
        dom_bridge::install(&global_object, dom);
        crate::runtime::timers::enable_event_loop();
        let runner = PageRunner { vm, global_object };
        body(&runner)
    })
}
