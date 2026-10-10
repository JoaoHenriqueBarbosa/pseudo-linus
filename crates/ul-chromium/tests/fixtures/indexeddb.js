const R = [];
const log = (...a) => R.push(a.map((x) => (typeof x === 'string' ? x : JSON.stringify(x))).join(' '));
const err = (e) => (e ? `${e.name}: ${e.message}` : String(e));
const t = (label, f) => { try { const v = f(); log(label, 'ok', v === undefined ? 'undefined' : v); return v; } catch (e) { log(label, 'threw', err(e)); return undefined; } };
const req = (r) => new Promise((resolve) => {
  r.onsuccess = () => resolve(['success', r.result]);
  r.onerror = (e) => { e.preventDefault(); resolve(['error', err(r.error)]); };
});
const done = (tx) => new Promise((resolve) => { tx.oncomplete = () => resolve('complete'); tx.onabort = () => resolve('abort ' + err(tx.error)); });
const show = (v) => (v instanceof Array ? v.map(show) : v instanceof Date ? 'Date(' + v.getTime() + ')' : v instanceof ArrayBuffer ? 'AB' + v.byteLength : v && typeof v === 'object' && !(v instanceof IDBKeyRange) ? JSON.stringify(v) : v);

(async () => {
  // cmp e chaves.
  for (const [a, b] of [[1, 2], [2, 1], [1, 1], ['a', 1], [1, 'a'], [new Date(5), 9], [[1], 'z'], [[1, 2], [1, 3]], [[], [0]], [new Uint8Array([1]).buffer, 'z'], [new Uint8Array([1]).buffer, [1]], [-Infinity, Infinity], [-0, 0]]) t('cmp ' + JSON.stringify([show(a), show(b)]), () => indexedDB.cmp(a, b));
  for (const bad of [NaN, null, undefined, {}, true, Symbol.iterator, new Date(NaN), [NaN], function () {}]) t('cmp bad ' + String(bad && bad.toString ? (typeof bad === 'symbol' ? 'sym' : bad) : bad).slice(0, 20), () => indexedDB.cmp(bad, 1));
  t('cmp 1 arg', () => indexedDB.cmp(1));
  t('open no arg', () => indexedDB.open());
  t('open v0', () => indexedDB.open('x', 0));
  t('open v-1', () => indexedDB.open('x', -1));
  t('open vNaN', () => indexedDB.open('x', NaN));
  // KeyRange.
  const kr = IDBKeyRange.bound(1, 5, true, false);
  log('range', kr.lower, kr.upper, kr.lowerOpen, kr.upperOpen, kr.includes(1), kr.includes(5), kr.includes(3), String(kr));
  t('bound inverted', () => IDBKeyRange.bound(5, 1));
  t('bound equal open', () => IDBKeyRange.bound(1, 1, true));
  t('only bad', () => IDBKeyRange.only(null));
  t('includes bad', () => kr.includes({}));
  t('lowerBound', () => [IDBKeyRange.lowerBound('a').lower, IDBKeyRange.lowerBound('a').upper, IDBKeyRange.lowerBound('a').upperOpen]);

  // Abertura com upgrade e a ordem dos eventos.
  const order = [];
  const open = indexedDB.open('db1', 2);
  log('open req', String(open), open.readyState, t('result early', () => open.result), t('error early', () => open.error), open.source, open.transaction);
  open.onupgradeneeded = (e) => {
    order.push(`upgradeneeded ${e.constructor.name} old=${e.oldVersion} new=${e.newVersion} ${e.dataLoss} ${JSON.stringify(e.dataLossMessage)} tx=${open.transaction && open.transaction.mode} ready=${open.readyState}`);
    const db = open.result;
    const s = db.createObjectStore('people', { keyPath: 'id' });
    s.createIndex('byName', 'name');
    s.createIndex('byTag', 'tags', { multiEntry: true });
    s.createIndex('byEmail', 'email', { unique: true });
    db.createObjectStore('auto', { autoIncrement: true });
    db.createObjectStore('kp', { keyPath: ['a', 'b'] });
    db.createObjectStore('plain');
    order.push('stores ' + Array.from(db.objectStoreNames).join(',') + ' ' + String(db.objectStoreNames) + ' ' + db.objectStoreNames.contains('auto') + ' ' + db.objectStoreNames.length);
    order.push('indexNames ' + Array.from(s.indexNames).join(','));
    order.push('store props ' + JSON.stringify([s.name, s.keyPath, s.autoIncrement, s.transaction === open.transaction]));
    for (const [n, f] of [['dup store', () => db.createObjectStore('plain')], ['bad keyPath', () => db.createObjectStore('z', { keyPath: 'a b' })], ['auto empty kp', () => db.createObjectStore('z2', { keyPath: '', autoIncrement: true })], ['auto array kp', () => db.createObjectStore('z3', { keyPath: ['a'], autoIncrement: true })], ['dup index', () => s.createIndex('byName', 'x')], ['multi array', () => s.createIndex('m', ['a', 'b'], { multiEntry: true })], ['delete missing', () => db.deleteObjectStore('nope')], ['index missing', () => s.index('nope')]]) {
      try { f(); order.push(n + ' ok'); } catch (x) { order.push(n + ' ' + err(x)); }
    }
    open.transaction.oncomplete = () => order.push('versionchange tx complete');
  };
  open.onsuccess = () => order.push(`success ready=${open.readyState} tx=${open.transaction}`);
  open.onblocked = () => order.push('blocked');
  open.onerror = () => order.push('error');
  await new Promise((r) => { const prev = open.onsuccess; open.onsuccess = (e) => { prev(e); r(); }; });
  log('open order', order);
  const db = open.result;
  log('db', String(db), db.name, db.version, Array.from(db.objectStoreNames));
  t('createObjectStore outside upgrade', () => db.createObjectStore('late'));
  t('transaction no stores', () => db.transaction([]));
  t('transaction missing', () => db.transaction('nope'));
  t('transaction bad mode', () => db.transaction('plain', 'versionchange'));

  // Escrita e leitura.
  let tx = db.transaction(['people', 'auto', 'kp', 'plain'], 'readwrite');
  log('tx', String(tx), tx.mode, tx.durability, Array.from(tx.objectStoreNames), tx.db === db, tx.error);
  const people = tx.objectStore('people');
  const rows = [{ id: 3, name: 'c', tags: ['x', 'y'], email: 'c@x' }, { id: 1, name: 'a', tags: ['y'], email: 'a@x' }, { id: 2, name: 'b', tags: [], email: 'b@x' }, { id: 4, name: 'a', tags: ['x', 'x'] }];
  const results = [];
  for (const row of rows) results.push(req(people.add(row)));
  results.push(req(people.add({ id: 1, name: 'dup' })));
  results.push(req(people.put({ id: 5, name: 'e', email: 'a@x' })));
  for (const [n, f] of [['add no key', () => people.add({ name: 'x' })], ['put explicit key with kp', () => people.put({ id: 9 }, 9)], ['add bad key', () => people.add({ id: {} })], ['clone fail', () => people.add({ id: 7, f() {} })]]) t(n, f);
  const auto = tx.objectStore('auto');
  results.push(req(auto.add('first')), req(auto.add('second')), req(auto.put('ten', 10)), req(auto.add('after')), req(auto.add('float', 20.5)), req(auto.add('next')));
  const kp = tx.objectStore('kp');
  results.push(req(kp.put({ a: 1, b: 'x', v: 1 })), req(kp.put({ a: 1, b: 'a', v: 2 })));
  const plain = tx.objectStore('plain');
  t('plain no key', () => plain.add('v'));
  results.push(req(plain.put({ d: new Date(7), buf: new Uint8Array([1, 2]).buffer, m: new Map([[1, 2]]) }, 'k1')), req(plain.put('s', [1, 'a'])), req(plain.put('d', new Date(3))));
  log('tx results', (await Promise.all(results)).map(([k, v]) => [k, show(v)]));
  log('tx end', await done(tx));
  t('use finished tx', () => people.get(1));

  tx = db.transaction(['people', 'auto', 'kp', 'plain']);
  const ps = tx.objectStore('people');
  t('readonly put', () => ps.put({ id: 8 }));
  const reads = {
    get1: ps.get(1), getRange: ps.get(IDBKeyRange.lowerBound(2)), getMissing: ps.get(99), getKey: ps.getKey(IDBKeyRange.bound(2, 9)), count: ps.count(), countRange: ps.count(IDBKeyRange.upperBound(2)),
    getAll: ps.getAll(), getAllCount: ps.getAll(null, 2), getAllKeys: ps.getAllKeys(IDBKeyRange.lowerBound(2, true)), byName: ps.index('byName').getAll('a'), byNameKeys: ps.index('byName').getAllKeys(), byTag: ps.index('byTag').getAllKeys('x'),
    tagCount: ps.index('byTag').count(), emailGet: ps.index('byEmail').get('b@x'), emailKey: ps.index('byEmail').getKey('c@x'), auto: tx.objectStore('auto').getAllKeys(), kp: tx.objectStore('kp').getAll(), plain: tx.objectStore('plain').getAllKeys(), plainV: tx.objectStore('plain').get('k1'),
  };
  for (const [k, r] of Object.entries(reads)) { const [s, v] = await req(r); log('read', k, s, show(v)); }
  const recs = await req(ps.getAllRecords({ query: IDBKeyRange.lowerBound(2), count: 2, direction: 'prev' }));
  log('records', recs[0], recs[1].map((r) => [String(r), r.key, r.primaryKey, show(r.value)]));
  const irecs = await req(ps.index('byName').getAllRecords());
  log('index records', irecs[1].map((r) => [r.key, r.primaryKey]));
  t('getAll bad count', () => ps.getAll(null, -1));
  t('get no arg', () => ps.get());
  // Cursores.
  const walk = (request, steps) => new Promise((resolve) => {
    const seen = [];
    request.onsuccess = () => {
      const c = request.result;
      if (!c) { resolve(seen); return; }
      seen.push([String(c), c.key, c.primaryKey, 'value' in c ? show(c.value) : '-', c.direction, c.source === request.source, c.request === request]);
      const step = steps.shift();
      if (step) step(c); else c.continue();
    };
    request.onerror = (e) => { e.preventDefault(); resolve(seen.concat(['error ' + err(request.error)])); };
  });
  log('cursor next', await walk(ps.openCursor(), []));
  log('cursor prev range', await walk(ps.openCursor(IDBKeyRange.bound(1, 4), 'prev'), []));
  log('cursor advance', await walk(ps.openCursor(), [(c) => c.advance(2)]));
  log('cursor continue key', await walk(ps.openCursor(), [(c) => c.continue(4)]));
  log('index cursor nextunique', await walk(ps.index('byName').openCursor(null, 'nextunique'), []));
  log('index key cursor prevunique', await walk(ps.index('byTag').openKeyCursor(null, 'prevunique'), []));
  log('index continuePrimaryKey', await walk(ps.index('byName').openCursor(), [(c) => c.continuePrimaryKey('a', 4)]));
  log('cursor errors', await walk(ps.openCursor(), [(c) => { for (const [n, f] of [['advance0', () => c.advance(0)], ['continue back', () => c.continue(0)], ['update ro', () => c.update({ id: 1 })], ['delete ro', () => c.delete()]]) t(n, f); c.continue(); }]));
  t('bad direction', () => ps.openCursor(null, 'sideways'));
  log('tx2 end', await done(tx));

  // Cursor com escrita, delete por faixa, clear e abort.
  tx = db.transaction('people', 'readwrite');
  const pw = tx.objectStore('people');
  log('cursor update', await walk(pw.openCursor(2), [(c) => { const r = c.update({ ...c.value, name: 'B' }); r.onsuccess = () => log('update result', r.result); c.continue(); }]));
  t('cursor update key change', () => 1);
  log('delete range', await req(pw.delete(IDBKeyRange.bound(4, 5))));
  log('after delete keys', await req(pw.getAllKeys()));
  log('tx3', await done(tx));
  tx = db.transaction('people', 'readwrite');
  tx.objectStore('people').clear();
  tx.abort();
  t('abort twice', () => tx.abort());
  log('aborted', await done(tx), tx.error);
  tx = db.transaction('people', 'readonly');
  log('after abort keys', await req(tx.objectStore('people').getAllKeys()));
  // Erro numa requisição sem preventDefault aborta a transação.
  tx = db.transaction('people', 'readwrite');
  const bad = tx.objectStore('people').add({ id: 1 });
  bad.onerror = () => log('request error event', err(bad.error), bad.readyState);
  log('constraint tx', await done(tx));
  // commit() explícito e o momento do auto-commit.
  tx = db.transaction('people', 'readwrite');
  tx.objectStore('people').put({ id: 30, name: 'z' });
  tx.commit();
  t('request after commit', () => tx.objectStore('people').get(1));
  log('committed', await done(tx));
  tx = db.transaction('people');
  await Promise.resolve();
  t('request after microtask', () => tx.objectStore('people').get(1));
  await new Promise((r) => setTimeout(r, 0));
  t('request after task', () => tx.objectStore('people').get(1));
  // databases, versionchange e blocked, close e deleteDatabase.
  log('databases', await indexedDB.databases());
  const events = [];
  db.onversionchange = (e) => { events.push(`versionchange old=${e.oldVersion} new=${e.newVersion}`); };
  const up = indexedDB.open('db1', 3);
  up.onblocked = (e) => { events.push(`blocked old=${e.oldVersion} new=${e.newVersion}`); db.close(); };
  up.onupgradeneeded = (e) => events.push(`upgrade old=${e.oldVersion} new=${e.newVersion}`);
  await new Promise((r) => { up.onsuccess = () => { events.push('success v' + up.result.version); r(); }; });
  log('upgrade events', events);
  t('closed db transaction', () => db.transaction('people'));
  const lower = indexedDB.open('db1', 1);
  log('lower version', await new Promise((r) => { lower.onerror = (e) => { e.preventDefault(); r(err(lower.error)); }; lower.onsuccess = () => r('success'); }));
  up.result.close();
  const del = indexedDB.deleteDatabase('db1');
  log('delete', await new Promise((r) => { del.onsuccess = (e) => r([e.constructor.name, e.oldVersion, e.newVersion, del.result]); del.onerror = () => r('error'); }));
  log('databases after', await indexedDB.databases());
  // Upgrade que lança aborta a abertura.
  const boom = indexedDB.open('db2');
  boom.onupgradeneeded = () => { throw new Error('upgrade boom'); };
  log('upgrade throws', await new Promise((r) => { boom.onerror = (e) => { e.preventDefault(); r(err(boom.error)); }; boom.onsuccess = () => r('success ' + boom.result.version); }));
  log('databases end', await indexedDB.databases());
  console.log(R.join('\n'));
})().catch((e) => { R.push('FATAL ' + err(e) + ' ' + e.stack); console.log(R.join('\n')); });
window.addEventListener('error', (e) => R.push('window error ' + e.message));
