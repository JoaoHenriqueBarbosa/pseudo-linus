(function (src) {
  var buffer = new DataView(new ArrayBuffer(8));
  function hex32(n) {
    var s = n.toString(16);
    while (s.length < 8) s = "0" + s;
    return s;
  }
  function bits(x) {
    if (x !== x) return "nan";
    buffer.setFloat64(0, x);
    return hex32(buffer.getUint32(0)) + hex32(buffer.getUint32(4));
  }
  function keyName(key) {
    return typeof key === "symbol" ? "[" + key.toString() + "]" : JSON.stringify(key);
  }
  function escapeLine(text) {
    return text.split("\n").join("\\n").split("\r").join("\\r").split("\t").join("\\t");
  }
  function serialize(value, seen) {
    var type = typeof value;
    if (type === "number") return bits(value);
    if (type === "string") return JSON.stringify(value);
    if (type === "bigint") return value.toString() + "n";
    if (type === "symbol") return value.toString();
    if (type !== "object" && type !== "function") return String(value);
    if (value === null) return "null";
    var cycle = seen.indexOf(value);
    if (cycle >= 0) return "#cycle" + cycle;
    if (type === "function") return "[function " + JSON.stringify(value.name) + "/" + value.length + "]";
    seen.push(value);
    var tag = Object.prototype.toString.call(value).slice(8, -1);
    var out = tag;
    var proto = Object.getPrototypeOf(value);
    if (proto === null) {
      out += "<null>";
    } else {
      var ctor = proto.constructor;
      out += "<" + (typeof ctor === "function" ? ctor.name : "?") + ">";
    }
    var extra = [];
    if (tag === "Error") {
      out += "{name:" + serialize(value.name, seen) + ",message:" + serialize(value.message, seen) + "}";
      seen.pop();
      return out;
    }
    if (tag === "Date") {
      out += "(" + bits(Date.prototype.getTime.call(value)) + ")";
    } else if (tag === "RegExp") {
      out += "(" + JSON.stringify(String(value)) + ")";
    } else if (tag === "Map") {
      Map.prototype.forEach.call(value, function (v, k) {
        extra.push(serialize(k, seen) + "=>" + serialize(v, seen));
      });
      out += "(" + extra.join(",") + ")";
    } else if (tag === "Set") {
      Set.prototype.forEach.call(value, function (v) {
        extra.push(serialize(v, seen));
      });
      out += "(" + extra.join(",") + ")";
    }
    var keys = Reflect.ownKeys(value);
    var parts = [];
    for (var i = 0; i < keys.length; i++) {
      var key = keys[i];
      var descriptor = Reflect.getOwnPropertyDescriptor(value, key);
      if (!descriptor) continue;
      var text;
      if ("value" in descriptor) {
        text = serialize(descriptor.value, seen);
      } else {
        text = "accessor(" + (descriptor.get ? "g" : "") + (descriptor.set ? "s" : "") + ")";
      }
      parts.push((descriptor.enumerable ? "" : "~") + keyName(key) + ":" + text);
    }
    out += "{" + parts.join(",") + "}";
    seen.pop();
    return out;
  }
  var result;
  try {
    result = (0, eval)(src);
  } catch (error) {
    var text;
    try {
      text = error.constructor.name + ": " + error.message;
    } catch (inner) {
      text = String(error);
    }
    return "throw\t" + escapeLine(text);
  }
  return typeof result + "\t" + serialize(result, []);
})
