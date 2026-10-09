// ---- XPath 1.0: document.evaluate, XPathEvaluator, XPathExpression e XPathResult.

const XPATH_TYPES = { ANY_TYPE: 0, NUMBER_TYPE: 1, STRING_TYPE: 2, BOOLEAN_TYPE: 3, UNORDERED_NODE_ITERATOR_TYPE: 4, ORDERED_NODE_ITERATOR_TYPE: 5, UNORDERED_NODE_SNAPSHOT_TYPE: 6, ORDERED_NODE_SNAPSHOT_TYPE: 7, ANY_UNORDERED_NODE_TYPE: 8, FIRST_ORDERED_NODE_TYPE: 9 };

const xpathError = (expr) => domException(`Failed to execute 'evaluate' on 'Document': The string '${expr}' is not a valid XPath expression.`, 'SyntaxError');

// Tokens: números, literais, nomes (com `:` e `*`), operadores e pontuação.
const xpathTokens = (source) => {
  const tokens = [];
  const re = /\s*(?:(\d+(?:\.\d*)?|\.\d+)|("[^"]*"|'[^']*')|(\.\.|::|\/\/|!=|<=|>=|[\/\[\]\(\)@,\|\+\-=<>\.\*\$])|([A-Za-z_][\w.\-]*(?::(?:[A-Za-z_][\w.\-]*|\*))?))/y;
  let match;
  re.lastIndex = 0;
  while (re.lastIndex < source.length) {
    if (/^\s*$/.test(source.slice(re.lastIndex))) break;
    match = re.exec(source);
    if (!match) throw xpathError(source);
    if (match[1] !== undefined) tokens.push({ t: 'num', v: parseFloat(match[1]) });
    else if (match[2] !== undefined) tokens.push({ t: 'str', v: match[2].slice(1, -1) });
    else if (match[3] !== undefined) tokens.push({ t: 'op', v: match[3] });
    else tokens.push({ t: 'name', v: match[4] });
  }
  return tokens;
};

const AXES = ['ancestor', 'ancestor-or-self', 'attribute', 'child', 'descendant', 'descendant-or-self', 'following', 'following-sibling', 'namespace', 'parent', 'preceding', 'preceding-sibling', 'self'];

const xpathParse = (source) => {
  const tokens = xpathTokens(source);
  let i = 0;
  const peek = (k = 0) => tokens[i + k];
  const isOp = (v, k = 0) => peek(k) && peek(k).t === 'op' && peek(k).v === v;
  const isName = (v, k = 0) => peek(k) && peek(k).t === 'name' && peek(k).v === v;
  const expect = (v) => { if (!isOp(v)) throw xpathError(source); i++; };
  // Precedência: or < and < igualdade < relacional < aditivo < multiplicativo < unário < união.
  const binary = (next, ops) => () => {
    let left = next();
    for (;;) {
      const tok = peek();
      const op = tok && ((tok.t === 'op' && ops.includes(tok.v)) || (tok.t === 'name' && ops.includes(tok.v))) ? tok.v : null;
      if (!op) return left;
      i++;
      left = { k: 'bin', op, l: left, r: next() };
    }
  };
  const orExpr = () => binary(andExpr, ['or'])();
  const andExpr = () => binary(eqExpr, ['and'])();
  const eqExpr = () => binary(relExpr, ['=', '!='])();
  const relExpr = () => binary(addExpr, ['<', '<=', '>', '>='])();
  const addExpr = () => binary(mulExpr, ['+', '-'])();
  const mulExpr = () => {
    let left = unary();
    for (;;) {
      const tok = peek();
      if (tok && ((tok.t === 'op' && tok.v === '*') || (tok.t === 'name' && ['div', 'mod'].includes(tok.v)))) { i++; left = { k: 'bin', op: tok.v, l: left, r: unary() }; }
      else return left;
    }
  };
  const unary = () => (isOp('-') ? (i++, { k: 'neg', e: unary() }) : union());
  const union = () => { let left = pathExpr(); while (isOp('|')) { i++; left = { k: 'union', l: left, r: pathExpr() }; } return left; };
  const startsPrimary = () => {
    const tok = peek();
    if (!tok) return false;
    if (tok.t === 'num' || tok.t === 'str' || (tok.t === 'op' && (tok.v === '(' || tok.v === '$'))) return true;
    return tok.t === 'name' && isOp('(', 1) && !['node', 'text', 'comment', 'processing-instruction'].includes(tok.v);
  };
  const pathExpr = () => {
    if (startsPrimary()) {
      let expr = primary();
      while (isOp('[')) { i++; expr = { k: 'filter', e: expr, p: orExpr() }; expect(']'); }
      if (isOp('/') || isOp('//')) { const steps = relSteps(); return { k: 'path', base: expr, steps }; }
      return expr;
    }
    if (isOp('/') || isOp('//')) {
      const absolute = { k: 'root' };
      if (isOp('/') && !startsStep(1)) { i++; return absolute; }
      return { k: 'path', base: absolute, steps: relSteps() };
    }
    return { k: 'path', base: null, steps: steps() };
  };
  const startsStep = (k) => { const tok = peek(k); return tok && (tok.t === 'name' || (tok.t === 'op' && ['.', '..', '@', '*'].includes(tok.v))); };
  const relSteps = () => {
    const out = [];
    while (isOp('/') || isOp('//')) {
      if (peek().v === '//') out.push({ axis: 'descendant-or-self', test: { type: 'node' }, preds: [] });
      i++;
      out.push(step());
    }
    return out;
  };
  const steps = () => { const out = [step()]; out.push(...relSteps()); return out; };
  const step = () => {
    if (isOp('.')) { i++; return { axis: 'self', test: { type: 'node' }, preds: [] }; }
    if (isOp('..')) { i++; return { axis: 'parent', test: { type: 'node' }, preds: [] }; }
    let axis = 'child';
    if (isOp('@')) { i++; axis = 'attribute'; }
    else if (peek() && peek().t === 'name' && isOp('::', 1)) {
      axis = peek().v;
      if (!AXES.includes(axis)) throw xpathError(source);
      i += 2;
    }
    let test;
    const tok = peek();
    if (!tok) throw xpathError(source);
    if (tok.t === 'op' && tok.v === '*') { i++; test = { type: 'name', name: '*' }; }
    else if (tok.t === 'name' && isOp('(', 1) && ['node', 'text', 'comment', 'processing-instruction'].includes(tok.v)) {
      i += 2;
      if (peek() && peek().t === 'str') i++;
      expect(')');
      test = { type: tok.v };
    } else if (tok.t === 'name') { i++; test = { type: 'name', name: tok.v }; }
    else throw xpathError(source);
    const preds = [];
    while (isOp('[')) { i++; preds.push(orExpr()); expect(']'); }
    return { axis, test, preds };
  };
  const primary = () => {
    const tok = peek();
    if (tok.t === 'num') { i++; return { k: 'num', v: tok.v }; }
    if (tok.t === 'str') { i++; return { k: 'str', v: tok.v }; }
    if (isOp('(')) { i++; const e = orExpr(); expect(')'); return e; }
    if (isOp('$')) throw domException('Variables are not supported.', 'NotSupportedError');
    i += 2;
    const args = [];
    if (!isOp(')')) { args.push(orExpr()); while (isOp(',')) { i++; args.push(orExpr()); } }
    expect(')');
    return { k: 'call', name: tok.v, args };
  };
  const tree = orExpr();
  if (i !== tokens.length) throw xpathError(source);
  return tree;
};

// Avaliação. Valores: número, string, booleano, ou conjunto de nós (array em ordem de documento).
const docOrder = (nodes) => {
  const unique = Array.from(new Set(nodes));
  return unique.sort((a, b) => (a === b ? 0 : a.__xattr || b.__xattr ? xattrOrder(a, b) : a.compareDocumentPosition(b) & 4 ? -1 : 1));
};
const xattrOwner = (n) => (n.__xattr ? n.__owner : n);
const xattrOrder = (a, b) => {
  const oa = xattrOwner(a), ob = xattrOwner(b);
  if (oa !== ob) return oa.compareDocumentPosition(ob) & 4 ? -1 : 1;
  return a.__xattr ? (b.__xattr ? 0 : 1) : -1;
};
const xattrs = (el) => (el.nodeType === 1 ? el.getAttributeNames().map((name) => { const a = makeAttr(el, name, el.getAttribute(name)); hidden(a, '__xattr', true); return a; }) : []);
const stringValue = (n) => (n.__xattr ? n.value : n.nodeType === 9 ? (n.documentElement ? n.documentElement.textContent : '') : n.textContent ?? '');
const toStringValue = (v) => (Array.isArray(v) ? (v.length ? stringValue(v[0]) : '') : typeof v === 'number' ? (Number.isInteger(v) ? String(v) : Number.isNaN(v) ? 'NaN' : String(v)) : String(v));
const toNumberValue = (v) => { if (typeof v === 'number') return v; if (typeof v === 'boolean') return v ? 1 : 0; const s = toStringValue(v).trim(); return s === '' || !/^-?(\d+(\.\d*)?|\.\d+)$/.test(s) ? NaN : Number(s); };
const toBooleanValue = (v) => (Array.isArray(v) ? v.length > 0 : typeof v === 'number' ? v !== 0 && !Number.isNaN(v) : typeof v === 'string' ? v.length > 0 : !!v);

const axisNodes = (node, axis) => {
  const kids = (n) => (n.__xattr ? [] : Array.from(n.childNodes));
  const descendants = (n, out = []) => { for (const c of kids(n)) { out.push(c); descendants(c, out); } return out; };
  const parent = (n) => (n.__xattr ? n.__owner : n.parentNode);
  const ancestors = (n) => { const out = []; for (let p = parent(n); p; p = p.parentNode) out.push(p); return out; };
  switch (axis) {
    case 'child': return kids(node);
    case 'descendant': return descendants(node);
    case 'descendant-or-self': return [node, ...descendants(node)];
    case 'self': return [node];
    case 'parent': { const p = parent(node); return p ? [p] : []; }
    case 'ancestor': return ancestors(node);
    case 'ancestor-or-self': return [node, ...ancestors(node)];
    case 'attribute': return xattrs(node);
    case 'following-sibling': { if (node.__xattr) return []; const out = []; for (let s = node.nextSibling; s; s = s.nextSibling) out.push(s); return out; }
    case 'preceding-sibling': { if (node.__xattr) return []; const out = []; for (let s = node.previousSibling; s; s = s.previousSibling) out.push(s); return out; }
    case 'following': { const out = []; for (let n = xattrOwner(node); n; n = parent(n)) for (let s = n.nextSibling; s; s = s.nextSibling) { out.push(s); descendants(s, out); } return out; }
    case 'preceding': {
      const out = [];
      const anc = new Set(ancestors(node));
      for (let n = xattrOwner(node); n; n = parent(n)) for (let s = n.previousSibling; s; s = s.previousSibling) { const block = [s, ...descendants(s)]; out.push(...block.reverse()); }
      return out.filter((n) => !anc.has(n));
    }
    default: return [];
  }
};
const reverseAxis = (axis) => ['ancestor', 'ancestor-or-self', 'preceding', 'preceding-sibling'].includes(axis);
const matchesTest = (n, test, axis) => {
  switch (test.type) {
    case 'node': return true;
    case 'text': return !n.__xattr && n.nodeType === 3;
    case 'comment': return !n.__xattr && n.nodeType === 8;
    case 'processing-instruction': return false;
    default: {
      const principal = axis === 'attribute' ? !!n.__xattr : !n.__xattr && n.nodeType === 1;
      if (!principal) return false;
      if (test.name === '*') return true;
      const name = n.__xattr ? n.name : n.localName;
      const wanted = test.name.includes(':') ? test.name.slice(test.name.indexOf(':') + 1) : test.name;
      if (wanted === '*') return true;
      return n.__xattr || n.namespaceURI !== HTML_NS ? name === wanted : name === wanted.toLowerCase();
    }
  }
};

const xpathFunctions = {
  last: (ctx) => ctx.size,
  position: (ctx) => ctx.position,
  count: (ctx, [a]) => a.length,
  id: (ctx, [a]) => { const ids = (Array.isArray(a) ? a.map(stringValue).join(' ') : toStringValue(a)).split(/\s+/).filter(Boolean); return docOrder(ids.map((x) => document.getElementById(x)).filter(Boolean)); },
  'local-name': (ctx, args) => { const n = args.length ? args[0][0] : ctx.node; return !n ? '' : n.__xattr ? n.name : n.nodeType === 1 ? n.localName : ''; },
  name: (ctx, args) => { const n = args.length ? args[0][0] : ctx.node; return !n ? '' : n.__xattr ? n.name : n.nodeType === 1 ? n.localName : ''; },
  'namespace-uri': (ctx, args) => { const n = args.length ? args[0][0] : ctx.node; return n && !n.__xattr && n.nodeType === 1 ? n.namespaceURI || '' : ''; },
  string: (ctx, args) => toStringValue(args.length ? args[0] : [ctx.node]),
  concat: (ctx, args) => args.map(toStringValue).join(''),
  'starts-with': (ctx, [a, b]) => toStringValue(a).startsWith(toStringValue(b)),
  'ends-with': (ctx, [a, b]) => toStringValue(a).endsWith(toStringValue(b)),
  contains: (ctx, [a, b]) => toStringValue(a).includes(toStringValue(b)),
  'substring-before': (ctx, [a, b]) => { const s = toStringValue(a), t = toStringValue(b); const k = s.indexOf(t); return k < 0 ? '' : s.slice(0, k); },
  'substring-after': (ctx, [a, b]) => { const s = toStringValue(a), t = toStringValue(b); const k = s.indexOf(t); return k < 0 ? '' : s.slice(k + t.length); },
  substring: (ctx, [a, b, c]) => {
    const s = Array.from(toStringValue(a));
    const start = Math.round(toNumberValue(b));
    const end = c === undefined ? Infinity : start + Math.round(toNumberValue(c));
    return s.filter((_, k) => k + 1 >= start && k + 1 < end).join('');
  },
  'string-length': (ctx, args) => Array.from(toStringValue(args.length ? args[0] : [ctx.node])).length,
  'normalize-space': (ctx, args) => toStringValue(args.length ? args[0] : [ctx.node]).replace(/[ \t\n\r]+/g, ' ').trim(),
  translate: (ctx, [a, b, c]) => { const from = Array.from(toStringValue(b)), to = Array.from(toStringValue(c)); return Array.from(toStringValue(a)).map((ch) => { const k = from.indexOf(ch); return k < 0 ? ch : k < to.length ? to[k] : ''; }).join(''); },
  boolean: (ctx, [a]) => toBooleanValue(a),
  not: (ctx, [a]) => !toBooleanValue(a),
  true: () => true,
  false: () => false,
  lang: (ctx, [a]) => { const want = toStringValue(a).toLowerCase(); for (let n = xattrOwner(ctx.node); n && n.nodeType === 1; n = n.parentNode) { const l = n.getAttribute('lang'); if (l !== null) { const v = l.toLowerCase(); return v === want || v.startsWith(want + '-'); } } return false; },
  number: (ctx, args) => toNumberValue(args.length ? args[0] : [ctx.node]),
  sum: (ctx, [a]) => a.reduce((s, n) => s + toNumberValue(stringValue(n)), 0),
  floor: (ctx, [a]) => Math.floor(toNumberValue(a)),
  ceiling: (ctx, [a]) => Math.ceil(toNumberValue(a)),
  round: (ctx, [a]) => { const v = toNumberValue(a); return Number.isNaN(v) || !Number.isFinite(v) ? v : Math.floor(v + 0.5); },
};

const compareValues = (op, a, b) => {
  const cmp = (x, y) => {
    if (op === '=' || op === '!=') {
      let eq;
      if (typeof x === 'boolean' || typeof y === 'boolean') eq = toBooleanValue(x) === toBooleanValue(y);
      else if (typeof x === 'number' || typeof y === 'number') eq = toNumberValue(x) === toNumberValue(y);
      else eq = toStringValue(x) === toStringValue(y);
      return op === '=' ? eq : !eq;
    }
    const l = toNumberValue(x), r = toNumberValue(y);
    return op === '<' ? l < r : op === '<=' ? l <= r : op === '>' ? l > r : l >= r;
  };
  const aSet = Array.isArray(a), bSet = Array.isArray(b);
  if (aSet && bSet) return a.some((x) => b.some((y) => cmp(stringValue(x), stringValue(y))));
  if (aSet) return typeof b === 'boolean' ? cmp(toBooleanValue(a), b) : a.some((x) => cmp(typeof b === 'number' ? toNumberValue(stringValue(x)) : stringValue(x), b));
  if (bSet) return typeof a === 'boolean' ? cmp(a, toBooleanValue(b)) : b.some((y) => cmp(a, typeof a === 'number' ? toNumberValue(stringValue(y)) : stringValue(y)));
  return cmp(a, b);
};

const applyPredicates = (nodes, preds, reverse) => {
  let current = nodes;
  for (const pred of preds) {
    const size = current.length;
    current = current.filter((node, k) => {
      const position = k + 1;
      const value = xpathEval(pred, { node, position, size });
      return typeof value === 'number' ? value === position : toBooleanValue(value);
    });
  }
  return current;
};

function xpathEval(expr, ctx) {
  switch (expr.k) {
    case 'num': return expr.v;
    case 'str': return expr.v;
    case 'neg': return -toNumberValue(xpathEval(expr.e, ctx));
    case 'root': { let n = xattrOwner(ctx.node); while (n.parentNode) n = n.parentNode; return [n]; }
    case 'union': { const l = xpathEval(expr.l, ctx), r = xpathEval(expr.r, ctx); if (!Array.isArray(l) || !Array.isArray(r)) throw new TypeError('The expression is not a node-set.'); return docOrder([...l, ...r]); }
    case 'filter': { const base = xpathEval(expr.e, ctx); if (!Array.isArray(base)) throw new TypeError('The expression is not a node-set.'); return applyPredicates(base, [expr.p], false); }
    case 'call': {
      const fn = xpathFunctions[expr.name];
      if (!fn) throw domException(`Failed to execute 'evaluate' on 'Document': The string '${expr.name}()' is not a valid XPath function.`, 'SyntaxError');
      return fn(ctx, expr.args.map((a) => xpathEval(a, ctx)));
    }
    case 'path': {
      let nodes = expr.base ? xpathEval(expr.base, ctx) : [ctx.node];
      if (!Array.isArray(nodes)) throw new TypeError('The expression is not a node-set.');
      for (const step of expr.steps) {
        const out = [];
        for (const n of nodes) {
          const candidates = axisNodes(n, step.axis).filter((c) => matchesTest(c, step.test, step.axis));
          out.push(...applyPredicates(candidates, step.preds, reverseAxis(step.axis)));
        }
        nodes = docOrder(out);
      }
      return nodes;
    }
    case 'bin': {
      if (expr.op === 'or') return toBooleanValue(xpathEval(expr.l, ctx)) || toBooleanValue(xpathEval(expr.r, ctx));
      if (expr.op === 'and') return toBooleanValue(xpathEval(expr.l, ctx)) && toBooleanValue(xpathEval(expr.r, ctx));
      const l = xpathEval(expr.l, ctx), r = xpathEval(expr.r, ctx);
      if (['=', '!=', '<', '<=', '>', '>='].includes(expr.op)) return compareValues(expr.op, l, r);
      const a = toNumberValue(l), b = toNumberValue(r);
      return expr.op === '+' ? a + b : expr.op === '-' ? a - b : expr.op === '*' ? a * b : expr.op === 'div' ? a / b : a % b;
    }
    default: throw xpathError('');
  }
}

class XPathResult {
  constructor() { throw new TypeError('Illegal constructor'); }
  get resultType() { return this.__type; }
  get numberValue() { if (this.__type !== 1) throw new TypeError("Failed to read the 'numberValue' property from 'XPathResult': The result type is not a number."); return this.__value; }
  get stringValue() { if (this.__type !== 2) throw new TypeError("Failed to read the 'stringValue' property from 'XPathResult': The result type is not a string."); return this.__value; }
  get booleanValue() { if (this.__type !== 3) throw new TypeError("Failed to read the 'booleanValue' property from 'XPathResult': The result type is not a boolean."); return this.__value; }
  get singleNodeValue() { if (this.__type !== 8 && this.__type !== 9) throw new TypeError("Failed to read the 'singleNodeValue' property from 'XPathResult': The result type is not a single node."); return this.__value[0] || null; }
  get snapshotLength() { if (this.__type !== 6 && this.__type !== 7) throw new TypeError("Failed to read the 'snapshotLength' property from 'XPathResult': The result type is not a snapshot."); return this.__value.length; }
  get invalidIteratorState() { return false; }
  iterateNext() { if (this.__type !== 4 && this.__type !== 5) throw new TypeError("Failed to execute 'iterateNext' on 'XPathResult': The result type is not an iterator."); return this.__value[this.__cursor++] || null; }
  snapshotItem(i) { if (this.__type !== 6 && this.__type !== 7) throw new TypeError("Failed to execute 'snapshotItem' on 'XPathResult': The result type is not a snapshot."); return this.__value[i] || null; }
}
Object.assign(XPathResult, XPATH_TYPES);
Object.assign(XPathResult.prototype, XPATH_TYPES);
tag(XPathResult, 'XPathResult');

class XPathExpression {
  constructor() { throw new TypeError('Illegal constructor'); }
  evaluate(contextNode, type = 0) {
    if (!isNode(contextNode)) throw new TypeError("Failed to execute 'evaluate' on 'XPathExpression': parameter 1 is not of type 'Node'.");
    let value = xpathEval(this.__tree, { node: contextNode, position: 1, size: 1 });
    let resultType = Number(type) || 0;
    if (resultType === 0) resultType = Array.isArray(value) ? 4 : typeof value === 'number' ? 1 : typeof value === 'string' ? 2 : 3;
    if (resultType === 1) value = toNumberValue(value);
    else if (resultType === 2) value = toStringValue(value);
    else if (resultType === 3) value = toBooleanValue(value);
    else if (!Array.isArray(value)) throw new TypeError("Failed to execute 'evaluate' on 'XPathExpression': The result is not a node set, and therefore cannot be converted to the desired type.");
    const result = Object.create(XPathResult.prototype);
    hidden(result, '__type', resultType);
    hidden(result, '__value', value);
    hidden(result, '__cursor', 0);
    return result;
  }
}
tag(XPathExpression, 'XPathExpression');

class XPathEvaluator {
  createExpression(expression) {
    const compiled = Object.create(XPathExpression.prototype);
    hidden(compiled, '__tree', xpathParse(String(expression)));
    return compiled;
  }
  createNSResolver(node) { return node; }
  evaluate(expression, contextNode, resolver, type, result) { return this.createExpression(expression).evaluate(contextNode, type); }
}
tag(XPathEvaluator, 'XPathEvaluator');
