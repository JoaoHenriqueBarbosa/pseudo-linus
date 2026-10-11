const op = __pl_dom;
const evalGlobal = __pl_eval;
const setCodeGuard = __pl_set_code_guard;
delete globalThis.__pl_set_code_guard;
const microtaskCheckpoint = __pl_microtask_checkpoint;
delete globalThis.__pl_microtask_checkpoint;
// O código que o próprio navegador compila de texto (`new Function` do tratador de atributo, expressões do GLSL) não
// passa pela guarda do Trusted Types: só o que a página pede.
let internalCodeDepth = 0;
const internalCode = (body) => {
  internalCodeDepth++;
  try { return body(); } finally { internalCodeDepth--; }
};
const privateSymbol = __pl_private;
delete globalThis.__pl_dom;
delete globalThis.__pl_eval;
delete globalThis.__pl_private;

// O estado interno do navegador vive em símbolos privados do JSC (`P.__nome`, criados sob demanda): invisíveis a
// Reflect.ownKeys, getOwnPropertyNames, `in`, JSON, descritores, structuredClone e às armadilhas de Proxy.
const privateNames = new Map();
const P = new Proxy(Object.create(null), {
  get(_, name) {
    let symbol = privateNames.get(name);
    if (symbol === undefined) privateNames.set(name, (symbol = privateSymbol(name)));
    return symbol;
  },
});
const hidden = (target, key, value) => { target[P[key]] = value; };
// Um nome privado lido num Proxy fica no próprio Proxy, não vai ao alvo nem ao protótipo dele: o objeto do DOM que
// é Proxy (listas vivas, classList, style, dataset, storage) leva consigo o estado e os métodos privados do alvo.
const proxied = (target, handler) => {
  const proxy = new Proxy(target, handler);
  for (const symbol of privateNames.values()) {
    const value = target[symbol];
    if (value !== undefined) proxy[symbol] = value;
  }
  return proxy;
};
const tag = (cls, name) => Object.defineProperty(cls.prototype, Symbol.toStringTag, { value: name, configurable: true });
const ILLEGAL = Symbol('illegal');
const HTML_NS = 'http://www.w3.org/1999/xhtml';
const SVG_NS = 'http://www.w3.org/2000/svg';
const MATHML_NS = 'http://www.w3.org/1998/Math/MathML';

// Identidade dos nós: um objeto JS por NodeId do hospedeiro, sempre o mesmo.
const ids = new WeakMap();
const wrappers = new Map();
const idOf = (node) => {
  const id = ids.get(node);
  if (id === undefined) throw new TypeError("Failed to execute on 'Node': parameter is not of type 'Node'.");
  return id;
};
const isNode = (value) => value !== null && typeof value === 'object' && ids.has(value);

const domException = (message, name) => {
  const error = new DOMException(message, name);
  return error;
};

const toStr = (value) => (value === undefined ? 'undefined' : String(value));
const lower = (value) => String(value).toLowerCase();
