// Gera tests/golden/import_no_host_bun.tsv: o que `import()` rejeita num script avulso (sem host de módulos),
// medido no bun 1.4.2 rodando o programa como arquivo. Cobre nome, mensagem, classe, String(e), code,
// specifier, referrer, importKind, instanceof Error, chaves próprias, JSON.stringify e o toStringTag.
// O diretório temporário vira "/" no referrer e na mensagem (`/main.js`), sem caminho do home no golden.
// Uso: bun scripts/gen-import-no-host-golden.js > tests/golden/import_no_host_bun.tsv
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const BUN = process.env.BUN || "bun";

const specifiers = [
  "./nao-existe.js",
  "../acima/nao-existe.js",
  "./sem-extensao",
  "./dir/",
  "./dados.json",
  "/abs/nao-existe.js",
  "file:///abs/nao-existe.js",
  "lodashx",
  "@escopo/pacote",
  "pacote/sub/caminho",
  "node:xyz",
  "node:fs/nao-existe",
  "",
  // sintaxe de caminho UNC: o bun lê como arquivo e rejeita com BuildMessage (ENOENT; EISDIR para a raiz)
  "//x",
  "//",
  "//a/b",
  "///",
  "//x.js",
  "///x",
];

const describe = `(e) => JSON.stringify([
  e && e.name, e && e.message, e && e.constructor && e.constructor.name, String(e), e && e.code,
  e && e.specifier, e && e.referrer, e && e.importKind, e instanceof Error,
  Object.keys(e), Object.getOwnPropertyNames(e), JSON.stringify(e),
  Object.prototype.toString.call(e), e && e.constructor === globalThis.ResolveMessage,
  typeof e.stack, e.level, e.position, e.line, e.column,
])`;

// o specificador literal de import() o transpilador do bun resolve na hora; atrás de uma função ele fica para a execução
const PRELUDE = 'const I = (s) => import(s); const I2 = (s) => import(s, { with: { type: "json" } }); ';
const programs = [];
for (const spec of specifiers) {
  const lit = JSON.stringify(spec);
  programs.push(`I(${lit}).catch((e) => { globalThis.R = (${describe})(e); });`);
  // com atributos e dentro de função assíncrona
  programs.push(`(async () => { try { await I(${lit}); globalThis.R = 'ok'; } catch (e) { globalThis.R = (${describe})(e); } })();`);
}
programs.push(`I2("./a.js").catch((e) => { globalThis.R = (${describe})(e); });`);
// Segundo argumento de import(): as opções inválidas rejeitam com o TypeError nativo do JSC (sem `code`, protótipo do
// próprio TypeError, chaves próprias message/originalLine/.../stack), antes de resolver o specifier; as válidas
// (inclusive `type` que o bun aceita: css, toml, file, text, sqlite, e a chave antiga `assert`) seguem para a
// resolução e rejeitam com o ResolveMessage. Medido igual para script e módulo no bun 1.4.2.
// A posição (line/column) do erro nativo é a do fonte transpilado do bun: fica fora do golden.
const noPos = describe.replace(", e.level, e.position, e.line, e.column,", ", e.level, e.position,");
const optionCases = [
  "1", "null", '"x"', "true", "[]", "(function () {})", "{}", "undefined",
  "{ with: 1 }", "{ with: null }", '{ with: "json" }', "{ with: [] }", "{ with: undefined }", "{ with: {} }",
  "{ with: { type: 1 } }", "{ with: { type: null } }", "{ with: { type: Symbol() } }", "{ with: { type: undefined } }",
  '{ with: { type: "json", x: 1 } }', '{ with: { x: 1 } }', '{ with: { x: "a", type: 1 } }', '{ with: { type: "" } }',
  '{ with: { type: "css" } }', '{ with: { type: "toml" } }', '{ with: { type: "text" } }', '{ with: { type: "file" } }',
  '{ with: { type: "sqlite" } }', '{ with: { type: "javascript" } }', '{ with: { type: "json" } }', '{ with: { foo: "bar" } }',
  '{ assert: { type: "json" } }', "{ assert: 1 }", "{ assert: null }",
  '{ with: { type: "json" }, assert: 1 }', "{ with: 1, assert: { type: 'json' } }",
  '{ get with() { throw new RangeError("getter"); } }', '{ with: { get type() { throw new RangeError("getter"); } } }',
  '{ with: new Proxy({}, { ownKeys() { throw new RangeError("trap"); } }) }',
  '{ with: Object.create({ type: 1 }) }', '{ with: Object.defineProperty({}, "type", { value: 1, enumerable: false }) }',
  '{ with: { [Symbol("s")]: 1 } }', '{ with: { type: new String("json") } }', '{ with: { type: 1n } }',
];
for (const opts of optionCases) {
  const d = `(e) => JSON.stringify([(${noPos})(e), e && e.stack && String(e.stack).split("\\n")[0]])`;
  programs.push(`const IO = (s, o) => import(s, o); IO("./nao-existe.js", ${opts}).catch((e) => { globalThis.R = (${d})(e); });`);
  programs.push(`const IO = (s, o) => import(s, o); (async () => { try { await IO("./nao-existe.js", ${opts}); globalThis.R = 'ok'; } catch (e) { globalThis.R = (${d})(e); } })();`);
}
// a ordem: o specifier é convertido antes das opções, ou depois?
for (const spec of ["Symbol()", "{ toString() { throw new RangeError('spec'); } }", "undefined"]) {
  // (não usar "null", "1" nem nome de pacote que exista no npm: o bun instala sozinho e o resultado vira "ok")
  programs.push(`const IO = (s, o) => import(s, o); IO(${spec}, 1).catch((e) => { globalThis.R = (${noPos})(e); });`);
  programs.push(`const IO = (s, o) => import(s, o); IO(${spec}, { with: { type: "css" } }).then(() => { globalThis.R = JSON.stringify('ok'); }, (e) => { globalThis.R = (${noPos})(e); });`);
}
// duas rejeições seguidas no mesmo realm: a classe e a identidade da instância se mantêm
programs.push(
  `Promise.all(["./a.js", "lodashx"].map((s) => I(s).catch((e) => e))).then(([a, b]) => { globalThis.R = JSON.stringify([a.constructor === b.constructor, a instanceof ResolveMessage, Object.getPrototypeOf(a) === ResolveMessage.prototype, a !== b]); });`
);
// o import() devolve uma promessa rejeitada, nunca lança de forma síncrona
programs.push(`let p; try { p = ["./a.js"].map(I)[0]; } catch (e) { p = 'throw'; } globalThis.R = JSON.stringify([typeof p, p instanceof Promise]); p.catch?.(() => {});`);

const classStart = programs.length;
// A classe ResolveMessage existe desde o início do programa, nativa (fiel ao bun: funções com [native code]).
const R_CLASS = `JSON.stringify([
  typeof ResolveMessage, ResolveMessage.toString(), ResolveMessage.length, ResolveMessage.name,
  Object.getPrototypeOf(ResolveMessage) === Function.prototype, Object.keys(globalThis).includes('ResolveMessage'),
  (({ writable, enumerable, configurable }) => [writable, enumerable, configurable])(Object.getOwnPropertyDescriptor(globalThis, 'ResolveMessage')),
  Object.getOwnPropertyDescriptor(globalThis, 'ResolveMessage').value === ResolveMessage,
  Object.getOwnPropertyNames(ResolveMessage), Object.getOwnPropertySymbols(ResolveMessage).length,
  Object.getOwnPropertyNames(ResolveMessage).map((k) => { const d = Object.getOwnPropertyDescriptor(ResolveMessage, k); return [k, typeof d.value, d.writable, d.enumerable, d.configurable]; }),
  Object.getPrototypeOf(ResolveMessage.prototype) === Error.prototype, ResolveMessage.prototype.constructor === ResolveMessage,
  Object.prototype.toString.call(ResolveMessage.prototype),
])`;
programs.push(`globalThis.R = ${R_CLASS};`);
programs.push(
  `globalThis.R = JSON.stringify([Object.getOwnPropertyNames(ResolveMessage.prototype), Object.getOwnPropertySymbols(ResolveMessage.prototype).map(String), Object.getOwnPropertyNames(ResolveMessage.prototype).map((k) => { const d = Object.getOwnPropertyDescriptor(ResolveMessage.prototype, k); const f = d.get || d.value; const isFn = typeof f === 'function'; return [k, !!d.get, !!d.set, isFn ? f.toString() : typeof d.value, isFn ? f.name : null, isFn ? f.length : null, d.writable, d.enumerable, d.configurable]; }), ['toPrimitive', 'toStringTag'].map((n) => { const d = Object.getOwnPropertyDescriptor(ResolveMessage.prototype, Symbol[n]); const isFn = typeof d.value === 'function'; return [n, isFn ? d.value.toString() : d.value, isFn ? d.value.name : null, isFn ? d.value.length : null, d.writable, d.enumerable, d.configurable]; })]);`
);
programs.push(`let a, b; try { new ResolveMessage(); a = 'ok'; } catch (e) { a = [e.name, e.message, e.constructor.name, e.code]; } try { ResolveMessage(); b = 'ok'; } catch (e) { b = [e.name, e.message, e.code]; } globalThis.R = JSON.stringify([a, b]);`);
// this inválido: getters, toString, toJSON e toPrimitive
programs.push(
  `const out = []; const P = ResolveMessage.prototype; const g = Object.getOwnPropertyDescriptor(P, 'code').get; const subjects = [undefined, null, 1, 'a', true, {}, [], function foo() {}, new (class Foo {})(), 5n]; for (const t of subjects) { for (const f of [() => g.call(t), () => P.toString.call(t), () => P.toJSON.call(t), () => P[Symbol.toPrimitive].call(t, 'string')]) { try { f(); out.push('ok'); } catch (e) { out.push([e.name, e.message, e.code]); } } } try { P.message; out.push('ok'); } catch (e) { out.push([e.name, e.message, e.code]); } globalThis.R = JSON.stringify(out);`
);
// setters de message e stack: guardam o valor sem mudar o texto original; setter com this inválido lança
programs.push(
  `I("./nope.js").catch((e) => { const P = Object.getPrototypeOf(e); const out = []; for (const k of ['message', 'stack']) { const d = Object.getOwnPropertyDescriptor(P, k); out.push([d.set.toString(), d.set.name, d.set.length]); } e.message = 'X'; e.stack = 'Y'; out.push([e.message, e.stack, Object.keys(e), Object.getOwnPropertyNames(e), String(e), e.toJSON().message]); try { Object.getOwnPropertyDescriptor(P, 'message').set.call({}, 1); } catch (x) { out.push([x.name, x.message, x.code]); } try { Object.getOwnPropertyDescriptor(P, 'stack').set.call(undefined, 1); } catch (x) { out.push([x.name, x.message, x.code]); } try { e.code = 'c'; out.push(e.code); } catch (x) { out.push(x.message); } globalThis.R = JSON.stringify(out); });`
);
// numa instância real: toPrimitive com cada dica, toJSON, stack, chaves
programs.push(
  `I("./nope.js").catch((e) => { const P = Object.getPrototypeOf(e); const out = []; for (const h of ['string', 'default', 'number', 'bogus', undefined]) out.push(e[Symbol.toPrimitive](h)); out.push(String(e), e + '', \`\${e}\`, e.stack, e.requireStack, Object.keys(e), Object.keys(e.toJSON()), e.toJSON(), P === ResolveMessage.prototype); const names = []; for (const k in e) names.push(k); out.push(names); globalThis.R = JSON.stringify(out); });`
);

// Descritores completos (valor, writable, enumerable, configurable, getter, setter, com name/length/toString das
// funções) de todas as chaves próprias do construtor e do protótipo, na ordem do bun.
const FULL_DESCRIPTORS =
  "const F = (f) => typeof f === 'function' ? [f.name, f.length, f.toString()] : f === undefined ? 'undefined' : typeof f; " +
  "const V = (v) => typeof v === 'function' ? F(v) : typeof v === 'object' && v !== null ? 'object' : typeof v === 'symbol' ? String(v) : v; " +
  "const X = (o) => Reflect.ownKeys(o).map((k) => { const d = Object.getOwnPropertyDescriptor(o, k); return [String(k), 'value' in d ? V(d.value) : 'accessor', d.writable, d.enumerable, d.configurable, F(d.get), F(d.set)]; }); ";
programs.push(`${FULL_DESCRIPTORS}globalThis.R = JSON.stringify([X(ResolveMessage), X(ResolveMessage.prototype)]);`);

// A BuildMessage (leitura de arquivo do bun) tem a mesma estrutura nativa: os mesmos programas de classe, protótipo,
// construtor, `this` inválido, setters e instância, medidos sobre ela (a instância vem de `import("//x")`).
for (const program of programs.slice(classStart)) programs.push(program.replaceAll("ResolveMessage", "BuildMessage").replaceAll("./nope.js", "//x").replaceAll("'code'", "'level'").replaceAll("['message', 'stack']", "['message']").replaceAll("try { Object.getOwnPropertyDescriptor(P, 'stack').set.call(undefined, 1); } catch (x) { out.push([x.name, x.message, x.code]); } ", ""));

const rows = [];
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "no-host-"));
try {
  for (const body of programs) {
    const program = PRELUDE + body;
    const file = path.join(dir, "main.js");
    fs.writeFileSync(file, `${program}\nsetTimeout(() => console.log(JSON.stringify(globalThis.R)), 50);\n`);
    const run = spawnSync(BUN, [file], { cwd: dir, encoding: "utf8" });
    if (!run.stdout) {
      process.stderr.write(`falhou: ${program}\n${run.stderr}\n`);
      process.exit(1);
    }
    if (run.stdout.trim() === "undefined") { process.stderr.write(`sem R: ${program}\n${run.stderr}\n`); process.exit(1); }
    const value = JSON.parse(run.stdout.trim());
    const clean = (text) => text.split(dir + "/main.js").join("/main.js").split(dir + "/").join("/");
    rows.push(`${JSON.stringify(program)}\t${JSON.stringify(clean(String(value)))}`);
  }
} finally {
  fs.rmSync(dir, { recursive: true, force: true });
}
process.stdout.write(rows.join("\n") + "\n");
