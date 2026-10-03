//! Protocolo comum dos binários de interpretador do F17 (H39).
//!
//! Cada binário `f15-interp-<engine>` lê um [`Request`] em JSON no stdin, roda os trechos e escreve
//! um [`Response`] em JSON no stdout. O orquestrador (`src/interp.rs` do crate principal) compara as
//! saídas com o esperado do corpus, mede tamanho de binário e roda o binário sob `strace` pra provar
//! que nenhum trecho toca o host.
//!
//! Regras que todo engine segue:
//!
//! - **Contexto novo por trecho.** Cada [`Snippet`] roda num runtime/contexto recém-criado, como um
//!   `python3 -c` ou `node -e` faria. O tempo de [`SnippetResult::elapsed_us`] inclui criar o contexto.
//! - **Saída só pelo nosso lado.** `print`/`console.log` escrevem no buffer que vira
//!   [`SnippetResult::stdout`]; nada vai pro stdout do processo host.
//! - **Arquivo só pelo nosso FS.** `open`/`readFileSync`/`io.open` (o que a linguagem oferecer) leem
//!   e escrevem no mapa [`Snippet::files`]; caminho ausente vira o erro da linguagem pra arquivo
//!   inexistente.
//! - **Marcadores pro strace.** [`serve`] chama [`marker`] antes e depois de rodar os trechos. O
//!   marcador é um `stat` num caminho que não existe; no trace, tudo que aparece entre os dois
//!   marcadores foi causado pelos trechos (ou pelo engine) e não pela inicialização do processo.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// Caminho do marcador de início (nunca existe).
pub const MARK_BEGIN: &str = "/f15-strace-marker-begin";
/// Caminho do marcador de fim (nunca existe).
pub const MARK_END: &str = "/f15-strace-marker-end";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Request {
    pub snippets: Vec<Snippet>,
    /// Quantas vezes criar contexto e rodar [`Request::startup_code`] pra medir startup.
    #[serde(default)]
    pub startup_iterations: usize,
    /// One-liner usado na medição de startup (ex.: `print(1+1)`).
    #[serde(default)]
    pub startup_code: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snippet {
    pub id: String,
    pub code: String,
    /// FS em memória visível pro trecho: caminho -> conteúdo.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SnippetResult {
    pub id: String,
    pub stdout: String,
    /// Erro não tratado do trecho (exceção, erro de sintaxe), já em texto.
    #[serde(default)]
    pub error: Option<String>,
    /// FS em memória depois do trecho (pra conferir escrita pelo nosso `open`).
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    pub elapsed_us: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Startup {
    pub iterations: usize,
    pub min_us: f64,
    pub median_us: f64,
    pub p90_us: f64,
    /// Saída da última iteração (confere que o one-liner rodou de verdade).
    pub output: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Response {
    pub engine: String,
    pub version: String,
    pub language: String,
    pub startup: Startup,
    pub results: Vec<SnippetResult>,
    /// Observações do próprio engine (ex.: o que foi removido da stdlib e por quê).
    #[serde(default)]
    pub notes: Vec<String>,
}

/// O que cada engine precisa saber fazer.
pub trait Engine {
    fn name(&self) -> &'static str;
    fn version(&self) -> &'static str;
    fn language(&self) -> &'static str;
    /// Cria um contexto novo, roda o trecho e devolve stdout, erro e o FS depois da execução.
    fn run(&self, snippet: &Snippet) -> SnippetResult;
    fn notes(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Prelúdio JS comum (boa e rquickjs). Usa primitivas nativas que o engine registra e depois some com
/// elas do escopo global:
///
/// - `__f15_out(s)`: escreve `s` no nosso stdout (sem quebra de linha);
/// - `__f15_err(s)`: idem no nosso stderr (descartado, mas não vai pro host);
/// - `__f15_read(path)`: conteúdo do arquivo do nosso FS, ou `undefined`;
/// - `__f15_write(path, data, append)`: grava no nosso FS.
///
/// Oferece `console.log/error/warn/info` e um `require` que só conhece `fs`/`node:fs` (shim com
/// `readFileSync`, `writeFileSync`, `appendFileSync`, `existsSync`), com os mesmos erros do node
/// (`ENOENT`, `MODULE_NOT_FOUND`).
pub const JS_PRELUDE: &str = r#"
(function () {
  const out = globalThis.__f15_out, err = globalThis.__f15_err;
  const read = globalThis.__f15_read, write = globalThis.__f15_write;
  delete globalThis.__f15_out; delete globalThis.__f15_err;
  delete globalThis.__f15_read; delete globalThis.__f15_write;
  const fmt = (args) => args.map((a) => String(a)).join(' ') + '\n';
  globalThis.console = {
    log: (...a) => out(fmt(a)),
    info: (...a) => out(fmt(a)),
    error: (...a) => err(fmt(a)),
    warn: (...a) => err(fmt(a)),
  };
  function enoent(p, syscall) {
    const e = new Error("ENOENT: no such file or directory, " + syscall + " '" + p + "'");
    e.code = 'ENOENT'; e.errno = -2; e.syscall = syscall; e.path = p;
    return e;
  }
  const norm = (p) => { p = String(p); return p.startsWith('./') ? p.slice(2) : p; };
  const fs = {
    readFileSync(p, _opts) {
      const v = read(norm(p));
      if (v === undefined) throw enoent(String(p), 'open');
      return v;
    },
    writeFileSync(p, data) { write(norm(p), String(data), false); },
    appendFileSync(p, data) { write(norm(p), String(data), true); },
    existsSync(p) { return read(norm(p)) !== undefined; },
  };
  const modules = { fs: fs, 'node:fs': fs };
  globalThis.require = function require(name) {
    if (Object.prototype.hasOwnProperty.call(modules, name)) return modules[name];
    const e = new Error("Cannot find module '" + name + "'");
    e.code = 'MODULE_NOT_FOUND';
    throw e;
  };
})();
"#;

/// Prelúdio Lua comum (mlua e piccolo). Só usa o que o piccolo também tem (sem `string.find`,
/// `table.concat`, nem metatable de string), pra que a diferença medida seja do engine e não do
/// prelúdio. Primitivas nativas, removidas do escopo global depois:
///
/// - `__f15_out(s)`: escreve `s` no nosso stdout;
/// - `__f15_read(path)`: conteúdo do nosso FS ou `nil`;
/// - `__f15_write(path, data, append)`: grava no nosso FS.
///
/// Oferece `print`, `io.write`, `io.open` (modos r, w, a; métodos read com "a"/"l"/"L"/"n" e as
/// formas com `*`, lines, write, close) e `io.lines`. O `json` (decode/encode, chaves ordenadas no
/// encode) é registrado em Rust por cada engine.
pub const LUA_PRELUDE: &str = r#"
local out, read, write = __f15_out, __f15_read, __f15_write
__f15_out, __f15_read, __f15_write = nil, nil, nil
local sub, len, tostr, sel = string.sub, string.len, tostring, select

print = function(...)
  local n = sel('#', ...)
  local s = ''
  for i = 1, n do
    if i > 1 then s = s .. '\t' end
    s = s .. tostr((sel(i, ...)))
  end
  out(s .. '\n')
end

local function next_line(f, keep)
  if f.pos > len(f.data) then return nil end
  local i = f.pos
  local n = len(f.data)
  while i <= n and sub(f.data, i, i) ~= '\n' do i = i + 1 end
  local line
  if keep then line = sub(f.data, f.pos, i) else line = sub(f.data, f.pos, i - 1) end
  f.pos = i + 1
  return line
end

local function new_file(path, mode, data)
  local f = { path = path, mode = mode, data = data, pos = 1, closed = false }
  f.read = function(self, fmt)
    fmt = fmt or 'l'
    if sub(fmt, 1, 1) == '*' then fmt = sub(fmt, 2) end
    local c = sub(fmt, 1, 1)
    if c == 'a' then
      local rest = sub(self.data, self.pos)
      self.pos = len(self.data) + 1
      return rest
    elseif c == 'l' then return next_line(self, false)
    elseif c == 'L' then return next_line(self, true)
    elseif c == 'n' then
      local line = next_line(self, false)
      if line == nil then return nil end
      return tonumber and tonumber(line) or nil
    end
    error("bad argument #1 to 'read' (invalid format)")
  end
  f.lines = function(self)
    return function() return next_line(self, false) end
  end
  f.write = function(self, ...)
    for i = 1, sel('#', ...) do
      self.data = self.data .. tostr((sel(i, ...)))
    end
    return self
  end
  f.close = function(self)
    if not self.closed and self.mode ~= 'r' then write(self.path, self.data, false) end
    self.closed = true
    return true
  end
  return f
end

io = {}
io.write = function(...)
  for i = 1, sel('#', ...) do out(tostr((sel(i, ...)))) end
  return io
end
io.open = function(path, mode)
  mode = mode or 'r'
  local m = sub(mode, 1, 1)
  if m == 'r' then
    local d = read(path)
    if d == nil then return nil, path .. ': No such file or directory', 2 end
    return new_file(path, 'r', d)
  elseif m == 'w' then
    write(path, '', false)
    return new_file(path, 'w', '')
  elseif m == 'a' then
    return new_file(path, 'a', read(path) or '')
  end
  error("bad argument #2 to 'open' (invalid mode)")
end
io.lines = function(path)
  local f, msg = io.open(path, 'r')
  if f == nil then error(msg) end
  return f:lines()
end
"#;

/// Faz um `stat` num caminho que não existe, pra servir de marcador no strace.
pub fn marker(path: &str) {
    let _ = std::fs::metadata(path);
}

/// Mede o startup: cria contexto e roda o one-liner `iterations` vezes.
pub fn measure_startup(engine: &dyn Engine, code: &str, iterations: usize) -> Startup {
    let mut samples = Vec::with_capacity(iterations);
    let mut output = String::new();
    let snippet = Snippet { id: "startup".into(), code: code.into(), files: BTreeMap::new() };
    for _ in 0..iterations {
        let start = Instant::now();
        let r = engine.run(&snippet);
        samples.push(start.elapsed().as_secs_f64() * 1e6);
        output = match r.error {
            Some(e) => format!("ERRO: {e}"),
            None => r.stdout,
        };
    }
    samples.sort_by(|a, b| a.total_cmp(b));
    let pick = |q: f64| -> f64 {
        if samples.is_empty() {
            return 0.0;
        }
        let idx = ((samples.len() as f64 - 1.0) * q).round() as usize;
        samples[idx]
    };
    Startup { iterations, min_us: pick(0.0), median_us: pick(0.5), p90_us: pick(0.9), output }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Engine de teste: devolve o código como saída.
    struct Echo;

    impl Engine for Echo {
        fn name(&self) -> &'static str {
            "echo"
        }
        fn version(&self) -> &'static str {
            "0"
        }
        fn language(&self) -> &'static str {
            "test"
        }
        fn run(&self, snippet: &Snippet) -> SnippetResult {
            SnippetResult { id: snippet.id.clone(), stdout: snippet.code.clone(), ..SnippetResult::default() }
        }
    }

    #[test]
    fn startup_reports_quantiles_and_last_output() {
        let s = measure_startup(&Echo, "2\n", 9);
        assert_eq!(s.iterations, 9);
        assert_eq!(s.output, "2\n");
        assert!(s.min_us <= s.median_us && s.median_us <= s.p90_us);
    }

    #[test]
    fn request_defaults_and_roundtrip() {
        let req: Request = serde_json::from_str(r#"{"snippets":[{"id":"a","code":"x"}]}"#).unwrap();
        assert_eq!(req.startup_iterations, 0);
        assert!(req.snippets[0].files.is_empty());
        let back: Request = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert_eq!(back.snippets[0].code, "x");
    }

    #[test]
    fn preludes_remove_primitives_from_global_scope() {
        // As primitivas nativas somem do escopo global depois do prelúdio.
        for p in ["__f15_out", "__f15_read", "__f15_write"] {
            assert!(JS_PRELUDE.contains(&format!("delete globalThis.{p}")), "JS: {p}");
            assert!(LUA_PRELUDE.contains(p), "Lua: {p}");
        }
        assert!(LUA_PRELUDE.contains("__f15_out, __f15_read, __f15_write = nil, nil, nil"));
    }
}

/// Lê o pedido do stdin, roda e escreve a resposta no stdout. É o `main` de todo engine.
pub fn serve(engine: &dyn Engine) {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).expect("ler pedido do stdin");
    let req: Request = serde_json::from_str(&input).expect("pedido JSON válido");
    marker(MARK_BEGIN);
    let mut results = Vec::with_capacity(req.snippets.len());
    for s in &req.snippets {
        let start = Instant::now();
        let mut r = engine.run(s);
        r.id = s.id.clone();
        r.elapsed_us = start.elapsed().as_micros() as u64;
        results.push(r);
    }
    let startup = if req.startup_iterations > 0 {
        measure_startup(engine, &req.startup_code, req.startup_iterations)
    } else {
        Startup::default()
    };
    marker(MARK_END);
    let resp = Response {
        engine: engine.name().into(),
        version: engine.version().into(),
        language: engine.language().into(),
        startup,
        results,
        notes: engine.notes(),
    };
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(&mut out, &resp).expect("escrever resposta");
    out.write_all(b"\n").expect("escrever resposta");
}
