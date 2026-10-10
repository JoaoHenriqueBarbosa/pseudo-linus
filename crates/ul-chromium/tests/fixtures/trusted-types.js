const out = []; const t = (f) => { try { const v = f(); return v && typeof v === 'object' ? Object.prototype.toString.call(v) + JSON.stringify(v) : typeof v + ':' + String(v); } catch (e) { return e.name + ': ' + e.message; } };
const f = trustedTypes;
const pol = f.createPolicy('a', { createHTML: (s, ...r) => 'H:' + s + r.join('|'), createScript: (s) => s, createScriptURL: (s) => s });
out.push(Object.prototype.toString.call(f), Object.prototype.toString.call(pol), pol.name, f.defaultPolicy,
  t(() => new TrustedHTML()), t(() => TrustedHTML()), t(() => new TrustedTypePolicy()), TrustedHTML.prototype.toString.length, Object.getOwnPropertyNames(TrustedHTML.prototype),
  t(() => pol.createHTML('x', 'y', 'z')), JSON.stringify(pol.createHTML('q')), pol.createHTML('<b>') + '!', t(() => pol.createHTML()), t(() => pol.createHTML(Symbol())),
  pol.createHTML('x') === pol.createHTML('x'), f.emptyHTML === f.emptyHTML, f.emptyScript.toString() === '', f.emptyHTML instanceof TrustedHTML,
  t(() => f.createPolicy()), t(() => f.createPolicy('n', 5)), t(() => f.createPolicy('n2', null).name), t(() => f.createPolicy('b c', {}).name),
  t(() => f.createPolicy('r4', { createHTML: 5 })), t(() => f.createPolicy('r5', { createHTML: null })), t(() => f.createPolicy('r6', {}).createHTML('x')),
  t(() => f.createPolicy('r1', { createHTML: () => 5 }).createHTML('x')), t(() => f.createPolicy('r2', { createHTML: () => null }).createHTML('x')),
  t(() => f.createPolicy('r3', { createHTML: () => { throw new RangeError('boom'); } }).createHTML('x')),
  t(() => f.createPolicy('s', { createScriptURL: () => Symbol('s') }).createScriptURL('x')),
  t(() => f.createPolicy('default', { createHTML: (s) => s }).name), f.defaultPolicy.name, t(() => f.createPolicy('default', {})),
  f.isHTML(pol.createHTML('x')), f.isScript(pol.createHTML('x')), f.isScriptURL('x'), t(() => f.isHTML()), f.isHTML({}),
  f.getAttributeType('IMG', 'onerror'), f.getAttributeType('a', 'href'), f.getAttributeType('script', 'SRC'), f.getAttributeType('script', 'src', 'http://a/'),
  f.getAttributeType('script', 'href', 'http://www.w3.org/2000/svg'), f.getAttributeType('script', 'href'), f.getAttributeType('svg', 'onclick', 'http://www.w3.org/2000/svg'),
  f.getAttributeType('div', 'onclick', 'http://www.w3.org/1999/xhtml', 'http://a/'), f.getAttributeType('iframe', 'srcdoc'), t(() => f.getAttributeType('div')),
  f.getPropertyType('div', 'innerHTML'), f.getPropertyType('div', 'INNERHTML'), f.getPropertyType('SCRIPT', 'text'), f.getPropertyType('script', 'href'), f.getPropertyType('script', 'href', 'http://www.w3.org/2000/svg'), f.getPropertyType('script', 'src', 'http://www.w3.org/2000/svg'),
  f.getTypeMapping() === f.getTypeMapping(), Object.keys(f.getTypeMapping()), f.getTypeMapping('http://www.w3.org/2000/svg'), Object.keys(f.getTypeMapping()['*'].attributes).length,
  JSON.stringify(pol), Object.getOwnPropertyNames(pol.createHTML('x')),
  (() => { const d = document.createElement('div'); d.innerHTML = pol.createHTML('<b>t</b>'); return d.innerHTML; })());
console.log(out.join('\n'));
