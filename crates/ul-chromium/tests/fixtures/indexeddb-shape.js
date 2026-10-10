const R = [];
const shape = (o) => Object.getOwnPropertyNames(o).map((k) => { const d = Object.getOwnPropertyDescriptor(o, k); return k + ':' + ('value' in d ? (typeof d.value === 'function' ? 'f' + d.value.length : 'v') + (d.writable ? 'w' : '') : 'a' + (d.set ? 's' : '')) + (d.enumerable ? 'e' : '') + (d.configurable ? 'c' : ''); }).join(',');
R.push('window.indexedDB ' + JSON.stringify(Object.keys(Object.getOwnPropertyDescriptor(window, 'indexedDB'))) + ' ' + Object.prototype.toString.call(indexedDB));
for (const name of Object.getOwnPropertyNames(window).filter((n) => /^IDB/.test(n)).sort()) {
  const C = window[name];
  R.push([name, C.length, shape(C.prototype), Object.getPrototypeOf(C.prototype).constructor.name, 'static:' + shape(C).replace(/(length|name|prototype):[^,]*,?/g, '')].join(' '));
  try { new C(); } catch (e) { R.push(' new ' + e.name + ': ' + e.message); }
}
console.log(R.join('\n'));
