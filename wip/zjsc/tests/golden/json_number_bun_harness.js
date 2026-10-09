(function (src) {
  function describe(v) {
    var t = typeof v;
    if (t === "string") return "string\t" + JSON.stringify(v);
    if (t === "number") return "number\t" + (Object.is(v, -0) ? "-0" : String(v));
    if (t === "bigint") return "bigint\t" + String(v);
    if (t === "boolean" || t === "undefined") return t + "\t" + String(v);
    if (t === "symbol") return "symbol\t" + v.toString();
    if (v === null) return "null";
    return "object\t" + Object.prototype.toString.call(v);
  }
  try {
    return "ok\t" + describe((0, eval)(src));
  } catch (e) {
    if (e === null || (typeof e !== "object" && typeof e !== "function")) return "thrown\t" + describe(e);
    var ctor = typeof e.constructor === "function" ? e.constructor.name : "?";
    return "error\t" + String(e.name) + "\t" + JSON.stringify(String(e.message)) + "\t" + ctor;
  }
})
