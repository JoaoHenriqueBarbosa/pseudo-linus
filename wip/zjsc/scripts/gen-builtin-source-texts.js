// Gera `src/api/builtin_source_texts.txt`: o texto que o bun 1.4.2 mostra no `Function.prototype.toString` das funções
// de JavaScript dos módulos embutidos (o fonte empacotado, com `@intrínsecos`, `!0` e os nomes com sufixo do
// bundler). Roda no bun do oráculo e anda as exportações de cada módulo como `tests/golden/builtin_function_shape_main.js`:
// valores e acessores (sem chamar acessor), as propriedades das funções e o `prototype` delas, até a profundidade 3.
//
// Uso: `bun scripts/gen-builtin-source-texts.js > src/api/builtin_source_texts.txt`.
//
// Formato: uma seção por módulo (`@@ nome`) e, em cada linha, o caminho da função a partir das exportações (os
// segmentos separados por U+001F; `<get x>` e `<set x>` são o acessor de `x`), um TAB e o texto com `\`, quebra de
// linha, retorno de carro e TAB escritos como `\\`, `\n`, `\r` e `\t`. Entram também as funções nativas do bun
// (`[native code]`): o runtime só põe o texto numa função que no nosso lado é de JavaScript (o `StringDecoder`, nativo
// no bun e escrito em JavaScript aqui), e deixa em paz a nativa de verdade, para a divergência dela aparecer no golden.
// Uma função alcançada por dois caminhos só entra no primeiro.
const modules = ["url", "events", "stream", "querystring", "timers", "timers/promises", "string_decoder", "async_hooks"];

const escape = text => text.replace(/\\/g, "\\\\").replace(/\n/g, "\\n").replace(/\r/g, "\\r").replace(/\t/g, "\\t");
const out = ["# GERADO por `scripts/gen-builtin-source-texts.js` no bun 1.4.2 do oráculo. Não editar à mão."];
const seen = new Set();
let entries;

function record(path, fn) {
  entries.push(`${path.join("\x1f")}\t${escape(Function.prototype.toString.call(fn))}`);
}

function walk(path, value, depth) {
  if (typeof value !== "function" || seen.has(value)) return;
  seen.add(value);
  record(path, value);
  if (depth >= 3) return;
  for (const key of Object.getOwnPropertyNames(value)) {
    if (key === "prototype" || key === "length" || key === "name" || key === "arguments" || key === "caller") continue;
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    walk([...path, key], descriptor.value, depth + 1);
    walk([...path, `<get ${key}>`], descriptor.get, depth + 1);
    walk([...path, `<set ${key}>`], descriptor.set, depth + 1);
  }
  const proto = Object.getOwnPropertyDescriptor(value, "prototype")?.value;
  if (proto && typeof proto === "object") {
    for (const key of Object.getOwnPropertyNames(proto)) {
      const descriptor = Object.getOwnPropertyDescriptor(proto, key);
      walk([...path, "prototype", key], descriptor.value, depth + 1);
      walk([...path, "prototype", `<get ${key}>`], descriptor.get, depth + 1);
      walk([...path, "prototype", `<set ${key}>`], descriptor.set, depth + 1);
    }
  }
}

for (const name of modules) {
  const mod = require(name);
  entries = [];
  walk([], mod, 0);
  for (const key of Object.getOwnPropertyNames(mod)) {
    const descriptor = Object.getOwnPropertyDescriptor(mod, key);
    walk([key], descriptor.value, 1);
    walk([`<get ${key}>`], descriptor.get, 1);
    walk([`<set ${key}>`], descriptor.set, 1);
  }
  out.push(`@@ ${name}`, ...entries);
}
console.log(out.join("\n"));
