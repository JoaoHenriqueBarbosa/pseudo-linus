// ---- Eventos: EventTarget, Event e os tipos de UI, com propagação pela árvore (captura, alvo, bolha).

const listenerStore = new WeakMap();
const handlerStore = new WeakMap();

const storeOf = (target) => {
  let store = listenerStore.get(target);
  if (!store) {
    store = new Map();
    listenerStore.set(target, store);
  }
  return store;
};

// `addEventListener(...)` chamado solto no topo da página: no navegador o `this` ausente é a janela; um `this` que não
// é objeto é "Illegal invocation", como num método nativo.
const targetOf = (self) => {
  if (self === undefined || self === null) return globalThis;
  if (typeof self !== 'object' && typeof self !== 'function') throw new TypeError('Illegal invocation');
  return self;
};

class EventTarget {
  addEventListener(type, callback, options) {
    const self = targetOf(this);
    if (callback === null || callback === undefined) return;
    const opts = typeof options === 'boolean' ? { capture: options } : options || {};
    const entry = { callback, capture: !!opts.capture, once: !!opts.once, passive: !!opts.passive, removed: false };
    const list = storeOf(self).get(String(type)) || [];
    if (list.some((e) => e.callback === callback && e.capture === entry.capture)) return;
    list.push(entry);
    storeOf(self).set(String(type), list);
    if (opts.signal) opts.signal.addEventListener('abort', () => self.removeEventListener(type, callback, entry.capture), { once: true });
  }
  removeEventListener(type, callback, options) {
    const self = targetOf(this);
    const capture = typeof options === 'boolean' ? options : !!(options && options.capture);
    const list = storeOf(self).get(String(type));
    if (!list) return;
    const index = list.findIndex((e) => e.callback === callback && e.capture === capture);
    if (index >= 0) {
      list[index].removed = true;
      list.splice(index, 1);
    }
  }
  dispatchEvent(event) {
    if (!(event instanceof Event)) throw new TypeError("Failed to execute 'dispatchEvent' on 'EventTarget': parameter 1 is not of type 'Event'.");
    if (event.__dispatching) throw domException('The event is already being dispatched.', 'InvalidStateError');
    return dispatch(targetOf(this), event);
  }
}
tag(EventTarget, 'EventTarget');

// O pai de um alvo no caminho do evento: nó -> pai; documento -> janela.
const eventParent = (target, event) => {
  if (isNode(target)) {
    const parent = op('parent', ids.get(target));
    if (parent !== null) return wrappers.get(parent) || wrap(parent);
    // A raiz de sombra só passa o evento ao host quando ele é `composed`.
    if (target.__host) return event && event.composed ? target.__host : null;
    return target.nodeType === 9 && target.__window ? target.__window : null;
  }
  return null;
};

const describeError = (error) => {
  if (error && typeof error === 'object' && 'message' in error) return `${error.name || 'Error'}: ${error.message}`;
  return String(error);
};

const reportError = (error) => {
  const event = new ErrorEvent('error', { message: 'Uncaught ' + describeError(error), error, cancelable: true });
  const handled = !window.dispatchEvent(event);
  if (!handled) console.error('Uncaught ' + describeError(error));
};

const invoke = (target, event, entry) => {
  event.__passive = entry.passive;
  try {
    if (typeof entry.callback === 'function') entry.callback.call(target, event);
    else if (entry.callback && typeof entry.callback.handleEvent === 'function') entry.callback.handleEvent(event);
  } catch (error) {
    reportError(error);
  }
  event.__passive = false;
};

const attributeHandler = (target, type) => {
  if (!isNode(target) || target.nodeType !== 1) return null;
  let owner = target;
  const source = owner.getAttribute('on' + type);
  if (source === null) return null;
  let cache = handlerStore.get(owner);
  if (!cache) {
    cache = new Map();
    handlerStore.set(owner, cache);
  }
  const cached = cache.get(type);
  if (cached && cached.source === source) return cached.fn;
  let fn;
  try {
    fn = new Function('event', source);
  } catch (error) {
    fn = null;
    reportError(error);
  }
  cache.set(type, { source, fn });
  return fn;
};

const callHandlerProperty = (target, event) => {
  const holder = handlerStore.get(target);
  let handler = holder && holder.props && holder.props.get(event.type);
  if (handler === undefined) handler = attributeHandler(target, event.type);
  if (typeof handler !== 'function') return;
  try {
    // `onerror` da janela recebe os campos do ErrorEvent como argumentos, e `true` cancela o relato.
    if (event.type === 'error' && target === globalThis && event instanceof ErrorEvent) {
      if (handler.call(target, event.message, event.filename, event.lineno, event.colno, event.error) === true) event.preventDefault();
      return;
    }
    const result = handler.call(target, event);
    if (result === false) event.preventDefault();
  } catch (error) {
    reportError(error);
  }
};

const dispatch = (target, event) => {
  event.__dispatching = true;
  event.target = event.__targetOverride || target;
  const path = [];
  for (let node = target; node; node = eventParent(node, event)) path.push(node);
  event.__path = path.slice();
  const run = (node, phase) => {
    event.currentTarget = node;
    event.eventPhase = phase;
    const list = (listenerStore.get(node) && listenerStore.get(node).get(event.type)) || [];
    for (const entry of list.slice()) {
      if (entry.removed) continue;
      if (phase === 1 && !entry.capture) continue;
      if (phase === 3 && entry.capture) continue;
      if (entry.once) node.removeEventListener(event.type, entry.callback, entry.capture);
      invoke(node, event, entry);
      if (event.__stopImmediate) return;
    }
    if (phase !== 1) callHandlerProperty(node, event);
  };
  for (let i = path.length - 1; i > 0 && !event.__stop; i--) run(path[i], 1);
  if (!event.__stop) run(path[0], 2);
  if (event.bubbles) for (let i = 1; i < path.length && !event.__stop; i++) run(path[i], 3);
  event.currentTarget = null;
  event.eventPhase = 0;
  event.__dispatching = false;
  event.__stop = false;
  event.__stopImmediate = false;
  event.__path = [];
  return !event.defaultPrevented;
};

class Event {
  constructor(type, init) {
    if (arguments.length === 0) throw new TypeError("Failed to construct 'Event': 1 argument required, but only 0 present.");
    const opts = init || {};
    hidden(this, '__dispatching', false);
    hidden(this, '__stop', false);
    hidden(this, '__stopImmediate', false);
    hidden(this, '__passive', false);
    hidden(this, '__path', []);
    this.type = String(type);
    this.target = null;
    this.currentTarget = null;
    this.eventPhase = 0;
    this.bubbles = !!opts.bubbles;
    this.cancelable = !!opts.cancelable;
    this.composed = !!opts.composed;
    this.defaultPrevented = false;
    this.timeStamp = performance.now();
    this.isTrusted = false;
    this.srcElement = null;
  }
  get cancelBubble() { return this.__stop; }
  set cancelBubble(value) { if (value) this.__stop = true; }
  get returnValue() { return !this.defaultPrevented; }
  set returnValue(value) { if (!value) this.preventDefault(); }
  stopPropagation() { this.__stop = true; }
  stopImmediatePropagation() { this.__stop = true; this.__stopImmediate = true; }
  preventDefault() { if (this.cancelable && !this.__passive) this.defaultPrevented = true; }
  composedPath() { return this.__dispatching ? this.__path.slice() : []; }
  initEvent(type, bubbles, cancelable) {
    this.type = String(type);
    this.bubbles = !!bubbles;
    this.cancelable = !!cancelable;
  }
}
tag(Event, 'Event');
Object.assign(Event, { NONE: 0, CAPTURING_PHASE: 1, AT_TARGET: 2, BUBBLING_PHASE: 3 });

const eventClass = (name, parent, fields) =>
  tag(
    class extends parent {
      constructor(type, init) {
        super(type, init);
        const opts = init || {};
        for (const key of Object.keys(fields)) this[key] = opts[key] === undefined ? fields[key] : opts[key];
      }
    },
    name,
  ).constructor;

const named = (name, cls) => {
  Object.defineProperty(cls, 'name', { value: name });
  tag(cls, name);
  return cls;
};

const CustomEvent = named('CustomEvent', eventClass('CustomEvent', Event, { detail: null }));
const UIEvent = named('UIEvent', eventClass('UIEvent', Event, { view: null, detail: 0 }));
const mouseFields = { screenX: 0, screenY: 0, clientX: 0, clientY: 0, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, button: 0, buttons: 0, relatedTarget: null };
const MouseEvent = named('MouseEvent', eventClass('MouseEvent', UIEvent, mouseFields));
const PointerEvent = named('PointerEvent', eventClass('PointerEvent', MouseEvent, { pointerId: 0, pointerType: '', isPrimary: false }));
const WheelEvent = named('WheelEvent', eventClass('WheelEvent', MouseEvent, { deltaX: 0, deltaY: 0, deltaZ: 0, deltaMode: 0 }));
const KeyboardEvent = named('KeyboardEvent', eventClass('KeyboardEvent', UIEvent, { key: '', code: '', location: 0, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, repeat: false, isComposing: false }));
const FocusEvent = named('FocusEvent', eventClass('FocusEvent', UIEvent, { relatedTarget: null }));
const InputEvent = named('InputEvent', eventClass('InputEvent', UIEvent, { data: null, inputType: '', isComposing: false }));
const ErrorEvent = named('ErrorEvent', eventClass('ErrorEvent', Event, { message: '', filename: '', lineno: 0, colno: 0, error: null }));
const MessageEvent = named('MessageEvent', eventClass('MessageEvent', Event, { data: null, origin: '', lastEventId: '', source: null, ports: [] }));
const HashChangeEvent = named('HashChangeEvent', eventClass('HashChangeEvent', Event, { oldURL: '', newURL: '' }));
const PopStateEvent = named('PopStateEvent', eventClass('PopStateEvent', Event, { state: null }));
const ProgressEvent = named('ProgressEvent', eventClass('ProgressEvent', Event, { lengthComputable: false, loaded: 0, total: 0 }));
const TouchEvent = named('TouchEvent', eventClass('TouchEvent', UIEvent, {}));
const AnimationEvent = named('AnimationEvent', eventClass('AnimationEvent', Event, { animationName: '', elapsedTime: 0, pseudoElement: '' }));
const TransitionEvent = named('TransitionEvent', eventClass('TransitionEvent', Event, { propertyName: '', elapsedTime: 0, pseudoElement: '' }));
const PageTransitionEvent = named('PageTransitionEvent', eventClass('PageTransitionEvent', Event, { persisted: false }));
const PromiseRejectionEvent = named('PromiseRejectionEvent', eventClass('PromiseRejectionEvent', Event, { promise: undefined, reason: undefined }));
const SubmitEvent = named('SubmitEvent', eventClass('SubmitEvent', Event, { submitter: null }));

// GlobalEventHandlers do Chromium 154: os `on<tipo>` de HTMLElement, SVGElement, MathMLElement, Document e Window.
const GLOBAL_HANDLERS = ['abort', 'animationend', 'animationiteration', 'animationstart', 'auxclick', 'beforeinput', 'beforematch', 'beforetoggle', 'beforexrselect', 'blur', 'cancel', 'canplay', 'canplaythrough', 'change', 'click', 'close', 'contentvisibilityautostatechange', 'contextlost', 'contextmenu', 'contextrestored', 'copy', 'cuechange', 'cut', 'dblclick', 'drag', 'dragend', 'dragenter', 'dragleave', 'dragover', 'dragstart', 'drop', 'durationchange', 'emptied', 'ended', 'error', 'focus', 'formdata', 'gotpointercapture', 'input', 'invalid', 'keydown', 'keypress', 'keyup', 'load', 'loadeddata', 'loadedmetadata', 'loadstart', 'lostpointercapture', 'mousedown', 'mouseenter', 'mouseleave', 'mousemove', 'mouseout', 'mouseover', 'mouseup', 'mousewheel', 'paste', 'pause', 'play', 'playing', 'pointercancel', 'pointerdown', 'pointerenter', 'pointerleave', 'pointermove', 'pointerout', 'pointerover', 'pointerrawupdate', 'pointerup', 'progress', 'ratechange', 'reset', 'resize', 'scroll', 'scrollend', 'scrollsnapchange', 'scrollsnapchanging', 'securitypolicyviolation', 'seeked', 'seeking', 'select', 'selectionchange', 'selectstart', 'slotchange', 'stalled', 'submit', 'suspend', 'timeupdate', 'toggle', 'transitioncancel', 'transitionend', 'transitionrun', 'transitionstart', 'volumechange', 'waiting', 'webkitanimationend', 'webkitanimationiteration', 'webkitanimationstart', 'webkittransitionend', 'wheel'];

// Os `on<tipo>` das interfaces de evento: propriedade que guarda um único tratador por tipo.
const defineHandlerProperties = (proto, types) => {
  for (const type of types) {
    Object.defineProperty(proto, 'on' + type, {
      configurable: true,
      enumerable: true,
      get() {
        const holder = handlerStore.get(this);
        const value = holder && holder.props && holder.props.get(type);
        return value === undefined ? null : value;
      },
      set(value) {
        let holder = handlerStore.get(this);
        if (!holder) {
          holder = {};
          handlerStore.set(this, holder);
        }
        holder.props = holder.props || new Map();
        holder.props.set(type, typeof value === 'function' ? value : null);
      },
    });
  }
};
