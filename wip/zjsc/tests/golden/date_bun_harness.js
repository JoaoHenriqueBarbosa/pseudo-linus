(function (src) {
  function show(value) {
    var type = typeof value;
    if (type === "number") return Object.is(value, -0) ? "-0" : String(value);
    if (type === "string") return JSON.stringify(value);
    if (type === "object" && value !== null && Object.prototype.toString.call(value) === "[object Date]") {
      var time = Date.prototype.getTime.call(value);
      return "Date(" + (time !== time ? "NaN" : Date.prototype.toISOString.call(value)) + ")";
    }
    return String(value);
  }
  try {
    var value = (0, eval)(src);
    return typeof value + "\t" + show(value);
  } catch (error) {
    return "throw\t" + error.name + ": " + error.message;
  }
})
