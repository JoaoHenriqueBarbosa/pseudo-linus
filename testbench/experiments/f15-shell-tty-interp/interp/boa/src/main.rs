//! boa_engine 0.22 (JS em Rust puro) sobre o protocolo comum do F17.
//!
//! O `Context` do boa só tem os intrínsecos do ECMAScript: nada de console, fetch, require ou
//! módulos de host (isso fica no `boa_runtime`, que não usamos). As primitivas nativas que o
//! prelúdio JS comum precisa (`__f15_out`, `__f15_err`, `__f15_read`, `__f15_write`) são ponteiros de
//! função sobre um estado `thread_local`: o boa só aceita closure com captura via
//! `NativeFunction::from_closure`, que é `unsafe`, então o estado do trecho mora fora da closure.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use boa_engine::context::ContextBuilder;
use boa_engine::module::IdleModuleLoader;
use boa_engine::property::Attribute;
use boa_engine::{Context, JsArgs, JsResult, JsString, JsValue, NativeFunction, Source, js_string};
use f15_interp_common::{Engine, JS_PRELUDE, Snippet, SnippetResult, serve};

#[derive(Default)]
struct State {
    out: String,
    files: BTreeMap<String, String>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn arg_string(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<String> {
    Ok(args.get_or_undefined(i).to_string(ctx)?.to_std_string_lossy())
}

fn f15_out(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let s = arg_string(args, 0, ctx)?;
    STATE.with(|st| st.borrow_mut().out.push_str(&s));
    Ok(JsValue::undefined())
}

fn f15_err(_this: &JsValue, _args: &[JsValue], _ctx: &mut Context) -> JsResult<JsValue> {
    // stderr do trecho: descartado (não é comparado), e nunca vai pro host.
    Ok(JsValue::undefined())
}

fn f15_read(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let path = arg_string(args, 0, ctx)?;
    let v = STATE.with(|st| st.borrow().files.get(&path).cloned());
    Ok(match v {
        Some(s) => JsValue::from(JsString::from(s.as_str())),
        None => JsValue::undefined(),
    })
}

fn f15_write(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let path = arg_string(args, 0, ctx)?;
    let data = arg_string(args, 1, ctx)?;
    let append = args.get_or_undefined(2).to_boolean();
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        if append {
            st.files.entry(path).or_default().push_str(&data);
        } else {
            st.files.insert(path, data);
        }
    });
    Ok(JsValue::undefined())
}

fn setup(ctx: &mut Context) -> JsResult<()> {
    let prims: [(JsString, NativeFunction); 4] = [
        (js_string!("__f15_out"), NativeFunction::from_fn_ptr(f15_out)),
        (js_string!("__f15_err"), NativeFunction::from_fn_ptr(f15_err)),
        (js_string!("__f15_read"), NativeFunction::from_fn_ptr(f15_read)),
        (js_string!("__f15_write"), NativeFunction::from_fn_ptr(f15_write)),
    ];
    for (name, f) in prims {
        ctx.register_global_callable(name, 1, f)?;
    }
    ctx.eval(Source::from_bytes(JS_PRELUDE))?;
    // `globalThis.global` como no node, pra trechos que testam o ambiente.
    let global = ctx.global_object();
    ctx.register_global_property(js_string!("global"), global, Attribute::all())?;
    Ok(())
}

struct BoaEngine {
    /// Variante ingênua (`--default-context`): `Context::default()`, sem trocar o loader de módulos.
    default_context: bool,
}

impl Engine for BoaEngine {
    fn name(&self) -> &'static str {
        "boa_engine"
    }
    fn version(&self) -> &'static str {
        "0.22.0"
    }
    fn language(&self) -> &'static str {
        "js"
    }

    fn run(&self, snippet: &Snippet) -> SnippetResult {
        STATE.with(|st| {
            *st.borrow_mut() = State { out: String::new(), files: snippet.files.clone() };
        });
        // O `Context::default()` usa `SimpleModuleLoader::new(".")` (boa_engine src/context/mod.rs,
        // linha 1217): canonicaliza o cwd do host ao criar o contexto e lê arquivo do host com
        // `std::fs` em todo `import`. O `IdleModuleLoader` recusa qualquer import. A variante
        // `--default-context` existe só pra medir essa diferença no strace.
        let built = if self.default_context {
            Ok(Context::default())
        } else {
            ContextBuilder::new().module_loader(Rc::new(IdleModuleLoader)).build()
        };
        let mut ctx = match built {
            Ok(c) => c,
            Err(e) => {
                return SnippetResult {
                    id: snippet.id.clone(),
                    error: Some(format!("contexto: {e}")),
                    files: snippet.files.clone(),
                    ..SnippetResult::default()
                };
            }
        };
        let mut error = None;
        if let Err(e) = setup(&mut ctx) {
            error = Some(format!("prelúdio: {e}"));
        } else {
            match ctx.eval(Source::from_bytes(snippet.code.as_bytes())) {
                Ok(_) => {
                    if let Err(e) = ctx.run_jobs() {
                        error = Some(format!("Uncaught {e}"));
                    }
                }
                Err(e) => error = Some(format!("Uncaught {e}")),
            }
        }
        let st = STATE.with(|st| std::mem::take(&mut *st.borrow_mut()));
        SnippetResult { id: snippet.id.clone(), stdout: st.out, error, files: st.files, elapsed_us: 0 }
    }

    fn notes(&self) -> Vec<String> {
        vec![
            "ContextBuilder com IdleModuleLoader, sem boa_runtime: só intrínsecos ECMAScript; console e require('fs') são o prelúdio comum sobre primitivas nossas.".into(),
            "O Context::default() do boa NÃO é isolado: o loader padrão (SimpleModuleLoader sobre \".\") faz realpath do cwd do host e lê arquivo do host em import().".into(),
            "Closures com captura exigem NativeFunction::from_closure (unsafe); usamos ponteiros de função sobre estado thread_local.".into(),
        ]
    }
}

fn main() {
    let default_context = std::env::args().any(|a| a == "--default-context");
    serve(&BoaEngine { default_context });
}
