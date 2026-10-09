// Gera tests/golden/setter_throw_bun.tsv: getter e setter que lançam (ou retornam normalmente) numa grade de posições
// de try/catch/finally, medido no bun. Cada programa roda num processo bun filho novo, sem APIs de host, e deixa o
// resultado em `globalThis.R` (o log de eventos unido por `|`). Cobre atribuição simples (nome, colchete, índice),
// destructuring (array e objeto, com iterador que tem ou não `return()`, e com `return()` que lança), compound
// assignment, `super.x`, Proxy/Reflect.set com receiver, acessores estáticos e privados de classe, e as posições
// try/catch/finally, for-of com break/return, generator, async, switch, parâmetro padrão e bloco estático.
// Colunas: a fonte do programa (JSON) e o valor de `R` (JSON). Só há efeitos síncronos no log.
// Uso: bun scripts/gen-setter-throw-golden.js > tests/golden/setter_throw_bun.tsv
const { emitFactoredLines } = require("./golden-prelude.js");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawn } = require("child_process");

const prelude = (tg, ts) =>
  `var log=[];function L(x){log.push(x)}var TG=${tg},TS=${ts};` +
  `var o={get x(){L('g');if(TG)throw new Error('G');return 1},set x(v){L('s'+v);if(TS)throw new Error('S')},` +
  `get 0(){L('g0');if(TG)throw new Error('G');return 1},set 0(v){L('s0:'+v);if(TS)throw new Error('S')}};` +
  `var k='x',i=0,src={a:1,b:2};` +
  `function mk(n,ret){var c=0;var it={next(){c++;return {value:c,done:c>n}}};` +
  `if(ret===1)it.return=function(){L('ret');return {}};if(ret===2)it.return=function(){L('ret');throw new Error('R')};` +
  `var r={};r[Symbol.iterator]=function(){return it};return r}` +
  `var so={__proto__:o,m1(){return super.x=5},m2(){return super.x+=1},m3(){return super.x++},m4(){return super[k]=5},` +
  `m5(){return [super.x]=mk(5,1)},m6(){return super.x},m7(){return super.x??=1}};` +
  `class C{static get x(){L('cg');if(TG)throw new Error('G');return 1}static set x(v){L('cs'+v);if(TS)throw new Error('S')}}` +
  `class P{get #p(){L('pg');if(TG)throw new Error('G');return 1}set #p(v){L('ps'+v);if(TS)throw new Error('S')}` +
  `m1(){return this.#p=5}m2(){return this.#p+=1}m3(){return [this.#p]=mk(5,1)}m4(){return this.#p++}m5(){return this.#p}` +
  `m6(){return this.#p??=1}m7(){return ({a:this.#p}=src)}}` +
  `var px=new Proxy(o,{});var pt=new Proxy(o,{set(t,key,v,r){L('trap');return Reflect.set(t,key,v,r)}});` +
  `var pg=new Proxy(o,{get(t,key,r){L('gtrap');return Reflect.get(t,key,r)}});`;

const ops = [
  // atribuição simples
  "o.x=5", "o['x']=5", "o[k]=5", "o[0]=5", "o[i]=5", "o.x=L('rhs')", "o.x=(L('a'),6)",
  // destructuring
  "[o.x]=mk(5,1)", "[o.x]=mk(5,0)", "[o.x]=mk(5,2)", "[o.x,o.x]=mk(5,1)", "[o.x,o.x]=mk(1,1)", "[o.x,o.x]=mk(2,0)",
  "({a:o.x}=src)", "({a:o.x,b:o.x}=src)", "[...o.x]=mk(2,1)", "[o[0]]=mk(5,1)", "[o.x=9]=mk(0,1)", "({a:o.x=9}={})",
  "[o.x,o.x]=mk(5,2)", "({...o.x}=src)", "[[o.x]]=[mk(5,1)]", "[o.x]=[1]", "[o.x,o[0]]=[1,2]", "[o.x,o.x]=mk(0,1)",
  // compound
  "o.x+=1", "o.x??=1", "o.x||=0", "o.x&&=2", "o.x++", "--o.x", "o[0]++", "o['x']+=1", "o[k]-=1", "o[i]??=3", "o.x**=2",
  // getters
  "o.x", "o[0]", "o[k]", "+o.x", "`${o.x}`", "({...o})", "Object.assign({},o)", "(({x})=>x)(o)", "o.x?.y",
  "Reflect.get(o,'x',{})", "Object.create(o).x", "JSON.stringify(o)", "Object.entries(o).length", "o.x.y.z",
  // super
  "so.m1()", "so.m2()", "so.m3()", "so.m4()", "so.m5()", "so.m6()", "so.m7()",
  // Proxy e Reflect
  "px.x=5", "px.x", "Reflect.set(o,'x',5)", "Reflect.set(o,'x',5,{})", "Reflect.set(o,'x',5,px)", "Reflect.set(px,'x',5)",
  "Reflect.set(pt,'x',5)", "pt.x=5", "pg.x", "Reflect.set(o,0,5,{})", "Object.create(o).x=5", "Object.assign(o,{x:1})",
  "Object.assign(Object.create(o),{x:1})", "Reflect.apply(Reflect.set,null,[o,'x',5])",
  // classe
  "C.x=5", "C.x+=1", "C.x", "C.x++", "[C.x]=mk(5,1)", "new P().m1()", "new P().m2()", "new P().m3()", "new P().m4()",
  "new P().m5()", "new P().m6()", "new P().m7()",
];

const wrappers = [
  "@",
  "try{@}catch(e){L('c:'+e.message)}",
  "try{L('pre');@;L('post')}catch(e){L('c:'+e.message)}",
  "try{@}catch(e){L('c:'+e.message)}finally{L('f')}",
  "try{L('t')}finally{@}",
  "try{throw new Error('T')}catch(e){@}finally{L('f')}",
  "try{try{L('t')}catch(e){}finally{@;L('fpost')}}catch(e){L('c:'+e.message)}",
  "for(var q of [1,2]){try{@;break}finally{L('f'+q)}}",
  "for(var q of mk(3,1)){try{@}catch(e){L('c:'+e.message);continue}break}",
  "L('r:'+(function(){for(var q of mk(3,1)){try{@;return 1}finally{L('f')}}})())",
  "L('r:'+(function(){try{@;return 1}finally{return 2}})())",
  "L('r:'+(function(){try{return 1}finally{@}})())",
  "L('r:'+(function(){try{@}finally{return 'ov'}})())",
  "L('r:'+(function(){a:try{@}finally{break a}return 'after'})())",
  "var g=(function*(){try{yield 1;@;yield 2}finally{L('gf')}})();L(g.next().value);try{L(g.next().value)}catch(e){L('c:'+e.message)}L(JSON.stringify(g.next()))",
  "var g=(function*(){try{yield 1}finally{@;L('gpost')}})();g.next();try{L(JSON.stringify(g.return(5)))}catch(e){L('c:'+e.message)}",
  "var g=(function*(){try{yield 1}catch(e){@}})();g.next();try{L(JSON.stringify(g.throw(new Error('T'))))}catch(e){L('c:'+e.message)}",
  "(async function(){try{@;await 0}catch(e){L('c:'+e.message)}})();L('sync')",
  "switch(1){case 1:try{@}catch(e){L('c:'+e.message)}case 2:L('fall')}",
  "L('r:'+(function(p=(@)){return 'body'})())",
  "do{try{@}finally{L('f')}}while(0)",
  "lbl:{try{@;break lbl}finally{L('f')}}",
  "try{try{@}finally{L('i')}}catch(e){L('c:'+e.message)}finally{L('o')}",
  "try{@}catch{L('c')}",
  "[1,2].forEach(function(v){try{@}catch(e){L('c:'+e.message)}})",
  "class Z{static{try{@}catch(e){L('c:'+e.message)}}}",
  "L('r:'+(function(){try{@;return 'try'}catch(e){return 'catch'}finally{L('f')}})())",
  "for(var q in {a:1,b:2}){try{@}catch(e){L('c:'+e.message)}}",
  "for(var q=0;q<2;q++){try{@;continue}finally{L('f'+q)}}",
  "L('r:'+(function(){try{throw new Error('T')}catch(e){return (@)}finally{L('f')}})())",
  "while(L('w')||1){try{@;break}catch(e){L('c:'+e.message);break}}",
  "(async function(){for(var q of mk(3,1)){try{@;break}finally{L('f')}}})();L('sync')",
];
const strictWrappers = new Set([1, 3, 7, 10, 14, 17, 22, 28]);
const behaviors = [[0, 0], [0, 1], [1, 0], [1, 1]];

const programs = [];
const seen = new Set();
for (const op of ops)
  for (const [wi, w] of wrappers.entries())
    for (const [tg, ts] of behaviors)
      for (const strict of strictWrappers.has(wi) && ts === 1 ? [false, true] : [false]) {
        const body = w.replace("@", () => op).replace(/@/g, () => op);
        const source =
          (strict ? '"use strict";' : "") + prelude(tg, ts) + `try{${body}}catch(e){L('U:'+e.message)}globalThis.R=log.join('|');`;
        if (/[\u2013\u2014]/.test(source)) throw new Error("travessão na fonte");
        if (seen.has(source)) continue;
        seen.add(source);
        // Amostra determinística de 1 em cada 4 da grade completa, para a geração caber em poucos minutos.
        if (seen.size % 4 === 0) programs.push(source);
      }

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "setter-throw-"));
const runner = path.join(tmp, "run.js");
fs.writeFileSync(
  runner,
  'const vm=require("node:vm");const fs=require("fs");globalThis.R=undefined;' +
    'try{vm.runInThisContext(fs.readFileSync(process.argv[2],"utf8"))}catch(e){process.stdout.write("\\u0000uncaught");process.exit(0)}' +
    'process.stdout.write(typeof R==="string"?R:"\\u0000undef");',
);
const runOne = (index) =>
  new Promise((resolve) => {
    const file = path.join(tmp, `p${index}.js`);
    fs.writeFileSync(file, programs[index]);
    const child = spawn(process.execPath, [runner, file], { cwd: tmp, stdio: ["ignore", "pipe", "ignore"] });
    let out = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 10000);
    child.stdout.on("data", (chunk) => (out += chunk));
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve(code === 0 ? out : "\u0000fail");
    });
  });

(async () => {
  const results = new Array(programs.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: 16 }, async () => {
      while (next < programs.length) {
        const index = next++;
        results[index] = await runOne(index);
      }
    }),
  );
  fs.rmSync(tmp, { recursive: true, force: true });
  const lines = [];
  let dropped = 0;
  programs.forEach((source, index) => {
    const result = results[index];
    if (result.startsWith("\u0000") || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result) || /[\u2013\u2014]/.test(result)) {
      dropped++;
      return;
    }
    lines.push(JSON.stringify(source) + "\t" + JSON.stringify(result));
  });
  process.stdout.write(emitFactoredLines("setter_throw", lines));
  process.stderr.write(`mantidos ${lines.length}, descartados ${dropped}\n`);
})();
