// ---- Nós: Node, CharacterData, Text, Comment, DocumentFragment, DocumentType, Document e as listas.

const mutationHook = { record() {}, active: false };

const wrap = (id) => {
  let node = wrappers.get(id);
  if (node) return node;
  const kind = op('kind', id);
  let proto;
  if (kind === 1) proto = elementPrototype(op('name', id), op('ns', id));
  else if (kind === 3) proto = Text.prototype;
  else if (kind === 8) proto = Comment.prototype;
  else if (kind === 11) proto = DocumentFragment.prototype;
  else proto = Document.prototype;
  node = Object.create(proto);
  ids.set(node, id);
  wrappers.set(id, node);
  return node;
};
const wrapOrNull = (id) => (id === null ? null : wrap(id));
const childIds = (node) => op('children', idOf(node));

// ---- NodeList e HTMLCollection (vivas: relêem o hospedeiro a cada acesso).
class NodeList {
  constructor() { throw new TypeError('Illegal constructor'); }
  item(index) { return this[index >>> 0] || null; }
  forEach(callback, thisArg) { for (let i = 0; i < this.length; i++) callback.call(thisArg, this[i], i, this); }
  entries() { return Array.prototype.entries.call(this); }
  keys() { return Array.prototype.keys.call(this); }
  values() { return Array.prototype.values.call(this); }
  [Symbol.iterator]() { return Array.prototype.values.call(this); }
}
tag(NodeList, 'NodeList');

class HTMLCollection {
  constructor() { throw new TypeError('Illegal constructor'); }
  item(index) { return this[index >>> 0] || null; }
  namedItem(name) {
    for (let i = 0; i < this.length; i++) {
      const el = this[i];
      if (el.getAttribute('id') === name || el.getAttribute('name') === name) return el;
    }
    return null;
  }
  [Symbol.iterator]() { return Array.prototype.values.call(this); }
}
tag(HTMLCollection, 'HTMLCollection');

const isIndex = (key) => typeof key === 'string' && key !== '' && /^(0|[1-9][0-9]*)$/.test(key);

const liveList = (proto, fetch) => {
  const target = Object.create(proto);
  return new Proxy(target, {
    get(t, key, receiver) {
      if (isIndex(key)) return fetch()[+key];
      if (key === 'length') return fetch().length;
      if (proto === HTMLCollection.prototype && typeof key === 'string' && !(key in t)) {
        const found = HTMLCollection.prototype.namedItem.call(receiver, key);
        if (found) return found;
      }
      return Reflect.get(t, key, receiver);
    },
    has(t, key) { return isIndex(key) ? +key < fetch().length : key === 'length' || Reflect.has(t, key); },
    ownKeys(t) { return [...fetch().keys()].map(String).concat(Reflect.ownKeys(t)); },
    getOwnPropertyDescriptor(t, key) {
      if (isIndex(key)) {
        const items = fetch();
        return +key < items.length ? { value: items[+key], writable: false, enumerable: true, configurable: true } : undefined;
      }
      return Reflect.getOwnPropertyDescriptor(t, key);
    },
    set(t, key) { return isIndex(key) ? false : Reflect.set(...arguments); },
  });
};
const staticNodeList = (nodes) => liveList(NodeList.prototype, () => nodes);

// ---- Node
class Node extends EventTarget {
  constructor() {
    if (arguments[0] !== ILLEGAL) throw new TypeError('Illegal constructor');
    super();
  }
  get nodeType() { return op('kind', idOf(this)); }
  get nodeName() {
    const id = idOf(this);
    const kind = op('kind', id);
    if (kind === 1) return this.tagName;
    return op('name', id);
  }
  get nodeValue() { const kind = this.nodeType; return kind === 3 || kind === 8 ? op('data', idOf(this)) : null; }
  set nodeValue(value) { const kind = this.nodeType; if (kind === 3 || kind === 8) this.__setData(toStr(value)); }
  __setData(value) {
    const old = op('data', idOf(this));
    op('setData', idOf(this), value);
    mutationHook.record({ type: 'characterData', target: this, oldValue: old });
  }
  get textContent() {
    const kind = this.nodeType;
    if (kind === 9) return null;
    if (kind === 3 || kind === 8) return op('data', idOf(this));
    return op('textContent', idOf(this));
  }
  set textContent(value) {
    const kind = this.nodeType;
    const text = value === null || value === undefined ? '' : String(value);
    if (kind === 3 || kind === 8) return this.__setData(text);
    if (kind === 9) return;
    const removed = this.childNodes.length ? Array.from(this.childNodes) : [];
    op('setTextContent', idOf(this), text);
    mutationHook.record({ type: 'childList', target: this, removedNodes: removed, addedNodes: text ? Array.from(this.childNodes) : [] });
  }
  get parentNode() { return wrapOrNull(op('parent', idOf(this))); }
  get parentElement() { const parent = this.parentNode; return parent && parent.nodeType === 1 ? parent : null; }
  get childNodes() { const id = idOf(this); return liveList(NodeList.prototype, () => op('children', id).map(wrap)); }
  get firstChild() { const kids = childIds(this); return kids.length ? wrap(kids[0]) : null; }
  get lastChild() { const kids = childIds(this); return kids.length ? wrap(kids[kids.length - 1]) : null; }
  get previousSibling() { return this.__sibling(-1); }
  get nextSibling() { return this.__sibling(1); }
  __sibling(step) {
    const id = idOf(this);
    const parent = op('parent', id);
    if (parent === null) return null;
    const kids = op('children', parent);
    const next = kids[kids.indexOf(id) + step];
    return next === undefined ? null : wrap(next);
  }
  get ownerDocument() { return this.nodeType === 9 ? null : document; }
  get isConnected() { return this.getRootNode() === document; }
  get baseURI() { return document.baseURI; }
  hasChildNodes() { return childIds(this).length > 0; }
  getRootNode() {
    let node = this;
    for (let parent = node.parentNode; parent; parent = node.parentNode) node = parent;
    if (node.__host && arguments[0] && arguments[0].composed) return node.__host.getRootNode(arguments[0]);
    return node;
  }
  contains(other) {
    if (other === null || other === undefined) return false;
    for (let node = other; node; node = node.parentNode) if (node === this) return true;
    return false;
  }
  __checkInsert(node, reference, method) {
    if (!isNode(node)) throw new TypeError(`Failed to execute '${method}' on 'Node': parameter 1 is not of type 'Node'.`);
    const kind = this.nodeType;
    if (kind !== 1 && kind !== 9 && kind !== 11) throw domException(`Failed to execute '${method}' on 'Node': This node type does not support this method.`, 'HierarchyRequestError');
    if (node.contains(this)) throw domException(`Failed to execute '${method}' on 'Node': The new child element contains the parent.`, 'HierarchyRequestError');
    if (reference !== null && reference.parentNode !== this) throw domException(`Failed to execute '${method}' on 'Node': The node before which the new node is to be inserted is not a child of this node.`, 'NotFoundError');
  }
  __insert(node, reference, method) {
    this.__checkInsert(node, reference, method);
    const wasFragment = node.nodeType === 11;
    const added = wasFragment ? Array.from(node.childNodes) : [node];
    const oldParent = wasFragment ? null : node.parentNode;
    const previous = reference ? reference.previousSibling : this.lastChild;
    if (oldParent) mutationHook.record({ type: 'childList', target: oldParent, removedNodes: [node], addedNodes: [], previousSibling: node.previousSibling, nextSibling: node.nextSibling });
    if (reference === node) reference = node.nextSibling;
    if (reference === null) op('append', idOf(this), idOf(node));
    else op('insertBefore', idOf(this), idOf(node), idOf(reference));
    mutationHook.record({ type: 'childList', target: this, addedNodes: added, removedNodes: [], previousSibling: previous, nextSibling: reference });
    for (const child of added) afterInsert(child);
    return node;
  }
  appendChild(node) { return this.__insert(node, null, 'appendChild'); }
  insertBefore(node, reference) {
    if (arguments.length < 2) throw new TypeError("Failed to execute 'insertBefore' on 'Node': 2 arguments required, but only 1 present.");
    return this.__insert(node, reference === undefined ? null : reference, 'insertBefore');
  }
  removeChild(child) {
    if (!isNode(child)) throw new TypeError("Failed to execute 'removeChild' on 'Node': parameter 1 is not of type 'Node'.");
    if (child.parentNode !== this) throw domException("Failed to execute 'removeChild' on 'Node': The node to be removed is not a child of this node.", 'NotFoundError');
    const previous = child.previousSibling;
    const next = child.nextSibling;
    op('remove', idOf(child));
    mutationHook.record({ type: 'childList', target: this, removedNodes: [child], addedNodes: [], previousSibling: previous, nextSibling: next });
    return child;
  }
  replaceChild(node, child) {
    if (!isNode(child) || child.parentNode !== this) throw domException("Failed to execute 'replaceChild' on 'Node': The node to be replaced is not a child of this node.", 'NotFoundError');
    const next = child.nextSibling === node ? node.nextSibling : child.nextSibling;
    this.removeChild(child);
    this.__insert(node, next, 'replaceChild');
    return child;
  }
  cloneNode(deep) { return wrap(op('clone', idOf(this), !!deep)); }
  isSameNode(other) { return this === other; }
  isEqualNode(other) {
    if (!isNode(other) || other.nodeType !== this.nodeType) return false;
    const kind = this.nodeType;
    if (kind === 1) return this.outerHTML === other.outerHTML;
    if (kind === 3 || kind === 8) return this.nodeValue === other.nodeValue;
    const a = Array.from(this.childNodes);
    const b = Array.from(other.childNodes);
    return a.length === b.length && a.every((child, i) => child.isEqualNode(b[i]));
  }
  normalize() {
    let child = this.firstChild;
    while (child) {
      const next = child.nextSibling;
      if (child.nodeType === 3) {
        if (child.data === '') this.removeChild(child);
        else {
          let after = child.nextSibling;
          while (after && after.nodeType === 3) {
            child.__setData(child.data + after.data);
            const following = after.nextSibling;
            this.removeChild(after);
            after = following;
          }
        }
      } else if (child.nodeType === 1) child.normalize();
      child = child.nodeType === 3 ? child.nextSibling : next;
    }
  }
  compareDocumentPosition(other) {
    if (this === other) return 0;
    const chain = (node) => { const out = []; for (; node; node = node.parentNode) out.unshift(node); return out; };
    const a = chain(this);
    const b = chain(other);
    if (a[0] !== b[0]) return 1 | 32 | (this < other ? 4 : 2);
    let i = 0;
    while (a[i] && a[i] === b[i]) i++;
    if (i === a.length) return 16 | 4;
    if (i === b.length) return 8 | 2;
    const parent = a[i - 1];
    const order = Array.from(parent.childNodes);
    return order.indexOf(a[i]) < order.indexOf(b[i]) ? 4 : 2;
  }
  lookupNamespaceURI() { return null; }
  isDefaultNamespace() { return true; }
}
tag(Node, 'Node');
Object.assign(Node, { ELEMENT_NODE: 1, ATTRIBUTE_NODE: 2, TEXT_NODE: 3, CDATA_SECTION_NODE: 4, PROCESSING_INSTRUCTION_NODE: 7, COMMENT_NODE: 8, DOCUMENT_NODE: 9, DOCUMENT_TYPE_NODE: 10, DOCUMENT_FRAGMENT_NODE: 11,
  DOCUMENT_POSITION_DISCONNECTED: 1, DOCUMENT_POSITION_PRECEDING: 2, DOCUMENT_POSITION_FOLLOWING: 4, DOCUMENT_POSITION_CONTAINS: 8, DOCUMENT_POSITION_CONTAINED_BY: 16, DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC: 32 });
Object.assign(Node.prototype, { ELEMENT_NODE: 1, ATTRIBUTE_NODE: 2, TEXT_NODE: 3, CDATA_SECTION_NODE: 4, PROCESSING_INSTRUCTION_NODE: 7, COMMENT_NODE: 8, DOCUMENT_NODE: 9, DOCUMENT_TYPE_NODE: 10, DOCUMENT_FRAGMENT_NODE: 11 });

// Métodos de ChildNode e ParentNode, compartilhados por Element, CharacterData e fragmentos.
const nodesToFragment = (items) => {
  const fragment = document.createDocumentFragment();
  for (const item of items) fragment.appendChild(isNode(item) ? item : document.createTextNode(String(item)));
  return fragment;
};
const ChildNode = {
  before(...items) { const parent = this.parentNode; if (parent) parent.insertBefore(nodesToFragment(items), this); },
  after(...items) { const parent = this.parentNode; if (parent) parent.insertBefore(nodesToFragment(items), this.nextSibling); },
  replaceWith(...items) { const parent = this.parentNode; if (parent) parent.replaceChild(nodesToFragment(items), this); },
  remove() { const parent = this.parentNode; if (parent) parent.removeChild(this); },
};
const ParentNode = {
  append(...items) { this.appendChild(nodesToFragment(items)); },
  prepend(...items) { this.insertBefore(nodesToFragment(items), this.firstChild); },
  replaceChildren(...items) {
    while (this.firstChild) this.removeChild(this.firstChild);
    this.appendChild(nodesToFragment(items));
  },
  get children() { const id = idOf(this); return liveList(HTMLCollection.prototype, () => op('children', id).filter((c) => op('kind', c) === 1).map(wrap)); },
  get childElementCount() { return op('children', idOf(this)).filter((c) => op('kind', c) === 1).length; },
  get firstElementChild() { const kid = op('children', idOf(this)).find((c) => op('kind', c) === 1); return kid === undefined ? null : wrap(kid); },
  get lastElementChild() { const kids = op('children', idOf(this)).filter((c) => op('kind', c) === 1); return kids.length ? wrap(kids[kids.length - 1]) : null; },
  querySelector(selector) { return wrapOrNull(op('query', idOf(this), toStr(selector), false)); },
  querySelectorAll(selector) { return staticNodeList(op('query', idOf(this), toStr(selector), true).map(wrap)); },
  getElementById(id) {
    const found = op('byId', toStr(id));
    return found === null ? null : wrap(found);
  },
};
const mixin = (target, source) => {
  for (const key of Reflect.ownKeys(source)) Object.defineProperty(target, key, { ...Object.getOwnPropertyDescriptor(source, key), enumerable: false, configurable: true });
};
const NonElementSiblings = {
  get previousElementSibling() { for (let n = this.previousSibling; n; n = n.previousSibling) if (n.nodeType === 1) return n; return null; },
  get nextElementSibling() { for (let n = this.nextSibling; n; n = n.nextSibling) if (n.nodeType === 1) return n; return null; },
};

class CharacterData extends Node {
  get data() { return op('data', idOf(this)); }
  set data(value) { this.__setData(toStr(value)); }
  get length() { return this.data.length; }
  substringData(offset, count) { return this.data.substr(offset, count); }
  appendData(value) { this.data = this.data + value; }
  insertData(offset, value) { const d = this.data; this.data = d.slice(0, offset) + value + d.slice(offset); }
  deleteData(offset, count) { const d = this.data; this.data = d.slice(0, offset) + d.slice(offset + count); }
  replaceData(offset, count, value) { const d = this.data; this.data = d.slice(0, offset) + value + d.slice(offset + count); }
}
tag(CharacterData, 'CharacterData');
mixin(CharacterData.prototype, ChildNode);
mixin(CharacterData.prototype, NonElementSiblings);

class Text extends CharacterData {
  constructor(data = '') {
    super(ILLEGAL);
    const id = op('createText', toStr(data));
    ids.set(this, id);
    wrappers.set(id, this);
  }
  get wholeText() { return this.data; }
  splitText(offset) {
    const tail = document.createTextNode(this.data.slice(offset));
    this.data = this.data.slice(0, offset);
    if (this.parentNode) this.parentNode.insertBefore(tail, this.nextSibling);
    return tail;
  }
}
tag(Text, 'Text');
class Comment extends CharacterData {
  constructor(data = '') {
    super(ILLEGAL);
    const id = op('createComment', toStr(data));
    ids.set(this, id);
    wrappers.set(id, this);
  }
}
tag(Comment, 'Comment');
class CDATASection extends Text {}
tag(CDATASection, 'CDATASection');

class DocumentFragment extends Node {
  constructor() {
    super(ILLEGAL);
    const id = op('createFragment');
    ids.set(this, id);
    wrappers.set(id, this);
  }
  getElementById(id) { return this.querySelector('#' + CSS.escape(String(id))); }
}
tag(DocumentFragment, 'DocumentFragment');
mixin(DocumentFragment.prototype, ParentNode);
delete DocumentFragment.prototype.getElementById;
DocumentFragment.prototype.getElementById = function (id) { return this.querySelector('#' + CSS.escape(String(id))); };

class DocumentType extends Node {}
tag(DocumentType, 'DocumentType');
mixin(DocumentType.prototype, ChildNode);
