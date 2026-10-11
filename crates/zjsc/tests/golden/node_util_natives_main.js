// Os nativos do `util`: tabela de erros do libuv, `parseEnv`, `inspect` (promessa, proxy, iterador, chaves sem índice) e o
// `_errnoException`. A saída tem de ser a do bun 1.4.2.
const util = require("util");

const show = (label, fn) => {
  try {
    const value = fn();
    console.log(label, typeof value === "string" ? value : util.inspect(value, { depth: 4 }));
  } catch (error) {
    console.log(label, "lançou", error?.name, error?.code, error?.message);
  }
};

show("superfície", () => Object.keys(util).join(","));
show("tamanho", () => Object.keys(util).length);

show("nome ENOENT", () => util.getSystemErrorName(-2));
show("nome EOF", () => util.getSystemErrorName(-4095));
show("nome desconhecido", () => util.getSystemErrorName(-99999));
show("nome positivo", () => util.getSystemErrorName(1));
show("nome texto", () => util.getSystemErrorName("x"));
show("mensagem", () => util.getSystemErrorMessage(-2));
show("mensagem desconhecida", () => util.getSystemErrorMessage(-99999));
show("mapa", () => [...util.getSystemErrorMap()].map(([code, [name, message]]) => `${code}=${name}:${message}`).join(" "));
show("mapa novo a cada chamada", () => util.getSystemErrorMap() !== util.getSystemErrorMap());

show("errno chaves", () => Object.getOwnPropertyNames(util._errnoException(-2, "open", "/x")).join(","));
show("errno mensagem", () => util._errnoException(-2, "open", "/x").message);
show("errno sem original", () => util._errnoException(-13, "read").message);
show("errno constructor", () => util._errnoException(-2, "open").constructor === Error);
show("errno inválido", () => util._errnoException(1, "open"));

show("parseEnv", () => util.parseEnv("a=1\nb=2\na=3"));
show("parseEnv índices", () => util.parseEnv("0=zero\n2023=x\nkey=v"));
show("parseEnv formas", () => util.parseEnv("export A=1\nB: 2\nC='x y'\nD=\"l1\\nl2\"\nE=`bt`\nF=a#b\nG = 7 # c"));
show("parseEnv aspas abertas", () => util.parseEnv("A=\"unterminated\nB=2"));
show("parseEnv objeto String", () => util.parseEnv(new String("a=1")));
show("parseEnv número", () => util.parseEnv(1));
show("parseEnv vazio", () => util.parseEnv());
show("parseEnv utf8", () => util.parseEnv("ok=é\u{1F600}"));
show("parseEnv nome", () => `${util.parseEnv.name}/${util.parseEnv.length}/${String(util.parseEnv)}`);

show("promessa pendente", () => util.inspect(new Promise(() => {})));
show("promessa cumprida", () => util.inspect(Promise.resolve({ a: 1 })));
show("promessa rejeitada", () => {
  const promise = Promise.reject(new Error("x"));
  promise.catch(() => {});
  return util.inspect(promise).split("\n")[0];
});
show("proxy", () => util.inspect(new Proxy({ a: 1 }, {}), { showProxy: true }));
show("proxy revogado", () => {
  const { proxy, revoke } = Proxy.revocable({ a: 1 }, {});
  revoke();
  return util.inspect(proxy, { showProxy: true });
});
show("iterador de mapa", () => util.inspect(new Map([[1, 2], [3, 4]]).entries()));
show("iterador de conjunto", () => util.inspect(new Set([1, 2]).values()));
show("mapa fraco", () => util.inspect(new WeakMap(), { showHidden: true }));
show("array com extras", () => {
  const array = [1, 2, 3];
  array.extra = "e";
  array[Symbol("s")] = 1;
  return util.inspect(array, { showHidden: true });
});
show("linhas", () => util.inspect("a\nb\nc\n"));
show("linhas longas", () => util.inspect({ text: "linha um\nlinha dois\nlinha três\n".repeat(4) }));
show("erro", () => util.inspect(Object.assign(new Error("boom"), { code: "E_X" })).split("\n")[0]);
show("largura", () => util.inspect("日本語", { colors: false }));
show("cores", () => util.inspect({ n: 1, s: "x", b: true, u: undefined, y: Symbol("k") }, { colors: true }));
show("format", () => util.format("%s:%d:%i:%f:%j:%o:%O %%", "a", 1.5, 2.7, "3.5", { x: 1 }, [1], { y: 2 }));
show("stripVTControlCharacters", () => util.stripVTControlCharacters("\u001b[31mred\u001b[0m"));
show("largura via inspect", () => util.inspect(["日本語abc", "😀"], { breakLength: 12 }));
