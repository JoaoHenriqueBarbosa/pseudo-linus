const R = []; const t = (n, f) => { try { const v = f(); R.push([n, 'ok', typeof v + ':' + String(v).slice(0, 60)]); } catch (e) { R.push([n, e.name, e.message]); } };
const V = []; const mask = (s) => (typeof s === 'string' ? s.split(location.origin).join('O') : s);
document.addEventListener('securitypolicyviolation', (e) => V.push([e.violatedDirective, e.effectiveDirective, e.blockedURI, e.disposition, e.sample, mask(e.sourceFile), e.lineNumber, e.columnNumber, e.statusCode, e.originalPolicy, mask(e.documentURI), e.bubbles, e.composed, e.isTrusted, e.referrer]));

const f=trustedTypes; const d=document.createElement('div');
const p=f.createPolicy('default',{createHTML:(s,type,sink)=>{R.push(['cb',s,type,sink,typeof this]);return 'D:'+s},createScript:(s,type,sink)=>{R.push(['cbs',s,type,sink]);return s==='bad'?null:s},createScriptURL:(s,type,sink)=>{R.push(['cbu',s,type,sink]);return s}});
t('innerHTML default',()=>{d.innerHTML='x';return d.innerHTML});
t('outerHTML',()=>{const e=document.createElement('p');d.append(e);e.outerHTML='y';return d.innerHTML});
t('eval default',()=>eval('1+1')); t('eval bad',()=>eval('bad')); t('Function',()=>new Function('a','return a+1')(1));
t('setTimeout',()=>clearTimeout(setTimeout('1',99999)));
t('script src',()=>{const s=document.createElement('script');s.src='/x.js';return s.src});
t('setAttribute onclick',()=>{d.setAttribute('onclick','z');return d.getAttribute('onclick')});
t('script text',()=>{const s=document.createElement('script');s.text='2';return s.text});
t('default again',()=>f.createPolicy('default',{}));

setTimeout(() => { console.log(R.map((r) => JSON.stringify(r)).concat(V.map((v) => 'V ' + JSON.stringify(v))).join('\n')); }, 200);
