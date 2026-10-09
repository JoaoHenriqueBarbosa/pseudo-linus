// Gera tests/golden/require-builtin.tsv rodando no bun (o oráculo): a forma do registro de módulos embutidos.
// Linhas "tipo<TAB>chave<TAB>valor": `names` (a lista exata de `require("module").builtinModules`, JSON), `identity`
// (os pares nu/`node:` devolvem o mesmo objeto), `namespace` (as chaves do namespace de `import("node:X")` e se o
// `default` é o objeto do `require`), `error` (nome, código e mensagem de `require` e `import()` que falham).
// Roda com `--no-install`, para um nome inexistente não baixar pacote do npm.
// Uso: bun --no-install scripts/gen-require-builtin-golden.js > tests/golden/require-builtin.tsv

const out = [];
const row = (kind, key, value) => out.push([kind, key, value].join("\t"));
const tidy = (text) => JSON.stringify(String(text));

const builtinModules = require("module").builtinModules;
row("names", "builtinModules", JSON.stringify(builtinModules));

for (const name of ["vm", "module", "buffer", "path", "os", "fs", "fs/promises", "path/posix", "util", "events"]) {
    row("identity", name, String(require(name) === require("node:" + name)));
}
row("identity", "builtinModules", String(require("module").builtinModules === require("node:module").builtinModules));

// Forma do node:os (tipos, chaves, erros); os valores dependem da máquina e não entram no golden.
{
    const os = require("os");
    row("os", "keys", JSON.stringify(Object.keys(os)));
    row("os", "functions", JSON.stringify(Object.keys(os).filter((k) => typeof os[k] === "function").map((k) => [k, os[k].name, os[k].length])));
    row("os", "types", JSON.stringify({
        EOL: os.EOL, devNull: os.devNull, arch: os.arch(), platform: os.platform(), endianness: os.endianness(), machine: os.machine(),
        type: os.type(), hostname: typeof os.hostname(), release: typeof os.release(), version: typeof os.version(),
        homedir: typeof os.homedir(), tmpdir: typeof os.tmpdir(), totalmem: typeof os.totalmem(), freemem: typeof os.freemem(),
        uptime: typeof os.uptime(), loadavg: [Array.isArray(os.loadavg()), os.loadavg().length], cpus: Array.isArray(os.cpus()),
        parallelism: Number.isInteger(os.availableParallelism()), networkInterfaces: typeof os.networkInterfaces(),
    }));
    row("os", "userInfo", JSON.stringify([Object.keys(os.userInfo()), typeof os.userInfo().uid, typeof os.userInfo().username]));
    row("os", "cpu-shape", JSON.stringify(os.cpus().slice(0, 1).map((c) => [Object.keys(c).filter((k) => k !== "toJSON"), Object.keys(c.times)])));
    row("os", "constants", JSON.stringify([
        Object.keys(os.constants), Object.keys(os.constants.priority), Object.keys(os.constants.dlopen),
        Object.keys(os.constants.signals).length, Object.keys(os.constants.errno).length,
        Object.getPrototypeOf(os.constants), Object.getPrototypeOf(os.constants.signals), os.constants.signals.SIGCHLD,
        os.constants.priority, os.constants.dlopen,
    ]));
    row("os", "descriptor-constants", JSON.stringify(Object.getOwnPropertyDescriptor(os, "constants")).replace(/"value":\{.*\},/, '"value":"...",'));
    const caught = (fn) => { try { return String(fn()); } catch (e) { return [e.name, e.code, e.message].join(" | "); } };
    row("os", "setPriority()", caught(() => os.setPriority()));
    row("os", "setPriority('a')", caught(() => os.setPriority("a")));
    row("os", "setPriority(1,'x')", caught(() => os.setPriority(1, "x")));
    row("os", "setPriority(0,99)", caught(() => os.setPriority(0, 99)));
    row("os", "setPriority(0,-21)", caught(() => os.setPriority(0, -21)));
    row("os", "getPriority('a')", caught(() => os.getPriority("a")));
    row("os", "getPriority(-1)", caught(() => os.getPriority(-1)));
    row("os", "getPriority(99999999)", caught(() => os.getPriority(99999999)));
    row("os", "userInfo(1)", caught(() => os.userInfo(1)));
    // Formas que não dependem dos valores da máquina: EOL acessor, toJSON dos cpus, info do SystemError, etiqueta.
    const eol = Object.getOwnPropertyDescriptor(os, "EOL");
    row("os", "EOL-descriptor", JSON.stringify([Object.keys(eol), eol.get.name, eol.get.length, eol.set, eol.enumerable, eol.configurable]));
    row("os", "toString-tag", Object.prototype.toString.call(os));
    const first = os.cpus()[0];
    row("os", "cpu-toJSON", JSON.stringify([Object.keys(first), first.toJSON.name, first.toJSON.length, Object.keys(first.toJSON()), Object.keys(JSON.parse(JSON.stringify(first)))]));
    row("os", "userInfo-buffer", JSON.stringify([typeof os.userInfo({ encoding: "buffer" }).homedir, Object.keys(os.userInfo({ encoding: "buffer" }))]));
    try {
        os.getPriority(99999999);
    } catch (e) {
        const info = Object.getOwnPropertyDescriptor(e, "info");
        row("os", "getPriority-info", JSON.stringify([Object.getOwnPropertyNames(e).slice(0, 6), e.info, info.enumerable, info.writable, info.configurable, e.errno, e.syscall]));
    }
    row("os", "availableParallelism", String(os.availableParallelism() === require("fs").readFileSync("/proc/self/status", "utf8").match(/Cpus_allowed_list:\s*(\S+)/)[1].split(",").reduce((n, p) => { const [a, b] = p.split("-").map(Number); return n + (b === undefined ? 1 : b - a + 1); }, 0)));
    row("os", "networkInterfaces-shape", JSON.stringify(Object.values(os.networkInterfaces()).flat().map((i) => Object.keys(i).join(",")).filter((v, i, a) => a.indexOf(v) === i)));
    row("os", "networkInterfaces-lo", JSON.stringify(os.networkInterfaces().lo));
}
row("identity", "test", (() => { try { require("test"); return "loaded"; } catch (error) { return error.code; } })());
row("identity", "node:test", (() => { try { return typeof require("node:test"); } catch (error) { return error.code; } })());

const failures = ["zz", "node:zz", "node:", "bun:zz", "bun:", "bun:test:x"];
for (const request of failures) {
    try {
        require(request);
        row("error", "require " + request, "loaded");
    } catch (error) {
        row("error", "require " + request, [error.name, error.code, tidy(String(error.message).split("\n")[0])].join(" "));
    }
}

const imported = ["node:path", "node:os", "node:vm", "node:module", "path", "node:fs/promises", "node:buffer"];
const pending = imported.map(async (specifier) => {
    const namespace = await import(specifier);
    const required = require(specifier);
    row("namespace", specifier, JSON.stringify({
        keys: Object.keys(namespace),
        defaultIsRequire: namespace.default === required,
        namedAreRequire: Object.keys(required).every((key) => key === "default" || namespace[key] === required[key]),
    }));
});
for (const specifier of ["node:zz", "bun:zz", "zz"]) {
    pending.push(
        import(specifier).then(
            () => row("error", "import " + specifier, "loaded"),
            (error) => row("error", "import " + specifier, [error.name, error.code, tidy(String(error.message).split("\n")[0])].join(" ")),
        ),
    );
}

Promise.all(pending).then(() => {
    const head = out.filter((line) => !line.startsWith("namespace\t") && !/^error\timport /.test(line));
    const tail = out.filter((line) => line.startsWith("namespace\t") || /^error\timport /.test(line)).sort();
    console.log([...head, ...tail].join("\n"));
});
