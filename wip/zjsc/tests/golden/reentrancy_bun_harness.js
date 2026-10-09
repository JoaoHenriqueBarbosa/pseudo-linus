(function (source) {
  function ser(value) {
    try {
      return JSON.stringify(value, function (key, item) {
        if (typeof item === 'bigint') return item + 'n';
        if (item === undefined) return '#undefined';
        if (typeof item === 'number' && !isFinite(item)) return '#' + item;
        if (typeof item === 'function') return '#function';
        if (typeof item === 'symbol') return '#symbol';
        if (ArrayBuffer.isView(item)) return 'TA:' + Array.prototype.join.call(item, ',');
        if (item instanceof ArrayBuffer) return 'AB:' + item.byteLength;
        if (item instanceof Map) return 'Map:' + JSON.stringify(Array.from(item));
        if (item instanceof Set) return 'Set:' + JSON.stringify(Array.from(item));
        return item;
      });
    } catch (error) {
      return 'serr:' + error.name;
    }
  }
  try {
    var result = (0, eval)(source);
    return 'ok ' + ser(result);
  } catch (error) {
    return 'throw ' + (error && error.name);
  }
})
