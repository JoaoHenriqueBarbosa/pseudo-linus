// Sonda a forma das funções dos módulos embutidos escritos em JavaScript: `String(fn)`, `prototype` próprio, `name`,
// `length`, o toString de método de classe e de função de seta, e os descritores. Roda no bun do oráculo.
const modules = ["url", "events", "stream", "querystring", "timers", "string_decoder", "async_hooks", "timers/promises"];
const out = [];
const show = (label, text) => out.push(label + " " + JSON.stringify(text));
for (const name of modules) {
  const mod = require(name);
  const holders = [["", mod]];
  for (const key of Object.getOwnPropertyNames(mod).slice(0, 12)) {
    const value = mod[key];
    if (typeof value === "function") holders.push([key, value]);
  }
  for (const [key, value] of holders) {
    if (typeof value !== "function") continue;
    const label = name + "." + (key || "<module>");
    show(label + " string", Function.prototype.toString.call(value).slice(0, 160));
    show(label + " shape", {
      hasPrototype: Object.prototype.hasOwnProperty.call(value, "prototype"),
      name: value.name,
      length: value.length,
      own: Object.getOwnPropertyNames(value),
      isClass: /^class\b/.test(Function.prototype.toString.call(value)),
    });
    if (value.prototype && typeof value.prototype === "object") {
      for (const method of Object.getOwnPropertyNames(value.prototype).slice(0, 4)) {
        const descriptor = Object.getOwnPropertyDescriptor(value.prototype, method);
        const fn = descriptor.value || descriptor.get;
        if (typeof fn !== "function") continue;
        show(label + ".prototype." + method + " string", Function.prototype.toString.call(fn).slice(0, 160));
        show(label + ".prototype." + method + " shape", {
          hasPrototype: Object.prototype.hasOwnProperty.call(fn, "prototype"),
          name: fn.name,
          length: fn.length,
        });
      }
    }
  }
}
console.log(out.join("\n"));
