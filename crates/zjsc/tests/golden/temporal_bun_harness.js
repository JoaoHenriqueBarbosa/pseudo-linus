(function (src) {
  function show(v) {
    if (typeof v === "string") return JSON.stringify(v);
    if (typeof v === "bigint") return v + "n";
    if (typeof v === "number") return Object.is(v, -0) ? "-0" : String(v);
    if (Array.isArray(v)) return "[" + v.map(show).join(",") + "]";
    if (typeof v === "symbol") return v.toString();
    if (v !== null && typeof v === "object" && Object.getPrototypeOf(v) === Object.prototype) {
      return "{" + Object.keys(v).map(function (k) { return k + ":" + show(v[k]); }).join(",") + "}";
    }
    return String(v);
  }
  try {
    return "ok\t" + show((0, eval)(src));
  } catch (e) {
    if (e === null || (typeof e !== "object" && typeof e !== "function")) return "thrown\t" + String(e);
    return "error\t" + String(e.name) + "\t" + JSON.stringify(String(e.message));
  }
})
