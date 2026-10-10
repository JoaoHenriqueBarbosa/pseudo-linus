// `require("async_hooks")` contra o bun 1.4.2. Saída de referência: `bun async_hooks_main.js`.
const ah = require('async_hooks');
const d = (o) => Object.getOwnPropertyNames(o).map((k) => { const x = Object.getOwnPropertyDescriptor(o, k); return k + ':' + (x.get ? 'get' : typeof x.value) + (x.enumerable ? '' : '!e'); }).join(',');
console.log(Object.keys(ah).join());
console.log(d(ah));
console.log(Object.prototype.toString.call(ah));
console.log(d(ah.AsyncLocalStorage));
console.log(d(ah.AsyncLocalStorage.prototype));
console.log(d(ah.AsyncResource));
console.log(d(ah.AsyncResource.prototype));
console.log(ah.AsyncLocalStorage.length, ah.AsyncResource.length, ah.createHook.length, ah.executionAsyncId.length, ah.triggerAsyncId.length, ah.executionAsyncResource.length);
console.log(ah.executionAsyncId(), ah.triggerAsyncId(), typeof ah.executionAsyncResource());
console.log(Object.keys(ah.asyncWrapProviders).length, ah.asyncWrapProviders.INSPECTORJSBINDING, Object.isFrozen(ah.asyncWrapProviders));
console.log(ah.executionAsyncResource() === process.stdin);
console.log(Object.getPrototypeOf(ah) === Object.prototype, Object.isExtensible(ah), require('node:async_hooks') === ah);
{
  const als = new ah.AsyncLocalStorage();
  console.log(als.getStore(), als.run(5, () => als.getStore()), als.getStore());
  console.log(Object.getOwnPropertyNames(als).join(), JSON.stringify(als.name));
  als.run({ a: 1 }, () => {
    setTimeout(() => console.log('timeout', JSON.stringify(als.getStore())), 1);
    setImmediate(() => console.log('immediate', JSON.stringify(als.getStore())));
    Promise.resolve().then(() => console.log('then', JSON.stringify(als.getStore())));
    process.nextTick(() => console.log('tick', JSON.stringify(als.getStore())));
    queueMicrotask(() => console.log('qm', JSON.stringify(als.getStore())));
  });
  als.enterWith(7); console.log(als.getStore()); als.exit(() => console.log('exit', als.getStore())); als.disable(); console.log(als.getStore());
  const r = new ah.AsyncResource('X');
  console.log(Object.getOwnPropertyNames(r).join(), r.asyncId(), r.triggerAsyncId(), typeof r.runInAsyncScope, r.emitDestroy() === r);
  console.log(r.runInAsyncScope((a, b) => a + b, null, 1, 2));
  const h = ah.createHook({ init() {} });
  console.log(Object.getOwnPropertyNames(h).join(), h.enable() === h, h.disable() === h);
  try { new ah.AsyncResource(); } catch (e) { console.log(e.code, e.message); }
  try { ah.createHook(); } catch (e) { console.log(e.code, e.message); }
  console.log(typeof ah.AsyncLocalStorage.bind(() => 1), typeof ah.AsyncLocalStorage.snapshot(), typeof ah.AsyncResource.bind(() => 1));
  console.log(typeof new ah.AsyncResource('Y').bind((x) => x));
}
{
  const show = (f) => { try { f(); console.log('ok'); } catch (e) { console.log(e.name, e.code, JSON.stringify(e.message)); } };
  show(() => new ah.AsyncResource('X', { triggerAsyncId: -2 }));
  show(() => new ah.AsyncResource('X', 1.5));
  show(() => new ah.AsyncResource('X', 'a'));
  show(() => ah.createHook({ init: 1 }));
  show(() => ah.createHook({ before: 'x' }));
  show(() => ah.createHook({ promiseResolve: 2 }));
  show(() => new ah.AsyncLocalStorage(1));
  show(() => new ah.AsyncLocalStorage(null));
  show(() => ah.AsyncLocalStorage.bind(1));
  show(() => new ah.AsyncResource('Y').bind(1));
  const h = ah.createHook({ init() {} }); h.enable();
  show(() => new ah.AsyncResource(''));
  h.disable();
  show(() => new ah.AsyncResource(''));
  console.log(new ah.AsyncResource('Z', 7).triggerAsyncId(), new ah.AsyncResource('Z', { triggerAsyncId: 3 }).triggerAsyncId());
  const als = new ah.AsyncLocalStorage({ name: 'nm', defaultValue: 9 });
  console.log(als.name, als.getStore(), als.run(1, () => als.getStore()), new ah.AsyncLocalStorage().name === '');
  console.log(typeof ah.AsyncLocalStorage.prototype.withScope, typeof als.withScope(3), als.getStore());
  const s = als.withScope(4); console.log(Object.getOwnPropertyNames(Object.getPrototypeOf(s)).join(), String(Object.getOwnPropertySymbols(Object.getPrototypeOf(s))[0]));
  s.dispose(); console.log(als.getStore());
  const snap = ah.AsyncLocalStorage.snapshot(); console.log(JSON.stringify(snap.name), snap.length, als.run(5, () => snap(() => als.getStore())));
  const b = ah.AsyncLocalStorage.bind(function q(a, b2) { return als.getStore() + a; }, 1); console.log(JSON.stringify(b.name), b.length, als.run(6, () => b()));
  const r = new ah.AsyncResource('W'); const bb = r.bind(function z(a, c) { return this; }); console.log(bb.name, bb.length);
  console.log(ah.AsyncResource.bind(function nn() {}).name, ah.AsyncResource.bind(() => {}).length);
}
{
  // Duas lojas aninhadas, `await` dentro de `run`, a cadeia de `then` e o recurso que guarda o contexto da criação.
  const a = new ah.AsyncLocalStorage();
  const b = new ah.AsyncLocalStorage();
  let resource;
  a.run('A1', () => b.run('B1', () => { resource = new ah.AsyncResource('R'); }));
  a.run('A2', async () => {
    console.log('async start', a.getStore(), b.getStore());
    await null;
    console.log('after await', a.getStore(), b.getStore());
    await b.run('B2', async () => { await new Promise((ok) => setTimeout(ok, 2)); console.log('inner', a.getStore(), b.getStore()); });
    console.log('after inner', a.getStore(), b.getStore());
    resource.runInAsyncScope(() => console.log('resource', a.getStore(), b.getStore()));
    console.log('outside resource', a.getStore(), b.getStore());
  });
  console.log('sync after run', a.getStore(), b.getStore());
}
