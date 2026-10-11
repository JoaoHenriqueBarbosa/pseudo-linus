const R = []; const t = (n, f) => { try { f(); R.push([n, 'ok']); } catch (e) { R.push([n, e.name]); } };
const d = document.createElement('div');
t('before any meta', () => { d.innerHTML = 'a'; });
const m = document.createElement('meta'); m.httpEquiv = 'Content-Security-Policy'; m.content = "require-trusted-types-for 'script'";
t('detached meta', () => { d.innerHTML = 'b'; });
document.head.append(m);
t('same script after insert', () => { d.innerHTML = 'c'; });
m.remove();
t('after remove', () => { d.innerHTML = 'd'; });
const m3 = document.createElement('meta'); m3.setAttribute('http-equiv', 'content-security-policy'); m3.setAttribute('content', 'trusted-types none'); document.body.append(m3);
t('body meta', () => { trustedTypes.createPolicy('zz', {}); });
const m4 = document.createElement('meta'); m4.setAttribute('http-equiv', 'Content-Security-Policy-Report-Only'); m4.setAttribute('content', 'trusted-types none'); document.head.append(m4);
t('report-only meta', () => { trustedTypes.createPolicy('yy', {}); });
setTimeout(() => {
  t('later task', () => { d.innerHTML = 'f'; });
  t('later policy', () => { trustedTypes.createPolicy('vv', {}); });
  console.log(JSON.stringify(R));
}, 50);
