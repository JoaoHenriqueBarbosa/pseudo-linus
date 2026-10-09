// ---- Document.

const baseUrlOf = () => {
  const base = document.querySelector('base[href]');
  const href = pageState.url;
  if (base) { try { return new URL(base.getAttribute('href'), href).href; } catch {} }
  return href;
};
const resolveUrl = (value) => { try { return new URL(String(value), baseUrlOf()).href; } catch { return String(value); } };

const cookieJar = new Map();

class Document extends Node {
  constructor() { throw new TypeError('Illegal constructor'); }
  get __html() { return true; }
  get nodeName() { return '#document'; }
  get documentElement() { return wrapOrNull(op('docElement', idOf(this))); }
  get head() { const html = this.documentElement; return html ? Array.from(html.children).find((c) => c.localName === 'head') || null : null; }
  get body() { const html = this.documentElement; return html ? Array.from(html.children).find((c) => c.localName === 'body' || c.localName === 'frameset') || null : null; }
  set body(value) { const html = this.documentElement; const old = this.body; if (old) html.replaceChild(value, old); else html.appendChild(value); }
  get doctype() { const dt = op('doctype'); if (dt === null) return null; if (!this.__doctype) { const o = Object.create(DocumentType.prototype); ids.set(o, -2); const m = /^<!DOCTYPE ([^ >]*)(?: PUBLIC "([^"]*)"(?: "([^"]*)")?)?/.exec(dt); hidden(o, 'name', m[1]); hidden(o, 'publicId', m[2] || ''); hidden(o, 'systemId', m[3] || ''); hidden(this, '__doctype', o); } return this.__doctype; }
  get title() { const t = this.querySelector('title'); return t ? t.textContent.replace(/[ \t\n\f\r]+/g, ' ').trim() : ''; }
  set title(value) {
    let t = this.querySelector('title');
    if (!t) { const head = this.head; if (!head) return; t = this.createElement('title'); head.appendChild(t); }
    t.textContent = value;
  }
  get URL() { return pageState.url; }
  get documentURI() { return pageState.url; }
  get baseURI() { return baseUrlOf(); }
  get domain() { try { return new URL(pageState.url).hostname; } catch { return ''; } }
  get referrer() { return ''; }
  get cookie() { return Array.from(cookieJar, ([k, v]) => `${k}=${v}`).join('; '); }
  set cookie(value) {
    const parts = String(value).split(';');
    const eq = parts[0].indexOf('=');
    const name = (eq < 0 ? '' : parts[0].slice(0, eq)).trim();
    const val = eq < 0 ? parts[0].trim() : parts[0].slice(eq + 1).trim();
    const attrs = parts.slice(1).map((p) => p.trim().toLowerCase());
    const expired = attrs.some((a) => a === 'max-age=0' || a.startsWith('max-age=-') || (a.startsWith('expires=') && Date.parse(a.slice(8)) < Date.now()));
    if (expired) cookieJar.delete(name); else cookieJar.set(name, val);
  }
  get contentType() { return 'text/html'; }
  get characterSet() { return 'UTF-8'; }
  get charset() { return 'UTF-8'; }
  get inputEncoding() { return 'UTF-8'; }
  get compatMode() { return op('doctype') === null ? 'BackCompat' : 'CSS1Compat'; }
  get readyState() { return this.__readyState || 'loading'; }
  get visibilityState() { return 'visible'; }
  get hidden() { return false; }
  get defaultView() { return window; }
  get currentScript() { return this.__currentScript || null; }
  get activeElement() { return this.__active || this.body; }
  get lastModified() { return '01/01/1970 00:00:00'; }
  get designMode() { return 'off'; }
  get dir() { return this.documentElement ? this.documentElement.dir : ''; }
  get location() { return location; }
  get all() { return this.querySelectorAll('*'); }
  get forms() { return liveList(HTMLCollection.prototype, () => Array.from(document.querySelectorAll('form'))); }
  get images() { return liveList(HTMLCollection.prototype, () => Array.from(document.querySelectorAll('img'))); }
  get links() { return liveList(HTMLCollection.prototype, () => Array.from(document.querySelectorAll('a[href],area[href]'))); }
  get anchors() { return liveList(HTMLCollection.prototype, () => Array.from(document.querySelectorAll('a[name]'))); }
  get scripts() { return liveList(HTMLCollection.prototype, () => Array.from(document.querySelectorAll('script'))); }
  get embeds() { return liveList(HTMLCollection.prototype, () => Array.from(document.querySelectorAll('embed'))); }
  get styleSheets() { return Object.assign(Array.from(document.querySelectorAll('style,link[rel~=stylesheet]'), (el) => el.sheet).filter(Boolean), { item(i) { return this[i] || null; } }); }
  get fonts() { return fontFaceSet; }
  get adoptedStyleSheets() { return this.__adopted || (this.__adopted = []); }
  set adoptedStyleSheets(v) { this.__adopted = v; }
  get scrollingElement() { return this.documentElement; }
  get timeline() { return { currentTime: performance.now() }; }
  get implementation() { return domImplementation; }
  evaluate(expression, contextNode, resolver, type, result) { return XPathEvaluator.prototype.evaluate(expression, contextNode, resolver, type, result); }
  createExpression(expression) { return XPathEvaluator.prototype.createExpression(expression); }
  createNSResolver(node) { return node; }
  createElement(name) {
    name = toStr(name);
    if (!/^[A-Za-z][^\s\/>]*$/.test(name) && !/^[A-Za-z_:][-A-Za-z0-9_:.]*$/.test(name)) throw domException(`Failed to execute 'createElement' on 'Document': The tag name provided ('${name}') is not a valid name.`, 'InvalidCharacterError');
    const local = name.toLowerCase();
    const def = customByName.get(local);
    if (def) { try { const el = new def.ctor(); hidden(el, '__upgraded', true); return el; } catch (error) { reportError(error); } }
    const el = wrap(op('createElement', local, HTML_NS));
    if (local === 'script') hidden(el, '__fresh', true);
    return el;
  }
  createElementNS(ns, qualified) {
    const name = toStr(qualified);
    const local = name.includes(':') ? name.slice(name.indexOf(':') + 1) : name;
    const namespace = ns === null || ns === undefined ? '' : String(ns);
    if (namespace === HTML_NS) return this.createElement(local);
    return wrap(op('createElement', local, namespace));
  }
  createTextNode(data) { return new Text(toStr(data)); }
  createComment(data) { return new Comment(toStr(data)); }
  createCDATASection(data) { return new Text(toStr(data)); }
  createDocumentFragment() { return new DocumentFragment(); }
  createAttribute(name) { return makeAttr(null, lower(name), ''); }
  createProcessingInstruction() { throw domException('Processing instructions are not supported.', 'NotSupportedError'); }
  createEvent(type) {
    const t = lower(type).replace(/s$/, '');
    const cls = { event: Event, htmlevent: Event, customevent: CustomEvent, uievent: UIEvent, mouseevent: MouseEvent, keyboardevent: KeyboardEvent, focusevent: FocusEvent, messageevent: MessageEvent, hashchangeevent: HashChangeEvent }[t];
    if (!cls) throw domException("Failed to execute 'createEvent' on 'Document': The provided event type ('" + type + "') is invalid.", 'NotSupportedError');
    const e = new cls('');
    e.initCustomEvent = e.initMouseEvent = e.initKeyboardEvent = e.initUIEvent = (a, b, c) => e.initEvent(a, b, c);
    return e;
  }
  createRange() { return new Range(); }
  createTreeWalker(root, whatToShow = 0xffffffff, filter = null) { return new TreeWalker(root, whatToShow, filter); }
  createNodeIterator(root, whatToShow = 0xffffffff, filter = null) { return new NodeIterator(root, whatToShow, filter); }
  importNode(node, deep) { return node.cloneNode(!!deep); }
  adoptNode(node) { if (node.parentNode) node.parentNode.removeChild(node); return node; }
  getElementById(id) { const found = op('byId', toStr(id)); return found === null ? null : wrap(found); }
  getElementsByName(name) { return liveList(NodeList.prototype, () => Array.from(document.querySelectorAll('[name]')).filter((e) => e.getAttribute('name') === String(name))); }
  hasFocus() { return true; }
  getSelection() { return selection; }
  elementFromPoint() { return null; }
  elementsFromPoint() { return []; }
  execCommand() { return false; }
  queryCommandSupported() { return false; }
  queryCommandEnabled() { return false; }
  open() { return this; }
  close() {}
  write(...parts) { documentWrite(parts.join('')); }
  writeln(...parts) { documentWrite(parts.join('') + '\n'); }
  startViewTransition(cb) { const p = Promise.resolve(typeof cb === 'function' ? cb() : undefined); return { finished: p, ready: p, updateCallbackDone: p, skipTransition() {} }; }
  exitFullscreen() { return Promise.resolve(); }
  get fullscreenElement() { return null; }
  get pictureInPictureElement() { return null; }
  get webkitVisibilityState() { return 'visible'; }
  get webkitHidden() { return false; }
  hasStorageAccess() { return Promise.resolve(true); }
}
tag(Document, 'HTMLDocument');
mixin(Document.prototype, ParentNode);
mixin(Document.prototype, { getElementsByTagName: Element.prototype.getElementsByTagName, getElementsByTagNameNS: Element.prototype.getElementsByTagNameNS, getElementsByClassName: Element.prototype.getElementsByClassName });
Object.defineProperty(Document.prototype, 'getElementById', { value: Document.prototype.getElementById, writable: true, configurable: true });
defineHandlerProperties(Document.prototype, ['readystatechange', 'visibilitychange', 'fullscreenchange', 'selectionchange', 'DOMContentLoaded', 'pointerlockchange', 'freeze', 'resume']);

function documentWrite(html) {
  const current = document.__currentScript;
  const holder = parseFragment(current ? current.parentNode : document.body, html);
  const fragment = document.createDocumentFragment();
  while (holder.firstChild) fragment.appendChild(holder.firstChild);
  const parent = current ? current.parentNode : document.body || document.documentElement;
  if (!parent) return;
  parent.insertBefore(fragment, current ? current.nextSibling : null);
}

class DOMImplementation {
  constructor() { throw new TypeError('Illegal constructor'); }
  createHTMLDocument(title) { return wrap(op('createDocument', title === undefined ? null : String(title))); }
  createDocument() { return wrap(op('createDocument', null)); }
  createDocumentType(name, publicId, systemId) {
    const o = Object.create(DocumentType.prototype);
    ids.set(o, -2);
    hidden(o, 'name', String(name));
    hidden(o, 'publicId', String(publicId));
    hidden(o, 'systemId', String(systemId));
    return o;
  }
  hasFeature() { return true; }
}
tag(DOMImplementation, 'DOMImplementation');
const domImplementation = Object.create(DOMImplementation.prototype);

class Selection {
  get rangeCount() { return 0; }
  get isCollapsed() { return true; }
  get anchorNode() { return null; }
  get focusNode() { return null; }
  get type() { return 'None'; }
  getRangeAt() { throw domException('index is out of range', 'IndexSizeError'); }
  addRange() {}
  removeAllRanges() {}
  removeRange() {}
  collapse() {}
  empty() {}
  toString() { return ''; }
  containsNode() { return false; }
}
tag(Selection, 'Selection');
const selection = new Selection();

class Range {
  constructor() { this.startContainer = document; this.endContainer = document; this.startOffset = 0; this.endOffset = 0; this.collapsed = true; this.commonAncestorContainer = document; }
  setStart(node, offset) { this.startContainer = node; this.startOffset = offset; this.collapsed = this.startContainer === this.endContainer && offset === this.endOffset; }
  setEnd(node, offset) { this.endContainer = node; this.endOffset = offset; this.collapsed = this.startContainer === this.endContainer && offset === this.startOffset; }
  setStartBefore(n) { this.setStart(n.parentNode, 0); }
  setStartAfter(n) { this.setStart(n.parentNode, 0); }
  setEndBefore(n) { this.setEnd(n.parentNode, 0); }
  setEndAfter(n) { this.setEnd(n.parentNode, 0); }
  selectNode(n) { this.setStart(n.parentNode, 0); this.setEnd(n.parentNode, 0); }
  selectNodeContents(n) { this.setStart(n, 0); this.setEnd(n, n.childNodes.length); }
  collapse() { this.collapsed = true; }
  cloneRange() { return Object.assign(new Range(), this); }
  detach() {}
  deleteContents() {}
  extractContents() { return document.createDocumentFragment(); }
  cloneContents() { return document.createDocumentFragment(); }
  insertNode(n) { if (this.startContainer.nodeType === 1) this.startContainer.insertBefore(n, this.startContainer.childNodes[this.startOffset] || null); }
  surroundContents() {}
  createContextualFragment(html) { const holder = parseFragment(this.startContainer, html); const f = document.createDocumentFragment(); while (holder.firstChild) f.appendChild(holder.firstChild); return f; }
  getBoundingClientRect() { return new DOMRect(); }
  getClientRects() { return []; }
  toString() { return ''; }
  compareBoundaryPoints() { return 0; }
  isPointInRange() { return false; }
}
tag(Range, 'Range');

class TreeWalker {
  constructor(root, whatToShow, filter) { this.root = root; this.whatToShow = whatToShow; this.filter = filter; this.currentNode = root; }
  __accept(node) {
    if (!(this.whatToShow & (1 << (node.nodeType - 1)))) return 3;
    if (!this.filter) return 1;
    return typeof this.filter === 'function' ? this.filter(node) : this.filter.acceptNode(node);
  }
  __next(from, skipChildren) {
    let node = from;
    for (;;) {
      if (!skipChildren && node.firstChild) node = node.firstChild;
      else {
        while (node !== this.root && !node.nextSibling) node = node.parentNode;
        if (node === this.root || !node) return null;
        node = node.nextSibling;
      }
      skipChildren = false;
      const verdict = this.__accept(node);
      if (verdict === 1) return node;
      if (verdict === 2) skipChildren = true;
    }
  }
  nextNode() { const n = this.__next(this.currentNode, false); if (n) this.currentNode = n; return n; }
  firstChild() { const kid = this.currentNode.firstChild; if (!kid) return null; this.currentNode = kid; return kid; }
  parentNode() { const p = this.currentNode.parentNode; if (!p || this.currentNode === this.root) return null; this.currentNode = p; return p; }
  nextSibling() { const s = this.currentNode.nextSibling; if (!s) return null; this.currentNode = s; return s; }
  previousSibling() { const s = this.currentNode.previousSibling; if (!s) return null; this.currentNode = s; return s; }
  lastChild() { const k = this.currentNode.lastChild; if (!k) return null; this.currentNode = k; return k; }
  previousNode() { return null; }
}
tag(TreeWalker, 'TreeWalker');
class NodeIterator extends TreeWalker {
  nextNode() { const n = this.__next(this.referenceNode || this.root, false); this.referenceNode = n || this.referenceNode; return n; }
  previousNode() { return null; }
  detach() {}
}
tag(NodeIterator, 'NodeIterator');
