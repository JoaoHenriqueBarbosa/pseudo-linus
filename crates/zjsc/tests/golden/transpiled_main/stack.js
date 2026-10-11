function thrower(n) {
  if (n === 0) {
    throw new Error("deep");
  }
  return thrower(n - 1);
}
function show(e) {
  console.log(e.stack.split("\n").slice(0, 6).join("\n"));
}
try { thrower(2); } catch (e) { show(e); }
const obj = {
  get g() { return missing(); },
  run() { return this.g; },
};
try { obj.run(); } catch (e) { show(e); }
try { [1].map(() => { throw new TypeError("cb"); }); } catch (e) { show(e); }
try { JSON.parse("{"); } catch (e) { show(e); }
const e1 = new Error("here");
console.log(e1.stack.split("\n")[1]);
function wrap() { return new Error("in wrap"); }
console.log(wrap().stack.split("\n")[1]);
console.log((() => new Error("arrow").stack.split("\n")[1])());
class A { constructor() { this.e = new Error("ctor"); } }
console.log(new A().e.stack.split("\n")[1]);
async function af() {
  await null;
  throw new Error("async");
}
af().catch(show);
Promise.resolve().then(() => { throw new RangeError("then"); }).catch(show);
const cs = (() => { const prep = Error.prepareStackTrace; Error.prepareStackTrace = (_, s) => s; const s = new Error("x").stack; Error.prepareStackTrace = prep; return s; })();
console.log(cs[0].getLineNumber(), cs[0].getColumnNumber(), cs[0].getFileName());
