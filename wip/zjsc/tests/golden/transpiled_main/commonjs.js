// CommonJS: `module.exports` marca o arquivo.
const dead = 1;
function f(x) {
    // comentário
    return x.y.z;
}
module.exports = { f };
try { f({}); } catch (e) { console.log(e.stack.split("\n").slice(0, 3).join("\n")); }
try { new module.exports.f.x(); } catch (e) { console.log(e.message); }
console.log(String(f));
console.log(typeof require, typeof module, __filename);
console.log(new Error("e").stack.split("\n")[1]);
