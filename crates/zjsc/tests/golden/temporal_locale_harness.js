(function (src) {
  function ascii(text) {
    return text.replace(/[^\x20-\x7e]/g, function (c) {
      return "\\u" + c.charCodeAt(0).toString(16).padStart(4, "0");
    });
  }
  try {
    var value = (0, eval)(src);
    return "ok:" + ascii(typeof value === "string" ? JSON.stringify(value) : JSON.stringify(value) + "");
  } catch (e) {
    return "throw:" + ascii(e.name + ": " + e.message);
  }
})
