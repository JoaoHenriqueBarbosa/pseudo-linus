// Gera tests/golden/weak_symbol_bun.tsv: parte síncrona e determinística de WeakRef, FinalizationRegistry, WeakMap e
// WeakSet com chaves symbol, medida no bun 1.4.2. Nada depende de o coletor de lixo rodar.
// Cobre construtores com argumentos inválidos (TypeError exato, Symbol.for registrado como chave proibida, símbolo
// comum e well-known permitidos), deref, register/unregister com token, register com alvo igual ao held, cleanupSome
// (se existir), Symbol.for/keyFor em grade (strings estranhas, coerção, valueOf que lança) e a descrição de Symbol().
// Colunas do tsv: `JSON(sufixo)<TAB>JSON(resultado)[<TAB>índice]`, fatorado por scripts/golden-prelude.js.
// Cada programa roda num bun filho novo (a tabela estática do JSC reifica por ordem de acesso), no máximo 6 em
// paralelo, cada um com timeout de 8 s. O resultado é o texto da variável global `R`.
// Uso: bun scripts/gen-weak-symbol-golden.js > tests/golden/weak_symbol_bun.tsv
const { emitFactoredLines, knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const { usesHostApi } = require("./host-api.js");
const fs = require("fs");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  process.exit(0);
}
const PRELOAD = writeResultPreload();

// S mostra o valor; texto fora do ASCII imprimível vira <U+XXXX> para o tsv ficar sem caractere de controle.
const PRELUDE =
  'var E=s=>s.replace(/[^\\x20-\\x7e]/g,c=>"<U+"+c.charCodeAt(0).toString(16).toUpperCase()+">"),' +
  'S=v=>typeof v=="symbol"?E(String(v)):typeof v=="bigint"?v+"n":Object.is(v,-0)?"-0":typeof v=="string"?E(JSON.stringify(v)):typeof v=="function"?"fn:"+E(v.name):' +
  'v&&typeof v=="object"?(Array.isArray(v)?"["+v.map(S).join()+"]":E(Object.prototype.toString.call(v))):String(v),' +
  'T=f=>{try{return S(f())}catch(e){return"!"+e.name+":"+E(e.message)}};\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const T = (body) => `T(()=>${body})`;

const WELL_KNOWN = [
  "asyncIterator", "hasInstance", "isConcatSpreadable", "iterator", "match", "matchAll", "replace", "search", "species",
  "split", "toPrimitive", "toStringTag", "unscopables", "dispose", "asyncDispose",
].map((name) => `Symbol.${name}`);

// Valores candidatos a chave, alvo ou token.
const symbols = [
  "Symbol()", "Symbol('')", "Symbol('a')", "Symbol(undefined)", "Symbol(null)", "Symbol(0)", "Symbol({})", "Symbol.for('a')", "Symbol.for('')",
  "Symbol.for('registered')", "Symbol.for(String.fromCharCode(8212))", "Symbol.for('Symbol.iterator')", "Symbol.for('x').valueOf()", "Object(Symbol())", "Object(Symbol.for('o'))",
  ...WELL_KNOWN,
];
const others = [
  "undefined", "null", "0", "-0", "1", "NaN", "''", "'a'", "true", "false", "1n", "{}", "[]", "()=>1", "function(){}", "class{}", "Object(1)", "Object('s')", "new Date(0)", "/x/",
  "new Map", "new WeakMap", "new WeakRef({})", "Object.create(null)", "Math", "JSON", "globalThis", "Reflect", "Proxy", "new Proxy({}, {})", "new Proxy(function(){}, {})", "Atomics", "new Error('e')",
  "new Uint8Array(1)", "Symbol", "Symbol.prototype", "WeakRef", "WeakRef.prototype", "FinalizationRegistry",
];
const values = [...symbols, ...others];

// ---- 1. Cada valor × cada operação das quatro estruturas.
const perValue = [
  (v) => `new WeakRef(${v})`,
  (v) => `typeof new WeakRef(${v})`,
  (v) => `new WeakRef(${v}).deref()===${v}`,
  (v) => `S(new WeakRef(${v}).deref())`,
  (v) => `WeakRef(${v})`,
  (v) => `new WeakMap().set(${v},1)`,
  (v) => `new WeakMap().set(${v},1).get(${v})`,
  (v) => `new WeakMap().set(${v},1).has(${v})`,
  (v) => `{var m=new WeakMap([[${v},7]]);return m.get(${v})+','+m.has(${v})+','+m.delete(${v})+','+m.has(${v})}`,
  (v) => `new WeakMap().has(${v})`,
  (v) => `new WeakMap().get(${v})`,
  (v) => `new WeakMap().delete(${v})`,
  (v) => `new WeakMap([[${v},1]])`,
  (v) => `new WeakMap([[{},1],[${v},2]])`,
  (v) => `new WeakMap().set({},${v}).has({})`,
  (v) => `new WeakSet().add(${v})`,
  (v) => `new WeakSet().add(${v}).has(${v})`,
  (v) => `{var s=new WeakSet([${v}]);return s.has(${v})+','+s.delete(${v})+','+s.has(${v})}`,
  (v) => `new WeakSet().has(${v})`,
  (v) => `new WeakSet().delete(${v})`,
  (v) => `new WeakSet([{},${v}])`,
  (v) => `new FinalizationRegistry(()=>{}).register(${v},1)`,
  (v) => `new FinalizationRegistry(()=>{}).register({},1,${v})`,
  (v) => `new FinalizationRegistry(()=>{}).register({},${v})`,
  (v) => `new FinalizationRegistry(()=>{}).register({},${v},${v})`,
  (v) => `new FinalizationRegistry(()=>{}).register(${v},${v})`,
  (v) => `new FinalizationRegistry(()=>{}).register(${v},1,${v})`,
  (v) => `new FinalizationRegistry(()=>{}).unregister(${v})`,
  (v) => `{var r=new FinalizationRegistry(()=>{});r.register({},1,${v});return r.unregister(${v})}`,
  (v) => `{var r=new FinalizationRegistry(()=>{});r.register({},1,${v});return r.unregister(${v})+','+r.unregister(${v})}`,
  (v) => `{var r=new FinalizationRegistry(()=>{});r.register({},1);return r.unregister(${v})}`,
  (v) => `{var r=new FinalizationRegistry(()=>{});var a={},b={};r.register(a,1,${v});r.register(b,2,${v});return r.unregister(${v})+','+r.unregister(${v})}`,
  (v) => `new FinalizationRegistry(${v})`,
  (v) => `FinalizationRegistry(${v})`,
  (v) => `new WeakMap(${v})`,
  (v) => `new WeakSet(${v})`,
  (v) => `new WeakRef(${v}) instanceof WeakRef`,
  (v) => `Object.prototype.toString.call(new WeakRef(${v}))`,
  (v) => `Reflect.construct(WeakRef,[${v}])`,
  (v) => `Reflect.apply(WeakMap.prototype.set,new WeakMap,[${v},1])`,
  (v) => `Reflect.apply(WeakSet.prototype.add,new WeakSet,[${v}])`,
  (v) => `WeakMap.prototype.set.call(${v},{},1)`,
  (v) => `WeakSet.prototype.add.call(${v},{})`,
  (v) => `WeakRef.prototype.deref.call(${v})`,
  (v) => `FinalizationRegistry.prototype.register.call(${v},{},1)`,
  (v) => `FinalizationRegistry.prototype.unregister.call(${v},{})`,
];
for (const v of values) for (const op of perValue) add(T(op(v)));

// ---- 2. Símbolos como chave: sequências e identidade.
for (const s of symbols) {
  add(
    T(`{var m=new WeakMap,k=${s};m.set(k,1);m.set(k,2);return m.get(k)}`),
    T(`{var s=new WeakSet,k=${s};s.add(k);s.add(k);return s.has(k)+','+s.delete(k)+','+s.delete(k)}`),
    T(`{var w=new WeakRef(${s});return w.deref()===w.deref()}`),
    T(`typeof new WeakRef(${s}).deref()`),
    T(`Object.getPrototypeOf(new WeakRef(${s}))===WeakRef.prototype`),
    T(`Reflect.ownKeys(new WeakRef(${s})).length`),
    T(`Object.keys(new WeakMap().set(${s},1)).length`),
    T(`new WeakMap().set(${s},1) instanceof WeakMap`),
    T(`{var a=${s};var m=new WeakMap([[a,1]]);return m.has(Object(a))+','+m.has(a.valueOf?a.valueOf():a)}`),
    T(`{var a=${s};var s=new WeakSet([a]);return s.has(typeof a=='symbol'?Object(a):a)}`),
    T(`{var r=new FinalizationRegistry(()=>{});var a=${s};return r.register(a,'held',a)}`),
    T(`{var r=new FinalizationRegistry(()=>{});var a=${s};r.register(a,'held',a);return r.unregister(a)}`),
    T(`{var r=new FinalizationRegistry(()=>{});var a=${s};r.register(a,'held');return r.unregister(a)}`),
    T(`{var r=new FinalizationRegistry(()=>{});return r.register({},1,${s})}`),
    T(`{var r=new FinalizationRegistry(()=>{});r.register({},1,${s});r.register({},2,${s});return r.unregister(${s})}`),
    T(`new WeakRef(new WeakRef(${s}))`),
    T(`new WeakRef(new WeakRef(${s})).deref().deref()===undefined`),
    T(`new WeakMap([[${s},1],[${s},2]]).get(${s})`),
    T(`new WeakSet([${s},${s}]).has(${s})`),
    T(`(m=>(m.set(${s},m),m.get(${s})===m))(new WeakMap)`),
    T(`Object.getOwnPropertyNames(WeakRef.prototype).join()`),
  );
}

// ---- 3. Forma dos construtores, protótipos e métodos.
add(
  T("WeakRef.length+','+WeakRef.name"), T("WeakMap.length+','+WeakMap.name"), T("WeakSet.length+','+WeakSet.name"), T("FinalizationRegistry.length+','+FinalizationRegistry.name"),
  T("Object.getOwnPropertyNames(WeakRef.prototype).join()"), T("Object.getOwnPropertyNames(WeakMap.prototype).join()"), T("Object.getOwnPropertyNames(WeakSet.prototype).join()"),
  T("Object.getOwnPropertyNames(FinalizationRegistry.prototype).join()"), T("Reflect.ownKeys(WeakRef.prototype).map(String).join()"), T("Reflect.ownKeys(FinalizationRegistry.prototype).map(String).join()"),
  T("Reflect.ownKeys(WeakMap.prototype).map(String).join()"), T("Reflect.ownKeys(WeakSet.prototype).map(String).join()"),
  T("WeakRef.prototype[Symbol.toStringTag]"), T("FinalizationRegistry.prototype[Symbol.toStringTag]"), T("WeakMap.prototype[Symbol.toStringTag]"), T("WeakSet.prototype[Symbol.toStringTag]"),
  T("JSON.stringify(Object.getOwnPropertyDescriptor(WeakRef.prototype,Symbol.toStringTag))"), T("JSON.stringify(Object.getOwnPropertyDescriptor(WeakRef.prototype,'deref'))"),
  T("JSON.stringify(Object.getOwnPropertyDescriptor(WeakRef,'prototype'))"), T("JSON.stringify(Object.getOwnPropertyDescriptor(FinalizationRegistry,'prototype'))"),
  T("JSON.stringify(Object.getOwnPropertyDescriptor(FinalizationRegistry.prototype,'register'))"), T("JSON.stringify(Object.getOwnPropertyDescriptor(FinalizationRegistry.prototype,'unregister'))"),
  T("WeakRef.prototype.deref.length+','+WeakRef.prototype.deref.name"), T("FinalizationRegistry.prototype.register.length+','+FinalizationRegistry.prototype.register.name"),
  T("FinalizationRegistry.prototype.unregister.length+','+FinalizationRegistry.prototype.unregister.name"), T("typeof FinalizationRegistry.prototype.cleanupSome"),
  T("FinalizationRegistry.prototype.hasOwnProperty('cleanupSome')"), T("'cleanupSome' in FinalizationRegistry.prototype"),
  T("WeakRef.prototype.constructor===WeakRef"), T("FinalizationRegistry.prototype.constructor===FinalizationRegistry"),
  T("Object.getPrototypeOf(WeakRef)===Function.prototype"), T("Object.getPrototypeOf(WeakRef.prototype)===Object.prototype"), T("Object.getPrototypeOf(FinalizationRegistry.prototype)===Object.prototype"),
  T("new WeakRef()"), T("new WeakRef"), T("WeakRef()"), T("WeakRef"), T("new FinalizationRegistry()"), T("new FinalizationRegistry"), T("FinalizationRegistry()"),
  T("new FinalizationRegistry(1)"), T("new FinalizationRegistry('f')"), T("new FinalizationRegistry({})"), T("new FinalizationRegistry(null)"), T("new FinalizationRegistry(undefined)"),
  T("new FinalizationRegistry(class{})"), T("new FinalizationRegistry(Symbol())"), T("new FinalizationRegistry(()=>{}) instanceof FinalizationRegistry"),
  T("new FinalizationRegistry(function(){}).register()"), T("new FinalizationRegistry(()=>{}).register({})"), T("new FinalizationRegistry(()=>{}).register({},undefined)"),
  T("new FinalizationRegistry(()=>{}).register({},1)"), T("new FinalizationRegistry(()=>{}).register({},1,undefined)"), T("new FinalizationRegistry(()=>{}).register({},1,null)"),
  T("new FinalizationRegistry(()=>{}).register({},1,1)"), T("new FinalizationRegistry(()=>{}).register({},1,'t')"), T("new FinalizationRegistry(()=>{}).register({},1,{})"),
  T("new FinalizationRegistry(()=>{}).unregister()"), T("new FinalizationRegistry(()=>{}).unregister(undefined)"), T("new FinalizationRegistry(()=>{}).unregister(null)"),
  T("new FinalizationRegistry(()=>{}).unregister({})"), T("new FinalizationRegistry(()=>{}).unregister(1)"), T("new FinalizationRegistry(()=>{}).unregister('t')"),
  T("{var t={},o={};var r=new FinalizationRegistry(()=>{});r.register(o,1,t);return r.unregister(t)+','+r.unregister(t)}"),
  T("{var o={};return new FinalizationRegistry(()=>{}).register(o,o)}"), T("{var o={};return new FinalizationRegistry(()=>{}).register(o,o,o)}"),
  T("{var o={};return new FinalizationRegistry(()=>{}).register(o,1,o)}"), T("{var o=function(){};return new FinalizationRegistry(()=>{}).register(o,o)}"),
  T("{var o=[];return new FinalizationRegistry(()=>{}).register(o,o)}"), T("{var o=Symbol();return new FinalizationRegistry(()=>{}).register(o,o)}"),
  T("{var o=Symbol.for('q');return new FinalizationRegistry(()=>{}).register(o,o)}"), T("{var o=Symbol.iterator;return new FinalizationRegistry(()=>{}).register(o,o)}"),
  T("{var o=Symbol();return new FinalizationRegistry(()=>{}).register(o,Symbol())}"), T("{var o=Symbol();return new FinalizationRegistry(()=>{}).register(o,o,o)}"),
  T("{var o=Symbol();return new FinalizationRegistry(()=>{}).register(o,NaN)}"), T("{var o={};return new FinalizationRegistry(()=>{}).register(o,NaN)}"),
  T("{var o={};return new FinalizationRegistry(()=>{}).register(o,Object(o))}"), T("{var o=Symbol();return new FinalizationRegistry(()=>{}).register(o,Object(o))}"),
  T("{var o=Symbol();return new FinalizationRegistry(()=>{}).register(o,{o})}"), T("{var o={};return new FinalizationRegistry(()=>{}).register(o,[o])}"),
  T("{var o={};var r=new FinalizationRegistry(()=>{});r.register(o,1);r.register(o,2);return r.unregister(o)}"),
  T("{var o={};var r=new FinalizationRegistry(()=>{});r.register(o,1,o);r.register(o,2,o);return r.unregister(o)+','+r.unregister(o)}"),
  T("{var a={},b={};var r=new FinalizationRegistry(()=>{});r.register(a,1,a);r.register(b,1,b);return r.unregister(a)+','+r.unregister(b)+','+r.unregister(a)}"),
  T("Reflect.construct(FinalizationRegistry,[()=>{}],Object) instanceof FinalizationRegistry"), T("Reflect.construct(WeakRef,[{}],Object) instanceof WeakRef"),
  T("Reflect.construct(WeakRef,[{}],Object)===undefined"), T("Reflect.construct(WeakRef,[{}],function(){}.bind())"), T("Reflect.construct(WeakRef,[{}],()=>{})"),
  T("{class A extends WeakRef{};var a=new A({});return a instanceof A&&a instanceof WeakRef&&typeof a.deref()}"),
  T("{class A extends WeakRef{};return new A(Symbol()).deref().toString()}"), T("{class A extends WeakRef{};return new A(Symbol.for('r'))}"),
  T("{class A extends FinalizationRegistry{};var a=new A(()=>{});return a instanceof A+','+a.register({},1)}"),
  T("{class A extends WeakMap{};return new A([[Symbol(),1]]) instanceof A}"), T("{class A extends WeakSet{};return new A([Symbol.for('a')]) instanceof A}"),
  T("{class A extends WeakMap{set(k,v){return super.set(k,v+1)}};var a=new A([[{},1]]);return a instanceof A}"),
  T("{class A extends WeakRef{constructor(){super()}};return new A}"), T("{class A extends WeakRef{constructor(){super(Symbol())}};return typeof new A().deref()}"),
  T("Object.setPrototypeOf(function(){},WeakRef) instanceof Function"),
  T("WeakRef.prototype.deref.call({})"), T("WeakRef.prototype.deref.call(new WeakMap)"), T("WeakRef.prototype.deref.call(null)"), T("WeakRef.prototype.deref.call(undefined)"),
  T("FinalizationRegistry.prototype.register.call(new WeakRef({}),{},1)"), T("FinalizationRegistry.prototype.unregister.call(new WeakMap,{})"),
  T("WeakMap.prototype.set.call(new WeakSet,{},1)"), T("WeakSet.prototype.add.call(new WeakMap,{})"), T("WeakMap.prototype.get.call(new Map,{})"),
  T("WeakMap.prototype.has.call({},{})"), T("WeakSet.prototype.has.call(new Set,{})"), T("WeakSet.prototype.delete.call([],{})"),
  T("WeakMap.prototype.set.length+','+WeakMap.prototype.get.length+','+WeakMap.prototype.has.length+','+WeakMap.prototype.delete.length"),
  T("WeakSet.prototype.add.length+','+WeakSet.prototype.has.length+','+WeakSet.prototype.delete.length"),
);

// ---- 4. Construtores de WeakMap/WeakSet com iteráveis inválidos ou com símbolos.
const iterables = [
  "undefined", "null", "0", "1", "''", "'ab'", "true", "{}", "[]", "[1]", "[[]]", "[[1,2]]", "[[{},1]]", "[[Symbol(),1]]", "[[Symbol.for('a'),1]]", "[[Symbol.iterator,1]]",
  "[[Symbol(),1],[Symbol(),2]]", "[[Symbol.for('a'),1],[Symbol(),2]]", "[[Symbol(),1],[Symbol.for('a'),2]]", "[Symbol()]", "[Symbol.for('a')]", "[Symbol.iterator]", "[{},Symbol()]",
  "[{},Symbol.for('z')]", "[Symbol(),{}]", "[null]", "[undefined]", "[[null,1]]", "[[undefined,1]]", "[['a',1]]", "[{0:{},1:2}]", "[{0:Symbol(),1:2}]", "new Set([Symbol()])", "new Set([Symbol.for('s')])",
  "new Map([[Symbol(),1]])", "new Map([[Symbol.for('m'),1]])", "new Map([[{},1]])", "new Map", "new Set", "(function*(){yield Symbol()})()", "(function*(){yield Symbol.for('g')})()",
  "(function*(){yield [Symbol(),1]})()", "(function*(){yield [Symbol.for('g'),1]})()", "(function*(){yield [{},1];throw new Error('boom')})()", "{[Symbol.iterator]:1}", "{[Symbol.iterator](){return 1}}",
  "{[Symbol.iterator](){return {}}}", "{[Symbol.iterator](){return {next(){return 1}}}}", "{[Symbol.iterator](){return {next(){return {done:true}}}}}",
  "{[Symbol.iterator](){return {next(){return {done:false,value:Symbol()}}}}}", "{[Symbol.iterator](){throw new RangeError('it')}}", "Symbol()", "Symbol.for('a')", "Object(Symbol())",
  "new WeakRef({})", "function(){}", "()=>[]", "new Uint8Array(2)", "new Uint8Array(0)", "arguments", "'\\ud800'",
];
for (const it of iterables) {
  add(
    T(`new WeakMap(${it})`), T(`new WeakSet(${it})`), T(`WeakMap(${it})`), T(`WeakSet(${it})`),
    T(`{var m=new WeakMap(${it});return m instanceof WeakMap}`),
    T(`{var L=[],o=WeakMap.prototype.set;WeakMap.prototype.set=function(k,v){L.push(typeof k);return o.call(this,k,v)};try{new WeakMap(${it})}catch(e){L.push(e.name)}finally{WeakMap.prototype.set=o}return L.join()}`),
    T(`{var L=[],o=WeakSet.prototype.add;WeakSet.prototype.add=function(k){L.push(typeof k);return o.call(this,k)};try{new WeakSet(${it})}catch(e){L.push(e.name)}finally{WeakSet.prototype.add=o}return L.join()}`),
  );
}
add(
  T("{var o=WeakMap.prototype.set;WeakMap.prototype.set=1;try{return new WeakMap([[{},1]])}finally{WeakMap.prototype.set=o}}"),
  T("{var o=WeakMap.prototype.set;WeakMap.prototype.set=1;try{return new WeakMap()instanceof WeakMap}finally{WeakMap.prototype.set=o}}"),
  T("{var o=WeakMap.prototype.set;WeakMap.prototype.set=1;try{return new WeakMap([])instanceof WeakMap}finally{WeakMap.prototype.set=o}}"),
  T("{var o=WeakSet.prototype.add;WeakSet.prototype.add=1;try{return new WeakSet([{}])}finally{WeakSet.prototype.add=o}}"),
  T("{var o=WeakSet.prototype.add;WeakSet.prototype.add=1;try{return new WeakSet(null)instanceof WeakSet}finally{WeakSet.prototype.add=o}}"),
  T("{var c=0,it={[Symbol.iterator](){return {next(){return {done:false,value:[Symbol(),1]}},return(){c++;return{}}}}};try{new WeakMap(it)}catch(e){}return c}"),
  T("{var c=0,it={[Symbol.iterator](){return {next(){return {done:false,value:1}},return(){c++;return{}}}}};try{new WeakMap(it)}catch(e){return e.name+c}}"),
  T("{var c=0,it={[Symbol.iterator](){return {next(){return {done:false,value:Symbol.for('x')}},return(){c++;return{}}}}};try{new WeakSet(it)}catch(e){return e.name+c}}"),
  T("{var c=0,it={[Symbol.iterator](){return {next(){return {done:false,value:Symbol.for('x')}},return(){c++;throw new EvalError('r')}}}};try{new WeakSet(it)}catch(e){return e.name+c}}"),
  T("{var c=0,it={[Symbol.iterator](){return {next(){return {done:false,value:Symbol()}},return(){c++;return{}}}}};var s=new WeakSet;var n=0;var o=WeakSet.prototype.add;WeakSet.prototype.add=function(x){if(++n>3)throw new SyntaxError('s');return o.call(this,x)};try{new WeakSet(it)}catch(e){return e.name+c+n}finally{WeakSet.prototype.add=o}}"),
);

// ---- 5. cleanupSome, se existir (o bun 1.4.2 decide o resultado).
add(
  T("typeof FinalizationRegistry.prototype.cleanupSome"), T("new FinalizationRegistry(()=>{}).cleanupSome"),
  T("FinalizationRegistry.prototype.cleanupSome&&new FinalizationRegistry(()=>{}).cleanupSome()"),
  T("FinalizationRegistry.prototype.cleanupSome&&new FinalizationRegistry(()=>{}).cleanupSome(()=>{})"),
  T("FinalizationRegistry.prototype.cleanupSome&&new FinalizationRegistry(()=>{}).cleanupSome(1)"),
  T("FinalizationRegistry.prototype.cleanupSome&&new FinalizationRegistry(()=>{}).cleanupSome({})"),
  T("FinalizationRegistry.prototype.cleanupSome&&new FinalizationRegistry(()=>{}).cleanupSome(null)"),
  T("FinalizationRegistry.prototype.cleanupSome&&new FinalizationRegistry(()=>{}).cleanupSome(undefined)"),
  T("FinalizationRegistry.prototype.cleanupSome&&FinalizationRegistry.prototype.cleanupSome.call({})"),
  T("FinalizationRegistry.prototype.cleanupSome&&FinalizationRegistry.prototype.cleanupSome.call(new WeakRef({}))"),
  T("FinalizationRegistry.prototype.cleanupSome&&FinalizationRegistry.prototype.cleanupSome.length"),
  T("FinalizationRegistry.prototype.cleanupSome&&FinalizationRegistry.prototype.cleanupSome.name"),
);

// ---- 6. Symbol.for / Symbol.keyFor em grade.
const strings = [
  "''", "' '", "'a'", "'A'", "'abc'", "'ab c'", "'undefined'", "'null'", "'NaN'", "'0'", "'-0'", "'1'", "'true'", "'constructor'", "'__proto__'", "'toString'", "'valueOf'", "'prototype'",
  "'Symbol.iterator'", "'Symbol(a)'", "'@@iterator'", "'\\0'", "'a\\0b'", "'\\n'", "'\\t'", "'\\ud800'", "'\\udc00'", "'\\ud83d\\ude00'", "'a\\ud800b'", "'\\u00e9'", "'e\\u0301'",
  "'\\u00e9'.normalize('NFD')", "'\\uffff'", "'\\ufeff'", "'\\u2028'", "'\\u2029'", "'\\u0085'", "String.fromCharCode(8212)", "String.fromCharCode(8211)", "'x'.repeat(1000)",
  "'\\u{10ffff}'", "'\\u{1f600}'", "'\\u200b'", "'\\u0130'", "'ß'", "'İ'.toLowerCase()", "'toString'+''", "String(Symbol.iterator)", "Symbol.iterator.description",
];
const forOps = [
  (s) => `Symbol.for(${s})`,
  (s) => `typeof Symbol.for(${s})`,
  (s) => `Symbol.for(${s})===Symbol.for(${s})`,
  (s) => `Symbol.for(${s})===Symbol(${s})`,
  (s) => `Symbol.keyFor(Symbol.for(${s}))`,
  (s) => `Symbol.keyFor(Symbol.for(${s}))===${s}`,
  (s) => `Symbol.keyFor(Symbol(${s}))`,
  (s) => `Symbol.for(${s}).description`,
  (s) => `Symbol.for(${s}).description===${s}`,
  (s) => `Symbol.for(${s}).toString()`,
  (s) => `String(Symbol.for(${s}))`,
  (s) => `Symbol.for(${s}).toString().length`,
  (s) => `Symbol(${s}).description`,
  (s) => `Symbol(${s}).toString()`,
  (s) => `Symbol(${s}).description===${s}`,
  (s) => `Symbol(${s})===Symbol(${s})`,
  (s) => `Object(Symbol.for(${s})).description`,
  (s) => `Object(Symbol(${s})).toString()`,
  (s) => `Symbol.for(${s}).valueOf()===Symbol.for(${s})`,
  (s) => `new WeakMap().set(Symbol.for(${s}),1)`,
  (s) => `new WeakSet().add(Symbol.for(${s}))`,
  (s) => `new WeakRef(Symbol.for(${s}))`,
  (s) => `new WeakMap().set(Symbol(${s}),1) instanceof WeakMap`,
  (s) => `new WeakSet().add(Symbol(${s})) instanceof WeakSet`,
  (s) => `new WeakRef(Symbol(${s})).deref().description===${s}`,
  (s) => `new FinalizationRegistry(()=>{}).register(Symbol.for(${s}),1)`,
  (s) => `new FinalizationRegistry(()=>{}).register({},1,Symbol.for(${s}))`,
  (s) => `new FinalizationRegistry(()=>{}).register(Symbol(${s}),1)`,
  (s) => `new FinalizationRegistry(()=>{}).unregister(Symbol.for(${s}))`,
  (s) => `{var k=Symbol(${s});var o={[k]:1};return Reflect.ownKeys(o).length+','+(k in o)}`,
  (s) => `{var k=Symbol.for(${s});var o={[k]:1};return Object.getOwnPropertySymbols(o)[0]===k}`,
  (s) => `JSON.stringify({[Symbol.for(${s})]:1})`,
  (s) => `Symbol.for(${s}).constructor===Symbol`,
  (s) => `Object.getPrototypeOf(Symbol.for(${s}))===Symbol.prototype`,
  (s) => `Symbol.keyFor(Object(Symbol.for(${s})))`,
  (s) => `Symbol.keyFor(Symbol.for(${s}).valueOf())`,
];
for (const s of strings) for (const op of forOps) add(T(forOps.length ? op(s) : ""));

// ---- 7. Coerção do argumento de Symbol.for, Symbol() e keyFor.
const coerce = [
  "undefined", "null", "0", "-0", "1", "1.5", "NaN", "Infinity", "true", "false", "1n", "[]", "[1]", "[1,2]", "[null]", "[[]]", "{}", "()=>1", "function f(){}", "class C{}", "Object('s')", "Object(1)",
  "new Date(0)", "/x/g", "new Error('m')", "Symbol()", "Symbol('d')", "Symbol.for('r')", "Symbol.iterator", "Object(Symbol())", "Object(Symbol.for('r'))",
  "{toString(){return 'ts'}}", "{valueOf(){return 'vo'}}", "{valueOf(){return 'vo'},toString(){return 'ts'}}", "{valueOf(){return 7},toString(){return {}}}", "{toString(){return {}},valueOf(){return {}}}",
  "{valueOf(){throw new RangeError('vo')}}", "{toString(){throw new RangeError('ts')}}", "{valueOf(){throw new RangeError('vo')},toString(){return 'ts'}}",
  "{toString(){throw new RangeError('ts')},valueOf(){return 'vo'}}", "{[Symbol.toPrimitive](){return 'tp'}}", "{[Symbol.toPrimitive](h){return h}}", "{[Symbol.toPrimitive](){throw new TypeError('tp')}}",
  "{[Symbol.toPrimitive](){return {}}}", "{[Symbol.toPrimitive](){return Symbol('inner')}}", "{[Symbol.toPrimitive]:1}", "{[Symbol.toPrimitive]:null,toString(){return 'n'}}",
  "{toString:null,valueOf:null}", "{toString:1}", "{get toString(){throw new SyntaxError('g')}}", "{get [Symbol.toPrimitive](){throw new SyntaxError('gp')}}",
  "new Proxy({}, {get(){throw new URIError('p')}})", "new Proxy({}, {})", "new Proxy(function(){}, {})", "Object.create(null)", "Object.create({toString(){return 'inh'}})",
  "{toString(){return Symbol('s')}}", "{toString(){return 1n}}", "{toString(){return null}}", "{toString(){return undefined}}",
];
const coerceOps = [
  (x) => `Symbol.for(${x})`,
  (x) => `Symbol.for(${x}).description`,
  (x) => `Symbol.keyFor(Symbol.for(${x}))`,
  (x) => `Symbol(${x})`,
  (x) => `Symbol(${x}).description`,
  (x) => `Symbol(${x}).toString()`,
  (x) => `Symbol.keyFor(${x})`,
  (x) => `Symbol.keyFor(Symbol(${x}))`,
  (x) => `{var L=[];var s=Symbol.for((L.push('a'),${x}));L.push('b');return L.join()}`,
  (x) => `{var L=[];try{Symbol.for(${x})}catch(e){L.push(e.name)}return L.join()+Symbol.for(${x}.x)}`,
  (x) => `new WeakMap().set(Symbol(${x}),1) instanceof WeakMap`,
  (x) => `new WeakRef(Symbol(${x})).deref().description`,
  (x) => `new WeakMap().set(Symbol.for(${x}),1)`,
  (x) => `new WeakSet().add(Symbol.for(${x}))`,
  (x) => `new FinalizationRegistry(()=>{}).register(Symbol.for(${x}),1)`,
  (x) => `Symbol(${x}).description===undefined`,
  (x) => `Symbol.prototype.toString.call(${x})`,
  (x) => `Symbol.prototype.valueOf.call(${x})`,
  (x) => `Symbol.prototype.description`,
  (x) => `Object.getOwnPropertyDescriptor(Symbol.prototype,'description').get.call(${x})`,
  (x) => `Symbol.prototype[Symbol.toPrimitive].call(${x})`,
  (x) => `Symbol.prototype[Symbol.toPrimitive].call(${x},'number')`,
];
for (const x of coerce) for (const op of coerceOps) add(T(op(x)));

// ---- 8. Descrição de Symbol() e forma da API de Symbol.
add(
  T("Symbol().description"), T("Symbol(undefined).description"), T("Symbol('').description"), T("Symbol(null).description"), T("Symbol().toString()"), T("Symbol('').toString()"),
  T("Object.getOwnPropertyNames(Symbol).join()"), T("Object.getOwnPropertyNames(Symbol.prototype).join()"), T("Reflect.ownKeys(Symbol.prototype).map(String).join()"),
  T("Symbol.length+','+Symbol.name"), T("Symbol.for.length+','+Symbol.for.name"), T("Symbol.keyFor.length+','+Symbol.keyFor.name"),
  T("new Symbol()"), T("new Symbol('x')"), T("Reflect.construct(Symbol,[])"), T("Symbol.prototype.constructor===Symbol"), T("Symbol.prototype[Symbol.toStringTag]"),
  T("Symbol.prototype[Symbol.toPrimitive].name"), T("Symbol.prototype[Symbol.toPrimitive].length"),
  T("JSON.stringify(Object.getOwnPropertyDescriptor(Symbol.prototype,Symbol.toPrimitive))"), T("JSON.stringify(Object.getOwnPropertyDescriptor(Symbol,'iterator'))"),
  T("Object.getOwnPropertyDescriptor(Symbol.prototype,'description').get.name"), T("typeof Object.getOwnPropertyDescriptor(Symbol.prototype,'description').set"),
  T("Symbol.for()"), T("Symbol.for().description"), T("Symbol.for(undefined)===Symbol.for('undefined')"), T("Symbol.for(null)===Symbol.for('null')"), T("Symbol.for(1)===Symbol.for('1')"),
  T("Symbol.keyFor()"), T("Symbol.keyFor(undefined)"), T("Symbol.keyFor(null)"), T("Symbol.keyFor('a')"), T("Symbol.keyFor(1)"), T("Symbol.keyFor({})"), T("Symbol.keyFor(Symbol())"),
  T("Symbol.keyFor(Symbol.iterator)"), T("Symbol.keyFor(Symbol.for('a'))"), T("Symbol.keyFor(Object(Symbol.for('a')))"), T("Symbol.keyFor(Object(Symbol()))"),
  T("Symbol.keyFor(Symbol.for('a'),1,2)"), T("Symbol.keyFor.call(null,Symbol.for('a'))"),
  T("Symbol.for('a')===Symbol.for.call(null,'a')"), T("Symbol.for.call(undefined,'a')===Symbol.for('a')"),
  T("Symbol.for('a')==Symbol.for('a')"), T("Symbol.for('a')==Object(Symbol.for('a'))"), T("Symbol.for('a')===Object(Symbol.for('a'))"),
  T("Symbol('a')==Symbol('a')"), T("Symbol.for('a')<Symbol.for('a')"), T("Symbol.for('a')+''"), T("`${Symbol.for('a')}`"), T("+Symbol.for('a')"), T("Symbol.for('a')+1"),
  T("Number(Symbol.for('a'))"), T("String(Symbol.for('a'))"), T("Boolean(Symbol.for('a'))"), T("!Symbol.for('a')"), T("Symbol.for('a')&&1"),
  T("[Symbol.for('a')].join()"), T("[Symbol.for('a')]+''"), T("'x'.concat(Symbol.for('a'))"), T("Symbol.for('a').toString.call(1)"),
  T("Object.getOwnPropertyNames(Object(Symbol.for('a'))).join()"), T("Object.keys(Object(Symbol.for('a'))).length"), T("typeof Object(Symbol.for('a'))"),
  T("Object(Symbol.for('a')) instanceof Symbol"), T("Symbol.for('a') instanceof Symbol"), T("Object.prototype.toString.call(Symbol.for('a'))"),
  T("Object.prototype.toString.call(Object(Symbol()))"), T("Object.is(Symbol.for('a'),Symbol.for('a'))"), T("Object.is(Symbol(),Symbol())"),
  T("{'use strict';var s=Symbol.for('a');s.x=1;return s.x}"), T("(()=>{'use strict';var s=Symbol.for('a');s.x=1})()"), T("Object.isFrozen(Symbol.for('a'))"),
  T("Symbol.for('a').length"), T("Symbol.for('a')[0]"), T("Symbol.for('a').constructor.name"), T("Symbol.for('a').hasOwnProperty('description')"), T("'description' in Symbol.for('a')"),
);

// ---- 9. Well-known symbols como chave fraca, em grade com as operações e com as propriedades do próprio símbolo.
for (const w of WELL_KNOWN) {
  add(
    T(`typeof ${w}`), T(`${w}.description`), T(`${w}.toString()`), T(`Symbol.keyFor(${w})`), T(`${w}===Symbol.for(${w}.description)`), T(`Symbol.for(${w}.description)===Symbol.for(${w}.description)`),
    T(`Object.getOwnPropertyDescriptor(Symbol,'${w.slice(7)}')&&JSON.stringify(Object.getOwnPropertyDescriptor(Symbol,'${w.slice(7)}'))`),
    T(`new WeakMap().set(${w},${w}).get(${w})===${w}`), T(`{var s=new WeakSet;s.add(${w});return s.has(${w})+','+s.delete(${w})+','+s.has(${w})}`),
    T(`new WeakRef(${w}).deref()===${w}`), T(`new WeakRef(${w}).deref().description`),
    T(`{var r=new FinalizationRegistry(()=>{});r.register(${w},'h');r.register({},'h',${w});return r.unregister(${w})}`),
    T(`new FinalizationRegistry(()=>{}).register({},${w})`), T(`new FinalizationRegistry(()=>{}).register(${w},${w})`),
    T(`new WeakMap([[${w},1]]).has(${w})`), T(`new WeakSet([${w}]).has(${w})`),
    T(`Reflect.ownKeys(Symbol).includes('${w.slice(7)}')`),
  );
}
// Pares de well-known: identidade entre eles e como par chave/valor.
for (const a of WELL_KNOWN) for (const b of WELL_KNOWN) {
  add(T(`new WeakMap([[${a},${b}]]).get(${a})===${b}`), T(`new WeakMap([[${a},1],[${b},2]]).get(${b})`));
}

// ---- 10. Registros e chaves misturados: strings do tipo Symbol.for em objeto e símbolo comum vs. registrado.
const mixKinds = ["Symbol()", "Symbol.for('k')", "Symbol.iterator", "{}", "function(){}", "[]"];
for (const a of mixKinds) for (const b of mixKinds) {
  add(
    T(`{var m=new WeakMap;m.set(${a},1);return m.set(${b},2)instanceof WeakMap}`),
    T(`{var s=new WeakSet;s.add(${a});return s.add(${b})instanceof WeakSet}`),
    T(`new FinalizationRegistry(()=>{}).register(${a},1,${b})`),
    T(`{var t=${b};var r=new FinalizationRegistry(()=>{});r.register(${a},1,t);return r.unregister(t)}`),
    T(`{var x=${a};return new FinalizationRegistry(()=>{}).register(x,${b})}`),
    T(`{var x=${a};return new FinalizationRegistry(()=>{}).register(x,x,${b})}`),
    T(`{var x=${a};var w=new WeakRef(x);return new WeakMap().set(w,${b}).get(w)!==undefined}`),
  );
}
// Reuso de um mesmo alvo em registros diferentes e com tokens diferentes.
for (const a of ["{}", "Symbol()", "Symbol.iterator"]) {
  add(
    T(`{var x=${a};var r=new FinalizationRegistry(()=>{});r.register(x,1);r.register(x,1);return r.unregister(x)}`),
    T(`{var x=${a};var r=new FinalizationRegistry(()=>{}),q=new FinalizationRegistry(()=>{});r.register(x,x)}`),
    T(`{var x=${a};var r=new FinalizationRegistry(()=>{}),q=new FinalizationRegistry(()=>{});r.register(x,1,x);q.register(x,1,x);return r.unregister(x)+','+q.unregister(x)}`),
    T(`{var x=${a};var r=new FinalizationRegistry(()=>{});r.register(x,1,x);return r.unregister(x)+','+r.unregister(x)}`),
  );
}

// ---- Filtro, dedup (contra os goldens vizinhos e contra si mesmo), execução.
exprs.splice(0, exprs.length, ...exprs.filter((p) => !usesHostApi(p)));
const baseSet = new Set(knownPrograms("weak_symbol_bun.tsv", (name) => /weak|symbol/i.test(name)));
const seen = new Set();
const unique = exprs.filter((e) => !seen.has(e) && seen.add(e));
const jobs = [];
let dup = 0;
for (const expr of unique) {
  const source = '"use strict";\n' + PRELUDE + `globalThis.R = ${expr}`;
  if (baseSet.has(source)) { dup++; continue; }
  jobs.push({ expr, source });
}

const MAX_CHILDREN = 6;
const TIMEOUT_MS = 8000;

const run = (job) =>
  new Promise((resolve) => {
    // Processo fresco por programa: o JSC reifica a tabela estática de Symbol, WeakRef etc. por ordem de acesso.
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"], env: { ...process.env, TZ: "America/Sao_Paulo" } });
    let out = "";
    let err = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), TIMEOUT_MS);
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (code) => { clearTimeout(timer); const decoded = code === 0 ? decodeResult(out) : null; resolve(decoded !== null ? { ok: true, out: decoded } : { ok: false, err: err || "código " + code }); });
    child.stdin.end(job.source);
  });

async function main() {
  if (process.env.COUNT) { console.error(jobs.length); return; }
  const results = new Array(jobs.length);
  let next = 0;
  const workers = Array.from({ length: MAX_CHILDREN }, async () => {
    while (next < jobs.length) {
      const i = next++;
      results[i] = await run(jobs[i]);
    }
  });
  await Promise.all(workers);
  let kept = 0;
  let dropped = 0;
  const lines = [];
  jobs.forEach((job, i) => {
    const r = results[i];
    if (!r.ok) { dropped++; process.stderr.write("erro de programa: " + JSON.stringify(job.expr).slice(0, 160) + " " + r.err.slice(0, 120) + "\n"); return; }
    const line = JSON.stringify(job.source) + "\t" + JSON.stringify(r.out);
    if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(r.out) || /[–—]/.test(line)) {
      dropped++;
      process.stderr.write("caminho, marca ou travessão: " + JSON.stringify(job.expr).slice(0, 160) + "\n");
      return;
    }
    kept++;
    lines.push(line);
  });
  process.stdout.write(emitFactoredLines("weak_symbol", lines));
  process.stderr.write(`mantidos ${kept}, descartados ${dropped}, repetidos dos goldens existentes ${dup}\n`);
}
main();
