// ---- GLSL ES (1.00 e 3.00) executável: pré-processador, análise sintática, tipos estáticos e compilação para
// fechamentos. A página nunca vê isto; o rasterizador do WebGL roda `main` de cada estágio por vértice e por fragmento.
// Toda aritmética de `float` passa por `Math.fround` (o SwiftShader calcula em binário32). Valores: `float`, `int` e
// `uint` são números, `bool` é booleano, vetores e matrizes (por colunas) são arrays planos, estruturas são objetos,
// arrays de GLSL são arrays de valores e um sampler é o número da unidade de textura.

class GlslCompileError extends Error {}
const glslFail = (message) => { throw new GlslCompileError(message); };

const fr = Math.fround;
const GLSL_KINDS = { float: 'float', int: 'int', uint: 'uint', bool: 'bool' };
const glslKind = (t) => {
  const m = /^(?:(b|i|u)?vec|mat)/.exec(t);
  if (GLSL_KINDS[t]) return t;
  if (m) return m[1] === 'b' ? 'bool' : m[1] === 'i' ? 'int' : m[1] === 'u' ? 'uint' : 'float';
  return null;
};
const glslDims = (t) => {
  let m = /^[biu]?vec([234])$/.exec(t);
  if (m) return { cols: 1, rows: Number(m[1]), n: Number(m[1]), vec: true, mat: false };
  m = /^mat([234])(?:x([234]))?$/.exec(t);
  if (m) { const c = Number(m[1]), r = Number(m[2] || m[1]); return { cols: c, rows: r, n: c * r, vec: false, mat: true }; }
  if (GLSL_KINDS[t]) return { cols: 1, rows: 1, n: 1, vec: false, mat: false, scalar: true };
  return null;
};
const glslVecType = (kind, n) => (n === 1 ? kind : (kind === 'float' ? '' : kind === 'int' ? 'i' : kind === 'uint' ? 'u' : 'b') + 'vec' + n);
const glslArrayOf = (t) => { const m = /^(.*)\[(\d+)\]$/.exec(t); return m ? { base: m[1], len: Number(m[2]) } : null; };
const GLSL_SAMPLER = /^[iu]?sampler(2D|3D|Cube|2DArray|2DShadow|CubeShadow|2DArrayShadow|ExternalOES)$/;

const toInt = (x) => { x = typeof x === 'boolean' ? (x ? 1 : 0) : x; return Number.isNaN(x) ? 0 : x >= 2147483647 ? 2147483647 : x <= -2147483648 ? -2147483648 : Math.trunc(x) | 0; };
const toUint = (x) => { x = typeof x === 'boolean' ? (x ? 1 : 0) : x; return Number.isNaN(x) ? 0 : x >= 4294967295 ? 4294967295 : x <= 0 ? (x > -2147483649 && x < 0 ? Math.trunc(x) >>> 0 : 0) : Math.trunc(x) >>> 0; };
const GLSL_CONV = { float: (x) => fr(typeof x === 'boolean' ? (x ? 1 : 0) : x), int: toInt, uint: toUint, bool: (x) => x !== 0 && x !== false };

const glslZero = (t, structs) => {
  const arr = glslArrayOf(t);
  if (arr) return Array.from({ length: arr.len }, () => glslZero(arr.base, structs));
  const d = glslDims(t);
  if (d) { const z = t === 'bool' ? false : 0; return d.scalar ? z : new Array(d.n).fill(z.constructor === Boolean ? false : 0); }
  if (GLSL_SAMPLER.test(t)) return 0;
  const st = structs.get(t);
  if (st) { const o = {}; for (const f of st) o[f.name] = glslZero(f.t, structs); return o; }
  return 0;
};
const glslClone = (v) => (Array.isArray(v) ? v.map(glslClone) : v && typeof v === 'object' ? Object.fromEntries(Object.entries(v).map(([k, x]) => [k, glslClone(x)])) : v);
const glslEqual = (a, b) => (Array.isArray(a) ? a.every((x, i) => glslEqual(x, b[i])) : a && typeof a === 'object' ? Object.keys(a).every((k) => glslEqual(a[k], b[k])) : a === b);

// ---- Pré-processador.
const glslPreprocess = (source, defines) => {
  const macros = new Map(Object.entries(defines));
  const text = source.replace(/\\\r?\n/g, '').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' ')).replace(/\/\/[^\n]*/g, '');
  const expand = (line, depth = 0) => {
    if (depth > 12) return line;
    let changed = false;
    const out = line.replace(/[A-Za-z_][A-Za-z0-9_]*/g, (name, offset) => {
      const m = macros.get(name);
      if (m === undefined || m.expanding) return name;
      if (m.params) {
        let i = offset + name.length;
        while (line[i] === ' ') i++;
        if (line[i] !== '(') return name;
        return name;
      }
      changed = true;
      return m.body;
    });
    // As macros com parâmetros: troca `NOME(args)` pelo corpo com os argumentos no lugar.
    let result = out;
    for (const [name, m] of macros) {
      if (!m.params) continue;
      const re = new RegExp('\\b' + name + '\\s*\\(');
      for (let guard = 0; guard < 100; guard++) {
        const hit = re.exec(result);
        if (!hit) break;
        let i = hit.index + hit[0].length, level = 1, start = i;
        const args = [];
        while (i < result.length && level > 0) {
          const c = result[i];
          if (c === '(') level++; else if (c === ')') { level--; if (level === 0) break; } else if (c === ',' && level === 1) { args.push(result.slice(start, i)); start = i + 1; }
          i++;
        }
        args.push(result.slice(start, i));
        let body = m.body;
        m.params.forEach((p, k) => { body = body.replace(new RegExp('\\b' + p + '\\b', 'g'), (args[k] || '').trim()); });
        result = result.slice(0, hit.index) + body + result.slice(i + 1);
        changed = true;
      }
    }
    return changed ? expand(result, depth + 1) : result;
  };
  const evalCond = (expr) => {
    const replaced = expand(expr.replace(/defined\s*\(\s*(\w+)\s*\)|defined\s+(\w+)/g, (_, a, b) => (macros.has(a || b) ? '1' : '0'))).replace(/[A-Za-z_]\w*/g, '0');
    try { return internalCode(() => Function('"use strict"; return (' + replaced + ');'))() !== 0; } catch (error) { return false; }
  };
  const lines = text.split('\n');
  const out = [];
  const stack = [];
  const active = () => stack.every((s) => s.on);
  let version = 100;
  for (const raw of lines) {
    const dir = /^\s*#\s*(\w+)\s*(.*)$/.exec(raw);
    if (!dir) { out.push(active() ? expand(raw) : ''); continue; }
    const [, name, rest] = dir;
    if (name === 'if') { const on = active() && evalCond(rest); stack.push({ on, taken: on, parent: active() }); } else if (name === 'ifdef') { const on = active() && macros.has(rest.trim()); stack.push({ on, taken: on }); } else if (name === 'ifndef') { const on = active() && !macros.has(rest.trim()); stack.push({ on, taken: on }); } else if (name === 'elif') {
      const top = stack[stack.length - 1];
      const parent = stack.slice(0, -1).every((s) => s.on);
      top.on = parent && !top.taken && evalCond(rest);
      if (top.on) top.taken = true;
    } else if (name === 'else') {
      const top = stack[stack.length - 1];
      const parent = stack.slice(0, -1).every((s) => s.on);
      top.on = parent && !top.taken;
      top.taken = true;
    } else if (name === 'endif') stack.pop();
    else if (active()) {
      if (name === 'define') {
        const m = /^(\w+)(\(([^)]*)\))?\s*(.*)$/.exec(rest);
        if (m) macros.set(m[1], { body: m[4], params: m[2] ? m[3].split(',').map((x) => x.trim()).filter(Boolean) : null });
      } else if (name === 'undef') macros.delete(rest.trim());
      else if (name === 'version') version = Number(/\d+/.exec(rest)[0]);
    }
    out.push('');
  }
  return { text: out.join('\n'), version };
};

// ---- Léxico.
const glslLex = (text) => {
  const tokens = [];
  const re = /\s+|([A-Za-z_][A-Za-z0-9_]*)|(0[xX][0-9A-Fa-f]+[uU]?|(?:\d+\.\d*|\.\d+)(?:[eE][+-]?\d+)?[fF]?|\d+[eE][+-]?\d+[fF]?|\d+[uU]?[fF]?)|(<<=|>>=|\+\+|--|<<|>>|<=|>=|==|!=|&&|\|\||\^\^|\+=|-=|\*=|\/=|%=|&=|\^=|\|=|.)/gy;
  let m;
  while (re.lastIndex < text.length && (m = re.exec(text)) !== null) {
    if (m[1]) tokens.push({ k: 'id', v: m[1] });
    else if (m[2]) {
      const s = m[2];
      if (/^0[xX]/.test(s)) tokens.push(/[uU]$/.test(s) ? { k: 'uint', v: Number(s.replace(/[uU]$/, '')) } : { k: 'int', v: Number(s) });
      else if (/[.eE]/.test(s) || /[fF]$/.test(s)) tokens.push({ k: 'float', v: fr(parseFloat(s)) });
      else if (/[uU]$/.test(s)) tokens.push({ k: 'uint', v: Number(s.slice(0, -1)) >>> 0 });
      else tokens.push({ k: 'int', v: /^0\d/.test(s) ? parseInt(s, 8) : Number(s) });
    } else if (m[3]) tokens.push({ k: 'op', v: m[3] });
  }
  return tokens;
};

const GLSL_TYPE_NAMES = new Set(['void', 'bool', 'int', 'uint', 'float', 'vec2', 'vec3', 'vec4', 'ivec2', 'ivec3', 'ivec4', 'uvec2', 'uvec3', 'uvec4', 'bvec2', 'bvec3', 'bvec4', 'mat2', 'mat3', 'mat4', 'mat2x2', 'mat2x3', 'mat2x4', 'mat3x2', 'mat3x3', 'mat3x4', 'mat4x2', 'mat4x3', 'mat4x4']);
const GLSL_QUAL = new Set(['const', 'in', 'out', 'inout', 'attribute', 'uniform', 'varying', 'flat', 'smooth', 'centroid', 'highp', 'mediump', 'lowp', 'invariant', 'noperspective', 'precise']);
const GLSL_MAT_FIX = (t) => (t === 'mat2x2' ? 'mat2' : t === 'mat3x3' ? 'mat3' : t === 'mat4x4' ? 'mat4' : t);
const isTypeWord = (name, structs) => GLSL_TYPE_NAMES.has(name) || GLSL_SAMPLER.test(name) || structs.has(name);

// ---- Operações sobre valores.
const lanesOf = (a, b) => (Array.isArray(a) ? a.length : Array.isArray(b) ? b.length : 0);
const comp = (fn, a, b) => {
  const n = lanesOf(a, b);
  if (!n) return fn(a, b);
  const out = new Array(n);
  for (let i = 0; i < n; i++) out[i] = fn(Array.isArray(a) ? a[i] : a, Array.isArray(b) ? b[i] : b);
  return out;
};
const ARITH = {
  float: { '+': (a, b) => fr(a + b), '-': (a, b) => fr(a - b), '*': (a, b) => fr(a * b), '/': (a, b) => fr(a / b) },
  int: { '+': (a, b) => (a + b) | 0, '-': (a, b) => (a - b) | 0, '*': (a, b) => Math.imul(a, b), '/': (a, b) => (b === 0 ? 0 : (a / b) | 0), '%': (a, b) => (b === 0 ? 0 : a % b | 0) },
  uint: { '+': (a, b) => (a + b) >>> 0, '-': (a, b) => (a - b) >>> 0, '*': (a, b) => Math.imul(a, b) >>> 0, '/': (a, b) => (b === 0 ? 0 : Math.floor(a / b) >>> 0), '%': (a, b) => (b === 0 ? 0 : a % b >>> 0) },
};
const BITWISE = { '&': (a, b) => a & b, '|': (a, b) => a | b, '^': (a, b) => a ^ b, '<<': (a, b) => a << b, '>>': (a, b) => a >> b };

// O produto linear de matrizes por colunas: `a` é ca x ra, `b` é cb x rb (com ca == rb); o resultado é cb x ra.
const matMul = (a, ca, ra, b, cb) => {
  const out = new Array(cb * ra);
  for (let c = 0; c < cb; c++) for (let r = 0; r < ra; r++) { let s = 0; for (let k = 0; k < ca; k++) s = fr(s + fr(a[k * ra + r] * b[c * ca + k])); out[c * ra + r] = s; }
  return out;
};

// ---- O compilador.
class GlslCompiler {
  constructor(source, stage, defines = {}) {
    const pre = glslPreprocess(source, { GL_ES: { body: '1', params: null }, __VERSION__: { body: '100', params: null }, ...defines });
    this.version = pre.version;
    this.stage = stage;
    this.tokens = glslLex(pre.text);
    this.pos = 0;
    this.structs = new Map();
    this.functions = new Map();
    this.globals = new Map();
    this.gvalues = [];
    this.scopes = [new Map()];
    this.locals = null;
    this.interface = [];
    this.inits = [];
    this.blocks = [];
    // As variáveis internas de cada estágio; o rasterizador lê e escreve pelo nome.
    const builtin = stage === 'vertex'
      ? [['gl_Position', 'vec4'], ['gl_PointSize', 'float'], ['gl_VertexID', 'int'], ['gl_InstanceID', 'int']]
      : [['gl_FragColor', 'vec4'], ['gl_FragData', 'vec4[8]'], ['gl_FragCoord', 'vec4'], ['gl_FrontFacing', 'bool'], ['gl_PointCoord', 'vec2'], ['gl_FragDepth', 'float']];
    for (const [name, t] of builtin) this.declareGlobal(name, t, { qualifier: 'builtin' });
  }
  peek(o = 0) { return this.tokens[this.pos + o]; }
  next() { return this.tokens[this.pos++]; }
  is(v, o = 0) { const t = this.tokens[this.pos + o]; return t !== undefined && t.v === v && (t.k === 'op' || t.k === 'id'); }
  eat(v) { if (this.is(v)) { this.pos++; return true; } return false; }
  expect(v) { if (!this.eat(v)) glslFail(`esperava '${v}'`); }
  ident() { const t = this.next(); if (!t || t.k !== 'id') glslFail('esperava identificador'); return t.v; }

  // ---- Escopos e variáveis.
  declareGlobal(name, t, extra = {}) {
    const slot = this.gvalues.length;
    this.gvalues.push(glslZero(t, this.structs));
    const info = { name, t, slot, global: true, ...extra };
    this.globals.set(name, info);
    return info;
  }
  declareLocal(name, t, extra = {}) {
    const info = { name, t, slot: this.locals.count++, global: false, ...extra };
    this.scopes[this.scopes.length - 1].set(name, info);
    return info;
  }
  lookup(name) {
    for (let i = this.scopes.length - 1; i >= 1; i--) { const v = this.scopes[i].get(name); if (v) return v; }
    return this.globals.get(name);
  }

  // ---- Tipos.
  parseType() {
    let name = this.ident();
    if (name === 'struct') { name = this.parseStructBody(); }
    if (!isTypeWord(name, this.structs)) glslFail(`tipo desconhecido '${name}'`);
    let t = GLSL_MAT_FIX(name);
    if (this.is('[')) { this.pos++; const n = this.constInt(); this.expect(']'); t += `[${n}]`; }
    return t;
  }
  constInt() {
    const e = this.expr();
    const v = e.f({ g: this.gvalues, l: [] });
    return v | 0;
  }
  parseStructBody() {
    const name = this.is('{') ? `__anon${this.structs.size}` : this.ident();
    this.expect('{');
    const fields = [];
    while (!this.is('}')) {
      while (this.peek().k === 'id' && GLSL_QUAL.has(this.peek().v)) this.pos++;
      let t = this.parseType();
      do {
        const fname = this.ident();
        let ft = t;
        if (this.eat('[')) { ft += `[${this.constInt()}]`; this.expect(']'); }
        fields.push({ name: fname, t: ft });
      } while (this.eat(','));
      this.expect(';');
    }
    this.expect('}');
    this.structs.set(name, fields);
    return name;
  }

  // ---- Programa.
  compile() {
    while (this.pos < this.tokens.length) this.topLevel();
    return this;
  }
  topLevel() {
    if (this.eat(';')) return;
    if (this.is('precision')) { while (!this.eat(';')) this.pos++; return; }
    if (this.is('invariant') && this.peek(1).k === 'id' && this.peek(2).v === ';') { this.pos += 3; return; }
    if (this.is('struct')) {
      this.pos++;
      const name = this.parseStructBody();
      if (this.peek() && this.peek().k === 'id') { this.pos--; this.pos = this.structDeclarator(name); } else this.expect(';');
      return;
    }
    // Qualificadores, incluindo `layout(...)`.
    const quals = new Set();
    let layout = {};
    for (;;) {
      if (this.is('layout')) { this.pos++; this.expect('('); do { const key = this.ident(); let value = true; if (this.eat('=')) value = this.next().v; layout[key] = value; } while (this.eat(',')); this.expect(')'); continue; }
      const t = this.peek();
      if (t && t.k === 'id' && GLSL_QUAL.has(t.v)) { quals.add(t.v); this.pos++; continue; }
      break;
    }
    // Bloco de uniformes.
    if (quals.has('uniform') && this.peek().k === 'id' && !isTypeWord(this.peek().v, this.structs) && this.is('{', 1)) { this.uniformBlock(layout); return; }
    if ((quals.has('in') || quals.has('out')) && this.peek().k === 'id' && !isTypeWord(this.peek().v, this.structs) && this.is('{', 1)) glslFail('bloco de interface');
    const t = this.parseType();
    const name = this.ident();
    if (this.is('(')) { this.functionDef(t, name); return; }
    this.globalDeclarators(t, name, quals, layout);
  }
  structDeclarator(structName) {
    this.pos++;
    const name = this.ident();
    this.declareGlobal(name, structName);
    this.expect(';');
    return this.pos;
  }
  uniformBlock(layout) {
    const blockName = this.ident();
    this.expect('{');
    const members = [];
    while (!this.is('}')) {
      while (this.peek().k === 'id' && (GLSL_QUAL.has(this.peek().v) || this.peek().v === 'layout')) { if (this.eat('layout')) { this.expect('('); while (!this.eat(')')) this.pos++; } else this.pos++; }
      const t = this.parseType();
      do {
        const name = this.ident();
        let mt = t;
        if (this.eat('[')) { mt += `[${this.constInt()}]`; this.expect(']'); }
        members.push({ name, t: mt });
      } while (this.eat(','));
      this.expect(';');
    }
    this.expect('}');
    let instance = null;
    if (this.peek().k === 'id') instance = this.ident();
    this.expect(';');
    const block = { name: blockName, instance, members, layout };
    this.blocks.push(block);
    if (instance) { const st = `__block_${blockName}`; this.structs.set(st, members); const info = this.declareGlobal(instance, st, { qualifier: 'uniform', block: blockName }); info.blockInstance = true; } else for (const m of members) this.declareGlobal(m.name, m.t, { qualifier: 'uniform', block: blockName });
  }
  globalDeclarators(t, firstName, quals, layout) {
    let name = firstName;
    for (;;) {
      let vt = t;
      if (this.eat('[')) { vt += `[${this.constInt()}]`; this.expect(']'); }
      const qualifier = quals.has('const') ? 'const' : quals.has('uniform') ? 'uniform' : quals.has('attribute') ? 'attribute' : quals.has('varying') ? (this.stage === 'vertex' ? 'out' : 'in') : quals.has('in') ? 'in' : quals.has('out') ? 'out' : null;
      const info = this.declareGlobal(name, vt, { qualifier, flat: quals.has('flat'), location: layout.location !== undefined ? Number(layout.location) : -1 });
      if (this.eat('=')) {
        const init = this.convertTo(this.assign(), vt);
        if (qualifier === 'const') this.gvalues[info.slot] = glslClone(init.f({ g: this.gvalues, l: [] }));
        else this.inits.push((e) => { e.g[info.slot] = glslClone(init.f(e)); });
      }
      if (this.eat(',')) { name = this.ident(); continue; }
      break;
    }
    this.expect(';');
  }
  functionDef(ret, name) {
    this.expect('(');
    const params = [];
    if (!this.is(')')) {
      if (this.is('void') && this.is(')', 1)) this.pos++;
      else do {
        let q = 'in';
        while (this.peek().k === 'id' && GLSL_QUAL.has(this.peek().v)) { const w = this.next().v; if (w === 'in' || w === 'out' || w === 'inout') q = w; }
        const pt = this.parseType();
        let pname = null;
        if (this.peek().k === 'id') pname = this.ident();
        let ptype = pt;
        if (pname && this.eat('[')) { ptype += `[${this.constInt()}]`; this.expect(']'); }
        params.push({ name: pname, t: ptype, q });
      } while (this.eat(','));
    }
    this.expect(')');
    let list = this.functions.get(name);
    if (!list) this.functions.set(name, (list = []));
    const sig = params.map((p) => p.t).join(',');
    let fn = list.find((f) => f.params.map((p) => p.t).join(',') === sig);
    if (!fn) { fn = { name, ret, params, body: null, count: 0 }; list.push(fn); }
    if (this.eat(';')) return;
    this.locals = { count: 0 };
    this.scopes.push(new Map());
    for (const p of params) { const info = this.declareLocal(p.name || `__p${this.locals.count}`, p.t); p.slot = info.slot; }
    fn.ret = ret;
    fn.body = this.block();
    fn.count = this.locals.count;
    this.scopes.pop();
    this.locals = null;
  }

  // ---- Instruções.
  block() {
    this.expect('{');
    this.scopes.push(new Map());
    const list = [];
    while (!this.is('}')) list.push(this.statement());
    this.expect('}');
    this.scopes.pop();
    return (e) => { for (let i = 0; i < list.length; i++) { const r = list[i](e); if (r) return r; } return 0; };
  }
  isDeclarationStart() {
    const t = this.peek();
    if (t.k !== 'id') return false;
    if (t.v === 'struct') return true;
    if (GLSL_QUAL.has(t.v)) return true;
    if (!isTypeWord(t.v, this.structs)) return false;
    const n = this.peek(1);
    return n && ((n.k === 'id') || (n.v === '[' && this.peek(2) && this.peek(2).v !== ']' ? this.isArrayTypeDecl() : n.v === '[' && this.peek(2) && this.peek(2).v === ']'));
  }
  isArrayTypeDecl() {
    // `float[3] a` (declaração) contra `float[3](...)` (construtor): olha o que vem depois do `]`.
    let i = 1;
    while (this.peek(i) && this.peek(i).v !== ']') i++;
    const after = this.peek(i + 1);
    return after && after.k === 'id';
  }
  statement() {
    const t = this.peek();
    if (this.is('{')) return this.block();
    if (this.eat(';')) return () => 0;
    if (t.k === 'id') {
      switch (t.v) {
        case 'if': { this.pos++; this.expect('('); const c = this.convertTo(this.expr(), 'bool').f; this.expect(')'); const a = this.scoped(() => this.statement()); const b = this.eat('else') ? this.scoped(() => this.statement()) : null; return (e) => (c(e) ? a(e) : b ? b(e) : 0); }
        case 'for': return this.forStatement();
        case 'while': { this.pos++; this.expect('('); this.scopes.push(new Map()); const c = this.convertTo(this.expr(), 'bool').f; this.expect(')'); const body = this.statement(); this.scopes.pop(); return (e) => { for (let n = 0; c(e); n++) { if (n > 1e6) glslFail('laço longo demais'); const r = body(e); if (r === 1) break; if (r > 2) return r; } return 0; }; }
        case 'do': { this.pos++; const body = this.scoped(() => this.statement()); this.expect('while'); this.expect('('); const c = this.convertTo(this.expr(), 'bool').f; this.expect(')'); this.expect(';'); return (e) => { for (let n = 0; ; n++) { if (n > 1e6) glslFail('laço longo demais'); const r = body(e); if (r === 1) break; if (r > 2) return r; if (!c(e)) break; } return 0; }; }
        case 'return': { this.pos++; if (this.eat(';')) return (e) => { e.ret = undefined; return 3; }; const v = this.expr(); this.expect(';'); const f = v.f; return (e) => { e.ret = f(e); return 3; }; }
        case 'break': this.pos++; this.expect(';'); return () => 1;
        case 'continue': this.pos++; this.expect(';'); return () => 2;
        case 'discard': this.pos++; this.expect(';'); return (e) => { e.rt.discarded = true; return 4; };
        case 'switch': return this.switchStatement();
        default:
      }
      if (this.isDeclarationStart()) return this.localDeclaration();
    }
    const x = this.expr();
    this.expect(';');
    const f = x.f;
    return (e) => { f(e); return 0; };
  }
  scoped(fn) { this.scopes.push(new Map()); const r = fn(); this.scopes.pop(); return r; }
  forStatement() {
    this.pos++;
    this.expect('(');
    this.scopes.push(new Map());
    let init = () => 0;
    if (!this.eat(';')) { if (this.isDeclarationStart()) init = this.localDeclaration(); else { const x = this.expr().f; this.expect(';'); init = (e) => { x(e); return 0; }; } }
    let cond = () => true;
    if (!this.eat(';')) { cond = this.convertTo(this.expr(), 'bool').f; this.expect(';'); }
    let step = () => 0;
    if (!this.is(')')) step = this.expr().f;
    this.expect(')');
    const body = this.statement();
    this.scopes.pop();
    return (e) => {
      init(e);
      for (let n = 0; cond(e); step(e), n++) {
        if (n > 1e6) glslFail('laço longo demais');
        const r = body(e);
        if (r === 1) break;
        if (r > 2) return r;
      }
      return 0;
    };
  }
  switchStatement() {
    this.pos++;
    this.expect('(');
    const subject = this.expr().f;
    this.expect(')');
    this.expect('{');
    this.scopes.push(new Map());
    const cases = [];
    let current = null;
    while (!this.is('}')) {
      if (this.eat('case')) { const v = this.constInt(); this.expect(':'); current = { value: v, body: [] }; cases.push(current); } else if (this.eat('default')) { this.expect(':'); current = { value: null, body: [] }; cases.push(current); } else if (current) current.body.push(this.statement()); else glslFail('instrução fora de case');
    }
    this.expect('}');
    this.scopes.pop();
    return (e) => {
      const v = subject(e);
      let start = cases.findIndex((c) => c.value === v);
      if (start < 0) start = cases.findIndex((c) => c.value === null);
      if (start < 0) return 0;
      for (let i = start; i < cases.length; i++) for (const s of cases[i].body) { const r = s(e); if (r === 1) return 0; if (r) return r; }
      return 0;
    };
  }
  localDeclaration() {
    let isConst = false;
    while (this.peek().k === 'id' && GLSL_QUAL.has(this.peek().v)) { if (this.next().v === 'const') isConst = true; }
    const base = this.parseType();
    const steps = [];
    do {
      const name = this.ident();
      let t = base;
      if (this.eat('[')) { t += `[${this.constInt()}]`; this.expect(']'); }
      let init = null;
      if (this.eat('=')) init = this.convertTo(this.assign(), t);
      const info = this.declareLocal(name, t);
      const slot = info.slot;
      const zero = () => glslZero(t, this.structs);
      steps.push(init ? (e) => { e.l[slot] = glslClone(init.f(e)); } : (e) => { e.l[slot] = zero(); });
    } while (this.eat(','));
    this.expect(';');
    return (e) => { for (const s of steps) s(e); return 0; };
  }

  // ---- Expressões.
  expr() {
    let x = this.assign();
    while (this.eat(',')) { const a = x, b = this.assign(); x = { t: b.t, f: (e) => { a.f(e); return b.f(e); } }; }
    return x;
  }
  assign() {
    const left = this.ternary();
    const t = this.peek();
    if (t && t.k === 'op' && /^(=|\+=|-=|\*=|\/=|%=|&=|\^=|\|=|<<=|>>=)$/.test(t.v)) {
      this.pos++;
      if (!left.lv) glslFail('atribuição a não lvalue');
      let right = this.assign();
      const lv = left.lv;
      if (t.v === '=') { right = this.convertTo(right, left.t); const f = right.f; return { t: left.t, f: (e) => { const v = glslClone(f(e)); lv.set(e, v); return v; } }; }
      const op = this.binaryOp(t.v.slice(0, -1), left, right);
      const conv = this.convertTo(op, left.t);
      return { t: left.t, f: (e) => { const v = conv.f(e); lv.set(e, v); return v; } };
    }
    return left;
  }
  ternary() {
    const c = this.binary(0);
    if (!this.eat('?')) return c;
    const a = this.assign();
    this.expect(':');
    const b = this.assign();
    let t = a.t;
    let x = a, y = b;
    if (a.t !== b.t) { const k = glslKind(a.t), k2 = glslKind(b.t); if (k === 'float' || k2 === 'float') { t = k === 'float' ? a.t : b.t; x = this.convertTo(a, t); y = this.convertTo(b, t); } }
    const cf = this.convertTo(c, 'bool').f;
    return { t, f: (e) => (cf(e) ? x.f(e) : y.f(e)) };
  }
  binary(level) {
    const levels = [['||'], ['^^'], ['&&'], ['|'], ['^'], ['&'], ['==', '!='], ['<', '>', '<=', '>='], ['<<', '>>'], ['+', '-'], ['*', '/', '%']];
    if (level === levels.length) return this.unary();
    let left = this.binary(level + 1);
    for (;;) {
      const t = this.peek();
      if (!t || t.k !== 'op' || !levels[level].includes(t.v)) return left;
      this.pos++;
      const right = this.binary(level + 1);
      left = this.binaryOp(t.v, left, right);
    }
  }
  unify(a, b) {
    if (a.t === b.t) return [a, b];
    const ka = glslKind(a.t), kb = glslKind(b.t), da = glslDims(a.t), db = glslDims(b.t);
    if (!da || !db || ka === kb) return [a, b];
    // O tipo vencedor: float vence int, e uint vence int (a conversão implícita da especificação).
    const rank = { bool: 0, int: 1, uint: 2, float: 3 };
    const kind = rank[ka] >= rank[kb] ? ka : kb;
    const conv = (x, d) => (glslKind(x.t) === kind ? x : this.convertTo(x, d.mat ? `mat${d.cols}${d.cols === d.rows ? '' : 'x' + d.rows}` : glslVecType(kind, d.n)));
    return [conv(a, da), conv(b, db)];
  }
  binaryOp(op, left, right) {
    if (op === '&&') { const a = left.f, b = right.f; return { t: 'bool', f: (e) => a(e) && b(e) }; }
    if (op === '||') { const a = left.f, b = right.f; return { t: 'bool', f: (e) => a(e) || b(e) }; }
    if (op === '^^') { const a = left.f, b = right.f; return { t: 'bool', f: (e) => a(e) !== b(e) }; }
    if (op === '==' || op === '!=') {
      [left, right] = this.unify(left, right);
      const a = left.f, b = right.f;
      return { t: 'bool', f: op === '==' ? (e) => glslEqual(a(e), b(e)) : (e) => !glslEqual(a(e), b(e)) };
    }
    if (op === '<' || op === '>' || op === '<=' || op === '>=') {
      [left, right] = this.unify(left, right);
      const a = left.f, b = right.f;
      const fn = op === '<' ? (x, y) => x < y : op === '>' ? (x, y) => x > y : op === '<=' ? (x, y) => x <= y : (x, y) => x >= y;
      return { t: 'bool', f: (e) => fn(a(e), b(e)) };
    }
    [left, right] = this.unify(left, right);
    const da = glslDims(left.t), db = glslDims(right.t);
    if (!da || !db) glslFail(`operandos inválidos para '${op}'`);
    const kind = glslKind(left.t);
    const a = left.f, b = right.f;
    if (op in BITWISE) { const fn = BITWISE[op]; const wrap = kind === 'uint' ? (x) => x >>> 0 : (x) => x | 0; const t = da.n >= db.n ? left.t : right.t; return { t, f: (e) => comp((x, y) => wrap(fn(x, y)), a(e), b(e)) }; }
    const fn = ARITH[kind] && ARITH[kind][op];
    if (!fn) glslFail(`operador '${op}' inválido`);
    if (op === '*' && (da.mat || db.mat) && !(da.scalar || db.scalar)) {
      if (da.mat && db.mat) return { t: `mat${db.cols}${da.rows === db.cols ? '' : 'x' + da.rows}`.replace(/^mat(\d)x\1$/, 'mat$1'), f: (e) => matMul(a(e), da.cols, da.rows, b(e), db.cols) };
      if (da.mat && db.vec) return { t: glslVecType('float', da.rows), f: (e) => matMul(a(e), da.cols, da.rows, b(e), 1) };
      if (da.vec && db.mat) return { t: glslVecType('float', db.cols), f: (e) => { const v = a(e), m = b(e), out = new Array(db.cols); for (let c = 0; c < db.cols; c++) { let s = 0; for (let k = 0; k < db.rows; k++) s = fr(s + fr(v[k] * m[c * db.rows + k])); out[c] = s; } return out; } };
    }
    const t = da.n >= db.n ? left.t : right.t;
    return { t, f: (e) => comp(fn, a(e), b(e)) };
  }
  unary() {
    const t = this.peek();
    if (t.k === 'op') {
      if (t.v === '-' || t.v === '+' || t.v === '!' || t.v === '~') {
        this.pos++;
        const x = this.unary();
        const f = x.f;
        if (t.v === '+') return x;
        if (t.v === '!') return { t: 'bool', f: (e) => !f(e) };
        if (t.v === '~') return { t: x.t, f: (e) => comp((v) => ~v, f(e)) };
        const k = glslKind(x.t);
        const neg = k === 'float' ? (v) => -v : k === 'uint' ? (v) => (-v) >>> 0 : (v) => -v | 0;
        return { t: x.t, f: (e) => comp(neg, f(e)) };
      }
      if (t.v === '++' || t.v === '--') {
        this.pos++;
        const x = this.unary();
        if (!x.lv) glslFail('++ em não lvalue');
        const one = { t: glslKind(x.t), f: () => (glslKind(x.t) === 'float' ? 1 : 1) };
        const op = this.binaryOp(t.v === '++' ? '+' : '-', x, one);
        const lv = x.lv;
        return { t: x.t, f: (e) => { const v = op.f(e); lv.set(e, v); return v; } };
      }
    }
    return this.postfix();
  }
  postfix() {
    let x = this.primary();
    for (;;) {
      if (this.eat('.')) {
        const name = this.ident();
        x = this.fieldAccess(x, name);
      } else if (this.eat('[')) {
        const index = this.convertTo(this.expr(), 'int');
        this.expect(']');
        x = this.indexAccess(x, index);
      } else if (this.is('++') || this.is('--')) {
        const inc = this.next().v === '++';
        if (!x.lv) glslFail('++ em não lvalue');
        const lv = x.lv;
        const op = this.binaryOp(inc ? '+' : '-', x, { t: glslKind(x.t), f: () => 1 });
        const get = x.f;
        x = { t: x.t, f: (e) => { const old = glslClone(get(e)); lv.set(e, op.f(e)); return old; } };
      } else return x;
    }
  }
  fieldAccess(x, name) {
    const st = this.structs.get(x.t);
    if (st) {
      const field = st.find((f) => f.name === name);
      if (!field) glslFail(`campo '${name}' inexistente`);
      const get = x.f;
      const lvbase = x.lv;
      return { t: field.t, f: (e) => get(e)[name], lv: lvbase ? { get: (e) => get(e)[name], set: (e, v) => { const o = glslClone(get(e)); o[name] = v; lvbase.set(e, o); } } : null };
    }
    const d = glslDims(x.t);
    if (d && d.vec) {
      const sets = ['xyzw', 'rgba', 'stpq'];
      const set = sets.find((s) => [...name].every((c) => s.indexOf(c) >= 0 && s.indexOf(c) < d.n));
      if (!set) glslFail(`seletor '${name}' inválido`);
      const idx = [...name].map((c) => set.indexOf(c));
      const kind = glslKind(x.t);
      const t = glslVecType(kind, idx.length);
      const get = x.f;
      const lvbase = x.lv;
      const f = idx.length === 1 ? (e) => get(e)[idx[0]] : (e) => { const v = get(e); return idx.map((i) => v[i]); };
      const lv = lvbase ? { get: f, set: (e, value) => { const v = get(e).slice(); if (idx.length === 1) v[idx[0]] = value; else idx.forEach((i, k) => { v[i] = value[k]; }); lvbase.set(e, v); } } : null;
      return { t, f, lv };
    }
    glslFail(`'.${name}' inválido`);
  }
  indexAccess(x, index) {
    const arr = glslArrayOf(x.t);
    const d = glslDims(x.t);
    let t, f, setter;
    const get = x.f, ix = index.f;
    if (arr) { t = arr.base; f = (e) => get(e)[ix(e)]; setter = (e, v, base) => { const a = glslClone(get(e)); a[ix(e)] = v; base.set(e, a); }; } else if (d && d.vec) { t = glslVecType(glslKind(x.t), 1); f = (e) => get(e)[ix(e)]; setter = (e, v, base) => { const a = get(e).slice(); a[ix(e)] = v; base.set(e, a); }; } else if (d && d.mat) {
      t = glslVecType('float', d.rows);
      f = (e) => { const m = get(e), c = ix(e); return m.slice(c * d.rows, c * d.rows + d.rows); };
      setter = (e, v, base) => { const m = get(e).slice(); const c = ix(e); for (let r = 0; r < d.rows; r++) m[c * d.rows + r] = v[r]; base.set(e, m); };
    } else glslFail('indexação inválida');
    const base = x.lv;
    return { t, f, lv: base ? { get: f, set: (e, v) => setter(e, v, base) } : null };
  }
  primary() {
    const t = this.next();
    if (!t) glslFail('fim inesperado');
    if (t.k === 'int') return { t: 'int', f: () => t.v | 0 };
    if (t.k === 'uint') return { t: 'uint', f: () => t.v };
    if (t.k === 'float') return { t: 'float', f: () => t.v };
    if (t.k === 'op' && t.v === '(') { const x = this.expr(); this.expect(')'); return { t: x.t, f: x.f }; }
    if (t.k !== 'id') glslFail(`símbolo inesperado '${t.v}'`);
    if (t.v === 'true') return { t: 'bool', f: () => true };
    if (t.v === 'false') return { t: 'bool', f: () => false };
    // Tipo seguido de `[n]` e `(`: construtor de array.
    if (isTypeWord(t.v, this.structs) && (this.is('(') || this.is('['))) {
      let type = GLSL_MAT_FIX(t.v);
      if (this.is('[')) { this.pos++; const n = this.is(']') ? -1 : this.constInt(); this.expect(']'); type += `[${n}]`; }
      return this.constructorCall(type);
    }
    if (this.is('(') && !this.lookup(t.v)) return this.call(t.v);
    const v = this.lookup(t.v);
    if (!v) glslFail(`'${t.v}' não declarado`);
    const slot = v.slot;
    const isConstLocal = v.qualifier === 'const' || v.qualifier === 'uniform';
    if (v.global) return { t: v.t, f: (e) => e.g[slot], lv: isConstLocal ? null : { get: (e) => e.g[slot], set: (e, x) => { e.g[slot] = x; } } };
    return { t: v.t, f: (e) => e.l[slot], lv: { get: (e) => e.l[slot], set: (e, x) => { e.l[slot] = x; } } };
  }

  // ---- Conversões e construtores.
  convertTo(x, t) {
    if (x.t === t) return x;
    const k = glslKind(t), kx = glslKind(x.t);
    const d = glslDims(t), dx = glslDims(x.t);
    if (d && dx && k && kx && d.n === dx.n && k !== kx) { const c = GLSL_CONV[k]; const f = x.f; return { t, f: d.scalar ? (e) => c(f(e)) : (e) => f(e).map(c) }; }
    if (glslArrayOf(t) && glslArrayOf(x.t)) return { t, f: x.f };
    return x;
  }
  constructorCall(type) {
    this.expect('(');
    const args = [];
    if (!this.is(')')) do { args.push(this.assign()); } while (this.eat(','));
    this.expect(')');
    const arr = glslArrayOf(type);
    if (arr) {
      const len = arr.len < 0 ? args.length : arr.len;
      const t = `${arr.base}[${len}]`;
      const conv = args.map((a) => this.convertTo(a, arr.base));
      return { t, f: (e) => conv.map((a) => glslClone(a.f(e))) };
    }
    const st = this.structs.get(type);
    if (st) { const conv = args.map((a, i) => this.convertTo(a, st[i].t)); return { t: type, f: (e) => { const o = {}; st.forEach((fd, i) => { o[fd.name] = glslClone(conv[i].f(e)); }); return o; } }; }
    const d = glslDims(type);
    if (!d) glslFail(`construtor de '${type}'`);
    const kind = glslKind(type);
    const conv = GLSL_CONV[kind];
    const fs = args.map((a) => a.f);
    const dims = args.map((a) => glslDims(a.t));
    const flat = (e) => { const out = []; fs.forEach((f, i) => { const v = f(e); if (Array.isArray(v)) for (const c of v) out.push(c); else out.push(v); }); return out; };
    if (d.scalar) return { t: type, f: (e) => { const v = fs[0](e); return conv(Array.isArray(v) ? v[0] : v); } };
    if (d.vec) {
      if (args.length === 1 && dims[0] && dims[0].scalar) return { t: type, f: (e) => { const v = conv(fs[0](e)); return new Array(d.n).fill(v); } };
      return { t: type, f: (e) => flat(e).slice(0, d.n).map(conv) };
    }
    // Matrizes.
    if (args.length === 1 && dims[0] && dims[0].scalar) return { t: type, f: (e) => { const s = conv(fs[0](e)); const m = new Array(d.n).fill(0); for (let i = 0; i < Math.min(d.cols, d.rows); i++) m[i * d.rows + i] = s; return m; } };
    if (args.length === 1 && dims[0] && dims[0].mat) return { t: type, f: (e) => { const src = fs[0](e), sd = dims[0]; const m = new Array(d.n).fill(0); for (let c = 0; c < d.cols; c++) for (let r = 0; r < d.rows; r++) m[c * d.rows + r] = c < sd.cols && r < sd.rows ? src[c * sd.rows + r] : c === r ? 1 : 0; return m; } };
    return { t: type, f: (e) => flat(e).slice(0, d.n).map(conv) };
  }

  // ---- Chamadas.
  call(name) {
    this.expect('(');
    const args = [];
    if (!this.is(')')) do { args.push(this.assign()); } while (this.eat(','));
    this.expect(')');
    const user = this.functions.get(name);
    if (user) return this.userCall(name, user, args);
    const builtin = GLSL_BUILTINS[name];
    if (!builtin) glslFail(`função '${name}' não declarada`);
    return builtin(args, this);
  }
  userCall(name, list, args) {
    let fn = list.find((f) => f.params.length === args.length && f.params.every((p, i) => p.t === args[i].t));
    if (!fn) fn = list.find((f) => f.params.length === args.length && f.params.every((p, i) => glslKind(p.t) === glslKind(args[i].t) || (glslKind(p.t) === 'float' && glslKind(args[i].t) === 'int') && glslDims(p.t) && glslDims(args[i].t) && glslDims(p.t).n === glslDims(args[i].t).n));
    if (!fn) glslFail(`sem sobrecarga de '${name}' para os argumentos`);
    const conv = args.map((a, i) => this.convertTo(a, fn.params[i].t));
    const self = this;
    return {
      t: fn.ret,
      f: (e) => {
        const local = { g: e.g, l: new Array(fn.count), ret: undefined, rt: e.rt };
        fn.params.forEach((p, i) => { local.l[p.slot] = p.q === 'out' ? glslZero(p.t, self.structs) : glslClone(conv[i].f(e)); });
        if (!fn.body) glslFail(`função '${name}' sem corpo`);
        const r = fn.body(local);
        fn.params.forEach((p, i) => { if ((p.q === 'out' || p.q === 'inout') && args[i].lv) args[i].lv.set(e, local.l[p.slot]); });
        return r === 4 ? undefined : local.ret;
      },
    };
  }
}

// ---- Funções internas.
const asArgs = (args) => args.map((a) => a.f);
const GLSL_F1 = (fn) => (args) => { if (args.length !== 1) glslFail('aridade'); const a = args[0].f; return { t: args[0].t, f: (e) => comp((x) => fr(fn(x)), a(e)) }; };
// Argumentos genType com escalares que se espalham: o tipo do resultado é o do primeiro argumento vetorial.
const GLSL_GEN = (fn, kinds = 'float', boolResult = false) => (args, c) => {
  const vecArg = args.find((a) => glslDims(a.t) && !glslDims(a.t).scalar) || args[0];
  let kind = glslKind(vecArg.t);
  const wantFloat = kinds === 'float' && kind !== 'float' && args.some((a) => glslKind(a.t) === 'float');
  if (wantFloat) kind = 'float';
  const n = glslDims(vecArg.t).n;
  const t = boolResult ? glslVecType('bool', n) : glslVecType(kind, n);
  const conv = args.map((a) => (glslKind(a.t) !== kind && glslDims(a.t) ? c.convertTo(a, glslVecType(kind, glslDims(a.t).n)) : a));
  const fs = conv.map((a) => a.f);
  const wrap = boolResult || kind === 'bool' ? (x) => x : kind === 'float' ? fr : kind === 'int' ? (x) => x | 0 : (x) => x >>> 0;
  return { t, f: (e) => { const v = fs.map((f) => f(e)); if (n === 1 && !v.some(Array.isArray)) return wrap(fn(...v)); const out = new Array(n); for (let i = 0; i < n; i++) out[i] = wrap(fn(...v.map((x) => (Array.isArray(x) ? x[i] : x)))); return out; } };
};
const fsign = (x) => (x > 0 ? 1 : x < 0 ? -1 : 0);
const fclamp = (x, lo, hi) => Math.min(Math.max(x, lo), hi);
const roundEven = (x) => { const r = Math.round(x); return Math.abs(x % 1) === 0.5 && r % 2 !== 0 ? r - 1 : r; };
const smooth = (a, b, x) => { const t = fclamp(fr((x - a) / (b - a)), 0, 1); return fr(t * t * (3 - 2 * t)); };
const lenOf = (v) => { if (!Array.isArray(v)) return Math.abs(v); let s = 0; for (const c of v) s = fr(s + fr(c * c)); return fr(Math.sqrt(s)); };
const dot = (a, b) => { if (!Array.isArray(a)) return fr(a * b); let s = 0; for (let i = 0; i < a.length; i++) s = fr(s + fr(a[i] * b[i])); return s; };
const GLSL_BUILTINS = {
  radians: GLSL_F1((x) => x * (Math.PI / 180)), degrees: GLSL_F1((x) => x * (180 / Math.PI)),
  sin: GLSL_F1(Math.sin), cos: GLSL_F1(Math.cos), tan: GLSL_F1(Math.tan), asin: GLSL_F1(Math.asin), acos: GLSL_F1(Math.acos),
  sinh: GLSL_F1(Math.sinh), cosh: GLSL_F1(Math.cosh), tanh: GLSL_F1(Math.tanh), asinh: GLSL_F1(Math.asinh), acosh: GLSL_F1(Math.acosh), atanh: GLSL_F1(Math.atanh),
  exp: GLSL_F1(Math.exp), log: GLSL_F1(Math.log), exp2: GLSL_F1((x) => 2 ** x), log2: GLSL_F1(Math.log2), sqrt: GLSL_F1(Math.sqrt), inversesqrt: GLSL_F1((x) => 1 / Math.sqrt(x)),
  atan: (args, c) => (args.length === 1 ? GLSL_F1(Math.atan)(args, c) : GLSL_GEN((y, x) => Math.atan2(y, x))(args, c)),
  pow: GLSL_GEN((x, y) => x ** y),
  abs: GLSL_GEN(Math.abs, 'any'), sign: GLSL_GEN(fsign, 'any'), floor: GLSL_GEN(Math.floor), trunc: GLSL_GEN(Math.trunc), round: GLSL_GEN((x) => Math.round(x)), roundEven: GLSL_GEN(roundEven), ceil: GLSL_GEN(Math.ceil),
  fract: GLSL_GEN((x) => x - Math.floor(x)),
  mod: GLSL_GEN((x, y) => x - y * Math.floor(x / y)),
  min: GLSL_GEN(Math.min, 'any'), max: GLSL_GEN(Math.max, 'any'),
  clamp: GLSL_GEN(fclamp, 'any'),
  mix: GLSL_GEN((x, y, a) => (typeof a === 'boolean' ? (a ? y : x) : x * (1 - a) + y * a)),
  step: GLSL_GEN((edge, x) => (x < edge ? 0 : 1)),
  smoothstep: GLSL_GEN(smooth),
  length: (args) => { const a = args[0].f; return { t: 'float', f: (e) => lenOf(a(e)) }; },
  distance: (args) => { const [a, b] = asArgs(args); return { t: 'float', f: (e) => { const x = a(e), y = b(e); return lenOf(Array.isArray(x) ? x.map((c, i) => fr(c - y[i])) : fr(x - y)); } }; },
  dot: (args) => { const [a, b] = asArgs(args); return { t: 'float', f: (e) => dot(a(e), b(e)) }; },
  cross: (args) => { const [a, b] = asArgs(args); return { t: 'vec3', f: (e) => { const x = a(e), y = b(e); return [fr(fr(x[1] * y[2]) - fr(x[2] * y[1])), fr(fr(x[2] * y[0]) - fr(x[0] * y[2])), fr(fr(x[0] * y[1]) - fr(x[1] * y[0]))]; } }; },
  normalize: (args) => { const a = args[0].f; return { t: args[0].t, f: (e) => { const v = a(e); const l = lenOf(v); return Array.isArray(v) ? v.map((c) => fr(c / l)) : fr(v / l); } }; },
  faceforward: (args) => { const [n, i, r] = asArgs(args); return { t: args[0].t, f: (e) => (dot(r(e), i(e)) < 0 ? n(e) : n(e).map((c) => -c)) }; },
  reflect: (args) => { const [i, n] = asArgs(args); return { t: args[0].t, f: (e) => { const iv = i(e), nv = n(e), d = fr(2 * dot(nv, iv)); return iv.map((c, k) => fr(c - fr(d * nv[k]))); } }; },
  refract: (args) => { const [i, n, eta] = asArgs(args); return { t: args[0].t, f: (e) => { const iv = i(e), nv = n(e), r = eta(e), d = dot(nv, iv); const k = fr(1 - fr(fr(r * r) * fr(1 - fr(d * d)))); return k < 0 ? iv.map(() => 0) : iv.map((c, q) => fr(fr(r * c) - fr(fr(r * d + Math.sqrt(k)) * nv[q]))); } }; },
  matrixCompMult: (args) => { const [a, b] = asArgs(args); return { t: args[0].t, f: (e) => a(e).map((x, i) => fr(x * b(e)[i])) }; },
  outerProduct: (args) => { const [a, b] = asArgs(args); const ra = glslDims(args[0].t).n, cb = glslDims(args[1].t).n; return { t: ra === cb ? `mat${ra}` : `mat${cb}x${ra}`, f: (e) => { const x = a(e), y = b(e), m = []; for (let c = 0; c < cb; c++) for (let r = 0; r < ra; r++) m.push(fr(x[r] * y[c])); return m; } }; },
  transpose: (args) => { const a = args[0].f, d = glslDims(args[0].t); return { t: d.cols === d.rows ? args[0].t : `mat${d.rows}x${d.cols}`, f: (e) => { const m = a(e), o = []; for (let r = 0; r < d.rows; r++) for (let c = 0; c < d.cols; c++) o.push(m[c * d.rows + r]); return o; } }; },
  determinant: (args) => { const a = args[0].f, n = glslDims(args[0].t).cols; return { t: 'float', f: (e) => glslDet(a(e), n) }; },
  inverse: (args) => { const a = args[0].f, n = glslDims(args[0].t).cols; return { t: args[0].t, f: (e) => glslInverse(a(e), n) }; },
  lessThan: GLSL_GEN((a, b) => a < b, 'any', true), lessThanEqual: GLSL_GEN((a, b) => a <= b, 'any', true), greaterThan: GLSL_GEN((a, b) => a > b, 'any', true), greaterThanEqual: GLSL_GEN((a, b) => a >= b, 'any', true),
  equal: GLSL_GEN((a, b) => a === b, 'any', true), notEqual: GLSL_GEN((a, b) => a !== b, 'any', true),
  any: (args) => { const a = args[0].f; return { t: 'bool', f: (e) => a(e).some(Boolean) }; },
  all: (args) => { const a = args[0].f; return { t: 'bool', f: (e) => a(e).every(Boolean) }; },
  not: (args) => { const a = args[0].f; return { t: args[0].t, f: (e) => a(e).map((x) => !x) }; },
  isnan: GLSL_GEN((x) => Number.isNaN(x), 'any', true), isinf: GLSL_GEN((x) => x === Infinity || x === -Infinity, 'any', true),
  floatBitsToInt: (args) => { const a = args[0].f; return { t: glslVecType('int', glslDims(args[0].t).n), f: (e) => comp((x) => { const b = new DataView(new ArrayBuffer(4)); b.setFloat32(0, x); return b.getInt32(0); }, a(e)) }; },
  floatBitsToUint: (args) => { const a = args[0].f; return { t: glslVecType('uint', glslDims(args[0].t).n), f: (e) => comp((x) => { const b = new DataView(new ArrayBuffer(4)); b.setFloat32(0, x); return b.getUint32(0); }, a(e)) }; },
  intBitsToFloat: (args) => { const a = args[0].f; return { t: glslVecType('float', glslDims(args[0].t).n), f: (e) => comp((x) => { const b = new DataView(new ArrayBuffer(4)); b.setInt32(0, x); return b.getFloat32(0); }, a(e)) }; },
  uintBitsToFloat: (args) => { const a = args[0].f; return { t: glslVecType('float', glslDims(args[0].t).n), f: (e) => comp((x) => { const b = new DataView(new ArrayBuffer(4)); b.setUint32(0, x); return b.getFloat32(0); }, a(e)) }; },
  dFdx: (args) => { const a = args[0].f; return { t: args[0].t, f: (e) => e.rt.derivative(a, e, 'x') }; },
  dFdy: (args) => { const a = args[0].f; return { t: args[0].t, f: (e) => e.rt.derivative(a, e, 'y') }; },
  fwidth: (args) => { const a = args[0].f; return { t: args[0].t, f: (e) => comp((x, y) => fr(Math.abs(x) + Math.abs(y)), e.rt.derivative(a, e, 'x'), e.rt.derivative(a, e, 'y')) }; },
};
GLSL_BUILTINS.dFdxCoarse = GLSL_BUILTINS.dFdxFine = GLSL_BUILTINS.dFdx;
GLSL_BUILTINS.dFdyCoarse = GLSL_BUILTINS.dFdyFine = GLSL_BUILTINS.dFdy;
// Amostragem: `rt.sample(unit, samplerType, coord, lod, bias, mode)` devolve o vec4 (ou ivec4) lido.
for (const name of ['texture', 'texture2D', 'textureCube', 'texture3D', 'texture2DProj', 'textureProj', 'texture2DLod', 'textureLod', 'textureCubeLod', 'texture2DProjLod', 'textureProjLod', 'textureOffset']) {
  GLSL_BUILTINS[name] = (args) => {
    const [s, c, extra] = asArgs(args);
    const st = args[0].t;
    const lodForm = /Lod$/.test(name);
    const proj = /Proj/.test(name);
    const t = /^i/.test(st) ? 'ivec4' : /^u/.test(st) ? 'uvec4' : /Shadow$/.test(st) ? 'float' : 'vec4';
    return { t, f: (e) => e.rt.sample(s(e), st, c(e), lodForm && extra ? extra(e) : null, !lodForm && extra && name !== 'textureOffset' ? extra(e) : 0, proj) };
  };
}
GLSL_BUILTINS.texelFetch = (args) => { const [s, p, l] = asArgs(args); const st = args[0].t; return { t: /^i/.test(st) ? 'ivec4' : /^u/.test(st) ? 'uvec4' : 'vec4', f: (e) => e.rt.texelFetch(s(e), st, p(e), l ? l(e) : 0) }; };
GLSL_BUILTINS.textureSize = (args) => { const [s, l] = asArgs(args); return { t: /3D|Array/.test(args[0].t) ? 'ivec3' : 'ivec2', f: (e) => e.rt.textureSize(s(e), args[0].t, l ? l(e) : 0) }; };
GLSL_BUILTINS.mod = GLSL_BUILTINS.mod;

const glslDet = (m, n) => {
  if (n === 2) return fr(fr(m[0] * m[3]) - fr(m[2] * m[1]));
  if (n === 3) return fr(m[0] * (m[4] * m[8] - m[7] * m[5]) - m[3] * (m[1] * m[8] - m[7] * m[2]) + m[6] * (m[1] * m[5] - m[4] * m[2]));
  const sub = (skipCol) => { const o = []; for (let c = 1; c < 4; c++) { if (c === skipCol) continue; for (let r = 1; r < 4; r++) o.push(m[c * 4 + r]); } return o; };
  let s = 0;
  for (let c = 0; c < 4; c++) s += (c % 2 ? -1 : 1) * m[c * 4] * glslDet(sub(c), 3);
  return fr(s);
};
const glslInverse = (m, n) => {
  const det = glslDet(m, n);
  const a = Array.from({ length: n }, (_, c) => Array.from({ length: n }, (_, r) => m[c * n + r]));
  const minor = (c, r) => { const o = []; for (let cc = 0; cc < n; cc++) { if (cc === c) continue; for (let rr = 0; rr < n; rr++) { if (rr === r) continue; o.push(a[cc][rr]); } } return o; };
  const out = new Array(n * n);
  for (let c = 0; c < n; c++) for (let r = 0; r < n; r++) out[r * n + c] = fr(((c + r) % 2 ? -1 : 1) * (n === 2 ? a[1 - c][1 - r] : glslDet(minor(c, r), n - 1)) / det);
  return out;
};

// Compila um estágio. Devolve o objeto que o rasterizador executa.
const glslCompile = (source, stage, defines) => {
  const c = new GlslCompiler(source, stage, defines);
  c.compile();
  const main = c.functions.get('main');
  if (!main || !main[0].body) glslFail('sem main');
  return c;
};
