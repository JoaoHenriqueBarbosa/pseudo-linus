const p = Bun.spawn(["echo", "hi"]);
console.log(Object.getOwnPropertyNames(Object.getPrototypeOf(p)).join(","));
console.log(Object.keys(p).join(","), typeof p.pid, p.killed, p.exitCode, p.signalCode, p.stdin, p.stdout.constructor.name, p.stderr);
console.log(await new Response(p.stdout).text(), await p.exited, p.exitCode, p.signalCode, p.killed);
console.log(Object.prototype.toString.call(p), p.constructor.name);
const s = Bun.spawnSync(["sh", "-c", "echo out; echo err >&2; exit 3"]);
console.log(Object.keys(s).join(","), s.exitCode, s.success, s.signalCode, s.stdout.constructor.name, String(s.stdout), String(s.stderr), typeof s.pid);
try { Bun.spawn(["nope-xyz"]); } catch (e) { console.log(e.name, e.code, e.errno, e.syscall, e.path, e.message); }
try { Bun.spawnSync(["nope-xyz"]); } catch (e) { console.log(e.name, e.code, e.errno, e.syscall, e.path, e.message); }
try { Bun.spawn([]); } catch (e) { console.log(e.name, e.code, e.message); }
try { Bun.spawn(); } catch (e) { console.log(e.name, e.code, e.message); }
const q = Bun.spawn(["sleep", "5"]);
q.kill();
console.log(await q.exited, q.signalCode, q.killed, q.exitCode);
const r = Bun.spawn(["cat"], { stdin: "pipe", stdout: "pipe" });
r.stdin.write("abc");
r.stdin.end();
console.log(await new Response(r.stdout).text(), await r.exited);
console.log(Object.getOwnPropertyNames(Object.getPrototypeOf(r.stdin)).join(","), r.stdin.constructor.name);
const t = Bun.spawn(["sh", "-c", "exit 7"], { onExit(proc, code, sig, err) { console.log("onExit", code, sig, err, proc === t); } });
await t.exited;
const names = Object.getOwnPropertyNames(Bun);
console.log(names.slice(names.indexOf("sleepSync"), names.indexOf("sleepSync") + 4).join(","));
