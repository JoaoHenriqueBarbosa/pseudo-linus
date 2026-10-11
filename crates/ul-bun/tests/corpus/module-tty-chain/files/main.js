
const tty = require("node:tty");
const fs = require("node:fs");
const stream = require("node:stream");
const out = [];
const log = (...a) => out.push(a.map(x => typeof x === "string" ? x : Bun.inspect(x)).join(" "));
const chain = (o) => { const names = []; for (let p = o; p; p = Object.getPrototypeOf(p)) names.push(p.constructor === Object && p !== Object.prototype ? "<obj>" : (typeof p === "function" ? "fn:" + p.name : p.constructor?.name ?? "null")); return names; };
log("WS proto chain", chain(tty.WriteStream.prototype));
log("RS proto chain", chain(tty.ReadStream.prototype));
log("fs.WS proto chain", chain(fs.WriteStream.prototype));
log("WS ctor chain", chain(tty.WriteStream));
log("fs.WS own", Object.getOwnPropertyNames(fs.WriteStream.prototype));
for (const k of ["open", "_construct", "_write", "_writev", "_destroy", "close", "destroySoon"]) {
  log("same as fs", k, tty.WriteStream.prototype[k] === fs.WriteStream.prototype[k], tty.WriteStream.prototype.hasOwnProperty(k));
}
const w = new tty.WriteStream(1);
log("instance", w instanceof stream.Writable, w instanceof fs.WriteStream, w instanceof stream.Duplex, w.writable, w.readable, w.constructor === tty.WriteStream);
log("instance props", Object.getOwnPropertyNames(w).sort());
const r = new tty.ReadStream(0);
log("rs instance", r instanceof stream.Readable, r instanceof fs.ReadStream, r.readable, Object.getOwnPropertyNames(r).sort());
console.log(out.join("\n"));
process.exit(0);
