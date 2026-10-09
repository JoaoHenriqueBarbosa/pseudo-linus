// Gera tests/golden/weak_more_bun.tsv: Map/Set com mutação durante a iteração, chaves -0/NaN/objeto/símbolo,
// WeakMap/WeakSet/WeakRef/FinalizationRegistry com cada tipo de chave (símbolo não registrado permitido, registrado
// TypeError), registro de símbolos (for/keyFor/description/well-known), ordem de getOwnPropertySymbols, subclasses,
// species, this inválido e descritor de size, medidos no bun 1.4.2. Complementa collection_mutation, symbol_weak e
// esnext com produtos cartesianos (coleção x iteração x mutação, chave x operação, alvo x held x token).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-scope-golden.js.
// Uso: bun scripts/gen-weak-more-golden.js > tests/golden/weak_more_bun.tsv
const fs = require("fs");
const { emitRow } = require("./golden-prelude.js");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const PRELUDE = [
  "function show(v, d) {",
  "  d = d || 0;",
  "  if (typeof v === 'symbol') return v.toString();",
  "  if (typeof v === 'string') return JSON.stringify(v);",
  "  if (typeof v === 'number') return Object.is(v, -0) ? '-0' : String(v);",
  "  if (typeof v === 'bigint') return v + 'n';",
  "  if (v === null || v === undefined || typeof v === 'boolean') return String(v);",
  "  if (typeof v === 'function') return 'fn';",
  "  if (d > 3) return '...';",
  "  if (Array.isArray(v)) return '[' + v.map(x => show(x, d + 1)).join(',') + ']';",
  "  return Object.prototype.toString.call(v);",
  "}",
  "function T(fn) { try { globalThis.R = show(fn()); } catch (e) { globalThis.R = e.name + ': ' + e.message; } }",
  "",
].join("\n");

const programs = [];
const add = body => programs.push(body);
const T = body => add(`T(() => { ${body} });`);

// ---- A. Mutação durante a iteração: coleção x iteração x mutação x quando.
const MUT = {
  del: "c.delete(k)",
  delnext: "c.delete(k + 1)",
  delprev: "c.delete(k - 1)",
  addnew: "ad(9)",
  readd: "c.delete(k); ad(k)",
  clear: "c.clear()",
  clearadd: "c.clear(); ad(7)",
  addexist: "ad(k + 1)",
  delothers: "for (const x of [...c.keys()]) if (x !== k) c.delete(x)",
};
const ITER = {
  forEach: "try { c.forEach((v, k) => visit(k)); } catch (e) { log.push('E'); }",
  forof: "try { for (const x of c) visit(Array.isArray(x) ? x[0] : x); } catch (e) { log.push('E'); }",
  keys: "try { for (const k of c.keys()) visit(k); } catch (e) { log.push('E'); }",
  manual:
    "const i = c.entries(); try { for (;;) { const r = i.next(); if (r.done) { log.push('done'); break; } visit(r.value[0]); } } catch (e) { log.push('E'); } ad(100); log.push(i.next().done);",
};
for (const C of ["Map", "Set"]) {
  for (const it of ["forEach", "forof", "manual"]) {
    for (const m of Object.keys(MUT)) {
      for (const when of [1, 2]) {
        T(
          `const c = new ${C}(${C === "Map" ? "[[1, 'a'], [2, 'b'], [3, 'c']]" : "[1, 2, 3]"}); const log = []; let n = 0, done = false;` +
            ` const ad = x => ${C === "Map" ? "c.set(x, 'n' + x)" : "c.add(x)"};` +
            ` const mut = k => { ${MUT[m]} };` +
            ` const visit = k => { log.push(k); if (k === ${when} && !done) { done = true; mut(k); } if (++n > 12) throw new Error('loop'); };` +
            ` ${ITER[it]} return [log, [...c.keys()]];`,
        );
      }
    }
  }
  for (const m of Object.keys(MUT)) {
    T(
      `const c = new ${C}(${C === "Map" ? "[[1, 'a'], [2, 'b'], [3, 'c']]" : "[1, 2, 3]"}); const log = []; let n = 0, done = false;` +
        ` const ad = x => ${C === "Map" ? "c.set(x, 'n' + x)" : "c.add(x)"};` +
        ` const mut = k => { ${MUT[m]} };` +
        ` const visit = k => { log.push(k); if (k === 2 && !done) { done = true; mut(k); } if (++n > 12) throw new Error('loop'); };` +
        ` ${ITER.keys} return [log, [...c.keys()]];`,
    );
  }
}

// ---- B. Igualdade de chaves (SameValueZero): valor armazenado x valor consultado.
const VALS = ["0", "-0", "NaN", "'0'", "0n", "null", "undefined", "''"];
for (const a of VALS) {
  for (const b of VALS) {
    T(`const m = new Map(); m.set(${a}, 'v'); return [m.has(${b}), m.get(${b}), [...m.keys()]];`);
    T(`const s = new Set([${a}]); s.add(${b}); return [s.size, [...s]];`);
  }
}

// ---- C. Chaves objeto/símbolo em Map e Set.
const KEYS = [
  "{}", "[]", "function () {}", "Symbol('a')", "Symbol.for('a')", "Symbol.iterator", "Symbol()", "Object(1)", "new String('a')",
];
for (const k of KEYS) {
  T(`const k = ${k}; const m = new Map([[k, 1]]); return [m.get(k), m.has(k), m.has(${k}), m.delete(k), m.size];`);
  T(`const k = ${k}; const s = new Set([k, k, ${k}]); return [s.size, s.has(k), [...s].indexOf(k)];`);
}
T("const a = Symbol('a'), b = Symbol('a'); const m = new Map([[a, 1], [b, 2]]); return [m.size, m.get(a), m.get(b), m.get(Symbol.for('a'))];");
T("const m = new Map([[Symbol.for('a'), 1]]); return [m.get(Symbol.for('a')), m.has(Symbol('a'))];");
T("const m = new Map([['Symbol(a)', 1]]); return [m.has(Symbol('a')), m.get('Symbol(a)')];");
T("const m = new Map(); m.set(NaN, 1).set(NaN, 2); return [m.size, m.get(NaN)];");
T("const m = new Map(); m.set(-0, 1); return [Object.is([...m.keys()][0], 0), m.get(0), m.get(-0)];");
T("const s = new Set(); s.add(-0); return [Object.is([...s][0], 0), Object.is([...s.entries()][0][1], 0)];");
T("const m = new Map([[0, 'a']]); m.forEach((v, k) => { globalThis.K = Object.is(k, 0); }); return globalThis.K;");

// ---- D. WeakMap/WeakSet/WeakRef/FinalizationRegistry: tipo de chave x operação.
const WK = ["{}", "function () {}", "[]", "Symbol('x')", "Symbol.for('x')", "Symbol.iterator", "Symbol()", "1", "'a'", "null", "undefined", "1n", "true"];
const WOPS = [
  "const w = new WeakMap(); w.set(k, 1); return [w.get(k), w.has(k), w.delete(k), w.has(k)];",
  "const w = new WeakMap(); return [w.get(k), w.has(k), w.delete(k)];",
  "const s = new WeakSet(); s.add(k); return [s.has(k), s.delete(k), s.has(k)];",
  "const s = new WeakSet(); return [s.has(k), s.delete(k)];",
  "new WeakMap([[k, 1]]); return 'ok';",
  "new WeakSet([k]); return 'ok';",
  "return new WeakRef(k).deref() === k;",
  "const r = new FinalizationRegistry(() => {}); return r.register(k, 1);",
  "const r = new FinalizationRegistry(() => {}); return r.register({}, 1, k);",
  "const r = new FinalizationRegistry(() => {}); r.register({}, 1, k); return r.unregister(k);",
  "const r = new FinalizationRegistry(() => {}); return r.unregister(k);",
  "const r = new FinalizationRegistry(() => {}); return r.register(k, k);",
];
for (const k of WK) for (const op of WOPS) T(`const k = ${k}; ${op}`);

// ---- E. WeakRef.
T("return [typeof WeakRef, WeakRef.length, WeakRef.name, WeakRef.prototype[Symbol.toStringTag]];");
T("return WeakRef({});");
T("return new WeakRef();");
T("return new WeakRef(undefined);");
T("return WeakRef.prototype.deref.call({});");
T("return WeakRef.prototype.deref.call(undefined);");
T("return WeakRef.prototype.deref.call(new WeakMap());");
T("const o = {}; const r = new WeakRef(o); return [r.deref() === o, r.deref() === r.deref()];");
T("const s = Symbol('q'); const r = new WeakRef(s); return r.deref() === s;");
T("const d = Object.getOwnPropertyDescriptor(WeakRef.prototype, 'deref'); return [d.writable, d.enumerable, d.configurable, d.value.name, d.value.length];");
T("return Object.getOwnPropertyNames(WeakRef.prototype).sort();");
T("return Object.getOwnPropertyNames(WeakRef).sort();");
T("class W extends WeakRef { constructor(o) { super(o); this.x = 1; } } const o = {}; const w = new W(o); return [w.deref() === o, w.x, w instanceof WeakRef];");
T("return Object.prototype.toString.call(new WeakRef({}));");
T("return Reflect.construct(WeakRef, [{}], Object).deref;");
T("const r = new WeakRef({}); return Object.keys(r).length;");
T("return Object.getOwnPropertyDescriptor(WeakRef.prototype, Symbol.toStringTag);");
T("return Object.getPrototypeOf(WeakRef) === Function.prototype;");

// ---- F. FinalizationRegistry: alvo x held x token.
const FT = ["{}", "Symbol('t')", "Symbol.for('t')", "1"];
const FH = ["same", "'h'", "undefined"];
const FK = ["undefined", "{}", "Symbol.for('k')", "1"];
for (const t of FT) {
  for (const h of FH) {
    for (const tk of FK) {
      T(`const t = ${t}; const r = new FinalizationRegistry(() => {}); return r.register(t, ${h === "same" ? "t" : h}, ${tk});`);
    }
  }
}
T("return [FinalizationRegistry.length, FinalizationRegistry.name, FinalizationRegistry.prototype[Symbol.toStringTag]];");
T("return FinalizationRegistry(() => {});");
T("return new FinalizationRegistry();");
T("return new FinalizationRegistry(1);");
T("return new FinalizationRegistry({});");
T("return new FinalizationRegistry(null);");
T("return typeof FinalizationRegistry.prototype.cleanupSome;");
T("return Object.getOwnPropertyNames(FinalizationRegistry.prototype).sort();");
T("return FinalizationRegistry.prototype.register.call({}, {}, 1);");
T("return FinalizationRegistry.prototype.unregister.call(new WeakMap(), {});");
T("const r = new FinalizationRegistry(() => {}); const tok = {}; r.register({}, 1, tok); r.register({}, 2, tok); return [r.unregister(tok), r.unregister(tok)];");
T("const r = new FinalizationRegistry(() => {}); const tok = Symbol('tok'); r.register({}, 1, tok); return [r.unregister(tok), r.unregister(Symbol('tok'))];");
T("const r = new FinalizationRegistry(() => {}); r.register({}, 1); return r.unregister({});");
T("const r = new FinalizationRegistry(() => {}); return r.register({});");
T("const r = new FinalizationRegistry(() => {}); return r.register();");
T("const r = new FinalizationRegistry(() => {}); return r.unregister();");
T("const r = new FinalizationRegistry(() => {}); return [r.register.length, r.unregister.length, r.register.name, r.unregister.name];");
T("class F extends FinalizationRegistry {} const f = new F(() => {}); return [f instanceof FinalizationRegistry, f.register({}, 1)];");
T("return Object.prototype.toString.call(new FinalizationRegistry(() => {}));");
T("return Reflect.construct(FinalizationRegistry, [() => {}], Object) instanceof Object;");

// ---- G. Registro de símbolos, description, well-known.
const SFOR = ["undefined", "null", "1", "-0", "true", "''", "'a'", "{ toString() { return 'ts'; } }", "{ toString() { throw new RangeError('boom'); } }", "Symbol('s')", "[1, 2]", "1n"];
for (const a of SFOR) {
  T(`const s = Symbol.for(${a}); return [typeof s, s.description, Symbol.keyFor(s)];`);
  T(`return Symbol.for(${a}) === Symbol.for(${a});`);
}
T("return Symbol.for();");
T("return Symbol.for().description;");
T("return [Symbol.for.length, Symbol.keyFor.length, Symbol.for.name, Symbol.keyFor.name];");
for (const a of ["undefined", "null", "1", "'a'", "{}", "Symbol", "Object(Symbol.for('o'))", "Object(Symbol('o'))", "Symbol.iterator"]) {
  T(`return Symbol.keyFor(${a});`);
}
T("return Symbol.keyFor();");
T("return new Symbol();");
T("return [Symbol().description, Symbol(undefined).description, Symbol('').description, Symbol(null).description, Symbol(0).description];");
T("return Symbol(Symbol.for('x').toString()).description;");
T("return Symbol(Symbol('x'));");
T("return [Symbol('a').toString(), Symbol().toString(), Symbol.for('a').toString(), Object(Symbol.for('a')).description, Object(Symbol.for('a')).toString()];");
T("const d = Object.getOwnPropertyDescriptor(Symbol.prototype, 'description'); return [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length];");
T("const g = Object.getOwnPropertyDescriptor(Symbol.prototype, 'description').get; return g.call({});");
T("const g = Object.getOwnPropertyDescriptor(Symbol.prototype, 'description').get; return g.call(undefined);");
T("const g = Object.getOwnPropertyDescriptor(Symbol.prototype, 'description').get; return [g.call(Symbol('z')), g.call(Object(Symbol('z')))];");
T("return Symbol.prototype.description;");
T("return Symbol.prototype.toString.call({});");
T("return Symbol.prototype.valueOf.call(1);");
T("return [Symbol.prototype[Symbol.toStringTag], typeof Symbol.prototype[Symbol.toPrimitive], Symbol.prototype[Symbol.toPrimitive].name];");
T("return Symbol.prototype[Symbol.toPrimitive].call(Symbol.for('p')) === Symbol.for('p');");
T("return `${Symbol('a')}`;");
T("return Symbol('a') + '';");
T("return +Symbol('a');");
T("return String(Symbol('a'));");
T("return [Symbol('a') == Symbol('a'), Symbol.for('a') == Symbol.for('a'), Object(Symbol.for('a')) == Symbol.for('a')];");
const WELL = ["asyncIterator", "hasInstance", "isConcatSpreadable", "iterator", "match", "matchAll", "replace", "search", "species", "split", "toPrimitive", "toStringTag", "unscopables"];
for (const w of WELL) {
  T(`const d = Object.getOwnPropertyDescriptor(Symbol, '${w}'); return [typeof d.value, d.writable, d.enumerable, d.configurable, d.value.description, d.value.toString()];`);
  T(`return [Symbol.keyFor(Symbol.${w}), Symbol.for('Symbol.${w}') === Symbol.${w}];`);
  T(`const k = Symbol.${w}; const w = new WeakMap(); w.set(k, 1); const s = new WeakSet([k]); return [w.get(k), s.has(k), new WeakRef(k).deref() === k];`);
  T(`const m = new Map([[Symbol.${w}, 1]]); return [m.get(Symbol.${w}), Symbol.${w} in Symbol.prototype || Object.getOwnPropertySymbols(Symbol.prototype).includes(Symbol.${w})];`);
}
T("return Object.getOwnPropertyNames(Symbol).filter(n => typeof Symbol[n] === 'symbol').sort();");
T("return Object.getOwnPropertyNames(Symbol).sort();");
T("return typeof Symbol.dispose + typeof Symbol.asyncDispose;");

// ---- H. Ordem de getOwnPropertySymbols e Reflect.ownKeys.
T("const a = Symbol('a'), b = Symbol('b'), c = Symbol('c'); const o = { [b]: 1, x: 1, [a]: 2, 1: 1, [c]: 3 }; return [Object.getOwnPropertySymbols(o).map(String), Reflect.ownKeys(o).map(String)];");
T("const a = Symbol('a'), b = Symbol('b'); const o = { [a]: 1, [b]: 2 }; delete o[a]; o[a] = 3; return Object.getOwnPropertySymbols(o).map(String);");
T("const a = Symbol('a'), b = Symbol('b'); const o = {}; Object.defineProperty(o, b, { value: 1 }); Object.defineProperty(o, a, { value: 2, enumerable: true }); return [Object.getOwnPropertySymbols(o).map(String), Object.keys(o).length, Object.assign({}, o)[a], Object.assign({}, o)[b]];");
T("const a = Symbol.for('a'), b = Symbol.for('b'); const o = { [b]: 1, [a]: 2 }; return Object.getOwnPropertySymbols(o).map(Symbol.keyFor);");
T("const o = { [Symbol.iterator]: 1, [Symbol.toStringTag]: 2, [Symbol.asyncIterator]: 3 }; return Object.getOwnPropertySymbols(o).map(String);");
T("const a = Symbol('a'); const o = { [a]: 1 }; const p = Object.create(o); return [Object.getOwnPropertySymbols(p).length, a in p, p[a]];");
T("const a = Symbol('a'), b = Symbol('b'); const o = { [a]: 1, [b]: 2 }; const c = { ...o }; return Object.getOwnPropertySymbols(c).map(String);");
T("const a = Symbol('a'); const o = { get [a]() { return 1; } }; const c = Object.assign({}, o); return [Object.getOwnPropertyDescriptor(c, a).value, typeof Object.getOwnPropertyDescriptor(c, a).get];");
T("const a = Symbol('a'); class C { static [a]() {} [a]() {} static s = 1; } return [Object.getOwnPropertySymbols(C).map(String), Object.getOwnPropertySymbols(C.prototype).map(String), Reflect.ownKeys(C).map(String)];");
T("return Object.getOwnPropertySymbols([]).length + Object.getOwnPropertySymbols(function () {}).length;");
T("return Object.getOwnPropertySymbols(Array.prototype).map(String);");
T("return Object.getOwnPropertySymbols(Map.prototype).map(String);");
T("return Object.getOwnPropertySymbols(Set.prototype).map(String);");
T("return Object.getOwnPropertySymbols(WeakMap.prototype).map(String);");
T("return [Object.getOwnPropertySymbols(Map).map(String), Object.getOwnPropertySymbols(Set).map(String), Object.getOwnPropertySymbols(Symbol.prototype).map(String)];");
T("return Object.getOwnPropertySymbols('abc').length;");
T("return Object.getOwnPropertySymbols(1).length;");
T("return Object.getOwnPropertySymbols(null);");
T("return Object.getOwnPropertySymbols(undefined);");
T("const a = Symbol('a'); const p = new Proxy({ [a]: 1 }, { ownKeys(t) { return [a, 'z']; } }); return [Object.getOwnPropertySymbols(p).map(String), Reflect.ownKeys(p).map(String)];");
T("const a = Symbol('a'); const o = Object.freeze({ [a]: 1 }); return [Object.getOwnPropertySymbols(o).length, Object.isFrozen(o)];");
T("const a = Symbol('a'); const o = { [a]: 1, b: 2 }; return [JSON.stringify(o), Object.entries(o).length, Object.getOwnPropertyNames(o)];");
T("const a = Symbol('a'); const o = { [a]: 1 }; for (const k in o) return 'enumerated'; return 'none';");
T("const syms = []; for (let i = 0; i < 5; i++) syms.push(Symbol('s' + i)); const o = {}; for (const s of [syms[3], syms[1], syms[4], syms[0], syms[2]]) o[s] = 1; return Object.getOwnPropertySymbols(o).map(s => s.description);");
T("const o = {}; const a = Symbol(); o[a] = 1; return [Object.getOwnPropertySymbols(o)[0] === a, String(a), a.description];");

// ---- I. Subclasses, species, constructor com iterável.
T("class M extends Map { set(k, v) { (this.log = this.log || []).push(k); return super.set(k, v); } } const m = new M([[1, 'a'], [2, 'b']]); return [m.log, m.size];");
T("class S extends Set { add(v) { (this.log = this.log || []).push(v); return super.add(v); } } const s = new S([1, 2, 2]); return [s.log, s.size];");
T("class M extends Map { set() { return this; } } const m = new M([[1, 'a']]); return [m.size, Map.prototype.get.call(m, 1)];");
T("const orig = Map.prototype.set; Map.prototype.set = 1; try { return new Map([[1, 2]]); } finally { Map.prototype.set = orig; }");
T("const orig = Set.prototype.add; Set.prototype.add = undefined; try { return new Set([1]); } finally { Set.prototype.add = orig; }");
T("const orig = Set.prototype.add; Set.prototype.add = undefined; try { return new Set(); } finally { Set.prototype.add = orig; }");
T("const orig = WeakMap.prototype.set; WeakMap.prototype.set = null; try { return new WeakMap([[{}, 1]]); } finally { WeakMap.prototype.set = orig; }");
T("const orig = WeakSet.prototype.add; WeakSet.prototype.add = 5; try { return new WeakSet([{}]); } finally { WeakSet.prototype.add = orig; }");
T("return new Map([1]);");
T("return new Map([[1, 2], 3]);");
T("return new Map(5);");
T("return new Map({});");
T("return new Map(null).size + new Set(undefined).size;");
T("return new Map('ab');");
T("return new Set('abca').size;");
T("return new Map([['a']]).get('a');");
T("return Map({});");
T("return Set([]);");
T("return WeakMap();");
T("return WeakSet();");
T("return new WeakMap([1]);");
T("return new WeakMap([[1, 1]]);");
T("return new WeakSet([1]);");
T("return new WeakSet(1);");
for (const C of ["Map", "Set", "WeakMap", "WeakSet"]) {
  T(`const d = Object.getOwnPropertyDescriptor(${C}, Symbol.species); return [${C}[Symbol.species] === ${C}, d === undefined ? 'none' : [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name]];`);
  T(`class X extends ${C} {} return [X[Symbol.species] === X, Object.getPrototypeOf(X) === ${C}, new X() instanceof ${C}, X.name, X.length];`);
  T(`return [${C}.length, ${C}.name, ${C}.prototype[Symbol.toStringTag], Object.getPrototypeOf(${C}) === Function.prototype, ${C}.prototype.constructor === ${C}];`);
  T(`class X extends ${C} { constructor() { super(); this.tag = 1; } } const x = new X(); return [x.tag, Object.prototype.toString.call(x), Object.getPrototypeOf(x) === X.prototype];`);
  T(`return Reflect.construct(${C}, [], Object) instanceof ${C};`);
  T(`return Reflect.construct(${C}, [], function () {}.bind()) instanceof ${C};`);
  T(`function F() {} F.prototype = null; const o = Reflect.construct(${C}, [], F); return Object.getPrototypeOf(o) === ${C}.prototype;`);
  T(`const o = Object.create(${C}.prototype); return Object.prototype.toString.call(o) + String(o instanceof ${C});`);
}
T("class M extends Map { static get [Symbol.species]() { return Array; } } const m = new M([[1, 2]]); return [m instanceof M, Map.groupBy([1, 2], x => x % 2) instanceof Map, M.groupBy([1], x => x) instanceof M];");
T("return [Map.groupBy([1, 2, 3], x => x % 2).get(1), Map.groupBy([], x => x).size];");
T("return Map.groupBy(1, x => x);");
T("return Map.groupBy([1], 1);");
T("return Object.getOwnPropertyNames(Map).sort();");
T("return Object.getOwnPropertyNames(Set).sort();");
T("return Object.getOwnPropertyNames(Map.prototype).sort();");
T("return Object.getOwnPropertyNames(Set.prototype).sort();");
T("return Object.getOwnPropertyNames(WeakMap.prototype).sort();");
T("return Object.getOwnPropertyNames(WeakSet.prototype).sort();");
T("return [Map.prototype.entries === Map.prototype[Symbol.iterator], Set.prototype.values === Set.prototype[Symbol.iterator], Set.prototype.keys === Set.prototype.values];");

// ---- J. this inválido por método e classe.
const MM = ["get", "set", "has", "delete", "clear", "forEach", "entries", "keys", "values"];
const SM = ["add", "has", "delete", "clear", "forEach", "entries", "keys", "values"];
const WMM = ["get", "set", "has", "delete"];
const WSM = ["add", "has", "delete"];
const thisVariants = ["undefined", "{}", "OTHER"];
const groups = [
  ["Map", MM, "new Set()"],
  ["Set", SM, "new Map()"],
  ["WeakMap", WMM, "new WeakSet()"],
  ["WeakSet", WSM, "new WeakMap()"],
];
for (const [C, ms, other] of groups) {
  for (const m of ms) {
    for (const t of thisVariants) {
      T(`return ${C}.prototype.${m}.call(${t === "OTHER" ? other : t}, {}, 1);`);
    }
  }
}
for (const C of ["Map", "Set"]) {
  T(`return ${C}.prototype.size;`);
  T(`return Object.getOwnPropertyDescriptor(${C}.prototype, 'size').get.call({});`);
  T(`const d = Object.getOwnPropertyDescriptor(${C}.prototype, 'size'); return [typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length, 'value' in d];`);
  T(`const x = new ${C}([1, 2, 3]); x.delete(1); return [x.size, Object.getOwnPropertyDescriptor(x, 'size')];`);
  T(`class X extends ${C} { get size() { return 99; } } const x = new X([1]); return [x.size, Object.getOwnPropertyDescriptor(${C}.prototype, 'size').get.call(x)];`);
  T(`const x = new ${C}(); x.size = 5; return x.size;`);
  T(`'use strict'; const x = new ${C}(); try { x.size = 5; } catch (e) { return e.name + ': ' + e.message; } return 'no throw';`);
  T(`const x = new ${C}(); Object.defineProperty(x, 'size', { value: 7 }); return [x.size, Object.getOwnPropertyDescriptor(${C}.prototype, 'size').get.call(x)];`);
  T(`return ${C}.prototype.forEach.call(new ${C}([1]), 1);`);
  T(`return new ${C}().forEach(undefined);`);
  T(`return new ${C}([1]).forEach(null);`);
  T(`return new ${C}([1]).forEach({});`);
  T(`const x = new ${C}([1]); const out = []; x.forEach(function (...a) { out.push(this === undefined ? 'u' : typeof this, a.length); }); return out;`);
  T(`const x = new ${C}([1]); const t = {}; let ok; x.forEach(function () { ok = this === t; }, t); return ok;`);
  T(`const x = new ${C}([1]); let ok; x.forEach(() => { ok = this === globalThis; }); return typeof ok;`);
  T(`const it = new ${C}([1]).values(); const p = Object.getPrototypeOf(it); return [p[Symbol.toStringTag], Object.prototype.toString.call(it), typeof p.next, p.next.length, Object.getOwnPropertyNames(p).sort(), Object.getPrototypeOf(p) === Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))];`);
  T(`const it = new ${C}([1]).values(); return Object.getPrototypeOf(it).next.call({});`);
  T(`const it = new ${C}([1]).values(); return it[Symbol.iterator]() === it;`);
  T(`const x = new ${C}([1, 2]); const a = x.values(); a.next(); return [[...a], [...a], a.next()];`);
}

// ---- Execução.
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "weak-more-golden-"));
// `vm.runInThisContext` roda como ProgramExecutable do JSC puro (sem o transpilador do bun sobre o arquivo).
const source_file = path.join(dir, "weak_more_source.js");
const file = path.join(dir, "weak_more_case.js");
fs.writeFileSync(
  file,
  `try { require("node:vm").runInThisContext(require("node:fs").readFileSync(${JSON.stringify(source_file)}, "utf8")) } catch (e) {}\n`,
);
const preload = path.join(dir, "preload.js");
fs.writeFileSync(
  preload,
  "process.on('exit', () => { process.stdout.write('\\u0001' + JSON.stringify(globalThis.R === undefined ? '<undefined>' : String(globalThis.R)) + '\\n') })\n",
);
const prefix = dir + "/";
const seen = new Set();
let kept = 0;
let dropped = 0;
// API de host (fora do JSC) não entra na coluna do programa.
const HOST = /(?<![.\w$])(setTimeout|setInterval|setImmediate|queueMicrotask|structuredClone|process|require|console|Bun|URL|Buffer|atob|btoa|TextDecoder|TextEncoder|AbortController|fetch|performance|gc)(?![\w$])/;
for (const body of programs) {
  if (HOST.test(body)) continue;
  const source = PRELUDE + body;
  if (seen.has(source)) continue;
  seen.add(source);
  fs.writeFileSync(source_file, source);
  const run = spawnSync(process.execPath, ["--preload", preload, file], { encoding: "utf8", cwd: dir, timeout: 10000 });
  const marked = (run.stdout || "").split("\n").find(line => line.startsWith("\u0001"));
  if (!marked) {
    dropped++;
    process.stderr.write("sem resultado para: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  const result = JSON.parse(marked.slice(1)).split("file://" + prefix).join("file:///").split(prefix).join("");
  if (result.includes(dir) || /\/home\/|\/tmp\/|\/Users\//.test(result)) {
    dropped++;
    process.stderr.write("caminho da máquina no resultado: " + JSON.stringify(body.slice(0, 160)) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
fs.rmSync(dir, { recursive: true, force: true });
