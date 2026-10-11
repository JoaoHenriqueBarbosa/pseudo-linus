const cp = require("node:child_process");
const child = cp.spawn("sh", ["-c", "echo from3 >&3; read line <&4; echo got:$line; echo to-stderr >&2"], { stdio: ["ignore", "pipe", "pipe", "pipe", "pipe"] });
console.log("stdio", child.stdio.length, child.stdio.map(s => (s ? s.constructor.name : s)).join(","));
let out = "";
let err = "";
let three = "";
child.stdout.on("data", d => (out += d));
child.stderr.on("data", d => (err += d));
child.stdio[3].on("data", d => (three += d));
child.stdio[4].write("reply\n");
child.on("close", (code, signal) => {
  console.log("close", code, signal, JSON.stringify(out), JSON.stringify(err), JSON.stringify(three));
});
