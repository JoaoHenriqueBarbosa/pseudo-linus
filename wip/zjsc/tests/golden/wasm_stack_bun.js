function leb(n){const o=[];do{let b=n&127;n>>=7;if(n)b|=128;o.push(b)}while(n);return o}
function str(s){return [...leb(s.length),...[...s].map(c=>c.charCodeAt(0))]}
function sec(id,b){return [id,...leb(b.length),...b]}
function mod(named,traps){
 const types=sec(1,[1,0x60,0,0]);
 const imports=sec(2,[1,...str("m"),...str("f"),0,0]);
 const funcs=sec(3,[2,0,0]);
 const exps=sec(7,[1,...str("run"),0,2]);
 // func1 (index1): call 0 ; func2 (index2): call 1 / unreachable
 const f1=[0,0x10,0,0x0b]; // call import
 const f2=traps?[0,0x00,0x0b]:[0,0x10,1,0x0b];
 // export run = func index 2
 const code=sec(10,[2,f1.length,...f1,f2.length,...f2]);
 let b=[0,0x61,0x73,0x6d,1,0,0,0,...types,...imports,...funcs,...exps,...code];
 if(named){const fn=[1,2,1,0,0]; // placeholder
  const names=[...str("name"),0,...leb(1+3),3,...[]];
 }
 return b;
}
function withNames(bytes,modname,fnames){
 const m=[...str(modname)];
 const fs=[fnames.length,...fnames.flatMap(([i,n])=>[i,...str(n)])];
 const sub=[0,...leb(m.length),...m,1,...leb(fs.length),...fs];
 const payload=[...str("name"),...sub];
 return [...bytes,...sec(0,payload)];
}
for (const [label,named,traps] of [["noname-js",false,false],["named-js",true,false],["noname-trap",false,true],["named-trap",true,true]]){
 let b=mod(false,traps); if(named)b=withNames(b,"mymod",[[1,"inner"],[2,"outer"]]);
 try{
  const inst=new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array(b)),{m:{f(){throw new Error('x')}}});
  inst.exports.run(); if(!inst)throw 1;
 }catch(e){console.log("== "+label+" "+e.constructor.name+"\n"+e.stack)}
}
// JSPI: o erro lançado por um import JS depois de uma retomada (a pilha vem de uma microtarefa) e, sem
// suspensão, dentro da entrada direta de um `promising`.
function jspiModule(){
 const types=sec(1,[1,0x60,0,0]);
 const imports=sec(2,[2,...str("m"),...str("s"),0,0,...str("m"),...str("t"),0,0]);
 const funcs=sec(3,[3,0,0,0]);
 const exps=sec(7,[2,...str("resume"),0,3,...str("start"),0,4]);
 // func2: call s ; call t    func3: call func2    func4: call t
 const f2=[0,0x10,0,0x10,1,0x0b];
 const f3=[0,0x10,2,0x0b];
 const f4=[0,0x10,1,0x0b];
 const code=sec(10,[3,f2.length,...f2,f3.length,...f3,f4.length,...f4]);
 return [0,0x61,0x73,0x6d,1,0,0,0,...types,...imports,...funcs,...exps,...code];
}
(async()=>{
 const inst=new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array(jspiModule())),{m:{s:new WebAssembly.Suspending(async()=>{await null}),t(){throw new Error('y')}}});
 for (const name of ["resume","start"]){
  try{ await WebAssembly.promising(inst.exports[name])(); }
  catch(e){console.log("== jspi-"+name+" "+e.constructor.name+"\n"+e.stack)}
 }
})();
