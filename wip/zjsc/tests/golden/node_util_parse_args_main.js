const { parseArgs } = require("util");

function show(name, fn) {
  try {
    const result = fn();
    console.log(name, "=>", JSON.stringify(result));
  } catch (e) {
    console.log(name, "!!", e.name, e.code, JSON.stringify(e.message), Object.keys(e).join(","));
  }
}

const opts = {
  foo: { type: "boolean", short: "f" },
  bar: { type: "string", short: "b" },
  baz: { type: "string", multiple: true },
  flag: { type: "boolean", multiple: true, short: "g" },
  x: { type: "boolean" },
};

// Valores e posicionais.
show("empty", () => parseArgs({ args: [] }));
show("undefined config uses argv", () => {
  const saved = process.argv;
  process.argv = ["bun", "script", "-q"];
  try {
    return parseArgs();
  } finally {
    process.argv = saved;
  }
});
show("bool", () => parseArgs({ args: ["--foo"], options: opts }));
show("short bool", () => parseArgs({ args: ["-f"], options: opts }));
show("string next", () => parseArgs({ args: ["--bar", "v"], options: opts }));
show("string inline", () => parseArgs({ args: ["--bar=v=w"], options: opts }));
show("short string next", () => parseArgs({ args: ["-b", "v"], options: opts }));
show("short string attached", () => parseArgs({ args: ["-bvalue"], options: opts }));
show("group", () => parseArgs({ args: ["-fg"], options: opts }));
show("group string last", () => parseArgs({ args: ["-fb", "v"], options: opts }));
show("group string middle", () => parseArgs({ args: ["-fbvalue"], options: opts }));
show("multiple strings", () => parseArgs({ args: ["--baz", "a", "--baz=b", "--baz", "c"], options: opts }));
show("multiple bools", () => parseArgs({ args: ["-g", "--flag", "-g"], options: opts }));
show("terminator", () => parseArgs({ args: ["--foo", "--", "--bar", "-f", "p"], options: opts, allowPositionals: true }));
show("positionals", () => parseArgs({ args: ["a", "--foo", "b"], options: opts, allowPositionals: true }));
show("non strict unknown", () => parseArgs({ args: ["--nope", "-z", "-abc", "--q=1", "p"], strict: false }));
show("non strict allows positionals", () => parseArgs({ args: ["p", "q"], strict: false }));
show("non-string args", () => parseArgs({ args: [1, 2], allowPositionals: true }));
show("proto key", () => parseArgs({ args: ["--__proto__=1", "--__proto__"], strict: false }));
show("proto default", () => parseArgs({ args: [], options: { __proto__x: { type: "string", default: "d" } } }));
show("index key", () => parseArgs({ args: ["--0=a", "--2023"], strict: false }));
show("values proto null", () => Object.getPrototypeOf(parseArgs({ args: [] }).values) === null);
show("result keys", () => Object.keys(parseArgs({ args: [], tokens: true })));

// Defaults.
show("defaults", () =>
  parseArgs({
    args: ["--foo"],
    options: {
      foo: { type: "boolean", default: false },
      bar: { type: "string", default: "d" },
      baz: { type: "string", multiple: true, default: ["x", "y"] },
      flag: { type: "boolean", multiple: true, default: [true] },
      none: { type: "string", default: undefined },
    },
  }));

// Negativo.
show("negative", () => parseArgs({ args: ["--no-foo", "--no-bar", "--no-unknown"], options: opts, allowNegative: true, strict: false }));
show("negative off", () => parseArgs({ args: ["--no-foo"], options: opts, strict: false }));
show("negative string", () => parseArgs({ args: ["--no-bar"], options: opts, allowNegative: true }));
show("negative bool strict", () => parseArgs({ args: ["--no-foo"], options: opts, allowNegative: true }));
show("negative unknown strict", () => parseArgs({ args: ["--no-unknown"], options: opts, allowNegative: true }));

// Tokens.
show("tokens", () =>
  parseArgs({
    args: ["-fb", "v", "--baz=1", "--x", "pos", "--", "-f"],
    options: opts,
    allowPositionals: true,
    tokens: true,
  }));
show("tokens group", () => parseArgs({ args: ["-fgbval"], options: opts, tokens: true }));
show("tokens non strict", () => parseArgs({ args: ["-z", "--q=1", "--r"], strict: false, tokens: true }));
show("tokens negative", () => parseArgs({ args: ["--no-foo"], options: opts, allowNegative: true, tokens: true }));

// Erros de uso.
show("unknown long", () => parseArgs({ args: ["--nope"], options: opts }));
show("unknown short", () => parseArgs({ args: ["-z"], options: opts }));
show("unknown short in group", () => parseArgs({ args: ["-fz"], options: opts }));
show("unknown with positionals", () => parseArgs({ args: ["--nope"], options: opts, allowPositionals: true }));
show("unknown inline", () => parseArgs({ args: ["--nope=1"], options: opts }));
show("string missing", () => parseArgs({ args: ["--bar"], options: opts }));
show("short string missing", () => parseArgs({ args: ["-b"], options: opts }));
show("bool with value", () => parseArgs({ args: ["--foo=1"], options: opts }));
show("short bool with value", () => parseArgs({ args: ["-f=1"], options: opts }));
show("ambiguous long", () => parseArgs({ args: ["--bar", "--foo"], options: opts }));
show("ambiguous short", () => parseArgs({ args: ["-b", "-f"], options: opts }));
show("ambiguous short group", () => parseArgs({ args: ["-fb", "-f"], options: opts }));
show("unexpected positional", () => parseArgs({ args: ["a"], options: opts }));
show("unexpected after terminator", () => parseArgs({ args: ["--", "a"], options: opts }));
show("unexpected number", () => parseArgs({ args: [7] }));

// Validação da configuração.
show("config string", () => parseArgs("x"));
show("config number", () => parseArgs(5));
show("config null", () => parseArgs(null));
show("args not array", () => parseArgs({ args: "x" }));
show("args null", () => parseArgs({ args: null, strict: false }));
show("args object", () => parseArgs({ args: {} }));
show("strict not bool", () => parseArgs({ args: [], strict: 1 }));
show("strict null", () => parseArgs({ args: [], strict: null }));
show("allowPositionals not bool", () => parseArgs({ args: [], allowPositionals: "x" }));
show("tokens not bool", () => parseArgs({ args: [], tokens: 0 }));
show("allowNegative not bool", () => parseArgs({ args: [], allowNegative: [] }));
show("options null", () => parseArgs({ args: [], options: null }));
show("options string", () => parseArgs({ args: [], options: "x" }));
show("options array", () => parseArgs({ args: [], options: [] }));
show("option not object", () => parseArgs({ args: [], options: { a: 1 } }));
show("option null", () => parseArgs({ args: [], options: { a: null } }));
show("option array", () => parseArgs({ args: [], options: { a: [] } }));
show("option function", () => parseArgs({ args: [], options: { a: function () {} } }));
show("type missing", () => parseArgs({ args: [], options: { a: {} } }));
show("type bad", () => parseArgs({ args: [], options: { a: { type: "number" } } }));
show("type number", () => parseArgs({ args: [], options: { a: { type: 1 } } }));
show("type symbol", () => parseArgs({ args: [], options: { a: { type: Symbol("s") } } }));
show("short not string", () => parseArgs({ args: [], options: { a: { type: "string", short: 1 } } }));
show("short undefined", () => parseArgs({ args: [], options: { a: { type: "string", short: undefined } } }));
show("short long", () => parseArgs({ args: [], options: { a: { type: "string", short: "ab" } } }));
show("short empty", () => parseArgs({ args: [], options: { a: { type: "string", short: "" } } }));
show("multiple not bool", () => parseArgs({ args: [], options: { a: { type: "string", multiple: 1 } } }));
show("default string bad", () => parseArgs({ args: [], options: { a: { type: "string", default: 1 } } }));
show("default bool bad", () => parseArgs({ args: [], options: { a: { type: "boolean", default: "x" } } }));
show("default multiple string bad", () => parseArgs({ args: [], options: { a: { type: "string", multiple: true, default: "x" } } }));
show("default multiple string item", () => parseArgs({ args: [], options: { a: { type: "string", multiple: true, default: ["a", 1] } } }));
show("default multiple bool bad", () => parseArgs({ args: [], options: { a: { type: "boolean", multiple: true, default: [true, "x"] } } }));
show("default null", () => parseArgs({ args: [], options: { a: { type: "string", default: null } } }));
show("long option name", () => parseArgs({ args: [], options: { "a.b": { type: "string", short: "xy" } } }));

// Acessores, protótipo e ordem de leitura.
show("getter order", () => {
  const log = [];
  const config = {
    get args() { log.push("args"); return []; },
    get strict() { log.push("strict"); return true; },
    get allowPositionals() { log.push("allowPositionals"); return false; },
    get tokens() { log.push("tokens"); return false; },
    get allowNegative() { log.push("allowNegative"); return false; },
    get options() { log.push("options"); return {}; },
  };
  parseArgs(config);
  return log;
});
show("inherited config ignored", () => parseArgs(Object.create({ strict: false, args: ["--q"] })));
show("own option def only", () => parseArgs({ args: ["--a=1"], options: { a: Object.create({ type: "string" }) } }));
show("sparse args", () => parseArgs({ args: ["-f", , "--foo"].slice(0, 3), options: opts, strict: false }));
show("function name", () => [parseArgs.name, parseArgs.length]);
show("error stack top", () => {
  try {
    parseArgs({ args: ["--nope"] });
  } catch (e) {
    return e.stack.split("\n").slice(0, 2).map(line => line.replace(/\(.*\)$/, "()"));
  }
});
