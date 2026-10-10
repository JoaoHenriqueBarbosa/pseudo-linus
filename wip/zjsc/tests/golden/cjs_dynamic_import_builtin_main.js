// `import()` de módulo embutido a partir do `/app/main.js` CommonJS: o namespace, o `default` igual ao `require`, os
// nomes com e sem `node:`, e a rejeição de `node:` desconhecido.
const path = require("path");
import("node:path").then(ns => {
  console.log("node:path", Object.prototype.toString.call(ns), ns.default === path, typeof ns.join, ns.join("a", "b"));
  return import("path");
}).then(ns => {
  console.log("path", ns.default === path, Object.keys(ns).includes("default"));
  return import("node:events");
}).then(ns => {
  console.log("node:events", typeof ns.default, ns.default === require("events"), typeof ns.EventEmitter);
  return import("node:nope");
}).then(
  () => console.log("node:nope resolveu"),
  error => console.log("node:nope", error.name, error.code, error.message),
);
