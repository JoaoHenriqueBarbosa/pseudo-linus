//! Geradores: uma função com `yield` vira um objeto que guarda o quadro (pilha, blocos protegidos
//! e `pc`) entre um `next` e o seguinte.

use std::cell::RefCell;
use std::rc::Rc;

use crate::compile::Code;
use crate::object::{Env, ExtObject, Kw, Value};
use crate::vm::{exc, type_error, Block, Exit, PyResult, Slot, Vm};

struct GenState {
    stack: Vec<Slot>,
    blocks: Vec<Block>,
    pc: usize,
    started: bool,
    done: bool,
    running: bool,
}

pub struct GenObj {
    vm: Vm,
    code: Rc<Code>,
    env: Rc<Env>,
    state: RefCell<GenState>,
}

/// Cria o gerador de uma chamada de função com `yield`; o corpo só roda no primeiro `next`.
pub fn new_generator(vm: Vm, code: Rc<Code>, env: Rc<Env>) -> Value {
    Value::Ext(Rc::new(GenObj {
        vm,
        code,
        env,
        state: RefCell::new(GenState {
            stack: Vec::new(),
            blocks: Vec::new(),
            pc: 0,
            started: false,
            done: false,
            running: false,
        }),
    }))
}

impl GenObj {
    /// Retoma o gerador; `Ok(None)` quando ele termina (o `StopIteration`).
    fn resume(&self, sent: Option<Value>) -> PyResult<Option<Value>> {
        let (mut stack, mut blocks, mut pc) = {
            let mut st = self.state.borrow_mut();
            if st.running {
                return Err(exc("ValueError", "generator already executing"));
            }
            if st.done {
                return Ok(None);
            }
            if st.started {
                st.stack.push(Slot::Val(sent.unwrap_or(Value::None)));
            } else if sent.is_some_and(|v| !matches!(v, Value::None)) {
                return Err(type_error("can't send non-None value to a just-started generator"));
            }
            st.started = true;
            st.running = true;
            (std::mem::take(&mut st.stack), std::mem::take(&mut st.blocks), st.pc)
        };
        let mut vm = self.vm.clone();
        let result = vm.run_loop(&self.code, &self.env, &mut stack, &mut blocks, &mut pc);
        let mut st = self.state.borrow_mut();
        st.running = false;
        match result {
            Ok(Exit::Yield(v)) => {
                st.stack = stack;
                st.blocks = blocks;
                st.pc = pc;
                Ok(Some(v))
            }
            Ok(Exit::Return(_)) => {
                st.done = true;
                Ok(None)
            }
            Err(e) => {
                st.done = true;
                Err(e)
            }
        }
    }
}

impl ExtObject for GenObj {
    fn type_name(&self) -> &'static str {
        "generator"
    }
    fn repr(&self) -> String {
        format!("<generator object {} at {:#x}>", self.code.name, self as *const GenObj as usize)
    }
    fn methods(&self) -> &'static [&'static str] {
        &["send", "close", "__next__"]
    }
    fn is_iterable(&self) -> bool {
        true
    }
    fn iter_next(&self) -> PyResult<Option<Value>> {
        self.resume(None)
    }
    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "send" => {
                let [v] = <[Value; 1]>::try_from(args)
                    .map_err(|a| type_error(format!("send() takes exactly one argument ({} given)", a.len())))?;
                self.resume(Some(v))?.ok_or_else(|| exc("StopIteration", ""))
            }
            "__next__" => self.resume(None)?.ok_or_else(|| exc("StopIteration", "")),
            "close" => {
                let mut st = self.state.borrow_mut();
                st.done = true;
                st.stack.clear();
                st.blocks.clear();
                Ok(Value::None)
            }
            _ => Err(exc("AttributeError", format!("'generator' object has no attribute '{name}'"))),
        }
    }
}
