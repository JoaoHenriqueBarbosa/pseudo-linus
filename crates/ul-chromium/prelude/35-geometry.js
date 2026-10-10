// ---- Geometry Interfaces: DOMPoint, DOMRect, DOMQuad e DOMMatrix, com o estado em nomes privados e os membros
// como acessores no protótipo, como no Blink. A matriz é a 4x4 da especificação; `is2D` segue as regras dela.

const num64 = (v) => Number(v);
// Refaz as propriedades do protótipo na ordem dada (a do Blink, que getOwnPropertyNames revela).
function reorderPrototype(cls, names) {
  for (const name of names) {
    const d = Object.getOwnPropertyDescriptor(cls.prototype, name);
    if (!d) continue;
    delete cls.prototype[name];
    Object.defineProperty(cls.prototype, name, d);
  }
}
class DOMPointReadOnly {
  constructor(x = 0, y = 0, z = 0, w = 1) { hidden(this, '__p', [num64(x), num64(y), num64(z), num64(w)]); }
  static fromPoint(other = {}) { return new this(other.x ?? 0, other.y ?? 0, other.z ?? 0, other.w ?? 1); }
  get x() { return this[P.__p][0]; }
  get y() { return this[P.__p][1]; }
  get z() { return this[P.__p][2]; }
  get w() { return this[P.__p][3]; }
  matrixTransform(matrix = {}) { return matrixFrom(matrix).transformPoint(this); }
  toJSON() { return { x: this.x, y: this.y, z: this.z, w: this.w }; }
}
tag(DOMPointReadOnly, 'DOMPointReadOnly');
class DOMPoint extends DOMPointReadOnly {
  get x() { return this[P.__p][0]; } set x(v) { this[P.__p][0] = num64(v); }
  get y() { return this[P.__p][1]; } set y(v) { this[P.__p][1] = num64(v); }
  get z() { return this[P.__p][2]; } set z(v) { this[P.__p][2] = num64(v); }
  get w() { return this[P.__p][3]; } set w(v) { this[P.__p][3] = num64(v); }
}
tag(DOMPoint, 'DOMPoint');

class DOMRectReadOnly {
  constructor(x = 0, y = 0, width = 0, height = 0) { hidden(this, '__r', [num64(x), num64(y), num64(width), num64(height)]); }
  static fromRect(other = {}) { return new this(other.x ?? 0, other.y ?? 0, other.width ?? 0, other.height ?? 0); }
  get x() { return this[P.__r][0]; }
  get y() { return this[P.__r][1]; }
  get width() { return this[P.__r][2]; }
  get height() { return this[P.__r][3]; }
  get top() { const [, y, , h] = this[P.__r]; return Math.min(y, y + h); }
  get right() { const [x, , w] = this[P.__r]; return Math.max(x, x + w); }
  get bottom() { const [, y, , h] = this[P.__r]; return Math.max(y, y + h); }
  get left() { const [x, , w] = this[P.__r]; return Math.min(x, x + w); }
  toJSON() { return { x: this.x, y: this.y, width: this.width, height: this.height, top: this.top, right: this.right, bottom: this.bottom, left: this.left }; }
}
tag(DOMRectReadOnly, 'DOMRectReadOnly');
class DOMRect extends DOMRectReadOnly {
  get x() { return this[P.__r][0]; } set x(v) { this[P.__r][0] = num64(v); }
  get y() { return this[P.__r][1]; } set y(v) { this[P.__r][1] = num64(v); }
  get width() { return this[P.__r][2]; } set width(v) { this[P.__r][2] = num64(v); }
  get height() { return this[P.__r][3]; } set height(v) { this[P.__r][3] = num64(v); }
}
tag(DOMRect, 'DOMRect');

class DOMQuad {
  constructor(p1 = {}, p2 = {}, p3 = {}, p4 = {}) { hidden(this, '__q', [p1, p2, p3, p4].map((p) => DOMPoint.fromPoint(p))); }
  static fromRect(r = {}) { const x = r.x ?? 0, y = r.y ?? 0, w = r.width ?? 0, h = r.height ?? 0; return new DOMQuad({ x, y }, { x: x + w, y }, { x: x + w, y: y + h }, { x, y: y + h }); }
  static fromQuad(q = {}) { return new DOMQuad(q.p1, q.p2, q.p3, q.p4); }
  get p1() { return this[P.__q][0]; }
  get p2() { return this[P.__q][1]; }
  get p3() { return this[P.__q][2]; }
  get p4() { return this[P.__q][3]; }
  getBounds() {
    const xs = this[P.__q].map((p) => p.x), ys = this[P.__q].map((p) => p.y);
    const left = Math.min(...xs), top = Math.min(...ys);
    return new DOMRect(left, top, Math.max(...xs) - left, Math.max(...ys) - top);
  }
  toJSON() { return { p1: this.p1, p2: this.p2, p3: this.p3, p4: this.p4 }; }
}
tag(DOMQuad, 'DOMQuad');

// A matriz em ordem de coluna (m11 m12 m13 m14 m21 ...), como o Float64Array de toFloat64Array.
const IDENTITY = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
const matrixState = (m) => m[P.__m];
const makeMatrix = (cls, values, is2D) => { const m = Object.create(cls.prototype); hidden(m, '__m', { v: values.slice(), is2D }); return m; };
// a·b, as duas em ordem de coluna.
const multiply4 = (a, b) => {
  const out = new Array(16);
  for (let col = 0; col < 4; col++) for (let row = 0; row < 4; row++) {
    let sum = 0;
    for (let k = 0; k < 4; k++) sum += a[k * 4 + row] * b[col * 4 + k];
    out[col * 4 + row] = sum;
  }
  return out;
};
const invert4 = (m) => {
  const inv = new Array(16);
  inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
  inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
  inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
  inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
  inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
  inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
  inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
  inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
  inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
  inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
  inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
  inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
  inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
  inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
  inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
  inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
  const det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
  if (det === 0 || !Number.isFinite(det)) return null;
  return inv.map((x) => x / det);
};
const matrix2D = (a, b, c, d, e, f) => [a, b, 0, 0, c, d, 0, 0, 0, 0, 1, 0, e, f, 0, 1];
const DEG = Math.PI / 180;
const rotationMatrix = (x, y, z, angleDeg) => {
  const len = Math.hypot(x, y, z);
  if (len === 0) return IDENTITY.slice();
  [x, y, z] = [x / len, y / len, z / len];
  const a = angleDeg * DEG, s = Math.sin(a / 2), sc = s * Math.cos(a / 2), sq = s * s;
  // A fórmula da especificação (CSS Transforms 2, rotate3d).
  return [
    1 - 2 * (y * y + z * z) * sq, 2 * (x * y * sq + z * sc), 2 * (x * z * sq - y * sc), 0,
    2 * (x * y * sq - z * sc), 1 - 2 * (x * x + z * z) * sq, 2 * (y * z * sq + x * sc), 0,
    2 * (x * z * sq + y * sc), 2 * (y * z * sq - x * sc), 1 - 2 * (x * x + y * y) * sq, 0,
    0, 0, 0, 1,
  ];
};
// Lista de transformações CSS (`matrix(...) translate(...)`) para a matriz, como no construtor por string.
const parseTransformList = (text) => {
  const source = String(text).trim();
  if (source === '' || source === 'none') return { v: IDENTITY.slice(), is2D: true };
  let m = IDENTITY.slice();
  let is2D = true;
  const re = /\s*([a-zA-Z0-9]+)\(([^)]*)\)\s*/y;
  let pos = 0;
  const syntax = () => { throw domException(`Failed to construct 'DOMMatrix': Failed to parse '${text}'.`, 'SyntaxError'); };
  const angle = (v) => { const r = /^(-?[\d.]+(?:e[+-]?\d+)?)(deg|rad|grad|turn)?$/i.exec(v.trim()); if (!r) syntax(); const n = Number(r[1]); switch ((r[2] || '').toLowerCase()) { case 'rad': return n / DEG; case 'grad': return n * 0.9; case 'turn': return n * 360; case 'deg': return n; default: if (n !== 0) syntax(); return 0; } };
  const length = (v) => { const r = /^(-?[\d.]+(?:e[+-]?\d+)?)(px)?$/i.exec(v.trim()); if (!r || (!r[2] && Number(r[1]) !== 0)) syntax(); return Number(r[1]); };
  const number = (v) => { const n = Number(v.trim()); if (v.trim() === '' || !Number.isFinite(n)) syntax(); return n; };
  while (pos < source.length) {
    re.lastIndex = pos;
    const found = re.exec(source);
    if (!found) syntax();
    pos = re.lastIndex;
    const name = found[1].toLowerCase();
    const args = found[2].split(',');
    let t;
    switch (name) {
      case 'matrix': if (args.length !== 6) syntax(); t = matrix2D(...args.map(number)); break;
      case 'matrix3d': if (args.length !== 16) syntax(); t = args.map(number); is2D = false; break;
      case 'translate': t = matrix2D(1, 0, 0, 1, length(args[0]), args[1] ? length(args[1]) : 0); break;
      case 'translatex': t = matrix2D(1, 0, 0, 1, length(args[0]), 0); break;
      case 'translatey': t = matrix2D(1, 0, 0, 1, 0, length(args[0])); break;
      case 'translatez': t = IDENTITY.slice(); t[14] = length(args[0]); is2D = false; break;
      case 'translate3d': t = IDENTITY.slice(); t[12] = length(args[0]); t[13] = length(args[1]); t[14] = length(args[2]); is2D = false; break;
      case 'scale': { const sx = number(args[0]); t = matrix2D(sx, 0, 0, args[1] ? number(args[1]) : sx, 0, 0); break; }
      case 'scalex': t = matrix2D(number(args[0]), 0, 0, 1, 0, 0); break;
      case 'scaley': t = matrix2D(1, 0, 0, number(args[0]), 0, 0); break;
      case 'scalez': t = IDENTITY.slice(); t[10] = number(args[0]); is2D = false; break;
      case 'scale3d': t = IDENTITY.slice(); t[0] = number(args[0]); t[5] = number(args[1]); t[10] = number(args[2]); is2D = false; break;
      case 'rotate': case 'rotatez': t = rotationMatrix(0, 0, 1, angle(args[0])); if (name === 'rotatez') is2D = false; break;
      case 'rotatex': t = rotationMatrix(1, 0, 0, angle(args[0])); is2D = false; break;
      case 'rotatey': t = rotationMatrix(0, 1, 0, angle(args[0])); is2D = false; break;
      case 'rotate3d': t = rotationMatrix(number(args[0]), number(args[1]), number(args[2]), angle(args[3])); is2D = false; break;
      case 'skew': t = matrix2D(1, Math.tan((args[1] ? angle(args[1]) : 0) * DEG), Math.tan(angle(args[0]) * DEG), 1, 0, 0); break;
      case 'skewx': t = matrix2D(1, 0, Math.tan(angle(args[0]) * DEG), 1, 0, 0); break;
      case 'skewy': t = matrix2D(1, Math.tan(angle(args[0]) * DEG), 0, 1, 0, 0); break;
      case 'perspective': { const d = length(args[0]); t = IDENTITY.slice(); if (d !== 0) t[11] = -1 / d; is2D = false; break; }
      default: syntax();
    }
    m = multiply4(m, t);
  }
  return { v: m, is2D };
};
// O dicionário DOMMatrixInit validado e completado (a especificação "validate and fixup").
const matrixFromInit = (init) => {
  if (init === undefined || init === null) return { v: IDENTITY.slice(), is2D: true };
  const pick = (a, b) => { const x = init[a], y = init[b]; if (x !== undefined && y !== undefined && !Object.is(Number(x), Number(y)) && !(Number.isNaN(Number(x)) && Number.isNaN(Number(y)))) throw new TypeError(`Failed to execute 'fromMatrix' on 'DOMMatrixReadOnly': The '${a}' property should equal the '${b}' property.`); return x !== undefined ? Number(x) : y !== undefined ? Number(y) : undefined; };
  const a = pick('a', 'm11') ?? 1, b = pick('b', 'm12') ?? 0, c = pick('c', 'm21') ?? 0, d = pick('d', 'm22') ?? 1, e = pick('e', 'm41') ?? 0, f = pick('f', 'm42') ?? 0;
  const z = (k, def) => (init[k] === undefined ? def : Number(init[k]));
  const third = [z('m13', 0), z('m14', 0), z('m23', 0), z('m24', 0), z('m31', 0), z('m32', 0), z('m34', 0), z('m43', 0)];
  const m33 = z('m33', 1), m44 = z('m44', 1);
  const flat = third.every((x) => x === 0) && m33 === 1 && m44 === 1;
  if (init.is2D === true && !flat) throw new TypeError("Failed to execute 'fromMatrix' on 'DOMMatrixReadOnly': The is2D member is set to true but the input matrix is a 3d matrix.");
  const is2D = init.is2D === undefined ? flat : Boolean(init.is2D);
  return { v: [a, b, third[0], third[1], c, d, third[2], third[3], third[4], third[5], m33, third[6], e, f, third[7], m44], is2D };
};
const matrixFrom = (value) => (value instanceof DOMMatrixReadOnly ? value : DOMMatrix.fromMatrix(value));
const matrixArgs = (cls, init) => {
  if (init === undefined) return { v: IDENTITY.slice(), is2D: true };
  if (typeof init === 'string') return parseTransformList(init);
  const list = Array.from(init, Number);
  if (list.length === 6) return { v: matrix2D(...list), is2D: true };
  if (list.length === 16) return { v: list, is2D: false };
  throw new TypeError(`Failed to construct '${cls}': The sequence must contain 6 elements for a 2D matrix or 16 elements for a 3D matrix.`);
};
const formatNumber = (n) => String(n);
class DOMMatrixReadOnly {
  constructor(init) { hidden(this, '__m', matrixArgs(new.target === DOMMatrix ? 'DOMMatrix' : 'DOMMatrixReadOnly', init)); }
  static fromMatrix(other) { const s = matrixFromInit(other); return makeMatrix(this, s.v, s.is2D); }
  static fromFloat32Array(array) { return new this(Array.from(array)); }
  static fromFloat64Array(array) { return new this(Array.from(array)); }
  get is2D() { return matrixState(this).is2D; }
  get isIdentity() { return matrixState(this).v.every((x, i) => x === IDENTITY[i]); }
  flipX() { return this.multiply(new DOMMatrix([-1, 0, 0, 1, 0, 0])); }
  flipY() { return this.multiply(new DOMMatrix([1, 0, 0, -1, 0, 0])); }
  inverse() { return DOMMatrix.fromMatrix(this).invertSelf(); }
  multiply(other) { return DOMMatrix.fromMatrix(this).multiplySelf(other); }
  rotate(x, y, z) { return DOMMatrix.fromMatrix(this).rotateSelf(x, y, z); }
  rotateAxisAngle(x, y, z, angle) { return DOMMatrix.fromMatrix(this).rotateAxisAngleSelf(x, y, z, angle); }
  rotateFromVector(x, y) { return DOMMatrix.fromMatrix(this).rotateFromVectorSelf(x, y); }
  scale(sx, sy, sz, ox, oy, oz) { return DOMMatrix.fromMatrix(this).scaleSelf(sx, sy, sz, ox, oy, oz); }
  scale3d(s, ox, oy, oz) { return DOMMatrix.fromMatrix(this).scale3dSelf(s, ox, oy, oz); }
  scaleNonUniform(sx = 1, sy = 1) { return DOMMatrix.fromMatrix(this).scaleSelf(sx, sy, 1, 0, 0, 0); }
  skewX(sx) { return DOMMatrix.fromMatrix(this).skewXSelf(sx); }
  skewY(sy) { return DOMMatrix.fromMatrix(this).skewYSelf(sy); }
  translate(tx, ty, tz) { return DOMMatrix.fromMatrix(this).translateSelf(tx, ty, tz); }
  toFloat32Array() { return new Float32Array(matrixState(this).v); }
  toFloat64Array() { return new Float64Array(matrixState(this).v); }
  transformPoint(point = {}) {
    const p = point instanceof DOMPointReadOnly ? point : DOMPoint.fromPoint(point);
    const m = matrixState(this).v;
    const [x, y, z, w] = [p.x, p.y, p.z, p.w];
    return new DOMPoint(m[0] * x + m[4] * y + m[8] * z + m[12] * w, m[1] * x + m[5] * y + m[9] * z + m[13] * w, m[2] * x + m[6] * y + m[10] * z + m[14] * w, m[3] * x + m[7] * y + m[11] * z + m[15] * w);
  }
  toJSON() { const o = {}; for (const k of MATRIX_FIELDS) o[k] = this[k]; o.is2D = this.is2D; o.isIdentity = this.isIdentity; return o; }
  toString() {
    const s = matrixState(this);
    if (!s.v.every(Number.isFinite)) throw domException("Failed to execute 'toString' on 'DOMMatrixReadOnly': Cannot be serialized with NaN or Infinity values.", 'InvalidStateError');
    if (s.is2D) return `matrix(${[s.v[0], s.v[1], s.v[4], s.v[5], s.v[12], s.v[13]].map(formatNumber).join(', ')})`;
    return `matrix3d(${s.v.map(formatNumber).join(', ')})`;
  }
}
const MATRIX_FIELDS = ['a', 'b', 'c', 'd', 'e', 'f', 'm11', 'm12', 'm13', 'm14', 'm21', 'm22', 'm23', 'm24', 'm31', 'm32', 'm33', 'm34', 'm41', 'm42', 'm43', 'm44'];
const FIELD_INDEX = { a: 0, b: 1, c: 4, d: 5, e: 12, f: 13, m11: 0, m12: 1, m13: 2, m14: 3, m21: 4, m22: 5, m23: 6, m24: 7, m31: 8, m32: 9, m33: 10, m34: 11, m41: 12, m42: 13, m43: 14, m44: 15 };
// Os campos que tiram a matriz do 2D quando recebem algo diferente do valor da identidade.
const THREE_D_FIELDS = { m13: 0, m14: 0, m23: 0, m24: 0, m31: 0, m32: 0, m33: 1, m34: 0, m43: 0, m44: 1 };
for (const k of MATRIX_FIELDS) Object.defineProperty(DOMMatrixReadOnly.prototype, k, { get() { return matrixState(this).v[FIELD_INDEX[k]]; }, enumerable: true, configurable: true });
// A ordem dos membros no protótipo é a medida no oráculo: campos, is2D/isIdentity, depois os métodos.
reorderPrototype(DOMMatrixReadOnly, [...MATRIX_FIELDS, 'is2D', 'isIdentity', 'flipX', 'flipY', 'inverse', 'multiply', 'rotate', 'rotateAxisAngle', 'rotateFromVector', 'scale', 'scale3d', 'scaleNonUniform', 'skewX', 'skewY', 'toFloat32Array', 'toFloat64Array', 'toJSON', 'transformPoint', 'translate', 'toString']);
tag(DOMMatrixReadOnly, 'DOMMatrixReadOnly');
class DOMMatrix extends DOMMatrixReadOnly {
  invertSelf() {
    const s = matrixState(this);
    const inv = invert4(s.v);
    if (!inv) { s.v = new Array(16).fill(NaN); s.is2D = false; return this; }
    s.v = inv;
    return this;
  }
  multiplySelf(other) { const o = matrixState(matrixFrom(other)); const s = matrixState(this); s.v = multiply4(s.v, o.v); s.is2D = s.is2D && o.is2D; return this; }
  preMultiplySelf(other) { const o = matrixState(matrixFrom(other)); const s = matrixState(this); s.v = multiply4(o.v, s.v); s.is2D = s.is2D && o.is2D; return this; }
  translateSelf(tx = 0, ty = 0, tz = 0) {
    const t = IDENTITY.slice(); t[12] = Number(tx); t[13] = Number(ty); t[14] = Number(tz);
    const s = matrixState(this); s.v = multiply4(s.v, t); if (Number(tz) !== 0) s.is2D = false;
    return this;
  }
  scaleSelf(sx = 1, sy, sz = 1, ox = 0, oy = 0, oz = 0) {
    sx = Number(sx); sy = sy === undefined ? sx : Number(sy); sz = Number(sz);
    this.translateSelf(ox, oy, oz);
    const t = IDENTITY.slice(); t[0] = sx; t[5] = sy; t[10] = sz;
    const s = matrixState(this); s.v = multiply4(s.v, t); if (sz !== 1 || Number(oz) !== 0) s.is2D = false;
    return this.translateSelf(-ox, -oy, -oz);
  }
  scale3dSelf(scale = 1, ox = 0, oy = 0, oz = 0) { return this.scaleSelf(scale, scale, scale, ox, oy, oz); }
  rotateSelf(rx = 0, ry, rz) {
    if (ry === undefined && rz === undefined) { rz = rx; rx = 0; ry = 0; }
    ry = ry === undefined ? 0 : Number(ry); rz = rz === undefined ? 0 : Number(rz); rx = Number(rx);
    const s = matrixState(this);
    if (rz) s.v = multiply4(s.v, rotationMatrix(0, 0, 1, rz));
    if (ry) s.v = multiply4(s.v, rotationMatrix(0, 1, 0, ry));
    if (rx) s.v = multiply4(s.v, rotationMatrix(1, 0, 0, rx));
    if (rx || ry) s.is2D = false;
    return this;
  }
  rotateFromVectorSelf(x = 0, y = 0) { const angle = x === 0 && y === 0 ? 0 : Math.atan2(y, x) / DEG; const s = matrixState(this); s.v = multiply4(s.v, rotationMatrix(0, 0, 1, angle)); return this; }
  rotateAxisAngleSelf(x = 0, y = 0, z = 0, angle = 0) { const s = matrixState(this); s.v = multiply4(s.v, rotationMatrix(Number(x), Number(y), Number(z), Number(angle))); if (x || y) s.is2D = false; return this; }
  skewXSelf(sx = 0) { const s = matrixState(this); s.v = multiply4(s.v, matrix2D(1, 0, Math.tan(sx * DEG), 1, 0, 0)); return this; }
  skewYSelf(sy = 0) { const s = matrixState(this); s.v = multiply4(s.v, matrix2D(1, Math.tan(sy * DEG), 0, 1, 0, 0)); return this; }
  setMatrixValue(text) { hidden(this, '__m', parseTransformList(text)); return this; }
}
for (const k of MATRIX_FIELDS) Object.defineProperty(DOMMatrix.prototype, k, {
  get() { return matrixState(this).v[FIELD_INDEX[k]]; },
  set(v) { const s = matrixState(this); s.v[FIELD_INDEX[k]] = Number(v); if (k in THREE_D_FIELDS && Number(v) !== THREE_D_FIELDS[k]) s.is2D = false; },
  enumerable: true, configurable: true,
});
reorderPrototype(DOMMatrix, [...MATRIX_FIELDS, 'invertSelf', 'multiplySelf', 'preMultiplySelf', 'rotateAxisAngleSelf', 'rotateFromVectorSelf', 'rotateSelf', 'scale3dSelf', 'scaleSelf', 'skewXSelf', 'skewYSelf', 'translateSelf', 'setMatrixValue']);
tag(DOMMatrix, 'DOMMatrix');
// Os construtores do Blink não declaram parâmetros obrigatórios (length 0).
for (const cls of [DOMPointReadOnly, DOMRectReadOnly, DOMQuad, DOMMatrixReadOnly]) Object.defineProperty(cls, 'length', { value: 0 });
