// node:timers e node:timers/promises contra o bun 1.4.2: superfície, identidade com os globais, promises,
// active/enroll/unenroll, ordem de resolução, scheduler, setInterval assíncrono, abort e validação.
const timers = require("timers");
const tp = require("timers/promises");
const err = e => [e.constructor.name, e.name, e.code, e.message].join(" | ");
console.log(timers === require("node:timers"), Object.keys(timers).join(","));
console.log(timers.setTimeout === setTimeout, timers.clearImmediate === clearImmediate, timers.promises === tp);
console.log(Object.keys(tp).join(","), Object.keys(tp.scheduler).join(","), tp.scheduler.yield === tp.setImmediate);
console.log(tp.setTimeout.name, tp.setTimeout.length, tp.setInterval.length, tp.setImmediate.length, tp.scheduler.wait.length);
const d = Object.getOwnPropertyDescriptor(timers, "promises");
console.log(typeof d.get, typeof d.set, d.enumerable, d.configurable);
for (const name of ["active", "enroll", "unenroll"]) {
  try {
    timers[name]({});
  } catch (e) {
    console.log(name, err(e), e instanceof Error, Object.keys(e).join(","));
  }
}
const t = setTimeout(() => console.log("never"), 50);
timers.unenroll(t);
const t2 = setTimeout(() => console.log("enrolled fired"), 1);
timers.enroll(t2, 5);
timers.active(t2);
(async () => {
  const order = [];
  const ps = [
    tp.setTimeout(5, "t5").then(v => order.push(v)),
    tp.setTimeout(1, "t1").then(v => order.push(v)),
    tp.setImmediate("imm").then(v => order.push(v)),
    tp.scheduler.wait(2).then(v => order.push("wait " + v)),
    tp.scheduler.yield().then(v => order.push("yield " + v)),
  ];
  await Promise.all(ps);
  console.log(order.join(","));
  console.log(await tp.setTimeout(), await tp.setTimeout(undefined, 7), await tp.setImmediate());
  for (const args of [["x"], [1, 1, null], [1, 1, { ref: 1 }], [1, 1, { signal: {} }], [1, 1, 5]]) {
    try {
      await tp.setTimeout(...args);
      console.log("ok", args.length);
    } catch (e) {
      console.log("setTimeout", err(e));
    }
  }
  try {
    await tp.setImmediate(1, { ref: "no" });
  } catch (e) {
    console.log("setImmediate", err(e));
  }
  const ac = new AbortController();
  ac.abort("why");
  try {
    await tp.setTimeout(1, 1, { signal: ac.signal });
  } catch (e) {
    console.log("aborted", err(e), e.cause, Object.keys(e).join(","));
  }
  const ac2 = new AbortController();
  const pending = tp.setTimeout(1000, 1, { signal: ac2.signal });
  ac2.abort();
  try {
    await pending;
  } catch (e) {
    console.log("abort later", err(e), e.cause?.name);
  }
  let n = 0;
  const it = tp.setInterval(2, "tick");
  console.log(typeof it.next, typeof it.return, typeof it[Symbol.asyncIterator], it[Symbol.asyncIterator]() === it);
  for await (const v of it) {
    console.log("iter", v, ++n);
    if (n === 3) break;
  }
  const bad = tp.setInterval("x");
  console.log(Object.keys(bad).join(","));
  try {
    await bad.next();
  } catch (e) {
    console.log("interval", err(e));
  }
  const ac3 = new AbortController();
  const it3 = tp.setInterval(1, "a", { signal: ac3.signal });
  console.log(JSON.stringify(await it3.next()));
  ac3.abort();
  try {
    console.log(JSON.stringify(await it3.next()));
    console.log(JSON.stringify(await it3.next()));
  } catch (e) {
    console.log("interval abort", err(e));
  }
  timers.promises = 5;
  console.log(timers.promises);
})();
