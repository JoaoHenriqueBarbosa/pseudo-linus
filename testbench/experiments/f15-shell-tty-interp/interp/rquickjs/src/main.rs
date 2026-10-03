//! rquickjs 0.14 (QuickJS em C) sobre o protocolo comum do F17.
//!
//! Sem as features `loader`/`full`: nenhum carregador de módulo que leia o FS do host. O
//! `quickjs-libc` (módulos `std` e `os` do qjs, que fazem I/O de host) não é compilado pelo
//! `rquickjs-sys`; o `Context::full` registra só os intrínsecos do ECMAScript. As primitivas do
//! prelúdio JS comum são closures Rust com o estado do trecho em `Rc<RefCell<..>>`, o que a API do
//! rquickjs permite sem unsafe.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use f15_interp_common::{Engine, JS_PRELUDE, Snippet, SnippetResult, serve};
use rquickjs::function::Opt;
use rquickjs::{CatchResultExt, Context, Ctx, Function, Runtime};

#[derive(Default)]
struct State {
    out: String,
    files: BTreeMap<String, String>,
}

fn setup<'js>(ctx: &Ctx<'js>, state: &Rc<RefCell<State>>) -> rquickjs::Result<()> {
    let g = ctx.globals();
    let st = state.clone();
    g.set(
        "__f15_out",
        Function::new(ctx.clone(), move |s: String| {
            st.borrow_mut().out.push_str(&s);
        })?,
    )?;
    g.set("__f15_err", Function::new(ctx.clone(), |_s: String| {})?)?;
    let st = state.clone();
    g.set(
        "__f15_read",
        Function::new(ctx.clone(), move |p: String| -> Option<String> { st.borrow().files.get(&p).cloned() })?,
    )?;
    let st = state.clone();
    g.set(
        "__f15_write",
        Function::new(ctx.clone(), move |p: String, data: String, append: Opt<bool>| {
            let mut st = st.borrow_mut();
            if append.0.unwrap_or(false) {
                st.files.entry(p).or_default().push_str(&data);
            } else {
                st.files.insert(p, data);
            }
        })?,
    )?;
    ctx.eval::<(), _>(JS_PRELUDE)?;
    ctx.eval::<(), _>("globalThis.global = globalThis;")?;
    Ok(())
}

struct QuickJsEngine;

impl Engine for QuickJsEngine {
    fn name(&self) -> &'static str {
        "rquickjs"
    }
    fn version(&self) -> &'static str {
        "0.14.0"
    }
    fn language(&self) -> &'static str {
        "js"
    }

    fn run(&self, snippet: &Snippet) -> SnippetResult {
        let state = Rc::new(RefCell::new(State { out: String::new(), files: snippet.files.clone() }));
        let mut error = None;
        match Runtime::new().and_then(|rt| Context::full(&rt).map(|c| (rt, c))) {
            Err(e) => error = Some(format!("runtime: {e}")),
            Ok((rt, context)) => {
                context.with(|ctx| {
                    if let Err(e) = setup(&ctx, &state).catch(&ctx) {
                        error = Some(format!("prelúdio: {e}"));
                        return;
                    }
                    if let Err(e) = ctx.eval::<(), _>(snippet.code.as_bytes().to_vec()).catch(&ctx) {
                        error = Some(format!("Uncaught {e}"));
                    }
                });
                // Promessas pendentes (código async).
                while rt.is_job_pending() {
                    if let Err(e) = rt.execute_pending_job() {
                        error = Some(format!("job: {e:?}"));
                        break;
                    }
                }
            }
        }
        let st = std::mem::take(&mut *state.borrow_mut());
        SnippetResult { id: snippet.id.clone(), stdout: st.out, error, files: st.files, elapsed_us: 0 }
    }

    fn notes(&self) -> Vec<String> {
        vec![
            "QuickJS em C (rquickjs-sys compila o quickjs.c); sem loader nem quickjs-libc: std/os do qjs não existem.".into(),
            "console e require('fs') são o prelúdio comum; primitivas são closures Rust com Rc<RefCell>, sem unsafe.".into(),
        ]
    }
}

fn main() {
    serve(&QuickJsEngine);
}
