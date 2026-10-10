// ---- Exposição: o que a página enxerga como globais, e o gancho devolvido ao hospedeiro.

const window = globalThis;
// O que o motor traz do runtime de servidor (bun) não existe numa janela; `SharedArrayBuffer` o Chromium só expõe com
// isolamento cross-origin, que nenhuma página daqui pede.
for (const name of ['process', 'Bun', 'Buffer', 'global', 'BuildMessage', 'ResolveMessage', 'ShadowRealm', 'SharedArrayBuffer', 'require', 'module', 'exports', '__dirname', '__filename']) {
  if (Object.prototype.hasOwnProperty.call(globalThis, name) && !delete globalThis[name]) throw new Error('global não removível: ' + name);
}
const document = wrap(op('root'));
hidden(document, '__window', window);
hidden(document, '__readyState', 'loading');

const NodeFilter = { FILTER_ACCEPT: 1, FILTER_REJECT: 2, FILTER_SKIP: 3, SHOW_ALL: 0xffffffff, SHOW_ELEMENT: 1, SHOW_ATTRIBUTE: 2, SHOW_TEXT: 4, SHOW_CDATA_SECTION: 8, SHOW_PROCESSING_INSTRUCTION: 64, SHOW_COMMENT: 128, SHOW_DOCUMENT: 256, SHOW_DOCUMENT_TYPE: 512, SHOW_DOCUMENT_FRAGMENT: 1024 };

class DOMParser {
  parseFromString(text, type) {
    const mime = String(type);
    const supported = ['text/html', 'text/xml', 'application/xml', 'application/xhtml+xml', 'image/svg+xml'];
    if (!supported.includes(mime)) throw new TypeError(`Failed to execute 'parseFromString' on 'DOMParser': The provided value '${mime}' is not a valid enum value of type DOMParserSupportedType.`);
    return wrap(op('parseDocument', String(text), mime !== 'text/html'));
  }
}
tag(DOMParser, 'DOMParser');
class XMLSerializer {
  serializeToString(node) { return node.nodeType === 1 ? node.outerHTML : node.nodeType === 11 || node.nodeType === 9 ? Array.from(node.childNodes).map((c) => (c.nodeType === 1 ? c.outerHTML : c.textContent)).join('') : node.textContent; }
}
tag(XMLSerializer, 'XMLSerializer');

const windowGlobals = {
  window, self: window, globalThis: window, frames: window, top: window, parent: window, document, location, history, navigator, CSS, NodeFilter,
  screen: { width: 800, height: 600, availWidth: 800, availHeight: 600, availLeft: 0, availTop: 0, colorDepth: 24, pixelDepth: 24, orientation: { type: 'landscape-primary', angle: 0, addEventListener() {}, removeEventListener() {} } },
  localStorage: makeStorage(), sessionStorage: makeStorage(), customElements,
  getComputedStyle, matchMedia, fetch: realFetch, Request, XMLHttpRequest, MutationObserver, MutationRecord, ResizeObserver, ResizeObserverEntry, IntersectionObserver, IntersectionObserverEntry, TextTrack, TextTrackList, PerformanceObserver,
  WebGLRenderingContext, WebGL2RenderingContext, WebGLObject, WebGLBuffer, WebGLFramebuffer, WebGLProgram, WebGLQuery, WebGLRenderbuffer, WebGLSampler, WebGLShader, WebGLSync, WebGLTexture, WebGLTransformFeedback, WebGLVertexArrayObject, WebGLUniformLocation, WebGLActiveInfo, WebGLShaderPrecisionFormat, WebGLContextEvent,
  Node, Element, HTMLElement, HTMLUnknownElement, SVGElement, SVGGraphicsElement, SVGSVGElement, MathMLElement, Text, Comment, CDATASection, CharacterData, DocumentFragment, ShadowRoot, DocumentType, Document, Attr, NamedNodeMap, NodeList, HTMLCollection,
  DOMTokenList, CSSStyleDeclaration, CSSStyleSheet, DOMRect, DOMRectReadOnly, DOMPoint, DOMPointReadOnly, DOMQuad, DOMMatrix, DOMMatrixReadOnly, CanvasRenderingContext2D, Path2D, CanvasGradient, CanvasPattern, ImageData, TextMetrics, ImageBitmap, createImageBitmap, Range, Selection, TreeWalker, NodeIterator, DOMParser, XMLSerializer, DOMImplementation, XPathEvaluator, XPathExpression, XPathResult, SVGAnimatedString, Location, History, Navigator, Storage, CustomElementRegistry,
  EventTarget, Event, CustomEvent, UIEvent, MouseEvent, PointerEvent, WheelEvent, KeyboardEvent, FocusEvent, InputEvent, ErrorEvent, MessageEvent, HashChangeEvent, PopStateEvent, ProgressEvent, TouchEvent, AnimationEvent, TransitionEvent, PageTransitionEvent, SubmitEvent, PromiseRejectionEvent,
  HTMLDocument: Document,
};
for (const [key, value] of Object.entries(windowGlobals)) Object.defineProperty(globalThis, key, { value, writable: true, configurable: true, enumerable: key === 'document' || key === 'location' ? true : false });
Object.defineProperty(globalThis, 'document', { value: document, writable: false, configurable: false, enumerable: true });
Object.defineProperty(globalThis, 'location', { get: () => location, set: (v) => { location.href = v; }, configurable: false, enumerable: true });

// Acesso nomeado da janela (o WindowProperties do Blink): o motor pergunta o nome que a janela e a cadeia dela não
// têm. Acha o elemento de id `x`, ou de nome `x` entre embed, form, img e object; mais de um vira HTMLCollection viva.
const namedTargets = (name) => {
  if (typeof name !== 'string' || name === '') return [];
  const quoted = cssEscape(name);
  return Array.from(document.querySelectorAll(`[id="${quoted}"], embed[name="${quoted}"], form[name="${quoted}"], img[name="${quoted}"], object[name="${quoted}"]`));
};
const namedValue = (name) => {
  const found = namedTargets(name);
  if (found.length === 0) return undefined;
  return found.length === 1 ? found[0] : liveList(HTMLCollection.prototype, () => namedTargets(name));
};

const windowMetrics = { innerWidth: VIEWPORT.width, innerHeight: VIEWPORT.height, outerWidth: VIEWPORT.outerWidth, outerHeight: VIEWPORT.outerHeight, devicePixelRatio: 1, scrollX: 0, scrollY: 0, pageXOffset: 0, pageYOffset: 0, screenX: 0, screenY: 0, screenLeft: 0, screenTop: 0, length: 0, closed: false, opener: null, name: '', status: '', isSecureContext: pageState.url.startsWith('https:') || pageState.url.startsWith('http://localhost'), origin: location.origin === 'null' ? 'null' : location.origin, crossOriginIsolated: false, originAgentCluster: true };
for (const [key, value] of Object.entries(windowMetrics)) Object.defineProperty(globalThis, key, { value, writable: true, configurable: true, enumerable: true });

// Métodos de EventTarget na própria janela.
for (const key of ['addEventListener', 'removeEventListener', 'dispatchEvent']) Object.defineProperty(globalThis, key, { value: EventTarget.prototype[key], writable: true, configurable: true, enumerable: true });
// GlobalEventHandlers e, na janela, WindowEventHandlers (sem os de toque: o headless de desktop não tem tela de toque).
defineHandlerProperties(globalThis, [...GLOBAL_HANDLERS, 'afterprint', 'beforeprint', 'beforeunload', 'hashchange', 'languagechange', 'message', 'messageerror', 'offline', 'online', 'pagehide', 'pagereveal', 'pageshow', 'pageswap', 'popstate', 'rejectionhandled', 'storage', 'unhandledrejection', 'unload', 'devicemotion', 'deviceorientation', 'deviceorientationabsolute', 'appinstalled', 'beforeinstallprompt', 'search']);
for (const cls of [HTMLElement, SVGElement, MathMLElement]) defineHandlerProperties(cls.prototype, GLOBAL_HANDLERS);
defineHandlerProperties(Element.prototype, ['beforecopy', 'beforecut', 'beforepaste', 'fullscreenchange', 'fullscreenerror', 'search', 'webkitfullscreenchange', 'webkitfullscreenerror']);

// Timers do navegador: exceção num callback é relatada (evento `error` da janela) e o laço segue, ao contrário do bun,
// onde ela encerra o processo. Handler em texto é avaliado no escopo global, como `setTimeout('código')`.
const guarded = (handler) => {
  if (typeof handler !== 'function') {
    const code = String(handler);
    return () => { try { evalGlobal(code, pageState.url); } catch (error) { reportError(error); } };
  }
  return function (...args) { try { return handler.apply(this, args); } catch (error) { reportError(error); } };
};
const engineSetTimeout = setTimeout;
const engineSetInterval = setInterval;
const engineClearTimeout = clearTimeout;
const engineQueueMicrotask = queueMicrotask;
// O id do navegador é um inteiro positivo; o do motor é um objeto à moda do node, então guardamos o mapa.
const timerHandles = new Map();
let nextTimerId = 1;
const browserTimer = (engine) => function (handler, delay, ...args) {
  const id = nextTimerId++;
  const wrapped = guarded(handler);
  const once = engine === engineSetTimeout;
  const handle = engine(function () { if (once) timerHandles.delete(id); return wrapped.apply(window, arguments); }, Math.max(0, Number(delay) || 0), ...args);
  timerHandles.set(id, handle);
  return id;
};
const clearBrowserTimer = (id) => { const handle = timerHandles.get(id); if (handle !== undefined) { engineClearTimeout(handle); timerHandles.delete(id); } };
for (const [name, value] of [['setTimeout', browserTimer(engineSetTimeout)], ['setInterval', browserTimer(engineSetInterval)], ['clearTimeout', clearBrowserTimer], ['clearInterval', clearBrowserTimer], ['queueMicrotask', (cb) => engineQueueMicrotask(guarded(cb))]]) {
  Object.defineProperty(globalThis, name, { value, writable: true, configurable: true, enumerable: true });
}
for (const name of ['setImmediate', 'clearImmediate']) delete globalThis[name];

// Navigation Timing do navegador: `performance.timing`, `performance.navigation`, a entrada `navigation` e os nomes
// legados (`navigationStart`, `domContentLoadedEventEnd`...) aceitos como marcas por `performance.measure`. Os
// instantes são os do carregamento desta página, em ms desde a origem de tempo.
{
  const origin = Math.round(performance.timeOrigin);
  const marks = { navigationStart: 0, fetchStart: 1, domainLookupStart: 1, domainLookupEnd: 1, connectStart: 1, connectEnd: 1, secureConnectionStart: 0, requestStart: 2, responseStart: 3, responseEnd: 4, unloadEventStart: 0, unloadEventEnd: 0, redirectStart: 0, redirectEnd: 0, domLoading: 5 };
  const lifecycle = { domInteractive: null, domContentLoadedEventStart: null, domContentLoadedEventEnd: null, domComplete: null, loadEventStart: null, loadEventEnd: null };
  document.addEventListener('readystatechange', () => {
    const now = Math.round(performance.now());
    if (document.readyState === 'interactive') lifecycle.domInteractive = now;
    if (document.readyState === 'complete') lifecycle.domComplete = now;
  });
  document.addEventListener('DOMContentLoaded', () => { lifecycle.domContentLoadedEventStart = lifecycle.domContentLoadedEventEnd = Math.round(performance.now()); }, { capture: true });
  window.addEventListener('load', () => { lifecycle.loadEventStart = Math.round(performance.now()); setTimeout(() => { lifecycle.loadEventEnd = Math.round(performance.now()); }, 0); }, { capture: true });
  const relative = (name) => (name in marks ? marks[name] : name in lifecycle ? lifecycle[name] : undefined);
  class PerformanceTiming {
    constructor() { throw new TypeError('Illegal constructor'); }
    toJSON() { const out = {}; for (const k of [...Object.keys(marks), ...Object.keys(lifecycle)]) out[k] = this[k]; return out; }
  }
  for (const name of [...Object.keys(marks), ...Object.keys(lifecycle)]) {
    Object.defineProperty(PerformanceTiming.prototype, name, { get() { const v = relative(name); return v === null || (v === 0 && name !== 'navigationStart' && marks[name] === 0) ? 0 : origin + v; }, configurable: true, enumerable: true });
  }
  tag(PerformanceTiming, 'PerformanceTiming');
  const timing = Object.create(PerformanceTiming.prototype);
  class PerformanceNavigation {
    constructor() { throw new TypeError('Illegal constructor'); }
    get type() { return 0; }
    get redirectCount() { return 0; }
    toJSON() { return { type: 0, redirectCount: 0 }; }
  }
  Object.assign(PerformanceNavigation, { TYPE_NAVIGATE: 0, TYPE_RELOAD: 1, TYPE_BACK_FORWARD: 2, TYPE_RESERVED: 255 });
  tag(PerformanceNavigation, 'PerformanceNavigation');
  const navigation = Object.create(PerformanceNavigation.prototype);
  Object.defineProperty(performance, 'timing', { get: () => timing, configurable: true, enumerable: true });
  Object.defineProperty(performance, 'navigation', { get: () => navigation, configurable: true, enumerable: true });
  const engineMeasure = performance.measure.bind(performance);
  const resolveMark = (value) => { if (typeof value === 'string') { const v = relative(value); if (v !== undefined) { if (v === null) throw domException(`Failed to execute 'measure' on 'Performance': '${value}' is empty: either the event hasn't happened yet, or it would provide cross-origin timing information.`, 'InvalidAccessError'); return v; } } return value; };
  Object.defineProperty(performance, 'measure', {
    value: function measure(name, startOrOptions, endMark) {
      if (startOrOptions && typeof startOrOptions === 'object') return engineMeasure(name, { ...startOrOptions, start: resolveMark(startOrOptions.start), end: resolveMark(startOrOptions.end) });
      const start = resolveMark(startOrOptions);
      const end = resolveMark(endMark);
      if (typeof start === 'number' || typeof end === 'number') return engineMeasure(name, { start: start === undefined ? 0 : start, end: end === undefined ? performance.now() : end });
      return engineMeasure(name, startOrOptions, endMark);
    },
    writable: true, configurable: true, enumerable: true,
  });
  Object.assign(globalThis, { PerformanceTiming, PerformanceNavigation });
  for (const key of ['PerformanceTiming', 'PerformanceNavigation']) Object.defineProperty(globalThis, key, { enumerable: false });
}

Object.assign(globalThis, {
  requestAnimationFrame,
  cancelAnimationFrame,
  requestIdleCallback,
  cancelIdleCallback,
  alert() {}, confirm() { return false; }, prompt() { return null; }, print() {}, open() { return null; }, close() {}, stop() {}, focus() {}, blur() {},
  scrollTo() {}, scrollBy() {}, scroll() {}, moveTo() {}, moveBy() {}, resizeTo() {}, resizeBy() {},
  getSelection: () => selection,
  reportError,
  postMessage(message, targetOrigin) { setTimeout(() => window.dispatchEvent(new MessageEvent('message', { data: message, origin: location.origin, source: window })), 0); },
  Image: class Image { constructor(w, h) { const img = document.createElement('img'); if (w !== undefined) img.setAttribute('width', w); if (h !== undefined) img.setAttribute('height', h); return img; } },
  Option: class Option { constructor(text = '', value, defaultSelected, selected) { const o = document.createElement('option'); if (text !== '') o.textContent = text; if (value !== undefined) o.setAttribute('value', value); if (defaultSelected) o.setAttribute('selected', ''); if (selected) o.selected = true; return o; } },
  visualViewport: Object.assign(Object.create(EventTarget.prototype), { width: VIEWPORT.width, height: VIEWPORT.height, scale: 1, offsetLeft: 0, offsetTop: 0, pageLeft: 0, pageTop: 0 }),
});
for (const key of ['requestAnimationFrame', 'cancelAnimationFrame', 'requestIdleCallback', 'cancelIdleCallback', 'alert', 'confirm', 'prompt', 'print', 'open', 'close', 'stop', 'focus', 'blur', 'scrollTo', 'scrollBy', 'scroll', 'moveTo', 'moveBy', 'resizeTo', 'resizeBy', 'getSelection', 'reportError', 'postMessage', 'Image', 'Option', 'visualViewport']) {
  Object.defineProperty(globalThis, key, { enumerable: false });
}

