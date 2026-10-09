// Gera tests/golden/stack_overflow_bun.tsv: estouro de pilha (`RangeError: Maximum call stack size exceeded.`) por
// recursão indireta, medido no bun 1.4.2. Cada programa captura o erro e devolve só `name: message`, nunca a
// profundidade em que estourou. Dois regimes:
//   1. recursão infinita (`go(Infinity)`), que estoura de forma determinística em qualquer motor;
//   2. profundidade claramente segura (5 e 1000), que devolve `ok <valor>`.
// Os mecanismos de recursão: getters, setters, toString, valueOf, Symbol.toPrimitive, traps de Proxy, replacer e
// reviver de JSON, comparador de sort, callbacks de array e de coleção, apply/call/bind encadeados, new e classes,
// geradores e async recursivos, RegExp com callback de replace e exec customizado, cadeias de closures construídas
// em laço (toString, getter, Proxy de Proxy, bind de bind) e estruturas aninhadas grandes (JSON.stringify de 100000
// níveis, flat(Infinity), toString/join de array aninhado). Cada programa roda num bun filho novo (timeout 10 s),
// sem APIs de host, e grava o resultado em `globalThis.R`. Programas cujo texto já aparece nos goldens existentes
// que falam de pilha são descartados.
// Colunas: a fonte do programa (JSON) e o valor de `R` (JSON).
// Uso: bun scripts/gen-stack-overflow-golden.js > tests/golden/stack_overflow_bun.tsv
const fs = require("fs");
const { emitRow, knownPrograms, writeResultPreload, decodeResult } = require("./golden-prelude.js");
const path = require("path");
const { spawn } = require("child_process");

// O filho só avalia o programa; quem imprime `R` é o preload (ver `writeResultPreload` em golden-prelude.js).
if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  setTimeout(() => {
    process.exit(0);
  }, 0);
  return;
}
const PRELOAD = writeResultPreload();

const MSG = "RangeError: Maximum call stack size exceeded.";

// ---- Corpos de recursão: cada um usa `n` e `go`, e chama `go(n-1)` pelo mecanismo medido.
const bodies = [];
const body = (name, text) => bodies.push([name, text]);

// Getters.
const getters = {
  literal: "return ({get x(){return go(n-1)}}).x",
  define_property: "var o={};Object.defineProperty(o,'x',{get(){return go(n-1)}});return o.x",
  class: "class K{get x(){return go(n-1)}}return new K().x",
  static: "class K{static get x(){return go(n-1)}}return K.x",
  inherited: "var p={get x(){return go(n-1)}};return Object.create(p).x",
  reflect_get: "return Reflect.get({get x(){return go(n-1)}},'x')",
  with_scope: "with({get x(){return go(n-1)}}){return x}",
  destructure: "var {x}={get x(){return go(n-1)}};return x",
  spread: "return {...{get x(){return go(n-1)}}}.x",
  assign: "return Object.assign({},{get x(){return go(n-1)}}).x",
  entries: "return Object.entries({get x(){return go(n-1)}})[0][1]",
  values: "return Object.values({get x(){return go(n-1)}})[0]",
  array_index: "var a=[];Object.defineProperty(a,0,{get(){return go(n-1)}});return a[0]",
  global: "Object.defineProperty(globalThis,'gg',{get(){return go(n-1)},configurable:true});return globalThis.gg",
  super: "class A{get x(){return go(n-1)}}class B extends A{get x(){return super.x}}return new B().x",
  template: "return `${{get x(){return go(n-1)}}.x}`",
  private: "class K{get #x(){return go(n-1)}m(){return this.#x}}return new K().m()",
  json: "return JSON.parse(JSON.stringify({get a(){return go(n-1)}})).a",
  create_null: "return Object.create(null,{x:{get(){return go(n-1)}}}).x",
  define_properties: "var o={};Object.defineProperties(o,{x:{get(){return go(n-1)}}});return o.x",
  descriptor_call: "return Object.getOwnPropertyDescriptor({get x(){return go(n-1)}},'x').get.call({})",
  length_getter: "var t;Array.prototype.forEach.call({get length(){t=go(n-1);return 0}},function(){});return t",
  index_getter: "return +Array.prototype.join.call({length:1,get 0(){return go(n-1)}})",
};
for (const [k, v] of Object.entries(getters)) body("getter_" + k, v);

// Setters.
const setters = {
  literal: "var t;({set x(v){t=go(n-1)}}).x=1;return t",
  class: "var t;class K{set x(v){t=go(n-1)}}new K().x=1;return t",
  static: "var t;class K{static set x(v){t=go(n-1)}}K.x=1;return t",
  inherited: "var t;var o=Object.create({set x(v){t=go(n-1)}});o.x=1;return t",
  reflect_set: "var t;Reflect.set({set x(v){t=go(n-1)}},'x',1);return t",
  compound: "var t;var o={get x(){return 0},set x(v){t=go(n-1)}};o.x+=1;return t",
  increment: "var t;var o={get x(){return 0},set x(v){t=go(n-1)}};o.x++;return t",
  destructure: "var t;var o={set x(v){t=go(n-1)}};[o.x]=[1];return t",
  object_destructure: "var t;var o={set x(v){t=go(n-1)}};({a:o.x}={a:1});return t",
  for_in: "var t;var o={set x(v){t=go(n-1)}};for(o.x in {a:1});return t",
  for_of: "var t;var o={set x(v){t=go(n-1)}};for(o.x of [1]);return t",
  assign: "var t;Object.assign({set x(v){t=go(n-1)}},{x:1});return t",
  array_index: "var t;var a=[];Object.defineProperty(a,0,{set(v){t=go(n-1)}});a[0]=1;return t",
  super: "var t;class A{set x(v){t=go(n-1)}}class B extends A{m(){super.x=1}}new B().m();return t",
  private: "var t;class K{set #x(v){t=go(n-1)}m(){this.#x=1}}new K().m();return t",
  define_property: "var t;var o={};Object.defineProperty(o,'x',{set(v){t=go(n-1)}});o.x=1;return t",
  with_scope: "var t;with({set x(v){t=go(n-1)}}){x=1}return t",
  logical_assign: "var t;var o={get x(){return 0},set x(v){t=go(n-1)}};o.x||=1;return t",
};
for (const [k, v] of Object.entries(setters)) body("setter_" + k, v);

// Conversões: o gancho (toString, valueOf, Symbol.toPrimitive) recursa e a conversão o dispara uma vez.
const hooks = {
  to_string: "toString(){return go(n-1)}",
  value_of: "valueOf(){return go(n-1)}",
  to_primitive: "[Symbol.toPrimitive](){return go(n-1)}",
};
const numeric = [
  "+o", "o*1", "-o", "o|0", "Number(o)", "Math.abs(o)", "o<1", "o==1", "isNaN(o)", "Math.max(o)", "new Date(o).getTime()",
  "'x'.repeat(o).length", "BigInt(o)", "o>>>0", "Math.floor(o)", "[1,2,3].at(o)", "'abc'.charAt(o)", "''+o", "o+1", "o+''",
  "o-0", "~o", "o**1", "o%7", "isFinite(o)", "Math.round(o)", "Math.sqrt(o)", "[1,2,3].slice(o).length",
];
const stringy = [
  "`${o}`", "String(o)", "[o].join()", "({})[o]", "parseInt(o)", "'a'.concat(o)", "escape(o)", "new String(o)", "parseFloat(o)",
  "'abc'.indexOf(o)", "Symbol.for(o)", "encodeURIComponent(o)", "new RegExp(o).source", "'abc'.includes(o)", "({})[o]=1",
  "o in {}", "Object.defineProperty({},o,{})", "Reflect.has({},o)", "Object.hasOwn({},o)", "Object.keys({[o]:1})",
  "'abc'.localeCompare(o)", "[1].join(o)", "''+o", "o+''", "o+1", "o==1", "new Function(o)", "'abc'.split(o)", "'abc'.replace(o,'')",
  "[1,2].includes(o)", "'abc'.startsWith(o)", "String.prototype.trim.call(o)", "`${o}`.length", "encodeURI(o)", "unescape(o)",
];
const shapes = {
  plain: (h) => `var o={${h}};`,
  klass: (h) => `class K{${h}}var o=new K();`,
  inherited: (h) => `var o=Object.create({${h}});`,
};
for (const [hookName, hook] of Object.entries(hooks)) {
  const convs = hookName === "value_of" ? numeric : hookName === "to_string" ? [...numeric, ...stringy] : [...numeric, ...stringy];
  const seen = new Set();
  convs.forEach((conv, index) => {
    if (seen.has(conv)) return;
    seen.add(conv);
    for (const [shapeName, shape] of Object.entries(shapes)) {
      if (shapeName !== "plain" && index % 3 !== 0) continue;
      body(`${hookName}_${shapeName}_${index}`, `${shape(hook)}return ${conv}`);
    }
  });
}
// toString devolve objeto, então o valueOf recursa.
body("to_string_then_value_of", "var o={toString(){return {}},valueOf(){return go(n-1)}};return ''+o");
body("value_of_then_to_string", "var o={valueOf(){return {}},toString(){return go(n-1)}};return `${o}`");
body("date_to_json", "var t;Date.prototype.toJSON.call({valueOf(){return 1},toISOString(){t=go(n-1);return ''}});return t");
body("array_to_string_join", "var t;Array.prototype.toString.call({join(){t=go(n-1);return ''}});return t");
body("array_to_locale", "var t;[{toLocaleString(){t=go(n-1);return ''}}].toLocaleString();return t");
body("object_to_locale", "var t;Object.prototype.toLocaleString.call({toString(){t=go(n-1);return ''}});return t");
body("bigint_value_of", "return Number(BigInt({valueOf(){return BigInt(go(n-1))}}))");
body("has_instance", "var t;1 instanceof {[Symbol.hasInstance](){t=go(n-1);return true}};return t");
body("species_getter", "var t;class A extends Array{static get [Symbol.species](){t=go(n-1);return Array}}new A().map(x=>x);return t");
body("concat_spreadable", "var t;[].concat({get [Symbol.isConcatSpreadable](){t=go(n-1);return false}});return t");
body("to_string_tag", "var t;Object.prototype.toString.call({get [Symbol.toStringTag](){t=go(n-1);return 'X'}});return t");
body("array_constructor_getter", "var t;var a=[1];Object.defineProperty(a,'constructor',{get(){t=go(n-1)}});a.map(x=>x);return t");
body("array_constructor_slice", "var t;var a=[1];Object.defineProperty(a,'constructor',{get(){t=go(n-1)}});a.slice();return t");
body("array_constructor_filter", "var t;var a=[1];Object.defineProperty(a,'constructor',{get(){t=go(n-1)}});a.filter(x=>x);return t");
body("typed_constructor_slice", "var t;var a=new Uint8Array(1);Object.defineProperty(a,'constructor',{get(){t=go(n-1)}});a.slice();return t");
body("typed_constructor_subarray", "var t;var a=new Uint8Array(1);Object.defineProperty(a,'constructor',{get(){t=go(n-1)}});a.subarray(0);return t");
body("array_buffer_constructor", "var t;var b=new ArrayBuffer(1);Object.defineProperty(b,'constructor',{get(){t=go(n-1)}});b.slice(0);return t");
body("promise_then_getter", "var t;Promise.resolve({get then(){t=go(n-1)}});return t");
body("promise_executor", "var t;new Promise(function(){t=go(n-1)});return t");
body("promise_executor_resolve", "var t;new Promise(function(res){res(t=go(n-1))});return t");

// Iteradores e protocolos.
body("iter_destructure", "var t;var [a]={[Symbol.iterator](){t=go(n-1);return [1][Symbol.iterator]()}};return t");
body("iter_spread", "var t;[...{[Symbol.iterator](){t=go(n-1);return [][Symbol.iterator]()}}];return t");
body("iter_for_of_next", "var t;for(var x of {[Symbol.iterator](){return {next(){t=go(n-1);return {done:true}}}}});return t");
body("iter_for_of_return", "var t;for(var x of {[Symbol.iterator](){return {next(){return {done:false,value:1}},return(){t=go(n-1);return {}}}}})break;return t");
body("iter_array_from", "var t;Array.from({[Symbol.iterator](){t=go(n-1);return [][Symbol.iterator]()}});return t");
body("iter_set", "var t;new Set({[Symbol.iterator](){t=go(n-1);return [][Symbol.iterator]()}});return t");
body("iter_map", "var t;new Map({[Symbol.iterator](){t=go(n-1);return [][Symbol.iterator]()}});return t");
body("iter_from_entries", "var t;Object.fromEntries({[Symbol.iterator](){t=go(n-1);return [][Symbol.iterator]()}});return t");
body("iter_math_max", "var t;Math.max(...{[Symbol.iterator](){t=go(n-1);return [][Symbol.iterator]()}});return t");
body("iter_getter", "var t;Array.from({get [Symbol.iterator](){t=go(n-1)}});return t");
body("iter_yield_star_custom", "var t;function*g(){yield*{[Symbol.iterator](){t=go(n-1);return [][Symbol.iterator]()}}}[...g()];return t");

// Proxy: cada trap recursa e a forma de uso o dispara.
const proxies = [
  ["get", "get(){t=go(n-1)}", ["p.x", "Reflect.get(p,'x')", "Object.create(p).x", "p['y']", "p[0]", "Object.prototype.toString.call(p)"]],
  ["has", "has(){t=go(n-1);return false}", ["'x' in p", "Reflect.has(p,'x')", "'x' in Object.create(p)", "(function(){with(p){return typeof x}})()"]],
  ["set", "set(){t=go(n-1);return true}", ["p.x=1", "Reflect.set(p,'x',1)", "Object.create(p).x=1", "p[0]=1"]],
  ["delete", "deleteProperty(){t=go(n-1);return true}", ["delete p.x", "Reflect.deleteProperty(p,'x')"]],
  ["own_keys", "ownKeys(){t=go(n-1);return []}", ["Object.keys(p)", "Object.getOwnPropertyNames(p)", "Reflect.ownKeys(p)", "Object.entries(p)", "JSON.stringify(p)", "({...p})", "Object.assign({},p)", "for(var k in p);", "Object.getOwnPropertySymbols(p)"]],
  ["gopd", "getOwnPropertyDescriptor(){t=go(n-1)}", ["Object.getOwnPropertyDescriptor(p,'x')", "Reflect.getOwnPropertyDescriptor(p,'x')", "Object.hasOwn(p,'x')", "p.hasOwnProperty('x')", "Object.prototype.propertyIsEnumerable.call(p,'x')"]],
  ["define", "defineProperty(){t=go(n-1);return true}", ["Object.defineProperty(p,'x',{value:1,configurable:true})", "Reflect.defineProperty(p,'x',{value:1,configurable:true})", "Object.defineProperties(p,{x:{value:1,configurable:true}})"]],
  ["get_proto", "getPrototypeOf(){t=go(n-1);return Object.prototype}", ["Object.getPrototypeOf(p)", "p.__proto__", "p instanceof Object", "Object.prototype.isPrototypeOf.call(Object.prototype,p)", "Reflect.getPrototypeOf(p)"]],
  ["set_proto", "setPrototypeOf(){t=go(n-1);return true}", ["Object.setPrototypeOf(p,Object.prototype)", "Reflect.setPrototypeOf(p,Object.prototype)", "p.__proto__=Object.prototype"]],
  ["is_extensible", "isExtensible(){t=go(n-1);return true}", ["Object.isExtensible(p)", "Object.isFrozen(p)"]],
  ["prevent_extensions", "preventExtensions(){t=go(n-1);return false}", ["Reflect.preventExtensions(p)"]],
  ["apply", "apply(){t=go(n-1)}", ["p()", "p.call(null)", "p.apply(null,[])", "Reflect.apply(p,null,[])", "[1].map(p)", "p.bind(null)()"], "function(){}"],
  ["construct", "construct(){t=go(n-1);return {}}", ["new p()", "Reflect.construct(p,[])"], "function(){}"],
];
for (const [name, handler, uses, target = "{}"] of proxies) {
  uses.forEach((use, index) => body(`proxy_${name}_${index}`, `var t;var p=new Proxy(${target},{${handler}});${use};return t`));
}
body("proxy_revocable_get", "var t;var r=Proxy.revocable({},{get(){t=go(n-1)}});r.proxy.x;return t");
body("proxy_handler_proxy_get", "var t;var h=new Proxy({},{get(){t=go(n-1);return undefined}});var p=new Proxy({},h);p.x;return t");
body("proxy_prototype_chain_get", "var t;var p=new Proxy({},{get(){t=go(n-1)}});class K{}Object.setPrototypeOf(K.prototype,p);new K().x;return t");

// JSON.
body("json_to_json", "return +JSON.stringify({toJSON(){return go(n-1)}})");
body("json_to_json_array", "return +JSON.stringify([{toJSON(){return go(n-1)}}])[1]");
body("json_replacer", "var t;JSON.stringify(1,function(k,v){t=go(n-1);return v});return t");
body("json_replacer_key", "var t;JSON.stringify({a:1},function(k,v){if(k==='a')t=go(n-1);return v});return t");
body("json_replacer_array_getter", "var t;JSON.stringify({a:1},{get length(){t=go(n-1);return 0}});return t");
body("json_space", "var t;JSON.stringify(1,null,{valueOf(){t=go(n-1);return 1}});return t");
body("json_reviver", "var t;JSON.parse('1',function(k,v){t=go(n-1);return v});return t");
body("json_reviver_object", "var t;JSON.parse('{\"a\":1}',function(k,v){if(k===''){t=go(n-1)}return v});return t");
body("json_reviver_array", "var t;JSON.parse('[1]',function(k,v){if(k==='0'){t=go(n-1)}return v});return t");
body("json_parse_text", "var t;JSON.parse({toString(){t=go(n-1);return '1'}});return t");
body("json_getter_nested", "return JSON.stringify({a:{get b(){return go(n-1)}}}).length");
body("json_big_int_to_json", "var t;BigInt.prototype.toJSON=function(){t=go(n-1);return 1};JSON.stringify(1n);return t");

// Callbacks.
const cb = "function(){t=go(n-1);return 0}";
const callbacks = {
  map: `[1].map(${cb})`, for_each: `[1].forEach(${cb})`, filter: `[1].filter(${cb})`, some: `[1].some(${cb})`, every: `[1].every(${cb})`,
  find: `[1].find(${cb})`, find_index: `[1].findIndex(${cb})`, find_last: `[1].findLast(${cb})`, find_last_index: `[1].findLastIndex(${cb})`,
  reduce: `[1].reduce(${cb},0)`, reduce_right: `[1].reduceRight(${cb},0)`, flat_map: `[1].flatMap(${cb})`, array_from: `Array.from([1],${cb})`,
  array_from_like: `Array.from({length:1},${cb})`, sort: `[2,1].sort(${cb})`, to_sorted: `[2,1].toSorted(${cb})`,
  typed_map: `new Uint8Array(1).map(${cb})`, typed_for_each: `new Uint8Array(1).forEach(${cb})`, typed_from: `Uint8Array.from([1],${cb})`,
  typed_sort: `new Uint8Array([2,1]).sort(${cb})`, typed_to_sorted: `new Uint8Array([2,1]).toSorted(${cb})`,
  typed_filter: `new Uint8Array(1).filter(${cb})`, typed_find: `new Uint8Array(1).find(${cb})`, typed_reduce: `new Uint8Array(1).reduce(${cb},0)`,
  map_for_each: `new Map([[1,1]]).forEach(${cb})`, set_for_each: `new Set([1]).forEach(${cb})`, string_array_for_each: `Array.prototype.forEach.call('a',${cb})`,
  group_by: `Object.groupBy([1],${cb})`, map_group_by: `Map.groupBy([1],${cb})`, array_like_map: `Array.prototype.map.call({length:1,0:1},${cb})`,
  replace: `'a'.replace(/a/,${cb})`, replace_string: `'a'.replace('a',${cb})`, replace_all: `'a'.replaceAll('a',${cb})`, replace_all_regexp: `'a'.replaceAll(/a/g,${cb})`,
  replace_named: `'ab'.replace(/(?<g>a)/,${cb})`, replace_sticky: `'a'.replace(/a/y,${cb})`, replace_global: `'a'.replace(/a/g,${cb})`,
  from_async_sync: `Array.fromAsync?0:0`, sort_default_to_string: `[{toString(){t=go(n-1);return 'a'}},{toString(){return 'b'}}].sort()`,
  to_sorted_default: `[{toString(){t=go(n-1);return 'a'}},{toString(){return 'b'}}].toSorted()`,
  locale_sort: `['b','a'].sort(function(a,b){t=go(n-1);return a.localeCompare(b)})`,
  array_at_valueof: `[1].at({valueOf(){t=go(n-1);return 0}})`, array_fill: `[1].fill(0,{valueOf(){t=go(n-1);return 0}})`,
  array_splice: `[1].splice({valueOf(){t=go(n-1);return 0}})`, array_index_of: `[1].indexOf(1,{valueOf(){t=go(n-1);return 0}})`,
  string_pad: `'a'.padStart({valueOf(){t=go(n-1);return 2}})`, string_slice: `'abc'.slice({valueOf(){t=go(n-1);return 0}})`,
  typed_ctor: `new Uint8Array({valueOf(){t=go(n-1);return 1}})`, array_buffer_ctor: `new ArrayBuffer({valueOf(){t=go(n-1);return 1}})`,
  date_ctor: `new Date(2020,{valueOf(){t=go(n-1);return 1}})`, number_to_fixed: `(1).toFixed({valueOf(){t=go(n-1);return 1}})`,
  number_to_string: `(1).toString({valueOf(){t=go(n-1);return 10}})`, string_from_char_code: `String.fromCharCode({valueOf(){t=go(n-1);return 65}})`,
  math_atan2: `Math.atan2({valueOf(){t=go(n-1);return 1}},1)`, math_hypot: `Math.hypot({valueOf(){t=go(n-1);return 1}})`,
  object_define_key: `Object.defineProperty({},{toString(){t=go(n-1);return 'k'}},{})`, string_raw: `String.raw({raw:{length:1,get 0(){t=go(n-1);return 'a'}}})`,
  structured_json_key: `JSON.stringify({[{toString(){t=go(n-1);return 'k'}}]:1})`, intl_number: `new Intl.NumberFormat().format({valueOf(){t=go(n-1);return 1}})`,
  intl_date: `new Intl.DateTimeFormat().format({valueOf(){t=go(n-1);return 0}})`, locale_compare_locales: `'a'.localeCompare('b',{get length(){t=go(n-1);return 0}})`,
};
for (const [k, v] of Object.entries(callbacks)) {
  if (k === "from_async_sync") continue;
  body("callback_" + k, `var t;${v};return t`);
}
// Array-likes com length e índice que recursam.
for (const method of ["forEach", "map", "filter", "some", "every", "indexOf", "includes", "join", "slice", "reverse", "pop", "lastIndexOf", "find", "findLast", "flat", "at", "keys", "entries", "fill", "copyWithin"]) {
  body("array_like_length_" + method, `var t;try{Array.prototype.${method}.call({get length(){t=go(n-1);return 0}},function(){})}catch(e){if(e.name!=='TypeError')throw e}return t`);
}

// apply, call, bind e formas de chamar.
const calls = {
  call: "go.call(null,n-1)", apply: "go.apply(null,[n-1])", bind: "go.bind(null)(n-1)", bind_args: "go.bind(null,n-1)()",
  reflect_apply: "Reflect.apply(go,null,[n-1])", call_call: "Function.prototype.call.call(go,null,n-1)",
  apply_call: "Function.prototype.apply.call(go,null,[n-1])", call_call_chain: "go.call.call(go,null,n-1)", apply_call_chain: "go.apply.call(go,null,[n-1])",
  call_apply: "go.call.apply(go,[null,n-1])", apply_apply: "go.apply.apply(go,[null,[n-1]])", bind_call: "go.bind.call(go,null)(n-1)",
  bind_triple: "go.bind(null).bind(null).bind(null)(n-1)", bind_then_call: "go.bind(null).call(null,n-1)", bind_then_apply: "go.bind(null).apply(null,[n-1])",
  call_bind: "Function.prototype.call.bind(go)(null,n-1)", apply_bind: "Function.prototype.apply.bind(go)(null,[n-1])",
  reflect_call: "Reflect.apply(Function.prototype.call,go,[null,n-1])", reflect_reflect: "Reflect.apply(Reflect.apply,null,[go,null,[n-1]])",
  bind_ten: "(function(){var f=go;for(var i=0;i<10;i++)f=f.bind(null);return f(n-1)})()", comma: "(0,go)(n-1)", eval_direct: "eval('go(n-1)')",
  eval_indirect: "(0,eval)('go('+(n-1)+')')", new_function: "new Function('return go('+(n-1)+')')()", function_ctor: "Function('return go('+(n-1)+')')()",
  arrow: "(()=>go(n-1))()", array_element: "[go][0](n-1)", optional: "go?.(n-1)", spread: "go(...[n-1])", spread_trailing: "go(n-1,...[])",
  math_max: "Math.max(go(n-1))", tagged: "(function(s,v){return v})`${go(n-1)}`", map_arg: "[n-1].map(go)[0]", from_arg: "Array.from([n-1],go)[0]",
  with_fn: "with({f:go}){f(n-1)}", arguments_callee: "(function(){return go.apply(null,arguments)})(n-1)", rest: "(function(...r){return go(...r)})(n-1)",
  default_param: "(function(a=go(n-1)){return a})()", destructure_default: "var {a=go(n-1)}={};a", array_default: "var [a=go(n-1)]=[];a",
  default_param_2: "(function(x,a=go(n-1)){return a})(1)", computed_key: "({[go(n-1)]:1})&&0", template_call: "`${go(n-1)}`.length",
  static_block: "(class{static{this.v=go(n-1)}}).v", field_init: "new (class{v=go(n-1)})().v", static_field: "(class{static v=go(n-1)}).v",
  computed_class_key: "(class{[go(n-1)](){}})&&0", super_call_arg: "(new (class extends (class{constructor(a){this.v=a}}){constructor(){super(go(n-1))}})).v",
  new_ctor: "new (function F(m){this.v=go(m)})(n-1).v", new_class: "new (class{constructor(){this.v=go(n-1)}})().v",
  new_derived: "new (class extends (class{constructor(){this.v=go(n-1)}}){})().v", new_derived_super: "new (class extends (class{constructor(){this.v=go(n-1)}}){constructor(){super()}})().v",
  reflect_construct: "Reflect.construct(function(){this.v=go(n-1)},[]).v", reflect_construct_target: "Reflect.construct(function(){this.v=go(n-1)},[],class{}).v",
  new_go: "new go(n-1)&&0", new_target: "(function F(){if(!new.target)return new F().v;this.v=go(n-1)})()", new_bound: "new (function(){this.v=go(n-1)}.bind(null))().v",
  generator_call: "(function*(){yield go(n-1)})().next().value", async_sync_part: "(async function(){return go(n-1)})()&&0",
  getter_in_arguments: "(function(){return go(n-1)}).apply(null,[])", comma_assign: "var a;a=go(n-1);a", logical: "(0||go(n-1))", nullish: "(null??go(n-1))",
  conditional: "(1?go(n-1):0)", switch_case: "switch(1){case go(n-1):default:}", try_finally: "(function(){try{return go(n-1)}finally{}})()",
  try_catch_rethrow: "(function(){try{return go(n-1)}catch(e){throw e}})()", try_finally_override: "(function(){var t;try{t=go(n-1)}finally{}return t})()",
  labeled: "(function(){a:{return go(n-1)}})()", loop_return: "(function(){for(;;){return go(n-1)}})()", do_while: "(function(){var t;do{t=go(n-1)}while(0);return t})()",
  seq_args: "Math.max(0,go(n-1),0)", in_array: "[go(n-1)][0]", in_object: "({a:go(n-1)}).a", in_new_array: "new Array(go(n-1)).length",
  string_concat: "'a'+go(n-1)", unary: "-go(n-1)", typeof: "typeof go(n-1)", void: "void go(n-1)", delete_call: "delete go(n-1)", instanceof: "go(n-1) instanceof Object",
};
for (const [k, v] of Object.entries(calls)) body("call_" + k, `return ${v}`);

// Funções recursivas por nome (go já é a recursão), com variantes que trocam o mecanismo de entrada.
body("call_named_function_expression", "return (function f(m){return go(m)})(n-1)");
body("call_closure_counter", "var c=0;function inc(){c++;return go(n-1)}return inc()");
body("call_object_method", "return ({m(){return go(n-1)}}).m()");
body("call_class_static_method", "return (class{static m(){return go(n-1)}}).m()");
body("call_class_method", "return new (class{m(){return go(n-1)}})().m()");
body("call_private_method", "return new (class{#m(){return go(n-1)}t(){return this.#m()}})().t()");
body("call_super_method", "class A{m(){return go(n-1)}}class B extends A{m(){return super.m()}}return new B().m()");
body("call_async_arrow_sync", "(async()=>go(n-1))();return 0");
body("call_generator_spread", "return [...(function*(){yield go(n-1)})()][0]");

// Regexp com exec e símbolos customizados.
const regexps = {
  exec_override_test: "var t;var re=/a/;re.exec=function(){t=go(n-1);return null};re.test('a');return t",
  exec_override_match: "var t;var re=/a/;re.exec=function(){t=go(n-1);return null};'a'.match(re);return t",
  exec_override_replace: "var t;var re=/a/;re.exec=function(){t=go(n-1);return null};'a'.replace(re,'');return t",
  exec_override_search: "var t;var re=/a/;re.exec=function(){t=go(n-1);return null};'a'.search(re);return t",
  exec_override_split: "var t;var re=/a/;re.exec=function(){t=go(n-1);return null};'a'.split(re);return t",
  exec_override_match_all: "var t;var re=/a/g;re.exec=function(){t=go(n-1);return null};[...'a'.matchAll(re)];return t",
  exec_subclass_test: "var t;class R2 extends RegExp{exec(){t=go(n-1);return null}}new R2('a').test('a');return t",
  exec_subclass_match: "var t;class R2 extends RegExp{exec(){t=go(n-1);return null}}'a'.match(new R2('a'));return t",
  exec_subclass_replace: "var t;class R2 extends RegExp{exec(){t=go(n-1);return null}}'a'.replace(new R2('a'),'');return t",
  exec_subclass_split: "var t;class R2 extends RegExp{exec(){t=go(n-1);return null}}'a'.split(new R2('a'));return t",
  exec_call_generic: "var t;RegExp.prototype.test.call({exec(){t=go(n-1);return null}},'a');return t",
  exec_result_getter: "var t;/a/[Symbol.replace].call({exec(){return {get index(){t=go(n-1);return 0},0:'a',length:1}},get flags(){return ''}},'a','')",
  last_index_value_of: "var t;var re=/a/g;re.lastIndex={valueOf(){t=go(n-1);return 0}};re.exec('a');return t",
  flags_getter: "var t;/a/[Symbol.replace].call({get flags(){t=go(n-1);return ''},exec(){return null}},'a','');return t",
  symbol_replace: "var t;''.replace({[Symbol.replace](){t=go(n-1)}},'');return t",
  symbol_match: "var t;''.match({[Symbol.match](){t=go(n-1)}});return t",
  symbol_match_all: "var t;''.matchAll({[Symbol.matchAll](){t=go(n-1);return [][Symbol.iterator]()},flags:'g'});return t",
  symbol_search: "var t;''.search({[Symbol.search](){t=go(n-1)}});return t",
  symbol_split: "var t;''.split({[Symbol.split](){t=go(n-1)}});return t",
  replace_value_to_string: "return +'a'.replace('a',{toString(){return go(n-1)}})",
  regexp_constructor_source: "var t;new RegExp({toString(){t=go(n-1);return 'a'}});return t",
  regexp_flags_value_of: "var t;new RegExp('a',{toString(){t=go(n-1);return 'g'}});return t",
  regexp_species: "var t;var re=/a/;re.constructor={[Symbol.species]:function(){t=go(n-1);return /a/}};'a'.split(re);return t",
  is_regexp: "var t;'a'.startsWith({get [Symbol.match](){t=go(n-1);return false},toString(){return 'a'}});return t",
};
for (const [k, v] of Object.entries(regexps)) body("regexp_" + k, v);

// ---- Programas completos (geradores, async, cadeias de closures e estruturas aninhadas): `$N` vira a profundidade.
const full = [];
const prog = (name, text, depths) => full.push([name, text, depths]);
const GEN_ASYNC_DEPTHS = ["Infinity", "5", "1000"];
const sync = (expr) => `globalThis.R=(function(){try{return 'ok '+(${expr})}catch(e){return e.name+': '+e.message}})();`;
const asyncRun = (expr) => `(async function(){try{globalThis.R='ok '+await (${expr})}catch(e){globalThis.R=e.name+': '+e.message}})();`;

prog("gen_yield_star", `function* g(n){if(n<=0)return 0;var r=yield* g(n-1);return r+1}` + sync("g($N).next().value"), GEN_ASYNC_DEPTHS);
prog("gen_next_nested", `function* g(n){if(n<=0)return 0;return 1+g(n-1).next().value}` + sync("g($N).next().value"), GEN_ASYNC_DEPTHS);
prog("gen_yield_nested", `function* g(n){yield (n<=0?0:1+g(n-1).next().value)}` + sync("g($N).next().value"), GEN_ASYNC_DEPTHS);
prog("gen_for_of_delegate", `function* g(n){if(n<=0){yield 0;return}for(var v of g(n-1))yield v+1}` + sync("g($N).next().value"), GEN_ASYNC_DEPTHS);
prog("gen_spread", `function* g(n){if(n<=0){yield 0;return}yield [...g(n-1)][0]+1}` + sync("g($N).next().value"), GEN_ASYNC_DEPTHS);
prog("gen_return_finally", `function* g(n){try{yield n}finally{if(n>0){var h=g(n-1);h.next();h.return()}}}` + sync("(function(){var it=g($N);it.next();it.return();return 0})()"), GEN_ASYNC_DEPTHS);
prog("gen_throw_finally", `function* g(n){try{yield n}catch(e){if(n>0){var h=g(n-1);h.next();h.throw(e)}throw e}}` + sync("(function(){var it=g($N);it.next();try{it.throw(1)}catch(e){return e}})()"), GEN_ASYNC_DEPTHS);
prog("gen_method", `var o={*g(n){if(n<=0)return 0;return 1+(yield* o.g(n-1))}}` + sync("o.g($N).next().value"), GEN_ASYNC_DEPTHS);
prog("gen_class_static", `class K{static *g(n){if(n<=0)return 0;return 1+(yield* K.g(n-1))}}` + sync("K.g($N).next().value"), GEN_ASYNC_DEPTHS);
prog("gen_array_from", `function* g(n){if(n<=0){yield 0;return}yield Array.from(g(n-1))[0]+1}` + sync("g($N).next().value"), GEN_ASYNC_DEPTHS);
prog("gen_destructure", `function* g(n){if(n<=0){yield 0;return}var [a]=g(n-1);yield a+1}` + sync("g($N).next().value"), GEN_ASYNC_DEPTHS);
prog("gen_iterator_return_chain", `function* g(n){yield 1;yield 2}var it=g(1);` + sync("(function(f){return f($N)})(function f(n){return n<=0?0:1+f(n-1)+it.next().value*0})"), GEN_ASYNC_DEPTHS);
prog("async_await", `async function f(n){if(n<=0)return 0;return 1+await f(n-1)}` + asyncRun("f($N)"), GEN_ASYNC_DEPTHS);
prog("async_arrow", `var f=async n=>n<=0?0:1+await f(n-1);` + asyncRun("f($N)"), GEN_ASYNC_DEPTHS);
prog("async_method", `var o={async f(n){if(n<=0)return 0;return 1+await o.f(n-1)}};` + asyncRun("o.f($N)"), GEN_ASYNC_DEPTHS);
prog("async_class_static", `class K{static async f(n){if(n<=0)return 0;return 1+await K.f(n-1)}}` + asyncRun("K.f($N)"), GEN_ASYNC_DEPTHS);
prog("async_try_finally", `async function f(n){if(n<=0)return 0;try{return 1+await f(n-1)}finally{}}` + asyncRun("f($N)"), GEN_ASYNC_DEPTHS);
prog("async_try_catch_rethrow", `async function f(n){if(n<=0)return 0;try{return 1+await f(n-1)}catch(e){throw e}}` + asyncRun("f($N)"), GEN_ASYNC_DEPTHS);
prog("async_then_chain", `function f(n){return n<=0?Promise.resolve(0):f(n-1).then(function(v){return v+1})}` + asyncRun("f($N)"), GEN_ASYNC_DEPTHS);
prog("async_promise_ctor", `function f(n){if(n<=0)return Promise.resolve(0);return new Promise(function(res,rej){f(n-1).then(function(v){res(v+1)},rej)})}` + asyncRun("f($N)"), GEN_ASYNC_DEPTHS);
prog("async_promise_resolve", `function f(n){return n<=0?0:Promise.resolve(f(n-1)).then(function(v){return v+1})}` + asyncRun("f($N)"), GEN_ASYNC_DEPTHS);
prog("async_gen_for_await", `async function* g(n){if(n<=0){yield 0;return}for await(var v of g(n-1))yield v+1}` + asyncRun("g($N).next().then(function(r){return r.value})"), GEN_ASYNC_DEPTHS);
prog("async_gen_yield_star", `async function* g(n){if(n<=0)return 0;return 1+(yield* g(n-1))}` + asyncRun("g($N).next().then(function(r){return r.value})"), GEN_ASYNC_DEPTHS);
prog("async_gen_await_next", `async function* g(n){if(n<=0){yield 0;return}yield 1+(await g(n-1).next()).value}` + asyncRun("g($N).next().then(function(r){return r.value})"), GEN_ASYNC_DEPTHS);
prog("async_all", `async function f(n){if(n<=0)return 0;return 1+(await Promise.all([f(n-1)]))[0]}` + asyncRun("f($N)"), GEN_ASYNC_DEPTHS);
prog("async_race", `async function f(n){if(n<=0)return 0;return 1+await Promise.race([f(n-1)])}` + asyncRun("f($N)"), GEN_ASYNC_DEPTHS);
prog("async_all_settled", `async function f(n){if(n<=0)return 0;return 1+(await Promise.allSettled([f(n-1)]))[0].value}` + asyncRun("f($N)"), GEN_ASYNC_DEPTHS);
prog("async_any", `async function f(n){if(n<=0)return 0;return 1+await Promise.any([f(n-1)])}` + asyncRun("f($N)"), GEN_ASYNC_DEPTHS);
prog("async_thenable", `async function f(n){if(n<=0)return 0;return 1+await {then(r){r(f(n-1))}}}` + asyncRun("f($N)"), ["5", "1000"]);
prog("async_catch_handler", `function f(n){return n<=0?Promise.reject(0):f(n-1).catch(function(v){throw v+1})}` + asyncRun("f($N).catch(function(v){return v})"), GEN_ASYNC_DEPTHS);
prog("async_finally_handler", `function f(n){return n<=0?Promise.resolve(0):f(n-1).finally(function(){}).then(function(v){return v+1})}` + asyncRun("f($N)"), GEN_ASYNC_DEPTHS);
prog("async_for_await_sync_iter", `async function f(n){if(n<=0)return 0;for await(var x of [f(n-1)])return 1+x}` + asyncRun("f($N)"), GEN_ASYNC_DEPTHS);
prog("async_default_param", `async function f(n,a=n<=0?0:f(n-1)){return 1+await a}` + asyncRun("f($N)"), GEN_ASYNC_DEPTHS);

// Cadeias de closures construídas em laço: o `let` por iteração dá um elo por volta.
const CHAIN_DEPTHS = ["10", "1000", "100000", "1000000"];
const chains = {
  to_string: ["var o={toString(){return 'x'}}", "let p=o;o={toString(){return String(p)}}", "String(o).length"],
  value_of: ["var o={valueOf(){return 1}}", "let p=o;o={valueOf(){return +p}}", "+o"],
  to_primitive: ["var o={[Symbol.toPrimitive](){return 1}}", "let p=o;o={[Symbol.toPrimitive](){return +p}}", "+o"],
  getter: ["var o={x:1}", "let p=o;o={get x(){return p.x}}", "o.x"],
  setter: ["var v=0,o={set x(a){v=a}}", "let p=o;o={set x(a){p.x=a}}", "(o.x=1,v)"],
  function: ["var f=function(){return 1}", "let p=f;f=function(){return p()+1}", "f()"],
  arrow: ["var f=()=>1", "let p=f;f=()=>p()+1", "f()"],
  method: ["var o={m(){return 1}}", "let p=o;o={m(){return p.m()+1}}", "o.m()"],
  call: ["var f=function(){return 1}", "let p=f;f=function(){return p.call(null)}", "f()"],
  apply: ["var f=function(){return 1}", "let p=f;f=function(){return p.apply(null,[])}", "f()"],
  reflect_apply: ["var f=function(){return 1}", "let p=f;f=function(){return Reflect.apply(p,null,[])}", "f()"],
  bind: ["var f=function(){return 1}", "f=f.bind(null)", "f()"],
  bind_new: ["var f=function(){this.v=1}", "f=f.bind(null)", "new f().v"],
  bind_args: ["var f=function(){return arguments.length}", "f=f.bind(null,1)", "f()"],
  proxy_get: ["var p={x:1}", "p=new Proxy(p,{})", "p.x"],
  proxy_has: ["var p={x:1}", "p=new Proxy(p,{})", "'x' in p"],
  proxy_set: ["var p={x:1}", "p=new Proxy(p,{})", "(p.x=2,p.x)"],
  proxy_keys: ["var p={x:1}", "p=new Proxy(p,{})", "Object.keys(p).length"],
  proxy_apply: ["var p=function(){return 1}", "p=new Proxy(p,{})", "p()"],
  proxy_construct: ["var p=function(){this.v=1}", "p=new Proxy(p,{})", "new p().v"],
  proxy_proto: ["var p={x:1}", "p=new Proxy(p,{})", "Object.getPrototypeOf(p)===Object.prototype"],
  proxy_trap_get: ["var p={x:1}", "let q=p;p=new Proxy({},{get(t,k){return q[k]}})", "p.x"],
  proxy_trap_has: ["var p={x:1}", "let q=p;p=new Proxy({},{has(t,k){return k in q}})", "'x' in p"],
  proto_chain_get: ["var o={x:1}", "o=Object.create(o)", "o.x"],
  proto_chain_has: ["var o={x:1}", "o=Object.create(o)", "'x' in o"],
  proto_chain_set: ["var o={}", "o=Object.create(o)", "(o.x=1,o.x)"],
  proto_chain_for_in: ["var o={x:1}", "o=Object.create(o)", "(function(){var c=0;for(var k in o)c++;return c})()"],
  proto_chain_instanceof: ["var F=function(){},o=F.prototype", "o=Object.create(o)", "o instanceof F"],
  proto_chain_to_string: ["var o={}", "o=Object.create(o)", "String(o)"],
  array_nest_to_string: ["var a=[]", "a=[a]", "String(a).length"],
  array_nest_join: ["var a=[]", "a=[a]", "a.join().length"],
  array_nest_plus: ["var a=[]", "a=[a]", "(''+a).length"],
  array_nest_template: ["var a=[]", "a=[a]", "`${a}`.length"],
  array_nest_locale: ["var a=[]", "a=[a]", "a.toLocaleString().length"],
  array_nest_flat: ["var a=[]", "a=[a]", "a.flat(Infinity).length"],
  array_nest_flat_big: ["var a=[]", "a=[a]", "a.flat(1e9).length"],
  array_nest_json: ["var a=[]", "a=[a]", "JSON.stringify(a).length"],
  array_nest_json_indent: ["var a=[]", "a=[a]", "JSON.stringify(a,null,1).length"],
  array_nest_json_replacer: ["var a=[]", "a=[a]", "JSON.stringify(a,function(k,v){return v}).length"],
  array_nest_number: ["var a=[]", "a=[a]", "Number(a)"],
  array_nest_sort: ["var a=[]", "a=[a]", "[a,a].sort().length"],
  array_nest_parse_int: ["var a=[]", "a=[a]", "parseInt(a)"],
  array_nest_escape: ["var a=[]", "a=[a]", "escape(a).length"],
  array_nest_concat_str: ["var a=[]", "a=[a]", "'x'.concat(a).length"],
  array_nest_key: ["var a=[]", "a=[a]", "Object.keys({[a]:1}).length"],
  array_nest_regexp: ["var a=[]", "a=[a]", "new RegExp(a).source.length"],
  array_nest_split: ["var a=[]", "a=[a]", "'abc'.split(a).length"],
  array_nest_eq: ["var a=[]", "a=[a]", "a==''"],
  array_nest_is_array: ["var a=[]", "a=[a]", "Array.isArray(a)"],
  array_nest_includes: ["var a=[]", "a=[a]", "[a].includes(a)"],
  array_nest_from: ["var a=[]", "a=[a]", "Array.from(a).length"],
  array_nest_spread: ["var a=[]", "a=[a]", "[...a].length"],
  array_nest_set: ["var a=[]", "a=[a]", "new Set(a).size"],
  array_nest_concat: ["var a=[]", "a=[a]", "[].concat(a).length"],
  array_nest_to_sorted: ["var a=[]", "a=[a]", "a.toSorted().length"],
  array_nest_to_string_method: ["var a=[]", "a=[a]", "a.toString().length"],
  array_nest_object_to_string: ["var a=[]", "a=[a]", "Object.prototype.toString.call(a)"],
  array_nest_map_json: ["var a=[]", "a=[a]", "JSON.stringify(a.map(function(x){return x})).length"],
  object_nest_json: ["var o={}", "o={a:o}", "JSON.stringify(o).length"],
  object_nest_json_indent: ["var o={}", "o={a:o}", "JSON.stringify(o,null,2).length"],
  object_nest_json_replacer_array: ["var o={}", "o={a:o}", "JSON.stringify(o,['a']).length"],
  object_nest_json_keys: ["var o={}", "o={a:o}", "Object.keys(o).length"],
  object_nest_assign: ["var o={}", "o={a:o}", "Object.assign({},o).a===o.a"],
  object_nest_to_json_array_mix: ["var o=[]", "o=[{a:o}]", "JSON.stringify(o).length"],
  object_nest_map_set: ["var o=new Map()", "o=new Map([[1,o]])", "o.size"],
  object_nest_to_string_tag: ["var o={}", "o={a:o,toString(){return 'x'}}", "String(o)"],
  json_text_nest_array: ["var s=''", "s=s", "JSON.parse('['.repeat($N)+']'.repeat($N)).length"],
  json_text_nest_object: ["var s=''", "s=s", "typeof JSON.parse('{\"a\":'.repeat($N)+'1'+'}'.repeat($N))"],
  json_text_nest_reviver: ["var s=''", "s=s", "JSON.parse('['.repeat($N)+']'.repeat($N),function(k,v){return v}).length"],
  json_text_nest_object_reviver: ["var s=''", "s=s", "typeof JSON.parse('{\"a\":'.repeat($N)+'1'+'}'.repeat($N),function(k,v){return v})"],
  json_text_nest_array_stringify: ["var s=''", "s=s", "JSON.stringify(JSON.parse('['.repeat($N)+']'.repeat($N))).length"],
  regexp_nest_group: ["var s=''", "s=s", "new RegExp('('.repeat($N)+'a'+')'.repeat($N)).test('a')"],
  regexp_nest_lookahead: ["var s=''", "s=s", "new RegExp('(?='.repeat($N)+'a'+')'.repeat($N)).test('a')"],
  regexp_nest_noncapture: ["var s=''", "s=s", "new RegExp('(?:'.repeat($N)+'a'+')'.repeat($N)).test('a')"],
  regexp_nest_class_alt: ["var s=''", "s=s", "new RegExp('(?:a|'.repeat($N)+'a'+')'.repeat($N)).test('a')"],
  function_nest_parens: ["var s=''", "s=s", "eval('('.repeat($N)+'1'+')'.repeat($N))"],
  function_nest_array_literal: ["var s=''", "s=s", "eval('['.repeat($N)+']'.repeat($N)).length"],
  function_nest_object_literal: ["var s=''", "s=s", "typeof eval('({a:'.repeat($N)+'1'+'})'.repeat($N))"],
  function_nest_unary: ["var s=''", "s=s", "eval('-'.repeat($N)+'1')"],
  function_nest_ternary: ["var s=''", "s=s", "eval('1?'.repeat($N)+'1'+':0'.repeat($N))"],
  function_nest_call: ["var s=''", "s=s", "eval('(function(){return '.repeat($N)+'1'+'})()'.repeat($N))"],
  function_nest_arrow: ["var s=''", "s=s", "eval('(()=>'.repeat($N)+'1'+')'.repeat($N)+'()'.repeat($N))"],
  function_nest_template: ["var s=''", "s=s", "eval('`${'.repeat($N)+'1'+'}`'.repeat($N))"],
  function_nest_new_function: ["var s=''", "s=s", "new Function('return '+'('.repeat($N)+'1'+')'.repeat($N))()"],
  function_nest_block: ["var s=''", "s=s", "eval('{'.repeat($N)+'1'+'}'.repeat($N))"],
  function_nest_if: ["var s=''", "s=s", "eval('if(1)'.repeat($N)+'1')"],
  function_nest_binary: ["var s=''", "s=s", "eval('1'+'+1'.repeat($N))"],
  function_nest_member: ["var s=''", "s=s", "eval('({a:1})'+'.constructor'.repeat($N)).name"],
};
for (const [name, [setup, step, expr]] of Object.entries(chains)) {
  const useN = expr.includes("$N");
  const text = useN
    ? setup + ";" + sync(expr)
    : setup + ";" + `for(var i=0;i<$N;i++){${step}}` + sync(expr);
  prog("chain_" + name, text, CHAIN_DEPTHS);
}

// ---- Montagem dos programas.
const programs = [];
const infinite = "Infinity";
const goText = (bodyText) => `function go(n){if(n<=0)return 0;return 1+ +(function(){${bodyText}})()}\n`;
const contexts = [
  (c) => sync(c),
  (c) => sync(`eval(${JSON.stringify(c)})`),
  (c) => sync(`(function*(){return ${c}})().next().value`),
  (c) => asyncRun(`(async function(){return ${c}})()`),
  (c) => sync(`(class{static{this.v=${c}}}).v`),
  (c) => sync(`(function(){try{return ${c}}finally{}})()`),
  (c) => sync(`new Function('return '+${JSON.stringify(c)})()`),
  (c) => sync(`(()=>${c})()`),
  (c) => sync(`(function(){try{return ${c}}catch(e){throw e}})()`),
  (c) => sync(`[1].map(function(){return ${c}})[0]`),
  (c) => sync(`(function(){return ${c}}).bind(null)()`),
  (c) => sync(`Reflect.apply(function(){return ${c}},null,[])`),
];
bodies.forEach(([name, text], index) => {
  const def = goText(text);
  programs.push({ name, text: def + sync(`go(${infinite})`), want: "inf" });
  programs.push({ name: name + "_ctx", text: def + contexts[1 + (index % (contexts.length - 1))](`go(${infinite})`), want: "inf" });
  programs.push({ name: name + "_1000", text: def + sync("go(1000)"), want: "ok" });
  if (index % 2 === 0) programs.push({ name: name + "_5", text: def + sync("go(5)"), want: "ok" });
});
for (const [name, text, depths] of full) {
  for (const depth of depths) {
    programs.push({ name: `${name}_${depth}`, text: text.split("$N").join(depth), want: depth === "Infinity" || +depth >= 100000 ? "inf" : "ok" });
  }
}

// ---- Execução e filtro.
const goldenDir = path.join(__dirname, "..", "tests", "golden");
let existing = "";
const stackFiles = (file) => file !== "stack_overflow_bun.tsv" && fs.readFileSync(path.join(goldenDir, file), "utf8").includes("Maximum call stack");
existing = knownPrograms("stack_overflow_bun.tsv", stackFiles).join("\n\u0000\n");
const seen = new Set();
const todo = programs.filter((p) => !seen.has(p.text) && seen.add(p.text));

const runOne = (source) =>
  new Promise((resolve) => {
    const child = spawn(process.execPath, ["--preload", PRELOAD, __filename, "--child"], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    const timer = setTimeout(() => { child.kill("SIGKILL"); resolve(null); }, 10000);
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", () => {});
    child.on("close", (code) => { clearTimeout(timer); resolve(code === 0 ? decodeResult(out) : null); });
    child.stdin.end(source);
  });

(async () => {
  const results = new Array(todo.length);
  let next = 0;
  const worker = async () => {
    while (next < todo.length) {
      const i = next++;
      results[i] = await runOne(todo[i].text);
    }
  };
  await Promise.all(Array.from({ length: 8 }, worker));
  const stats = { kept: 0, wrong: 0, failed: 0, dup: 0 };
  const droppedNames = [];
  todo.forEach((p, i) => {
    const result = results[i];
    if (result === null) { stats.failed++; droppedNames.push(p.name + ":falhou"); return; }
    if (p.text.length > 24 && existing.includes(p.text)) { stats.dup++; return; }
    const ok = p.want === "inf" ? result === MSG : /^ok /.test(result);
    if (!ok || /\/home\/|\/tmp\/|\.js:\d|bun/i.test(result)) { stats.wrong++; droppedNames.push(p.name + ":" + result.slice(0, 60)); return; }
    stats.kept++;
    emitRow(JSON.stringify(p.text) + "\t" + JSON.stringify(result));
  });
  process.stderr.write(`mantidos ${stats.kept}, fora do esperado ${stats.wrong}, falhas ${stats.failed}, repetidos ${stats.dup}\n`);
  if (process.env.SHOW_DROPPED) process.stderr.write(droppedNames.join("\n") + "\n");
})();
