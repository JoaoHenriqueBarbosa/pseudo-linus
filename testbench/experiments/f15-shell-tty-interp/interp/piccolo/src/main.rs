//! piccolo 0.3.3 (VM Lua em Rust puro, stackless) sobre o protocolo comum do F17.
//!
//! `Lua::core()` carrega base, coroutine, math, string e table e nenhuma I/O: o `print` do piccolo
//! mora em `load_io` (que escreve direto em `std::io::stdout()`) e não é carregado. O `print`, o `io`
//! e o `json` que o trecho vê são nossos: o prelúdio Lua comum sobre primitivas Rust, e `json` com
//! conversão serde_json <-> valores do piccolo feita aqui.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use f15_interp_common::{Engine, LUA_PRELUDE, Snippet, SnippetResult, serve};
use piccolo::{Callback, CallbackReturn, Closure, Context, Executor, Lua, Table, Value};

#[derive(Default)]
struct State {
    out: String,
    files: BTreeMap<String, String>,
}

fn arg_string<'gc>(v: Value<'gc>) -> String {
    match v {
        Value::String(s) => s.to_str_lossy().into_owned(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        other => format!("{other}"),
    }
}

fn str_value<'gc>(ctx: Context<'gc>, s: &str) -> Value<'gc> {
    Value::String(piccolo::String::from_slice(&ctx, s.as_bytes()))
}

fn json_to_lua<'gc>(ctx: Context<'gc>, v: &serde_json::Value) -> Result<Value<'gc>, piccolo::Error<'gc>> {
    Ok(match v {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(b) => Value::Boolean(*b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::Integer(i),
            None => Value::Number(n.as_f64().unwrap_or(f64::NAN)),
        },
        serde_json::Value::String(s) => str_value(ctx, s),
        serde_json::Value::Array(items) => {
            let t = Table::new(&ctx);
            for (i, item) in items.iter().enumerate() {
                let value = json_to_lua(ctx, item)?;
                t.set(ctx, (i + 1) as i64, value)?;
            }
            Value::Table(t)
        }
        serde_json::Value::Object(map) => {
            let t = Table::new(&ctx);
            for (k, item) in map {
                let value = json_to_lua(ctx, item)?;
                t.set(ctx, str_value(ctx, k), value)?;
            }
            Value::Table(t)
        }
    })
}

fn lua_to_json(v: Value<'_>, depth: usize) -> Result<serde_json::Value, String> {
    if depth > 128 {
        return Err("json.encode: tabela recursiva ou funda demais".into());
    }
    Ok(match v {
        Value::Nil => serde_json::Value::Null,
        Value::Boolean(b) => serde_json::Value::Bool(b),
        Value::Integer(i) => serde_json::Value::from(i),
        Value::Number(n) => serde_json::Number::from_f64(n)
            .map(serde_json::Value::Number)
            .ok_or_else(|| "json.encode: número não finito".to_string())?,
        Value::String(s) => serde_json::Value::String(s.to_str_lossy().into_owned()),
        Value::Table(t) => {
            let pairs: Vec<(Value<'_>, Value<'_>)> = t.iter().collect();
            let n = pairs.len() as i64;
            let is_array = n > 0
                && pairs.iter().all(|(k, _)| matches!(k, Value::Integer(i) if *i >= 1 && *i <= n));
            if is_array {
                let mut items: Vec<(i64, Value<'_>)> = pairs
                    .into_iter()
                    .map(|(k, v)| (if let Value::Integer(i) = k { i } else { 0 }, v))
                    .collect();
                items.sort_by_key(|(i, _)| *i);
                let mut out = Vec::with_capacity(items.len());
                for (_, v) in items {
                    out.push(lua_to_json(v, depth + 1)?);
                }
                serde_json::Value::Array(out)
            } else {
                let mut map = serde_json::Map::new();
                for (k, v) in pairs {
                    let key = match k {
                        Value::String(s) => s.to_str_lossy().into_owned(),
                        Value::Integer(i) => i.to_string(),
                        Value::Number(f) => f.to_string(),
                        other => return Err(format!("json.encode: chave não suportada ({})", other.type_name())),
                    };
                    map.insert(key, lua_to_json(v, depth + 1)?);
                }
                serde_json::Value::Object(map)
            }
        }
        other => return Err(format!("json.encode: tipo não suportado ({})", other.type_name())),
    })
}

fn setup(lua: &mut Lua, state: &Rc<RefCell<State>>) -> Result<(), String> {
    let st_out = state.clone();
    let st_read = state.clone();
    let st_write = state.clone();
    lua.try_enter(|ctx| {
        let out = Callback::from_fn(&ctx, move |_ctx, _, mut stack| {
            let s = arg_string(stack.get(0));
            st_out.borrow_mut().out.push_str(&s);
            stack.clear();
            Ok(CallbackReturn::Return)
        });
        let read = Callback::from_fn(&ctx, move |ctx, _, mut stack| {
            let path = arg_string(stack.get(0));
            let v = st_read.borrow().files.get(&path).cloned();
            stack.clear();
            if let Some(s) = v {
                stack.push_back(str_value(ctx, &s));
            } else {
                stack.push_back(Value::Nil);
            }
            Ok(CallbackReturn::Return)
        });
        let write = Callback::from_fn(&ctx, move |_ctx, _, mut stack| {
            let path = arg_string(stack.get(0));
            let data = arg_string(stack.get(1));
            let append = stack.get(2).to_bool();
            let mut st = st_write.borrow_mut();
            if append {
                st.files.entry(path).or_default().push_str(&data);
            } else {
                st.files.insert(path, data);
            }
            stack.clear();
            Ok(CallbackReturn::Return)
        });
        ctx.set_global("__f15_out", out)?;
        ctx.set_global("__f15_read", read)?;
        ctx.set_global("__f15_write", write)?;
        let json = Table::new(&ctx);
        let decode = Callback::from_fn(&ctx, |ctx, _, mut stack| {
            let s = arg_string(stack.get(0));
            let v: serde_json::Value =
                serde_json::from_str(&s).map_err(|e| piccolo::Error::from(str_value(ctx, &e.to_string())))?;
            let lv = json_to_lua(ctx, &v)?;
            stack.clear();
            stack.push_back(lv);
            Ok(CallbackReturn::Return)
        });
        let encode = Callback::from_fn(&ctx, |ctx, _, mut stack| {
            let v = lua_to_json(stack.get(0), 0).map_err(|e| piccolo::Error::from(str_value(ctx, &e)))?;
            let s = serde_json::to_string(&v).map_err(|e| piccolo::Error::from(str_value(ctx, &e.to_string())))?;
            stack.clear();
            stack.push_back(str_value(ctx, &s));
            Ok(CallbackReturn::Return)
        });
        json.set(ctx, "decode", decode)?;
        json.set(ctx, "encode", encode)?;
        ctx.set_global("json", json)?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    run_chunk(lua, "f15-prelude", LUA_PRELUDE)
}

fn run_chunk(lua: &mut Lua, name: &str, code: &str) -> Result<(), String> {
    let ex = lua
        .try_enter(|ctx| {
            let closure = Closure::load(ctx, Some(name), code.as_bytes())?;
            Ok(ctx.stash(Executor::start(ctx, closure.into(), ())))
        })
        .map_err(|e| e.to_string())?;
    lua.execute::<()>(&ex).map_err(|e| e.to_string())
}

struct PiccoloEngine;

impl Engine for PiccoloEngine {
    fn name(&self) -> &'static str {
        "piccolo"
    }
    fn version(&self) -> &'static str {
        "0.3.3"
    }
    fn language(&self) -> &'static str {
        "lua"
    }

    fn run(&self, snippet: &Snippet) -> SnippetResult {
        let state = Rc::new(RefCell::new(State { out: String::new(), files: snippet.files.clone() }));
        let mut lua = Lua::core();
        let error = match setup(&mut lua, &state) {
            Err(e) => Some(format!("prelúdio: {e}")),
            Ok(()) => run_chunk(&mut lua, "main", &snippet.code).err(),
        };
        drop(lua);
        let st = std::mem::take(&mut *state.borrow_mut());
        SnippetResult { id: snippet.id.clone(), stdout: st.out, error, files: st.files, elapsed_us: 0 }
    }

    fn notes(&self) -> Vec<String> {
        vec![
            "Lua::core() (sem load_io): print, io e json são nossos.".into(),
            "Stdlib do piccolo 0.3.3 é mínima: string só tem len/lower/upper/reverse/sub; table só pack/unpack; não há tonumber, string.format, string.find/gsub/gmatch, table.insert/concat/sort.".into(),
        ]
    }
}

fn main() {
    serve(&PiccoloEngine);
}
