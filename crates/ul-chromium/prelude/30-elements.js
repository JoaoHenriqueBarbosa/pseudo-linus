// ---- Element, atributos, classList, style, dataset e as interfaces HTML/SVG.

const attrNames = (id) => { const flat = op('attrs', id); const out = []; for (let i = 0; i < flat.length; i += 2) out.push(flat[i]); return out; };

class Attr extends Node {
  constructor() { super(ILLEGAL); }
  get name() { return this[P.__name]; }
  get localName() { return this[P.__name]; }
  get namespaceURI() { return null; }
  get prefix() { return null; }
  get value() { return this[P.__owner] ? this[P.__owner].getAttribute(this[P.__name]) ?? '' : this[P.__value]; }
  set value(v) { if (this[P.__owner]) this[P.__owner].setAttribute(this[P.__name], v); else this[P.__value] = String(v); }
  get nodeValue() { return this.value; }
  get textContent() { return this.value; }
  get ownerElement() { return this[P.__owner] || null; }
  get specified() { return true; }
  get nodeType() { return 2; }
  get nodeName() { return this[P.__name]; }
}
tag(Attr, 'Attr');
const makeAttr = (owner, name, value) => {
  const attr = Object.create(Attr.prototype);
  ids.set(attr, -1);
  hidden(attr, '__owner', owner);
  hidden(attr, '__name', name);
  hidden(attr, '__value', value);
  return attr;
};

class NamedNodeMap {
  constructor() { throw new TypeError('Illegal constructor'); }
  get length() { return attrNames(idOf(this[P.__owner])).length; }
  item(i) { const names = attrNames(idOf(this[P.__owner])); return i < names.length ? this.getNamedItem(names[i]) : null; }
  getNamedItem(name) { const v = this[P.__owner].getAttribute(name); return v === null ? null : makeAttr(this[P.__owner], lower(name) === name ? name : name, v); }
  setNamedItem(attr) { this[P.__owner].setAttribute(attr.name, attr.value); return null; }
  removeNamedItem(name) { const a = this.getNamedItem(name); if (!a) throw domException('Failed to execute \'removeNamedItem\' on \'NamedNodeMap\': No item with name \'' + name + '\' was found.', 'NotFoundError'); this[P.__owner].removeAttribute(name); return a; }
  [Symbol.iterator]() { return Array.from({ length: this.length }, (_, i) => this.item(i))[Symbol.iterator](); }
}
tag(NamedNodeMap, 'NamedNodeMap');
const attributeMap = (owner) => {
  const target = Object.create(NamedNodeMap.prototype);
  hidden(target, '__owner', owner);
  return proxied(target, {
    get(t, key, receiver) {
      if (isIndex(key)) return t.item(+key) ?? undefined;
      if (typeof key === 'string' && !(key in t)) { const a = t.getNamedItem(key); if (a) return a; }
      return Reflect.get(t, key, receiver);
    },
    has(t, key) { return isIndex(key) ? +key < t.length : Reflect.has(t, key); },
  });
};

class DOMTokenList {
  constructor() { throw new TypeError('Illegal constructor'); }
  [P.__tokens]() { const v = this[P.__element].getAttribute(this[P.__attr]); return v ? v.split(/[ \t\n\f\r]+/).filter(Boolean) : []; }
  // Os passos de atualização: sem o atributo e com o conjunto vazio, nada se escreve (nada de `class=""`); o conjunto
  // é ordenado e sem repetição.
  [P.__write](tokens) {
    const unique = tokens.filter((t, i) => tokens.indexOf(t) === i);
    if (unique.length === 0 && !this[P.__element].hasAttribute(this[P.__attr])) return;
    this[P.__element].setAttribute(this[P.__attr], unique.join(' '));
  }
  [P.__check](token, method) {
    token = String(token);
    if (token === '') throw domException(`Failed to execute '${method}' on 'DOMTokenList': The token provided must not be empty.`, 'SyntaxError');
    if (/[ \t\n\f\r]/.test(token)) throw domException(`Failed to execute '${method}' on 'DOMTokenList': The token provided ('${token}') contains HTML space characters, which are not valid in tokens.`, 'InvalidCharacterError');
    return token;
  }
  get length() { return this[P.__tokens]().length; }
  get value() { return this[P.__element].getAttribute(this[P.__attr]) ?? ''; }
  set value(v) { this[P.__element].setAttribute(this[P.__attr], v); }
  item(i) { return this[P.__tokens]()[i] ?? null; }
  contains(token) { return this[P.__tokens]().includes(String(token)); }
  add(...tokens) {
    const list = this[P.__tokens]();
    for (const t of tokens) { const token = this[P.__check](t, 'add'); if (!list.includes(token)) list.push(token); }
    this[P.__write](list);
  }
  remove(...tokens) {
    const set = new Set(tokens.map((t) => this[P.__check](t, 'remove')));
    this[P.__write](this[P.__tokens]().filter((t) => !set.has(t)));
  }
  toggle(token, force) {
    token = this[P.__check](token, 'toggle');
    const has = this.contains(token);
    if (has && force !== true) { this.remove(token); return false; }
    if (!has && force !== false) { this.add(token); return true; }
    return has;
  }
  replace(oldToken, newToken) {
    const list = this[P.__tokens]();
    const i = list.indexOf(String(oldToken));
    if (i < 0) return false;
    list[i] = this[P.__check](newToken, 'replace');
    this[P.__write](list);
    return true;
  }
  supports() { return true; }
  forEach(cb, thisArg) { this[P.__tokens]().forEach((t, i) => cb.call(thisArg, t, i, this)); }
  entries() { return this[P.__tokens]().entries(); }
  keys() { return this[P.__tokens]().keys(); }
  values() { return this[P.__tokens]().values(); }
  toString() { return this.value; }
  [Symbol.iterator]() { return this[P.__tokens]().values(); }
}
tag(DOMTokenList, 'DOMTokenList');
const tokenList = (element, attr) => {
  const target = Object.create(DOMTokenList.prototype);
  hidden(target, '__element', element);
  hidden(target, '__attr', attr);
  return proxied(target, { get(t, key, receiver) { return isIndex(key) ? t.item(+key) ?? undefined : Reflect.get(t, key, receiver); } });
};

// CSSStyleDeclaration sobre o atributo `style`: lê e escreve `prop: valor;` e serializa como o Chromium.
const kebab = (name) => name.replace(/[A-Z]/g, (c) => '-' + c.toLowerCase());
const parseDeclarations = (text) => {
  const out = [];
  let depth = 0, quote = '', start = 0;
  const flush = (end) => {
    const part = text.slice(start, end);
    const colon = part.indexOf(':');
    if (colon > 0) {
      let name = part.slice(0, colon).trim();
      let value = part.slice(colon + 1).trim();
      let priority = '';
      const m = /\s*!\s*important$/i.exec(value);
      if (m) { priority = 'important'; value = value.slice(0, m.index).trim(); }
      if (name && value) { if (!name.startsWith('--')) name = name.toLowerCase(); out.push({ name, value, priority }); }
    }
  };
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quote) { if (c === '\\') i++; else if (c === quote) quote = ''; }
    else if (c === '"' || c === "'") quote = c;
    else if (c === '(') depth++;
    else if (c === ')') depth--;
    else if (c === ';' && depth === 0) { flush(i); start = i + 1; }
  }
  flush(text.length);
  return out;
};
class CSSStyleDeclaration {
  constructor() { throw new TypeError('Illegal constructor'); }
  // O bloco de declarações é o do motor de CSS: o atributo lido passa pela serialização canônica (a que o Chromium
  // mostra em `cssText` e grava no atributo depois de uma escrita via CSSOM). A escrita do CSSOM vai pela ponte
  // (`setStyle`), que só cria o atributo quando alguém o lê, como o Blink.
  [P.__list]() { const raw = op('getStyle', idOf(this[P.__element])); return raw ? parseDeclarations(op('normalizeStyle', raw)) : []; }
  [P.__save](list) {
    const raw = op('getStyle', idOf(this[P.__element]));
    if (list.length === 0 && raw === null) return;
    const text = op('normalizeStyle', list.map((d) => `${d.name}: ${d.value}${d.priority ? ' !important' : ''};`).join(' '));
    // O Blink só regrava o atributo quando o bloco muda: a mesma declaração de novo deixa o texto original intacto.
    if (raw !== null && op('normalizeStyle', raw) === text) return;
    this[P.__write](text);
  }
  [P.__write](text) {
    const element = this[P.__element];
    const old = op('getStyle', idOf(element));
    op('setStyle', idOf(element), text);
    attributeChanged(element, 'style', old, text);
  }
  get cssText() { const raw = op('getStyle', idOf(this[P.__element])); return raw ? op('normalizeStyle', raw) : ''; }
  set cssText(v) { this[P.__write](op('normalizeStyle', String(v))); }
  get length() { return this[P.__list]().length; }
  item(i) { const l = this[P.__list](); return i < l.length ? l[i].name : ''; }
  getPropertyValue(name) { const n = String(name); const d = this[P.__list]().find((x) => x.name === (n.startsWith('--') ? n : n.toLowerCase())); return d ? d.value : ''; }
  getPropertyPriority(name) { const d = this[P.__list]().find((x) => x.name === lower(name)); return d ? d.priority : ''; }
  setProperty(name, value, priority = '') {
    name = String(name);
    if (!name.startsWith('--')) name = name.toLowerCase();
    const list = this[P.__list]();
    const i = list.findIndex((d) => d.name === name);
    if (value === null || value === undefined || String(value) === '') { if (i >= 0) list.splice(i, 1); }
    // Valor inválido não muda a declaração que já existe.
    else if (op('normalizeStyle', `${name}: ${String(value)};`) === '') return;
    else if (i >= 0) list[i] = { name, value: String(value), priority: priority === 'important' ? 'important' : '' };
    else list.push({ name, value: String(value), priority: priority === 'important' ? 'important' : '' });
    this[P.__save](list);
  }
  removeProperty(name) {
    const old = this.getPropertyValue(name);
    this.setProperty(name, '');
    return old;
  }
  [Symbol.iterator]() { return this[P.__list]().map((d) => d.name).values(); }
  get parentRule() { return null; }
}
tag(CSSStyleDeclaration, 'CSSStyleDeclaration');
const styleOf = (element) => {
  const target = Object.create(CSSStyleDeclaration.prototype);
  hidden(target, '__element', element);
  return proxied(target, {
    get(t, key, receiver) {
      if (typeof key === 'string' && !(key in t)) {
        if (isIndex(key)) return t.item(+key);
        if (key === 'cssFloat') return t.getPropertyValue('float');
        return t.getPropertyValue(kebab(key));
      }
      return Reflect.get(t, key, receiver);
    },
    set(t, key, value) {
      if (typeof key === 'string' && !(key in t)) { t.setProperty(key === 'cssFloat' ? 'float' : kebab(key), value); return true; }
      return Reflect.set(t, key, value, t);
    },
  });
};

const datasetOf = (element) => {
  const toAttr = (key) => 'data-' + key.replace(/[A-Z]/g, (c) => '-' + c.toLowerCase());
  const toKey = (attr) => attr.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
  return new Proxy(Object.create(null), {
    get: (t, key) => (typeof key === 'string' ? element.getAttribute(toAttr(key)) ?? undefined : undefined),
    set: (t, key, value) => { element.setAttribute(toAttr(String(key)), toStr(value)); return true; },
    deleteProperty: (t, key) => { element.removeAttribute(toAttr(String(key))); return true; },
    has: (t, key) => typeof key === 'string' && element.hasAttribute(toAttr(key)),
    ownKeys: () => attrNames(idOf(element)).filter((n) => n.startsWith('data-')).map(toKey),
    getOwnPropertyDescriptor: (t, key) => (typeof key === 'string' && element.hasAttribute(toAttr(key)) ? { value: element.getAttribute(toAttr(key)), writable: true, enumerable: true, configurable: true } : undefined),
  });
};

const parseFragment = (context, html) => {
  const holder = document.createElement(context && context.nodeType === 1 ? context.localName : 'div');
  op('setInnerHTML', idOf(holder), String(html));
  return holder;
};

// Troca os filhos pelo que o parser faz do texto: o miolo de `innerHTML` e de `setHTMLUnsafe`.
const replaceChildrenWithHtml = (element, html) => {
  const removed = Array.from(element.childNodes);
  op('setInnerHTML', idOf(element), toStr(html));
  mutationHook.record({ type: 'childList', target: element, removedNodes: removed, addedNodes: Array.from(element.childNodes) });
  for (const child of Array.from(element.childNodes)) afterInsert(child);
};

class Element extends Node {
  constructor() { super(ILLEGAL); }
  get namespaceURI() { return op('ns', idOf(this)); }
  get localName() { return op('name', idOf(this)); }
  get prefix() { return null; }
  get tagName() {
    const name = this.localName;
    return this.namespaceURI === HTML_NS ? name.toUpperCase() : name;
  }
  get id() { return this.getAttribute('id') ?? ''; }
  set id(v) { this.setAttribute('id', v); }
  get className() { return this.getAttribute('class') ?? ''; }
  set className(v) { this.setAttribute('class', v); }
  get classList() { return tokenList(this, 'class'); }
  set classList(v) { this.setAttribute('class', v); }
  get slot() { return this.getAttribute('slot') ?? ''; }
  set slot(v) { this.setAttribute('slot', v); }
  get attributes() { return attributeMap(this); }
  getAttribute(name) { return op('getAttr', idOf(this), this[P.__attrName](name)); }
  [P.__attrName](name) { name = toStr(name); return this.namespaceURI === HTML_NS && this.ownerDocument[P.__html] ? name.toLowerCase() : name; }
  getAttributeNS(ns, local) { return this.getAttribute(local); }
  getAttributeNode(name) { const v = this.getAttribute(name); return v === null ? null : makeAttr(this, this[P.__attrName](name), v); }
  getAttributeNames() { return attrNames(idOf(this)); }
  hasAttribute(name) { return this.getAttribute(name) !== null; }
  hasAttributes() { return attrNames(idOf(this)).length > 0; }
  setAttribute(name, value) {
    name = this[P.__attrName](name);
    if (!/^[A-Za-z_:][-A-Za-z0-9_:.]*$/.test(name)) throw domException(`Failed to execute 'setAttribute' on 'Element': '${name}' is not a valid attribute name.`, 'InvalidCharacterError');
    const old = op('getAttr', idOf(this), name);
    op('setAttr', idOf(this), name, toStr(value));
    attributeChanged(this, name, old, toStr(value));
  }
  setAttributeNS(ns, name, value) { this.setAttribute(name, value); }
  setAttributeNode(attr) { this.setAttribute(attr.name, attr.value); return null; }
  removeAttribute(name) {
    name = this[P.__attrName](name);
    const old = op('getAttr', idOf(this), name);
    if (old === null) return;
    op('removeAttr', idOf(this), name);
    attributeChanged(this, name, old, null);
  }
  removeAttributeNS(ns, local) { this.removeAttribute(local); }
  toggleAttribute(name, force) {
    const has = this.hasAttribute(name);
    if (has && force !== true) { this.removeAttribute(name); return false; }
    if (!has && force !== false) { this.setAttribute(name, ''); return true; }
    return has;
  }
  get innerHTML() { return op('innerHTML', idOf(this)); }
  set innerHTML(html) { replaceChildrenWithHtml(this, html === null ? '' : html); }
  get outerHTML() { return op('outerHTML', idOf(this)); }
  set outerHTML(html) {
    const parent = this.parentNode;
    if (!parent) throw domException("Failed to set the 'outerHTML' property on 'Element': This element has no parent node.", 'NoModificationAllowedError');
    const holder = parseFragment(parent, html);
    const fragment = document.createDocumentFragment();
    while (holder.firstChild) fragment.appendChild(holder.firstChild);
    parent.replaceChild(fragment, this);
  }
  get innerText() { return this.textContent; }
  set innerText(v) { this.textContent = v; }
  insertAdjacentHTML(position, html) {
    const holder = parseFragment(this, html);
    const fragment = document.createDocumentFragment();
    while (holder.firstChild) fragment.appendChild(holder.firstChild);
    this.insertAdjacentElement(position, fragment);
  }
  insertAdjacentElement(position, node) {
    switch (lower(position)) {
      case 'beforebegin': if (!this.parentNode) return null; this.parentNode.insertBefore(node, this); break;
      case 'afterbegin': this.insertBefore(node, this.firstChild); break;
      case 'beforeend': this.appendChild(node); break;
      case 'afterend': if (!this.parentNode) return null; this.parentNode.insertBefore(node, this.nextSibling); break;
      default: throw domException(`Failed to execute 'insertAdjacentElement' on 'Element': The value provided ('${position}') is not one of 'beforeBegin', 'afterBegin', 'beforeEnd', or 'afterEnd'.`, 'SyntaxError');
    }
    return node;
  }
  insertAdjacentText(position, text) { this.insertAdjacentElement(position, document.createTextNode(text)); }
  matches(selector) { return op('matches', idOf(this), toStr(selector)); }
  webkitMatchesSelector(selector) { return this.matches(selector); }
  closest(selector) { for (let el = this; el && el.nodeType === 1; el = el.parentElement) if (el.matches(selector)) return el; return null; }
  getElementsByTagName(name) {
    const id = idOf(this);
    const wanted = name === '*' ? null : lower(name);
    return liveList(HTMLCollection.prototype, () => op('query', id, '*', true).map(wrap).filter((e) => wanted === null || e.localName.toLowerCase() === wanted));
  }
  getElementsByTagNameNS(ns, name) { return this.getElementsByTagName(name); }
  getElementsByClassName(names) {
    const id = idOf(this);
    const wanted = String(names).split(/\s+/).filter(Boolean);
    return liveList(HTMLCollection.prototype, () => (wanted.length ? op('query', id, wanted.map((n) => '.' + CSS.escape(n)).join(''), true).map(wrap) : []));
  }
  get style() { return styleOf(this); }
  set style(v) { this.style.cssText = v; }
  get dataset() { return datasetOf(this); }
  get shadowRoot() { return this[P.__shadow] && this[P.__shadow].mode === 'open' ? this[P.__shadow] : null; }
  attachShadow(init) {
    if (this[P.__shadow]) throw domException("Failed to execute 'attachShadow' on 'Element': Shadow root cannot be created on a host which already hosts a shadow tree.", 'NotSupportedError');
    const root = new DocumentFragment();
    Object.setPrototypeOf(root, ShadowRoot.prototype);
    hidden(root, '__host', this);
    hidden(root, '__mode', init.mode);
    hidden(this, '__shadow', root);
    return root;
  }
  // No elemento raiz, client* é a janela (sem barra de rolagem: o headless não desenha nenhuma).
  get clientWidth() { if (this === document.documentElement) return VIEWPORT.width; const b = boxOf(this); return b ? Math.round(b.width - b.border[1] - b.border[3] - b.scrollbar[0]) : 0; }
  get clientHeight() { if (this === document.documentElement) return VIEWPORT.height; const b = boxOf(this); return b ? Math.round(b.height - b.border[0] - b.border[2] - b.scrollbar[1]) : 0; }
  get clientTop() { const b = boxOf(this); return b ? Math.round(b.border[0]) : 0; }
  get clientLeft() { const b = boxOf(this); return b ? Math.round(b.border[3]) : 0; }
  get scrollWidth() { return this.clientWidth; }
  get scrollHeight() { return this.clientHeight; }
  get scrollTop() { return 0; }
  set scrollTop(v) {}
  get scrollLeft() { return 0; }
  set scrollLeft(v) {}
  getBoundingClientRect() { const b = boxOf(this); return b ? new DOMRect(b.x, b.y, b.width, b.height) : new DOMRect(0, 0, 0, 0); }
  getClientRects() { return []; }
  scrollIntoView() {}
  scrollTo() {}
  scroll() {}
  focus() { if (document[P.__active] !== this) { const prev = document[P.__active]; document[P.__active] = this; if (prev) prev.dispatchEvent(new FocusEvent('blur')); this.dispatchEvent(new FocusEvent('focus')); this.dispatchEvent(new FocusEvent('focusin', { bubbles: true })); } }
  blur() { if (document[P.__active] === this) { document[P.__active] = null; this.dispatchEvent(new FocusEvent('blur')); this.dispatchEvent(new FocusEvent('focusout', { bubbles: true })); } }
  click() { this.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, composed: true })); }
  animate() { return { cancel() {}, finish() {}, play() {}, pause() {}, finished: Promise.resolve(), onfinish: null }; }
  getAnimations() { return []; }
  checkVisibility() { return true; }
  setPointerCapture() {}
  releasePointerCapture() {}
  hasPointerCapture() { return false; }
  requestFullscreen() { return Promise.reject(new TypeError('Fullscreen is not supported')); }
}
tag(Element, 'Element');
mixin(Element.prototype, ChildNode);
mixin(Element.prototype, ParentNode);
delete Element.prototype.getElementById;
mixin(Element.prototype, NonElementSiblings);

class ShadowRoot extends DocumentFragment {
  get mode() { return this[P.__mode]; }
  get host() { return this[P.__host]; }
  get innerHTML() { return op('innerHTML', idOf(this)); }
  set innerHTML(html) { op('setInnerHTML', idOf(this), toStr(html)); }
  get activeElement() { return null; }
}
tag(ShadowRoot, 'ShadowRoot');

// A caixa de borda do layout do hospedeiro (estilo e layout do Blitz na janela de 800x600); `null` desconectado.
const boxOf = (el) => {
  if (!el.isConnected) return null;
  const v = op('box', idOf(el));
  return v && { x: v[0], y: v[1], width: v[2], height: v[3], border: v.slice(4, 8), padding: v.slice(8, 12), scrollbar: v.slice(12, 14) };
};

// Reflexão de atributos: propriedade <-> atributo, com o tipo do valor.
const reflect = (cls, spec) => {
  for (const [prop, attr, kind] of spec) {
    Object.defineProperty(cls.prototype, prop, {
      configurable: true,
      enumerable: true,
      get() {
        const v = this.getAttribute(attr);
        if (kind === 'bool') return v !== null;
        if (kind === 'long') { const n = parseInt(v, 10); return Number.isFinite(n) ? n : 0; }
        if (kind === 'url') return v === null ? '' : resolveUrl(v);
        if (kind === 'enum') return v === null ? '' : lower(v);
        // `[Reflect, ReflectEmpty=anonymous, ReflectInvalid=anonymous] DOMString?` (crossOrigin): ausente é `null`.
        if (kind === 'cors') return v === null ? null : lower(v) === 'use-credentials' ? 'use-credentials' : 'anonymous';
        return v === null ? '' : v;
      },
      set(value) {
        if (kind === 'bool') { if (value) this.setAttribute(attr, ''); else this.removeAttribute(attr); }
        else if (kind === 'cors' && value === null) this.removeAttribute(attr);
        else this.setAttribute(attr, toStr(value));
      },
    });
  }
};
// ARIAMixin vive em Element; o resto da reflexão global é de HTMLElement e SVGElement (`40-html.js`), como na IDL.
reflect(Element, [['ariaLabel', 'aria-label'], ['role', 'role']]);
