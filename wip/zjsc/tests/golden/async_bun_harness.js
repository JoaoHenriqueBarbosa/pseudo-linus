globalThis.log = [];
globalThis.__err = null;
globalThis.L = function (x) { log.push(x); };
globalThis.tick = function (n, label) {
  var p = Promise.resolve();
  for (var i = 0; i < n; i++) p = p.then(function () {});
  return p.then(function () { L(label); });
};
globalThis.thenable = function (v, label) {
  return { then: function (res) { L("then:" + label); res(v); } };
};
globalThis.__final = function () {
  return __err !== null ? __err : JSON.stringify(log);
};
globalThis.__run = function (src) {
  try {
    (0, eval)(src);
  } catch (e) {
    __err = "error\t" + e.name + "\t" + JSON.stringify(String(e.message));
  }
};
