const R = []; const t = (n, f) => { try { const v = f(); R.push([n, 'ok', typeof v + ':' + String(v).slice(0, 60)]); } catch (e) { R.push([n, e.name, e.message]); } };
const V = []; const mask = (s) => (typeof s === 'string' ? s.split(location.origin).join('O') : s);
document.addEventListener('securitypolicyviolation', (e) => V.push([e.violatedDirective, e.effectiveDirective, e.blockedURI, e.disposition, e.sample, mask(e.sourceFile), e.lineNumber, e.columnNumber, e.statusCode, e.originalPolicy, mask(e.documentURI), e.bubbles, e.composed, e.isTrusted, e.referrer]));

const f=trustedTypes;
t('a',()=>f.createPolicy('a',{}).name); t('a again',()=>f.createPolicy('a',{}).name); t('c',()=>f.createPolicy('c',{}).name); t('default',()=>f.createPolicy('default',{}).name);

setTimeout(() => { console.log(R.map((r) => JSON.stringify(r)).concat(V.map((v) => 'V ' + JSON.stringify(v))).join('\n')); }, 200);
