"use strict";
var S = function S(v) {
  if (typeof v === 'string') return 's:' + JSON.stringify(v);
  if (typeof v === 'number') return Object.is(v, -0) ? 'n:-0' : 'n:' + v;
  if (typeof v === 'bigint') return 'b:' + v;
  if (typeof v === 'symbol') return 'y:' + v.toString();
  if (typeof v === 'function') return 'f:' + v.name + '/' + v.length;
  if (v === null || v === undefined || typeof v === 'boolean') return String(v);
  if (Array.isArray(v)) { var o = []; for (var i = 0; i < v.length; i++) o.push(i in v ? S(v[i]) : '<hole>'); return 'a[' + o.join(',') + ']'; }
  return 'o:' + Object.prototype.toString.call(v);
};
