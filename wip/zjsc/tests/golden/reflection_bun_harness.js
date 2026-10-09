(function (src) {
  function ser(v, depth) {
    var t = typeof v;
    if (v === null) return "null";
    if (t === "undefined") return "undefined";
    if (t === "string") return JSON.stringify(v);
    if (t === "number") return Object.is(v, -0) ? "-0" : String(v);
    if (t === "boolean" || t === "bigint") return String(v) + (t === "bigint" ? "n" : "");
    if (t === "symbol") return v.toString();
    if (t === "function") return "[fn " + String(Object.getOwnPropertyDescriptor(v, "name") && v.name) + "]";
    if (depth > 3) return "[deep]";
    var isArr = Array.isArray(v);
    var keys = Reflect.ownKeys(v);
    var parts = [];
    for (var i = 0; i < keys.length; i++) {
      var k = keys[i];
      var d = Object.getOwnPropertyDescriptor(v, k);
      var ks = typeof k === "symbol" ? k.toString() : k;
      if (isArr && ks === "length") continue;
      var body = d === undefined ? "?" : "value" in d ? ser(d.value, depth + 1) : "<accessor " + (d.get ? "g" : "") + (d.set ? "s" : "") + ">";
      parts.push((isArr ? "" : ks + ":") + body);
    }
    return (isArr ? "[" : "{") + parts.join(",") + (isArr ? "]" : "}");
  }
  function describe(e) {
    if (e === null || (typeof e !== "object" && typeof e !== "function")) {
      return "thrown\t" + typeof e + "\t" + String(e);
    }
    return "error\t" + String(e.name) + "\t" + JSON.stringify(String(e.message));
  }
  var result;
  try {
    result = (0, eval)(src);
  } catch (e) {
    return describe(e);
  }
  try {
    return "ok\t" + ser(result, 0);
  } catch (e) {
    return "serfail\t" + describe(e);
  }
})
