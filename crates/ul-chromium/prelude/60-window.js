// ---- Window: location, history, navigator, storage, observers, XHR, fetch e o gancho de ciclo de vida.

const UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/154.0.0.0 Safari/537.36';
const pageState = { url: op('pageUrl') };

class Location {
  constructor() { throw new TypeError('Illegal constructor'); }
  get __u() { return new URL(pageState.url); }
  get href() { return pageState.url; }
  set href(v) { navigate(resolveUrl(v)); }
  get origin() { return this.__u.origin; }
  get protocol() { return this.__u.protocol; }
  set protocol(v) { const u = this.__u; u.protocol = v; navigate(u.href); }
  get host() { return this.__u.host; }
  set host(v) { const u = this.__u; u.host = v; navigate(u.href); }
  get hostname() { return this.__u.hostname; }
  set hostname(v) { const u = this.__u; u.hostname = v; navigate(u.href); }
  get port() { return this.__u.port; }
  set port(v) { const u = this.__u; u.port = v; navigate(u.href); }
  get pathname() { return this.__u.pathname; }
  set pathname(v) { const u = this.__u; u.pathname = v; navigate(u.href); }
  get search() { return this.__u.search; }
  set search(v) { const u = this.__u; u.search = v; navigate(u.href); }
  get hash() { return this.__u.hash; }
  set hash(v) { const u = this.__u; u.hash = v; navigate(u.href); }
  get ancestorOrigins() { return []; }
  assign(url) { navigate(resolveUrl(url)); }
  replace(url) { navigate(resolveUrl(url)); }
  reload() {}
  toString() { return pageState.url; }
}
tag(Location, 'Location');
const location = Object.create(Location.prototype);

// Muda só o fragmento: dispara hashchange. Qualquer outra mudança é navegação, que o hospedeiro executa.
function navigate(url) {
  const old = pageState.url;
  if (url === old) return;
  const strip = (u) => u.replace(/#.*$/, '');
  if (strip(url) === strip(old)) {
    pageState.url = url;
    setTimeout(() => { window.dispatchEvent(new HashChangeEvent('hashchange', { oldURL: old, newURL: url })); }, 0);
    return;
  }
  op('navigate', url);
}

class History {
  constructor() { throw new TypeError('Illegal constructor'); }
  get length() { return this.__entries.length; }
  get state() { return this.__entries[this.__index].state; }
  get scrollRestoration() { return this.__scroll || 'auto'; }
  set scrollRestoration(v) { this.__scroll = v; }
  __check(url) {
    if (url === undefined || url === null) return pageState.url;
    const next = resolveUrl(url);
    const a = new URL(next), b = new URL(pageState.url);
    if (a.origin !== b.origin && !(a.protocol === 'file:' && b.protocol === 'file:')) throw domException(`Failed to execute 'pushState' on 'History': A history state object with URL '${next}' cannot be created in a document with origin '${b.origin}' and URL '${pageState.url}'.`, 'SecurityError');
    return next;
  }
  pushState(state, title, url) { const next = this.__check(url); this.__entries.splice(this.__index + 1); this.__entries.push({ state, url: next }); this.__index++; pageState.url = next; }
  replaceState(state, title, url) { const next = this.__check(url); this.__entries[this.__index] = { state, url: next }; pageState.url = next; }
  go(delta) {
    const target = this.__index + (delta | 0);
    if (!delta || target < 0 || target >= this.__entries.length) return;
    this.__index = target;
    const entry = this.__entries[target];
    pageState.url = entry.url;
    setTimeout(() => window.dispatchEvent(new PopStateEvent('popstate', { state: entry.state })), 0);
  }
  back() { this.go(-1); }
  forward() { this.go(1); }
}
tag(History, 'History');
const history = Object.create(History.prototype);
hidden(history, '__entries', [{ state: null, url: pageState.url }]);
hidden(history, '__index', 0);

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
  get length() { return this.__data.size; }
  key(i) { return Array.from(this.__data.keys())[i] ?? null; }
  getItem(k) { const v = this.__data.get(String(k)); return v === undefined ? null : v; }
  setItem(k, v) { this.__data.set(String(k), String(v)); }
  removeItem(k) { this.__data.delete(String(k)); }
  clear() { this.__data.clear(); }
}
tag(Storage, 'Storage');
const makeStorage = () => {
  const target = Object.create(Storage.prototype);
  hidden(target, '__data', new Map());
  return new Proxy(target, {
    get(t, key, receiver) { if (typeof key === 'string' && !(key in t)) { const v = t.__data.get(key); return v === undefined ? undefined : v; } return Reflect.get(t, key, receiver); },
    set(t, key, value) { if (typeof key === 'string' && !(key in t)) { t.__data.set(key, String(value)); return true; } return Reflect.set(t, key, value, t); },
    deleteProperty(t, key) { t.__data.delete(key); return true; },
    has(t, key) { return Reflect.has(t, key) || (typeof key === 'string' && t.__data.has(key)); },
    ownKeys(t) { return Array.from(t.__data.keys()); },
    getOwnPropertyDescriptor(t, key) { return typeof key === 'string' && t.__data.has(key) ? { value: t.__data.get(key), writable: true, enumerable: true, configurable: true } : undefined; },
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
  get cssRules() { return Object.assign(this.__rules.slice(), { item(i) { return this[i] || null; } }); }
  get rules() { return this.cssRules; }
  insertRule(rule, index = 0) { this.__rules.splice(index, 0, { cssText: String(rule), selectorText: String(rule).split('{')[0].trim(), style: styleOfText(String(rule)) }); return index; }
  deleteRule(index) { this.__rules.splice(index, 1); }
  addRule(selector, style, index) { this.insertRule(`${selector} { ${style} }`, index); return -1; }
  removeRule(index) { this.deleteRule(index); }
  replaceSync(text) { this.__rules.length = 0; }
  replace(text) { this.replaceSync(text); return Promise.resolve(this); }
}
tag(CSSStyleSheet, 'CSSStyleSheet');
const styleOfText = (rule) => { const body = rule.slice(rule.indexOf('{') + 1, rule.lastIndexOf('}')); const holder = document.createElement('span'); holder.setAttribute('style', body); return holder.style; };
function sheetFor(owner) {
  if (!owner.__sheet) { const sheet = new CSSStyleSheet(); sheet.ownerNode = owner; sheet.href = owner.localName === 'link' ? owner.href : null; hidden(owner, '__sheet', sheet); }
  return owner.__sheet;
}
const fontFaceSet = Object.assign(Object.create(EventTarget.prototype), { ready: Promise.resolve(), status: 'loaded', size: 0, check: () => true, load: () => Promise.resolve([]), add() {}, delete() { return false; }, clear() {}, forEach() {}, onloading: null, onloadingdone: null, onloadingerror: null });

const defaultDisplay = (el) => {
  const t = el.localName;
  if (/^(div|p|ul|ol|li|h[1-6]|section|article|header|footer|nav|main|aside|form|table|pre|blockquote|body|html|dl|dt|dd|fieldset|figure|figcaption|hr|address|details|summary|dialog|menu|legend)$/.test(t)) return t === 'li' ? 'list-item' : t === 'table' ? 'table' : 'block';
  if (/^(head|script|style|link|meta|title|base|template|noscript|area|datalist|param|source|track)$/.test(t)) return 'none';
  return 'inline';
};
const getComputedStyle = (el) => {
  const inline = el.style;
  const target = Object.create(CSSStyleDeclaration.prototype);
  hidden(target, '__element', el);
  const read = (name) => { const v = inline.getPropertyValue(name); if (v) return v; if (name === 'display') return el.hidden ? 'none' : defaultDisplay(el); if (name === 'visibility') return 'visible'; if (name === 'position') return 'static'; if (name === 'opacity') return '1'; return ''; };
  return new Proxy(target, {
    get(t, key, receiver) {
      if (key === 'getPropertyValue') return (n) => read(String(n).toLowerCase());
      if (typeof key === 'string' && !(key in t)) return read(kebab(key));
      return Reflect.get(t, key, receiver);
    },
    set() { return true; },
  });
};

const matchMedia = (query) => {
  const q = String(query).toLowerCase();
  const evalOne = (c) => {
    let m;
    if ((m = /\(\s*min-width\s*:\s*(\d+(?:\.\d+)?)px\s*\)/.exec(c))) return 800 >= +m[1];
    if ((m = /\(\s*max-width\s*:\s*(\d+(?:\.\d+)?)px\s*\)/.exec(c))) return 800 <= +m[1];
    if (/prefers-color-scheme\s*:\s*light/.test(c)) return true;
    if (/prefers-color-scheme\s*:\s*dark/.test(c) || /prefers-reduced-motion\s*:\s*reduce/.test(c)) return false;
    if (/^\s*(all|screen)\s*$/.test(c)) return true;
    if (/print/.test(c)) return false;
    if (/hover\s*:\s*hover|pointer\s*:\s*fine/.test(c)) return true;
    return false;
  };
  const matches = q.split(',').some((part) => part.split(/\band\b/).every((c) => evalOne(c)));
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
    const existing = this.__regs.find((r) => r.target === target);
    if (existing) existing.opts = opts; else this.__regs.push({ target, opts });
    observers.add(this);
  }
  disconnect() { this.__regs.length = 0; this.__queue.length = 0; observers.delete(this); }
  takeRecords() { return this.__queue.splice(0); }
}
tag(MutationObserver, 'MutationObserver');
mutationHook.record = (rec) => {
  if (!observers.size) return;
  for (const observer of observers) {
    for (const reg of observer.__regs) {
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
      observer.__queue.push(record);
      break;
    }
  }
  if (!deliveryScheduled) {
    deliveryScheduled = true;
    queueMicrotask(() => {
      deliveryScheduled = false;
      for (const observer of Array.from(observers)) {
        const records = observer.__queue.splice(0);
        if (records.length) { try { observer.__callback.call(observer, records, observer); } catch (error) { reportError(error); } }
      }
    });
  }
};
class ResizeObserver { constructor(cb) { hidden(this, '__cb', cb); } observe() {} unobserve() {} disconnect() {} }
tag(ResizeObserver, 'ResizeObserver');
class IntersectionObserver { constructor(cb, options) { hidden(this, '__cb', cb); this.root = (options && options.root) || null; this.rootMargin = (options && options.rootMargin) || '0px'; this.thresholds = [0]; } observe() {} unobserve() {} disconnect() {} takeRecords() { return []; } }
tag(IntersectionObserver, 'IntersectionObserver');
class PerformanceObserver { constructor(cb) { hidden(this, '__cb', cb); } observe() {} disconnect() {} takeRecords() { return []; } static get supportedEntryTypes() { return []; } }
tag(PerformanceObserver, 'PerformanceObserver');

// ---- Rede: fetch e XMLHttpRequest sobre a operação síncrona `fetch` do hospedeiro, executada num timer.
const netFetch = (method, url, headers, body) => op('fetch', method, url, headers, body);
const headerList = (list) => { const out = []; for (let i = 0; i < list.length; i += 2) out.push([list[i], list[i + 1]]); return out; };

class XMLHttpRequest extends EventTarget {
  constructor() {
    super();
    hidden(this, '__headers', []);
    hidden(this, '__state', { method: 'GET', url: '', async: true, sent: false, aborted: false });
    this.readyState = 0;
    this.status = 0;
    this.statusText = '';
    this.responseType = '';
    this.responseURL = '';
    this.timeout = 0;
    this.withCredentials = false;
    this.upload = Object.create(EventTarget.prototype);
    this.__response = null;
  }
  get response() {
    const r = this.__response;
    if (!r) return this.responseType === '' || this.responseType === 'text' ? '' : null;
    if (this.responseType === 'json') { try { return JSON.parse(r.text); } catch { return null; } }
    if (this.responseType === 'arraybuffer') return new TextEncoder().encode(r.text).buffer;
    return r.text;
  }
  get responseText() { if (this.responseType !== '' && this.responseType !== 'text') throw domException("Failed to read the 'responseText' property from 'XMLHttpRequest': The value is only accessible if the object's 'responseType' is '' or 'text' (was '" + this.responseType + "').", 'InvalidStateError'); return this.__response ? this.__response.text : ''; }
  get responseXML() { return null; }
  __setState(n) { this.readyState = n; this.dispatchEvent(new Event('readystatechange')); }
  open(method, url, async = true) {
    const s = this.__state;
    s.method = String(method).toUpperCase(); s.url = resolveUrl(url); s.async = async !== false; s.sent = false; s.aborted = false;
    this.__headers.length = 0;
    this.__response = null;
    this.__setState(1);
  }
  setRequestHeader(name, value) { this.__headers.push([String(name), String(value)]); }
  overrideMimeType() {}
  getResponseHeader(name) { const r = this.__response; if (!r) return null; const found = r.headers.filter(([k]) => k.toLowerCase() === lower(name)).map(([, v]) => v); return found.length ? found.join(', ') : null; }
  getAllResponseHeaders() { const r = this.__response; return r ? r.headers.map(([k, v]) => `${k.toLowerCase()}: ${v}`).join('\r\n') + (r.headers.length ? '\r\n' : '') : ''; }
  abort() { this.__state.aborted = true; if (this.readyState > 1 && this.readyState < 4) { this.readyState = 4; this.dispatchEvent(new Event('readystatechange')); this.dispatchEvent(new ProgressEvent('abort')); this.dispatchEvent(new ProgressEvent('loadend')); } this.readyState = 0; }
  send(body) {
    const s = this.__state;
    if (this.readyState !== 1) throw domException("Failed to execute 'send' on 'XMLHttpRequest': The object's state must be OPENED.", 'InvalidStateError');
    s.sent = true;
    const run = () => {
      if (s.aborted) return;
      const result = netFetch(s.method, s.url, this.__headers.flat(), body === undefined || body === null ? null : String(body));
      if (s.aborted) return;
      if (result === null) {
        this.__setState(4);
        this.dispatchEvent(new ProgressEvent('error'));
        this.dispatchEvent(new ProgressEvent('loadend'));
        return;
      }
      this.status = result[0]; this.statusText = result[1]; this.responseURL = result[2];
      this.__response = { headers: headerList(result[3]), text: result[4] };
      this.__setState(2);
      this.__setState(3);
      this.__setState(4);
      this.dispatchEvent(new ProgressEvent('load', { loaded: result[4].length, total: result[4].length }));
      this.dispatchEvent(new ProgressEvent('loadend', { loaded: result[4].length, total: result[4].length }));
    };
    if (s.async) setTimeout(run, 0); else run();
  }
}
tag(XMLHttpRequest, 'XMLHttpRequest');
Object.assign(XMLHttpRequest, { UNSENT: 0, OPENED: 1, HEADERS_RECEIVED: 2, LOADING: 3, DONE: 4 });
defineHandlerProperties(XMLHttpRequest.prototype, ['readystatechange', 'load', 'error', 'abort', 'loadend', 'loadstart', 'progress', 'timeout']);

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
  setTimeout(() => {
    if (init.signal && init.signal.aborted) return reject(domException('The user aborted a request.', 'AbortError'));
    const result = netFetch(method, url, headers, body);
    if (result === null) return reject(new TypeError('Failed to fetch'));
    const response = new Response(result[0] === 204 || result[0] === 205 || result[0] === 304 ? null : result[4], { status: result[0], statusText: result[1], headers: headerList(result[3]) });
    Object.defineProperty(response, 'url', { value: result[2], configurable: true });
    resolve(response);
  }, 0);
});

// ---- Scripts inseridos pela página.
const isClassic = (type) => type === '' || /^(text|application)\/(x-)?(java|ecma)script$/.test(type) || type === 'text/jscript' || type === 'text/livescript';
function execScript(el, code, url) {
  const previous = document.__currentScript;
  hidden(document, '__currentScript', el);
  try { evalGlobal(code, url); } catch (error) { reportError(error); } finally { hidden(document, '__currentScript', previous || null); }
}
function runDynamicScript(el) {
  if (!el.__fresh || el.__started) return;
  hidden(el, '__started', true);
  const type = lower(el.getAttribute('type') || '').trim();
  if (!isClassic(type)) return;
  const src = el.getAttribute('src');
  if (src !== null && src !== '') {
    const url = resolveUrl(src);
    setTimeout(() => {
      const result = netFetch('GET', url, [], null);
      if (result === null || result[0] >= 400) { el.dispatchEvent(new Event('error')); return; }
      execScript(el, result[4], result[2]);
      el.dispatchEvent(new Event('load'));
    }, 0);
    return;
  }
  execScript(el, el.textContent, pageState.url);
}

// ---- Gancho de ciclo de vida, chamado pelo hospedeiro.
const hook = (name, a, b) => {
  switch (name) {
    case 'interactive': {
      hidden(document, '__readyState', 'interactive');
      document.dispatchEvent(new Event('readystatechange'));
      document.dispatchEvent(new Event('DOMContentLoaded', { bubbles: true }));
      setTimeout(() => hook('complete'), 0);
      break;
    }
    case 'complete': {
      hidden(document, '__readyState', 'complete');
      document.dispatchEvent(new Event('readystatechange'));
      const load = new Event('load');
      hidden(load, '__targetOverride', document);
      window.dispatchEvent(load);
      window.dispatchEvent(new PageTransitionEvent('pageshow', { persisted: false }));
      break;
    }
    case 'fire': { const el = wrappers.get(a) || wrap(a); el.dispatchEvent(new Event(b)); break; }
    case 'scriptError': {
      const event = new ErrorEvent('error', { message: 'Uncaught ' + a, filename: b || '', cancelable: true });
      if (window.dispatchEvent(event)) console.error('Uncaught ' + a);
      break;
    }
    case 'currentScript': hidden(document, '__currentScript', a === null ? null : wrappers.get(a) || null); break;
    case 'url': pageState.url = a; break;
    default: break;
  }
};
