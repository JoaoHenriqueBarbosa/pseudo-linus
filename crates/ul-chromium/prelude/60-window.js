// ---- Window: location, history, navigator, storage, observers, XHR, fetch e o gancho de ciclo de vida.

const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/154.0.0.0 Safari/537.36';
// A ida e volta de rede em tempo virtual (o NETWORK_LATENCY_MS do hospedeiro): o que já está na fila roda antes.
// Maior que o 1ms a que o setTimeout(0) é arredondado, para a resposta nunca empatar com uma tarefa já agendada.
const NETWORK_LATENCY_MS = 2;
// A ida e volta da busca do ícone da aba, que o navegador faz fora da página depois do `load`.
const FAVICON_ROUND_TRIP_MS = 50;
// Buscas em voo. Sob --virtual-time-budget o relógio virtual para enquanto a página espera a rede, e os quadros
// continuam saindo nesse meio tempo: com rede em voo, o quadro pedido sai sem esperar a grade.
let networkInFlight = 0;
// Buscas que seguram o `load` da janela (script inserido, folha de estilo, imagem), e se o `load` já espera por elas.
let loadBlockers = 0;
let loadWaiting = false;
const completeWhenUnblocked = () => {
  if (!loadWaiting || loadBlockers > 0) return;
  loadWaiting = false;
  setTimeout(() => hook('complete'), 0);
};
const afterNetwork = (fn, delaysLoad = false) => {
  networkInFlight++;
  if (delaysLoad) loadBlockers++;
  // A tarefa da resposta ainda conta como rede: o quadro que ela pede sai antes da página ficar ociosa. Enquanto a
  // rede não responde, o relógio virtual está parado e a página está ociosa de verdade: o quadro pedido e os idle
  // callbacks pendentes saem antes da resposta (medido na wikipedia: o rIC de 94ms roda aos 106, antes dos scripts).
  const deliver = () => {
    try { fn(); } finally {
      networkInFlight--;
      if (delaysLoad) { loadBlockers--; completeWhenUnblocked(); }
    }
  };
  setTimeout(() => {
    if (frameTimer === null && !idleCallbacks.size) { deliver(); return; }
    if (frameTimer !== null) { clearTimeout(frameTimer); renderFrame(); }
    setTimeout(deliver, 0);
  }, NETWORK_LATENCY_MS);
};
// A janela do headless medida no oráculo (o WINDOW do hospedeiro): conteúdo 780x493, janela 780x580, tela 800x600.
const VIEWPORT = Object.freeze({ width: 780, height: 493, outerWidth: 780, outerHeight: 580 });
const pageState = { url: op('pageUrl') };

class Location {
  constructor() { throw new TypeError('Illegal constructor'); }
  get [P.__u]() { return new URL(pageState.url); }
  get href() { return pageState.url; }
  set href(v) { navigate(resolveUrl(v)); }
  get origin() { return this[P.__u].origin; }
  get protocol() { return this[P.__u].protocol; }
  set protocol(v) { const u = this[P.__u]; u.protocol = v; navigate(u.href); }
  get host() { return this[P.__u].host; }
  set host(v) { const u = this[P.__u]; u.host = v; navigate(u.href); }
  get hostname() { return this[P.__u].hostname; }
  set hostname(v) { const u = this[P.__u]; u.hostname = v; navigate(u.href); }
  get port() { return this[P.__u].port; }
  set port(v) { const u = this[P.__u]; u.port = v; navigate(u.href); }
  get pathname() { return this[P.__u].pathname; }
  set pathname(v) { const u = this[P.__u]; u.pathname = v; navigate(u.href); }
  get search() { return this[P.__u].search; }
  set search(v) { const u = this[P.__u]; u.search = v; navigate(u.href); }
  get hash() { return this[P.__u].hash; }
  set hash(v) { const u = this[P.__u]; u.hash = v; navigate(u.href); }
  get ancestorOrigins() { return []; }
  assign(url) { navigate(resolveUrl(url)); }
  replace(url) { navigate(resolveUrl(url)); }
  reload() {}
  toString() { return pageState.url; }
}
tag(Location, 'Location');
const location = Object.create(Location.prototype);

// Muda só o fragmento: cria entrada no histórico, dispara popstate na hora e hashchange numa tarefa. Qualquer outra
// mudança é navegação, que o hospedeiro executa.
function navigate(url) {
  const old = pageState.url;
  if (url === old) return;
  if (sameDocument(url, old)) {
    history[P.__entries].splice(history[P.__index] + 1);
    history[P.__entries].push({ state: null, url });
    history[P.__index]++;
    pageState.url = url;
    window.dispatchEvent(new PopStateEvent('popstate', { state: null }));
    setTimeout(() => { window.dispatchEvent(new HashChangeEvent('hashchange', { oldURL: old, newURL: url })); }, 0);
    return;
  }
  op('navigate', url);
}
const sameDocument = (a, b) => a.replace(/#.*$/, '') === b.replace(/#.*$/, '');

class History {
  constructor() { throw new TypeError('Illegal constructor'); }
  get length() { return this[P.__entries].length; }
  get state() { return this[P.__entries][this[P.__index]].state; }
  get scrollRestoration() { return this[P.__scroll] || 'auto'; }
  set scrollRestoration(v) { this[P.__scroll] = v; }
  [P.__check](url) {
    if (url === undefined || url === null) return pageState.url;
    const next = resolveUrl(url);
    const a = new URL(next), b = new URL(pageState.url);
    if (a.origin !== b.origin && !(a.protocol === 'file:' && b.protocol === 'file:')) throw domException(`Failed to execute 'pushState' on 'History': A history state object with URL '${next}' cannot be created in a document with origin '${b.origin}' and URL '${pageState.url}'.`, 'SecurityError');
    return next;
  }
  pushState(state, title, url) { const next = this[P.__check](url); this[P.__entries].splice(this[P.__index] + 1); this[P.__entries].push({ state, url: next }); this[P.__index]++; pageState.url = next; }
  replaceState(state, title, url) { const next = this[P.__check](url); this[P.__entries][this[P.__index]] = { state, url: next }; pageState.url = next; }
  go(delta) {
    const target = this[P.__index] + (delta | 0);
    if (!delta || target < 0 || target >= this[P.__entries].length) return;
    const entry = this[P.__entries][target];
    // A primeira entrada é o about:blank de onde o headless partiu: voltar até ela é sair do documento.
    if (target === 0) { op('navigate', entry.url); return; }
    const old = pageState.url;
    this[P.__index] = target;
    pageState.url = entry.url;
    setTimeout(() => {
      window.dispatchEvent(new PopStateEvent('popstate', { state: entry.state }));
      if (old !== entry.url && sameDocument(old, entry.url)) window.dispatchEvent(new HashChangeEvent('hashchange', { oldURL: old, newURL: entry.url }));
    }, 0);
  }
  back() { this.go(-1); }
  forward() { this.go(1); }
}
tag(History, 'History');
const history = Object.create(History.prototype);
// O headless abre about:blank e só então navega para a URL pedida: o histórico começa com duas entradas.
hidden(history, '__entries', [{ state: null, url: 'about:blank' }, { state: null, url: pageState.url }]);
hidden(history, '__index', 1);

class Navigator {
  constructor() { throw new TypeError('Illegal constructor'); }
  get userAgent() { return UA; }
  get appVersion() { return UA.slice(8); }
  get appName() { return 'Netscape'; }
  get appCodeName() { return 'Mozilla'; }
  get product() { return 'Gecko'; }
  get productSub() { return '20030107'; }
  get vendor() { return 'Google Inc.'; }
  get vendorSub() { return ''; }
  get platform() { return 'Linux x86_64'; }
  get language() { return 'en-US'; }
  get languages() { return Object.freeze(['en-US']); }
  get onLine() { return true; }
  get cookieEnabled() { return true; }
  get webdriver() { return false; }
  get hardwareConcurrency() { return 8; }
  get deviceMemory() { return 8; }
  get maxTouchPoints() { return 0; }
  get doNotTrack() { return null; }
  get pdfViewerEnabled() { return true; }
  get plugins() { return []; }
  get mimeTypes() { return []; }
  get userActivation() { return { hasBeenActive: false, isActive: false }; }
  get connection() { return { effectiveType: '4g', rtt: 50, downlink: 10, saveData: false, addEventListener() {}, removeEventListener() {} }; }
  get userAgentData() { return { brands: [{ brand: 'Chromium', version: '154' }], mobile: false, platform: 'Linux', getHighEntropyValues: () => Promise.resolve({}), toJSON() { return { brands: this.brands, mobile: false, platform: 'Linux' }; } }; }
  get clipboard() { return { writeText: () => Promise.reject(domException('Write permission denied.', 'NotAllowedError')), readText: () => Promise.reject(domException('Read permission denied.', 'NotAllowedError')) }; }
  get permissions() { return { query: () => Promise.resolve({ state: 'prompt', onchange: null, addEventListener() {}, removeEventListener() {} }) }; }
  get serviceWorker() { return { register: () => Promise.reject(new TypeError('Failed to register a ServiceWorker: The document is in an invalid state.')), getRegistrations: () => Promise.resolve([]), ready: new Promise(() => {}), controller: null, addEventListener() {}, removeEventListener() {} }; }
  get geolocation() { return { getCurrentPosition(_, error) { if (error) setTimeout(() => error({ code: 1, message: 'User denied Geolocation' }), 0); }, watchPosition() { return 0; }, clearWatch() {} }; }
  sendBeacon() { return true; }
  javaEnabled() { return false; }
  vibrate() { return false; }
  getGamepads() { return []; }
  share() { return Promise.reject(domException('Share is not supported.', 'NotAllowedError')); }
}
tag(Navigator, 'Navigator');
const navigator = Object.create(Navigator.prototype);

class Storage {
  constructor() { throw new TypeError('Illegal constructor'); }
  get length() { return this[P.__data].size; }
  key(i) { return Array.from(this[P.__data].keys())[i] ?? null; }
  getItem(k) { const v = this[P.__data].get(String(k)); return v === undefined ? null : v; }
  setItem(k, v) { this[P.__data].set(String(k), String(v)); }
  removeItem(k) { this[P.__data].delete(String(k)); }
  clear() { this[P.__data].clear(); }
}
tag(Storage, 'Storage');
const makeStorage = () => {
  const target = Object.create(Storage.prototype);
  hidden(target, '__data', new Map());
  return proxied(target, {
    get(t, key, receiver) { if (typeof key === 'string' && !(key in t)) { const v = t[P.__data].get(key); return v === undefined ? undefined : v; } return Reflect.get(t, key, receiver); },
    set(t, key, value) { if (typeof key === 'string' && !(key in t)) { t[P.__data].set(key, String(value)); return true; } return Reflect.set(t, key, value, t); },
    deleteProperty(t, key) { t[P.__data].delete(key); return true; },
    has(t, key) { return Reflect.has(t, key) || (typeof key === 'string' && t[P.__data].has(key)); },
    ownKeys(t) { return Array.from(t[P.__data].keys()); },
    getOwnPropertyDescriptor(t, key) { return typeof key === 'string' && t[P.__data].has(key) ? { value: t[P.__data].get(key), writable: true, enumerable: true, configurable: true } : undefined; },
  });
};

// ---- CSS
const cssEscape = (value) => {
  const s = String(value);
  let out = '';
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c === 0) out += '�';
    else if ((c >= 1 && c <= 0x1f) || c === 0x7f || (i === 0 && c >= 48 && c <= 57) || (i === 1 && c >= 48 && c <= 57 && s.charCodeAt(0) === 45)) out += '\\' + c.toString(16) + ' ';
    else if (i === 0 && c === 45 && s.length === 1) out += '\\' + s[i];
    else if (c >= 0x80 || c === 45 || c === 95 || (c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122)) out += s[i];
    else out += '\\' + s[i];
  }
  return out;
};
const CSS = { escape: cssEscape, supports: () => true, px: (n) => ({ value: n, unit: 'px' }) };

class CSSStyleSheet {
  constructor(options) { hidden(this, '__rules', []); this.media = { mediaText: '' }; this.disabled = false; this.ownerNode = null; this.href = null; this.type = 'text/css'; this.parentStyleSheet = null; this.ownerRule = null; this.title = null; }
  get cssRules() { return Object.assign(this[P.__rules].slice(), { item(i) { return this[i] || null; } }); }
  get rules() { return this.cssRules; }
  insertRule(rule, index = 0) { this[P.__rules].splice(index, 0, { cssText: String(rule), selectorText: String(rule).split('{')[0].trim(), style: styleOfText(String(rule)) }); return index; }
  deleteRule(index) { this[P.__rules].splice(index, 1); }
  addRule(selector, style, index) { this.insertRule(`${selector} { ${style} }`, index); return -1; }
  removeRule(index) { this.deleteRule(index); }
  replaceSync(text) { this[P.__rules].length = 0; }
  replace(text) { this.replaceSync(text); return Promise.resolve(this); }
}
tag(CSSStyleSheet, 'CSSStyleSheet');
const styleOfText = (rule) => { const body = rule.slice(rule.indexOf('{') + 1, rule.lastIndexOf('}')); const holder = document.createElement('span'); holder.setAttribute('style', body); return holder.style; };
function sheetFor(owner) {
  if (!owner[P.__sheet]) { const sheet = new CSSStyleSheet(); sheet.ownerNode = owner; sheet.href = owner.localName === 'link' ? owner.href : null; hidden(owner, '__sheet', sheet); }
  return owner[P.__sheet];
}
const fontFaceSet = Object.assign(Object.create(EventTarget.prototype), { ready: Promise.resolve(), status: 'loaded', size: 0, check: () => true, load: () => Promise.resolve([]), add() {}, delete() { return false; }, clear() {}, forEach() {}, onloading: null, onloadingdone: null, onloadingerror: null });

// O estilo computado é o do stylo; `width` e `height` de elemento com caixa saem no valor usado do layout (o valor
// resolvido do CSSOM), na caixa que o `box-sizing` mede.
const resolvedSize = (el, name) => {
  const b = boxOf(el);
  if (!b || op('computedStyle', idOf(el), 'display') === 'inline') return null;
  const borderBox = op('computedStyle', idOf(el), 'box-sizing') === 'border-box';
  const [top, right, bottom, left] = [0, 1, 2, 3].map((i) => b.border[i] + b.padding[i]);
  const size = name === 'width' ? b.width - (borderBox ? 0 : left + right) : b.height - (borderBox ? 0 : top + bottom);
  return `${+size.toFixed(4)}px`;
};
const getComputedStyle = (el) => {
  const target = Object.create(CSSStyleDeclaration.prototype);
  hidden(target, '__element', el);
  const read = (name) => {
    if ((name === 'width' || name === 'height') && el.isConnected) { const size = resolvedSize(el, name); if (size !== null) return size; }
    return op('computedStyle', idOf(el), name) ?? '';
  };
  return proxied(target, {
    get(t, key, receiver) {
      if (key === 'getPropertyValue') return (n) => { const name = String(n); return read(name.startsWith('--') ? name : name.toLowerCase()); };
      if (typeof key === 'string' && !(key in t)) return read(kebab(key));
      return Reflect.get(t, key, receiver);
    },
    set() { return true; },
  });
};

const matchMedia = (query) => {
  const matches = op('matchMedia', String(query));
  return Object.assign(Object.create(EventTarget.prototype), { matches, media: String(query), onchange: null, addListener() {}, removeListener() {} });
};

// ---- Observers
const observers = new Set();
let deliveryScheduled = false;
class MutationRecord {}
tag(MutationRecord, 'MutationRecord');
class MutationObserver {
  constructor(callback) {
    if (typeof callback !== 'function') throw new TypeError("Failed to construct 'MutationObserver': The callback provided as parameter 1 is not a function.");
    hidden(this, '__callback', callback);
    hidden(this, '__regs', []);
    hidden(this, '__queue', []);
  }
  observe(target, options = {}) {
    if (!isNode(target)) throw new TypeError("Failed to execute 'observe' on 'MutationObserver': parameter 1 is not of type 'Node'.");
    const opts = { childList: !!options.childList, subtree: !!options.subtree, attributes: options.attributes, characterData: options.characterData, attributeOldValue: !!options.attributeOldValue, characterDataOldValue: !!options.characterDataOldValue, attributeFilter: options.attributeFilter };
    if (opts.attributes === undefined && (opts.attributeOldValue || opts.attributeFilter)) opts.attributes = true;
    if (opts.characterData === undefined && opts.characterDataOldValue) opts.characterData = true;
    if (!opts.childList && !opts.attributes && !opts.characterData) throw new TypeError("Failed to execute 'observe' on 'MutationObserver': The options object must set at least one of 'attributes', 'characterData', or 'childList' to true.");
    const existing = this[P.__regs].find((r) => r.target === target);
    if (existing) existing.opts = opts; else this[P.__regs].push({ target, opts });
    observers.add(this);
  }
  disconnect() { this[P.__regs].length = 0; this[P.__queue].length = 0; observers.delete(this); }
  takeRecords() { return this[P.__queue].splice(0); }
}
tag(MutationObserver, 'MutationObserver');
mutationHook.record = (rec) => {
  // Toda mutação pode mudar o layout: os observadores de tamanho e interseção medem de novo no próximo quadro.
  if (renderObservers.size) requestRender();
  if (!observers.size) return;
  for (const observer of observers) {
    for (const reg of observer[P.__regs]) {
      if (!(rec.target === reg.target || (reg.opts.subtree && reg.target.contains(rec.target)))) continue;
      const o = reg.opts;
      if (rec.type === 'childList' && !o.childList) continue;
      if (rec.type === 'attributes' && (!o.attributes || (o.attributeFilter && !o.attributeFilter.includes(rec.attributeName)))) continue;
      if (rec.type === 'characterData' && !o.characterData) continue;
      const record = Object.create(MutationRecord.prototype);
      Object.assign(record, {
        type: rec.type, target: rec.target, addedNodes: staticNodeList(rec.addedNodes || []), removedNodes: staticNodeList(rec.removedNodes || []),
        previousSibling: rec.previousSibling || null, nextSibling: rec.nextSibling || null, attributeName: rec.attributeName ?? null, attributeNamespace: null,
        oldValue: (rec.type === 'attributes' && o.attributeOldValue) || (rec.type === 'characterData' && o.characterDataOldValue) ? rec.oldValue ?? null : null,
      });
      observer[P.__queue].push(record);
      break;
    }
  }
  if (!deliveryScheduled) {
    deliveryScheduled = true;
    queueMicrotask(() => {
      deliveryScheduled = false;
      for (const observer of Array.from(observers)) {
        const records = observer[P.__queue].splice(0);
        if (records.length) { try { observer[P.__callback].call(observer, records, observer); } catch (error) { reportError(error); } }
      }
    });
  }
};
// ---- Quadros: requestAnimationFrame, ResizeObserver, IntersectionObserver e idle callbacks, sobre o layout do
// hospedeiro. A janela é a raiz do IntersectionObserver e não rola. Medido no oráculo (sob --virtual-time-budget): o
// headless produz quadros a cada 16ms enquanto a página carrega, mais um logo depois do `load`, e nenhum depois dele;
// o rAF pedido depois desse último nunca roda e a mudança de layout não gera entrega nova. Em cada quadro os
// callbacks de rAF rodam antes das entregas de ResizeObserver, que vêm antes das de IntersectionObserver; o idle
// callback espera o quadro seguinte.
const renderObservers = new Set();
const frameCallbacks = new Map();
let nextFrameCallback = 1;
const idleCallbacks = new Map();
let frameTimer = null;
// O quadro agendado sai sem esperar a grade (pedido com rede em voo).
let frameImmediate = false;
let framesOver = false;
// Os quadros caem na grade do `BeginFrameArgs::DefaultInterval()` (16666 µs) do tempo virtual; com rede em voo, saem
// já (o relógio virtual está parado). O carimbo do quadro é o instante da grade, clampado como no Blink.
const FRAME_INTERVAL_MS = 16.666;
let frameAt = 0;
const requestRender = () => {
  if (framesOver && !networkInFlight) return;
  if (frameTimer !== null) {
    if (frameImmediate || !networkInFlight) return;
    clearTimeout(frameTimer);
  }
  frameImmediate = networkInFlight > 0;
  const now = performance.now();
  const tick = Math.floor(now / FRAME_INTERVAL_MS) + (frameImmediate ? 0 : 1);
  frameAt = tick * FRAME_INTERVAL_MS;
  frameTimer = setTimeout(renderFrame, frameImmediate ? 0 : Math.ceil(frameAt - now));
};
const renderFrame = () => {
  frameTimer = null;
  const requestedDuringNetwork = frameImmediate;
  frameImmediate = false;
  if (framesOver && !networkInFlight && !requestedDuringNetwork) return;
  if (document.readyState === 'complete') framesOver = true;
  const now = op('frameTime', frameAt);
  const callbacks = Array.from(frameCallbacks.values());
  frameCallbacks.clear();
  for (const cb of callbacks) { try { cb.call(window, now); } catch (error) { reportError(error); } }
  const all = Array.from(renderObservers);
  for (const observer of all) if (observer instanceof ResizeObserver) observer[P.__measure]();
  for (const observer of all) if (!(observer instanceof ResizeObserver)) observer[P.__measure]();
  startLazyImages();
  for (const id of Array.from(idleCallbacks.keys())) setTimeout(() => runIdle(id), 0);
  if (frameCallbacks.size) requestRender();
};
const requestAnimationFrame = (cb) => {
  const id = nextFrameCallback++;
  frameCallbacks.set(id, cb);
  requestRender();
  return id;
};
const cancelAnimationFrame = (id) => { frameCallbacks.delete(id); };
const requestIdleCallback = (cb) => {
  const id = nextFrameCallback++;
  idleCallbacks.set(id, cb);
  if (framesOver && !networkInFlight) setTimeout(() => runIdle(id), 1); else requestRender();
  return id;
};
const cancelIdleCallback = (id) => { idleCallbacks.delete(id); };
const runIdle = (id) => {
  const cb = idleCallbacks.get(id);
  if (!cb) return;
  idleCallbacks.delete(id);
  cb.call(window, { didTimeout: false, timeRemaining: () => 50 });
};
const sizeEntry = (inlineSize, blockSize) => Object.freeze([Object.freeze({ inlineSize, blockSize })]);
class ResizeObserverEntry {
  constructor() { throw new TypeError('Illegal constructor'); }
}
tag(ResizeObserverEntry, 'ResizeObserverEntry');
class ResizeObserver {
  constructor(cb) {
    if (typeof cb !== 'function') throw new TypeError("Failed to construct 'ResizeObserver': parameter 1 is not of type 'ResizeObserverCallback'.");
    hidden(this, '__cb', cb);
    hidden(this, '__targets', new Map());
  }
  observe(target) {
    if (!this[P.__targets].has(target)) this[P.__targets].set(target, null);
    renderObservers.add(this);
    requestRender();
  }
  unobserve(target) { this[P.__targets].delete(target); }
  disconnect() { this[P.__targets].clear(); renderObservers.delete(this); }
  [P.__measure]() {
    const entries = [];
    for (const [target, last] of this[P.__targets]) {
      const b = boxOf(target);
      const width = b ? b.width - b.border[1] - b.border[3] - b.padding[1] - b.padding[3] : 0;
      const height = b ? b.height - b.border[0] - b.border[2] - b.padding[0] - b.padding[2] : 0;
      // A primeira observação de um alvo de tamanho zero não entra: o tamanho registrado começa em 0x0.
      const previous = last || { width: 0, height: 0, first: true };
      if (previous.width === width && previous.height === height && !(previous.first && b)) continue;
      this[P.__targets].set(target, { width, height });
      const entry = Object.create(ResizeObserverEntry.prototype);
      Object.defineProperties(entry, {
        target: { value: target, enumerable: true },
        contentRect: { value: new DOMRect(b ? b.padding[3] : 0, b ? b.padding[0] : 0, width, height), enumerable: true },
        borderBoxSize: { value: sizeEntry(b ? b.width : 0, b ? b.height : 0), enumerable: true },
        contentBoxSize: { value: sizeEntry(width, height), enumerable: true },
        devicePixelContentBoxSize: { value: sizeEntry(width, height), enumerable: true },
      });
      entries.push(entry);
    }
    if (entries.length) invokeReaction(() => this[P.__cb].call(this, entries, this));
  }
}
tag(ResizeObserver, 'ResizeObserver');
class IntersectionObserverEntry {
  constructor() { throw new TypeError('Illegal constructor'); }
}
tag(IntersectionObserverEntry, 'IntersectionObserverEntry');
class IntersectionObserver {
  constructor(cb, options) {
    if (typeof cb !== 'function') throw new TypeError("Failed to construct 'IntersectionObserver': parameter 1 is not of type 'IntersectionObserverCallback'.");
    hidden(this, '__cb', cb);
    hidden(this, '__targets', new Map());
    const threshold = options && options.threshold !== undefined ? options.threshold : 0;
    const list = (Array.isArray(threshold) ? threshold : [threshold]).map(Number);
    if (list.some((t) => !(t >= 0 && t <= 1))) throw new RangeError("Failed to construct 'IntersectionObserver': Threshold values must be numbers between 0 and 1");
    Object.defineProperties(this, {
      root: { value: (options && options.root) || null, enumerable: true },
      rootMargin: { value: (options && options.rootMargin) || '0px 0px 0px 0px', enumerable: true },
      thresholds: { value: Object.freeze(list.sort((a, b) => a - b)), enumerable: true },
    });
  }
  observe(target) {
    if (!this[P.__targets].has(target)) this[P.__targets].set(target, -1);
    renderObservers.add(this);
    requestRender();
  }
  unobserve(target) { this[P.__targets].delete(target); }
  disconnect() { this[P.__targets].clear(); renderObservers.delete(this); }
  takeRecords() { return []; }
  [P.__measure]() {
    const entries = [];
    const root = { x: 0, y: 0, width: windowMetrics.innerWidth, height: windowMetrics.innerHeight };
    for (const [target, last] of this[P.__targets]) {
      // Sem caixa (`display: none`, desconectado) o alvo não intersecta, nem na borda da raiz.
      const box = boxOf(target);
      const b = box || { x: 0, y: 0, width: 0, height: 0 };
      const left = Math.max(b.x, root.x), top = Math.max(b.y, root.y);
      const right = Math.min(b.x + b.width, root.x + root.width), bottom = Math.min(b.y + b.height, root.y + root.height);
      const isIntersecting = Boolean(box) && target.isConnected && right >= left && bottom >= top;
      const area = b.width * b.height;
      const ratio = isIntersecting ? (area > 0 ? ((right - left) * (bottom - top)) / area : 1) : 0;
      // O índice do limiar cruzado: -1 é "ainda não medido", que sempre entrega.
      let index = isIntersecting ? this.thresholds.filter((t) => ratio >= t).length : 0;
      if (!isIntersecting) index = 0;
      const state = index * 2 + (isIntersecting ? 1 : 0);
      if (state === last) continue;
      this[P.__targets].set(target, state);
      const entry = Object.create(IntersectionObserverEntry.prototype);
      Object.defineProperties(entry, {
        time: { value: performance.now(), enumerable: true },
        rootBounds: { value: new DOMRect(root.x, root.y, root.width, root.height), enumerable: true },
        boundingClientRect: { value: new DOMRect(b.x, b.y, b.width, b.height), enumerable: true },
        intersectionRect: { value: isIntersecting ? new DOMRect(left, top, right - left, bottom - top) : new DOMRect(0, 0, 0, 0), enumerable: true },
        isIntersecting: { value: isIntersecting, enumerable: true },
        intersectionRatio: { value: ratio, enumerable: true },
        target: { value: target, enumerable: true },
        isVisible: { value: false, enumerable: true },
      });
      entries.push(entry);
    }
    if (entries.length) invokeReaction(() => this[P.__cb].call(this, entries, this));
  }
}
tag(IntersectionObserver, 'IntersectionObserver');
class PerformanceObserver { constructor(cb) { hidden(this, '__cb', cb); } observe() {} disconnect() {} takeRecords() { return []; } static get supportedEntryTypes() { return []; } }
tag(PerformanceObserver, 'PerformanceObserver');

// ---- Rede: fetch e XMLHttpRequest sobre a operação síncrona `fetch` do hospedeiro, executada num timer.
const netFetch = (method, url, headers, body) => op('fetch', method, url, headers, body);
// O corpo chega da ponte um byte por unidade de código.
const rawBytes = (latin1) => { const out = new Uint8Array(latin1.length); for (let i = 0; i < latin1.length; i++) out[i] = latin1.charCodeAt(i); return out; };
const decodeBody = (bytes, label) => { try { return new TextDecoder(label || 'utf-8').decode(bytes); } catch { return new TextDecoder('utf-8').decode(bytes); } };
const headerList = (list) => { const out = []; for (let i = 0; i < list.length; i += 2) out.push([list[i], list[i + 1]]); return out; };

class XMLHttpRequest extends EventTarget {
  constructor() {
    super();
    hidden(this, '__headers', []);
    hidden(this, '__state', { method: 'GET', url: '', async: true, sent: false, aborted: false });
    // Os atributos do XHR são acessores do protótipo, como no Chromium: o objeto não tem propriedade própria.
    hidden(this, '__fields', { readyState: 0, status: 0, statusText: '', responseType: '', responseURL: '', timeout: 0, withCredentials: false, upload: Object.create(EventTarget.prototype) });
    hidden(this, '__response', null);
  }
  get readyState() { return this[P.__fields].readyState; }
  get status() { return this[P.__fields].status; }
  get statusText() { return this[P.__fields].statusText; }
  get responseURL() { return this[P.__fields].responseURL; }
  get responseType() { return this[P.__fields].responseType; }
  set responseType(v) { const t = String(v); if (['', 'arraybuffer', 'blob', 'document', 'json', 'text'].includes(t)) this[P.__fields].responseType = t; }
  get timeout() { return this[P.__fields].timeout; }
  set timeout(v) { this[P.__fields].timeout = Math.max(0, Number(v) >>> 0); }
  get withCredentials() { return this[P.__fields].withCredentials; }
  set withCredentials(v) { this[P.__fields].withCredentials = !!v; }
  get upload() { return this[P.__fields].upload; }
  get response() {
    const r = this[P.__response];
    if (!r) return this.responseType === '' || this.responseType === 'text' ? '' : null;
    if (this.responseType === 'json') { try { return JSON.parse(r.text); } catch { return null; } }
    if (this.responseType === 'arraybuffer') return r.bytes.slice().buffer;
    if (this.responseType === 'blob') return new Blob([r.bytes], { type: this.getResponseHeader('content-type') || '' });
    return r.text;
  }
  get responseText() { if (this.responseType !== '' && this.responseType !== 'text') throw domException("Failed to read the 'responseText' property from 'XMLHttpRequest': The value is only accessible if the object's 'responseType' is '' or 'text' (was '" + this.responseType + "').", 'InvalidStateError'); return this[P.__response] ? this[P.__response].text : ''; }
  get responseXML() { return null; }
  [P.__setState](n) { this[P.__fields].readyState = n; this.dispatchEvent(new Event('readystatechange')); }
  open(method, url, async = true) {
    const s = this[P.__state];
    s.method = String(method).toUpperCase(); s.url = resolveUrl(url); s.async = async !== false; s.sent = false; s.aborted = false;
    this[P.__headers].length = 0;
    this[P.__response] = null;
    this[P.__setState](1);
  }
  setRequestHeader(name, value) { this[P.__headers].push([String(name), String(value)]); }
  overrideMimeType() {}
  getResponseHeader(name) { const r = this[P.__response]; if (!r) return null; const found = r.headers.filter(([k]) => k.toLowerCase() === lower(name)).map(([, v]) => v); return found.length ? found.join(', ') : null; }
  getAllResponseHeaders() { const r = this[P.__response]; return r ? r.headers.map(([k, v]) => `${k.toLowerCase()}: ${v}`).join('\r\n') + (r.headers.length ? '\r\n' : '') : ''; }
  abort() { const f = this[P.__fields]; this[P.__state].aborted = true; if (f.readyState > 1 && f.readyState < 4) { f.readyState = 4; this.dispatchEvent(new Event('readystatechange')); this.dispatchEvent(new ProgressEvent('abort')); this.dispatchEvent(new ProgressEvent('loadend')); } f.readyState = 0; }
  send(body) {
    const s = this[P.__state];
    if (this.readyState !== 1) throw domException("Failed to execute 'send' on 'XMLHttpRequest': The object's state must be OPENED.", 'InvalidStateError');
    s.sent = true;
    const run = () => {
      if (s.aborted) return;
      const result = netFetch(s.method, s.url, this[P.__headers].flat(), body === undefined || body === null ? null : String(body));
      if (s.aborted) return;
      if (result === null) {
        this[P.__setState](4);
        this.dispatchEvent(new ProgressEvent('error'));
        this.dispatchEvent(new ProgressEvent('loadend'));
        return;
      }
      Object.assign(this[P.__fields], { status: result[0], statusText: result[1], responseURL: result[2] });
      const bytes = rawBytes(result[4]);
      this[P.__response] = { headers: headerList(result[3]), bytes, text: decodeBody(bytes, result[5]) };
      this[P.__setState](2);
      this[P.__setState](3);
      this[P.__setState](4);
      this.dispatchEvent(new ProgressEvent('load', { lengthComputable: true, loaded: bytes.length, total: bytes.length }));
      this.dispatchEvent(new ProgressEvent('loadend', { lengthComputable: true, loaded: bytes.length, total: bytes.length }));
    };
    if (s.async) afterNetwork(run); else run();
  }
}
tag(XMLHttpRequest, 'XMLHttpRequest');
Object.assign(XMLHttpRequest, { UNSENT: 0, OPENED: 1, HEADERS_RECEIVED: 2, LOADING: 3, DONE: 4 });
defineHandlerProperties(XMLHttpRequest.prototype, ['readystatechange', 'load', 'error', 'abort', 'loadend', 'loadstart', 'progress', 'timeout']);

// O corpo de uma resposta da rede chega em outra tarefa: ler `json()`, `text()` e afins espera uma volta do laço,
// como no Chromium (o corpo vem do processo de rede depois dos cabeçalhos).
const networkBody = (bytes) => new ReadableStream({
  pull(controller) {
    return new Promise((resolve) => setTimeout(resolve, 0)).then(() => { controller.enqueue(bytes); controller.close(); });
  },
});

// As URLs `blob:` da página: `blob:<origem>/<uuid>` (origem `null` em file:), resolvidas por imagem e fetch.
const blobUrls = new Map();
const blobOrigin = () => { const origin = new URL(pageState.url).origin; return origin === 'null' || origin === 'file://' ? 'null' : origin; };
Object.defineProperty(URL, 'createObjectURL', { value: { createObjectURL(object) {
  if (!(object instanceof Blob)) throw new TypeError("Failed to execute 'createObjectURL' on 'URL': Overload resolution failed.");
  const url = `blob:${blobOrigin()}/${crypto.randomUUID()}`;
  blobUrls.set(url, object);
  return url;
} }.createObjectURL, writable: true, enumerable: true, configurable: true });
Object.defineProperty(URL, 'revokeObjectURL', { value: { revokeObjectURL(url) { blobUrls.delete(String(url)); } }.revokeObjectURL, writable: true, enumerable: true, configurable: true });
// O Request do motor não conhece o documento: o da janela resolve a URL relativa contra a base da página. É o mesmo
// protótipo, com o construtor trocado, para a cadeia de protótipos ser a do Chromium.
const EngineRequest = globalThis.Request;
const Request = function Request(input, ...init) {
  if (!new.target) throw new TypeError("Failed to construct 'Request': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
  return Reflect.construct(EngineRequest, [typeof input === 'string' || input instanceof URL ? resolveUrl(String(input)) : input, ...init], new.target);
};
Object.defineProperty(Request, 'prototype', { value: EngineRequest.prototype, writable: false, enumerable: false, configurable: false });
Object.defineProperty(EngineRequest.prototype, 'constructor', { value: Request, writable: true, configurable: true });
const realFetch = (input, init = {}) => new Promise((resolve, reject) => {
  const request = typeof input === 'object' && input !== null && !(input instanceof URL) ? input : null;
  const url = resolveUrl(request ? request.url : String(input));
  const method = String((init.method || (request && request.method) || 'GET')).toUpperCase();
  const headers = [];
  const addHeaders = (h) => { if (!h) return; if (typeof h.forEach === 'function' && !Array.isArray(h)) h.forEach((v, k) => headers.push(k, v)); else if (Array.isArray(h)) for (const [k, v] of h) headers.push(k, String(v)); else for (const k of Object.keys(h)) headers.push(k, String(h[k])); };
  addHeaders(request && request.headers);
  addHeaders(init.headers);
  const body = init.body === undefined || init.body === null ? null : String(init.body);
  if (init.signal && init.signal.aborted) return reject(domException('The user aborted a request.', 'AbortError'));
  if (url.startsWith('blob:')) {
    const blob = blobUrls.get(url.replace(/#.*$/, ''));
    afterNetwork(() => {
      if (!blob || method !== 'GET') return reject(new TypeError('Failed to fetch'));
      const response = new Response(blob, { status: 200, statusText: 'OK', headers: [['content-length', String(blob.size)], ['content-type', blob.type]] });
      Object.defineProperty(response, 'url', { value: url, configurable: true });
      Object.defineProperty(response, 'type', { value: 'basic', configurable: true });
      resolve(response);
    });
    return undefined;
  }
  afterNetwork(() => {
    if (init.signal && init.signal.aborted) return reject(domException('The user aborted a request.', 'AbortError'));
    const result = netFetch(method, url, headers, body);
    if (result === null) return reject(new TypeError('Failed to fetch'));
    const response = new Response(result[0] === 204 || result[0] === 205 || result[0] === 304 ? null : networkBody(rawBytes(result[4])), { status: result[0], statusText: result[1], headers: headerList(result[3]) });
    Object.defineProperty(response, 'url', { value: result[2], configurable: true });
    // A resposta filtrada: `basic` na mesma origem, `cors` na que passou pelo CORS.
    Object.defineProperty(response, 'type', { value: new URL(result[2]).origin === new URL(pageState.url).origin ? 'basic' : 'cors', configurable: true });
    Object.defineProperty(response, 'redirected', { value: result[2] !== url.replace(/#.*$/, ''), configurable: true });
    resolve(response);
  });
});

// ---- Scripts inseridos pela página.
const isClassic = (type) => type === '' || /^(text|application)\/(x-)?(java|ecma)script$/.test(type) || type === 'text/jscript' || type === 'text/livescript';
function execScript(el, code, url) {
  const previous = document[P.__currentScript];
  hidden(document, '__currentScript', el);
  op('chargeSource', code.length);
  try { evalGlobal(code, url); } catch (error) { reportError(error); } finally { hidden(document, '__currentScript', previous || null); }
}
function runDynamicScript(el) {
  if (!el[P.__fresh] || el[P.__started]) return;
  hidden(el, '__started', true);
  const type = lower(el.getAttribute('type') || '').trim();
  if (!isClassic(type)) return;
  const src = el.getAttribute('src');
  if (src !== null && src !== '') {
    const url = resolveUrl(src);
    afterNetwork(() => {
      const result = netFetch('GET', url, [], null);
      if (result === null || result[0] >= 400) { el.dispatchEvent(new Event('error')); return; }
      execScript(el, decodeBody(rawBytes(result[4]), result[5]), result[2]);
      el.dispatchEvent(new Event('load'));
    }, true);
    return;
  }
  execScript(el, el.textContent, pageState.url);
}

// ---- Gancho de ciclo de vida, chamado pelo hospedeiro.
const reportedRejections = new WeakSet();
const hook = (name, a, b) => {
  switch (name) {
    // As imagens do documento pedem a fonte quando o parser as cria (o preload scanner adianta as eager).
    case 'parsedImages': {
      for (const img of Array.from(document.getElementsByTagName('img'))) if (img.namespaceURI === HTML_NS && !img[P.__imageStarted]) updateImage(img);
      break;
    }
    case 'interactive': {
      hidden(document, '__readyState', 'interactive');
      document.dispatchEvent(new Event('readystatechange'));
      document.dispatchEvent(new Event('DOMContentLoaded', { bubbles: true }));
      devtoolsAgent.lifecycle('DOMContentLoaded');
      break;
    }
    // Os `async` do documento e as folhas dele chegaram: o `load` só espera o que a página pôs em voo.
    case 'subresourcesDone': {
      loadWaiting = true;
      completeWhenUnblocked();
      break;
    }
    case 'complete': {
      hidden(document, '__readyState', 'complete');
      document.dispatchEvent(new Event('readystatechange'));
      const load = new Event('load');
      hidden(load, '__targetOverride', document);
      window.dispatchEvent(load);
      window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: false }));
      devtoolsAgent.lifecycle('load');
      // Logo depois do `load` o navegador busca o ícone declarado (`link rel=icon`; sem ele, nenhuma busca aparece no
      // resource timing do oráculo). A busca fica em voo por FAVICON_ROUND_TRIP_MS, e enquanto isso os quadros pedidos
      // ainda saem (medido na wikipedia: o rAF pedido pelo mw.loader depois do `load` roda e os módulos ficam prontos).
      const icon = document.querySelector('link[rel~="icon" i][href]');
      if (icon) {
        const iconUrl = resolveUrl(icon.getAttribute('href'));
        networkInFlight++;
        setTimeout(() => { networkInFlight--; afterNetwork(() => { netFetch('GET', iconUrl, [], null); }); }, FAVICON_ROUND_TRIP_MS);
      }
      // O quadro que segue o `load` acontece mesmo sem pedido, e é o último.
      requestRender();
      break;
    }
    case 'fire': { const el = wrappers.get(a) || wrap(a); el.dispatchEvent(new Event(b)); break; }
    case 'scriptError': {
      const event = new ErrorEvent('error', { message: 'Uncaught ' + a, filename: b || '', cancelable: true });
      if (window.dispatchEvent(event)) console.error('Uncaught ' + a);
      break;
    }
    case 'currentScript': hidden(document, '__currentScript', a === null ? null : wrap(a)); break;
    case 'namedLookup': return namedValue;
    // O agente do DevTools (`65-devtools.js`): configuração do contexto, uma mensagem, e a caixa de saída.
    case 'cdpInit': devtoolsAgent.init(a); break;
    case 'cdp': devtoolsAgent.dispatch(a); break;
    case 'cdpDrain': return devtoolsAgent.drain();
    case 'url': pageState.url = a; break;
    // Rejeições sem tratador, do rastreador do motor. `handled` também chega para a promessa tratada no mesmo
    // checkpoint, que nunca foi relatada: só as relatadas ganham `rejectionhandled`.
    case 'unhandled': {
      reportedRejections.add(a);
      const event = new PromiseRejectionEvent('unhandledrejection', { promise: a, reason: b, cancelable: true });
      if (window.dispatchEvent(event)) console.error('Uncaught (in promise)', b);
      break;
    }
    case 'handled': {
      if (!reportedRejections.delete(a)) break;
      setTimeout(() => window.dispatchEvent(new PromiseRejectionEvent('rejectionhandled', { promise: a, reason: undefined })), 0);
      break;
    }
    default: break;
  }
};
