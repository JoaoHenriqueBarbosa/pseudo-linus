// `import` estático e `import()` de módulo embutido a partir do `/app/main.js` módulo ES.
import path, { join } from "node:path";
import * as events from "events";
console.log("static", typeof path.join, join("a", "b"), typeof events.default, typeof events.EventEmitter);
const ns = await import("node:path");
console.log("dynamic", ns.default === path, ns.join === join);
