// ---- Exposição: o que a página enxerga como globais, e o gancho devolvido ao hospedeiro.

const window = globalThis;
// O que o motor traz do runtime de servidor (bun) não existe numa janela; `SharedArrayBuffer` o Chromium só expõe com
// isolamento cross-origin (COOP e COEP, medido no oráculo).
const crossOriginIsolated = op('crossOriginIsolated');
for (const name of ['process', 'Bun', 'Buffer', 'global', 'BuildMessage', 'ResolveMessage', 'ShadowRealm', 'require', 'module', 'exports', '__dirname', '__filename'].concat(crossOriginIsolated ? [] : ['SharedArrayBuffer'])) {
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
  customElements,
  getComputedStyle, matchMedia, fetch: realFetch, Request, XMLHttpRequest, MutationObserver, MutationRecord, ResizeObserver, ResizeObserverEntry, IntersectionObserver, IntersectionObserverEntry, TextTrack, TextTrackList, PerformanceObserver,
  WebGLRenderingContext, WebGL2RenderingContext, WebGLObject, WebGLBuffer, WebGLFramebuffer, WebGLProgram, WebGLQuery, WebGLRenderbuffer, WebGLSampler, WebGLShader, WebGLSync, WebGLTexture, WebGLTransformFeedback, WebGLVertexArrayObject, WebGLUniformLocation, WebGLActiveInfo, WebGLShaderPrecisionFormat, WebGLContextEvent,
  Node, Element, HTMLElement, HTMLUnknownElement, SVGElement, SVGGraphicsElement, SVGSVGElement, MathMLElement, Text, Comment, CDATASection, CharacterData, DocumentFragment, ShadowRoot, DocumentType, Document, Attr, NamedNodeMap, NodeList, HTMLCollection,
  DOMTokenList, CSSStyleDeclaration, CSSStyleSheet, DOMRect, DOMRectReadOnly, DOMPoint, DOMPointReadOnly, DOMQuad, DOMMatrix, DOMMatrixReadOnly, CanvasRenderingContext2D, Path2D, CanvasGradient, CanvasPattern, ImageData, TextMetrics, ImageBitmap, createImageBitmap, Range, Selection, TreeWalker, NodeIterator, DOMParser, XMLSerializer, DOMImplementation, XPathEvaluator, XPathExpression, XPathResult, SVGAnimatedString, Location, History, Navigator, Storage, CustomElementRegistry,
  EventTarget, Event, CustomEvent, UIEvent, MouseEvent, PointerEvent, WheelEvent, KeyboardEvent, FocusEvent, InputEvent, ErrorEvent, MessageEvent, HashChangeEvent, PopStateEvent, ProgressEvent, TouchEvent, AnimationEvent, TransitionEvent, PageTransitionEvent, SubmitEvent, PromiseRejectionEvent,
  HTMLDocument: Document,
};
for (const [key, value] of Object.entries(windowGlobals)) Object.defineProperty(globalThis, key, { value, writable: true, configurable: true, enumerable: key === 'document' || key === 'location' ? true : false });
// Configuráveis por ora: a passagem final (`WINDOW_KEYS`) os redefine na ordem e na forma do Chromium e os tranca.
Object.defineProperty(globalThis, 'document', { value: document, writable: false, configurable: true, enumerable: true });
Object.defineProperty(globalThis, 'location', { get: () => location, set: (v) => { location.href = v; }, configurable: true, enumerable: true });
// O storage do documento: um objeto por área, sempre o mesmo; num documento `data:` (origem opaca), a leitura lança.
for (const [name, area] of [['localStorage', 'local'], ['sessionStorage', 'session']]) {
  let storage = null;
  readonly(name, () => {
    if (pageState.url.startsWith('data:')) throw domException(`Failed to read the '${name}' property from 'Window': Storage is disabled inside 'data:' URLs.`, 'SecurityError');
    return storage || (storage = makeStorage(area));
  });
}

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

const windowMetrics = { innerWidth: VIEWPORT.width, innerHeight: VIEWPORT.height, outerWidth: VIEWPORT.outerWidth, outerHeight: VIEWPORT.outerHeight, devicePixelRatio: 1, scrollX: 0, scrollY: 0, pageXOffset: 0, pageYOffset: 0, screenX: 0, screenY: 0, screenLeft: 0, screenTop: 0, length: 0, closed: false, opener: null, name: '', status: '', isSecureContext: pageState.url.startsWith('https:') || pageState.url.startsWith('http://localhost'), origin: location.origin === 'null' ? 'null' : location.origin, crossOriginIsolated, originAgentCluster: true };
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
    return () => { refreshPolicies(); try { evalGlobal(code, pageState.url); } catch (error) { reportError(error); } };
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
  if (typeof handler !== 'function') {
    const name = engine === engineSetTimeout ? 'setTimeout' : 'setInterval';
    handler = enforceSink(`Window ${name}`, 'TrustedScript', handler, assignmentError('call', name, 'Window', 'TrustedScript'));
  }
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

// Navigation Timing do navegador: a entrada `navigation` do Performance Timeline (PerformanceNavigationTiming sobre
// PerformanceResourceTiming, com a forma do Blink medida no oráculo), `performance.timing`, `performance.navigation`
// e os nomes legados (`navigationStart`, `domContentLoadedEventEnd`...) aceitos como marcas por
// `performance.measure`. Os instantes são os do carregamento desta página, em ms desde a origem de tempo.
{
  const origin = performance.timeOrigin;
  // Os marcos da busca chegam em tempo real desde o início da navegação e saem no grão do `TimeClamper`.
  const fetchMarks = Object.fromEntries(Object.entries(DOCUMENT_TRANSFER.marks).map(([name, at]) => [name, at === 0 ? 0 : op('frameTime', at)]));
  const marks = { navigationStart: 0, unloadEventStart: 0, unloadEventEnd: 0, redirectStart: 0, redirectEnd: 0, ...fetchMarks };
  const lifecycle = { domInteractive: null, domContentLoadedEventStart: null, domContentLoadedEventEnd: null, domComplete: null, loadEventStart: null, loadEventEnd: null };
  document.addEventListener('readystatechange', () => {
    const now = performance.now();
    if (document.readyState === 'interactive') lifecycle.domInteractive = now;
    if (document.readyState === 'complete') lifecycle.domComplete = now;
  });
  document.addEventListener('DOMContentLoaded', () => { lifecycle.domContentLoadedEventStart = lifecycle.domContentLoadedEventEnd = performance.now(); }, { capture: true });
  window.addEventListener('load', () => { lifecycle.loadEventStart = performance.now(); setTimeout(() => { lifecycle.loadEventEnd = performance.now(); }, 0); }, { capture: true });
  const relative = (name) => (name in marks ? marks[name] : name in lifecycle ? lifecycle[name] : undefined);
  // A ordem do `toJSON` do Blink, que não é a do protótipo (medida no oráculo).
  const TIMING_JSON = ['connectStart', 'secureConnectionStart', 'unloadEventEnd', 'domainLookupStart', 'domainLookupEnd', 'responseStart', 'connectEnd', 'responseEnd', 'requestStart', 'domLoading', 'redirectStart', 'loadEventEnd', 'domComplete', 'navigationStart', 'loadEventStart', 'domContentLoadedEventEnd', 'unloadEventStart', 'redirectEnd', 'domInteractive', 'fetchStart', 'domContentLoadedEventStart'];
  const TIMING_PROTOTYPE = ['navigationStart', 'unloadEventStart', 'unloadEventEnd', 'redirectStart', 'redirectEnd', 'fetchStart', 'domainLookupStart', 'domainLookupEnd', 'connectStart', 'connectEnd', 'secureConnectionStart', 'requestStart', 'responseStart', 'responseEnd', 'domLoading', 'domInteractive', 'domContentLoadedEventStart', 'domContentLoadedEventEnd', 'domComplete', 'loadEventStart', 'loadEventEnd'];
  class PerformanceTiming {
    constructor() { throw new TypeError('Illegal constructor'); }
    toJSON() { const out = {}; for (const k of TIMING_JSON) out[k] = this[k]; return out; }
  }
  // O legado é a época em milissegundos inteiros (o piso, como o `monotonicTimeToIntegerMilliseconds` do Blink); o que
  // não aconteceu, ou não se aplica, é 0.
  for (const name of TIMING_PROTOTYPE) {
    Object.defineProperty(PerformanceTiming.prototype, name, { get() { const v = relative(name); return v === null || (v === 0 && name !== 'navigationStart') ? 0 : Math.floor(origin + v); }, configurable: true, enumerable: true });
  }
  blinkInterface(PerformanceTiming, 'PerformanceTiming', ['toJSON', 'constructor']);
  const timing = Object.create(PerformanceTiming.prototype);
  class PerformanceNavigation {
    constructor() { throw new TypeError('Illegal constructor'); }
    get type() { return 0; }
    get redirectCount() { return DOCUMENT_TRANSFER.redirectCount; }
    toJSON() { return { type: this.type, redirectCount: this.redirectCount }; }
  }
  for (const [key, value] of [['TYPE_NAVIGATE', 0], ['TYPE_RELOAD', 1], ['TYPE_BACK_FORWARD', 2], ['TYPE_RESERVED', 255]]) {
    Object.defineProperty(PerformanceNavigation, key, { value, enumerable: true });
    Object.defineProperty(PerformanceNavigation.prototype, key, { value, enumerable: true });
  }
  blinkInterface(PerformanceNavigation, 'PerformanceNavigation', ['toJSON', 'constructor']);
  const navigation = Object.create(PerformanceNavigation.prototype);

  // A entrada `navigation`: um objeto só, o mesmo em toda leitura, com o endereço do documento quando carregou.
  const illegalConstructor = (name) => new TypeError(`Failed to construct '${name}': Illegal constructor`);
  const documentAddress = location.href;
  const navigationId = 1 + (crypto.getRandomValues(new Uint16Array(1))[0] & 0x1fff);
  // O `confidence` do Blink: a taxa da resposta aleatorizada é fixa, e o valor sorteado uma vez por documento.
  const confidenceValue = crypto.getRandomValues(new Uint8Array(1))[0] & 1 ? 'high' : 'low';
  const lifecycleAt = (name) => (lifecycle[name] === null ? 0 : lifecycle[name]);
  const entryFields = {
    name: () => documentAddress,
    entryType: () => 'navigation',
    startTime: () => 0,
    duration: () => lifecycleAt('loadEventEnd'),
    navigationId: () => navigationId,
    initiatorType: () => 'navigation',
    deliveryType: () => '',
    nextHopProtocol: () => DOCUMENT_TRANSFER.protocol,
    renderBlockingStatus: () => 'non-blocking',
    contentType: () => DOCUMENT_TRANSFER.contentType,
    contentEncoding: () => DOCUMENT_TRANSFER.contentEncoding,
    workerStart: () => 0,
    workerRouterEvaluationStart: () => 0,
    workerCacheLookupStart: () => 0,
    workerMatchedSourceType: () => '',
    workerFinalSourceType: () => '',
    redirectStart: () => 0,
    redirectEnd: () => 0,
    fetchStart: () => marks.fetchStart,
    domainLookupStart: () => marks.domainLookupStart,
    domainLookupEnd: () => marks.domainLookupEnd,
    connectStart: () => marks.connectStart,
    secureConnectionStart: () => marks.secureConnectionStart,
    connectEnd: () => marks.connectEnd,
    requestStart: () => marks.requestStart,
    responseStart: () => marks.responseStart,
    firstInterimResponseStart: () => 0,
    finalResponseHeadersStart: () => (DOCUMENT_TRANSFER.network ? marks.responseStart : 0),
    responseEnd: () => marks.responseEnd,
    // O Blink soma 300 bytes de cabeçalho ao corpo como veio no fio.
    transferSize: () => DOCUMENT_TRANSFER.encodedBodySize + 300,
    encodedBodySize: () => DOCUMENT_TRANSFER.encodedBodySize,
    decodedBodySize: () => DOCUMENT_TRANSFER.decodedBodySize,
    responseStatus: () => DOCUMENT_TRANSFER.status,
    serverTiming: () => Object.freeze([]),
    unloadEventStart: () => 0,
    unloadEventEnd: () => 0,
    domInteractive: () => lifecycleAt('domInteractive'),
    domContentLoadedEventStart: () => lifecycleAt('domContentLoadedEventStart'),
    domContentLoadedEventEnd: () => lifecycleAt('domContentLoadedEventEnd'),
    domComplete: () => lifecycleAt('domComplete'),
    loadEventStart: () => lifecycleAt('loadEventStart'),
    loadEventEnd: () => lifecycleAt('loadEventEnd'),
    type: () => 'navigate',
    redirectCount: () => DOCUMENT_TRANSFER.redirectCount,
    activationStart: () => 0,
    criticalCHRestart: () => 0,
    notRestoredReasons: () => null,
    confidence: () => newConfidence(),
  };
  const ENTRY_JSON = ['name', 'entryType', 'startTime', 'duration', 'navigationId', 'initiatorType', 'deliveryType', 'nextHopProtocol', 'renderBlockingStatus', 'contentType', 'contentEncoding', 'workerStart', 'workerRouterEvaluationStart', 'workerCacheLookupStart', 'workerMatchedSourceType', 'workerFinalSourceType', 'redirectStart', 'redirectEnd', 'fetchStart', 'domainLookupStart', 'domainLookupEnd', 'connectStart', 'secureConnectionStart', 'connectEnd', 'requestStart', 'responseStart', 'firstInterimResponseStart', 'finalResponseHeadersStart', 'responseEnd', 'transferSize', 'encodedBodySize', 'decodedBodySize', 'responseStatus', 'serverTiming', 'unloadEventStart', 'unloadEventEnd', 'domInteractive', 'domContentLoadedEventStart', 'domContentLoadedEventEnd', 'domComplete', 'loadEventStart', 'loadEventEnd', 'type', 'redirectCount', 'activationStart', 'criticalCHRestart', 'notRestoredReasons', 'confidence'];
  const entries = new WeakSet();
  const field = (self, key) => {
    if (!entries.has(self)) throw new TypeError('Illegal invocation');
    return entryFields[key]();
  };
  // O `confidence` entra como o próprio objeto (o `JSON.stringify` chama o `toJSON` dele depois).
  const entryJSON = function toJSON() {
    if (!entries.has(this)) throw new TypeError('Illegal invocation');
    const out = {};
    for (const key of ENTRY_JSON) out[key] = this[key];
    return out;
  };

  class PerformanceTimingConfidence {
    constructor() { throw illegalConstructor('PerformanceTimingConfidence'); }
    get randomizedTriggerRate() { receiver(this, P.__confidence); return 0.4994798; }
    get value() { receiver(this, P.__confidence); return confidenceValue; }
    toJSON() { receiver(this, P.__confidence); return { randomizedTriggerRate: this.randomizedTriggerRate, value: this.value }; }
  }
  blinkInterface(PerformanceTimingConfidence, 'PerformanceTimingConfidence', ['toJSON', 'constructor']);
  const newConfidence = () => {
    const confidence = Object.create(PerformanceTimingConfidence.prototype);
    hidden(confidence, '__confidence', true);
    return confidence;
  };

  // O PerformanceEntry do motor ganha o `navigationId` e responde pela entrada `navigation`; as marcas e medidas dele
  // seguem nos getters originais.
  const entryProto = PerformanceEntry.prototype;
  for (const key of ['name', 'entryType', 'startTime', 'duration']) {
    const original = Object.getOwnPropertyDescriptor(entryProto, key).get;
    const holder = { get [key]() { return entries.has(this) ? entryFields[key]() : original.call(this); } };
    Object.defineProperty(entryProto, key, { get: Object.getOwnPropertyDescriptor(holder, key).get, enumerable: true, configurable: true });
  }
  const originalToJSON = entryProto.toJSON;
  Object.defineProperty(entryProto, 'toJSON', { value: { toJSON() { return entries.has(this) ? entryJSON.call(this) : originalToJSON.call(this); } }.toJSON, writable: true, enumerable: true, configurable: true });
  // Todas as entradas do documento levam o mesmo `navigationId`.
  Object.defineProperty(entryProto, 'navigationId', { get: Object.getOwnPropertyDescriptor({ get navigationId() { return navigationId; } }, 'navigationId').get, enumerable: true, configurable: true });
  for (const key of ['name', 'entryType', 'startTime', 'duration', 'toJSON', 'constructor', 'navigationId']) {
    const descriptor = Object.getOwnPropertyDescriptor(entryProto, key);
    delete entryProto[key];
    Object.defineProperty(entryProto, key, descriptor);
  }

  const RESOURCE_MEMBERS = ['initiatorType', 'nextHopProtocol', 'deliveryType', 'workerStart', 'redirectStart', 'redirectEnd', 'fetchStart', 'domainLookupStart', 'domainLookupEnd', 'connectStart', 'connectEnd', 'secureConnectionStart', 'requestStart', 'responseStart', 'responseEnd', 'transferSize', 'encodedBodySize', 'decodedBodySize', 'serverTiming', 'renderBlockingStatus', 'responseStatus', 'contentType', 'contentEncoding', 'finalResponseHeadersStart', 'firstInterimResponseStart', 'toJSON', 'workerRouterEvaluationStart', 'workerCacheLookupStart', 'workerMatchedSourceType', 'workerFinalSourceType', 'constructor'];
  const NAVIGATION_MEMBERS = ['unloadEventStart', 'unloadEventEnd', 'domInteractive', 'domContentLoadedEventStart', 'domContentLoadedEventEnd', 'domComplete', 'loadEventStart', 'loadEventEnd', 'type', 'redirectCount', 'confidence', 'criticalCHRestart', 'activationStart', 'toJSON', 'constructor', 'notRestoredReasons'];
  // Os membros de cada interface, como getters do Blink (nome `get x`, enumeráveis), na ordem do protótipo medida.
  const defineMembers = (cls, members) => {
    for (const key of members) {
      if (key === 'constructor') {
        const descriptor = Object.getOwnPropertyDescriptor(cls.prototype, 'constructor');
        delete cls.prototype.constructor;
        Object.defineProperty(cls.prototype, 'constructor', descriptor);
      } else if (key === 'toJSON') {
        Object.defineProperty(cls.prototype, 'toJSON', { value: entryJSON, writable: true, enumerable: true, configurable: true });
      } else {
        const holder = { get [key]() { return field(this, key); } };
        Object.defineProperty(cls.prototype, key, { get: Object.getOwnPropertyDescriptor(holder, key).get, enumerable: true, configurable: true });
      }
    }
    tag(cls, cls.name);
    return cls;
  };
  // O PerformanceEntry do motor não serve de superclasse a `extends` (não tem [[Construct]]): a herança é posta à mão.
  const PerformanceResourceTiming = defineMembers(class PerformanceResourceTiming {
    constructor() { throw illegalConstructor('PerformanceResourceTiming'); }
  }, RESOURCE_MEMBERS);
  Object.setPrototypeOf(PerformanceResourceTiming, PerformanceEntry);
  Object.setPrototypeOf(PerformanceResourceTiming.prototype, entryProto);
  const PerformanceNavigationTiming = defineMembers(class PerformanceNavigationTiming extends PerformanceResourceTiming {
    constructor() { throw illegalConstructor('PerformanceNavigationTiming'); }
  }, NAVIGATION_MEMBERS);
  const navigationEntry = Object.create(PerformanceNavigationTiming.prototype);
  entries.add(navigationEntry);

  // A linha do tempo começa pela entrada `navigation` (`startTime` 0), antes das marcas e medidas do motor.
  const engineEntries = { getEntries: performance.getEntries, getEntriesByType: performance.getEntriesByType, getEntriesByName: performance.getEntriesByName };
  const timeline = {
    getEntries() { return [navigationEntry, ...engineEntries.getEntries.call(this)]; },
    getEntriesByType(type) {
      const own = String(type) === 'navigation' ? [navigationEntry] : [];
      return [...own, ...engineEntries.getEntriesByType.apply(this, arguments)];
    },
    getEntriesByName(name, type) {
      const own = String(name) === documentAddress && (type === undefined || String(type) === 'navigation') ? [navigationEntry] : [];
      return [...own, ...engineEntries.getEntriesByName.apply(this, arguments)];
    },
  };
  for (const key of Object.keys(timeline)) Object.defineProperty(performance, key, { value: timeline[key], writable: true, enumerable: true, configurable: true });
  Object.assign(globalThis, { PerformanceResourceTiming, PerformanceNavigationTiming, PerformanceTimingConfidence });
  for (const key of ['PerformanceResourceTiming', 'PerformanceNavigationTiming', 'PerformanceTimingConfidence']) Object.defineProperty(globalThis, key, { enumerable: false });
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

const dialogText = (value) => (value === undefined ? '' : String(value));

Object.assign(globalThis, {
  requestAnimationFrame,
  cancelAnimationFrame,
  requestIdleCallback,
  cancelIdleCallback,
  alert(message) { op('dialog', 'alert', dialogText(message), ''); }, confirm(message) { return op('dialog', 'confirm', dialogText(message), '') !== null; }, prompt(message, fallback) { return op('dialog', 'prompt', dialogText(message), dialogText(fallback)); }, print() {}, open() { return null; }, close() {}, stop() {}, focus() {}, blur() {},
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

installTrustedTypesSinks();

// As propriedades próprias enumeráveis da janela na ordem e na forma do Chromium (`data/window-keys.json`, medido no
// oráculo): `v` é valor (`w` gravável), `a` é acessor (`s` com setter), `c` configurável. O resto fica não enumerável.
// O que o navegador ainda não tem continua ausente, como pede a regra de não imitar pela metade.
{
  const chromium = new Map(WINDOW_KEYS);
  for (const key of Reflect.ownKeys(globalThis)) {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, key);
    if (descriptor.enumerable && !chromium.has(key) && descriptor.configurable) Object.defineProperty(globalThis, key, { enumerable: false });
  }
  for (const [key, shape] of WINDOW_KEYS) {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, key);
    if (descriptor === undefined || !descriptor.configurable) continue;
    delete globalThis[key];
    const configurable = shape.includes('c');
    if (shape[0] === 'v') {
      const value = 'value' in descriptor ? descriptor.value : descriptor.get.call(window);
      Object.defineProperty(globalThis, key, { value, writable: shape.includes('w'), configurable, enumerable: true });
      continue;
    }
    let get = descriptor.get;
    let set = descriptor.set;
    if (get === undefined) {
      let current = descriptor.value;
      get = function () { return current; };
      Object.defineProperty(get, 'name', { value: 'get ' + key });
      if (shape.includes('s')) {
        set = function (value) { current = value; };
        Object.defineProperty(set, 'name', { value: 'set ' + key });
      }
    }
    Object.defineProperty(globalThis, key, shape.includes('s') ? { get, set, configurable, enumerable: true } : { get, configurable, enumerable: true });
  }
}
