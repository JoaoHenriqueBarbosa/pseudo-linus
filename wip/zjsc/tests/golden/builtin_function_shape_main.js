// A forma das funções de JavaScript dos módulos embutidos do bun: `prototype` próprio, `name`, `length`, as chaves
// próprias e o texto do `Function.prototype.toString` (comprimento, hash e o começo). Anda as exportações de cada
// módulo (valores e acessores, sem chamar acessor), as propriedades das funções e o `prototype` delas, até a
// profundidade 3. Roda como `/app/main.js`; a saída é a do bun do oráculo.
const modules = ["url", "events", "stream", "querystring", "timers", "timers/promises", "string_decoder", "async_hooks"];
// "skip": tudo menos o `URLPattern`; "only": só o `URLPattern`, que o zjsc ainda não tem (o teste dele fica `ignore`).
const URL_PATTERN = "skip";
const allowed = path => (URL_PATTERN === "only") === path.startsWith("url.URLPattern");

function fnv1a(text) {
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, "0");
}

const lines = [];
const seen = new Set();

function describe(path, fn) {
  if (!allowed(path)) return;
  const text = Function.prototype.toString.call(fn);
  const own = Object.getOwnPropertyNames(fn).join(",");
  const preview = text.slice(0, 48).replace(/\s+/g, " ");
  lines.push(`${path}\t${Object.prototype.hasOwnProperty.call(fn, "prototype") ? "P" : "-"}\t${fn.name}\t${fn.length}\t[${own}]\t${text.length}:${fnv1a(text)}:${preview}`);
}

function walk(path, value, depth) {
  if (typeof value !== "function" || seen.has(value)) return;
  seen.add(value);
  describe(path, value);
  if (depth >= 3) return;
  for (const key of Object.getOwnPropertyNames(value)) {
    if (key === "prototype" || key === "length" || key === "name" || key === "arguments" || key === "caller") continue;
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    walk(`${path}.${key}`, descriptor.value, depth + 1);
    walk(`${path}.<get ${key}>`, descriptor.get, depth + 1);
    walk(`${path}.<set ${key}>`, descriptor.set, depth + 1);
  }
  const proto = Object.getOwnPropertyDescriptor(value, "prototype")?.value;
  if (proto && typeof proto === "object") {
    for (const key of Object.getOwnPropertyNames(proto)) {
      const descriptor = Object.getOwnPropertyDescriptor(proto, key);
      walk(`${path}.prototype.${key}`, descriptor.value, depth + 1);
      walk(`${path}.prototype.<get ${key}>`, descriptor.get, depth + 1);
      walk(`${path}.prototype.<set ${key}>`, descriptor.set, depth + 1);
    }
  }
}

for (const name of modules) {
  const mod = require(name);
  walk(name, mod, 0);
  for (const key of Object.getOwnPropertyNames(mod)) {
    const descriptor = Object.getOwnPropertyDescriptor(mod, key);
    walk(`${name}.${key}`, descriptor.value, 1);
    walk(`${name}.<get ${key}>`, descriptor.get, 1);
    walk(`${name}.<set ${key}>`, descriptor.set, 1);
  }
}
console.log(lines.join("\n"));
