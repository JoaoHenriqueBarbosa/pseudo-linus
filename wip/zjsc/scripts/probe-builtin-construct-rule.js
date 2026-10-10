// Classifica as funções de JavaScript (não nativas, não classes) de todos os módulos embutidos do bun por:
// tem `prototype` próprio, nome com inicial maiúscula, e se é declaração ou expressão (pelo início do texto). Serve
// para achar a regra com que o bun decide quais funções dos módulos embutidos constroem. Roda no bun do oráculo.
const names = require("module").builtinModules.filter(n => !n.startsWith("bun") && !n.startsWith("node:") && !n.startsWith("_"));
const rows = [];
const seen = new Set();
function visit(label, fn) {
  if (typeof fn !== "function" || seen.has(fn)) return;
  seen.add(fn);
  const text = Function.prototype.toString.call(fn);
  if (/\{ \[native code\] \}$/.test(text)) return;
  const kind = /^class\b/.test(text) ? "class" : /^async function\b/.test(text) ? "asyncdecl" : /^function\b/.test(text) ? "function" : /^async\b/.test(text) ? "asyncmethod" : /=>/.test(text.slice(0, 80)) ? "arrow" : "method";
  rows.push([label, Object.prototype.hasOwnProperty.call(fn, "prototype") ? "P" : "-", fn.name, kind].join("\t"));
}
for (const name of names) {
  let mod;
  try { mod = require(name); } catch { continue; }
  visit(name, mod);
  for (const key of Object.getOwnPropertyNames(mod)) {
    let value;
    try { value = mod[key]; } catch { continue; }
    visit(name + "." + key, value);
    if (typeof value === "function" && value.prototype && typeof value.prototype === "object") {
      for (const method of Object.getOwnPropertyNames(value.prototype)) {
        const descriptor = Object.getOwnPropertyDescriptor(value.prototype, method);
        visit(name + "." + key + ".prototype." + method, descriptor.value);
        visit(name + "." + key + ".prototype.<get " + method + ">", descriptor.get);
      }
    }
  }
}
console.log(rows.join("\n"));
