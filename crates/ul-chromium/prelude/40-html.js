// ---- Interfaces HTML e SVG por tag, custom elements e template.

const customByName = new Map();
const customByCtor = new Map();
let upgrading = null;

class HTMLElement extends Element {
  constructor() {
    super();
    if (upgrading) {
      const target = upgrading;
      upgrading = null;
      return target;
    }
    const def = customByCtor.get(new.target);
    if (!def) throw new TypeError('Illegal constructor');
    const id = op('createElement', def.name, HTML_NS);
    ids.set(this, id);
    wrappers.set(id, this);
  }
  get contentEditable() { return this.getAttribute('contenteditable') ?? 'inherit'; }
  set contentEditable(v) { this.setAttribute('contenteditable', v); }
  get isContentEditable() { return lower(this.contentEditable) === 'true'; }
  get spellcheck() { return this.getAttribute('spellcheck') !== 'false'; }
  set spellcheck(v) { this.setAttribute('spellcheck', String(!!v)); }
  get translate() { return this.getAttribute('translate') !== 'no'; }
  set translate(v) { this.setAttribute('translate', v ? 'yes' : 'no'); }
  get inert() { return this.hasAttribute('inert'); }
  set inert(v) { this.toggleAttribute('inert', !!v); }
  get popover() { return this.getAttribute('popover'); }
  get autofocus() { return this.hasAttribute('autofocus'); }
  set autofocus(v) { this.toggleAttribute('autofocus', !!v); }
  get enterKeyHint() { return this.getAttribute('enterkeyhint') ?? ''; }
  get inputMode() { return this.getAttribute('inputmode') ?? ''; }
  get offsetParent() { return null; }
  attachInternals() { return { setFormValue() {}, setValidity() {}, checkValidity: () => true, form: null, states: new Set(), shadowRoot: null }; }
}
tag(HTMLElement, 'HTMLElement');
reflect(HTMLElement, [['title', 'title'], ['lang', 'lang'], ['dir', 'dir', 'enum'], ['hidden', 'hidden', 'bool'], ['accessKey', 'accesskey'], ['draggable', 'draggable', 'bool'], ['tabIndex', 'tabindex', 'long'], ['nonce', 'nonce']]);

const htmlByTag = new Map();
const defineHtml = (name, tags, install) => {
  const cls = { [name]: class extends HTMLElement {} }[name];
  tag(cls, name);
  for (const t of tags) htmlByTag.set(t, cls);
  if (install) install(cls);
  globalThis[name] = cls;
  return cls;
};
const plain = (name, ...tags) => defineHtml(name, tags);

class HTMLUnknownElement extends HTMLElement {}
tag(HTMLUnknownElement, 'HTMLUnknownElement');

const formOwner = (el) => el.closest('form');
const formControls = (form) => Array.from(form.querySelectorAll('input,select,textarea,button,fieldset,object,output')).filter((c) => c.closest('form') === form);

defineHtml('HTMLAnchorElement', ['a'], (cls) => {
  reflect(cls, [['href', 'href', 'url'], ['target', 'target'], ['rel', 'rel'], ['download', 'download'], ['hreflang', 'hreflang'], ['type', 'type'], ['referrerPolicy', 'referrerpolicy'], ['ping', 'ping']]);
  const part = (name) => ({
    get() { try { return new URL(this.href)[name]; } catch { return ''; } },
    set(v) { try { const u = new URL(this.href); u[name] = v; this.setAttribute('href', u.href); } catch {} },
    configurable: true, enumerable: true,
  });
  for (const name of ['origin', 'protocol', 'username', 'password', 'host', 'hostname', 'port', 'pathname', 'search', 'hash']) Object.defineProperty(cls.prototype, name, part(name));
  Object.defineProperty(cls.prototype, 'text', { get() { return this.textContent; }, set(v) { this.textContent = v; }, configurable: true });
  Object.defineProperty(cls.prototype, 'relList', { get() { return tokenList(this, 'rel'); }, configurable: true });
  cls.prototype.toString = function () { return this.href; };
});
defineHtml('HTMLImageElement', ['img'], (cls) => {
  reflect(cls, [['src', 'src', 'url'], ['alt', 'alt'], ['srcset', 'srcset'], ['sizes', 'sizes'], ['useMap', 'usemap'], ['isMap', 'ismap', 'bool'], ['loading', 'loading'], ['decoding', 'decoding'], ['crossOrigin', 'crossorigin'], ['referrerPolicy', 'referrerpolicy'], ['fetchPriority', 'fetchpriority']]);
  for (const dim of ['width', 'height']) Object.defineProperty(cls.prototype, dim, { get() { return parseInt(this.getAttribute(dim), 10) || 0; }, set(v) { this.setAttribute(dim, v); }, configurable: true, enumerable: true });
  Object.defineProperties(cls.prototype, {
    naturalWidth: { get() { return 0; }, configurable: true }, naturalHeight: { get() { return 0; }, configurable: true },
    complete: { get() { return !this.hasAttribute('src') || this.getAttribute('src') === ''; }, configurable: true },
    currentSrc: { get() { return this.src; }, configurable: true },
    x: { get() { return 0; }, configurable: true }, y: { get() { return 0; }, configurable: true },
  });
  cls.prototype.decode = function () { return Promise.resolve(); };
});
defineHtml('HTMLScriptElement', ['script'], (cls) => {
  reflect(cls, [['src', 'src', 'url'], ['type', 'type'], ['async', 'async', 'bool'], ['defer', 'defer', 'bool'], ['noModule', 'nomodule', 'bool'], ['crossOrigin', 'crossorigin'], ['integrity', 'integrity'], ['charset', 'charset'], ['referrerPolicy', 'referrerpolicy'], ['fetchPriority', 'fetchpriority']]);
  Object.defineProperty(cls.prototype, 'text', { get() { return this.textContent; }, set(v) { this.textContent = v; }, configurable: true, enumerable: true });
  cls.supports = (type) => ['classic', 'module', 'importmap'].includes(type);
});
defineHtml('HTMLLinkElement', ['link'], (cls) => {
  reflect(cls, [['href', 'href', 'url'], ['rel', 'rel'], ['as', 'as'], ['type', 'type'], ['media', 'media'], ['hreflang', 'hreflang'], ['crossOrigin', 'crossorigin'], ['integrity', 'integrity'], ['disabled', 'disabled', 'bool'], ['referrerPolicy', 'referrerpolicy'], ['imageSrcset', 'imagesrcset'], ['imageSizes', 'imagesizes']]);
  Object.defineProperty(cls.prototype, 'relList', { get() { return tokenList(this, 'rel'); }, configurable: true });
  Object.defineProperty(cls.prototype, 'sheet', { get() { return lower(this.getAttribute('rel') || '').split(/\s+/).includes('stylesheet') ? sheetFor(this) : null; }, configurable: true });
});
defineHtml('HTMLStyleElement', ['style'], (cls) => {
  reflect(cls, [['media', 'media'], ['type', 'type'], ['disabled', 'disabled', 'bool']]);
  Object.defineProperty(cls.prototype, 'sheet', { get() { return sheetFor(this); }, configurable: true });
});
defineHtml('HTMLMetaElement', ['meta'], (cls) => reflect(cls, [['name', 'name'], ['content', 'content'], ['httpEquiv', 'http-equiv'], ['charset', 'charset'], ['media', 'media']]));
defineHtml('HTMLBaseElement', ['base'], (cls) => reflect(cls, [['href', 'href', 'url'], ['target', 'target']]));
defineHtml('HTMLTitleElement', ['title'], (cls) => Object.defineProperty(cls.prototype, 'text', { get() { return this.textContent; }, set(v) { this.textContent = v; }, configurable: true }));
defineHtml('HTMLInputElement', ['input'], (cls) => {
  reflect(cls, [['name', 'name'], ['placeholder', 'placeholder'], ['disabled', 'disabled', 'bool'], ['required', 'required', 'bool'], ['readOnly', 'readonly', 'bool'], ['multiple', 'multiple', 'bool'], ['accept', 'accept'], ['autocomplete', 'autocomplete'], ['min', 'min'], ['max', 'max'], ['step', 'step'], ['pattern', 'pattern'], ['src', 'src', 'url'], ['alt', 'alt'], ['formAction', 'formaction', 'url'], ['defaultValue', 'value'], ['defaultChecked', 'checked', 'bool'], ['maxLength', 'maxlength', 'long'], ['minLength', 'minlength', 'long'], ['size', 'size', 'long']]);
  Object.defineProperties(cls.prototype, {
    type: { get() { const t = lower(this.getAttribute('type') || 'text'); return ['button', 'checkbox', 'color', 'date', 'datetime-local', 'email', 'file', 'hidden', 'image', 'month', 'number', 'password', 'radio', 'range', 'reset', 'search', 'submit', 'tel', 'text', 'time', 'url', 'week'].includes(t) ? t : 'text'; }, set(v) { this.setAttribute('type', v); }, configurable: true, enumerable: true },
    value: { get() { return this[P.__value] !== undefined ? this[P.__value] : this.getAttribute('value') ?? (this.type === 'checkbox' || this.type === 'radio' ? 'on' : ''); }, set(v) { hidden(this, '__value', toStr(v)); }, configurable: true, enumerable: true },
    checked: { get() { return this[P.__checked] !== undefined ? this[P.__checked] : this.hasAttribute('checked'); }, set(v) { hidden(this, '__checked', !!v); }, configurable: true, enumerable: true },
    form: { get() { return formOwner(this); }, configurable: true },
    labels: { get() { return staticNodeList([]); }, configurable: true },
    files: { get() { return null; }, configurable: true },
    valueAsNumber: { get() { return parseFloat(this.value); }, configurable: true },
    selectionStart: { get() { return this[P.__selStart] ?? this.value.length; }, set(v) { hidden(this, '__selStart', v); }, configurable: true },
    selectionEnd: { get() { return this[P.__selEnd] ?? this.value.length; }, set(v) { hidden(this, '__selEnd', v); }, configurable: true },
    validity: { get() { return { valid: true, valueMissing: false, typeMismatch: false, patternMismatch: false, tooLong: false, tooShort: false, rangeUnderflow: false, rangeOverflow: false, stepMismatch: false, badInput: false, customError: false }; }, configurable: true },
    validationMessage: { get() { return ''; }, configurable: true },
    willValidate: { get() { return !this.disabled; }, configurable: true },
    indeterminate: { get() { return !!this[P.__indeterminate]; }, set(v) { hidden(this, '__indeterminate', !!v); }, configurable: true },
  });
  Object.assign(cls.prototype, { select() {}, setSelectionRange(a, b) { hidden(this, '__selStart', a); hidden(this, '__selEnd', b); }, setCustomValidity() {}, checkValidity() { return true; }, reportValidity() { return true; }, stepUp() {}, stepDown() {}, showPicker() {} });
});
defineHtml('HTMLTextAreaElement', ['textarea'], (cls) => {
  reflect(cls, [['name', 'name'], ['placeholder', 'placeholder'], ['disabled', 'disabled', 'bool'], ['required', 'required', 'bool'], ['readOnly', 'readonly', 'bool'], ['rows', 'rows', 'long'], ['cols', 'cols', 'long'], ['maxLength', 'maxlength', 'long'], ['wrap', 'wrap']]);
  Object.defineProperties(cls.prototype, {
    defaultValue: { get() { return this.textContent; }, set(v) { this.textContent = v; }, configurable: true },
    value: { get() { return this[P.__value] !== undefined ? this[P.__value] : this.textContent; }, set(v) { hidden(this, '__value', toStr(v)); }, configurable: true, enumerable: true },
    type: { get() { return 'textarea'; }, configurable: true },
    form: { get() { return formOwner(this); }, configurable: true },
    textLength: { get() { return this.value.length; }, configurable: true },
    selectionStart: { get() { return this[P.__selStart] ?? this.value.length; }, set(v) { hidden(this, '__selStart', v); }, configurable: true },
    selectionEnd: { get() { return this[P.__selEnd] ?? this.value.length; }, set(v) { hidden(this, '__selEnd', v); }, configurable: true },
  });
  Object.assign(cls.prototype, { select() {}, setSelectionRange(a, b) { hidden(this, '__selStart', a); hidden(this, '__selEnd', b); }, setCustomValidity() {}, checkValidity() { return true; }, reportValidity() { return true; } });
});
defineHtml('HTMLSelectElement', ['select'], (cls) => {
  reflect(cls, [['name', 'name'], ['disabled', 'disabled', 'bool'], ['required', 'required', 'bool'], ['multiple', 'multiple', 'bool'], ['size', 'size', 'long']]);
  Object.defineProperties(cls.prototype, {
    options: { get() { const self = this; return liveList(HTMLCollection.prototype, () => Array.from(self.querySelectorAll('option'))); }, configurable: true },
    selectedOptions: { get() { const self = this; return liveList(HTMLCollection.prototype, () => Array.from(self.querySelectorAll('option')).filter((o) => o.selected)); }, configurable: true },
    selectedIndex: {
      get() { const opts = Array.from(this.querySelectorAll('option')); const i = opts.findIndex((o) => o.selected); return i >= 0 ? i : opts.length && !this.multiple ? 0 : -1; },
      set(v) { Array.from(this.querySelectorAll('option')).forEach((o, i) => { o.selected = i === v; }); }, configurable: true,
    },
    value: {
      get() { const o = this.options[this.selectedIndex]; return o ? o.value : ''; },
      set(v) { let found = false; for (const o of Array.from(this.querySelectorAll('option'))) { o.selected = !found && o.value === String(v); found = found || o.selected; } }, configurable: true, enumerable: true,
    },
    type: { get() { return this.multiple ? 'select-multiple' : 'select-one'; }, configurable: true },
    form: { get() { return formOwner(this); }, configurable: true },
    length: { get() { return this.querySelectorAll('option').length; }, configurable: true },
  });
  Object.assign(cls.prototype, { add(option, before) { this.insertBefore(option, before ?? null); }, item(i) { return this.options[i] || null; }, checkValidity() { return true; }, reportValidity() { return true; }, setCustomValidity() {} });
});
defineHtml('HTMLOptionElement', ['option'], (cls) => {
  reflect(cls, [['disabled', 'disabled', 'bool'], ['label', 'label'], ['defaultSelected', 'selected', 'bool']]);
  Object.defineProperties(cls.prototype, {
    value: { get() { return this.getAttribute('value') ?? this.text; }, set(v) { this.setAttribute('value', v); }, configurable: true, enumerable: true },
    text: { get() { return this.textContent.replace(/[ \t\n\f\r]+/g, ' ').trim(); }, set(v) { this.textContent = v; }, configurable: true },
    selected: { get() { return this[P.__selected] !== undefined ? this[P.__selected] : this.hasAttribute('selected'); }, set(v) { hidden(this, '__selected', !!v); }, configurable: true, enumerable: true },
    index: { get() { const s = this.closest('select'); return s ? Array.from(s.querySelectorAll('option')).indexOf(this) : 0; }, configurable: true },
    form: { get() { return formOwner(this); }, configurable: true },
  });
});
defineHtml('HTMLButtonElement', ['button'], (cls) => {
  reflect(cls, [['name', 'name'], ['value', 'value'], ['disabled', 'disabled', 'bool'], ['formAction', 'formaction', 'url']]);
  Object.defineProperties(cls.prototype, {
    type: { get() { const t = lower(this.getAttribute('type') || 'submit'); return ['button', 'reset', 'submit'].includes(t) ? t : 'submit'; }, set(v) { this.setAttribute('type', v); }, configurable: true, enumerable: true },
    form: { get() { return formOwner(this); }, configurable: true },
  });
});
defineHtml('HTMLFormElement', ['form'], (cls) => {
  reflect(cls, [['name', 'name'], ['target', 'target'], ['action', 'action', 'url'], ['acceptCharset', 'accept-charset'], ['autocomplete', 'autocomplete'], ['noValidate', 'novalidate', 'bool'], ['enctype', 'enctype'], ['encoding', 'enctype']]);
  Object.defineProperties(cls.prototype, {
    method: { get() { const m = lower(this.getAttribute('method') || 'get'); return m === 'post' || m === 'dialog' ? m : 'get'; }, set(v) { this.setAttribute('method', v); }, configurable: true, enumerable: true },
    elements: { get() { const self = this; return liveList(HTMLCollection.prototype, () => formControls(self)); }, configurable: true },
    length: { get() { return formControls(this).length; }, configurable: true },
  });
  Object.assign(cls.prototype, {
    submit() {}, reset() { this.dispatchEvent(new Event('reset', { bubbles: true, cancelable: true })); },
    requestSubmit() { this.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true })); },
    checkValidity() { return true; }, reportValidity() { return true; },
  });
});
defineHtml('HTMLLabelElement', ['label'], (cls) => {
  reflect(cls, [['htmlFor', 'for']]);
  Object.defineProperty(cls.prototype, 'control', { get() { const f = this.getAttribute('for'); return f ? document.getElementById(f) : this.querySelector('input,select,textarea,button'); }, configurable: true });
});
defineHtml('HTMLTemplateElement', ['template'], (cls) => {
  Object.defineProperty(cls.prototype, 'content', {
    get() {
      if (!this[P.__content]) {
        const content = Object.create(TemplateContent.prototype);
        ids.set(content, idOf(this));
        hidden(content, '__template', this);
        hidden(this, '__content', content);
      }
      return this[P.__content];
    },
    configurable: true,
  });
  Object.defineProperty(cls.prototype, 'innerHTML', { get() { return op('innerHTML', idOf(this)); }, set(v) { op('setInnerHTML', idOf(this), toStr(v)); }, configurable: true });
});
// `template.content` compartilha o NodeId do template: os filhos do template SÃO o conteúdo.
class TemplateContent extends DocumentFragment {
  get nodeType() { return 11; }
  get nodeName() { return '#document-fragment'; }
  cloneNode(deep) {
    const fragment = document.createDocumentFragment();
    for (const child of Array.from(this.childNodes)) fragment.appendChild(child.cloneNode(!!deep));
    return fragment;
  }
  [P.__flatten]() {
    const fragment = document.createDocumentFragment();
    for (const child of Array.from(this.childNodes)) fragment.appendChild(child);
    return fragment;
  }
}
tag(TemplateContent, 'DocumentFragment');
defineHtml('HTMLIFrameElement', ['iframe'], (cls) => {
  reflect(cls, [['src', 'src', 'url'], ['srcdoc', 'srcdoc'], ['name', 'name'], ['allow', 'allow'], ['allowFullscreen', 'allowfullscreen', 'bool'], ['loading', 'loading'], ['referrerPolicy', 'referrerpolicy']]);
  Object.defineProperties(cls.prototype, { contentWindow: { get() { return null; }, configurable: true }, contentDocument: { get() { return null; }, configurable: true } });
});
defineHtml('HTMLCanvasElement', ['canvas'], (cls) => {
  for (const [dim, def] of [['width', 300], ['height', 150]]) Object.defineProperty(cls.prototype, dim, { get() { const n = parseInt(this.getAttribute(dim), 10); return Number.isFinite(n) ? n : def; }, set(v) { this.setAttribute(dim, v); }, configurable: true, enumerable: true });
  Object.assign(cls.prototype, { getContext() { return null; }, toDataURL() { return 'data:,'; }, toBlob(cb) { cb(null); }, transferControlToOffscreen() { throw new TypeError('not supported'); } });
});
for (const [name, tags] of [['HTMLVideoElement', ['video']], ['HTMLAudioElement', ['audio']]]) {
  defineHtml(name, tags, (cls) => {
    reflect(cls, [['src', 'src', 'url'], ['autoplay', 'autoplay', 'bool'], ['loop', 'loop', 'bool'], ['muted', 'muted', 'bool'], ['controls', 'controls', 'bool'], ['preload', 'preload'], ['poster', 'poster', 'url'], ['crossOrigin', 'crossorigin']]);
    Object.defineProperties(cls.prototype, { paused: { get() { return true; }, configurable: true }, ended: { get() { return false; }, configurable: true }, currentTime: { get() { return 0; }, set() {}, configurable: true }, duration: { get() { return NaN; }, configurable: true }, readyState: { get() { return 0; }, configurable: true }, volume: { get() { return 1; }, set() {}, configurable: true } });
    Object.assign(cls.prototype, { play() { return Promise.reject(domException('The play() request was interrupted because the media has no source.', 'NotSupportedError')); }, pause() {}, load() {}, canPlayType() { return ''; } });
  });
}
defineHtml('HTMLSourceElement', ['source'], (cls) => reflect(cls, [['src', 'src', 'url'], ['type', 'type'], ['srcset', 'srcset'], ['sizes', 'sizes'], ['media', 'media']]));
defineHtml('HTMLProgressElement', ['progress'], (cls) => reflect(cls, [['value', 'value'], ['max', 'max']]));
defineHtml('HTMLDetailsElement', ['details'], (cls) => reflect(cls, [['open', 'open', 'bool'], ['name', 'name']]));
defineHtml('HTMLDialogElement', ['dialog'], (cls) => {
  reflect(cls, [['open', 'open', 'bool']]);
  Object.assign(cls.prototype, { show() { this.setAttribute('open', ''); }, showModal() { this.setAttribute('open', ''); }, close(v) { this.removeAttribute('open'); this.dispatchEvent(new Event('close')); } });
});
defineHtml('HTMLSlotElement', ['slot'], (cls) => {
  reflect(cls, [['name', 'name']]);
  // Atribuição manual por nome: o slot nomeado recebe os filhos do host com `slot` igual; o padrão, os sem `slot`.
  const assigned = (slot) => {
    let root = slot.parentNode;
    while (root && !(root instanceof ShadowRoot)) root = root.parentNode;
    if (!root) return [];
    const name = slot.getAttribute('name') || '';
    return Array.from(root[P.__host].childNodes).filter((n) => (n.nodeType === 1 ? (n.getAttribute('slot') || '') === name : n.nodeType === 3 && name === ''));
  };
  Object.assign(cls.prototype, { assignedNodes() { return assigned(this); }, assignedElements() { return assigned(this).filter((n) => n.nodeType === 1); } });
});
defineHtml('HTMLTableElement', ['table']);
defineHtml('HTMLTableRowElement', ['tr'], (cls) => Object.defineProperty(cls.prototype, 'cells', { get() { const self = this; return liveList(HTMLCollection.prototype, () => Array.from(self.children).filter((c) => /^(td|th)$/i.test(c.localName))); }, configurable: true }));
defineHtml('HTMLTableCellElement', ['td', 'th'], (cls) => reflect(cls, [['colSpan', 'colspan', 'long'], ['rowSpan', 'rowspan', 'long'], ['headers', 'headers'], ['scope', 'scope'], ['abbr', 'abbr']]));
defineHtml('HTMLTableSectionElement', ['tbody', 'thead', 'tfoot']);
defineHtml('HTMLTableColElement', ['col', 'colgroup']);
defineHtml('HTMLTableCaptionElement', ['caption']);
defineHtml('HTMLOListElement', ['ol'], (cls) => reflect(cls, [['reversed', 'reversed', 'bool'], ['start', 'start', 'long'], ['type', 'type']]));
defineHtml('HTMLLIElement', ['li'], (cls) => reflect(cls, [['value', 'value', 'long']]));
defineHtml('HTMLHeadingElement', ['h1', 'h2', 'h3', 'h4', 'h5', 'h6']);
defineHtml('HTMLModElement', ['ins', 'del'], (cls) => reflect(cls, [['cite', 'cite', 'url'], ['dateTime', 'datetime']]));
defineHtml('HTMLQuoteElement', ['blockquote', 'q'], (cls) => reflect(cls, [['cite', 'cite', 'url']]));
defineHtml('HTMLObjectElement', ['object'], (cls) => reflect(cls, [['data', 'data', 'url'], ['type', 'type'], ['name', 'name']]));
defineHtml('HTMLEmbedElement', ['embed'], (cls) => reflect(cls, [['src', 'src', 'url'], ['type', 'type']]));
defineHtml('HTMLFieldSetElement', ['fieldset'], (cls) => reflect(cls, [['disabled', 'disabled', 'bool'], ['name', 'name']]));
defineHtml('HTMLOutputElement', ['output']);
defineHtml('HTMLMeterElement', ['meter'], (cls) => reflect(cls, [['value', 'value'], ['min', 'min'], ['max', 'max'], ['low', 'low'], ['high', 'high'], ['optimum', 'optimum']]));
defineHtml('HTMLTimeElement', ['time'], (cls) => reflect(cls, [['dateTime', 'datetime']]));
defineHtml('HTMLDataElement', ['data'], (cls) => reflect(cls, [['value', 'value']]));
defineHtml('HTMLAreaElement', ['area'], (cls) => reflect(cls, [['href', 'href', 'url'], ['alt', 'alt'], ['target', 'target'], ['shape', 'shape'], ['coords', 'coords']]));
defineHtml('HTMLMapElement', ['map'], (cls) => reflect(cls, [['name', 'name']]));
defineHtml('HTMLTrackElement', ['track'], (cls) => reflect(cls, [['src', 'src', 'url'], ['kind', 'kind'], ['srclang', 'srclang'], ['label', 'label'], ['default', 'default', 'bool']]));
defineHtml('HTMLPictureElement', ['picture']);
defineHtml('HTMLHtmlElement', ['html']);
defineHtml('HTMLHeadElement', ['head']);
defineHtml('HTMLBodyElement', ['body']);
defineHtml('HTMLDivElement', ['div']);
defineHtml('HTMLSpanElement', ['span']);
defineHtml('HTMLParagraphElement', ['p']);
defineHtml('HTMLPreElement', ['pre']);
defineHtml('HTMLBRElement', ['br']);
defineHtml('HTMLHRElement', ['hr']);
defineHtml('HTMLUListElement', ['ul']);
defineHtml('HTMLDListElement', ['dl']);
defineHtml('HTMLMenuElement', ['menu']);
defineHtml('HTMLLegendElement', ['legend']);
defineHtml('HTMLOptGroupElement', ['optgroup'], (cls) => reflect(cls, [['disabled', 'disabled', 'bool'], ['label', 'label']]));
defineHtml('HTMLDataListElement', ['datalist']);
defineHtml('HTMLParamElement', ['param']);
defineHtml('HTMLFrameSetElement', ['frameset']);

// `className` de SVG é um SVGAnimatedString vivo sobre o atributo `class`.
class SVGAnimatedString {
  constructor() { throw new TypeError('Illegal constructor'); }
  get baseVal() { return this[P.__element].getAttribute(this[P.__attr]) ?? ''; }
  set baseVal(v) { this[P.__element].setAttribute(this[P.__attr], v); }
  get animVal() { return this.baseVal; }
}
tag(SVGAnimatedString, 'SVGAnimatedString');
const animatedString = (element, attr) => { const s = Object.create(SVGAnimatedString.prototype); hidden(s, '__element', element); hidden(s, '__attr', attr); return s; };

class SVGElement extends Element {
  get ownerSVGElement() { return this.closest('svg'); }
  get viewportElement() { return this.closest('svg'); }
  get className() { return animatedString(this, 'class'); }
  get style() { return styleOf(this); }
}
tag(SVGElement, 'SVGElement');
reflect(SVGElement, [['tabIndex', 'tabindex', 'long'], ['nonce', 'nonce']]);
class SVGGraphicsElement extends SVGElement {
  getBBox() { return new DOMRect(0, 0, 0, 0); }
  getCTM() { return null; }
  getScreenCTM() { return null; }
}
tag(SVGGraphicsElement, 'SVGGraphicsElement');
class SVGSVGElement extends SVGGraphicsElement {
  createSVGPoint() { return { x: 0, y: 0 }; }
  createSVGRect() { return new DOMRect(); }
  createSVGMatrix() { return { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 }; }
  createSVGNumber() { return { value: 0 }; }
  get viewBox() { return { baseVal: { x: 0, y: 0, width: 0, height: 0 } }; }
}
tag(SVGSVGElement, 'SVGSVGElement');
class MathMLElement extends Element {}
tag(MathMLElement, 'MathMLElement');

function elementPrototype(local, ns) {
  if (ns === HTML_NS) {
    const cls = htmlByTag.get(local);
    if (cls) return cls.prototype;
    return (local.includes('-') ? HTMLElement : HTMLUnknownElement).prototype;
  }
  if (ns === SVG_NS) return (local === 'svg' ? SVGSVGElement : /^(svg|g|path|rect|circle|ellipse|line|polyline|polygon|text|tspan|use|image|foreignObject)$/.test(local) ? SVGGraphicsElement : SVGElement).prototype;
  if (ns === MATHML_NS) return MathMLElement.prototype;
  return Element.prototype;
}

// ---- Custom elements.
class CustomElementRegistry {
  constructor() { throw new TypeError('Illegal constructor'); }
  define(name, ctor, options) {
    name = String(name);
    if (!/^[a-z][-.0-9_a-z]*-[-.0-9_a-z]*$/.test(name)) throw domException(`Failed to execute 'define' on 'CustomElementRegistry': "${name}" is not a valid custom element name`, 'SyntaxError');
    if (customByName.has(name)) throw domException(`Failed to execute 'define' on 'CustomElementRegistry': the name "${name}" has already been used with this registry`, 'NotSupportedError');
    if (customByCtor.has(ctor)) throw domException("Failed to execute 'define' on 'CustomElementRegistry': this constructor has already been used with this registry", 'NotSupportedError');
    const def = { name, ctor, observed: new Set(ctor.observedAttributes || []), waiters: [] };
    customByName.set(name, def);
    customByCtor.set(ctor, def);
    for (const el of Array.from(document.querySelectorAll(name))) upgradeElement(el, def);
    for (const resolve of def.waiters) resolve(ctor);
  }
  get(name) { const def = customByName.get(String(name)); return def ? def.ctor : undefined; }
  getName(ctor) { const def = customByCtor.get(ctor); return def ? def.name : null; }
  whenDefined(name) { const def = customByName.get(String(name)); return def ? Promise.resolve(def.ctor) : new Promise((resolve) => { const w = this[P.__waiting] || (this[P.__waiting] = new Map()); (w.get(name) || w.set(name, []).get(name)).push(resolve); this[P.__pending] = true; }); }
  upgrade(root) { for (const el of Array.from(root.querySelectorAll('*'))) { const def = customByName.get(el.localName); if (def) upgradeElement(el, def); } }
}
tag(CustomElementRegistry, 'CustomElementRegistry');
const upgradeElement = (el, def) => {
  if (el[P.__upgraded] || !(el instanceof HTMLElement)) return;
  hidden(el, '__upgraded', true);
  Object.setPrototypeOf(el, def.ctor.prototype);
  upgrading = el;
  try { new def.ctor(); } catch (error) { upgrading = null; reportError(error); return; }
  op('setDefined', idOf(el));
  if (def.observed.size && typeof el.attributeChangedCallback === 'function') {
    for (const name of attrNames(idOf(el))) if (def.observed.has(name)) invokeReaction(() => el.attributeChangedCallback(name, null, el.getAttribute(name)));
  }
  if (el.isConnected && typeof el.connectedCallback === 'function') invokeReaction(() => el.connectedCallback());
};
const invokeReaction = (fn) => { try { fn(); } catch (error) { reportError(error); } };
const customElements = Object.create(CustomElementRegistry.prototype);
const originalDefine = CustomElementRegistry.prototype.define;
CustomElementRegistry.prototype.define = function (name, ctor, options) {
  originalDefine.call(this, name, ctor, options);
  const waiting = this[P.__waiting] && this[P.__waiting].get(String(name));
  if (waiting) for (const resolve of waiting) resolve(ctor);
};

// Chamado depois de um nó entrar na árvore: upgrade de custom elements, `connectedCallback` e scripts dinâmicos.
function afterInsert(node) {
  if (node.nodeType !== 1) return;
  const connected = node.isConnected;
  const walk = (el) => {
    const def = customByName.get(el.localName);
    if (def && !el[P.__upgraded] && connected) upgradeElement(el, def);
    else if (def && el[P.__upgraded] && connected && typeof el.connectedCallback === 'function') invokeReaction(() => el.connectedCallback());
    if (connected && el.localName === 'script') runDynamicScript(el);
    if (connected && el.localName === 'link') loadLink(el);
    for (const child of Array.from(el.children)) walk(child);
  };
  walk(node);
}

// Chamado depois de um nó conectado sair da árvore: `disconnectedCallback` dos custom elements da subárvore.
function afterRemove(node) {
  if (node.nodeType !== 1) return;
  const walk = (el) => {
    if (el[P.__upgraded] && typeof el.disconnectedCallback === 'function') invokeReaction(() => el.disconnectedCallback());
    for (const child of Array.from(el.children)) walk(child);
  };
  walk(node);
}

// O `<link>` que a página insere e que o Chromium busca (folha de estilo, preload, modulepreload) dispara `load` ou
// `error` quando a resposta chega. Só a primeira inserção busca.
function loadLink(link) {
  if (link[P.__linkStarted]) return;
  const rel = lower(link.getAttribute('rel') || '').split(/[ \t\n\f\r]+/);
  const href = link.getAttribute('href');
  if (href === null || href === '' || !rel.some((r) => r === 'stylesheet' || r === 'preload' || r === 'modulepreload')) return;
  hidden(link, '__linkStarted', true);
  const url = resolveUrl(href);
  setTimeout(() => {
    const result = netFetch('GET', url, [], null);
    const ok = result !== null && result[0] >= 200 && result[0] < 300;
    link.dispatchEvent(new Event(ok ? 'load' : 'error'));
  }, NETWORK_LATENCY_MS);
}

// A imagem cuja `src` a página define é buscada (conectada ou não, como no Chromium) e dispara `load` ou `error`
// quando a resposta chega; a última `src` vence.
function loadImage(img, src) {
  const url = src === '' ? '' : resolveUrl(src);
  hidden(img, '__imageUrl', url);
  setTimeout(() => {
    if (img[P.__imageUrl] !== url) return;
    const result = url === '' ? null : netFetch('GET', url, [], null);
    const ok = result !== null && result[0] >= 200 && result[0] < 300;
    img.dispatchEvent(new Event(ok ? 'load' : 'error'));
  }, NETWORK_LATENCY_MS);
}

function attributeChanged(el, name, oldValue, newValue) {
  if (name === 'src' && el.localName === 'img' && el.namespaceURI === HTML_NS && newValue !== null) loadImage(el, newValue);
  mutationHook.record({ type: 'attributes', target: el, attributeName: name, oldValue });
  const def = customByCtor.get(el.constructor);
  if (def && def.observed.has(name) && typeof el.attributeChangedCallback === 'function') invokeReaction(() => el.attributeChangedCallback(name, oldValue, newValue));
}
