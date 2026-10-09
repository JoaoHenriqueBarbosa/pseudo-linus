// Gera tests/golden/json_bun.tsv: JSON.parse (reviver, erros de sintaxe com mensagem exata), JSON.stringify
// (replacer, space, toJSON, ciclos, surrogates, wrappers, esparsos, typed arrays), JSON.rawJSON/isRawJSON e
// JSON[Symbol.toStringTag], medidos no bun 1.4.2. Não repete json_number_bun.tsv (números isolados em eval).
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON), igual a gen-function-error-golden.js.
// Todo programa carrega um prelúdio com `S` (descreve um valor) e `T` (roda e descreve o valor ou a exceção).
// Uso: bun scripts/gen-json-golden.js > tests/golden/json_bun.tsv
const PRELUDE =
  'function S(v){try{return typeof v==="string"?JSON.stringify(v):Object.is(v,-0)?"-0":typeof v==="bigint"?v+"n":' +
  'typeof v==="symbol"?v.toString():typeof v==="undefined"?"undefined":typeof v==="function"?"function":String(v)}catch(e){return "?"}}\n' +
  'function T(f){try{return S(f())}catch(e){return "throw "+(e&&e.name)+": "+(e&&e.message)}}\n';

const exprs = [];
const add = (...list) => exprs.push(...list);
const q = s => JSON.stringify(s);

// ---- JSON.parse: prefixos de documentos válidos (fim inesperado, mensagens de posição).
const validDocs = [
  '{"a":1,"b":[true,false,null],"c":{"d":"e"}}',
  '[1,2.5,-3e2,"x",null]',
  '{"a":"\\u00e9\\n\\t","b":-0}',
  '["a\\"b",{"k":[]}]',
  ' { "x" : [ 1 , 2 ] } ',
  '"str"',
  "-12.5e+3",
  "true",
  "null",
  '{"__proto__":{"x":1}}',
];
for (const doc of validDocs) {
  for (let i = 0; i <= doc.length; i++) add(`JSON.parse(${q(doc.slice(0, i))})`);
}
// Inserção de um caractere ruim em cada posição de dois documentos.
for (const doc of ['{"a":[1,"b"],"c":null}', '[1,{"x":true}]']) {
  for (const bad of ["x", ",", ":", "'", "\u0001", "}", "]"]) {
    for (let i = 0; i <= doc.length; i += 2) add(`JSON.parse(${q(doc.slice(0, i) + bad + doc.slice(i))})`);
  }
}
// Erros nomeados.
const syntaxCases = [
  "{,}", "[,]", "[1,]", "[1 2]", "{\"a\":1,}", "{\"a\" 1}", "{\"a\":}", "{a:1}", "{'a':1}", "{1:2}", "{\"a\":1 \"b\":2}",
  "[1,,2]", "[,1]", "[1,2", "{\"a\":1", "{\"a\"", "{\"a\":", "\"abc", "\"abc\\", "\"abc\\u12", "\"\\u12G4\"", "\"\\x41\"", "\"\\'\"",
  "\"a\nb\"", "\"a\tb\"", "\"a\u0000b\"", "\"a\u001fb\"", "\"a\u007fb\"", "\"a\u2028b\"", "\"a\u2029b\"",
  "01", "00", "-", "-a", "+1", ".5", "5.", "1e", "1e+", "1.e1", "0x10", "1_0", "Infinity", "-Infinity", "NaN", "undefined",
  "1 2", "1,2", "[] []", "{} {}", "null null", "nul", "nulll", "tru", "truee", "fals", "falsee", "True", "NULL",
  "\uFEFF1", "\uFEFF[]", " \uFEFF1", "1\uFEFF", "\u00a01", "\u20281", "\v1", "\f1", "\r\n\t 1 \r\n\t", "1\u0000", "\u00001",
  "/* c */ 1", "// c\n1", "1 // c", "#1", "[1] x", "{\"a\":1}}", "[[1]", "[1]]", "{\"a\":{\"b\":1}", "'a'", "`a`", "\\", "\"", "'",
  "{\"a\":1}\u0000", "[\"\\u0000\"]", "\"\\ud800\"", "\"\\udc00\"", "\"\\ud800\\udc00\"", "\"\ud800\"", "\"\ud83d\ude00\"",
  "{\"a\":1,\"a\":2}", "{\"a\":1,\"b\":2,\"a\":3}", "{\"__proto__\":1}", "{\"__proto__\":{\"polluted\":true}}", "[{\"__proto__\":[]}]",
  "{\"\":1}", "{\"\\u0061\":1}", "{\"a\\nb\":1}", "{\"1\":1,\"0\":2,\"a\":3,\"-1\":4}", "{\"4294967295\":1,\"4294967294\":2,\"2\":3}",
  "1e999", "-1e999", "1e-999", "123456789012345678901234567890", "0.1e1", "-0", "-0.0", "0e0", "1E2", "1e+2", "1e-2", "9007199254740993",
];
for (const s of syntaxCases) add(`JSON.parse(${q(s)})`);
add('JSON.parse()', 'JSON.parse(undefined)', 'JSON.parse(null)', 'JSON.parse(1)', 'JSON.parse(true)', 'JSON.parse({})', 'JSON.parse([])',
  'JSON.parse([1])', 'JSON.parse({toString(){return "[2]"}})', 'JSON.parse({toString(){throw new RangeError("boom")}})',
  'JSON.parse(Symbol())', 'JSON.parse(1n)', 'JSON.parse(new String("[3]"))', 'JSON.parse(NaN)', 'JSON.parse(-0)',
  'JSON.parse("1", null)', 'JSON.parse("1", 5)', 'JSON.parse("1", {})', 'JSON.parse("[1]", "x")');

// Profundidade.
for (const n of [100, 1000, 5000, 9999, 10000, 10001, 20000, 100000]) {
  add(`JSON.parse("[".repeat(${n})+"]".repeat(${n})) instanceof Array`);
  add(`(()=>{let d=0,v=JSON.parse("[".repeat(${n})+"]".repeat(${n}));while(Array.isArray(v)&&v.length){v=v[0];d++}return d})()`);
  add(`JSON.parse('{"a":'.repeat(${n})+"1"+"}".repeat(${n})) !== null`);
  add(`JSON.parse("[".repeat(${n})+"]".repeat(${n - 1}))`);
}
add('JSON.parse("[".repeat(10000))', 'JSON.parse("[".repeat(100000))', 'JSON.parse("{\\"a\\":".repeat(100000))');

// Chaves e valores especiais.
add('Object.getPrototypeOf(JSON.parse(\'{"__proto__":null}\')) === Object.prototype',
  'Object.keys(JSON.parse(\'{"__proto__":null}\')).join()',
  'Object.getOwnPropertyDescriptor(JSON.parse(\'{"__proto__":{"a":1}}\'),"__proto__").value.a',
  'JSON.parse(\'{"__proto__":{"a":1}}\').a',
  'Object.keys(JSON.parse(\'{"b":1,"a":2,"1":3,"0":4}\')).join()',
  'Object.keys(JSON.parse(\'{"a":1,"a":2,"b":3,"a":4}\')).join()+JSON.parse(\'{"a":1,"a":2}\').a',
  'JSON.stringify(JSON.parse(\'{"__proto__":1,"__proto__":2}\'))',
  'Object.keys(JSON.parse(\'{"__proto__":1,"__proto__":2}\')).length',
  'JSON.parse("-0") === 0 && Object.is(JSON.parse("-0"), -0)',
  'Object.is(JSON.parse("[-0]")[0], -0)',
  'JSON.parse("1e999")', 'JSON.parse("[1e999,-1e999]").join()',
  'JSON.parse("\\"\\\\ud800\\"").length', 'JSON.parse("\\"\\\\ud800\\"").charCodeAt(0)',
  'JSON.parse("\\"\\\\u0041\\\\u00e9\\\\ud83d\\\\ude00\\"")',
  'JSON.parse("\\"\\\\/\\\\b\\\\f\\\\n\\\\r\\\\t\\\\\\\\\\"") === "/\\b\\f\\n\\r\\t\\\\"',
  'JSON.parse(\'{"a":[]}\').a.length', 'Array.isArray(JSON.parse("[]"))',
  'Object.getPrototypeOf(JSON.parse("{}")) === Object.prototype', 'Object.isExtensible(JSON.parse("{}"))',
  'JSON.parse("[1,2,3]").length', 'JSON.parse(" \\t\\r\\n[ ] \\t\\r\\n").length',
  'JSON.parse("1e5")', 'JSON.parse("1E+5")', 'JSON.parse("0.000001")', 'JSON.parse("1e21")', 'JSON.parse("123456789012345680000")');

// ---- JSON.parse com reviver.
const revSrc = 'var L=[];function rv(k,v){L.push((typeof k==="string"?k:"?")+"="+(v&&typeof v==="object"?(Array.isArray(v)?"[]":"{}"):S(v))+"@"+(Array.isArray(this)?"A":this&&typeof this==="object"?"O":"?"));return v}';
const revDocs = [
  '1', '"s"', 'null', '[]', '{}', '[1,2,3]', '{"a":1,"b":2}', '{"b":1,"a":2,"1":3,"0":4}', '[[1],[2,[3]]]', '{"a":{"b":{"c":1}}}',
  '[{"a":1},{"b":[2]}]', '{"a":[1,{"b":2}],"c":null}', '{"__proto__":1}', '{"a":1,"a":2}', '[-0,1e999]', '{"":1}', '{"":{"":[]}}',
];
for (const doc of revDocs) add(`(()=>{${revSrc};JSON.parse(${q(doc)},rv);return L.join("|")})()`);
const revTransforms = [
  ['delete', 'function(k,v){return k==="a"?undefined:v}'],
  ['undef-array', 'function(k,v){return k==="1"?undefined:v}'],
  ['undef-root', 'function(k,v){return undefined}'],
  ['double', 'function(k,v){return typeof v==="number"?v*2:v}'],
  ['replace-obj', 'function(k,v){return k==="a"?{z:1}:v}'],
  ['replace-arr', 'function(k,v){return k==="a"?[9]:v}'],
  ['null', 'function(k,v){return k==="b"?null:v}'],
  ['throw', 'function(k,v){if(k==="b")throw new Error("rv "+k);return v}'],
  ['mutate-this-delete', 'function(k,v){if(k==="a"){delete this.b}return v}'],
  ['mutate-this-add', 'function(k,v){if(k==="a"){this.zz=5}return v}'],
  ['mutate-this-set', 'function(k,v){if(k==="a"){this.b=77}return v}'],
  ['mutate-array-push', 'function(k,v){if(k==="0"&&Array.isArray(this)){this.push(9)}return v}'],
  ['mutate-array-len', 'function(k,v){if(k==="0"&&Array.isArray(this)){this.length=1}return v}'],
  ['defineProperty-nonconf', 'function(k,v){if(k==="a"){Object.defineProperty(this,"b",{value:5,configurable:false})}return v}'],
  ['freeze', 'function(k,v){if(k==="a"){Object.freeze(this)}return v}'],
  ['key-type', 'function(k,v){return typeof k==="string"?v:"bad"}'],
  ['arguments-length', 'function(k,v){return arguments.length}'],
  ['arrow', '(k,v)=>typeof v==="number"?v+1:v'],
  ['proxy-this', 'function(k,v){return Object.getPrototypeOf(this)===Object.prototype||Array.isArray(this)?v:"x"}'],
];
for (const [name, fn] of revTransforms) {
  for (const doc of ['{"a":1,"b":2,"c":3}', '[1,2,3]', '{"a":[1,2],"b":{"c":3}}', '5', '[[1,2],[3]]', '{"a":{"b":1},"b":2}']) {
    add(`T(()=>JSON.stringify(JSON.parse(${q(doc)},${fn})))`);
  }
}
// Reviver não função, `this` do reviver, argumentos, context com source.
add('JSON.stringify(JSON.parse("[1]",null))', 'JSON.stringify(JSON.parse("[1]",undefined))', 'JSON.stringify(JSON.parse("[1]",{}))',
  'JSON.stringify(JSON.parse("[1]",[]))', 'JSON.stringify(JSON.parse("[1]",Symbol()))',
  '(()=>{var th;JSON.parse("1",function(){th=this});return Object.keys(th).join()+"|"+Object.getPrototypeOf(th)===Object.prototype})()',
  '(()=>{var th;JSON.parse("1",function(){th=this});return JSON.stringify(th)})()',
  '(()=>{var th;JSON.parse("[1]",function(k){if(k==="")th=this});return JSON.stringify(th)})()',
  '(()=>{var th;JSON.parse("1",function(){th=this});return Object.getOwnPropertyNames(th).join()})()',
  '(()=>{var a=[];JSON.parse("[1]",function(){a.push(arguments.length)});return a.join()})()',
  '(()=>{var a=[];JSON.parse("[1,{\\"a\\":2}]",function(k,v,c){a.push(typeof c)});return a.join()})()',
  '(()=>{var a=[];JSON.parse("[1,{\\"a\\":2},\\"x\\",null,true]",function(k,v,c){a.push(S(c&&c.source))});return a.join()})()',
  '(()=>{var a=[];JSON.parse("[1.0,1e2,-0,\\"\\\\u0041\\",12345678901234567890]",function(k,v,c){a.push(S(c&&c.source))});return a.join()})()',
  '(()=>{var a=[];JSON.parse("{\\"a\\":1}",function(k,v,c){a.push(S(c&&Object.keys(c).join()))});return a.join()})()',
  '(()=>{var a=[];JSON.parse("[1]",function(k,v,c){if(k==="0")this[0]=2;a.push(S(c&&c.source))});return a.join()})()',
  '(()=>{var a=[];JSON.parse("[1]",function(k,v,c){a.push(S(c&&c.source));return 5});return a.join()})()',
  'JSON.parse("12345678901234567890",function(k,v,c){return c&&c.source})',
  'JSON.parse("[12345678901234567890]",function(k,v,c){return typeof v==="number"&&c?BigInt(c.source):v})[0]',
  'JSON.parse("1",function(k,v,c){return Object.isFrozen(c)})',
  'JSON.parse("1",function(k,v,c){return c&&Object.getPrototypeOf(c)===Object.prototype})',
  'JSON.parse("1",function(k,v,c){return c&&Object.getOwnPropertyNames(c).join()})',
  'JSON.parse.length', 'JSON.parse.name', 'JSON.stringify.length', 'JSON.stringify.name',
  'Object.getOwnPropertyNames(JSON).sort().join()', 'Object.getOwnPropertyNames(JSON).join()',
  'Object.getOwnPropertySymbols(JSON).length', 'JSON[Symbol.toStringTag]', 'Object.prototype.toString.call(JSON)',
  'String(JSON)', 'JSON+""', 'typeof JSON', 'Object.getPrototypeOf(JSON)===Object.prototype',
  'JSON.toString()', 'typeof JSON.parse', 'JSON.parse.hasOwnProperty("prototype")', 'Reflect.construct.length',
  'new JSON.parse("1")', 'new JSON.stringify(1)', 'new JSON()', 'JSON()',
  'JSON.hasOwnProperty("rawJSON")', 'JSON.hasOwnProperty("isRawJSON")', 'typeof JSON.rawJSON', 'typeof JSON.isRawJSON',
  'JSON.rawJSON.length', 'JSON.isRawJSON.length', 'JSON.rawJSON.name',
  'Object.getOwnPropertyDescriptor(JSON,Symbol.toStringTag)&&JSON.stringify(Object.getOwnPropertyDescriptor(JSON,Symbol.toStringTag))',
  'JSON.stringify(Object.getOwnPropertyDescriptor(JSON,"parse"))', 'JSON.stringify(Object.getOwnPropertyDescriptor(globalThis,"JSON"))');

// ---- JSON.rawJSON / isRawJSON.
for (const s of ['1', '"x"', 'true', 'null', '12345678901234567890', '1e999', '-0', '0.10', '"a\\u0041"', '', ' 1', '1 ', '[1]', '{}', '{"a":1}',
  '\t1', '1\n', 'nul', 'abc', '"unterminated', '1e', '01', 'undefined', 'NaN']) {
  add(`T(()=>JSON.stringify(JSON.rawJSON(${q(s)})))`);
  add(`T(()=>JSON.isRawJSON(JSON.rawJSON(${q(s)})))`);
  add(`T(()=>JSON.stringify({a:JSON.rawJSON(${q(s)}),b:[JSON.rawJSON(${q(s)})]}))`);
  add(`T(()=>{var r=JSON.rawJSON(${q(s)});return JSON.stringify(Object.getOwnPropertyNames(r))+Object.isFrozen(r)+(Object.getPrototypeOf(r)===null)+r.rawJSON})`);
}
add('JSON.isRawJSON({})', 'JSON.isRawJSON({rawJSON:"1"})', 'JSON.isRawJSON()', 'JSON.isRawJSON(1)', 'JSON.isRawJSON(null)', 'JSON.isRawJSON("1")',
  'JSON.stringify(JSON.rawJSON(1))', 'JSON.stringify(JSON.rawJSON(null))', 'JSON.stringify(JSON.rawJSON(true))', 'JSON.stringify(JSON.rawJSON())',
  'JSON.stringify(JSON.rawJSON({toString(){return "7"}}))', 'JSON.stringify(JSON.rawJSON(Symbol()))', 'JSON.stringify(JSON.rawJSON(1n))',
  'Object.prototype.toString.call(JSON.rawJSON("1"))', 'typeof JSON.rawJSON("1")', 'JSON.stringify([JSON.rawJSON("1"),JSON.rawJSON("2")])',
  'JSON.stringify(JSON.rawJSON("1"),null,2)', 'JSON.stringify({a:JSON.rawJSON("1")},null,2)',
  'JSON.stringify({a:JSON.rawJSON("1")},(k,v)=>v)', 'JSON.stringify({a:1},(k,v)=>k==="a"?JSON.rawJSON("99999999999999999999"):v)',
  'JSON.stringify({a:JSON.rawJSON("1")},["a"])', 'JSON.stringify(Object.create(JSON.rawJSON("1")))',
  'JSON.stringify({toJSON(){return JSON.rawJSON("5")}})', 'Object.keys(JSON.rawJSON("1")).join()', 'Object.isFrozen(JSON.rawJSON("1"))',
  'JSON.rawJSON("1")==="1"', 'JSON.stringify(Object.assign({}, JSON.rawJSON("1")))', 'JSON.stringify(JSON.rawJSON("\\"a\\nb\\""))',
  'JSON.stringify(JSON.rawJSON("\\"\\u2028\\""))');

// ---- JSON.stringify: valores primitivos e wrappers.
const prims = [
  'undefined', 'null', 'true', 'false', '0', '-0', '1', '-1', '1.5', '1e21', '1e-7', '123456789012345680000', '5e-324', '1.7976931348623157e308',
  'NaN', 'Infinity', '-Infinity', '0.1+0.2', '""', '"a"', '"\\""', '"\\\\"', '"\\n\\r\\t\\b\\f\\v"', '"\\u0000\\u0001\\u001f\\u007f"', '"\\u2028\\u2029"',
  '"é😀"', '"\\ud800"', '"\\udc00"', '"\\udc00\\ud800"', '"a\\ud800b"', '"\\ud83d"', '"\\ud83d\\ude00"', '"\\ud83d\\u0041"', '"\\ude00\\ud83d"',
  '"</script>"', '"\\u00ff\\u0100"', '"\\u007f"', '"\\u0080"', '"\\ufeff"', '"\\ufffe\\uffff"', 'Symbol("s")', 'Symbol.iterator', '1n', '(function(){})',
  '(()=>1)', 'class A{}', 'new Boolean(true)', 'new Boolean(false)', 'new Number(5)', 'new Number(-0)', 'new Number(NaN)', 'new Number(Infinity)',
  'new String("s")', 'new String("\\ud800")', 'Object(1n)', 'Object(Symbol())', 'Object(Symbol("x"))', 'new Date(0)', 'new Date(NaN)', 'new Date(8.64e15)',
  '/re/g', 'new Error("m")', 'new Map([[1,2]])', 'new Set([1])', 'new WeakMap', 'Promise.resolve(1)', 'globalThis.nonexistent', '[undefined]',
  '[function(){}]', '[Symbol()]', '{a:undefined}', '{a:function(){}}', '{a:Symbol()}', '{[Symbol("k")]:1}', '[NaN,Infinity,-Infinity,-0]', '{a:NaN}',
];
for (const p of prims) {
  add(`T(()=>JSON.stringify(${p}))`, `T(()=>JSON.stringify([${p}]))`, `T(()=>JSON.stringify({k:${p}}))`, `T(()=>JSON.stringify(${p},null,2))`);
}
// Wrappers com valueOf/toString sobrescritos e prototype alterado.
add('JSON.stringify(Object.assign(new Number(1),{valueOf(){return 2},toString(){return "3"}}))',
  'JSON.stringify(Object.assign(new Number(1),{valueOf(){return "x"}}))',
  'JSON.stringify(Object.assign(new String("a"),{toString(){return "b"}}))',
  'JSON.stringify(Object.assign(new String("a"),{valueOf(){return "c"}}))',
  'T(()=>JSON.stringify(Object.assign(new Number(1),{valueOf(){throw new Error("vo")}})))',
  'T(()=>JSON.stringify(Object.assign(new String("a"),{toString(){throw new Error("ts")}})))',
  'JSON.stringify(Object.assign(new Boolean(false),{valueOf(){return true}}))',
  'JSON.stringify(Object.assign(new Number(1),{[Symbol.toPrimitive](){return 9}}))',
  'JSON.stringify(Object.assign(new String("a"),{[Symbol.toPrimitive](){return "p"}}))',
  'T(()=>{var n=new Number(1);Object.setPrototypeOf(n,null);return JSON.stringify(n)})',
  'T(()=>{var s=new String("a");Object.setPrototypeOf(s,null);return JSON.stringify(s)})',
  'JSON.stringify(Object.create(Number.prototype))', 'JSON.stringify(Object.create(String.prototype))', 'JSON.stringify(Object.create(Boolean.prototype))',
  'T(()=>JSON.stringify(Object(1n)))', 'T(()=>{BigInt.prototype.toJSON=function(){return "big"+this};try{return JSON.stringify([1n,Object(2n),{a:3n}])}finally{delete BigInt.prototype.toJSON}})',
  'T(()=>JSON.stringify(1n))', 'T(()=>JSON.stringify({a:1n}))', 'T(()=>JSON.stringify([1n]))', 'T(()=>JSON.stringify({a:[{b:1n}]}))',
  'T(()=>JSON.stringify(10n**30n))', 'T(()=>JSON.stringify(-1n))', 'T(()=>JSON.stringify(1n,()=>"x"))', 'T(()=>JSON.stringify({a:1n},(k,v)=>typeof v==="bigint"?String(v):v))',
  'T(()=>JSON.stringify({a:1n},["b"]))', 'T(()=>JSON.stringify({b:1n},["b"]))', 'T(()=>JSON.stringify(Object.assign(Object(1n),{toJSON(){return 5}})))',
  'T(()=>{var o={toJSON(){return 2n}};return JSON.stringify(o)})', 'T(()=>JSON.stringify(Object(Symbol())))');

// ---- toJSON.
add('JSON.stringify(new Date(0))', 'JSON.stringify({d:new Date(1e12)})', 'JSON.stringify(new Date(NaN))', 'JSON.stringify({d:new Date(NaN)})',
  'JSON.stringify(new Date(-1))', 'JSON.stringify(new Date(-62198755200000))', 'JSON.stringify(new Date(253402300800000))',
  'JSON.stringify(new Date(-1e14))', 'JSON.stringify(new Date(8.64e15))', 'JSON.stringify(new Date(-8.64e15))',
  'T(()=>{var d=new Date(0);d.toJSON=function(k){return "k:"+k};return JSON.stringify({x:d})})',
  'T(()=>{var d=new Date(0);d.toISOString=()=>"iso";return JSON.stringify(d)})',
  'T(()=>{var d=new Date(0);d.toISOString=()=>{throw new TypeError("iso")};return JSON.stringify(d)})',
  'T(()=>Date.prototype.toJSON.call({toISOString(){return "x"}}))', 'T(()=>Date.prototype.toJSON.call({valueOf(){return NaN},toISOString(){return "x"}}))',
  'T(()=>Date.prototype.toJSON.call({valueOf(){return 1},toISOString:1}))', 'T(()=>Date.prototype.toJSON.call(null))',
  'T(()=>Date.prototype.toJSON.call(1))', 'T(()=>Date.prototype.toJSON.call("x"))', 'T(()=>Date.prototype.toJSON.call({toString(){return "s"},toISOString(){return "t"}}))',
  'T(()=>JSON.stringify({toJSON:1}))', 'T(()=>JSON.stringify({toJSON:null,a:1}))', 'T(()=>JSON.stringify({toJSON(){return undefined}}))',
  'T(()=>JSON.stringify({a:{toJSON(){return undefined}}}))', 'T(()=>JSON.stringify([{toJSON(){return undefined}}]))',
  'T(()=>JSON.stringify({toJSON(k){return typeof k+":"+k}}))', 'T(()=>JSON.stringify({a:{toJSON(k){return k}}}))', 'T(()=>JSON.stringify([{toJSON(k){return typeof k+k}}]))',
  'T(()=>JSON.stringify({toJSON(){return this}}))', 'T(()=>JSON.stringify({a:1,toJSON(){return {b:this.a}}}))',
  'T(()=>JSON.stringify({toJSON(){return {toJSON(){return 5}}}}))', 'T(()=>JSON.stringify({get toJSON(){throw new Error("getter")}}))',
  'T(()=>JSON.stringify({a:{get toJSON(){throw new RangeError("g2")}}}))', 'T(()=>JSON.stringify({toJSON(){throw new Error("tj")}}))',
  'T(()=>JSON.stringify({get a(){throw new Error("ga")}}))', 'T(()=>JSON.stringify([{get a(){throw new Error("ga2")}}]))',
  'T(()=>JSON.stringify({toJSON(){return 1n}}))', 'T(()=>JSON.stringify({toJSON(){return Symbol()}}))', 'T(()=>JSON.stringify({toJSON(){return ()=>1}}))',
  'T(()=>{String.prototype.toJSON=function(){return "S"};try{return JSON.stringify(["a",new String("b"),{k:"c"}])}finally{delete String.prototype.toJSON}})',
  'T(()=>{Number.prototype.toJSON=function(){return "N"};try{return JSON.stringify([1,new Number(2),{k:3}])}finally{delete Number.prototype.toJSON}})',
  'T(()=>{Boolean.prototype.toJSON=function(){return "B"};try{return JSON.stringify([true,{k:false}])}finally{delete Boolean.prototype.toJSON}})',
  'T(()=>{Object.prototype.toJSON=function(){return "O"};try{return JSON.stringify([{},[],1,"s",null])}finally{delete Object.prototype.toJSON}})',
  'T(()=>{Array.prototype.toJSON=function(){return "A"};try{return JSON.stringify({a:[1]})}finally{delete Array.prototype.toJSON}})',
  'T(()=>{Symbol.prototype.toJSON=function(){return "Y"};try{return JSON.stringify([Symbol("q")])}finally{delete Symbol.prototype.toJSON}})',
  'T(()=>JSON.stringify(new Proxy({},{get(t,k){return k==="toJSON"?()=>"viaProxy":undefined}})))',
  'T(()=>JSON.stringify({toJSON(){return arguments.length}}))', 'T(()=>JSON.stringify({a:{toJSON(){return arguments.length+typeof arguments[0]}}}))');

// ---- replacer função/array.
const objs = ['{a:1,b:2,c:3}', '[1,2,3]', '{a:{b:{c:1}},d:[1,{e:2}]}', '{1:"x",a:"y",0:"z"}', '5', '"s"', 'null', '{a:undefined,b:()=>1,c:Symbol(),d:1}'];
const replFns = [
  '(k,v)=>v', '(k,v)=>k==="a"?undefined:v', '(k,v)=>typeof v==="number"?v+1:v', '(k,v)=>k===""?v:String(k)+":"+typeof v',
  '(k,v)=>Array.isArray(v)?v.map(x=>x):v', '(k,v)=>undefined', '(k,v)=>v&&typeof v==="object"&&!Array.isArray(v)?{...v,z:0}:v',
  '(k,v)=>k==="b"?{n:1}:v', '(k,v)=>k===""?[v]:v', '(k,v)=>typeof v==="string"?v.toUpperCase():v', '(k,v)=>k==="a"?Symbol():v',
  '(k,v)=>k==="a"?()=>1:v', '(k,v)=>k==="a"?1n:v', '(k,v)=>k==="a"?NaN:v', '(k,v)=>k==="c"?new Date(0):v',
  'function(k,v){return this===undefined?"u":(this&&typeof this)==="object"?v:"?"}',
  'function(k,v){if(k==="a")throw new SyntaxError("rep")}', '(k,v)=>{return {toJSON(){return 7}}}',
  'function(k,v){return k===""&&this[""]===v?v:"bad"}', 'function(k,v){return Array.isArray(this)?"A"+k:v}',
  '(k,v)=>typeof k', '(k,v)=>k===""?{a:1,b:2}:v', '(k,v)=>k===""?[1,2]:v',
];
for (const o of objs) for (const f of replFns) add(`T(()=>JSON.stringify(${o},${f}))`);
add('T(()=>{var log=[];JSON.stringify({a:1,b:[2,{c:3}],d:{e:4}},function(k,v){log.push(typeof k+":"+k);return v});return log.join()})',
  'T(()=>{var log=[];JSON.stringify([[1],[2]],function(k,v){log.push(k);return v});return log.join()})',
  'T(()=>{var log=[];JSON.stringify({b:1,a:2,1:3,0:4},function(k,v){log.push(k);return v});return log.join()})',
  'T(()=>{var log=[];JSON.stringify({a:{toJSON(){return {x:1}}}},function(k,v){log.push(k+typeof v);return v});return log.join()})',
  'T(()=>{var log=[];JSON.stringify(new Date(0),function(k,v){log.push(typeof v);return v});return log.join()})',
  'T(()=>{var log=[];JSON.stringify(Object(1),function(k,v){log.push(typeof v);return v});return log.join()})',
  'T(()=>{var log=[];JSON.stringify({a:Object("s")},function(k,v){log.push(typeof v);return v});return log.join()})',
  'T(()=>{var args;JSON.stringify(1,function(){args=arguments.length});return args})');
const arrReplacers = [
  '["a"]', '["a","a"]', '["b","a"]', '[1]', '["1"]', '[1,"1"]', '[0,1,2]', '[new String("a")]', '[new Number(1)]', '[{}]', '[null]', '[undefined]',
  '[true]', '[Symbol()]', '[1n]', '["a","c"]', '["z"]', '[]', '["a",["b"]]', '[new String("a"),"a"]', '[1.5]', '[-0]', '["a","b","a","b"]',
  '[NaN]', '[Infinity]', '["toString"]', '[0.0]', '["01"]', '[1e21]', '[new Number(NaN)]', '[new Boolean(true)]',
];
for (const r of arrReplacers) {
  for (const o of ['{a:1,b:{a:2,b:3,c:4},c:[{a:5,b:6}],1:"x",0:"y"}', '[{a:1,b:2},{b:3}]', '{a:1}']) add(`T(()=>JSON.stringify(${o},${r}))`);
}
add('T(()=>JSON.stringify({a:1,b:2},Object.assign(["a"],{length:5})))', 'T(()=>JSON.stringify({a:1,b:2},new Proxy(["b"],{})))',
  'T(()=>JSON.stringify({a:1,b:2},Object.assign(["a","b"],{x:1})))', 'T(()=>JSON.stringify({a:1,b:2},{0:"a",length:1}))',
  'T(()=>JSON.stringify({a:1},new String("a")))', 'T(()=>JSON.stringify({a:1},"a"))', 'T(()=>JSON.stringify({a:1},1))', 'T(()=>JSON.stringify({a:1},{}))',
  'T(()=>JSON.stringify({a:1},null))', 'T(()=>JSON.stringify({a:1},undefined,1))', 'T(()=>JSON.stringify({a:1},Symbol()))',
  'T(()=>JSON.stringify({a:1,b:2},[{toString(){return "a"}}]))', 'T(()=>JSON.stringify({a:1,b:2},[new String("b")]))',
  'T(()=>JSON.stringify({a:1,b:2},[{valueOf(){return "a"}}]))', 'T(()=>JSON.stringify({a:1},[{toString(){throw new Error("rk")}}]))',
  'T(()=>JSON.stringify({a:1,2:3},Object.assign(function(){},{})))', 'T(()=>JSON.stringify({a:1},class{}))',
  'T(()=>JSON.stringify({a:[{a:1,b:2}],b:3},["a"]))', 'T(()=>JSON.stringify([{a:1,b:2}],["a"]))', 'T(()=>JSON.stringify("x",["a"]))',
  'T(()=>JSON.stringify({"":1,a:2},[""]))', 'T(()=>JSON.stringify({a:{b:1}},["a"]))', 'T(()=>JSON.stringify({__proto__:{a:1},b:2},["a","b"]))',
  'T(()=>JSON.stringify(Object.create({a:1},{b:{value:2,enumerable:true}}),["a","b"]))');

// ---- space.
const spaces = ['0', '1', '2', '10', '11', '100', '-1', '0.9', '1.9', '2.5', '10.9', '11.5', 'NaN', 'Infinity', '-Infinity', '1e21', '"  "', '"\\t"', '"ab"',
  '"abcdefghij"', '"abcdefghijk"', '"abcdefghijklmnop"', '""', '"\\n"', '"\\ud800"', '"é"', '"😀😀😀😀😀😀"', 'new Number(3)', 'new Number(11)', 'new Number(-3)',
  'new String("xy")', 'new String("")', 'new String("12345678901")', 'true', 'false', 'null', 'undefined', '{}', '[]', '[1]', 'Symbol()', '1n', '()=>2',
  '{valueOf(){return 3}}', '{toString(){return "T"}}', 'Object.assign(new Number(2),{valueOf(){return 4}})', 'Object.assign(new String("a"),{toString(){return "zz"}})',
  'Object.assign(new Number(2),{valueOf(){throw new Error("sp")}})', 'Object.assign(new String("a"),{toString(){throw new Error("sp2")}})',
  '"\\u2028"', '"1"', '" "', '"\\u0000"', '"\\r\\n"', 'new Boolean(true)', '3.999', '-0', '+0', '9.99', '10.0001', '1.0000000001'];
const spaceVals = ['{a:1,b:[1,2,{c:3}],d:{}}', '[]', '{}', '[[],{}]', '[1,[2,[3]]]', '{a:[]}', '{a:{b:{}}}', '"s"', '[undefined,()=>1]', '{a:undefined}'];
for (const s of spaces) for (const v of spaceVals.slice(0, 5)) add(`T(()=>JSON.stringify(${v},null,${s}))`);
for (const s of ['2', '"\\t"', '"abcdefghijk"', 'new Number(3)']) for (const v of spaceVals.slice(5)) add(`T(()=>JSON.stringify(${v},null,${s}))`);
add('T(()=>JSON.stringify({a:1,b:[2]},["a","b"],2))', 'T(()=>JSON.stringify({a:1,b:[2]},(k,v)=>v,"--"))', 'T(()=>JSON.stringify([1,2],null,"\\ud83d\\ude00x"))',
  'T(()=>JSON.stringify({a:1},null,"\\ud83d\\ude00\\ud83d\\ude00\\ud83d\\ude00\\ud83d\\ude00\\ud83d\\ude00\\ud83d\\ude00"))',
  'T(()=>JSON.stringify({a:{b:1}},null,"ab\\ncd"))', 'T(()=>JSON.stringify({a:[]},null,1))', 'T(()=>JSON.stringify({a:{}},null,1))',
  'T(()=>JSON.stringify([[]],null,1))', 'T(()=>JSON.stringify({a:1},null,{}))', 'T(()=>JSON.stringify({a:1},undefined,"x"))');

// ---- Ciclos: mensagem exata do TypeError com a cadeia.
const cycles = [
  'var a={};a.a=a;JSON.stringify(a)',
  'var a={};a.self=a;JSON.stringify(a)',
  'var a=[];a[0]=a;JSON.stringify(a)',
  'var a={};var b={a};a.b=b;JSON.stringify(a)',
  'var a={};var b={a};a.b=b;JSON.stringify(b)',
  'var a=[];var b=[a];a.push(b);JSON.stringify(a)',
  'var a={x:[]};a.x.push({y:a});JSON.stringify(a)',
  'var a={};a.k={};a.k.j={};a.k.j.i=a;JSON.stringify(a)',
  'var a={};a.k={};a.k.j={};a.k.j.i=a.k;JSON.stringify(a)',
  'var a={};a.k=[{}];a.k[0].z=a.k;JSON.stringify(a)',
  'var a={};a.b=[a];JSON.stringify(a)',
  'var a=[{}];a[0].a=a;JSON.stringify(a)',
  'var a={toJSON(){return this}};JSON.stringify(a)',
  'var a={};a.x={toJSON(){return a}};JSON.stringify(a)',
  'var a={};a.x={toJSON(){return {y:a}}};JSON.stringify(a)',
  'var a={};JSON.stringify(a,(k,v)=>k==="n"?a:v)',
  'var a={n:1};JSON.stringify(a,(k,v)=>k==="n"?a:v)',
  'var a={};a.a=a;JSON.stringify(a,null,2)',
  'var a={};a.a=a;JSON.stringify(a,["a"])',
  'var a={};a.a=a;JSON.stringify([a])',
  'var a={};a.a=a;JSON.stringify({z:{y:[a]}})',
  'var a={};a.a=a;try{JSON.stringify(a)}catch(e){e.constructor===TypeError}',
  'var p=new Proxy({},{get(t,k,r){return k==="self"?r:undefined},ownKeys(){return["self"]},getOwnPropertyDescriptor(){return{value:1,enumerable:true,configurable:true}}});JSON.stringify(p)',
  'var a={};var b={};a.b=b;b.a=a;JSON.stringify({root:a})',
  'var o={};o[1]=o;JSON.stringify(o)', 'var o={};o[Symbol()]=o;o.x=o;JSON.stringify(o)', 'var o={"a b":0};o["a b"]=o;JSON.stringify(o)',
  'var o={};o[""]=o;JSON.stringify(o)', 'var o={};o["\\n"]=o;JSON.stringify(o)', 'var o={};o["é"]=o;JSON.stringify(o)',
  'var a=[1,2];a[5]=a;JSON.stringify(a)', 'var o={a:[{b:[{c:null}]}]};o.a[0].b[0].c=o.a[0];JSON.stringify(o)',
  'var o={};var l=o;for(var i=0;i<30;i++){l.n={};l=l.n}l.n=o;JSON.stringify(o)',
  'var o={};var l=o;for(var i=0;i<3;i++){l.n=[{}];l=l.n[0]}l.n=o;JSON.stringify(o)',
  'class K{constructor(){this.me=this}}JSON.stringify(new K)',
  'var o=Object.create(null);o.me=o;JSON.stringify(o)', 'var m=new Map;m.m=m;JSON.stringify(m)', 'var f=function(){};f.f=f;JSON.stringify(f)',
  'var o={a:Object(1)};o.a.o=o;JSON.stringify(o)', 'var a={};a.a=a;JSON.stringify(a,(k,v)=>v)',
  'var d=new Date(0);d.d=d;JSON.stringify(d)', 'var a={};a.a=a;JSON.stringify(a,null,"\\t")',
];
for (const c of cycles) add(`T(()=>{${c.startsWith("var") ? c.replace(/;([^;]*)$/, ";return $1") : "return " + c}})`);
// Compartilhado sem ciclo é válido.
add('T(()=>{var s={x:1};return JSON.stringify({a:s,b:s,c:[s,s]})})', 'T(()=>{var s=[];return JSON.stringify([s,s,s])})');
// Profundidade no stringify.
for (const n of [1000, 5000, 10000, 100000]) {
  add(`T(()=>{var o={};var l=o;for(var i=0;i<${n};i++){l.n={};l=l.n}return JSON.stringify(o).length})`);
  add(`T(()=>{var a=[];var l=a;for(var i=0;i<${n};i++){var x=[];l.push(x);l=x}return JSON.stringify(a).length})`);
  add(`T(()=>{var o={};var l=o;for(var i=0;i<${n};i++){l.n={};l=l.n}return JSON.stringify(o,null,1).length})`);
}

// ---- Symbol, Proxy, ordem de chaves, esparsos, typed arrays.
add('T(()=>JSON.stringify({[Symbol("k")]:1,a:2}))', 'T(()=>JSON.stringify({a:Symbol("v"),b:2}))', 'T(()=>JSON.stringify([Symbol("v"),2]))',
  'T(()=>JSON.stringify(Symbol("v")))', 'T(()=>JSON.stringify({[Symbol.toPrimitive]:1}))', 'T(()=>JSON.stringify({a:1},(k,v)=>typeof k==="symbol"?1:v))',
  'T(()=>JSON.stringify(Object.defineProperty({},Symbol("hidden"),{value:1,enumerable:true})))',
  'T(()=>JSON.stringify(Object.defineProperty({a:1},"h",{value:2,enumerable:false})))',
  'T(()=>JSON.stringify(Object.create({inherited:1},{own:{value:2,enumerable:true}})))',
  'T(()=>JSON.stringify(new Proxy({a:1,b:2},{})))', 'T(()=>JSON.stringify(new Proxy([1,2,3],{})))', 'T(()=>JSON.stringify(new Proxy(function(){},{})))',
  'T(()=>JSON.stringify({p:new Proxy({a:1},{})}))', 'T(()=>JSON.stringify(new Proxy({a:1},{ownKeys(){return["a","b"]},getOwnPropertyDescriptor(t,k){return{value:1,enumerable:k==="b",configurable:true}},get(t,k){return k}})))',
  'T(()=>{var log=[];JSON.stringify(new Proxy({a:1,b:2},{ownKeys(t){log.push("ownKeys");return Reflect.ownKeys(t)},getOwnPropertyDescriptor(t,k){log.push("gopd:"+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},get(t,k,r){log.push("get:"+String(k));return Reflect.get(t,k,r)},has(t,k){log.push("has");return k in t}}));return log.join()})',
  'T(()=>{var log=[];JSON.stringify(new Proxy([1,2],{get(t,k,r){log.push("get:"+String(k));return Reflect.get(t,k,r)},getOwnPropertyDescriptor(t,k){log.push("gopd:"+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},ownKeys(t){log.push("ownKeys");return Reflect.ownKeys(t)}}));return log.join()})',
  'T(()=>JSON.stringify(new Proxy({},{ownKeys(){throw new Error("ok")}})))', 'T(()=>JSON.stringify(new Proxy({},{get(){throw new Error("pg")}})))',
  'T(()=>{var r=Proxy.revocable({},{});r.revoke();return JSON.stringify(r.proxy)})', 'T(()=>{var r=Proxy.revocable([],{});r.revoke();return JSON.stringify(r.proxy)})',
  'T(()=>{var r=Proxy.revocable([],{});r.revoke();return JSON.stringify({a:r.proxy})})', 'T(()=>JSON.stringify({a:1},new Proxy(["a"],{})))',
  'T(()=>{var r=Proxy.revocable([],{});r.revoke();return JSON.stringify({a:1},r.proxy)})', 'T(()=>JSON.stringify(new Proxy({a:1},{get(t,k){return k==="toJSON"?function(){return "tj"}:t[k]}})))',
  'T(()=>JSON.parse("[1]",new Proxy(function(k,v){return v},{})))', 'T(()=>JSON.stringify(new Proxy(Object.create(null),{})))',
  'T(()=>Array.isArray(new Proxy([],{}))+JSON.stringify(new Proxy([],{})))', 'T(()=>JSON.stringify(new Proxy(new Date(0),{})))',
  'T(()=>JSON.stringify(new Proxy(new Number(1),{})))', 'T(()=>JSON.stringify(new Proxy(new String("a"),{})))', 'T(()=>JSON.stringify(new Proxy(new Boolean(true),{})))');
// Ordem de chaves.
const keyOrders = [
  '{b:1,a:2,1:3,0:4}', '{"-1":1,"1":2,"01":3,"1.5":4,"4294967294":5,"4294967295":6,"4294967296":7}', '{z:1,[Symbol()]:2,2:3,y:4,1:5}',
  '{"10":1,"9":2,"a":3,"8":4}', '{"2":1,"1":2}', '{"1e3":1,"1000":2}', '{"9007199254740991":1,"9007199254740992":2,"0":3}', '{"-0":1,"0":2}',
  '{"":1,"0":2}', '{"00":1,"0":2}', '{"0x1":1,"1":2}', '{"+1":1,"1":2}', '{"1 ":1,"1":2}', '{"4294967295":1,"4294967294":2}',
  'Object.assign({a:1},{0:1})', 'Object.fromEntries([["b",1],["a",2],["1",3]])', 'Object.defineProperty({b:1},"0",{value:1,enumerable:true})',
  '(()=>{var o={};o.b=1;o[1]=2;o.a=3;o[0]=4;delete o.b;o.b=5;return o})()', '(()=>{var o={a:1,b:2};delete o.a;o.a=3;return o})()',
  'Object.setPrototypeOf({b:1},{a:1,0:1})', 'Object.create({x:1},{y:{value:1,enumerable:true},3:{value:2,enumerable:true}})',
];
for (const k of keyOrders) add(`T(()=>JSON.stringify(${k}))`, `T(()=>JSON.stringify(${k},null,1))`, `T(()=>Object.keys(JSON.parse(JSON.stringify(${k}))).join())`);
// Arrays esparsos e especiais.
const arrays = [
  '[,]', '[,,]', '[1,,3]', '[,1]', '[1,]', 'new Array(3)', 'Object.assign(new Array(3),{1:"x"})', '(()=>{var a=[1,2,3];a.length=5;return a})()',
  '(()=>{var a=[];a[2]=1;return a})()', '(()=>{var a=[1];a.x=2;return a})()', '(()=>{var a=[1];a["-1"]=2;return a})()', '(()=>{var a=[1];a[1.5]=2;return a})()',
  '[undefined,,null]', '[[],[,]]', '(()=>{var a=[1,2,3];delete a[1];return a})()', 'Object.assign([],{length:2})', '[1,2,3].map(x=>x>1?undefined:x)',
  '(()=>{var a=[1,2,3];Object.defineProperty(a,1,{get(){return "g"},enumerable:true});return a})()',
  '(()=>{var a=[1,2,3];Object.defineProperty(a,1,{get(){throw new Error("ag")}});return a})()',
  '(()=>{var a=[1,2,3];Object.defineProperty(a,1,{value:5,enumerable:false});return a})()', 'Array.from({length:3},(_, i)=>i)',
  '(()=>{var a=[1,2];a.length=0;return a})()', '(()=>{var a=[1];Object.defineProperty(a,"length",{value:3});return a})()',
  'Array.prototype.concat.call(1,2)', '(()=>{class MyA extends Array{};var a=new MyA;a.push(1,2);return a})()', 'Object.assign(Object.create(Array.prototype),{length:2,0:1,1:2})',
  '{length:2,0:"a",1:"b"}', '(function(){return arguments})(1,2,3)', '(()=>{var a=[1,2,3];a.toJSON=()=>"tj";return a})()', '(()=>{var a=[];a.length=4294967295;return a.length})()',
];
for (const a of arrays) add(`T(()=>JSON.stringify(${a}))`, `T(()=>JSON.stringify(${a},null,1))`);
// Typed arrays, ArrayBuffer e afins.
const typed = ['Int8Array', 'Uint8Array', 'Uint8ClampedArray', 'Int16Array', 'Uint16Array', 'Int32Array', 'Uint32Array', 'Float32Array', 'Float64Array', 'BigInt64Array', 'BigUint64Array'];
for (const t of typed) {
  const big = t.startsWith("Big");
  const init = big ? "[1n,2n,3n]" : "[1,2,3]";
  add(`T(()=>JSON.stringify(new ${t}(${init})))`, `T(()=>JSON.stringify(new ${t}(0)))`, `T(()=>JSON.stringify({a:new ${t}(${init})}))`,
    `T(()=>JSON.stringify(new ${t}(${init}),null,1))`, `T(()=>JSON.stringify(new ${t}(2),["0"]))`, `T(()=>JSON.stringify(new ${t}(${init}).subarray(1)))`,
    `T(()=>JSON.stringify(Object.getPrototypeOf(${t}.prototype)===Object.getPrototypeOf(Int8Array.prototype)))`);
}
add('T(()=>JSON.stringify(new Float32Array([NaN,Infinity,-0,1.5,0.1])))', 'T(()=>JSON.stringify(new Float64Array([NaN,Infinity,-0,1.5,0.1])))',
  'T(()=>JSON.stringify(new ArrayBuffer(4)))', 'T(()=>JSON.stringify(new DataView(new ArrayBuffer(4))))', 'T(()=>JSON.stringify(new SharedArrayBuffer(4)))',
  'T(()=>JSON.stringify(new Uint8Array(new ArrayBuffer(8,{maxByteLength:16}))))', 'T(()=>JSON.stringify(Object.assign(new Uint8Array(2),{x:1})))',
  'T(()=>JSON.stringify(new Uint8Array([1,2]),(k,v)=>v))', 'T(()=>JSON.stringify([new Uint8Array(1),new Int8Array(1)]))',
  'T(()=>JSON.stringify(new Uint8Array([1,2]),["1"]))', 'T(()=>JSON.stringify(Array.from(new Uint8Array([1,2]))))',
  'T(()=>JSON.stringify(new Uint8Array(new ArrayBuffer(2),0,0)))', 'T(()=>{var u=new Uint8Array(2);Object.defineProperty(u,"toJSON",{value(){return "u"}});return JSON.stringify(u)})',
  'T(()=>JSON.stringify(Buffer.from("ab")))', 'T(()=>JSON.stringify(new Uint8Array([255,0]).buffer))',
  'T(()=>JSON.stringify(new Map([[1,2]])))', 'T(()=>JSON.stringify(new Set([1,2])))', 'T(()=>JSON.stringify(new WeakSet))', 'T(()=>JSON.stringify([new Map,new Set]))',
  'T(()=>JSON.stringify(new Error("x")))', 'T(()=>JSON.stringify(Object.assign(new Error("x"),{code:1})))', 'T(()=>JSON.stringify(/re/))', 'T(()=>JSON.stringify(Object.assign(/re/,{a:1})))',
  'T(()=>JSON.stringify(Promise.resolve(1)))', 'T(()=>JSON.stringify(new (class A{constructor(){this.a=1}get g(){return 2}static s=3})))',
  'T(()=>JSON.stringify(Math))', 'T(()=>JSON.stringify(JSON))', 'T(()=>JSON.stringify(Reflect))', 'T(()=>JSON.stringify(globalThis.Atomics))',
  'T(()=>JSON.stringify(Object.create(null)))', 'T(()=>JSON.stringify(Object.create(null,{a:{value:1,enumerable:true}})))',
  'T(()=>JSON.stringify(new Intl.NumberFormat))', 'T(()=>JSON.stringify(new WeakRef({})))', 'T(()=>JSON.stringify(function*(){}()))',
  'T(()=>JSON.stringify([].values()))', 'T(()=>JSON.stringify(new URL("http://a.b/c")))', 'T(()=>JSON.stringify(new URLSearchParams("a=1")))',
  'T(()=>JSON.stringify(new TextEncoder().encode("é")))', 'T(()=>JSON.stringify(Object(Symbol.iterator)))');
// Strings: well-formed por posição.
const wf = ['\\ud800', '\\udc00', '\\ud800\\ud800', '\\udc00\\udc00', '\\ud800\\udc00', '\\udc00\\ud800', 'a\\ud800', '\\ud800a', 'a\\udc00b', '\\ud83d\\ude00\\ud83d',
  '\\ud83d\\ud83d\\ude00', '\\ude00\\ud83d\\ude00', '\\udbff\\udfff', '\\udbff\\udbff', '\\udfff\\udbff', '\\ud7ff\\ue000', '\\ud800\\u0041', '\\u0041\\udc00'];
for (const s of wf) {
  add(`T(()=>JSON.stringify("${s}"))`, `T(()=>JSON.stringify({"${s}":"${s}"}))`, `T(()=>JSON.stringify(["${s}"]))`, `T(()=>JSON.stringify(new String("${s}")))`,
    `T(()=>JSON.stringify("${s}").length)`, `T(()=>JSON.parse(JSON.stringify("${s}")).length)`, `T(()=>JSON.stringify({a:"${s}"},null,"${s}"))`);
}
for (let c = 0; c < 0x20; c++) add(`T(()=>JSON.stringify(String.fromCharCode(${c})))`);
for (const c of [0x7f, 0x80, 0x9f, 0xa0, 0xad, 0x2028, 0x2029, 0xfeff, 0xfffe, 0xffff, 0x22, 0x5c, 0x2f, 0x27, 0x60, 0x3c, 0x3e, 0x26]) {
  add(`T(()=>JSON.stringify(String.fromCharCode(${c})))`, `T(()=>JSON.stringify({[String.fromCharCode(${c})]:1}))`,
    `T(()=>JSON.parse('"'+String.fromCharCode(${c})+'"').charCodeAt(0))`);
}
add('T(()=>JSON.stringify("\\u{10ffff}"))', 'T(()=>JSON.stringify("\\u{1f600}"))', 'T(()=>JSON.stringify("a".repeat(100000)).length)',
  'T(()=>JSON.stringify("\\n".repeat(1000)).length)', 'T(()=>JSON.stringify({["k".repeat(1000)]:1}).length)',
  'T(()=>JSON.parse(JSON.stringify("\\u0000\\u001f\\"\\\\")).length)', 'T(()=>JSON.stringify(["a","b"],null,"\\t").split("\\n").length)');
// Números em stringify.
for (const n of ['0', '-0', '1', '-1', '0.1', '1e21', '1e-7', '1e-6', '123456789.123456789', '2**53', '2**53+2', '2**64', '2**-1074', '1/3', '100', '1e100', '-1e-100', '0.000001', '1.5e300', '4.35', '0.1*3', '1e23', '5e-324', '1.7976931348623157e308', '999999999999999900000', '1e20', '123e-20']) {
  add(`T(()=>JSON.stringify(${n}))`, `T(()=>JSON.stringify([${n}]))`, `T(()=>JSON.stringify({"${n}":${n}}))`);
}
// Objetos quaisquer: getters, defineProperty, herança, frozen.
add('T(()=>JSON.stringify({get a(){return 1},get b(){return undefined},set c(v){}}))', 'T(()=>JSON.stringify(Object.freeze({a:1,b:[1]})))',
  'T(()=>JSON.stringify(Object.seal({a:1})))', 'T(()=>JSON.stringify(Object.preventExtensions({a:1})))', 'T(()=>{var o={a:1,b:2};return JSON.stringify(o,function(k,v){if(k==="a")delete this.b;return v})})',
  'T(()=>{var o={a:1,b:2};return JSON.stringify(o,function(k,v){if(k==="a")this.c=3;return v})})',
  'T(()=>{var o={get a(){delete this.b;return 1},b:2};return JSON.stringify(o)})', 'T(()=>{var o={get a(){this.c=3;return 1},b:2};return JSON.stringify(o)})',
  'T(()=>{var o={get a(){Object.defineProperty(this,"b",{enumerable:false});return 1},b:2};return JSON.stringify(o)})',
  'T(()=>{var o={a:1,b:2};Object.defineProperty(o,"a",{get(){delete o.b;return 1},enumerable:true});return JSON.stringify(o)})',
  'T(()=>{var a=[1,2,3];Object.defineProperty(a,0,{get(){a.length=1;return 0},enumerable:true});return JSON.stringify(a)})',
  'T(()=>{var a=[1,2,3];Object.defineProperty(a,0,{get(){a.push(4);return 0},enumerable:true});return JSON.stringify(a)})',
  'T(()=>{var o={toJSON(){return {a:1}}};return JSON.stringify([o,o])})', 'T(()=>JSON.stringify({a:1}.constructor))',
  'T(()=>JSON.stringify(Object.assign(Object.create({toJSON(){return "inh"}}),{a:1})))', 'T(()=>JSON.stringify(Object.defineProperty({},"a",{value:1})))',
  'T(()=>JSON.stringify(new (class extends Array{})))', 'T(()=>JSON.stringify(Object.assign(()=>{}, {a:1})))',
  'T(()=>JSON.stringify(new (function F(){this.a=1})))', 'T(()=>JSON.stringify(new (class{#p=1;a=2})))',
  'T(()=>JSON.stringify({__proto__:null,a:1}))', 'T(()=>JSON.stringify({["__proto__"]:1}))', 'T(()=>JSON.stringify(JSON.parse(\'{"__proto__":{"a":1}}\')))',
  'T(()=>JSON.stringify({a:1},null,"  ").replace(/\\n/g,"|"))', 'T(()=>JSON.stringify({a:[1,{b:2}]},null,2).replace(/\\n/g,"|"))',
  'T(()=>JSON.stringify(JSON.parse(\'{"a":[1,2,{"b":null}],"c":"\\\\u00e9"}\')))', 'T(()=>JSON.stringify(JSON.parse("[1,2,3]",(k,v)=>Array.isArray(v)?v.reverse():v)))',
  'T(()=>JSON.stringify(JSON.parse(\'{"b":1,"a":2}\',(k,v)=>v&&typeof v==="object"?Object.fromEntries(Object.entries(v).sort()):v)))',
  'T(()=>JSON.stringify(Reflect.ownKeys(JSON.parse(\'{"a":1,"1":2}\'))))', 'T(()=>JSON.stringify(JSON.parse(\'{"toJSON":1}\')))',
  'T(()=>JSON.stringify(JSON.parse(\'{"constructor":1,"hasOwnProperty":2}\')))', 'T(()=>JSON.parse(\'{"hasOwnProperty":2}\').hasOwnProperty)',
  'T(()=>JSON.parse(\'{"length":2}\').length)', 'T(()=>JSON.parse(\'[1,2]\').constructor===Array)');

// ---- Execução.
const seen = new Set();
const { usesHostApi } = require("./host-api.js");
const { emitRow } = require("./golden-prelude.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
exprs.splice(0, exprs.length, ...exprs.filter((p) => !usesHostApi(p)));
const unique = exprs.filter(e => !seen.has(e) && seen.add(e));
let kept = 0;
let dropped = 0;
for (const expr of unique) {
  const source = '"use strict";\n' + PRELUDE + (/^T\(/.test(expr) ? `globalThis.R = ${expr}` : `globalThis.R = T(()=>{${/^(var|class)\b/.test(expr) ? expr.replace(/;([^;]*)$/, ";return $1") : "return " + expr}})`);
  let result;
  try {
    (0, eval)(source);
    result = String(globalThis.R);
  } catch (e) {
    process.stderr.write("erro de programa: " + JSON.stringify(expr) + " " + e + "\n");
    dropped++;
    continue;
  }
  globalThis.R = undefined;
  if (/\/home\/|\/tmp\/|\/Users\/|\.js:\d/.test(result)) {
    dropped++;
    process.stderr.write("caminho no resultado: " + JSON.stringify(expr) + "\n");
    continue;
  }
  kept++;
  emitRow(JSON.stringify(source) + "\t" + JSON.stringify(result));
}
process.stderr.write(`mantidos ${kept}, descartados ${dropped}\n`);
