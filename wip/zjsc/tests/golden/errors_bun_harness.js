(function (src) {
  function describe(e) {
    if (e === null || (typeof e !== "object" && typeof e !== "function")) {
      return "thrown\t" + typeof e + "\t" + String(e);
    }
    var name = String(e.name);
    var message = JSON.stringify(String(e.message));
    var c = e.constructor;
    var ctor = typeof c === "function" ? c.name : "?";
    var text = String(e);
    var header = String(typeof e.stack === "string" && e.stack.slice(0, text.length) === text);
    return "error\t" + name + "\t" + message + "\t" + ctor + "\t" + header;
  }
  try {
    (0, eval)(src);
  } catch (e) {
    return describe(e);
  }
  return "nothrow";
})
