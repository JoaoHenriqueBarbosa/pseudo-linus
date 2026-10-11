(function (src) {
  function ser(v, depth, seen) {
    var t = typeof v;
    if (v === undefined) return "undefined";
    if (v === null) return "null";
    if (t === "number") return Object.is(v, -0) ? "-0" : String(v);
    if (t === "boolean") return String(v);
    if (t === "bigint") return String(v) + "n";
    if (t === "string") return JSON.stringify(v);
    if (t === "symbol") return String(v);
    if (v === globalThis) return "[global]";
    if (depth > 6) return "[deep]";
    if (seen.indexOf(v) >= 0) return "[cycle]";
    seen = seen.concat([v]);
    if (t === "function") {
      var nd = Object.getOwnPropertyDescriptor(v, "name");
      var ld = Object.getOwnPropertyDescriptor(v, "length");
      return "function(" + (nd ? ser(nd.value, depth + 1, seen) : "-") + "," + (ld ? ser(ld.value, depth + 1, seen) : "-") + ")";
    }
    if (Array.isArray(v)) {
      var items = [];
      for (var i = 0; i < v.length; i++) items.push(i in v ? ser(v[i], depth + 1, seen) : "<hole>");
      return "[" + items.join(",") + "]";
    }
    var tag = Object.prototype.toString.call(v);
    if (tag === "[object Date]" || tag === "[object RegExp]" || tag === "[object Error]") {
      return tag + ":" + String(v);
    }
    var keys = Reflect.ownKeys(v);
    var parts = [];
    for (var j = 0; j < keys.length; j++) {
      var k = keys[j];
      var d = Object.getOwnPropertyDescriptor(v, k);
      var kk = typeof k === "symbol" ? String(k) : JSON.stringify(k);
      if (!("value" in d)) parts.push(kk + ":<accessor>");
      else parts.push(kk + ":" + ser(d.value, depth + 1, seen));
    }
    return tag + "{" + parts.join(",") + "}";
  }
  function describe(e) {
    if (e === null || (typeof e !== "object" && typeof e !== "function")) {
      return "thrown\t" + typeof e + "\t" + String(e);
    }
    return "error\t" + String(e.name) + "\t" + JSON.stringify(String(e.message));
  }
  var value;
  try {
    value = (0, eval)(src);
  } catch (e) {
    return describe(e);
  }
  try {
    return "value\t" + ser(value, 0, []);
  } catch (e) {
    return "serialize\t" + describe(e);
  }
})
