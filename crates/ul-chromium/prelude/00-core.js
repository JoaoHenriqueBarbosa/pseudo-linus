const op = __pl_dom;
const evalGlobal = __pl_eval;
delete globalThis.__pl_dom;
delete globalThis.__pl_eval;

const hidden = (target, key, value) => Object.defineProperty(target, key, { value, writable: true, configurable: true, enumerable: false });
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
