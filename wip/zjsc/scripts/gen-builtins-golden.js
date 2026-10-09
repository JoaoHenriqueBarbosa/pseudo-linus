// Gera tests/golden/builtins_bun.tsv: programas de Array, String, Map/Set, Number/Math/BigInt, JSON e globais,
// avaliados no bun com o harness tests/golden/builtins_bun_harness.js (serializa o resultado com os tipos
// preservados, ou `error<TAB>name<TAB>message JSON`). Colunas: fonte, resultado.
// Os programas não alteram protótipos nem o global, então a ordem não importa.
// Uso: timeout 120 bun scripts/gen-builtins-golden.js > tests/golden/builtins_bun.tsv
const fs = require("fs");
const path = require("path");

const harness = (0, eval)(fs.readFileSync(path.join(__dirname, "../tests/golden/builtins_bun_harness.js"), "utf8").trimEnd());

const programs = new Set();
const add = (...sources) => sources.forEach((source) => programs.add(source));
const each = (template, values) => values.forEach((value) => add(template.replace(/\$\$/g, value)));

// ---------------------------------------------------------------- Array
const arrays = ["[]", "[1,2,3]", "[3,1,2]", "[1,,3]", "[,]", "[1,[2,[3,[4]]]]", "[NaN,0,-0,'a']", "[undefined,null,1]"];
const arrayMethods = [
  "at(0)", "at(-1)", "at(5)", "at('1')", "concat([9],[8,[7]])", "concat(1,'a')", "copyWithin(0,1)", "copyWithin(1,0,2)",
  "copyWithin(-1,0)", "entries()", "every(x=>x)", "fill(7)", "fill(7,1)", "fill(7,-1)", "fill(7,1,2)", "filter(x=>x)",
  "find(x=>x>1)", "findIndex(x=>x>1)", "findLast(x=>x>1)", "findLastIndex(x=>x>1)", "flat()", "flat(2)", "flat(Infinity)",
  "flat(0)", "flatMap(x=>[x,x])", "flatMap(x=>x)", "includes(NaN)", "includes(0)", "includes(undefined)", "indexOf(NaN)",
  "indexOf(1)", "indexOf(1,-1)", "join()", "join('-')", "join(undefined)", "join(null)", "keys()", "lastIndexOf(1)",
  "lastIndexOf(3,-1)", "map(x=>x)", "map(String)", "reduce((a,b)=>a+b)", "reduce((a,b)=>a+b,10)", "reduceRight((a,b)=>a+b)",
  "reduceRight((a,b)=>a+'|'+b,'s')", "reverse()", "slice()", "slice(1)", "slice(-2)", "slice(1,-1)", "slice(5)", "some(x=>x>2)",
  "sort()", "sort((a,b)=>b-a)", "splice(1)", "splice(1,1)", "splice(1,0,'x','y')", "splice()", "splice(-1,1,9)", "toLocaleString()",
  "toReversed()", "toSorted()", "toSorted((a,b)=>b-a)", "toSpliced(1,1)", "toSpliced(0,0,'z')", "toSpliced()", "toString()",
  "values()", "with(0,9)", "with(-1,9)", "with(5,9)", "with(-5,9)", "with('a',1)", "pop()", "shift()", "push(4,5)", "unshift(0)",
  "length", "keys().next()", "entries().next()", "values().next()", "[Symbol.iterator]().next()",
];
arrays.forEach((array) => arrayMethods.forEach((method) => add(`${array}.${method}`)));

add(
  "Array.from('abc')", "Array.from({length:3})", "Array.from({length:2,0:'a',1:'b'})", "Array.from([1,2,3],x=>x*2)",
  "Array.from(new Set([1,1,2]))", "Array.from(new Map([[1,2]]))", "Array.from(5)", "Array.from(null)", "Array.from(undefined)",
  "Array.from([1],5)", "Array.of(1,2,3)", "Array.of()", "Array.of(undefined)", "Array.isArray([])", "Array.isArray({length:0})",
  "Array.isArray(new Proxy([],{}))", "Array(3)", "Array(1,2)", "Array('3')", "new Array(2).length", "Array(-1)", "Array(1.5)",
  "Array(4294967296)", "Array(4294967295).length", "new Array(NaN)", "[].length = -1", "(function(){var a=[];a.length=4294967296})()",
  "(function(){var a=[1,2,3];a.length=1;return a})()", "(function(){var a=[1,2,3];a.length=5;return a})()",
  "(function(){var a=[];a[4294967294]=1;return a.length})()", "(function(){var a=[];a[4294967295]=1;return [a.length,Object.keys(a)]})()",
  "Object.groupBy([1,2,3,4],x=>x%2?'odd':'even')", "Object.groupBy('abca',x=>x)", "Object.groupBy([1],null)", "Object.groupBy(null,x=>x)",
  "Map.groupBy([1,2,3,4],x=>x%2)", "Map.groupBy([0,-0,NaN,NaN],x=>x)", "Map.groupBy([1],5)", "Object.getPrototypeOf(Object.groupBy([],x=>x))",
  // array-likes
  "Array.prototype.map.call('abc',x=>x+x)", "Array.prototype.join.call({length:3,0:'a',2:'c'})", "Array.prototype.slice.call({length:2,0:1,1:2})",
  "Array.prototype.push.call({length:2},'x')", "(function(){var o={length:2};Array.prototype.push.call(o,'x','y');return o})()",
  "(function(){var o={0:'a',1:'b',length:2};Array.prototype.reverse.call(o);return o})()",
  "(function(){var o={0:'a',length:1};Array.prototype.pop.call(o);return o})()",
  "(function(){var o={};Array.prototype.pop.call(o);return o})()", "(function(){var o={};Array.prototype.shift.call(o);return o})()",
  "(function(){var o={length:'2',0:1,1:2};Array.prototype.unshift.call(o,0);return o})()",
  "Array.prototype.indexOf.call({length:3,1:'x'},'x')", "Array.prototype.includes.call({length:2,0:NaN},NaN)",
  "Array.prototype.filter.call({length:3,0:1,2:3},x=>true)", "Array.prototype.flat.call({length:1,0:[1,[2]]})",
  "Array.prototype.toSorted.call({length:3,0:3,1:1,2:2})", "Array.prototype.toReversed.call({length:2,0:'a',1:'b'})",
  "Array.prototype.with.call({length:2,0:'a',1:'b'},1,'z')", "Array.prototype.at.call({length:2,0:'a',1:'b'},-1)",
  "Array.prototype.fill.call({length:2},1)", "Array.prototype.copyWithin.call({length:3,0:1,1:2,2:3},0,1)",
  "Array.prototype.concat.call(1,2)", "Array.prototype.concat.call('a','b')", "Array.prototype.map.call(null,x=>x)",
  "Array.prototype.map.call(undefined,x=>x)", "Array.prototype.join.call(null)", "Array.prototype.push.call(undefined)",
  "Array.prototype.forEach.call({length:3},x=>x)", "Array.prototype.splice.call({length:3,0:1,1:2,2:3},1,1)",
  "(function(){var o={length:3,0:1,1:2,2:3};var r=Array.prototype.splice.call(o,1,1);return [r,o]})()",
  "Array.prototype.sort.call({length:3,0:'c',1:'a',2:'b'})", "Array.prototype.lastIndexOf.call({length:2,0:1,1:1},1)",
  // length enorme
  "Array.prototype.push.call({length:2**53-1},1)", "Array.prototype.push.call({length:2**53-1})",
  "Array.prototype.unshift.call({length:2**53-1},1)", "Array.prototype.splice.call({length:2**53-1},0,0,1)",
  "[].concat({length:1,[Symbol.isConcatSpreadable]:true,0:7})",
  "[].concat({length:2,[Symbol.isConcatSpreadable]:true,1:7})",
  "Array.prototype.toSpliced.call({length:2**32},0,0)", "Array.prototype.toSorted.call({length:2**32})",
  "Array.prototype.toReversed.call({length:2**32})", "Array.prototype.with.call({length:2**32},0,1)",
  "Array.prototype.splice.call({length:2**53-1},0,0)", "Array.prototype.pop.call({length:2**53+5,[2**53-2]:'x'})",
  "(function(){var o={length:2**53+5};Array.prototype.pop.call(o);return o.length})()",
  "(function(){var o={length:-5};Array.prototype.push.call(o,1);return o})()",
  "(function(){var o={length:Infinity};Array.prototype.pop.call(o);return o.length})()",
  "Array.prototype.at.call({length:2**53+1},-1)", "Array.prototype.lastIndexOf.call({length:2**53-1},1,-2**53)",
  "Array.prototype.includes.call({length:2**53-1,[2**53-2]:7},7,2**53-3)", "Array.prototype.indexOf.call({length:2**53-1,[2**53-2]:7},7,2**53-3)",
  "Array.prototype.slice.call({length:2**53-1,[2**53-2]:7},2**53-2)", "Array.prototype.fill.call({length:2**53-1},1,2**53-2)",
  "Array.prototype.copyWithin.call({length:2**53-1},2**53-3,2**53-2)", "Array.prototype.flat.call({length:3})",
  "Array.prototype.splice.call({length:2**53-1},2**53-2,1)", "new Array(2**32-1).length",
  "Array.from({length:2**32})", "Array.from({length:-1})", "Array.from({length:NaN})", "Array.from({length:2.7,0:'a',1:'b',2:'c'})",
  "[].fill.call({length:3},0,-1)", "Array.prototype.join.call({length:3})", "Array.prototype.join.call({length:-1})",
  // esparsos
  "(function(){var a=[1,,3];return [a.map(x=>x*2),a.filter(x=>true),a.indexOf(undefined),a.includes(undefined)]})()",
  "(function(){var a=[,,1];var c=0;a.forEach(()=>c++);return c})()", "[1,,3].every(x=>x)", "[,,].some(x=>true)",
  "[1,,3].reduce((a,b)=>a+b)", "[,,].reduce((a,b)=>a+b)", "[,,3].reduce((a,b)=>a+b)", "[,].reduceRight((a,b)=>a+b,'i')",
  "[1,,3].sort()", "[3,,1].sort()", "[3,undefined,,1].sort()", "[3,,1].toSorted()", "[1,,3].toReversed()", "[1,,3].with(0,9)",
  "[1,,3].toSpliced(0,0)", "[1,,3].flat()", "[[1,,2],,[3]].flat()", "[1,,3].flatMap(x=>[x])", "[1,,3].join('-')", "[1,,3].toString()",
  "[1,,3].reverse()", "[1,,3].slice(0,2)", "[1,,3].splice(0,2)", "[1,,3].concat([,4])", "[1,,3].fill(0)", "[1,,3].keys().next()",
  "Array.from([1,,3])", "[...[1,,3]]", "Object.keys([1,,3])", "Object.entries([1,,3])", "[1,,3].find(x=>x===undefined)",
  "[1,,3].findIndex(x=>x===undefined)", "[1,,3].findLast(x=>x===undefined)", "[1,,3].findLastIndex(x=>x===undefined)",
  "[1,,3].copyWithin(0,1)", "[1,,3].entries().next().value", "[1,,3].lastIndexOf(undefined)", "[1,,3].at(1)", "[,'a'].shift()",
  "[,'a'].unshift(1)", "(function(){var a=[1,2,3];a.length=1;a.length=3;return [a,1 in a]})()",
  "(function(){var a=[];a[2]='x';return [a.length,0 in a,a.findIndex(x=>x===undefined)]})()",
  "(function(){var a=[1,2,3];delete a[1];return [a,a.length,a.flat()]})()",
  "(function(){var a=[1,2,3];a[10]=1;return a.map(x=>x)})()",
  // species
  "(function(){class A extends Array{};var a=new A(1,2,3);return [a.map(x=>x) instanceof A,a.filter(x=>x) instanceof A,a.slice() instanceof A,a.splice(0,1) instanceof A,a.concat() instanceof A,a.flat() instanceof A,a.flatMap(x=>x) instanceof A]})()",
  "(function(){class A extends Array{};var a=new A(1,2,3);return [a.toSorted() instanceof A,a.toReversed() instanceof A,a.with(0,1) instanceof A,a.toSpliced(0,1) instanceof A]})()",
  "(function(){class A extends Array{static get [Symbol.species](){return Array}};return new A(1,2).map(x=>x) instanceof A})()",
  "(function(){class A extends Array{static get [Symbol.species](){return null}};return Object.getPrototypeOf(new A(1,2).map(x=>x))===Array.prototype})()",
  "(function(){class A extends Array{static get [Symbol.species](){return 5}};return new A(1,2).map(x=>x)})()",
  "(function(){var a=[1,2];a.constructor={[Symbol.species]:function(n){return {length:0,n:n}}};return a.map(x=>x)})()",
  "(function(){var a=[1,2];a.constructor={[Symbol.species]:function(n){return {length:0,n:n}}};return a.filter(x=>x)})()",
  "(function(){var a=[1,2];a.constructor=5;return a.map(x=>x)})()", "(function(){var a=[1,2];a.constructor=undefined;return a.map(x=>x)})()",
  "(function(){var a=[1,2];a.constructor=null;return a.map(x=>x)})()", "Array[Symbol.species]===Array", "Object.getOwnPropertyDescriptor(Array,Symbol.species).get.name",
  "(function(){var a=[1,2];a.constructor={[Symbol.species]:function(n){return Object.freeze([])}};return a.map(x=>x)})()",
  "(function(){var a=[1,2];a.constructor={[Symbol.species]:function(n){return Object.freeze({length:0})}};return a.slice()})()",
  // erros e mensagens
  "[].reduce((a,b)=>a)", "[].reduceRight((a,b)=>a)", "[1].map(5)", "[1].forEach()", "[1].filter({})", "[1].find(null)", "[1].findLast('x')",
  "[1].some(undefined)", "[1].every(1)", "[1].flatMap(5)", "[1].reduce(5)", "[3,1].sort(5)", "[3,1].sort(null)", "[3,1].toSorted(5)", "[3,1].toSorted(null)",
  "[3,1].sort(undefined)", "[1].findIndex(5)", "[1].findLastIndex(5)", "Object.freeze([1]).push(2)", "Object.freeze([1]).pop()", "Object.freeze([1]).shift()",
  "Object.freeze([1]).unshift(0)", "Object.freeze([1]).splice(0,1)", "Object.freeze([1,2]).reverse()", "Object.freeze([2,1]).sort()", "Object.freeze([1]).fill(0)",
  "Object.freeze([1,2]).copyWithin(0,1)", "Object.freeze([1]).length=0", "(function(){'use strict';Object.freeze([1]).length=0})()",
  "(function(){'use strict';var a=[1];Object.defineProperty(a,'length',{writable:false});a.push(2)})()",
  "(function(){var a=[1];Object.defineProperty(a,'length',{writable:false});a.push(2)})()",
  "(function(){var a=[1];Object.defineProperty(a,'length',{writable:false});a.pop()})()",
  "Object.seal([1]).push(2)", "Object.preventExtensions([1]).push(2)", "[].with(0,1)", "[1].with(1,1)", "[1].with(-2,1)",
  "[1].toSpliced(0,0,...Array(3))", "[1].flat(-1)", "[1].flat('x')", "[1].at(NaN)", "[1].at(Infinity)", "[1,2].at(-Infinity)",
  "[].concat.call(null)", "[].keys.call(null)", "[].values.call(undefined)", "Array.prototype.entries.call(1).next()",
  "Array.prototype.values.call('ab').next()", "Array.prototype.toString.call({join(){return 'J'}})", "Array.prototype.toString.call({})",
  "Array.prototype.toString.call(null)", "Array.prototype.toLocaleString.call([1,null,undefined,2])", "[1,[2,[3]]].toString()",
  "[Symbol()].join()", "[1n].join()", "(function(){var a=[];a[0]=a;return a.join()})()", "(function(){var a=[1];a.push(a);return a.toString()})()",
  "[{toString(){return 'x'}}].join()", "[{toString(){throw new RangeError('boom')}}].join()", "[1,2,3].indexOf(2,-100)", "[1,2,3].lastIndexOf(2,-100)",
  "[1,2,3].lastIndexOf(2,100)", "[1,2,3].slice(NaN)", "[1,2,3].slice(undefined,undefined)", "[1,2,3].slice(-Infinity,Infinity)",
  "[1,2,3].splice(undefined)", "[1,2,3].splice(1,undefined)", "[1,2,3].splice(1,-1)", "[1,2,3].splice(1,Infinity)", "[1,2,3].fill(0,NaN,NaN)",
  "[1,2,3,4,5].copyWithin(0,3)", "[1,2,3,4,5].copyWithin(1,0,3)", "[1,2,3,4,5].copyWithin(-2,-4,-3)", "[1,2,3].includes(3,-1)", "[1,2,3].includes(1,1)",
  "[0].includes(-0)", "[-0].indexOf(0)", "[NaN].indexOf(NaN)", "[NaN].findIndex(Number.isNaN)", "[1,2,3].entries().toString()", "[][Symbol.unscopables]",
  "Object.keys(Array.prototype[Symbol.unscopables])", "Object.getPrototypeOf(Array.prototype[Symbol.unscopables])", "Array.prototype.length",
  "Array.prototype.concat.length", "Array.prototype.push.length", "Array.prototype.splice.length", "Array.prototype.toSpliced.length", "Array.prototype.with.length",
  "Array.prototype.reduce.length", "Array.prototype.flat.length", "Array.prototype.at.name", "Array.prototype[Symbol.iterator]===Array.prototype.values",
  "[3,2,1].sort((a,b)=>a<b)", "[3,2,1,5,4].sort(()=>0)", "['b',undefined,'a'].sort()", "[10,9,1].sort()", "[true,false].sort()", "[[2],[1,5],[1]].sort()",
  "[1,2,3].sort((a,b)=>{throw new TypeError('cmp')})", "['é','e','z','a'].sort()", "['B','a','C'].sort()",
  "[5,1,4].toSorted((a,b)=>a-b)", "[{k:1,v:'a'},{k:1,v:'b'},{k:0,v:'c'}].sort((a,b)=>a.k-b.k).map(x=>x.v)",
);

// ---------------------------------------------------------------- String
const strings = ["''", "'abc'", "'  a b  '", "'a-b-c'", "'ß'", "'İ'", "'\\ud83d\\ude00x'", "'\\ud800'", "'a\\udc00b'", "'aXbXc'", "'abcabc'"];
const stringMethods = [
  "at(0)", "at(-1)", "at(9)", "at(NaN)", "charAt(1)", "charAt(-1)", "charCodeAt(0)", "charCodeAt(9)", "codePointAt(0)", "codePointAt(1)", "codePointAt(9)",
  "concat('x',1,null)", "endsWith('c')", "endsWith('b',2)", "endsWith('')", "includes('b')", "includes('b',2)", "includes('')", "indexOf('b')", "indexOf('b',2)",
  "indexOf('')", "indexOf('',9)", "isWellFormed()", "toWellFormed()", "lastIndexOf('b')", "lastIndexOf('b',0)", "lastIndexOf('')", "localeCompare('b')",
  "localeCompare('abc')", "localeCompare('')", "normalize()", "normalize('NFD')", "normalize('NFKC')", "padEnd(6)", "padEnd(6,'xy')", "padEnd(2)", "padEnd(5,'')",
  "padStart(6)", "padStart(6,'xy')", "padStart(2)", "padStart(5,'')", "repeat(0)", "repeat(2)", "repeat(1)", "replace('b','X')", "replace('b','$&$&')",
  "replace('b','$`')", "replace('b',\"$'\")", "replace('b','$$')", "replace('b','$1')", "replace('b',()=>'F')", "replace('',' ')", "replace(/b/g,'X')", "replace(/(b)/,'[$1]')",
  "replaceAll('b','X')", "replaceAll('','-')", "replaceAll('b','$&$&')", "replaceAll(/b/g,'X')", "replaceAll('b',(m,i)=>i)", "search('b')", "search(/c/)", "search(/z/)",
  "slice(1)", "slice(-2)", "slice(1,-1)", "slice(5,1)", "split('')", "split('b')", "split('b',1)", "split()", "split(undefined,0)", "split(/b/)", "split(/(b)/)", "split(/(?:)/u)",
  "split(/b*/)", "split('',2)", "startsWith('a')", "startsWith('b',1)", "startsWith('')", "substring(1)", "substring(2,0)", "substring(NaN,2)", "substring(-1,2)",
  "substr(1)", "substr(-2,1)", "substr(1,-1)", "toLowerCase()", "toUpperCase()", "toLocaleLowerCase()", "toLocaleUpperCase()", "toString()", "trim()", "trimStart()",
  "trimEnd()", "trimLeft()", "trimRight()", "valueOf()", "length", "match(/b/)", "match(/b/g)", "match(/z/g)", "match('b')", "match()", "matchAll(/b/g).next()",
  "[...matchAll(/b/g)]", "[...matchAll('b')]", "[...matchAll(/(?<x>b)/g)].map(m=>m.groups.x)", "anchor('n')", "big()", "bold()", "fixed()", "fontcolor('red')", "fontsize(3)",
  "italics()", "link('u')", "small()", "strike()", "sub()", "sup()", "[Symbol.iterator]().next()", "[...'']", "Array.from(this=>1)",
];
strings.forEach((string) => stringMethods.forEach((method) => {
  if (method === "Array.from(this=>1)" || method === "[...'']") return;
  if (method.startsWith("[...")) add(method.replace("matchAll", string + ".matchAll"));
  else if (method.startsWith("[Symbol")) add(`${string}${method}`);
  else add(`${string}.${method}`);
}));

add(
  "String.fromCharCode(65,66)", "String.fromCharCode(0x10041)", "String.fromCharCode()", "String.fromCharCode(-1).charCodeAt(0)", "String.fromCodePoint(0x1F600).length",
  "String.fromCodePoint(65,66)", "String.fromCodePoint(-1)", "String.fromCodePoint(1.5)", "String.fromCodePoint(0x110000)", "String.fromCodePoint('x')", "String.fromCodePoint(NaN)",
  "String.fromCodePoint(Infinity)", "String.raw`a\\n${1}b`", "String.raw({raw:['x','y','z']},1,2,3)", "String.raw({raw:[]})", "String.raw({raw:'abc'},1,2)", "String.raw()",
  "String.raw({})", "String(Symbol('q'))", "String(null)", "String(undefined)", "String()", "String(1n)", "String([1,[2]])", "String({})", "String(-0)", "new String('ab').length",
  "typeof new String('a')", "Object.keys(new String('ab'))", "Object.getOwnPropertyNames('ab')", "new String('ab')[1]", "'ab'[5]", "'ab'['1']",
  "''.repeat(2**30)", "'a'.repeat(-1)", "'a'.repeat(Infinity)", "'a'.repeat(2**31)", "'ab'.repeat(2**30)", "'a'.repeat(NaN)", "'a'.padStart(2**31)", "'a'.padEnd(2**31-1)",
  "'a'.padEnd(2**30,'bc').length", "'abc'.normalize('NFX')", "'abc'.normalize(null)", "'abc'.normalize(undefined)", "'abc'.normalize('nfc')", "String.prototype.at.call(null)",
  "String.prototype.trim.call(undefined)", "String.prototype.toString.call(1)", "String.prototype.valueOf.call({})", "String.prototype.toString.call(new String('z'))",
  "String.prototype.includes.call(null,'a')", "'abc'.includes(/b/)", "'abc'.startsWith(/a/)", "'abc'.endsWith(/c/)", "'abc'.replaceAll(/b/,'x')", "'abc'.matchAll(/b/)",
  "'abc'.matchAll({[Symbol.matchAll]:()=>'custom'})", "'abc'.match({[Symbol.match]:()=>'M'})", "'abc'.search({[Symbol.search]:()=>7})", "'abc'.split({[Symbol.split]:()=>'S'})",
  "'abc'.replace({[Symbol.replace]:()=>'R'},'x')", "'abc'.replaceAll({[Symbol.replace]:()=>'RA'},'x')", "'a'.localeCompare('b')", "'b'.localeCompare('a')", "'a'.localeCompare('A')",
  "'a'.localeCompare('a')", "'ä'.localeCompare('z')", "'résumé'.localeCompare('resume')", "'a'.localeCompare()", "['b','a','C'].sort((x,y)=>x.localeCompare(y))",
  "'\\u0041\\u030a'.normalize('NFC').length", "'\\u00c5'.normalize('NFD').length", "'\\ufb01'.normalize('NFKD')", "'\\u1e9b\\u0323'.normalize('NFKC').length",
  "'\\ud83d'.isWellFormed()", "'\\ud83d\\ude00'.isWellFormed()", "'\\ud83d x'.toWellFormed().charCodeAt(0)", "'\\ude00\\ud83d'.toWellFormed().length",
  "'\\ud83d\\ude00'.split('').length", "[...'\\ud83d\\ude00'].length", "'\\ud83d\\ude00'.at(0).length", "'\\ud83d\\ude00'.codePointAt(0)", "'\\ud83d\\ude00'.codePointAt(1)",
  "'\\ud83d\\ude00'.slice(1).charCodeAt(0)", "'\\ud83d\\ude00'.toUpperCase().length", "'\\ud83d\\ude00'.padStart(4,'\\ud83d').length", "'\\ud83d\\ude00'.replace(/./gu,'x')",
  "'\\ud83d\\ude00'.replace(/./g,'x')", "'\\ud83d\\ude00'.split(/(?:)/u).length", "'\\ud83d\\ude00'.split(/(?:)/).length", "encodeURIComponent('\\ud83d\\ude00')",
  "'ǅ'.toLowerCase()", "'ǅ'.toUpperCase()", "'ŉ'.toUpperCase()", "'ΑΣ'.toLowerCase()", "'ΑΣ Σ'.toLowerCase()", "'ı'.toUpperCase()", "'İ'.toLowerCase().length",
  "'ﬃ'.toUpperCase()", "'ß'.toUpperCase()", "'I'.toLocaleLowerCase('tr')", "'i'.toLocaleUpperCase('tr')", "'abc'.toLocaleUpperCase('xx-invalid-')",
  "'a,b,,c'.split(',')", "'a,b,,c'.split(',',-1)", "'a,b,,c'.split(',',2**32+1)", "'abc'.split('',-1)", "'test'.split(/(?:)/)", "'A<B>bold</B>'.split(/<(\\/)?([^<>]+)>/)",
  "'abc'.split(undefined)", "'abc'.split(null)", "''.split('')", "''.split('a')", "''.split(/a/)", "''.split(/(?:)/)", "'ab'.split(/a*?/)", "'ab'.split(/a*/)",
  "'x'.replace(/x/,'$<n>')", "'x'.replace(/(?<n>x)/,'[$<n>]')", "'x'.replace(/(?<n>x)/,'[$<m>]')", "'x'.replace(/(x)/,'$01$10$2')", "'abc'.replace(/(?<first>a)/,(...a)=>JSON.stringify(a.slice(-1)))",
  "'aaa'.replace(/a/g,(m,i)=>i)", "'aaa'.lastIndexOf('a',-5)", "'abc'.substring(Infinity,0)", "'abc'.substr(NaN)", "'abc'.at('x')", "'abc'.codePointAt(-1)", "'abc'.charAt(Infinity)",
  "'abc'.concat()", "'abc'.concat(undefined)", "'abc'.concat(Symbol())", "'abc'+Symbol()", "`${Symbol()}`", "'abc'.indexOf(Symbol())", "'abc'.padStart(5,Symbol())",
  "'  \\t\\n\\u00a0\\ufeff\\u2003x\\u200b '.trim().length", "'\\u180ex'.trim().length", "'\\u0085x'.trim().length", "'x\\u2028'.trim()", "'abc'.search(/(?:)/)",
  "'abc'.match(/(?:)/g)", "'abc'.match(/(?<q>b)/).groups", "'abc'.match(/(?<q>b)/).index", "'abc'.match(/(b)(z)?/)", "'aBc'.match(/b/i).input", "'abcabc'.matchAll(/b/g).toString()",
  "Object.getPrototypeOf('abc'.matchAll(/b/g))===Object.getPrototypeOf(''.matchAll(/x/g))", "'a'.matchAll(/a/g)[Symbol.iterator]().next().value.index",
  "String.prototype.at.length", "String.prototype.padStart.length", "String.prototype.replaceAll.length", "String.prototype.localeCompare.length", "String.prototype.normalize.length",
  "String.prototype.split.length", "String.prototype.concat.length", "String.prototype.trimLeft.name", "String.prototype.trimRight.name", "String.prototype.trimStart.name",
  "String.prototype.isWellFormed.name", "String.prototype[Symbol.iterator].name", "String.prototype.length", "String.fromCodePoint.length", "String.raw.length",
);

// ---------------------------------------------------------------- Map, Set, WeakMap, WeakSet
add(
  "new Map([[1,'a'],[2,'b']])", "new Map([[1,'a'],[1,'b']])", "new Map([[NaN,1],[NaN,2]])", "new Map([[0,1],[-0,2]])", "[...new Map([[-0,1]]).keys()].map(k=>Object.is(k,-0))",
  "new Map([[1,2]]).get(1)", "new Map([[1,2]]).get(2)", "new Map([[1,2]]).has(1)", "new Map([[1,2]]).delete(1)", "new Map([[1,2]]).delete(2)", "new Map([[1,2]]).size",
  "new Map([[1,2]]).set(3,4).size", "new Map([[1,2]]).clear()", "[...new Map([[1,2],[3,4]])]", "[...new Map([[1,2],[3,4]]).keys()]", "[...new Map([[1,2],[3,4]]).values()]",
  "[...new Map([[1,2],[3,4]]).entries()]", "new Map([[1,2]]).forEach((v,k,m)=>m.size)", "(function(){var r=[];new Map([[1,2],[3,4]]).forEach((v,k)=>r.push(k,v));return r})()",
  "(function(){var m=new Map([[1,1]]);var r=[];m.forEach((v,k)=>{r.push(k);if(k<3)m.set(k+1,1)});return r})()",
  "(function(){var m=new Map([[1,1],[2,2],[3,3]]);var r=[];for(var [k] of m){r.push(k);m.delete(2)}return r})()",
  "(function(){var m=new Map([[1,1]]);var it=m.entries();m.clear();m.set(5,5);return it.next()})()", "new Map(null)", "new Map(undefined)", "new Map([])", "new Map(5)", "new Map([1])",
  "new Map(['ab'])", "new Map([[1]])", "new Map({})", "Map()", "Map.prototype.get.call({},1)", "Map.prototype.size", "Object.getOwnPropertyDescriptor(Map.prototype,'size').get.call(new Set)",
  "Map.prototype.set.call(new Set,1,2)", "Map.prototype.has.call(null,1)", "Map.prototype.forEach.call(new Map,1)", "new Map().forEach()", "new Map([[1,2]]).forEach(null)",
  "Map.prototype[Symbol.iterator]===Map.prototype.entries", "Map.prototype[Symbol.toStringTag]", "Object.prototype.toString.call(new Map)", "Object.prototype.toString.call(new Map().entries())",
  "new Map().entries().toString()", "new Map().keys().next()", "Map.prototype.entries.call(new Set)", "Map.length", "Map.name", "Map.prototype.set.length", "Map.prototype.forEach.length",
  "Map[Symbol.species]===Map", "Map.groupBy.length", "Map.groupBy.name", "(function(){class M extends Map{set(k,v){return super.set(k,v+1)}};return new M([[1,1]]).get(1)})()",
  "(function(){var n=0;var o=Map.prototype.set;return new Map([[1,2]]).size})()", "new Map([[{},1],[{},2]]).size", "new Map([['1',1],[1,2]]).size", "new Map([[1n,1],[1n,2]]).size",
  "new Map([[Symbol.iterator,1]]).size", "Map.prototype.getOrInsert", "typeof Map.prototype.getOrInsert", "typeof Map.prototype.getOrInsertComputed", "typeof WeakMap.prototype.getOrInsert",
  "typeof Map.prototype.emplace", "new Map([[1,2]]).getOrInsert(1,5)", "new Map().getOrInsert(1,5)", "new Map().getOrInsertComputed(1,k=>k+1)", "new Map().getOrInsertComputed(1,5)",
  "new Set([1,2,2,3])", "new Set('aab')", "new Set([NaN,NaN,0,-0])", "new Set([1]).add(2).size", "new Set([1]).has(1)", "new Set([1]).delete(1)", "new Set([1]).delete(2)",
  "[...new Set([1,2])]", "[...new Set([1,2]).entries()]", "[...new Set([1,2]).keys()]", "[...new Set([1,2]).values()]", "Set.prototype.keys===Set.prototype.values",
  "Set.prototype[Symbol.iterator]===Set.prototype.values", "new Set([1]).forEach((v,k,s)=>s.size)", "new Set(null)", "new Set(5)", "new Set({})", "Set()", "new Set().forEach(5)",
  "Set.prototype.add.call({},1)", "Set.prototype.has.call(new Map,1)", "Object.getOwnPropertyDescriptor(Set.prototype,'size').get.call(new Map)", "Set.length", "Set.prototype.add.length",
  "Set.prototype.union.length", "Set.prototype.isSubsetOf.name", "Object.prototype.toString.call(new Set)", "Object.prototype.toString.call(new Set().values())",
  "new Set([1,2,3]).union(new Set([3,4]))", "new Set([1,2,3]).intersection(new Set([2,3,4]))", "new Set([1,2,3]).difference(new Set([2]))", "new Set([1,2,3]).symmetricDifference(new Set([3,4]))",
  "new Set([1,2]).isSubsetOf(new Set([1,2,3]))", "new Set([1,2,3]).isSubsetOf(new Set([1,2]))", "new Set([1,2,3]).isSupersetOf(new Set([1,2]))", "new Set([1]).isDisjointFrom(new Set([2]))",
  "new Set([1]).isDisjointFrom(new Set([1]))", "new Set([1,2]).union(new Map([[3,4]]))", "new Set([1,2]).intersection(new Map([[2,4]]))", "new Set([1,2]).union([3])", "new Set([1,2]).union(5)",
  "new Set([1,2]).union({})", "new Set([1,2]).union({size:1,has(){return true},keys(){return [5][Symbol.iterator]()}})", "new Set([1]).union({size:NaN,has(){},keys(){}})",
  "new Set([1]).union({size:-1,has(){},keys(){}})", "new Set([1]).union({size:1,has:5,keys(){}})", "new Set([1]).union({size:1,has(){},keys:5})", "new Set([1]).union({size:'x',has(){},keys(){}})",
  "new Set([1]).union({size:undefined,has(){},keys(){}})", "new Set([3,1,2]).intersection(new Set([2,1]))", "new Set([1,2,3]).intersection({size:1,has:x=>x==3,keys(){return [3][Symbol.iterator]()}})",
  "new Set([1,2]).union.call({},new Set)", "Set.prototype.union.call(new Map,new Set)", "new Set([0]).union(new Set([-0]))", "[...new Set([1,2]).union(new Set([-0]))].map(x=>Object.is(x,-0))",
  "new Set().isSubsetOf(new Set)", "new Set([1]).isSupersetOf(new Set)", "(function(){class S extends Set{};return new S([1]).union(new Set) instanceof S})()",
  "typeof Set.prototype.getOrInsert", "typeof Set.groupBy", "typeof Map.prototype.groupBy", "typeof Set.prototype.map",
  "new WeakMap().set({},1) instanceof WeakMap", "(function(){var k={};var w=new WeakMap([[k,1]]);return [w.get(k),w.has(k),w.delete(k),w.has(k)]})()", "new WeakMap().set(1,1)", "new WeakMap().set('a',1)",
  "new WeakMap().set(null,1)", "new WeakMap().set(Symbol('x'),1) instanceof WeakMap", "new WeakMap().set(Symbol.for('x'),1)", "new WeakMap().set(Symbol.iterator,1) instanceof WeakMap",
  "new WeakMap().get(1)", "new WeakMap().has(1)", "new WeakMap().delete(1)", "new WeakMap([[1,2]])", "new WeakMap([1])", "WeakMap()", "WeakMap.prototype.get.call(new Map,{})", "WeakMap.length", "WeakMap.prototype[Symbol.toStringTag]",
  "Object.getOwnPropertyNames(WeakMap.prototype).sort()", "Object.getOwnPropertyNames(WeakSet.prototype).sort()", "Object.getOwnPropertyNames(Map.prototype).sort()", "Object.getOwnPropertyNames(Set.prototype).sort()",
  "new WeakSet().add(1)", "new WeakSet().add({}) instanceof WeakSet", "(function(){var k={};var w=new WeakSet([k]);return [w.has(k),w.delete(k),w.has(k)]})()", "new WeakSet().has(1)", "new WeakSet().delete('a')",
  "new WeakSet([1])", "WeakSet()", "WeakSet.prototype.add.call(new Set,{})", "new WeakSet().add(Symbol('s')) instanceof WeakSet", "new WeakSet().add(Symbol.for('s'))", "WeakSet.length",
  "new WeakRef({}).deref() !== undefined", "new WeakRef(1)", "typeof FinalizationRegistry", "new FinalizationRegistry(1)", "new FinalizationRegistry(()=>{}).register(1)",
  "new FinalizationRegistry(()=>{}).register({},1,1)", "(function(){var o={};return new FinalizationRegistry(()=>{}).register(o,o)})()", "new FinalizationRegistry(()=>{}).unregister(1)",
);

// ---------------------------------------------------------------- Number
const numbers = ["0", "-0", "1", "1.5", "-1.5", "0.5", "1.005", "123.456", "1e21", "1e-7", "123456789012345680000", "0.000001", "NaN", "Infinity", "-Infinity", "255", "0.1", "5e-324", "1.7976931348623157e308"];
const numberMethods = [
  "toFixed()", "toFixed(0)", "toFixed(1)", "toFixed(2)", "toFixed(5)", "toFixed(20)", "toFixed(100)", "toFixed(101)", "toFixed(-1)", "toFixed(NaN)", "toFixed('2')",
  "toPrecision()", "toPrecision(1)", "toPrecision(2)", "toPrecision(5)", "toPrecision(21)", "toPrecision(100)", "toPrecision(101)", "toPrecision(0)", "toPrecision(undefined)",
  "toExponential()", "toExponential(0)", "toExponential(2)", "toExponential(20)", "toExponential(100)", "toExponential(101)", "toExponential(-1)", "toExponential(undefined)",
  "toString()", "toString(2)", "toString(8)", "toString(16)", "toString(36)", "toString(3)", "toString(10)", "toString(1)", "toString(37)", "toString(0)", "toString(undefined)", "toString(null)",
  "toLocaleString()", "valueOf()",
];
numbers.forEach((number) => numberMethods.forEach((method) => add(`(${number}).${method}`)));
add(
  "Number('')", "Number(' 12 ')", "Number('0x1f')", "Number('0b11')", "Number('0o7')", "Number('1_0')", "Number('1e3')", "Number('.5')", "Number('5.')", "Number('+5')", "Number('--5')",
  "Number('Infinity')", "Number('-Infinity')", "Number('infinity')", "Number(null)", "Number(undefined)", "Number([])", "Number([5])", "Number([1,2])", "Number({})", "Number(true)", "Number(1n)",
  "Number(2n**64n)", "Number(Symbol())", "Number('0x')", "Number('-0x1')", "Number(new Date(5))", "Number.isInteger(5.0)", "Number.isInteger('5')", "Number.isInteger(2**53)", "Number.isSafeInteger(2**53)",
  "Number.isSafeInteger(2**53-1)", "Number.isFinite('1')", "isFinite('1')", "Number.isNaN('x')", "isNaN('x')", "Number.parseFloat===parseFloat", "Number.parseInt===parseInt", "Number.EPSILON",
  "Number.MAX_SAFE_INTEGER", "Number.MIN_SAFE_INTEGER", "Number.MAX_VALUE", "Number.MIN_VALUE", "Number.NEGATIVE_INFINITY", "new Number(5)+1", "typeof new Number(5)", "Number.prototype.toFixed.call('1')",
  "Number.prototype.toString.call('1')", "Number.prototype.valueOf.call({})", "Number.prototype.toFixed.call(new Number(2),1)", "Number.prototype.toFixed.length", "Number.prototype.toString.length",
  "Number.prototype.toPrecision.length", "Number.prototype.toLocaleString.length", "(25).toString(36)", "(0.5).toString(2)", "(0.1).toString(2)", "(-255).toString(16)", "(1e21).toString(7)", "(2**53).toString(2)",
  "(3.14159).toString(8)", "(0.000001).toString(16)", "(1/3).toString(3)", "(123.456).toString(36)", "1..toString()", "1.0.toFixed(1)", "(0.1+0.2)", "0.1*3", "1e21+1", "2**53+1", "5e-324/2", "-5e-324/2",
  "9007199254740993", "0.30000000000000004===0.1+0.2", "(1.45).toFixed(1)", "(8.345).toFixed(2)", "(1e-10).toFixed(2)", "(-1e-10).toFixed(2)", "(-0.0001).toFixed(2)", "(1000000000000000128).toFixed(0)",
  "(1000000000000000128).toString()", "(0.00001).toString()", "(1e300*10).toString()", "(1234.5678).toFixed(2)", "(0).toPrecision(5)", "(0).toExponential(3)", "(1.255).toPrecision(3)", "(99.99).toPrecision(2)",
  "(123456).toExponential(2)", "(0.00015).toPrecision(1)", "(1e21).toPrecision(3)", "(1e-7).toPrecision(3)", "(15).toExponential()", "(NaN).toExponential(500)", "(Infinity).toPrecision(500)", "(Infinity).toFixed(500)",
  "(1).toExponential(1e9)", "(1).toFixed(2**32)", "(1).toString(2**32)", "(1).toPrecision(Infinity)", "parseInt('  42px')", "parseInt('0x1f')", "parseInt('0x')", "parseInt('-0')", "Object.is(parseInt('-0'),-0)",
  "parseInt('12',2)", "parseInt('12',37)", "parseInt('12',1)", "parseInt('12',0)", "parseInt('z',36)", "parseInt('11',2.9)", "parseInt('11',-2)", "parseInt('11',2**32+2)", "parseInt('')", "parseInt('1e3')",
  "parseInt(1e21)", "parseInt(0.0000005)", "parseInt('123456789012345678901234567890')", "parseInt('0b11')", "parseInt('1_000')", "parseInt(null,36)", "parseInt('\\u00a0 7')", "parseInt(Symbol())", "parseInt(1n)",
  "parseInt('9007199254740993')", "parseInt('-9007199254740993')", "parseInt('0x1f',16)", "parseInt('0x1f',10)", "parseFloat('3.14abc')", "parseFloat('.5')", "parseFloat('-.5e1')", "parseFloat('1e')", "parseFloat('1e+')",
  "parseFloat('Infinityx')", "parseFloat('-Infinity')", "parseFloat('infinity')", "parseFloat('0x10')", "parseFloat('')", "parseFloat('  \\n 7')", "parseFloat('1_0')", "parseFloat('1.2.3')", "parseFloat('-0')",
  "Object.is(parseFloat('-0'),-0)", "parseFloat('5e-324')", "parseFloat('1e400')", "parseFloat('.')", "parseFloat('+.1')", "parseFloat(Symbol())", "parseFloat(null)", "parseFloat([1.5,2])",
  "Number.prototype.toFixed.call(null)", "(1).toFixed(1.9)", "(1).toFixed(-0.5)", "(1).toFixed(Symbol())", "(5).toString(Symbol())", "(5).toString(2n)",
);

// ---------------------------------------------------------------- Math
const mathUnary = ["abs", "acos", "acosh", "asin", "asinh", "atan", "atanh", "cbrt", "ceil", "clz32", "cos", "cosh", "exp", "expm1", "floor", "fround", "log", "log10", "log1p", "log2", "round", "sign", "sin", "sinh", "sqrt", "tan", "tanh", "trunc", "f16round"];
const mathArgs = ["0", "-0", "1", "-1", "0.5", "-0.5", "2.5", "-2.5", "NaN", "Infinity", "-Infinity", "'3'", "undefined", "null", "[]", "[4]", "1e308", "5e-324", "2**31", "-2**31", "2**32"];
mathUnary.forEach((name) => mathArgs.forEach((arg) => add(`Math.${name}(${arg})`)));
add(
  "Math.max()", "Math.min()", "Math.max(1,NaN)", "Math.max(0,-0)", "Math.min(0,-0)", "Math.max(-0,0)", "Math.min(-0,0)", "Math.max(1,'3',2)", "Math.max(1,{})", "Math.max(1,undefined)", "Math.max(Symbol())", "Math.max(1n)",
  "Math.max.apply(null,[1,5,3])", "Math.max(...[1,5,3])", "Math.hypot()", "Math.hypot(3,4)", "Math.hypot(NaN,Infinity)", "Math.hypot(1e200,1e200)", "Math.hypot(-0)", "Math.hypot('3','4')", "Math.hypot(3)",
  "Math.atan2(1,1)", "Math.atan2(0,-0)", "Math.atan2(-0,-0)", "Math.atan2(0,0)", "Math.atan2(NaN,1)", "Math.atan2(1,Infinity)", "Math.atan2(Infinity,Infinity)", "Math.pow(2,10)", "Math.pow(NaN,0)", "Math.pow(1,Infinity)",
  "Math.pow(-8,1/3)", "Math.pow(0,-1)", "Math.pow(-0,-1)", "Math.pow(-0,-2)", "2**-1074", "2**1024", "(-2)**2", "(-8)**(1/3)", "Math.imul(2**31,2)", "Math.imul(0xffffffff,5)", "Math.imul(1.9,3.9)", "Math.imul()",
  "Math.round(2.5)", "Math.round(-2.5)", "Math.round(0.49999999999999994)", "Math.round(-0.4)", "Math.round(2**52+0.5)", "Math.round(1.4999999999999998)", "Math.ceil(-0.5)", "Math.floor(-0)", "Math.trunc(-0.9)", "Math.sign(-0)",
  "Math.fround(5.5)", "Math.fround(5.05)", "Math.fround(2**128)", "Math.fround(1e-46)", "Math.clz32(0)", "Math.clz32(1)", "Math.clz32(-1)", "Math.clz32(2**32)", "Math.cbrt(27)", "Math.cbrt(-8)", "Math.expm1(1e-10)",
  "Math.log1p(-1)", "Math.log1p(-2)", "Math.sqrt(-1)", "Math.sqrt(-0)", "Math.log2(8)", "Math.log2(2**-1074)", "Math.log10(1000)", "Math.log(0)", "Math.log(-0)", "Math.exp(-Infinity)", "Math.sinh(1e-300)", "Math.cosh(710)",
  "Math.E", "Math.PI", "Math.LN2", "Math.LN10", "Math.LOG2E", "Math.LOG10E", "Math.SQRT2", "Math.SQRT1_2", "Object.prototype.toString.call(Math)", "Math[Symbol.toStringTag]", "typeof Math.random()", "Math.random()<1",
  "Math.max.length", "Math.min.length", "Math.hypot.length", "Math.atan2.length", "Math.pow.length", "Math.imul.length", "Math.abs.length", "Math.sumPrecise.length", "typeof Math.sumPrecise", "typeof Math.f16round",
  "Math.sumPrecise([1,2,3])", "Math.sumPrecise([0.1,0.2,0.3])", "Math.sumPrecise([1e20,0.1,-1e20])", "Math.sumPrecise([])", "Math.sumPrecise([-0])", "Math.sumPrecise([NaN,1])", "Math.sumPrecise([Infinity,-Infinity])",
  "Math.sumPrecise([Infinity,1])", "Math.sumPrecise([1,'2'])", "Math.sumPrecise(5)", "Math.sumPrecise('ab')", "Math.sumPrecise([1n])", "Math.sumPrecise([1e308,1e308,-1e308])", "Math.sumPrecise([1,2],3)",
  "Math.sumPrecise(new Set([1,2]))", "Math.sumPrecise()", "Math.sumPrecise([0.1,0.1,0.1,0.1,0.1,0.1,0.1,0.1,0.1,0.1])", "Math.sumPrecise([-0,-0])", "Math.sumPrecise([0,-0])",
);

// ---------------------------------------------------------------- BigInt
const bigints = ["0n", "-1n", "1n", "255n", "-255n", "2n**64n", "-(2n**64n)", "123456789012345678901234567890n", "10n**30n"];
const bigintMethods = ["toString()", "toString(2)", "toString(16)", "toString(36)", "toString(37)", "toString(1)", "toString(0)", "toLocaleString()", "valueOf()"];
bigints.forEach((big) => bigintMethods.forEach((method) => add(`(${big}).${method}`)));
add(
  "BigInt(5)", "BigInt('5')", "BigInt(' 0x1f ')", "BigInt('0b101')", "BigInt('0o17')", "BigInt('')", "BigInt('  ')", "BigInt('1.5')", "BigInt('1e3')", "BigInt('abc')", "BigInt('-5')", "BigInt('-0x1')", "BigInt('+5')", "BigInt('5n')",
  "BigInt(1.5)", "BigInt(NaN)", "BigInt(Infinity)", "BigInt(2**53)", "BigInt(1e21)", "BigInt(-0)", "BigInt(true)", "BigInt(false)", "BigInt(null)", "BigInt(undefined)", "BigInt(Symbol())", "BigInt({})", "BigInt([5])", "BigInt([])",
  "BigInt()", "new BigInt(1)", "BigInt.asIntN(8,255n)", "BigInt.asIntN(8,128n)", "BigInt.asIntN(8,-129n)", "BigInt.asUintN(8,-1n)", "BigInt.asUintN(8,256n)", "BigInt.asUintN(64,-1n)", "BigInt.asIntN(64,2n**63n)", "BigInt.asIntN(0,5n)",
  "BigInt.asUintN(0,5n)", "BigInt.asIntN(-1,5n)", "BigInt.asUintN(2**53,5n)", "BigInt.asUintN(2**53-1,5n)", "BigInt.asIntN(1,1n)", "BigInt.asIntN(8,5)", "BigInt.asIntN('8',5n)", "BigInt.asIntN(8)", "BigInt.asUintN(1.5,3n)",
  "BigInt.asIntN(Infinity,3n)", "BigInt.asUintN(8,'300')", "1n+1n", "1n+1", "1n*2", "1n-'1'", "1n+'1'", "'1'+1n", "1n<2", "1n<'2'", "1n==1", "1n===1", "1n==1.5", "2n>1.5", "1n<NaN", "1n==='1'", "1n=='1'", "0n==false", "0n==''",
  "5n/2n", "-5n/2n", "5n%3n", "-5n%3n", "5n%-3n", "5n/0n", "5n%0n", "2n**10n", "2n**-1n", "2n**0n", "0n**0n", "(-2n)**3n", "2n**(2n**64n)", "1n**(2n**64n)", "0n**(2n**64n)", "(-1n)**(2n**64n)", "(-1n)**(2n**64n+1n)",
  "-(-5n)", "+5n", "~5n", "5n&3n", "5n|3n", "5n^3n", "5n<<2n", "5n>>1n", "-5n>>1n", "5n>>>1n", "5n<<-1n", "1n<<(2n**40n)", "1n>>(2n**40n)", "-1n>>(2n**40n)", "0n<<(2n**40n)", "1n<<2n**64n", "5n&-1n", "-5n|2n", "-5n^-3n",
  "typeof 1n", "typeof Object(1n)", "Object(1n)+1n", "!0n", "!!1n", "0n?1:2", "1n&&2", "++Object(0n)", "(function(){var a=1n;a++;return a})()", "(function(){var a=1n;return a++ + a})()", "1n+Symbol()", "1n/1", "Math.max(1n)", "Math.abs(1n)",
  "Number(1n)", "Number(2n**1024n)", "Number(2n**53n+1n)", "Number(-(2n**53n)-1n)", "parseInt('1n')", "isNaN(1n)", "isFinite(1n)", "Number.isInteger(1n)", "JSON.stringify(1n)", "JSON.stringify({a:1n})", "JSON.stringify([1n])",
  "BigInt.prototype.toString.call(1)", "BigInt.prototype.valueOf.call(1)", "BigInt.prototype.toString.call(Object(5n))", "BigInt.prototype.toString.call({})", "BigInt.prototype[Symbol.toStringTag]", "BigInt.length", "BigInt.name",
  "BigInt.asIntN.length", "BigInt.prototype.toString.length", "BigInt.prototype.toLocaleString.length", "Object.getOwnPropertyNames(BigInt).sort()", "Object.getOwnPropertyNames(BigInt.prototype).sort()",
  "[3n,1n,2n].sort()", "[3n,1n,10n].sort((a,b)=>(a<b?-1:a>b?1:0))", "[1n,2]","new Set([1n,1n,1])", "new Map([[1n,'a']]).get(1n)", "[1n].includes(1)", "[1n].indexOf(1n)", "BigInt(Number.MAX_SAFE_INTEGER)+2n",
  "0x10n", "0b11n", "0o7n", "1_000n", "9007199254740993n", "-0n", "Object.is(-0n,0n)", "BigInt('0x'+'f'.repeat(40))", "BigInt('9'.repeat(40))", "(10n**40n).toString(2).length", "(2n**100n).toString(36)", "BigInt.asUintN(200,-1n).toString(16)",
);

// ---------------------------------------------------------------- JSON
add(
  "JSON.stringify(1)", "JSON.stringify('a\"b')", "JSON.stringify(undefined)", "JSON.stringify(null)", "JSON.stringify(()=>1)", "JSON.stringify(Symbol())", "JSON.stringify([undefined,()=>1,Symbol()])", "JSON.stringify({a:undefined,b:()=>1,c:Symbol()})",
  "JSON.stringify(NaN)", "JSON.stringify(-0)", "JSON.stringify(Infinity)", "JSON.stringify([NaN])", "JSON.stringify({a:[1,{b:2}]})", "JSON.stringify({a:1,b:[1,2]},null,2)", "JSON.stringify({a:1},null,'--')", "JSON.stringify({a:1},null,20)",
  "JSON.stringify({a:1},null,'12345678901')", "JSON.stringify([],null,2)", "JSON.stringify({},null,2)", "JSON.stringify([[]],null,2)", "JSON.stringify({a:1,b:2},['b'])", "JSON.stringify({a:1,b:2},['b','b','a'])", "JSON.stringify({1:1},[1])",
  "JSON.stringify({a:1,b:2},(k,v)=>typeof v==='number'?v*2:v)", "JSON.stringify({a:1},(k,v)=>k===''?v:undefined)", "JSON.stringify({a:{b:1}},function(k,v){return k==='b'?this.b+1:v})", "JSON.stringify({toJSON(){return 5}})",
  "JSON.stringify({a:{toJSON(k){return k}}})", "JSON.stringify(new Date(0))", "JSON.stringify(new Date(NaN))", "JSON.stringify(new Map([[1,2]]))", "JSON.stringify(new Set([1]))", "JSON.stringify(/x/)", "JSON.stringify(new Error('e'))",
  "JSON.stringify(new Number(5))", "JSON.stringify(new String('s'))", "JSON.stringify(new Boolean(false))", "JSON.stringify(Object(1n))", "JSON.stringify('\\u2028\\u2029')", "JSON.stringify('\\ud800')", "JSON.stringify('\\ud83d\\ude00')",
  "JSON.stringify('\\u0000\\u001f\\u007f')", "JSON.stringify('\\b\\f\\n\\r\\t')", "JSON.stringify({[Symbol()]:1})", "JSON.stringify({a:1,[Symbol()]:2})", "JSON.stringify(Object.create({a:1}))", "JSON.stringify(Object.defineProperty({},'a',{value:1}))",
  "(function(){var a={};a.a=a;return JSON.stringify(a)})()", "(function(){var a=[];a[0]=a;return JSON.stringify(a)})()", "(function(){var a={b:{}};a.b.c=a;return JSON.stringify(a)})()", "JSON.stringify([1,[2,[3]]],null,1)",
  "JSON.stringify({a:1,b:{c:2}},null,'\\t')", "JSON.stringify({a:1},null,0)", "JSON.stringify({a:1},null,-1)", "JSON.stringify({a:1},null,'')", "JSON.stringify({a:1},null,new Number(2))", "JSON.stringify({a:1},null,new String('x'))",
  "JSON.stringify([,1])", "JSON.stringify({length:1,0:'a'})", "JSON.stringify(new Proxy([1,2],{}))", "JSON.stringify(new Proxy({a:1},{}))", "JSON.stringify(new Uint8Array([1,2]))", "JSON.stringify(Object.create(null))", "JSON.stringify(1,()=>2)",
  "JSON.stringify(2n)", "JSON.stringify({toJSON(){return 1n}})", "JSON.stringify(Object.assign(()=>1,{a:1}))", "JSON.stringify({a:1},5)", "JSON.stringify({a:1},{})", "JSON.stringify({a:1},[{}])", "JSON.stringify({a:1,1:2},[new String('a'),new Number(1)])",
  "JSON.parse('1')", "JSON.parse(' [1 , 2 ] ')", "JSON.parse('{\"a\":{\"b\":[1,2,{\"c\":null}]}}')", "JSON.parse('\"\\\\u0041\"')", "JSON.parse('\"\\\\ud83d\\\\ude00\"')", "JSON.parse('\"\\\\ud800\"').length", "JSON.parse('1e3')", "JSON.parse('-0')", "Object.is(JSON.parse('-0'),-0)",
  "JSON.parse('1E+2')", "JSON.parse('true')", "JSON.parse('null')", "JSON.parse(null)", "JSON.parse(1)", "JSON.parse(true)", "JSON.parse([1])", "JSON.parse('')", "JSON.parse(' ')", "JSON.parse(undefined)", "JSON.parse()", "JSON.parse('{')", "JSON.parse('[')",
  "JSON.parse('[1,]')", "JSON.parse('{\"a\":1,}')", "JSON.parse(\"{'a':1}\")", "JSON.parse('{a:1}')", "JSON.parse('01')", "JSON.parse('+1')", "JSON.parse('.5')", "JSON.parse('1.')", "JSON.parse('0x1')", "JSON.parse('NaN')", "JSON.parse('Infinity')",
  "JSON.parse('undefined')", "JSON.parse('\"a\\nb\"')", "JSON.parse('\"a\\tb\"')", "JSON.parse('\"\\\\x41\"')", "JSON.parse('\"\\\\\\'\"')", "JSON.parse('\"abc')", "JSON.parse('1 2')", "JSON.parse('[1] x')", "JSON.parse('tru')", "JSON.parse('nul')",
  "JSON.parse('{\"a\":1,\"a\":2}')", "JSON.parse('{\"__proto__\":1}')", "Object.getPrototypeOf(JSON.parse('{\"__proto__\":null}'))===Object.prototype", "Object.keys(JSON.parse('{\"__proto__\":{\"x\":1}}'))", "JSON.parse('{\"b\":1,\"a\":2,\"1\":3}')",
  "JSON.parse('[1,2,3]',(k,v)=>typeof v==='number'?v*2:v)", "JSON.parse('{\"a\":1,\"b\":2}',(k,v)=>k==='a'?undefined:v)", "JSON.parse('[1,2]',(k,v)=>k==='0'?undefined:v)", "JSON.parse('{\"a\":[1]}',function(k,v){return Array.isArray(v)?v.length:v})",
  "(function(){var r=[];JSON.parse('{\"a\":{\"b\":1},\"c\":2}',(k,v)=>{r.push(k);return v});return r})()", "JSON.parse('1',5)", "JSON.parse('9007199254740993')", "JSON.parse('1e999')", "JSON.parse('-1e999')", "JSON.parse('0.1e-999')",
  "JSON.parse('\\u00a01')", "JSON.parse('\\ufeff1')", "JSON.parse('\\n1\\r\\t')", "JSON.parse('[\\u000b1]')", "JSON.parse('\"\\u2028\"').length", "JSON.parse('{\"a\":1}x')", "JSON.parse('{\"a\" 1}')", "JSON.parse('{\"a\":}')", "JSON.parse('{,}')", "JSON.parse('[,]')",
  "JSON.parse('\"\\u0001\"')", "JSON.parse('\"\\\\u00zz\"')", "JSON.parse('\"\\\\u12\"')", "JSON.parse('-')", "JSON.parse('--1')", "JSON.parse('1e')", "JSON.parse('1e+')", "JSON.parse('[[[[[[[[[[1]]]]]]]]]]')", "JSON.parse('{\"a\":{\"a\":{\"a\":{}}}}')",
  "JSON.parse(String(Symbol.iterator.description))", "JSON.parse({toString(){return '7'}})", "JSON.parse(Symbol())", "JSON.parse(1n)", "JSON[Symbol.toStringTag]", "JSON.stringify.length", "JSON.parse.length", "typeof JSON.rawJSON", "typeof JSON.isRawJSON",
  "JSON.stringify(JSON.rawJSON('12345678901234567890'))", "JSON.isRawJSON(JSON.rawJSON('1'))", "JSON.rawJSON('{}')", "JSON.rawJSON(' 1')", "JSON.rawJSON('')", "JSON.rawJSON(1)", "JSON.rawJSON('\"a\"')", "JSON.rawJSON('null')", "Object.isFrozen(JSON.rawJSON('1'))",
  "JSON.parse('[1,2]',function(k,v,c){return c&&c.source!==undefined?c.source:v})", "JSON.parse('1',(k,v,c)=>JSON.stringify(c))", "JSON.parse('{\"a\":1.0}',(k,v,c)=>k==='a'?c.source:v)",
);

// ---------------------------------------------------------------- globais
add(
  "encodeURI('a b')", "encodeURI('http://x.y/a b?c=d&e=f#g')", "encodeURI(';/?:@&=+$,#')", "encodeURI('-_.!~*\\'()')", "encodeURI('é')", "encodeURI('\\ud83d\\ude00')", "encodeURI('\\ud800')", "encodeURI('\\udc00')", "encodeURI('\\ud83d')", "encodeURI('\\ud83dx')",
  "encodeURI('%')", "encodeURI('[]')", "encodeURI('{}|^`')", "encodeURI('\\u0000')", "encodeURI('\\u007f')", "encodeURI('\\u0080')", "encodeURI('\\u07ff')", "encodeURI('\\u0800')", "encodeURI('\\uffff')", "encodeURI()", "encodeURI(null)", "encodeURI(Symbol())",
  "encodeURIComponent(';/?:@&=+$,#')", "encodeURIComponent('a b&c=d')", "encodeURIComponent('-_.!~*\\'()')", "encodeURIComponent('é€')", "encodeURIComponent('\\udc00')", "encodeURIComponent('\\ud800\\ud800')", "encodeURIComponent('')", "encodeURIComponent(undefined)", "encodeURIComponent({})",
  "decodeURI('%41')", "decodeURI('a%20b')", "decodeURI('%3B%2F%3F')", "decodeURI('%23')", "decodeURI('%25')", "decodeURI('%2f')", "decodeURI('%C3%A9')", "decodeURI('%c3%a9')", "decodeURI('%E2%82%AC')", "decodeURI('%F0%9F%98%80')", "decodeURI('%F0%9F%98')", "decodeURI('%')", "decodeURI('%4')",
  "decodeURI('%G0')", "decodeURI('%C3')", "decodeURI('%C3%28')", "decodeURI('%80')", "decodeURI('%C0%80')", "decodeURI('%E0%80%80')", "decodeURI('%ED%A0%80')", "decodeURI('%F4%90%80%80')", "decodeURI('%F8%80%80%80%80')", "decodeURI('%FF')", "decodeURI('%C3a')", "decodeURI('%E2%82')",
  "decodeURIComponent('%3B%2F%3F')", "decodeURIComponent('%23%25')", "decodeURIComponent('a+b')", "decodeURIComponent('%E4%BD%A0')", "decodeURIComponent('%')", "decodeURIComponent('%zz')", "decodeURIComponent('%E4%BD')", "decodeURIComponent('%F0%9F%98%80').length", "decodeURIComponent()",
  "decodeURIComponent(null)", "decodeURIComponent('%00').length", "decodeURIComponent('%7f')", "decodeURIComponent('%C2%80').charCodeAt(0)", "decodeURI('%C2%80').charCodeAt(0)", "encodeURI.length", "decodeURIComponent.name", "encodeURIComponent.length",
  "escape('a b')", "escape('äö')", "escape('\\u0100')", "escape('\\ud83d\\ude00')", "escape('@*_+-./')", "escape('!#$%&()')", "escape('')", "escape(undefined)", "escape(null)", "escape(1)", "escape('\\u0000')", "escape('\\uffff')", "escape('~')", "escape(Symbol())", "escape.length",
  "unescape('%41')", "unescape('%u0041')", "unescape('%u00e9')", "unescape('%E9')", "unescape('%')", "unescape('%4')", "unescape('%u004')", "unescape('%uzzzz')", "unescape('%zz')", "unescape('%u0041%41')", "unescape('a%20b')", "unescape('%ud83d%ude00').length", "unescape('%U0041')",
  "unescape('%u00411')", "unescape()", "unescape(undefined)", "unescape(null)", "unescape(Symbol())", "unescape.length", "unescape('%%41')", "unescape('%u%u0041')", "unescape('%0')", "unescape('%00').length",
  "typeof structuredClone", "structuredClone(1)", "structuredClone('a')", "structuredClone({a:[1,{b:2}]})", "structuredClone([1,,3])", "structuredClone(new Map([[1,{a:1}]]))", "structuredClone(new Set([1,2]))", "structuredClone(new Date(5))", "structuredClone(/x/gi)",
  "structuredClone(undefined)", "structuredClone(null)", "structuredClone(NaN)", "structuredClone(-0)", "structuredClone(1n)", "structuredClone(()=>1)", "structuredClone(Symbol())", "structuredClone({a:()=>1})", "structuredClone(new Error('m'))", "structuredClone(new WeakMap)",
  "structuredClone(new Number(5))", "structuredClone(new String('s'))", "structuredClone(new Uint8Array([1,2]))", "structuredClone(new ArrayBuffer(2)).byteLength", "structuredClone(Object.create({a:1}))", "structuredClone({get a(){return 1}})",
  "(function(){var a={};a.a=a;var b=structuredClone(a);return b.a===b})()", "(function(){var o={};var b=structuredClone([o,o]);return b[0]===b[1]})()", "structuredClone()", "structuredClone(1,2)", "structuredClone(new Proxy({},{}))", "structuredClone(Object(Symbol()))",
  "structuredClone(class A{})", "structuredClone({a:1n,b:undefined})", "structuredClone([1,2],{transfer:5})", "structuredClone(new Boolean(false))", "structuredClone(Object(1n))", "structuredClone({__proto__:null,a:1})", "structuredClone(new Error('m',{cause:1})).cause",
  "typeof globalThis", "globalThis===this", "typeof queueMicrotask", "typeof atob", "atob('YWJj')", "btoa('abc')", "atob('')", "atob('YQ')", "atob('YQ==')", "atob('Y')", "atob('YQ=')", "atob(' Y W J j ')", "atob('!!!!')", "btoa('é')", "btoa('\\u0100')", "btoa()", "atob()",
  "isNaN()", "isNaN('')", "isNaN(' ')", "isNaN([])", "isNaN({})", "isNaN(Symbol())", "isNaN(null)", "isNaN(undefined)", "isFinite(null)", "isFinite('Infinity')", "isFinite(undefined)", "isNaN.length", "isFinite.length", "parseInt.length", "parseFloat.length",
  "typeof globalThis.eval", "eval.length", "eval(5)", "eval('1+1')", "eval(Object('1+1'))", "(0,eval)('var __x=1;__x')", "Object.getOwnPropertyDescriptor(globalThis,'NaN')", "Object.getOwnPropertyDescriptor(globalThis,'undefined')", "Object.getOwnPropertyDescriptor(globalThis,'Infinity')",
  "NaN=1", "(function(){'use strict';NaN=1})()", "(function(){'use strict';undefined=1})()", "(function(){'use strict';Infinity=1})()", "void 0", "undefined", "typeof undeclared", "Object.getOwnPropertyDescriptor(globalThis,'parseInt').enumerable", "Object.getOwnPropertyDescriptor(globalThis,'globalThis').writable",
);

const rows = [];
const { usesHostApi } = require("./host-api.js");
// O porte é só o motor: nada de programa que dependa de API de host do bun (ver host-api.js).
for (const p of [...programs]) if (usesHostApi(p)) programs.delete(p);
for (const source of programs) {
  if (process.env.TRACE) process.stderr.write(source + "\n");
  if (/[\t\n\r]/.test(source)) throw new Error("fonte com tab ou quebra de linha: " + source);
  const result = harness(source);
  if (/[\n\r]/.test(result)) throw new Error("resultado com quebra de linha: " + source);
  if (result.includes("/home/") || result.includes("/Users/") || result.includes("node_modules")) throw new Error("caminho da máquina em: " + source);
  rows.push(source + "\t" + result);
}
process.stdout.write(require("./golden-prelude.js").assertPublicResult(rows.join("\n") + "\n"));
