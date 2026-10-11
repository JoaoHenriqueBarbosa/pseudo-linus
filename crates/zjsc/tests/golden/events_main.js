const E = require("events");
const L = (...a) => console.log(...a);
function t(f) {
  try { f(); L("no throw"); } catch (e) { L(e.constructor.name + "|" + e.name + "|" + e.code + "|" + e.message + "|" + JSON.stringify(Object.getOwnPropertyNames(e)) + "|" + String(e.stack).split("\n")[0]); }
}
const e = new E();
t(() => e.on("x", 1));
t(() => e.on("x"));
t(() => e.setMaxListeners(-1));
t(() => e.setMaxListeners("a"));
t(() => e.emit("error", 1));
t(() => e.emit("error", { a: 1 }));
t(() => e.emit("error"));
t(() => e.emit("error", new Error("x")));
t(() => E.setMaxListeners(1, {}));
t(() => E.getMaxListeners({}));
t(() => E.listenerCount({}, "x"));
t(() => { E.defaultMaxListeners = -1; });
t(() => { E.captureRejections = 1; });
t(() => new E({ captureRejections: 1 }));
L(Object.keys(E).join());
L(Object.getOwnPropertyNames(E).join());
L(Object.getOwnPropertyNames(E.prototype).join());
L(Object.getOwnPropertyNames(E.EventEmitterAsyncResource.prototype).join());
L(E.once.constructor.name, E.on.length, E.once.length, E.name, E.length, E.init === E, E.prototype.on === E.prototype.addListener);
L(Object.getPrototypeOf(process) instanceof E, Object.getPrototypeOf(Object.getPrototypeOf(process)) === E.prototype);
const w = new E(); w.on("a", function f() {}); w.once("a", function g() {});
L(w.rawListeners("a").map(f => f.name).join(), w.listeners("a").map(f => f.name).join(), Object.keys(w).join());
class X extends E {}
const x = new X();
L(Object.keys(x).join(), x instanceof E, x.getMaxListeners(), x.eventNames().length);
const r = [];
e.on("newListener", (t) => r.push("nl:" + t)); e.on("removeListener", (t) => r.push("rl:" + t)); e.on("q", () => {}); e.removeAllListeners("q");
L(r.join());
const et = new EventTarget(); et.addEventListener("x", function h() {});
L(E.getEventListeners(et, "x").length, E.listenerCount(et, "x"), E.getMaxListeners(et), (E.setMaxListeners(3, et), E.getMaxListeners(et)));
E.captureRejections = true;
const c = new E(); c.on("x", async () => { throw new Error("boom"); }); c.on("error", err => L("captured " + err.message)); c.emit("x");
E.captureRejections = false;
E.once(e, "z").then(v => L("onced " + JSON.stringify(v))); e.emit("z", 1, 2);
E.once(e, "x", 5).catch(x => L("once " + x.code + " " + x.message));
const a = new AbortController(); a.abort();
E.once(new E(), "x", { signal: a.signal }).catch(x => L("abort " + Object.getOwnPropertyNames(x).join() + "|" + (Object.getPrototypeOf(x) === Error.prototype) + "|" + x.constructor.name + "|" + x.name + "|" + x.code + "|" + String(x.stack).split("\n")[0]));
(async () => {
  const it = E.on(e, "w");
  setTimeout(() => { e.emit("w", 1); e.emit("w", 2); }, 0);
  let n = 0;
  for await (const v of it) { L("on " + JSON.stringify(v)); if (++n == 2) break; }
})();
