// ---- Valores que atravessam documentos: o que o IndexedDB guarda no perfil sai deste programa como texto e volta
// noutro. Serializa o que o clone estruturado aceita (o valor já é um clone), com a identidade dos objetos repetidos e
// dos ciclos preservada: cada objeto ganha um número na ordem da visita, e a segunda visita vira referência.

const TYPED_ARRAYS = { Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array, Int32Array, Uint32Array, Float32Array, Float64Array, BigInt64Array, BigUint64Array, DataView };
const WRAPPERS = { Boolean, Number, String, BigInt };
const ERROR_TYPES = { Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError };
const bytesToLatin1 = (bytes) => {
  let out = '';
  for (let i = 0; i < bytes.length; i += 0x8000) out += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  return out;
};
const latin1ToBytes = (text) => Uint8Array.from(text, (c) => c.charCodeAt(0));
const specialNumber = (value) => (Number.isNaN(value) ? 'NaN' : value === Infinity ? 'Infinity' : value === -Infinity ? '-Infinity' : Object.is(value, -0) ? '-0' : null);

const serializeValue = (root) => {
  const ids = new Map();
  const visit = (value) => {
    if (value === null || typeof value === 'boolean' || typeof value === 'string') return value;
    if (value === undefined) return ['u'];
    if (typeof value === 'number') {
      const special = specialNumber(value);
      return special === null ? value : ['d', special];
    }
    if (typeof value === 'bigint') return ['i', value.toString()];
    if (ids.has(value)) return ['r', ids.get(value)];
    ids.set(value, ids.size);
    if (value instanceof ArrayBuffer) return ['B', bytesToLatin1(new Uint8Array(value))];
    if (ArrayBuffer.isView(value)) {
      const name = Object.keys(TYPED_ARRAYS).find((key) => value instanceof TYPED_ARRAYS[key]);
      const length = value instanceof DataView ? value.byteLength : value.length;
      return ['V', name, visit(value.buffer), value.byteOffset, length];
    }
    if (value instanceof Date) return ['D', specialNumber(value.getTime()) || value.getTime()];
    if (value instanceof RegExp) return ['R', value.source, value.flags];
    for (const [name, cls] of Object.entries(WRAPPERS)) if (value instanceof cls) return ['W', name, visit(cls.prototype.valueOf.call(value))];
    if (value instanceof Map) return ['M', [...value].map(([k, v]) => [visit(k), visit(v)])];
    if (value instanceof Set) return ['S', [...value].map(visit)];
    if (value instanceof Error) return ['E', value.name in ERROR_TYPES ? value.name : 'Error', visit(Object.prototype.hasOwnProperty.call(value, 'message') ? value.message : undefined), visit(typeof value.stack === 'string' ? value.stack : undefined)];
    if (Array.isArray(value)) {
      const items = [];
      for (const key of Object.keys(value)) items.push([key, visit(value[key])]);
      return ['a', value.length, items];
    }
    const fields = [];
    for (const key of Object.keys(value)) fields.push([key, visit(value[key])]);
    return ['o', fields];
  };
  return JSON.stringify(visit(root));
};

const deserializeValue = (text) => {
  const objects = [];
  const remember = (value) => { objects.push(value); return value; };
  const build = (node) => {
    if (!Array.isArray(node)) return node;
    switch (node[0]) {
      case 'u': return undefined;
      case 'd': return node[1] === '-0' ? -0 : Number(node[1]);
      case 'i': return BigInt(node[1]);
      case 'r': return objects[node[1]];
      case 'B': return remember(latin1ToBytes(node[1]).buffer);
      case 'V': {
        const slot = objects.length;
        objects.push(null);
        const buffer = build(node[2]);
        const view = new TYPED_ARRAYS[node[1]](buffer, node[3], node[4]);
        objects[slot] = view;
        return view;
      }
      case 'D': return remember(new Date(typeof node[1] === 'string' ? Number(node[1]) : node[1]));
      case 'R': return remember(new RegExp(node[1], node[2]));
      case 'W': {
        const slot = objects.length;
        objects.push(null);
        const wrapper = Object(build(node[2]));
        objects[slot] = wrapper;
        return wrapper;
      }
      case 'M': {
        const map = remember(new Map());
        for (const [k, v] of node[1]) map.set(build(k), build(v));
        return map;
      }
      case 'S': {
        const set = remember(new Set());
        for (const v of node[1]) set.add(build(v));
        return set;
      }
      case 'E': {
        // Construída pela classe (o objeto precisa ser erro de verdade), sem a pilha deste ponto.
        const error = remember(new ERROR_TYPES[node[1]]());
        delete error.stack;
        const message = build(node[2]);
        if (message !== undefined) Object.defineProperty(error, 'message', { value: message, writable: true, configurable: true });
        const stack = build(node[3]);
        if (stack !== undefined) Object.defineProperty(error, 'stack', { value: stack, writable: true, configurable: true });
        return error;
      }
      case 'a': {
        const array = remember(new Array(node[1]));
        for (const [key, v] of node[2]) array[key] = build(v);
        return array;
      }
      default: {
        const object = remember({});
        for (const [key, v] of node[1]) object[key] = build(v);
        return object;
      }
    }
  };
  return build(JSON.parse(text));
};
