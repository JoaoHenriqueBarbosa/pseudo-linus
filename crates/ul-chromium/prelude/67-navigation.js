// ---- Navigation API (`navigation`): as entradas da mesma origem do histórico da sessão, a ordem dos eventos
// (`navigate`, `currententrychange`, `navigatesuccess`, `navigateerror`), as promessas `committed` e `finished`, a
// interceptação, o cancelamento, o aborto por outra navegação e os percursos (`back`, `forward`, `traverseTo`, e o
// `history.go`) medidos no oráculo em tempo real. As entradas são as do `history` (`60-window.js`): cada uma ganha
// chave, id e o estado da Navigation API, e o `pushState`, o `replaceState`, o fragmento do `location` e o `go` passam
// por aqui.

// A ordem dos membros do protótipo como o Blink a instala (`order` vai para o fim, na ordem dada), os membros
// enumeráveis e a etiqueta do `toString`.
const blinkInterface = (cls, name, order) => {
  for (const key of order) {
    const descriptor = Object.getOwnPropertyDescriptor(cls.prototype, key);
    delete cls.prototype[key];
    Object.defineProperty(cls.prototype, key, descriptor);
  }
  return webInterface(cls, name);
};
const settledPair = () => {
  const pair = {};
  for (const name of ['committed', 'finished']) {
    let resolve;
    let reject;
    const promise = new Promise((ok, fail) => { resolve = ok; reject = fail; });
    // As duas promessas saem marcadas como tratadas: a rejeição não vira `unhandledrejection`.
    promise.catch(() => {});
    pair[name] = { promise, resolve, reject };
  }
  return pair;
};
const navigationResult = (pair) => ({ committed: pair.committed.promise, finished: pair.finished.promise });
const rejectedResult = (error) => {
  const pair = settledPair();
  pair.committed.reject(error);
  pair.finished.reject(error);
  return navigationResult(pair);
};
const abortedError = () => domException('Navigation was aborted', 'AbortError');
const cloneState = (state) => (state === undefined ? undefined : structuredClone(state));
const originOfUrl = (url) => {
  try { return new URL(url).origin; } catch { return 'null'; }
};
const fragmentOnlyChange = (from, to) => sameDocument(from, to) && from !== to && new URL(from).hash !== new URL(to).hash;

class NavigationHistoryEntry extends EventTarget {
  constructor(key = undefined, record = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('NavigationHistoryEntry');
    super();
    hidden(this, '__navigationEntry', record);
  }
  get key() { receiver(this, P.__navigationEntry); return this[P.__navigationEntry].key; }
  get id() { receiver(this, P.__navigationEntry); return this[P.__navigationEntry].id; }
  get url() { receiver(this, P.__navigationEntry); return this[P.__navigationEntry].url; }
  get index() { receiver(this, P.__navigationEntry); return indexOfRecord(this[P.__navigationEntry]); }
  get sameDocument() { receiver(this, P.__navigationEntry); return true; }
  getState() { receiver(this, P.__navigationEntry); return cloneState(this[P.__navigationEntry].navigationState); }
}
defineHandlerProperties(NavigationHistoryEntry.prototype, ['dispose']);
blinkInterface(NavigationHistoryEntry, 'NavigationHistoryEntry', ['getState', 'constructor']);

class NavigationDestination {
  constructor(key = undefined, fields = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('NavigationDestination');
    hidden(this, '__destination', fields);
  }
  get key() { receiver(this, P.__destination); return this[P.__destination].key; }
  get id() { receiver(this, P.__destination); return this[P.__destination].id; }
  get url() { receiver(this, P.__destination); return this[P.__destination].url; }
  get index() { receiver(this, P.__destination); return this[P.__destination].index; }
  get sameDocument() { receiver(this, P.__destination); return this[P.__destination].sameDocument; }
  getState() { receiver(this, P.__destination); return cloneState(this[P.__destination].state); }
}
blinkInterface(NavigationDestination, 'NavigationDestination', ['constructor']);

class NavigationTransition {
  constructor(key = undefined, fields = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('NavigationTransition');
    hidden(this, '__transition', fields);
  }
  get navigationType() { receiver(this, P.__transition); return this[P.__transition].navigationType; }
  get from() { receiver(this, P.__transition); return this[P.__transition].from; }
  get to() { receiver(this, P.__transition); return this[P.__transition].to; }
  get committed() { receiver(this, P.__transition); return this[P.__transition].committed; }
  get finished() { receiver(this, P.__transition); return this[P.__transition].finished; }
}
blinkInterface(NavigationTransition, 'NavigationTransition', ['constructor']);

class NavigationActivation {
  constructor(key = undefined, fields = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('NavigationActivation');
    hidden(this, '__activation', fields);
  }
  get entry() { receiver(this, P.__activation); return this[P.__activation].entry; }
  get from() { receiver(this, P.__activation); return this[P.__activation].from; }
  get navigationType() { receiver(this, P.__activation); return this[P.__activation].navigationType; }
}
blinkInterface(NavigationActivation, 'NavigationActivation', ['constructor']);

// Só existe dentro do `precommitHandler` do `intercept`, que o headless não chama sem navegação entre documentos.
class NavigationPrecommitController {
  constructor(key = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('NavigationPrecommitController');
    hidden(this, '__precommit', true);
  }
  addHandler(handler) {
    receiver(this, P.__precommit);
    if (arguments.length < 1) throw new TypeError("Failed to execute 'addHandler' on 'NavigationPrecommitController': 1 argument required, but only 0 present.");
  }
  redirect(url) {
    receiver(this, P.__precommit);
    if (arguments.length < 1) throw new TypeError("Failed to execute 'redirect' on 'NavigationPrecommitController': 1 argument required, but only 0 present.");
  }
}
blinkInterface(NavigationPrecommitController, 'NavigationPrecommitController', ['constructor']);

const requireEventInit = (name, type, init, count, required) => {
  if (count < 2) throw new TypeError(`Failed to construct '${name}': 2 arguments required, but only ${count} present.`);
  if (init === null || typeof init !== 'object') throw new TypeError(`Failed to construct '${name}': The provided value is not of type '${name}Init'.`);
  for (const member of required) {
    if (init[member] === undefined) throw new TypeError(`Failed to construct '${name}': Failed to read the '${member}' property from '${name}Init': Required member is undefined.`);
  }
};

class NavigateEvent extends Event {
  constructor(type, init) {
    requireEventInit('NavigateEvent', type, init, arguments.length, ['destination']);
    super(type, init);
    hidden(this, '__navigateEvent', {
      navigationType: init.navigationType === undefined ? 'push' : String(init.navigationType), destination: init.destination,
      canIntercept: !!init.canIntercept, userInitiated: !!init.userInitiated, hashChange: !!init.hashChange,
      signal: init.signal === undefined ? new AbortController().signal : init.signal, formData: init.formData === undefined ? null : init.formData,
      downloadRequest: init.downloadRequest === undefined ? null : init.downloadRequest, info: init.info, sourceElement: init.sourceElement === undefined ? null : init.sourceElement,
      hasUAVisualTransition: !!init.hasUAVisualTransition, navigation: null,
    });
  }
  get navigationType() { receiver(this, P.__navigateEvent); return this[P.__navigateEvent].navigationType; }
  get destination() { receiver(this, P.__navigateEvent); return this[P.__navigateEvent].destination; }
  get canIntercept() { receiver(this, P.__navigateEvent); return this[P.__navigateEvent].canIntercept; }
  get userInitiated() { receiver(this, P.__navigateEvent); return this[P.__navigateEvent].userInitiated; }
  get hashChange() { receiver(this, P.__navigateEvent); return this[P.__navigateEvent].hashChange; }
  get signal() { receiver(this, P.__navigateEvent); return this[P.__navigateEvent].signal; }
  get formData() { receiver(this, P.__navigateEvent); return this[P.__navigateEvent].formData; }
  get downloadRequest() { receiver(this, P.__navigateEvent); return this[P.__navigateEvent].downloadRequest; }
  get info() { receiver(this, P.__navigateEvent); return this[P.__navigateEvent].info; }
  get sourceElement() { receiver(this, P.__navigateEvent); return this[P.__navigateEvent].sourceElement; }
  intercept(options = {}) {
    receiver(this, P.__navigateEvent);
    const state = this[P.__navigateEvent];
    if (!this.isTrusted || state.navigation === null || !this[P.__dispatching]) {
      throw domException("Failed to execute 'intercept' on 'NavigateEvent': intercept() may only be called while the navigate event is being dispatched.", 'InvalidStateError');
    }
    if (!state.canIntercept) throw domException("Failed to execute 'intercept' on 'NavigateEvent': A navigation with URL '" + state.destination.url + "' cannot be intercepted by in a window with origin '" + location.origin + "' and URL '" + location.href + "'.", 'SecurityError');
    if (this.defaultPrevented) throw domException("Failed to execute 'intercept' on 'NavigateEvent': intercept() may not be called after preventDefault().", 'InvalidStateError');
    const handler = options === null || options === undefined ? undefined : options.handler;
    if (handler !== undefined && typeof handler !== 'function') throw new TypeError("Failed to execute 'intercept' on 'NavigateEvent': Failed to read the 'handler' property from 'NavigationInterceptOptions': The provided value is not a function.");
    state.navigation.intercepted = true;
    if (handler !== undefined) state.navigation.handlers.push(handler);
  }
  scroll() {
    receiver(this, P.__navigateEvent);
    if (!this[P.__navigateEvent].navigation || !this[P.__navigateEvent].navigation.intercepted) {
      throw domException("Failed to execute 'scroll' on 'NavigateEvent': intercept() must be called before scroll().", 'InvalidStateError');
    }
  }
  get hasUAVisualTransition() { receiver(this, P.__navigateEvent); return this[P.__navigateEvent].hasUAVisualTransition; }
}
blinkInterface(NavigateEvent, 'NavigateEvent', ['constructor']);

class NavigationCurrentEntryChangeEvent extends Event {
  constructor(type, init) {
    requireEventInit('NavigationCurrentEntryChangeEvent', type, init, arguments.length, ['from']);
    super(type, init);
    hidden(this, '__entryChange', { navigationType: init.navigationType === undefined ? null : init.navigationType, from: init.from });
  }
  get navigationType() { receiver(this, P.__entryChange); return this[P.__entryChange].navigationType; }
  get from() { receiver(this, P.__entryChange); return this[P.__entryChange].from; }
}
blinkInterface(NavigationCurrentEntryChangeEvent, 'NavigationCurrentEntryChangeEvent', ['constructor']);

// As entradas: os registros do `history` com chave, id, estado da Navigation API e o objeto da entrada.
const newKey = () => crypto.randomUUID();
const ensureRecord = (record) => {
  if (record.key === undefined) {
    record.key = newKey();
    record.id = newKey();
  }
  return record;
};
const historyRecords = () => history[P.__entries];
// O trecho do histórico que a Navigation API enxerga: as entradas vizinhas da atual com a mesma origem.
const navigableRange = () => {
  const records = historyRecords();
  const current = history[P.__index];
  const origin = originOfUrl(records[current].url);
  let first = current;
  while (first > 0 && originOfUrl(records[first - 1].url) === origin) first--;
  let last = current;
  while (last + 1 < records.length && originOfUrl(records[last + 1].url) === origin) last++;
  return [first, last];
};
const indexOfRecord = (record) => {
  const position = historyRecords().indexOf(record);
  const [first, last] = navigableRange();
  return position < first || position > last ? -1 : position - first;
};
const entryOf = (record) => {
  ensureRecord(record);
  if (record.view === undefined) Object.defineProperty(record, 'view', { value: new NavigationHistoryEntry(ILLEGAL, record) });
  return record.view;
};
const currentRecord = () => historyRecords()[history[P.__index]];
const dispose = (record) => {
  if (record.view !== undefined) {
    const event = new Event('dispose');
    event.isTrusted = true;
    record.view.dispatchEvent(event);
  }
};
// `fromTask`: a navegação roda numa tarefa do navegador (o percurso), com checkpoint de microtarefas entre ouvintes.
const fireTrusted = (target, event, fromTask = false) => {
  event.isTrusted = true;
  if (fromTask) dispatchFromTask(target, event);
  else target.dispatchEvent(event);
  return event;
};

// A navegação em curso: só uma de cada vez; a seguinte aborta a anterior.
let ongoingNavigation = null;
let currentTransition = null;
// O relato de uma navegação que falhou: um erro nativo do motor dá `Uncaught Tipo: mensagem` e a posição da pilha
// dele; o `DOMException` (que o V8 não formata) dá só `Uncaught ` e a posição de quem começou a navegação (medido).
const fireNavigateError = (error, fallback, fromTask) => {
  const native = error instanceof Error && !(error instanceof DOMException);
  const position = (native && stackPosition(error.stack)) || fallback;
  let text = '';
  if (native) {
    try { text = String(error); } catch { text = ''; }
  }
  fireTrusted(navigation, new ErrorEvent('navigateerror', { message: 'Uncaught ' + text, filename: position[0], lineno: position[1], colno: position[2], error, cancelable: true }), fromTask);
};
// As promessas são resolvidas (ou rejeitadas) antes do evento: com checkpoint entre ouvintes, as reações delas rodam
// entre um ouvinte e outro (medido no percurso cancelado). `fromTask` é o contexto de quem encerrou a navegação.
const finishNavigation = (current, error, fromTask = false) => {
  if (ongoingNavigation !== current) return;
  ongoingNavigation = null;
  if (error === undefined) {
    fireTrusted(navigation, new Event('navigatesuccess'));
    currentTransition = null;
    current.pair.finished.resolve(entryOf(currentRecord()));
    return;
  }
  current.controller.abort(error);
  currentTransition = null;
  if (!current.committed) current.pair.committed.reject(error);
  current.pair.finished.reject(error);
  fireNavigateError(error, current.position, fromTask);
};
const abortOngoingNavigation = (fromTask) => {
  if (ongoingNavigation !== null) finishNavigation(ongoingNavigation, abortedError(), fromTask);
};

// Uma navegação da página: dispara o `navigate` e, se ninguém cancelou e ela fica no documento (fragmento, estado do
// histórico ou interceptada), aplica `commit` às entradas e completa. Devolve se aconteceu.
const runNavigation = (request) => {
  const fromTask = !!request.fromTask;
  abortOngoingNavigation(fromTask);
  const current = { pair: request.pair || settledPair(), position: request.position, controller: new AbortController(), handlers: [], intercepted: false, committed: false };
  ongoingNavigation = current;
  const destination = new NavigationDestination(ILLEGAL, request.destination);
  const event = new NavigateEvent('navigate', {
    cancelable: true, navigationType: request.type, destination, canIntercept: true, hashChange: request.hashChange, signal: current.controller.signal, info: request.info,
  });
  event[P.__navigateEvent].navigation = current;
  fireTrusted(navigation, event, fromTask);
  event[P.__navigateEvent].navigation = null;
  if (ongoingNavigation !== current) return false;
  if (event.defaultPrevented) {
    finishNavigation(current, abortedError(), fromTask);
    return false;
  }
  if (!request.destination.sameDocument && !current.intercepted) {
    // Navegação para outro documento: o hospedeiro carrega a página nova, que tem a sua própria Navigation API.
    ongoingNavigation = null;
    op('navigate', request.destination.url);
    return false;
  }
  const from = entryOf(currentRecord());
  request.commit();
  if (current.intercepted) {
    currentTransition = new NavigationTransition(ILLEGAL, { navigationType: request.type, from, to: destination, committed: current.pair.committed.promise, finished: current.pair.finished.promise });
  }
  fireTrusted(navigation, new NavigationCurrentEntryChangeEvent('currententrychange', { navigationType: request.type, from }), fromTask);
  if (request.afterCommit) request.afterCommit(current.intercepted);
  current.committed = true;
  current.pair.committed.resolve(entryOf(currentRecord()));
  // O "wait for all" do Blink: uma reação direta em cada promessa dos tratadores (sem o salto do `Promise.all`), e sem
  // tratador o sucesso numa microtarefa.
  const settled = current.handlers.map((handler) => {
    try { return Promise.resolve(handler()); } catch (error) { return Promise.reject(error); }
  });
  if (settled.length === 0) {
    Promise.resolve().then(() => finishNavigation(current));
  } else {
    let pending = settled.length;
    for (const promise of settled) {
      promise.then(() => { if (--pending === 0) finishNavigation(current); }, (error) => finishNavigation(current, error === undefined ? null : error));
    }
  }
  return true;
};

// Acrescenta uma entrada depois da atual (as seguintes somem) ou troca a atual, como o histórico da sessão.
const pushRecord = (record) => {
  const records = historyRecords();
  for (const removed of records.splice(history[P.__index] + 1)) dispose(removed);
  records.push(record);
  history[P.__index]++;
  pageState.url = record.url;
};
const replaceRecord = (record) => {
  const records = historyRecords();
  const old = records[history[P.__index]];
  ensureRecord(old);
  record.key = old.key;
  record.id = newKey();
  records[history[P.__index]] = record;
  pageState.url = record.url;
  dispose(old);
};
const sameDocumentDestination = (url, state) => ({ url, key: '', id: '', index: -1, sameDocument: true, state });

// O fragmento do `location` e o `navigation.navigate` para o mesmo documento: popstate na hora, hashchange numa tarefa
// (a navegação interceptada não dispara nenhum dos dois, medido).
const fragmentNavigation = (url, type, options) => {
  const old = pageState.url;
  return runNavigation({
    type, info: options.info, pair: options.pair, position: options.position, hashChange: fragmentOnlyChange(old, url),
    destination: sameDocumentDestination(url, options.state),
    commit: () => {
      const record = { state: null, url, navigationState: options.state };
      if (type === 'push') pushRecord(record);
      else replaceRecord(record);
    },
    afterCommit: (intercepted) => {
      if (intercepted) return;
      window.dispatchEvent(new PopStateEvent('popstate', { state: null }));
      if (fragmentOnlyChange(old, url)) setTimeout(() => { window.dispatchEvent(new HashChangeEvent('hashchange', { oldURL: old, newURL: url })); }, 0);
    },
  });
};
// `pushState` e `replaceState`: sem popstate nem hashchange, e o estado da Navigation API da entrada nova é vazio.
const historyStateNavigation = (type, state, url) => runNavigation({
  type, position: callerPosition(), hashChange: false, destination: sameDocumentDestination(url, undefined),
  commit: () => {
    const record = { state, url, navigationState: undefined };
    if (type === 'push') pushRecord(record);
    else replaceRecord(record);
  },
});
// O percurso até a entrada `target` do histórico, numa tarefa: `navigate`, troca da entrada, `currententrychange`,
// `committed`, `navigatesuccess` e `finished`; popstate e hashchange numa tarefa depois.
const traverseHistory = (target, info, pair) => {
  setTimeout(() => {
    const records = historyRecords();
    if (target < 0 || target >= records.length) return;
    const record = ensureRecord(records[target]);
    const old = pageState.url;
    const [first] = navigableRange();
    runNavigation({
      type: 'traverse', fromTask: true, info, pair, position: [pageState.url, 0, 0], hashChange: fragmentOnlyChange(old, record.url),
      destination: { url: record.url, key: record.key, id: record.id, index: target - first, sameDocument: true, state: record.navigationState },
      commit: () => {
        history[P.__index] = target;
        pageState.url = record.url;
      },
      afterCommit: () => {
        setTimeout(() => {
          dispatchFromTask(window, new PopStateEvent('popstate', { state: record.state }));
          if (fragmentOnlyChange(old, record.url)) dispatchFromTask(window, new HashChangeEvent('hashchange', { oldURL: old, newURL: record.url }));
        }, 0);
      },
    });
  }, 0);
};

const navigationOperation = (name, count, needed) => {
  if (count < needed) throw new TypeError(`Failed to execute '${name}' on 'Navigation': ${needed} argument required, but only ${count} present.`);
};
// O URL de `navigate` como o `KURL` o mostra na mensagem: o host sem caminho ganha a barra.
const invalidUrlText = (text) => (/^[a-z][a-z0-9+.-]*:\/\/[^/?#]*$/i.test(text) ? text + '/' : text);

class Navigation extends EventTarget {
  constructor(key = undefined) {
    if (key !== ILLEGAL) throw illegalConstructor('Navigation');
    super();
    hidden(this, '__navigation', true);
  }
  get currentEntry() { receiver(this, P.__navigation); return entryOf(currentRecord()); }
  get transition() { receiver(this, P.__navigation); return currentTransition; }
  get activation() { receiver(this, P.__navigation); return activation; }
  get canGoBack() { receiver(this, P.__navigation); return history[P.__index] > navigableRange()[0]; }
  get canGoForward() { receiver(this, P.__navigation); return history[P.__index] < navigableRange()[1]; }
  back(options = {}) {
    receiver(this, P.__navigation);
    if (!this.canGoBack) return rejectedResult(domException('Cannot go back', 'InvalidStateError'));
    const pair = settledPair();
    traverseHistory(history[P.__index] - 1, options === null ? undefined : options.info, pair);
    return navigationResult(pair);
  }
  entries() {
    receiver(this, P.__navigation);
    const [first, last] = navigableRange();
    return historyRecords().slice(first, last + 1).map(entryOf);
  }
  forward(options = {}) {
    receiver(this, P.__navigation);
    if (!this.canGoForward) return rejectedResult(domException('Cannot go forward', 'InvalidStateError'));
    const pair = settledPair();
    traverseHistory(history[P.__index] + 1, options === null ? undefined : options.info, pair);
    return navigationResult(pair);
  }
  navigate(url, options = {}) {
    receiver(this, P.__navigation);
    navigationOperation('navigate', arguments.length, 1);
    const position = callerPosition();
    const text = String(url);
    let target;
    try { target = new URL(text, pageState.url).href; } catch { return rejectedResult(domException(`Invalid URL '${invalidUrlText(text)}'.`, 'SyntaxError')); }
    if (/^javascript:/i.test(target)) return rejectedResult(domException('The javascript: protocol is not supported', 'NotSupportedError'));
    const opts = options === null ? {} : options;
    const state = opts.state === undefined ? undefined : structuredClone(opts.state);
    // `auto` vira `replace` para o mesmo URL e enquanto o documento carrega (medido).
    const behavior = opts.history === undefined ? 'auto' : String(opts.history);
    const type = behavior === 'replace' || (behavior === 'auto' && (target === pageState.url || document.readyState === 'loading')) ? 'replace' : 'push';
    const pair = settledPair();
    if (sameDocument(target, pageState.url)) {
      fragmentNavigation(target, type, { info: opts.info, pair, position, state });
    } else {
      runNavigation({
        type, info: opts.info, pair, position, hashChange: false, destination: { url: target, key: '', id: '', index: -1, sameDocument: false, state },
        commit: () => {
          const record = { state: null, url: target, navigationState: state };
          if (type === 'push') pushRecord(record);
          else replaceRecord(record);
        },
      });
    }
    return navigationResult(pair);
  }
  reload(options = {}) {
    receiver(this, P.__navigation);
    const opts = options === null ? {} : options;
    const record = currentRecord();
    const state = opts.state === undefined ? record.navigationState : structuredClone(opts.state);
    const pair = settledPair();
    runNavigation({
      type: 'reload', info: opts.info, pair, position: [pageState.url, 0, 0], hashChange: false,
      destination: { url: pageState.url, key: '', id: '', index: -1, sameDocument: false, state },
      commit: () => { record.navigationState = state; },
    });
    return navigationResult(pair);
  }
  traverseTo(key, options = {}) {
    receiver(this, P.__navigation);
    navigationOperation('traverseTo', arguments.length, 1);
    const [first, last] = navigableRange();
    const records = historyRecords();
    const wanted = String(key);
    let target = -1;
    for (let index = first; index <= last; index++) if (ensureRecord(records[index]).key === wanted) target = index;
    if (target === -1) return rejectedResult(domException('Invalid key', 'InvalidStateError'));
    const pair = settledPair();
    if (target === history[P.__index]) {
      pair.committed.resolve(entryOf(currentRecord()));
      pair.finished.resolve(entryOf(currentRecord()));
    } else {
      traverseHistory(target, options === null ? undefined : options.info, pair);
    }
    return navigationResult(pair);
  }
  updateCurrentEntry(options) {
    receiver(this, P.__navigation);
    navigationOperation('updateCurrentEntry', arguments.length, 1);
    if (options === null || typeof options !== 'object' || !('state' in options)) {
      throw new TypeError("Failed to execute 'updateCurrentEntry' on 'Navigation': Failed to read the 'state' property from 'NavigationUpdateCurrentEntryOptions': Required member is undefined.");
    }
    const record = currentRecord();
    record.navigationState = structuredClone(options.state);
    const from = entryOf(record);
    fireTrusted(navigation, new NavigationCurrentEntryChangeEvent('currententrychange', { navigationType: null, from }));
  }
}
defineHandlerProperties(Navigation.prototype, ['navigate', 'navigatesuccess', 'navigateerror', 'currententrychange']);
blinkInterface(Navigation, 'Navigation', ['back', 'entries', 'forward', 'navigate', 'reload', 'traverseTo', 'updateCurrentEntry', 'constructor']);

const navigation = new Navigation(ILLEGAL);
const activation = new NavigationActivation(ILLEGAL, { entry: entryOf(currentRecord()), from: null, navigationType: 'push' });
replaceable('navigation', () => navigation);
const blinkInterfaces = { Navigation, NavigationHistoryEntry, NavigationTransition, NavigateEvent, NavigationCurrentEntryChangeEvent, NavigationDestination, NavigationActivation, NavigationPrecommitController };
Object.assign(globalThis, blinkInterfaces);
for (const key of Object.keys(blinkInterfaces)) Object.defineProperty(globalThis, key, { enumerable: false });
