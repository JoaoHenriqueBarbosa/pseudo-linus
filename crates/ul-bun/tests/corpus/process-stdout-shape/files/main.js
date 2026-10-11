
const tty = require("node:tty");
const stream = require("node:stream");
const out = [];
const log = (...a) => out.push(a.map(x => typeof x === "string" ? x : Bun.inspect(x)).join(" "));
const chain = (o) => { const names = []; for (let p = o; p; p = Object.getPrototypeOf(p)) names.push(p.constructor === Object && p !== Object.prototype ? "<obj>" : (typeof p === "function" ? "fn:" + p.name : p.constructor?.name ?? "null")); return names; };
log("process.stdout", process.stdout.constructor.name, process.stdout.isTTY, process.stdout.fd, process.stderr.constructor.name, process.stdin.constructor.name, process.stdin.isTTY);
log("getColorDepth", typeof process.stdout.getColorDepth, typeof process.stdout.hasColors, typeof process.stdout.columns);
log("process.stdout chain", chain(Object.getPrototypeOf(process.stdout)), process.stdout instanceof stream.Writable);
log("process.stdout props", Object.getOwnPropertyNames(process.stdout).sort());
log("process.stdin chain", chain(Object.getPrototypeOf(process.stdin)));
console.log(out.join("\n"));
process.exit(0);
