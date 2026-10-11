// `util.styleText` com `validateStream` carrega `internal/streams/utils` pelo registro interno (`host.requireInternal`):
// o `isNodeStream`, o `isReadableStream` e o `isWritableStream` decidem se o `stream` serve, e o texto sai pintado ou
// não conforme `shouldColorize`. A saída tem de ser a do bun 1.4.2 do oráculo.
const util = require("util");
const stream = require("stream");

function run(label, fn) {
  try {
    const value = fn();
    console.log(label, "ok", JSON.stringify(value));
  } catch (error) {
    console.log(label, error.name, error.code, error.message);
  }
}

const sink = new stream.Writable({ write(chunk, encoding, callback) { callback(); } });
const source = new stream.Readable({ read() {} });
const duplex = new stream.PassThrough();

run("stdout", () => typeof util.styleText("red", "x", { validateStream: true, stream: process.stdout }));
run("writable", () => util.styleText("red", "x", { validateStream: true, stream: sink }));
run("readable", () => util.styleText("red", "x", { validateStream: true, stream: source }));
run("duplex", () => util.styleText("red", "x", { validateStream: true, stream: duplex }));
run("web-readable", () => util.styleText("red", "x", { validateStream: true, stream: new ReadableStream() }));
run("web-writable", () => util.styleText("red", "x", { validateStream: true, stream: new WritableStream() }));
run("web-transform", () => util.styleText("red", "x", { validateStream: true, stream: new TransformStream() }));
run("object", () => util.styleText("red", "x", { validateStream: true, stream: {} }));
run("number", () => util.styleText("red", "x", { validateStream: true, stream: 1 }));
run("null", () => util.styleText("red", "x", { validateStream: true, stream: null }));
run("sem-validar", () => util.styleText("red", "x", { validateStream: false, stream: {} }));

// O objeto exportado de `stream` não carrega nada do mecanismo.
console.log(Object.keys(stream).join(","));
console.log(Object.getOwnPropertyNames(stream).length, Object.getOwnPropertySymbols(stream).length);
