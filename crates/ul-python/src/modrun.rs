//! O corpo de um módulo importado e o código de `exec`/`eval` como quadro do laço de instruções (fatia G3 de
//! `wip/notes/python-fork.md`).
//!
//! Antes, cada um deles rodava num `run_loop` aninhado (`rust_nest > 1`), e `os.fork` ou uma troca de thread
//! dentro dele não alcançavam o estado, que morava na pilha Rust. Agora o quadro do corpo é um [`Callee`] como o
//! de uma função, com um [`Dunder`] no `CallLink::then` que diz o que fazer quando ele fecha:
//!
//! - [`Dunder::Import`]: o módulo já estava em `sys.modules` (e em `Vm::module_globals`) desde o início, para a
//!   importação circular enxergar o módulo parcial; ao fechar com sucesso ele é ligado ao pacote pai e a cadeia do
//!   `import a.b.c` segue com o módulo seguinte; ao fechar com exceção ele sai de `sys.modules` e a exceção sobe.
//! - [`Dunder::Exec`]: o quadro do `exec`/`eval` (`Vm::enter_nested`) e o que falta fazer com as globais e o
//!   `locals` quando ele acaba (`builtins_ext::finish_exec`).
//!
//! O estado em andamento (a cadeia de módulos que falta, o que `exec` devolve às globais) é dado: a imagem do heap
//! o copia no `os.fork` (`heapimage.rs`, `DunderNode::Import` e `DunderNode::Exec`).
//!
//! O caminho recursivo (`Vm::run_module_callee`) roda o mesmo quadro num `run_loop` aninhado, para as nativas que
//! ainda importam ou executam código por recursão Rust (`importlib.reload` não passa por aqui).

use std::cell::RefCell;
use std::rc::Rc;

use crate::compile::{Code, Op};
use crate::modules::Initializing;
use crate::object::{Env, ModuleObj, Value, VarMap};
use crate::vm::{internal, CallLink, Callee, Dunder, Entered, Frame, Next, PyException, PyResult, Vm};

/// O corpo de um módulo pronto para rodar: o módulo já registrado em `sys.modules`, as globais vivas dele e o
/// código compilado.
pub(crate) struct Body {
    pub(crate) name: String,
    pub(crate) module: Rc<ModuleObj>,
    pub(crate) code: Rc<Code>,
    pub(crate) globals: Rc<RefCell<VarMap>>,
}

/// O que importar um nome devolve para a etapa seguinte: o módulo pronto, ou o corpo dele para rodar.
pub(crate) enum Load {
    Ready(Rc<ModuleObj>),
    Body(Body),
}

/// A cadeia de um `import a.b.c`: os módulos que ainda faltam importar (o próximo por último) e o nome cujo
/// módulo o `import` entrega no fim.
pub(crate) struct ImportPlan {
    pub(crate) rest: Vec<String>,
    pub(crate) result: String,
}

impl ImportPlan {
    /// Importa `name` e os pais dele, de `a` até `a.b.c`, e entrega o módulo `result`.
    pub(crate) fn new(name: &str, result: &str) -> ImportPlan {
        let mut rest: Vec<String> = name.match_indices('.').map(|(at, _)| name[..at].to_string()).collect();
        rest.push(name.to_string());
        rest.reverse();
        ImportPlan { rest, result: result.to_string() }
    }
}

/// O corpo de um módulo em execução: o módulo, a cadeia que segue depois dele (`None` fora do laço, quando o
/// chamador só quer o módulo) e a marca de "em importação" que vive enquanto o corpo roda.
pub(crate) struct ImportRun {
    pub(crate) name: String,
    pub(crate) module: Rc<ModuleObj>,
    pub(crate) plan: Option<ImportPlan>,
    pub(crate) guard: Initializing,
}

impl Vm {
    fn body_link(&self, caller_globals: Option<Rc<RefCell<VarMap>>>, then: Dunder) -> CallLink {
        CallLink {
            func: None,
            caller_line: self.cur_line.get(),
            handled_len: self.handled.borrow().len(),
            profiled: false,
            caller_globals,
            instance: None,
            on_stop: None,
            then: Some(then),
            resuming: None,
        }
    }

    /// A instrução `import` ou `from . import` do quadro: o módulo pronto, ou o quadro do corpo dele.
    pub(crate) fn import_entered(&mut self, code: &Rc<Code>, op: Op) -> PyResult<Entered> {
        let name = match op {
            Op::Import(i) => code.names[i as usize].to_string(),
            Op::ImportRel { name, level } => {
                crate::modules::resolve_relative(self, &code.names[name as usize], level as usize)?
            }
            _ => return Err(internal("not an import instruction")),
        };
        crate::modules::begin_import_visible(self, &name, code.internal)
    }

    /// O quadro do corpo de `body`: as globais do módulo passam a ser as da `Vm` até o fechamento.
    pub(crate) fn open_body(&mut self, body: Body, plan: Option<ImportPlan>) -> Callee {
        let Body { name, module, code, globals } = body;
        let run = ImportRun { name, guard: Initializing::enter(module.name), module, plan };
        let caller = std::mem::replace(&mut self.globals, globals);
        Callee { frame: Frame::new(code, Env::new(None, false, true)), link: self.body_link(Some(caller), Dunder::Import(Box::new(run))) }
    }

    /// O módulo seguinte da cadeia do `import`: o corpo dele vira quadro; sem mais corpo a rodar, a cadeia acabou
    /// e o valor do `import` é o módulo de `result`.
    pub(crate) fn continue_import(&mut self, mut plan: ImportPlan) -> PyResult<Next> {
        while let Some(name) = plan.rest.pop() {
            if let Some(body) = crate::modules::import_step(self, &name)? {
                return Ok(Next::Spawn(self.open_body(body, Some(plan))));
            }
        }
        crate::modules::import_value(self, &plan.result).map(Next::Value)
    }

    /// O corpo de um módulo acabou com `outcome`.
    fn close_import(&mut self, run: ImportRun, link: CallLink, outcome: PyResult<Value>) -> PyResult<Next> {
        self.handled.borrow_mut().truncate(link.handled_len);
        self.cur_line.set(link.caller_line);
        if let Some(globals) = link.caller_globals {
            self.globals = globals;
        }
        let ImportRun { name, module, plan, guard } = run;
        drop(guard);
        if let Err(e) = outcome {
            // Como no CPython, o módulo que falhou ao rodar sai de `sys.modules` e a exceção sobe para quem importou.
            self.modules.borrow_mut().remove(&name);
            self.module_globals.borrow_mut().remove(module.name);
            return Err(e);
        }
        crate::modules::userimport::bind_to_parent(self, &name, &module);
        match plan {
            Some(plan) => self.continue_import(plan),
            None => Ok(Next::Value(Value::Module(module))),
        }
    }

    /// O quadro do código de `exec`/`eval` (o `run_nested` de antes): entra em `frames` (para `sys._getframe`, `f_back`
    /// e os eventos do `sys.settrace`) e roda nas `globals` dadas (ou nas atuais). Se o rastreador recusar a entrada,
    /// devolve o erro com o `then` intacto, para o chamador concluir o `exec` como se o código tivesse falhado.
    pub(crate) fn enter_nested(
        &mut self,
        code: &Rc<Code>,
        globals: Option<Rc<RefCell<VarMap>>>,
        then: Dunder,
    ) -> Result<Callee, (PyException, Dunder)> {
        let env = Env::new(None, false, true);
        let caller = globals.map(|g| std::mem::replace(&mut self.globals, g));
        let caller_line = self.cur_line.get();
        self.frames.borrow_mut().push((code.clone(), caller_line, env.clone()));
        crate::frameobj::bind_globals(&env, &self.globals);
        if let Err(e) = crate::tracing::enter(self, code) {
            crate::frameobj::unbind_globals(&env);
            self.frames.borrow_mut().pop();
            self.cur_line.set(caller_line);
            if let Some(g) = caller {
                self.globals = g;
            }
            return Err((e, then));
        }
        Ok(Callee { frame: Frame::new(code.clone(), env), link: self.body_link(caller, then) })
    }

    /// O código de `exec`/`eval` acabou com `outcome`.
    fn close_exec(
        &mut self,
        run: crate::builtins_ext::ExecRun,
        link: CallLink,
        env: &Rc<Env>,
        mut outcome: PyResult<Value>,
    ) -> PyResult<Next> {
        if let Err(e) = crate::tracing::leave(self, &outcome) {
            outcome = Err(e);
        }
        crate::frameobj::unbind_globals(env);
        self.frames.borrow_mut().pop();
        self.cur_line.set(link.caller_line);
        if let Some(globals) = link.caller_globals {
            self.globals = globals;
        }
        crate::builtins_ext::finish_exec(self, run, outcome.map(|_| ())).map(Next::Value)
    }

    /// O quadro de corpo `body` acabou: conclui o módulo (e abre o seguinte da cadeia) ou entrega o que o `exec`
    /// devolve.
    pub(crate) fn close_body(&mut self, body: Dunder, link: CallLink, env: &Rc<Env>, outcome: PyResult<Value>) -> PyResult<Next> {
        match body {
            Dunder::Import(run) => self.close_import(*run, link, outcome),
            Dunder::Exec(run) => self.close_exec(*run, link, env, outcome),
            _ => Err(internal("a module body closed as a method")),
        }
    }

    /// Roda até o fim o quadro de um corpo, num `run_loop` aninhado: o caminho das nativas que ainda recursam
    /// (`__import__` chamado de Rust, `import_checked`, `exec` chamado por uma nativa). Segue a cadeia do `import`
    /// até o valor final.
    pub(crate) fn run_module_callee(&mut self, mut callee: Callee) -> PyResult<Value> {
        loop {
            let env = callee.frame.env.clone();
            let result = self.run_frame(&mut callee.frame);
            let Some(body) = callee.link.then.take() else { return Err(internal("a module body without its closing")) };
            match self.close_body(body, callee.link, &env, result)? {
                Next::Value(v) => return Ok(v),
                Next::Spawn(next) => callee = next,
                _ => return Ok(Value::None),
            }
        }
    }

    /// Termina `entered` (o resultado de abrir um `import` ou um `exec`) no `run_loop` aninhado.
    pub(crate) fn run_entered(&mut self, entered: Entered) -> PyResult<Value> {
        match entered {
            Entered::Done(v) => Ok(v),
            Entered::Frame(callee) => self.run_module_callee(callee),
        }
    }
}
