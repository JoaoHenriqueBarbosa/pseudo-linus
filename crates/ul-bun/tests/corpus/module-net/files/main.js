const net = require("node:net");
const util = require("node:util");
const out = [];
const log = (...a) => out.push(a.map(x => typeof x === "string" ? x : util.inspect(x)).join(" "));
log("exports", Object.keys(net).sort().join(","));
log("isIP", net.isIP("127.0.0.1"), net.isIP("::1"), net.isIP("x"), net.isIPv4("1.2.3.4"), net.isIPv6("::"));
log("autoSelect", net.getDefaultAutoSelectFamily(), net.getDefaultAutoSelectFamilyAttemptTimeout());
const server = net.createServer(socket => {
  log("server connection", socket.remoteAddress, typeof socket.remotePort, socket.localAddress);
  socket.setEncoding("utf8");
  socket.on("data", d => { log("server data", d); socket.end("pong:" + d); });
  socket.on("end", () => log("server end"));
  socket.on("close", hadError => log("server socket close", hadError));
});
server.on("listening", () => log("listening"));
server.listen(0, "127.0.0.1", () => {
  const { port, address, family } = server.address();
  log("address", address, family, port > 0);
  const client = net.connect(port, "127.0.0.1");
  client.setEncoding("utf8");
  client.on("connect", () => { log("client connect", client.connecting, client.remotePort === port); client.write("ping"); });
  client.on("data", d => log("client data", d));
  client.on("end", () => log("client end"));
  client.on("close", hadError => { log("client close", hadError, client.bytesWritten, client.bytesRead); server.close(() => log("server closed")); });
});
const refused = net.connect(1, "127.0.0.1");
refused.on("error", e => log("refused error", e.code, e.message, e.syscall, e.errno, e.address, e.port));
refused.on("close", h => log("refused close", h));
try { net.connect(70000); } catch (e) { log("bad port", e.name, e.code, e.message); }
setTimeout(() => { console.log(out.join("\n")); process.exit(0); }, 500);
