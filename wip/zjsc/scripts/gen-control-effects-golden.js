// Gera tests/golden/control_effects_bun.tsv: grade de EFEITOS das instruções de controle, medida no bun.
// Cada programa grava num log (`L`, via `p(x)`) a ordem em que os efeitos acontecem e termina em `globalThis.R`. Não
// mede completion value (isso é o golden de completion_value). Cobre: switch (fallthrough, default em qualquer
// posição, ordem de avaliação dos cases, let/const/function/class em case, TDZ), break/continue com label em laços
// aninhados dentro de switch/try/finally/with, for com múltiplos let e closures (init, teste, update, corpo), for-in
// (ordem de chaves: inteiros, strings, herdadas, sombreadas, removidas e acrescentadas durante), for-of em array,
// Set e Map mutados, destructuring em cabeçalho de for-in/of dentro de função, generator, async e for await com
// break/continue/throw/return, do-while e while com continue, vírgula/ternário/curto-circuito/atribuição lógica,
// function em if sem chaves (Annex B), exceções em cada parte do cabeçalho do for e nos ganchos do iterador,
// try/catch/finally com return/break/continue/throw sobrescrevendo, catch sem binding e finally com yield.
// Cada programa roda num processo bun novo, sem API de host; o filho só espera as microtarefas esvaziarem.
// Colunas: a fonte do programa (JSON) e o valor da variável global `R` (JSON).
// Uso: bun scripts/gen-control-effects-golden.js > tests/golden/control_effects_bun.tsv
const fs = require("fs");
const { knownPrograms, emitFactored } = require("./golden-prelude.js");
const path = require("path");
const { spawn } = require("child_process");

if (process.argv[2] === "--child") {
  (0, eval)(fs.readFileSync(0, "utf8"));
  // O setTimeout fica fora do programa: só serve para as microtarefas esvaziarem antes de ler `R`.
  setTimeout(() => {
    process.stdout.write(typeof globalThis.R === "string" ? globalThis.R : "\u0000undefined");
    process.exit(0);
  }, 0);
  return;
}

const HEAD =
  'var L=[];function p(x){L.push(x===undefined?"u":x===null?"n":Object.is(x,-0)?"-0":typeof x==="symbol"?String(x):x);return x}\n' +
  'function q(x){L.push("=>"+typeof x+":"+(typeof x==="symbol"?"sym":String(x)))}\n';
const ERR = 'L.push("throw:"+(e&&e.name)+":"+(e&&e.message))';

const sync = body => HEAD + 'try{var v=(function(){' + body + '\n})();L.push("ret:"+String(v))}catch(e){' + ERR + '}\nglobalThis.R=L.join();';
const asyn = body =>
  HEAD +
  '(async function(){' + body + '\n})().then(function(v){L.push("ret:"+String(v))},function(e){L.push("rej:"+(e&&e.name)+":"+(e&&e.message))}).then(function(){globalThis.R=L.join()});';
const step = 'L.push("y:"+String(r.value)+":"+r.done);';
const DRIVERS = {
  all: 'var it=g(),r,k=0;do{r=it.next("s"+k);' + step + '}while(!r.done&&++k<12)',
  ret: 'var it=g(),r=it.next();' + step + 'r=it.return("R");' + step + 'r=it.next();' + step,
  ret2: 'var it=g(),r=it.next();' + step + 'r=it.next();' + step + 'r=it.return("R");' + step + 'r=it.return("R2");' + step + 'r=it.next();' + step,
  thr: 'var it=g(),r=it.next();' + step + 'try{r=it.throw("T");' + step + '}catch(e){L.push("th:"+e)}try{r=it.next();' + step + '}catch(e){L.push("th2:"+e)}',
  thr2: 'var it=g(),r=it.next();' + step + 'r=it.next();' + step + 'try{r=it.throw("T");' + step + '}catch(e){L.push("th:"+e)}try{r=it.throw("T2");' + step + '}catch(e){L.push("th2:"+e)}',
  retfirst: 'var it=g(),r=it.return("R0");' + step + 'r=it.next();' + step,
  thrfirst: 'var it=g();try{var r=it.throw("T0");' + step + '}catch(e){L.push("th:"+e)}var r2=it.next();L.push("n:"+r2.done)',
};
const gen = (body, driver) => HEAD + 'function* g(){' + body + '\n}\ntry{' + DRIVERS[driver] + '}catch(e){' + ERR + '}\nglobalThis.R=L.join();';
const genSends = (body, sends) =>
  HEAD + 'function* g(){' + body + '\n}\ntry{var it=g(),r,sends=' + JSON.stringify(sends) + ';for(var k=0;k<sends.length+3;k++){r=it.next(sends[k]);' + step + 'if(r.done)break}}catch(e){' + ERR + '}\nglobalThis.R=L.join();';

const progs = [];
const add = (...list) => progs.push(...list);

// ---- 1. switch: grade de discriminante x rótulos x posição do default x padrão de break.
{
  const labelSets = [[1, 2, 3], [3, 2, 1], [1, 1, 2], ["'1'", 1, 2], ["NaN", 0, "-0"], [2], ["'a'", "'b'", "'a'"]];
  const discs = ["1", "2", "3", "'1'", "NaN", "undefined", "0"];
  for (const labels of labelSets) for (const defPos of ["none", "first", "mid", "last"]) for (const brk of ["all", "none", "alt"]) {
    const items = labels.map((l, i) => ({ l, i }));
    if (defPos !== "none") items.splice(defPos === "first" ? 0 : defPos === "mid" ? Math.floor(items.length / 2) : items.length, 0, { def: true });
    const body = items.map((it, idx) => {
      const jump = brk === "all" || (brk === "alt" && idx % 2 === 0) ? "break;" : "";
      return it.def ? 'default:p("d");' + jump : 'case (p("e' + it.i + '"),' + it.l + '):p("b' + it.i + '");' + jump;
    }).join("");
    for (const d of discs) add(sync('switch(p("sd"),' + d + '){' + body + '}p("end")'));
  }
}

// ---- 2. switch com declarações no escopo, TDZ, labels e ordem de avaliação.
{
  const templates = [
    "switch(D){case 1:let a=p('a1');case 2:p(typeof a);break;case 3:a=1}",
    "switch(D){case 1:const c=1;break;case 2:try{c=2}catch(e){p(e.name)}p(c)}",
    "switch(D){case 1:function f(){return 1};case 2:p(typeof f);break;default:p(f&&f())}",
    "var fs=[];switch(D){case 1:let a=1;fs.push(()=>a);case 2:let b=2;fs.push(()=>b);break;case 3:fs.push(()=>typeof a)}p(fs.map(f=>{try{return f()}catch(e){return e.name}}).join())",
    "switch(D){case 1:class C{};p(typeof C);break;case 2:p(typeof C)}",
    "switch(D){default:let z=1;case 1:p(z)}",
    "let x=0;switch(D){case 1:{let x=1;p(x)}case 2:p(x);break}",
    "switch(D){case p(1):case p(2):case p(3):p('hit')}",
    "switch(p(D)){case p(1):p('a');case p(2):p('b');default:p('d')}",
    "switch(D){case (p('e'),1):var v=1;break;case 2:p(typeof v)}p(v)",
    "var x=1;switch(D){case x++:p('a');case x++:p('b');default:p('d')}p(x)",
    "switch(D){case 1:p('a');{break}case 2:p('b');if(1){break}case 3:p('c')}",
    "switch(D){case 1:try{break}finally{p('f')}case 2:try{p('t');return 1}finally{p('f2')}}",
    "L1:switch(D){case 1:for(;;){break L1}case 2:for(var i=0;i<3;i++){if(i==1)break;p(i)}p('after')}",
    "switch(D){case 1:switch(2){case 2:p('in');break;default:p('x')}p('mid');case 2:p('c2')}",
    "switch(D){case 1:with({a:1}){p(a);break}case 2:p('c2')}",
    "switch(D){case 1:eval('var q=1');p(typeof q)}",
    "switch(D){}p('empty')",
    "switch(D){default:}p('only-default')",
    "switch(D){case 1:}p('only-case')",
    "switch(D){case p(D):p('same')}",
    "switch(D){case 1:let a;break;case 2:a=3;p(a)}",
    "switch(D){case 1:var a=1;case 2:let a2=2;p(a2)}",
    "switch(D){case 1:p('x');default:p('d');case 2:p('2')}",
    "switch(D){case 1:{p('1');break}default:{p('d')}case 2:{p('2')}}",
    "switch(D){case 1:p('1');continue_label:break;case 2:p('2')}",
    "var i=0;for(;i<4;i++){switch(D){case 1:continue;case 2:break;default:p('d'+i)}p('after'+i)}",
    "var o={valueOf(){p('vo');return 1}};switch(o){case 1:p('num');break;default:p('def')}",
    "var o={valueOf(){p('vo');return 1}};switch(D){case o:p('eq');break;default:p('def')}",
    "switch(D){case 1:function f(){p('f1')}case 2:function f(){p('f2')}}f()",
  ];
  for (const t of templates) for (const d of ["1", "2", "3", "'a'"]) add(sync(t.split("D").join(d)));
  const genBodies = [
    "switch(yield 'd'){case (yield 'c1'):yield 'b1';case (yield 'c2'):yield 'b2';break;default:yield 'dd'}",
    "switch(1){case (yield 'c1'):yield 'b1';default:yield 'dd';case (yield 'c2'):yield 'b2'}",
    "switch(yield 'd'){default:yield 'dd';case 1:yield 'b1';break;case 2:yield 'b2'}",
    "L:switch(yield 'd'){case 1:for(var i=0;i<3;i++){yield i;if(i==1)break L}case 2:yield 'b2'}",
  ];
  const sendSets = [[0, 1, 1, 2], [0, 2, 2, 2], [0, 1, 2, 1], [0, 3, 1, 2], [0, 2, 1, 1], [0, 1, 2, 2]];
  for (const b of genBodies) for (const s of sendSets) add(genSends(b, s));
}

// ---- 3. break/continue com label em laços aninhados dentro de switch/try/finally/with.
{
  const outers = {
    for: b => "L:for(var i=0;i<3;i++){" + b + "}",
    forlet: b => "L:for(let i=0;i<3;i++){" + b + "}",
    while: b => "var i=-1;L:while(i<2){i++;" + b + "}",
    do: b => "var i=-1;L:do{i++;" + b + "}while(i<2)",
    forin: b => "L:for(var i in {0:1,1:1,2:1}){" + b + "}",
    forof: b => "L:for(var i of [0,1,2]){" + b + "}",
  };
  const mid = j => 'p("m"+i);if(i==1)' + j + ';p("n"+i);';
  const constructs = {
    blk: (m, n) => n + ":{" + m + "}",
    sw: (m, n) => n + ":switch(+i){case 0:case 1:case 2:" + m + "}",
    tryf: (m, n) => n + ":try{" + m + '}finally{p("f"+i)}',
    trycf: (m, n) => n + ':try{if(i==1)throw 1;p("t"+i)}catch(e){' + m + '}finally{p("f"+i)}',
    finj: (m, n) => n + ':try{p("t"+i)}finally{p("f"+i);' + m + "}",
    with: (m, n) => n + ":with({w:i}){" + m + "}",
    loop: (m, n) => n + ":for(var j=0;j<2;j++){" + m + "}",
    deep: (m, n) => n + ":with({}){try{switch(+i){default:" + m + '}}finally{p("f"+i)}}',
  };
  const jumps = ["break", "continue", "break L", "continue L", "break M"];
  for (const [on, o] of Object.entries(outers)) for (const [cn, c] of Object.entries(constructs)) {
    for (const j of [...jumps, ...(cn === "loop" ? ["continue M"] : [])]) add(sync(o('p("i"+i);' + c(mid(j), "M")) + 'p("end")'));
  }
  for (const on of ["for", "forof"]) for (const [xn, x] of Object.entries(constructs)) for (const [yn, y] of Object.entries(constructs)) {
    for (const j of jumps) {
      add(sync(outers[on]('p("i"+i);' + x(y(mid(j), "M"), "N")) + 'p("end")'));
    }
  }
}

// ---- 4. for com múltiplos let/var, closures e modificação no corpo.
{
  const inits = ["i=0,j=10", "i=(fs.push(()=>i),0),j=5", "i=0,f=()=>i,j=2", "i=0,j=()=>i", "i=0,j=(p('j'),3)"];
  const tests = ["i<3", "(fs.push(()=>i),i<3)", "(p('t'+i),i<3)"];
  const updates = ["i++", "(fs.push(()=>i),i++)", "i++,j--", "(p('u'+i),i++)"];
  const bodies = [
    "fs.push(()=>i+','+j)",
    "{fs.push(()=>i);i++}",
    "if(i==1)continue;fs.push(()=>i)",
    "if(i==1){i=5}fs.push(()=>i)",
    "fs.push(()=>{i+=10;return i});p('b'+i)",
  ];
  for (const kind of ["let", "var"]) for (const init of inits) for (const test of tests) for (const upd of updates) for (const b of bodies) {
    add(sync("var fs=[],n=0;for(" + kind + " " + init + ";" + test + ";" + upd + "){if(++n>20)break;" + b + '}L.push(fs.map(f=>{try{return typeof f()=="function"?"fn":f()}catch(e){return e.name}}).join("|"))'));
  }
  for (const b of bodies) for (const upd of updates) add(sync("var fs=[],n=0;for(const i=0;i<3;" + upd + "){if(++n>5)break;" + b + "}L.push(fs.length)"));
}

// ---- 5. for-in: ordem de chaves e mutação durante a iteração.
{
  const objs = [
    "{b:1,a:2,1:3,0:4}", "{'10':1,'9':2,x:3,'-1':4,'01':5}", "{[Symbol.iterator]:1,z:1,y:2}",
    "{2:1,1:1,a:1,'4294967294':1,'4294967295':1,'4294967296':1}",
    "Object.create({p:1,2:2,a:3},{o:{value:1,enumerable:true},1:{value:1,enumerable:true}})",
    "Object.create({a:1,b:2},{a:{value:1,enumerable:false}})", "[5,,7]", "Object.assign([1,2],{x:1})", "'ab'",
    "Object.assign(function(){},{q:1})", "new Number(1)", "new Proxy({a:1,b:2,1:3},{})", "Object.create(Object.create({deep:1,1:1}))",
  ];
  const muts = [
    "", "delete o[Object.keys(o).slice(-1)[0]]", "o.zz=1", "o[99]=1", "delete o[k]", "Object.keys(o).forEach(x=>delete o[x])",
    "var pr=Object.getPrototypeOf(o);if(pr)pr.added=1", "var pr=Object.getPrototypeOf(o);if(pr)for(var x in pr)delete pr[x]",
    "Object.setPrototypeOf(o,{q:1})", "delete o[k];o[k]=1",
  ];
  for (const o of objs) for (const m of muts) for (const form of ["var k in o", "let k in o", "var [k] in o", "k in o"]) {
    add(sync("var k;var o=" + o + ";var n=0;for(" + form + "){L.push(String(k));if(n++==0){" + m + "}if(n>30)break}"));
  }
}

// ---- 6. for-of em array, Set e Map mutados durante a iteração.
{
  const arrs = ["[1,2,3]", "[1,,3]", "[1]"];
  const muts = ["a.push(9)", "a.pop()", "a.shift()", "a.unshift(0)", "a.splice(0,1)", "a.length=0", "a.length=5", "a[5]=1", "a.reverse()", "a=[9]", "a.sort((x,y)=>y-x)", "a.splice(1,0,7,8)"];
  for (const arr of arrs) for (const src of ["a", "a.entries()", "a.keys()", "a.values()"]) for (const m of muts) for (const idx of [0, 1]) {
    add(sync("var a=" + arr + ";var n=0;for(var x of " + src + "){L.push(String(x));if(n==" + idx + "){" + m + "}n++;if(n>12)break}L.push('a='+a.join())"));
  }
  const setMuts = ["s.delete(2)", "s.delete(1)", "s.add(9)", "s.clear()", "s.delete(2);s.add(2)", "s.clear();s.add(7)"];
  const mapMuts = ["s.delete(2)", "s.delete(1)", "s.set(9,'z')", "s.clear()", "s.delete(2);s.set(2,'again')", "s.set(1,'changed')"];
  for (const [init, ms] of [["new Set([1,2,3])", setMuts], ["new Set()", setMuts], ["new Map([[1,'a'],[2,'b'],[3,'c']])", mapMuts]]) {
    for (const src of ["s", "s.values()", "s.entries()"]) for (const m of ms) for (const idx of [0, 1]) {
      add(sync("var s=" + init + ";var n=0;for(var x of " + src + "){L.push(String(x));if(n==" + idx + "){" + m + "}n++;if(n>12)break}L.push('size='+s.size)"));
    }
  }
}

// ---- 7. destructuring no cabeçalho de for-in/of dentro de função, generator, async e for await.
{
  const heads = [
    ["var [a,b]", "[a,b]"], ["let {a,b=p('def')}", "[a,b]"], ["const [a,...r]", "[a,r]"], ["[a,b]", "[a,b]"], ["{a,b}", "[a,b]"],
    ["var {x:[a]}", "[a]"], ["let [a=p('d1'),b=p('d2')]", "[a,b]"], ["o.k", "[o.k]"],
  ];
  const sources = [
    "[[1,2],[3,4]]", "[{a:1,b:2},{a:3}]", "[undefined,[5]]",
    '{[Symbol.iterator](){var i=0;return{next(){p("next"+i);return i<3?{value:[i++,i],done:false}:{done:true,value:p("dv")}},return(){p("return");return{}}}}}',
  ];
  const exits = { log: "", brk: ";break", thr: ";throw p('T')", ret: ";return p('R')" };
  for (const [h, logExpr] of heads) for (const src of sources) for (const [en, ex] of Object.entries(exits)) for (const ctx of ["sync", "gen", "async", "await"]) {
    const marker = ctx === "gen" ? "yield " : ctx === "async" || ctx === "await" ? "await " : "";
    const loop = "var a,b,r,o={};var X=" + src + ";for" + (ctx === "await" ? " await" : "") + "(" + h + " of X){" + marker + 'p("v:"+String(' + logExpr + "))" + ex + "}p('end')";
    if (ctx === "sync") add(sync(loop));
    else if (ctx === "gen") add(HEAD + "function* g(){" + loop + '}\ntry{L.push("sp:"+[...g()].join("|"))}catch(e){' + ERR + "}\nglobalThis.R=L.join();");
    else add(asyn(loop));
  }
  for (const [h, logExpr] of [["var [c]", "c"], ["let {length}", "length"], ["var [c,d=p('dd')]", "[c,d]"]]) for (const ex of Object.values(exits)) for (const ctx of ["sync", "gen", "async"]) {
    const marker = ctx === "gen" ? "yield " : ctx === "async" ? "await " : "";
    const loop = "var c,d;for(" + h + " in {ab:1,cd:2,3:1}){" + marker + 'p("k:"+String(' + logExpr + "))" + ex + "}p('end')";
    if (ctx === "sync") add(sync(loop));
    else if (ctx === "gen") add(gen(loop, "all"));
    else add(asyn(loop));
  }
}

// ---- 8. do-while e while com continue, labeled continue.
{
  const conds = ["i<3", "p('c'+i)&&i<3", "(p('c'+i),i<3)", "i++<2", "!p(i>=2)"];
  const bodies = [
    "p('a'+i);i++;if(i==2)continue;p('b'+i)", "i++;if(i%2)continue;p('even'+i)", "i++;try{if(i==1)continue}finally{p('f'+i)}p('t'+i)",
    "i++;try{continue}finally{p('f'+i)}", "i++;try{p('t')}finally{if(i==1)continue}p('after'+i)", "i++;switch(i){case 1:continue;default:p('d'+i)}p('after'+i)",
    "i++;L:{if(i==1)break L;p('x'+i)}p('y'+i)", "i++;with({}){if(i==1)continue;p('w'+i)}", "i++;for(var j=0;j<2;j++){if(j)continue;p('j'+i+j)}",
    "i++;M:for(var j=0;j<2;j++){if(j==0)continue M;p('j'+i+j);break}p('z'+i)",
  ];
  for (const c of conds) for (const b of bodies) {
    add(sync("var i=0;do{" + b + "}while(" + c + ");p('end'+i)"), sync("var i=0;while(" + c + "){" + b + "}p('end'+i)"));
  }
  const nested = [
    "var i=0;L:while(i<3){i++;var j=0;do{j++;if(j==2)continue L;p('in'+i+j)}while(j<3);p('never')}",
    "var i=0;L:do{i++;var j=0;while(j<3){j++;if(j==2)continue L;p('in'+i+j)}p('never')}while(i<3)",
    "var i=0;L:while(i<3){i++;for(var j=0;j<3;j++){if(j==1)continue L;p('in'+i+j)}p('never')}",
    "var i=0;L:while(i<3){i++;for(var j in {a:1,b:1}){try{continue L}finally{p('f'+i+j)}}p('never')}",
    "var i=0;L:while(i<3){i++;switch(i){case 2:continue L;default:p('d'+i)}p('after'+i)}",
    "var i=0;L:do{i++;try{p('t'+i);if(i<3)continue L}finally{p('f'+i)}p('after'+i)}while(i<3)",
    "var i=0;L:while(i<4){i++;M:while(true){if(i%2)continue L;break M}p('even'+i)}",
    "var i=0;L:while(i<3){i++;with({}){M:do{if(i==2)continue L;p('w'+i)}while(false)}p('after'+i)}",
    "var i=0;L:while(i<3){i++;try{throw i}catch(e){if(e==2)continue L;p('c'+e)}finally{p('f'+i)}p('after'+i)}",
    "var i=0;L:while(i<3){i++;try{p('t'+i)}finally{if(i==2)continue L}p('after'+i)}",
    "var i=0;L:do{i++;try{try{continue L}finally{p('f1'+i)}}finally{p('f2'+i)}}while(i<2);p('end')",
    "var i=0;L:while(i<2){i++;try{continue L}finally{try{p('f'+i);break L}finally{p('g'+i)}}}p('end'+i)",
    "var i=0;L:while(i<3){i++;try{continue L}finally{p('f'+i);continue}}p('end'+i)",
    "var c=0;L:for(var i=0;i<3;i++){for(var j=0;j<3;j++){if(j==1)continue L;c++}}p(c)",
    "var c=0;L:for(var i=0;i<3;i++){M:for(var j=0;j<3;j++){if(j==1)continue M;if(i==1)continue L;c++}p('o'+i)}p(c)",
  ];
  for (const n of nested) add(sync(n));
}

// ---- 9. vírgula, ternário, curto-circuito e atribuição lógica com efeitos.
{
  const vals = ["0", "1", "null", "undefined"];
  const forms = [
    "a&&b||c", "a||b&&c", "(a||b)&&c", "a&&(b||c)", "a??b??c", "(a??b)&&c", "a??(b||c)", "a?b:c", "a?b?c:'x':'y'", "(a,b,c)", "a&&b&&c", "a||b||c",
  ];
  for (const f of forms) for (const A of vals) for (const B of vals) for (const C of vals) {
    // Os operandos só são avaliados na ordem em que a forma os alcança: cada um loga ao ser avaliado.
    let k = 0;
    const src = f.replace(/\b[abc]\b/g, name => 'p(' + { a: A, b: B, c: C }[name] + ')' + (k++, ""));
    add(sync("q(" + src + ")"));
  }
  const targets = {
    var: ["var x=INIT;", "x"],
    prop: ["var o={x:INIT};", "o.x"],
    accessor: ["var s=INIT;var o={get x(){p('get');return s},set x(v){p('set'+v);s=v}};", "o.x"],
    keyed: ["var o={x:INIT};function k(){p('key');return 'x'}", "o[k()]"],
    frozen: ["var o=Object.freeze({x:INIT});", "o.x"],
    constant: ["const x=INIT;", "x"],
  };
  for (const op of ["&&=", "||=", "??="]) for (const init of ["0", "1", "null", "undefined", "'s'"]) for (const [, [decl, target]] of Object.entries(targets)) {
    add(sync(decl.replace("INIT", init) + "try{q(" + target + op + "p('rhs'))}catch(e){p(e.name)}p(String(" + target + "))"));
  }
  for (const op of ["&&=", "||=", "??="]) for (const init of ["0", "null", "undefined"]) {
    add(sync("var x=" + init + ";x" + op + "function(){};p(typeof x==='function'?x.name:String(x))"));
    add(sync("var o={x:" + init + "};o.x" + op + "()=>1;p(typeof o.x==='function'?'['+o.x.name+']':String(o.x))"));
  }
  const bases = ["null", "undefined", "{}", "{b:null}", "{b:{c(){p('call');return 1}}}"];
  const chains = [
    "a?.[p('k')]", "a?.b(p('arg'))", "a?.b.c(p('arg'))", "a?.b?.c(p('arg'))", "delete a?.b", "a?.b.c.d", "(a?.b).c", "a?.[p('k')]?.[p('k2')]", "a?.b?.c?.()", "a?.b.c?.(p('arg'))",
  ];
  for (const a of bases) for (const ch of chains) add(sync("var a=" + a + ";q(" + ch + ")"));
  const loopForms = [
    "var i=0;while(p('c'+i)&&i<2||p('alt')&&0){i++}",
    "var i=0;for(;p('t'+i),i<2;i++,p('u'+i));",
    "var i=0;for(p('init'),i=1;i<3?p('yes'):p('no');p('up'),i++){p('body'+i)}",
    "if(p(0)||p(null)??p('z')){p('then')}else{p('else')}",
    "var r=p(1)?p(0)||p(2):p(3);q(r)",
    "var r=(p(1),p(0)?p('x'):(p('y'),p('z')));q(r)",
    "var x=(p('a'),p('b'),p('c'));q(x)",
    "var a=[p(1),p(2)];var o={[p('k1')]:p('v1'),[p('k2')]:p('v2')};q(Object.keys(o).join())",
  ];
  for (const l of loopForms) add(sync(l));
}

// ---- 10. function em if sem chaves e em bloco (Annex B).
{
  const forms = [
    "p(typeof f);if(C){function f(){return 1}}p(typeof f)", "if(C)function f(){return 1}p(typeof f)",
    "if(C)function f(){return 1}else function f(){return 2}p(f())", "p(typeof f);{function f(){return 1}}p(typeof f)",
    "{function f(){return 1}f=2;p(typeof f)}p(typeof f)", "{f=2;function f(){}}p(typeof f)", "{function f(){return 1}}{function f(){return 2}}p(f())",
    "let r=typeof f;{function f(){}}p(r+typeof f)", "switch(C){case true:default:function f(){}}p(typeof f)", "L:function f(){}p(typeof f)",
    "if(C){function f(){return 1}}else{function f(){return 2}}p(f())", "for(var i=0;i<2;i++){function f(){return i}}p(f())",
    "try{throw 1}catch(f){{function f(){}}p(typeof f)}p(typeof f)", "function g(f){{function f(){}}return typeof f}p(g(1))",
    "let f=1;{function f(){}}p(typeof f)", "{let f=1;{function f(){}}}p(typeof f)", "var f=7;{function f(){}}p(typeof f)",
    "{function f(){p('first')}function f(){p('second')}}f()", "if(C){function f(){}}f()", "(function(){p(typeof f);if(C){function f(){}}p(typeof f)})()",
    "p(eval('typeof f'));{function f(){}}", "eval('if(C)function f(){}');p(typeof f)", "var g=()=>typeof f;if(C){function f(){}}p(g())",
    "if(C){p(typeof f);function f(){}}p(typeof f)", "if(C){f=1;function f(){}}p(typeof f)", "while(C){function f(){}break}p(typeof f)",
    "do{function f(){}}while(0);p(typeof f)", "with({f:5}){{function f(){}}p(typeof f)}p(typeof f)", "function f(){return 'outer'}{function f(){return 'inner'}}p(f())",
    "{function* f(){}}p(typeof f)", "{async function f(){}}p(typeof f)", "{class f{}}p(typeof f)", "(function(){'use strict';{function f(){}}p(typeof f)})()",
    "(function(){'use strict';if(C){p(typeof f)}p(typeof f);{function f(){}}})()", "if(C)function f(){return 1}p(Object.getOwnPropertyNames(function(){}).length);p(f.name)",
    "{function f(){}}p(f===f);p(typeof f)", "if(C)function f(){}else;p(typeof f)", "if(!C);else function f(){return 5}p(typeof f)",
    "var f=1;if(C)function f(){}p(typeof f)", "p(typeof f);if(C)function f(){}p(typeof f)",
  ];
  for (const f of forms) for (const c of ["1", "0", "p(1)", "p(0)"]) add(sync(f.split("C").join(c)));
  const pre = ["var f=7;", "let f=7;", "", "function f(){return 'decl'}"];
  const wraps = ["{function f(){return 'blk'}}", "if(1)function f(){return 'blk'}", "if(0)function f(){return 'blk'}", "switch(1){case 1:function f(){return 'blk'}}", "try{function f(){return 'blk'}}catch(e){}"];
  for (const a of pre) for (const w of wraps) for (const tail of ["p(typeof f)", "p(typeof f==='function'?f():f)"]) add(sync(a + "p(typeof f);" + w + tail));
}

// ---- 11. exceções em cada parte do cabeçalho do for e nos ganchos do iterador.
{
  const T = "function T(){p('T');throw new RangeError('x')}";
  const heads = [
    "for(DECL i=T();i<2;i++){p('b'+i)}", "for(DECL i=0;i<3&&(i<1||T());i++){p('b'+i)}",
    "for(DECL i=0;i<3;(p('u'+i),i++>0&&T())){p('b'+i)}", "for(DECL i=0;i<3;i++){p('b'+i);if(i==1)T()}",
    "for(DECL i=0,j=T();i<2;i++){p('b'+i)}", "for(DECL i=(p('i1'),0),j=(p('i2'),T());i<2;i++){}",
  ];
  const wraps = [b => b, b => "try{" + b + "}catch(e){p('c:'+e.name)}finally{p('fin')}", b => "L:for(var z=0;z<2;z++){try{" + b + "}catch(e){continue L}p('after'+z)}"];
  for (const h of heads) for (const kind of ["var", "let"]) for (const w of wraps) add(sync(T + "\n" + w(h.split("DECL").join(kind))));
  const hooks = {
    ok: "next(){p('next'+i);return i<3?{value:i++,done:false}:{value:'end',done:true}},return(){p('return');return{}}",
    nextThrow: "next(){p('next'+i);if(i==1)throw new RangeError('nx');return{value:i++,done:false}},return(){p('return');return{}}",
    valueThrow: "next(){p('next'+i);var j=i++;return{get value(){if(j==1)throw new RangeError('vg');return j},done:false}},return(){p('return');return{}}",
    doneThrow: "next(){p('next'+i);var j=i++;return{value:j,get done(){if(j==1)throw new RangeError('dg');return false}}},return(){p('return');return{}}",
    retThrow: "next(){p('next'+i);return{value:i++,done:false}},return(){p('return');throw new EvalError('rt')}",
    retNonObj: "next(){p('next'+i);return{value:i++,done:false}},return(){p('return');return 1}",
    noReturn: "next(){p('next'+i);return i<3?{value:i++,done:false}:{done:true}}",
    retGetterThrow: "next(){p('next'+i);return{value:i++,done:false}},get return(){p('getret');throw new EvalError('rg')}",
    retNull: "next(){p('next'+i);return{value:i++,done:false}},return:null",
    retNotFn: "next(){p('next'+i);return{value:i++,done:false}},return:5",
    nextNonObj: "next(){p('next'+i);return 1},return(){p('return');return{}}",
    nextNotFn: "next:1,return(){p('return');return{}}",
  };
  const exitsFor = { exhaust: "", brk: "if(x==1)break;", thr: "if(x==1)throw new TypeError('body');", ret: "if(x==1)return 'R';", contO: "if(x==1)continue O;", brkO: "if(x==1)break O;" };
  for (const [hn, h] of Object.entries(hooks)) for (const [en, ex] of Object.entries(exitsFor)) {
    const iterable = "var X={[Symbol.iterator](){var i=0;p('iter');return{" + h + "}}};";
    add(sync(iterable + "O:for(var o=0;o<1;o++){for(var x of X){p('x'+x);" + ex + "}p('after')}p('end')"));
    add(asyn(iterable + "O:for(var o=0;o<1;o++){for await(var x of X){p('x'+x);" + ex + "}p('after')}p('end')"));
  }
  for (const si of ["throw new RangeError('si')", "return 1", "return {}", "return {next:1}", "return {next(){return{done:true}}}"]) {
    add(sync("var X={[Symbol.iterator](){p('iter');" + si + "}};for(var x of X){p('x')}p('end')"));
    add(asyn("var X={[Symbol.iterator](){p('iter');" + si + "}};for await(var x of X){p('x')}p('end')"));
    add(sync("var X={[Symbol.iterator](){p('iter');" + si + "}};var [a,b]=X;p('end')"));
  }
  const forin = [
    "for(var k in T()){p(k)}", "var o={};for(o[(p('key'),'k')] in {a:1,b:1}){p(o.k)}", "for(var [x=T()] in {a:1}){p(x)}",
    "var o={set k(v){p('set'+v);throw 1}};for(o.k in {a:1,b:1}){p('body')}", "for(var k in {get a(){p('get');return 1},b:2}){p(k)}",
    "for(var k in {a:1,b:2}){p(k);if(k=='a')T()}", "for(let k in (p('expr'),{a:1})){p(k)}", "for(let k in {a:1,b:2}){p(k);if(k=='a')break;p('no')}",
    "for(var k in null){p('never')}p('null-ok')", "for(var k in undefined){p('never')}p('undef-ok')", "for(var k in 5){p('never')}p('num-ok')",
    "for(let [a,b] in {xy:1}){p(a+b)}", "for(let {length} in {xyz:1}){p(length)}", "for(var k in {a:1}){var k2=k;p(k2)}p(typeof k)",
    "for(var i=0 in {}){p('x')}", "for(let k in {a:1}){try{throw k}catch(e){p('c'+e)}finally{p('f')}}",
  ];
  for (const f of forin) add(sync(T + "\n" + f));
}

// ---- 12. try/catch/finally: return/break/continue/throw sobrescrevendo.
{
  const exit = (kind, tag, brk, cont) => ({ n: "p('x" + tag + "')", r: "return p('r" + tag + "')", t: "throw p('E" + tag + "')", b: brk, c: cont })[kind];
  const kinds = ["n", "r", "t", "b", "c"];
  const ctxs = {
    for: [(t) => "for(var i=0;i<2;i++){p('s'+i);" + t + ";p('after'+i)}p('end')", "break", "continue"],
    do: [(t) => "var i=0;do{i++;p('s'+i);" + t + ";p('after'+i)}while(i<2);p('end')", "break", "continue"],
    of: [(t) => "for(var i of [0,1]){p('s'+i);" + t + ";p('after'+i)}p('end')", "break", "continue"],
    nested: [(t) => "O:for(var i=0;i<2;i++){for(var j=0;j<2;j++){p('s'+i+j);" + t + ";p('after'+i+j)}p('mid'+i)}p('end')", "break O", "continue O"],
  };
  for (const [, [wrap, brk, cont]] of Object.entries(ctxs)) {
    const E = (k, tag) => exit(k, tag, brk, cont);
    for (const T of kinds) for (const F of kinds) {
      add(sync(wrap("try{p('t');" + E(T, "T") + "}finally{p('f');" + E(F, "F") + "}")));
      for (const C of kinds) {
        add(sync(wrap("try{p('t');" + E(T, "T") + "}catch(e){p('c:'+e);" + E(C, "C") + "}finally{p('f');" + E(F, "F") + "}")));
        add(sync(wrap("try{p('t');" + E(T, "T") + "}catch{p('c');" + E(C, "C") + "}finally{p('f');" + E(F, "F") + "}")));
      }
    }
    for (const T of kinds) for (const C of kinds) {
      add(sync(wrap("try{p('t');" + E(T, "T") + "}catch(e){p('c:'+e);" + E(C, "C") + "}")));
      add(sync(wrap("try{p('t');" + E(T, "T") + "}catch({length}){p('c'+length);" + E(C, "C") + "}")));
    }
  }
  {
    const [wrap, brk, cont] = ctxs.for;
    const E = (k, tag) => exit(k, tag, brk, cont);
    for (const T of kinds) for (const T2 of kinds) for (const F2 of kinds) {
      add(sync(wrap("try{p('t');" + E(T, "T") + "}finally{try{p('t2');" + E(T2, "U") + "}finally{p('f2');" + E(F2, "V") + "}}")));
    }
    for (const T of kinds) for (const F of kinds) {
      add(sync(wrap("try{try{p('t');" + E(T, "T") + "}finally{p('f');" + E(F, "F") + "}}catch(e){p('outer:'+e)}finally{p('of')}")));
    }
  }
  // Valor de retorno sobrescrito e ordem de avaliação do operando do return antes do finally.
  add(
    sync("function f(){try{return p('a')}finally{p('f')}}q(f())"),
    sync("function f(){var x=1;try{return x}finally{x=2}}q(f())"),
    sync("function f(){var o={v:1};try{return o}finally{o.v=2}}q(f().v)"),
    sync("function f(){try{return p('a')}finally{return p('b')}}q(f())"),
    sync("function f(){try{throw p('a')}finally{return p('b')}}q(f())"),
    sync("function f(){try{return p('a')}finally{throw p('b')}}q(f())"),
    sync("function f(){L:try{return p('a')}finally{break L}return p('after')}q(f())"),
    sync("function f(){for(;;){try{return p('a')}finally{break}}return p('after')}q(f())"),
    sync("function f(){for(var i=0;i<3;i++){try{return p('a'+i)}finally{if(i<2)continue}}return p('after')}q(f())"),
    sync("function f(){try{try{throw p('in')}finally{p('f1')}}catch(e){return p('c'+e)}finally{p('f2')}}q(f())"),
    sync("function f(){try{throw p('a')}catch(e){throw p('b')}finally{p('f')}}q(f())"),
    sync("function f(){try{throw p('a')}catch(e){try{throw p('b')}catch(e2){p('c'+e+e2)}finally{p('fi')}}finally{p('fo')}}q(f())"),
    sync("function f(){try{throw 1}catch(e){var e=2;p(e)}p(e)}q(f())"),
    sync("function f(){try{throw 1}catch(e){{var e=2}p(e)}p(e)}q(f())"),
    sync("function f(){try{throw 1}catch(e){e=5;p(e)}p(typeof e)}q(f())"),
    sync("var e='outer';try{throw 'inner'}catch(e){p(e)}p(e)"),
    sync("try{throw {a:1,b:[2]}}catch({a,b:[c]}){p(a+c)}"),
    sync("try{throw null}catch({a}){p('no')}finally{p('f')}"),
    sync("try{throw undefined}catch{p('nobind')}"),
    sync("try{throw 1}catch(e){let e2=e;p(e2)}p(typeof e2)"),
    sync("var fs=[];for(var i=0;i<2;i++){try{throw i}catch(e){fs.push(()=>e)}}q(fs.map(f=>f()).join())"),
    sync("function f(){try{return}finally{p('f')}}q(f())"),
    sync("function* g(){try{yield 1}finally{p('cleanup')}}for(var x of g()){p(x);break}p('end')"),
    sync("function* g(){try{yield 1;yield 2}finally{p('cleanup')}}var [a]=g();p(a)"),
    sync("function* g(){try{yield 1}finally{return 'fr'}}var it=g();it.next();var r=it.return('x');p(r.value+':'+r.done)"),
  );
}

// ---- 13. generator com finally e yield, e o equivalente com await.
{
  const bodies = [
    "try{yield 1;yield 2}finally{yield 'f';p('after-f')}", "try{yield 1}finally{yield 'f1';yield 'f2'}", "try{try{yield 1}finally{yield 'i'}}finally{yield 'o'}",
    "try{yield 1}catch(e){yield 'c'+e}finally{yield 'f'}", "try{throw 'x'}catch{yield 'c'}finally{yield 'f'}", "try{return 5}finally{yield 'f'}",
    "try{yield 1}finally{return 'fr'}", "try{yield 1}finally{throw 'ft'}", "for(var i=0;i<3;i++){try{yield i;continue}finally{yield 'f'+i}}",
    "L:for(var i=0;i<3;i++){try{yield i;break L}finally{yield 'f'+i}}", "try{yield 1}finally{try{yield 'a'}finally{yield 'b'}}",
    "for(var x of [1,2]){try{yield x}finally{yield 'f'+x}}", "try{yield 1}finally{yield 'f';return 'x'}", "try{yield* [1,2]}finally{yield 'f'}",
    "yield 0;try{yield 1}catch(e){yield 'c'+e;yield 'c2'}finally{yield 'f';p('end')}", "try{yield 1}catch{yield 'c'}",
    "try{yield 1}finally{p('f')}yield 'after'", "try{var x=yield 1;p('got'+x)}finally{var y=yield 'f';p('fgot'+y)}",
  ];
  for (const b of bodies) for (const d of Object.keys(DRIVERS)) add(gen(b, d));
  for (const b of bodies.filter(x => !x.includes("yield*"))) add(asyn(b.split("yield ").join("await ")));
  for (const b of bodies.filter(x => !x.includes("yield*") && !x.includes("var x=yield"))) {
    add(asyn("try{" + b.split("yield ").join("await ") + "}catch(e){p('caught:'+e)}finally{p('outer-f')}"));
  }
}

// ---- Execução: dedup por fonte, processo novo por programa, ordem estável.
const existingSources = new Set();
for (const program of knownPrograms("control_effects_bun.tsv", (entry) => !(!entry.endsWith(".tsv") || entry === "control_effects_bun.tsv"))) existingSources.add(JSON.stringify(program));
const seen = new Set();
const unique = [];
let dup = 0;
for (const source of progs) {
  if (seen.has(source) || existingSources.has(JSON.stringify(source))) { dup++; continue; }
  seen.add(source);
  unique.push(source);
}

function runOne(source) {
  return new Promise(resolve => {
    const child = spawn(process.execPath, [__filename, "--child"], { stdio: ["pipe", "pipe", "ignore"] });
    let out = "";
    const timer = setTimeout(() => child.kill("SIGKILL"), 10000);
    child.stdout.on("data", chunk => (out += chunk));
    child.on("close", code => { clearTimeout(timer); resolve(code === 0 ? out : null); });
    child.stdin.on("error", () => {});
    child.stdin.end(source);
  });
}

(async () => {
  const results = new Array(unique.length);
  let next = 0;
  const workers = Array.from({ length: 12 }, async () => {
    while (next < unique.length) {
      const index = next++;
      results[index] = await runOne(unique[index]);
    }
  });
  await Promise.all(workers);
  let kept = 0;
  let dropped = 0;
  const rows = [];
  for (let index = 0; index < unique.length; index++) {
    const result = results[index];
    if (result === null || result.startsWith("\u0000") || /\/home\/|\/tmp\/|\/Users\/|\.js:\d|bun/i.test(result)) {
      dropped++;
      process.stderr.write("descartado: " + JSON.stringify(unique[index]).slice(0, 200) + " => " + JSON.stringify(result === null ? null : result.slice(0, 80)) + "\n");
      continue;
    }
    kept++;
    rows.push({ source: unique[index], result });
  }
  process.stdout.write(emitFactored("control_effects", rows));
  process.stderr.write("mantidos " + kept + ", descartados " + dropped + ", repetidos " + dup + "\n");
})();
