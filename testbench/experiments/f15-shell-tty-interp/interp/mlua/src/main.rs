//! mlua 0.12 (Lua 5.4 de referência em C, vendored) sobre o protocolo comum do F17.
//!
//! `Lua::new_with` carrega só coroutine, table, string, utf8 e math: sem `io`, `os`, `package` (e
//! portanto sem `require`), sem `debug`. A biblioteca base vem sempre, e nela há três portas pro
//! host que a seleção de StdLib não fecha: `print` (escreve no stdout C via `lua_writestring`),
//! `dofile` e `loadfile` (abrem arquivo com `fopen`). Trocamos `print` pelo nosso e removemos
//! `dofile`/`loadfile`; `load` fica, porque só compila string ou função. O `io` que o trecho vê é o
//! prelúdio Lua comum sobre o nosso FS, e `json` é nosso (serde_json).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use f15_interp_common::{Engine, LUA_PRELUDE, Snippet, SnippetResult, serve};
use mlua::{Lua, LuaOptions, LuaSerdeExt, StdLib, Value};

#[derive(Default)]
struct State {
    out: String,
    files: BTreeMap<String, String>,
}

fn setup(lua: &Lua, state: &Rc<RefCell<State>>, keep_base_io: bool) -> mlua::Result<()> {
    let g = lua.globals();
    let st = state.clone();
    g.set(
        "__f15_out",
        lua.create_function(move |_, s: mlua::LuaString| {
            st.borrow_mut().out.push_str(&s.to_string_lossy());
            Ok(())
        })?,
    )?;
    let st = state.clone();
    g.set(
        "__f15_read",
        lua.create_function(move |_, p: String| Ok(st.borrow().files.get(&p).cloned()))?,
    )?;
    let st = state.clone();
    g.set(
        "__f15_write",
        lua.create_function(move |_, (p, data, append): (String, mlua::LuaString, Option<bool>)| {
            let data = data.to_string_lossy();
            let mut st = st.borrow_mut();
            if append.unwrap_or(false) {
                st.files.entry(p).or_default().push_str(&data);
            } else {
                st.files.insert(p, data);
            }
            Ok(())
        })?,
    )?;
    // Portas da biblioteca base pro FS do host (a variante `--keep-base-io` mantém, só pra medir).
    if !keep_base_io {
        g.set("dofile", Value::Nil)?;
        g.set("loadfile", Value::Nil)?;
    }
    let json = lua.create_table()?;
    json.set(
        "decode",
        lua.create_function(|lua, s: String| {
            let v: serde_json::Value = serde_json::from_str(&s).map_err(mlua::Error::external)?;
            let opts = mlua::serde::SerializeOptions::new()
                .set_array_metatable(false)
                .serialize_none_to_null(false)
                .serialize_unit_to_null(false);
            lua.to_value_with(&v, opts)
        })?,
    )?;
    json.set(
        "encode",
        lua.create_function(|_, v: Value| {
            let ser = v.to_serializable().sort_keys(true);
            serde_json::to_string(&ser).map_err(mlua::Error::external)
        })?,
    )?;
    g.set("json", json)?;
    lua.load(LUA_PRELUDE).set_name("=f15-prelude").exec()?;
    Ok(())
}

struct MluaEngine {
    /// Variante ingênua (`--keep-base-io`): só troca o print, deixa dofile/loadfile da base.
    keep_base_io: bool,
}

impl Engine for MluaEngine {
    fn name(&self) -> &'static str {
        "mlua"
    }
    fn version(&self) -> &'static str {
        "0.12.1"
    }
    fn language(&self) -> &'static str {
        "lua"
    }

    fn run(&self, snippet: &Snippet) -> SnippetResult {
        let state = Rc::new(RefCell::new(State { out: String::new(), files: snippet.files.clone() }));
        let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
        let error = match Lua::new_with(libs, LuaOptions::default()) {
            Err(e) => Some(format!("Lua::new_with: {e}")),
            Ok(lua) => match setup(&lua, &state, self.keep_base_io) {
                Err(e) => Some(format!("prelúdio: {e}")),
                Ok(()) => lua.load(snippet.code.as_str()).set_name("=main").exec().err().map(|e| e.to_string()),
            },
        };
        let st = std::mem::take(&mut *state.borrow_mut());
        SnippetResult { id: snippet.id.clone(), stdout: st.out, error, files: st.files, elapsed_us: 0 }
    }

    fn notes(&self) -> Vec<String> {
        vec![
            "StdLib = COROUTINE|TABLE|STRING|UTF8|MATH; base sempre carregada: print trocado, dofile/loadfile removidos.".into(),
            "io é o prelúdio Lua comum sobre o nosso FS; json.decode/encode nossos via serde_json (encode com chaves ordenadas).".into(),
        ]
    }
}

fn main() {
    let keep_base_io = std::env::args().any(|a| a == "--keep-base-io");
    serve(&MluaEngine { keep_base_io });
}
