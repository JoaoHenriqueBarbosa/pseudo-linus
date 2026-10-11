import Module, { createRequire, builtinModules, isBuiltin } from "node:module";
const r = createRequire(import.meta.url);
console.log("createRequire", typeof createRequire, typeof r, r("node:path") === (await import("node:path")).default, typeof r.resolve);
console.log("builtinModules", builtinModules.length, Module.builtinModules === builtinModules);
console.log("isBuiltin", isBuiltin("fs"), isBuiltin("node:fs"), isBuiltin("nope"));
console.log("_load", typeof Module._load, typeof Module._resolveFilename, typeof Module._cache);
