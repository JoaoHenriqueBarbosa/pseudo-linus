const cp = require("node:child_process");
const show = (label, fn) => {
  try {
    const r = fn();
    console.log(label, "ok", typeof r);
  } catch (e) {
    console.log(label, "throws", e.name, e.code, e.message);
  }
};
console.log("keys", Object.keys(cp).join(","));
console.log("names", cp.spawn.name, cp.exec.name, cp.fork.name, cp.ChildProcess.name);
show("spawn no args", () => cp.spawn());
show("spawn number", () => cp.spawn(1));
show("spawn empty", () => cp.spawn(""));
show("spawn args", () => cp.spawn("x", 5));
show("spawn options", () => cp.spawn("x", [], 5));
show("spawn cwd", () => cp.spawn("x", [], { cwd: 5 }));
show("spawn shell", () => cp.spawn("x", [], { shell: 5 }));
show("spawn timeout", () => cp.spawn("x", [], { timeout: -1 }));
show("spawn killSignal", () => cp.spawn("x", [], { killSignal: "NOPE" }));
show("spawn stdio", () => cp.spawn("x", [], { stdio: "nope" }));
show("exec no args", () => cp.exec());
show("execFile no args", () => cp.execFile());
show("fork bad", () => cp.fork(5));
show("execSync no args", () => cp.execSync());
show("spawnSync no args", () => cp.spawnSync());
const c = new cp.ChildProcess();
console.log("child", c.pid, c.killed, c.exitCode, c.signalCode, c.spawnfile, c.spawnargs);
// Divergência declarada: o bun compila o módulo embutido com o bundler, que inline o `const handle`, e a mensagem
// diz `(evaluating 'this.#handle')`; aqui o fonte é o do TS, e o trecho é `handle.connected`. Só o tipo é conferido.
try { c.connected; } catch (e) { console.log("connected throws", e.name); }
show("kill unspawned", () => c.kill());
