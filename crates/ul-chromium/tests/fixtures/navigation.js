const R = [];
const shape = (o) => Object.getOwnPropertyNames(o).map((k) => { const d = Object.getOwnPropertyDescriptor(o, k); return k + ':' + ('value' in d ? (typeof d.value === 'function' ? 'f' + d.value.length : 'v') + (d.writable ? 'w' : '') : 'a' + (d.set ? 's' : '')) + (d.enumerable ? 'e' : '') + (d.configurable ? 'c' : ''); }).join(',');
const mask = (s) => String(s).split(location.origin).join('O');
const entry = (e) => (e ? `${mask(e.url)}@${e.index}${e.sameDocument ? '' : '!'}` : String(e));
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const watch = () => {
  const out = [];
  const on = (t, f) => { navigation.addEventListener(t, f); return () => navigation.removeEventListener(t, f); };
  const offs = [
    on('navigate', (e) => out.push(`navigate ${e.navigationType} ${mask(e.destination.url)} idx=${e.destination.index} same=${e.destination.sameDocument} key=${e.destination.key === '' ? 'empty' : e.destination.key === navigation.currentEntry.key ? 'cur' : 'other'} ci=${e.canIntercept} hash=${e.hashChange} info=${JSON.stringify(e.info)} state=${JSON.stringify(e.destination.getState())} tr=${navigation.transition ? navigation.transition.navigationType : null}`)),
    on('navigatesuccess', (e) => out.push(`navigatesuccess ${e.constructor.name} tr=${navigation.transition ? navigation.transition.navigationType : null}`)),
    on('navigateerror', (e) => out.push(`navigateerror ${e.constructor.name} ${e.message} ${e.error && e.error.name}`)),
    on('currententrychange', (e) => out.push(`currententrychange ${e.navigationType} from=${entry(e.from)} to=${entry(navigation.currentEntry)} tr=${navigation.transition ? navigation.transition.navigationType + ':' + entry(navigation.transition.from) : null}`)),
  ];
  window.addEventListener('hashchange', () => out.push('hashchange'), { once: true });
  window.addEventListener('popstate', (e) => out.push('popstate ' + JSON.stringify(e.state)), { once: true });
  return { out, stop: () => offs.forEach((f) => f()) };
};
const track = (label, result, out) => {
  out.push(`${label} returned`);
  result.committed.then((e) => out.push(`${label} committed ${entry(e)}`), (e) => out.push(`${label} committed ${e.name}: ${e.message}`));
  result.finished.then((e) => out.push(`${label} finished ${entry(e)}`), (e) => out.push(`${label} finished ${e.name}: ${e.message}`));
};
const scenario = async (name, body) => {
  const w = watch();
  try { await body(w.out); } catch (e) { w.out.push(`threw ${e.name}: ${e.message}`); }
  await wait(200);
  w.stop();
  R.push(`== ${name}`, ...w.out, `now ${entry(navigation.currentEntry)} entries=${navigation.entries().map(entry).join(',')} back=${navigation.canGoBack} fwd=${navigation.canGoForward}`);
};
window.addEventListener('load', () => setTimeout(async () => {
  R.push('window.navigation ' + JSON.stringify(Object.keys(Object.getOwnPropertyDescriptor(window, 'navigation'))));
  for (const name of ['Navigation', 'NavigationHistoryEntry', 'NavigationTransition', 'NavigateEvent', 'NavigationCurrentEntryChangeEvent', 'NavigationDestination', 'NavigationActivation', 'NavigationPrecommitController']) {
    const C = window[name];
    R.push([name, typeof C, C.length, shape(C.prototype), Object.getPrototypeOf(C.prototype).constructor.name].join(' '));
    try { new C(); } catch (e) { R.push(' new ' + e.name + ': ' + e.message); }
  }
  R.push(Object.prototype.toString.call(navigation) + ' ' + String(navigation.currentEntry) + ' ' + typeof navigation.currentEntry.key + navigation.currentEntry.key.length + ' ' + (navigation.currentEntry.key === navigation.currentEntry.id) + ' ' + navigation.transition + ' ' + navigation.activation.from + ' ' + (navigation.activation.entry === navigation.currentEntry));
  for (const [n, f] of [['navigate()', () => navigation.navigate()], ['updateCurrentEntry()', () => navigation.updateCurrentEntry()], ['traverseTo()', () => navigation.traverseTo()], ['getState no state', () => String(navigation.currentEntry.getState())]]) { try { R.push(n + ' ok ' + f()); } catch (e) { R.push(n + ' ' + e.name + ': ' + e.message); } }
  // A ativação depende do modo do headless (tempo real com URL na linha de comando: `replace`; `--dump-dom`: `push`),
  // e fica num teste à parte.
  R.push(`start ${entry(navigation.currentEntry)}`);
  await scenario('navigate hash push', async (o) => { track('n', navigation.navigate('#a', { state: { s: 1 }, info: 'i' }), o); o.push(`sync ${entry(navigation.currentEntry)} state=${JSON.stringify(navigation.currentEntry.getState())}`); });
  await scenario('navigate hash replace', async (o) => { track('n', navigation.navigate('#b', { history: 'replace' }), o); });
  await scenario('navigate same url', async (o) => { track('n', navigation.navigate('#b'), o); });
  await scenario('pushState', async (o) => { history.pushState({ h: 1 }, '', '#p'); o.push(`sync ${entry(navigation.currentEntry)} state=${JSON.stringify(navigation.currentEntry.getState())}`); });
  await scenario('replaceState', async (o) => { history.replaceState(null, '', '#r'); });
  await scenario('location.hash', async (o) => { location.hash = 'h'; o.push(`sync ${entry(navigation.currentEntry)}`); });
  await scenario('back', async (o) => { track('b', navigation.back({ info: 'x' }), o); o.push(`sync ${entry(navigation.currentEntry)}`); });
  await scenario('forward', async (o) => { track('f', navigation.forward(), o); });
  await scenario('traverseTo first', async (o) => { track('t', navigation.traverseTo(navigation.entries()[0].key), o); });
  await scenario('traverseTo current', async (o) => { track('t', navigation.traverseTo(navigation.currentEntry.key), o); });
  await scenario('history.back', async (o) => { history.go(2); });
  await scenario('back at start', async (o) => { track('t', navigation.traverseTo(navigation.entries()[0].key), o); await wait(100); track('b', navigation.back(), o); });
  await scenario('forward at end', async (o) => { const es = navigation.entries(); track('t', navigation.traverseTo(es[es.length - 1].key), o); await wait(100); track('f', navigation.forward(), o); });
  await scenario('intercept', async (o) => {
    const f = (e) => { e.intercept({ handler: async () => { o.push('handler start'); await wait(10); o.push('handler end'); } }); };
    navigation.addEventListener('navigate', f, { once: true });
    track('i', navigation.navigate('/other?q'), o);
    o.push(`sync ${entry(navigation.currentEntry)} tr=${navigation.transition && navigation.transition.navigationType}`);
  });
  await scenario('preventDefault', async (o) => {
    navigation.addEventListener('navigate', (e) => e.preventDefault(), { once: true });
    track('p', navigation.navigate('#prevented'), o);
  });
  await scenario('abort by second', async (o) => { track('one', navigation.navigate('#x1'), o); track('two', navigation.navigate('#x2'), o); });
  await scenario('updateCurrentEntry', async (o) => { navigation.updateCurrentEntry({ state: [1] }); o.push(`state ${JSON.stringify(navigation.currentEntry.getState())}`); });
  await scenario('reload intercept', async (o) => {
    navigation.addEventListener('navigate', (e) => e.intercept(), { once: true });
    track('r', navigation.reload({ state: 'rs' }), o);
  });
  await scenario('navigate bad url', async (o) => { track('u', navigation.navigate('http://[x'), o); });
  await scenario('navigate js url', async (o) => { track('j', navigation.navigate('javascript:1'), o); });
  await scenario('navigateerror fields', async (o) => {
    const show = (e) => o.push(`error ${JSON.stringify([e.message, mask(e.filename), e.lineno, e.colno, e.error && e.error.name, e.cancelable, e.bubbles, e.isTrusted])}`);
    navigation.addEventListener('navigateerror', show);
    navigation.addEventListener('navigate', (e) => e.preventDefault(), { once: true });
    navigation.navigate('#cancelled');
    navigation.addEventListener('navigate', (e) => { e.intercept({ handler: () => Promise.reject(new TypeError('boom')) }); }, { once: true });
    track('h', navigation.navigate('#rejected'), o);
    await wait(50);
    navigation.addEventListener('navigate', (e) => { o.push(`traverse cancelable ${e.cancelable}`); e.preventDefault(); }, { once: true });
    track('tb', navigation.back(), o);
    await wait(100);
    navigation.removeEventListener('navigateerror', show);
  });
  await scenario('entry identity', async (o) => { o.push(`eq ${navigation.entries()[0] === navigation.entries()[0]} cur ${navigation.currentEntry === navigation.entries()[navigation.currentEntry.index]}`); });
  console.log(R.join('\n'));
}, 0));
