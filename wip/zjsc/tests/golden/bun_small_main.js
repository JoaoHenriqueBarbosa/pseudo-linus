// Os membros pequenos e puros do global `Bun` contra o bun 1.4.2: a forma de cada um (descritor, `name`, `length`,
// `toString`) e o comportamento em entradas que não dependem da máquina (nada de tempo absoluto nem de caminhos do
// executável). Roda como `/app/main.js`. Texto fora do ASCII sai de `String.fromCodePoint`, para o arquivo ser ASCII.
const out = (...values) => console.log(values.map((value) => (typeof value === "string" ? value : JSON.stringify(value))).join(" "));
const attempt = (thunk) => {
  try {
    return ["ok", thunk()];
  } catch (error) {
    return ["throw", error && error.name, error && error.code, error && error.message];
  }
};
const text = (...points) => String.fromCodePoint(...points);

const members = ["version", "revision", "argv", "env", "main", "sleep", "sleepSync", "inspect", "nanoseconds", "escapeHTML", "peek", "pathToFileURL", "fileURLToPath"];
for (const name of members) {
  const descriptor = Object.getOwnPropertyDescriptor(Bun, name);
  const value = descriptor.value;
  out("descriptor", name, descriptor.writable, descriptor.enumerable, descriptor.configurable, typeof descriptor.get, typeof descriptor.set, typeof value);
  if (typeof value === "function") {
    out("function", name, value.name, value.length, String(value), Object.getOwnPropertyNames(value).join(","), Object.getPrototypeOf(value) === Function.prototype, "prototype" in value);
  }
}
const mainDescriptor = Object.getOwnPropertyDescriptor(Bun, "main");
out("main accessor", mainDescriptor.get.name, mainDescriptor.get.length, mainDescriptor.set.name, mainDescriptor.set.length);
const inspectCustom = Object.getOwnPropertyDescriptor(Bun.inspect, "custom");
const inspectTable = Object.getOwnPropertyDescriptor(Bun.inspect, "table");
out("inspect members", String(inspectCustom.value), inspectCustom.value === Symbol.for("nodejs.util.inspect.custom"), inspectCustom.writable, inspectCustom.enumerable, inspectCustom.configurable);
out("inspect table", inspectTable.writable, inspectTable.enumerable, inspectTable.configurable, inspectTable.value.name, inspectTable.value.length, String(inspectTable.value));
console.log(Bun.inspect.table([{ a: 1, b: "x" }, { a: 3 }]));
out("inspect table edges", Bun.inspect.table(5), Bun.inspect.table("ab"), Bun.inspect.table(), Bun.inspect.table([[1, 2]], ["0"]), Bun.inspect.table([{ a: 1 }], 5));
const peekStatus = Object.getOwnPropertyDescriptor(Bun.peek, "status");
out("peek status", peekStatus.writable, peekStatus.enumerable, peekStatus.configurable, peekStatus.value.name, peekStatus.value.length, String(peekStatus.value));

// Valores.
out("version", Bun.version, Bun.version === process.versions.bun, /^\d+\.\d+\.\d+$/.test(Bun.version));
out("revision", Bun.revision.length, Bun.revision === process.revision, /^[0-9a-f]{40}$/.test(Bun.revision));
out("argv", Array.isArray(Bun.argv), Bun.argv === process.argv, Bun.argv.length, Bun.argv[1]);
out("env", Bun.env === process.env, typeof Bun.env, Object.getPrototypeOf(Bun.env) === Object.prototype);
out("main", Bun.main, Bun.main === Bun.main);
Bun.main = "/outro.js";
out("main set", Bun.main);
Bun.main = 5;
out("main set number", typeof Bun.main, Bun.main);

// nanoseconds: número inteiro, crescente, na mesma base do `performance.now()`.
const first = Bun.nanoseconds();
const now = performance.now();
const second = Bun.nanoseconds();
out("nanoseconds", typeof first, Number.isInteger(first), first >= 0, second >= first, Math.abs(now - first / 1e6) < 1000, attempt(() => typeof Bun.nanoseconds(1)));

// sleepSync bloqueia, e aceita só número. `Infinity` dorme para sempre no bun, por isso fica de fora.
const before = performance.now();
Bun.sleepSync(20);
out("sleepSync", performance.now() - before >= 19, Bun.sleepSync(0));
out("sleepSync no argument", attempt(() => Bun.sleepSync()));
for (const argument of [undefined, null, "10", -1, NaN, -Infinity, 1.5, true, 10n, -0, -0.5, -2147483649]) {
  out("sleepSync arg", String(argument), attempt(() => Bun.sleepSync(argument)));
}

// escapeHTML.
for (const input of ["", "abc", "<a href=\"x\">&'</a>", text(0xe9, 0x3c, 0xe9, 0x3e), "a\u0000b", text(0x1f600, 0x26), "&&&&", 5, true, null, undefined, { toString: () => "<x>" }, [1, "<"]]) {
  out("escapeHTML", String(input), attempt(() => Bun.escapeHTML(input)));
}
out("escapeHTML no argument", attempt(() => Bun.escapeHTML()), attempt(() => Bun.escapeHTML(Symbol("s"))));

// pathToFileURL e fileURLToPath.
for (const input of ["/a/b c", "/a/%20", text(0x2f, 0xe9), "/a#b?c", "/", "/a/../b", "/a/./b/", "relative/x", 5]) {
  out("pathToFileURL", String(input), attempt(() => {
    const url = Bun.pathToFileURL(input);
    return [url instanceof URL, String(input).startsWith("/") ? url.href : url.href.endsWith(String(input))];
  }));
}
for (const input of ["file:///a/b%20c", "file:///%C3%A9", "file://host/x", "file://localhost/y", "http://x/y", "file:///a%2Fb", "file:///a%2fb", "file:///a%25b", "/plain", "not a url", new URL("file:///z"), 5, undefined]) {
  out("fileURLToPath", String(input), attempt(() => Bun.fileURLToPath(input)));
}

// peek(promise) e peek.status.
const resolved = Promise.resolve(7);
const rejected = Promise.reject(new Error("no"));
rejected.catch(() => {});
const pending = new Promise(() => {});
out("peek", Bun.peek(5), Bun.peek(resolved), Bun.peek(rejected) instanceof Error, Bun.peek(pending) === pending, Bun.peek(), Bun.peek.status(resolved), Bun.peek.status(rejected), Bun.peek.status(pending), Bun.peek.status(1), Bun.peek.status());
const thenable = { then() {} };
out("peek thenable", Bun.peek(thenable) === thenable, Bun.peek.status(thenable));

// sleep devolve promessa resolvida com undefined.
const sleeping = Bun.sleep(5);
out("sleep", sleeping instanceof Promise, attempt(() => Bun.sleep("x") instanceof Promise), attempt(() => Bun.sleep() instanceof Promise));
sleeping.then((value) => out("sleep done", String(value)));
// A `Date` fica 50 ms à frente para a ordem não depender de quanto tempo passa entre as duas chamadas.
Bun.sleep(new Date(Date.now() + 50)).then((value) => out("sleep date", String(value)));
Bun.sleep(-5).then((value) => out("sleep negative", String(value)));
