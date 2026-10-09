// ---- Exposição: o que a página enxerga como globais, e o gancho devolvido ao hospedeiro.

const window = globalThis;
const document = wrap(op('root'));
hidden(document, '__window', window);
hidden(document, '__readyState', 'loading');

const NodeFilter = { FILTER_ACCEPT: 1, FILTER_REJECT: 2, FILTER_SKIP: 3, SHOW_ALL: 0xffffffff, SHOW_ELEMENT: 1, SHOW_ATTRIBUTE: 2, SHOW_TEXT: 4, SHOW_CDATA_SECTION: 8, SHOW_PROCESSING_INSTRUCTION: 64, SHOW_COMMENT: 128, SHOW_DOCUMENT: 256, SHOW_DOCUMENT_TYPE: 512, SHOW_DOCUMENT_FRAGMENT: 1024 };

class DOMParser {
  parseFromString(text, type) {
    throw domException('DOMParser is not supported in this build.', 'NotSupportedError');
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
  getComputedStyle, matchMedia, fetch: realFetch, XMLHttpRequest, MutationObserver, MutationRecord, ResizeObserver, IntersectionObserver, PerformanceObserver,
  Node, Element, HTMLElement, HTMLUnknownElement, SVGElement, SVGGraphicsElement, SVGSVGElement, MathMLElement, Text, Comment, CDATASection, CharacterData, DocumentFragment, ShadowRoot, DocumentType, Document, Attr, NamedNodeMap, NodeList, HTMLCollection,
  DOMTokenList, CSSStyleDeclaration, CSSStyleSheet, DOMRect, DOMRectReadOnly: DOMRect, Range, Selection, TreeWalker, NodeIterator, DOMParser, XMLSerializer, Location, History, Navigator, Storage, CustomElementRegistry,
  EventTarget, Event, CustomEvent, UIEvent, MouseEvent, PointerEvent, WheelEvent, KeyboardEvent, FocusEvent, InputEvent, ErrorEvent, MessageEvent, HashChangeEvent, PopStateEvent, ProgressEvent, TouchEvent, AnimationEvent, TransitionEvent, PageTransitionEvent, SubmitEvent,
  HTMLDocument: Document,
};
for (const [key, value] of Object.entries(windowGlobals)) Object.defineProperty(globalThis, key, { value, writable: true, configurable: true, enumerable: key === 'document' || key === 'location' ? true : false });
Object.defineProperty(globalThis, 'document', { value: document, writable: false, configurable: false, enumerable: true });
Object.defineProperty(globalThis, 'location', { get: () => location, set: (v) => { location.href = v; }, configurable: false, enumerable: true });

const windowMetrics = { innerWidth: 800, innerHeight: 600, outerWidth: 800, outerHeight: 600, devicePixelRatio: 1, scrollX: 0, scrollY: 0, pageXOffset: 0, pageYOffset: 0, screenX: 0, screenY: 0, screenLeft: 0, screenTop: 0, length: 0, closed: false, opener: null, name: '', status: '', isSecureContext: pageState.url.startsWith('https:') || pageState.url.startsWith('http://localhost'), origin: location.origin === 'null' ? 'null' : location.origin, crossOriginIsolated: false, originAgentCluster: true };
for (const [key, value] of Object.entries(windowMetrics)) Object.defineProperty(globalThis, key, { value, writable: true, configurable: true, enumerable: true });

// Métodos de EventTarget na própria janela.
for (const key of ['addEventListener', 'removeEventListener', 'dispatchEvent']) Object.defineProperty(globalThis, key, { value: EventTarget.prototype[key], writable: true, configurable: true, enumerable: true });
defineHandlerProperties(globalThis, ['load', 'unload', 'beforeunload', 'error', 'resize', 'scroll', 'hashchange', 'popstate', 'message', 'pageshow', 'pagehide', 'storage', 'online', 'offline', 'focus', 'blur', 'languagechange', 'visibilitychange', 'orientationchange', 'unhandledrejection', 'rejectionhandled', 'beforeprint', 'afterprint', 'click', 'keydown', 'keyup', 'keypress', 'mousedown', 'mouseup', 'mousemove', 'wheel', 'touchstart', 'touchmove', 'touchend', 'pointerdown', 'pointerup', 'pointermove', 'contextmenu', 'dblclick', 'input', 'change', 'submit', 'animationend', 'transitionend']);
// Eventos de elemento: `on<tipo>` em Element (a propriedade vive no protótipo, o tratador no nó).
defineHandlerProperties(Element.prototype, ['click', 'dblclick', 'mousedown', 'mouseup', 'mousemove', 'mouseover', 'mouseout', 'mouseenter', 'mouseleave', 'contextmenu', 'keydown', 'keyup', 'keypress', 'focus', 'blur', 'focusin', 'focusout', 'input', 'change', 'submit', 'reset', 'select', 'scroll', 'wheel', 'touchstart', 'touchmove', 'touchend', 'touchcancel', 'pointerdown', 'pointerup', 'pointermove', 'pointerover', 'pointerout', 'pointerenter', 'pointerleave', 'pointercancel', 'drag', 'dragstart', 'dragend', 'dragover', 'dragenter', 'dragleave', 'drop', 'load', 'error', 'abort', 'animationstart', 'animationend', 'animationiteration', 'transitionend', 'transitionstart', 'transitionrun', 'transitioncancel', 'copy', 'cut', 'paste', 'toggle', 'invalid', 'cancel', 'close', 'canplay', 'play', 'pause', 'ended', 'loadeddata', 'loadedmetadata', 'timeupdate', 'volumechange', 'beforeinput', 'resize', 'fullscreenchange']);

const scheduleFrame = (cb) => setTimeout(() => cb(performance.now()), 16);
Object.assign(globalThis, {
  requestAnimationFrame: scheduleFrame,
  cancelAnimationFrame: (id) => clearTimeout(id),
  requestIdleCallback: (cb) => setTimeout(() => cb({ didTimeout: false, timeRemaining: () => 50 }), 1),
  cancelIdleCallback: (id) => clearTimeout(id),
  alert() {}, confirm() { return false; }, prompt() { return null; }, print() {}, open() { return null; }, close() {}, stop() {}, focus() {}, blur() {},
  scrollTo() {}, scrollBy() {}, scroll() {}, moveTo() {}, moveBy() {}, resizeTo() {}, resizeBy() {},
  getSelection: () => selection,
  reportError,
  postMessage(message, targetOrigin) { setTimeout(() => window.dispatchEvent(new MessageEvent('message', { data: message, origin: location.origin, source: window })), 0); },
  Image: class Image { constructor(w, h) { const img = document.createElement('img'); if (w !== undefined) img.setAttribute('width', w); if (h !== undefined) img.setAttribute('height', h); return img; } },
  Option: class Option { constructor(text = '', value, defaultSelected, selected) { const o = document.createElement('option'); if (text !== '') o.textContent = text; if (value !== undefined) o.setAttribute('value', value); if (defaultSelected) o.setAttribute('selected', ''); if (selected) o.selected = true; return o; } },
  visualViewport: Object.assign(Object.create(EventTarget.prototype), { width: 800, height: 600, scale: 1, offsetLeft: 0, offsetTop: 0, pageLeft: 0, pageTop: 0 }),
});
for (const key of ['requestAnimationFrame', 'cancelAnimationFrame', 'requestIdleCallback', 'cancelIdleCallback', 'alert', 'confirm', 'prompt', 'print', 'open', 'close', 'stop', 'focus', 'blur', 'scrollTo', 'scrollBy', 'scroll', 'moveTo', 'moveBy', 'resizeTo', 'resizeBy', 'getSelection', 'reportError', 'postMessage', 'Image', 'Option', 'visualViewport']) {
  Object.defineProperty(globalThis, key, { enumerable: false });
}

