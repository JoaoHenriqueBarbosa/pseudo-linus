(function (src) {
  function ser(v, depth, seen) {
    var t = typeof v;
    if (v === undefined) return "undefined";
    if (v === null) return "null";
    if (t === "boolean") return "bool:" + v;
    if (t === "number") return "num:" + (Object.is(v, -0) ? "-0" : String(v));
    if (t === "bigint") return "big:" + String(v);
    if (t === "string") return "str:" + JSON.stringify(v);
    if (t === "symbol") return "sym:" + String(v);
    if (t === "function") return "fn:" + v.name + "/" + v.length;
    if (depth > 4) return "deep";
    if (seen.indexOf(v) >= 0) return "cycle";
    seen = seen.concat([v]);
    var d = depth + 1;
    if (Array.isArray(v)) {
      var parts = [];
      var holes = 0;
      for (var i = 0; i < v.length && i < 40; i++) {
        if (i in v) {
          if (holes) { parts.push("<" + holes + " holes>"); holes = 0; }
          parts.push(ser(v[i], d, seen));
        } else holes++;
      }
      if (holes) parts.push("<" + holes + " holes>");
      var extra = Object.keys(v).filter(function (k) { return String(k >>> 0) !== k; });
      var tail = extra.map(function (k) { return k + "=" + ser(v[k], d, seen); });
      return "arr(" + v.length + ")[" + parts.concat(tail).join(",") + "]";
    }
    if (v instanceof Map) {
      var m = [];
      v.forEach(function (val, key) { m.push(ser(key, d, seen) + "=>" + ser(val, d, seen)); });
      return "map{" + m.join(",") + "}";
    }
    if (v instanceof Set) {
      var s = [];
      v.forEach(function (val) { s.push(ser(val, d, seen)); });
      return "set{" + s.join(",") + "}";
    }
    if (v instanceof WeakMap) return "weakmap";
    if (v instanceof WeakSet) return "weakset";
    if (v instanceof Date) return "date:" + (isNaN(v) ? "invalid" : v.toISOString());
    if (v instanceof RegExp) return "re:" + String(v) + ":" + v.lastIndex;
    if (v instanceof Error) return "err:" + v.name + ":" + JSON.stringify(v.message);
    var keys = Object.keys(v);
    var proto = Object.getPrototypeOf(v);
    var tag = proto === null ? "nullproto" : proto === Object.prototype ? "obj" : "obj<" + Object.prototype.toString.call(v) + ">";
    var o = keys.map(function (k) { return k + ":" + ser(v[k], d, seen); });
    return tag + "{" + o.join(",") + "}";
  }
  try {
    return "ok\t" + ser((0, eval)(src), 0, []);
  } catch (e) {
    if (e === null || (typeof e !== "object" && typeof e !== "function")) return "thrown\t" + typeof e + "\t" + String(e);
    return "error\t" + String(e.name) + "\t" + JSON.stringify(String(e.message));
  }
})
