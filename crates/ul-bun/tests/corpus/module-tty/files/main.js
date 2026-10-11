
const tty = require("node:tty");
const fs = require("node:fs");
const out = [];
const log = (...a) => out.push(a.map(x => typeof x === "string" ? x : Bun.inspect(x)).join(" "));
log("keys", Object.keys(tty));
log("same module", require("tty") === tty, require("node:tty") === tty);
log("isatty", [0, 1, 2, 3, 99, -1, 1.5, "1", null, undefined, {}].map(fd => { try { return tty.isatty(fd); } catch (e) { return `${e.name}:${e.code}:${e.message}`; } }));
log("types", typeof tty.ReadStream, typeof tty.WriteStream, tty.ReadStream.name, tty.WriteStream.name, tty.ReadStream.length, tty.WriteStream.length, tty.isatty.length, tty.isatty.name);
log("chain RS", Object.getPrototypeOf(tty.ReadStream) === fs.ReadStream, Object.getPrototypeOf(tty.ReadStream.prototype) === fs.ReadStream.prototype);
log("chain WS", Object.getPrototypeOf(tty.WriteStream) === fs.WriteStream, Object.getPrototypeOf(tty.WriteStream.prototype) === fs.WriteStream.prototype);
log("proto WS", Object.getOwnPropertyNames(tty.WriteStream.prototype));
log("proto RS", Object.getOwnPropertyNames(tty.ReadStream.prototype));
log("tostring", String(Object.prototype.toString.call(tty)));
for (const fd of [0, 1, 2]) {
  for (const [label, Ctor] of [["RS", tty.ReadStream], ["WS", tty.WriteStream]]) {
    try { const s = new Ctor(fd); log(label, fd, "created", s.isTTY, s.constructor.name, s.fd, s.isRaw, s instanceof Ctor, Object.keys(s).length > 0); }
    catch (e) { log(label, fd, "throws", e.name, e.code, e.message); }
  }
}
for (const bad of [-1, 1.5, "1", null, undefined, 2 ** 31]) {
  for (const [label, Ctor] of [["RS", tty.ReadStream], ["WS", tty.WriteStream]]) {
    try { new Ctor(bad); log(label, String(bad), "created"); }
    catch (e) { log(label, String(bad), "throws", e.name, e.code, e.message); }
  }
}
log("call without new", (() => { try { const s = tty.WriteStream(1); return [s instanceof tty.WriteStream, s.isTTY]; } catch (e) { return e.message; } })());
console.log(out.join("\n"));
process.exit(0);
