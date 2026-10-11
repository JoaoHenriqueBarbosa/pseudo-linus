const out = [];
const log = (...a) => out.push(a.map(x => typeof x === "string" ? x : Bun.inspect(x)).join(" "));
const t = (label, fn) => { try { log(label, fn()); } catch (e) { log(label, "throws", e.name, e.code, e.message); } };
t("listen type", () => [typeof Bun.listen, Bun.listen.name, Bun.listen.length, typeof Bun.connect, Bun.connect.name, Bun.connect.length]);
t("listen noargs", () => Bun.listen());
t("listen empty", () => Bun.listen({}));
t("listen nosocket", () => Bun.listen({ hostname: "127.0.0.1", port: 0 }));
t("listen nohost", () => Bun.listen({ port: 0, socket: {} }));
t("listen badport", () => Bun.listen({ hostname: "127.0.0.1", port: -1, socket: { data() {} } }));
t("connect noargs", () => Bun.connect());
t("connect nosocket", () => Bun.connect({ hostname: "127.0.0.1", port: 1 }));
t("connect empty", () => Bun.connect({}));
const server = Bun.listen({
  hostname: "127.0.0.1", port: 0, data: { tag: "srv" },
  socket: {
    open(s) { log("srv open this", this === s, typeof s.remoteAddress, s.data, s.readyState, s.localPort > 0, Object.getOwnPropertyNames(Object.getPrototypeOf(s)).sort().join(",")); },
    data(s, d) { log("srv data", Bun.inspect(d), d.constructor.name); s.write("pong:" + d.toString()); s.end(); },
    drain(s) { log("srv drain"); },
    close(s, err) { log("srv close", err, s.listener, s.data); },
    end(s) { log("srv end"); },
    error(s, e) { log("srv error", e.message); },
    handshake() { log("srv handshake"); },
  },
});
log("listener", Object.getOwnPropertyNames(Object.getPrototypeOf(server)).sort().join(","), server.hostname, server.port > 0, server.data, server.constructor.name);
log("listener keys", Object.keys(server));
const port = server.port;
Bun.connect({
  hostname: "127.0.0.1", port, data: { tag: "cli" },
  socket: {
    open(s) { log("cli open this", this === s, s.data, s.readyState, Object.getOwnPropertyNames(Object.getPrototypeOf(s)).sort().join(","), s.constructor.name, Object.keys(s)); log("write ret", s.write("ping")); },
    data(s, d) { log("cli data", d.toString()); },
    close(s, err) { log("cli close", err); server.stop(); log("stopped"); },
    end(s) { log("cli end"); },
    error(s, e) { log("cli error", e.message); },
    connectError(s, e) { log("cli connectError", e.message); },
    drain() { log("cli drain"); },
  },
}).then(s => { log("connect resolved", s.constructor.name, typeof s.write); });
Bun.connect({ hostname: "127.0.0.1", port: 1, socket: { data() {}, open() { log("refused open"); }, connectError(s, e) { log("refused connectError", e.name, e.code, e.message, e.errno, e.syscall); }, close() { log("refused close"); }, error(s, e) { log("refused error", e.message); } } }).then(() => log("refused resolved"), e => log("refused rejected", e.name, e.code, e.message));
setTimeout(() => { console.log(out.join("\n")); process.exit(0); }, 400);
