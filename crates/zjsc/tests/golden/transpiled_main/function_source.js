// Comentário no topo.
function plain(x) {
    // comentário dentro
    const dead = 1;
    return x + 1;
}
const arrow = (a, b) => {
    /* bloco */
    return a * b;
};
class K {
    // comentário de classe
    static s = 1;
    m(x) {
        return x ? "y" : 'n';
    }
    get g() { return `t${1}`; }
}
async function af() { return await 1; }
function* gf() { yield 1; }
const o = { method() { return void 0; }, ["k" + 1]: 2 };
console.log(String(plain));
console.log(String(arrow));
console.log(String(K));
console.log(String(K.prototype.m));
console.log(String(Object.getOwnPropertyDescriptor(K.prototype, "g").get));
console.log(String(af));
console.log(String(gf));
console.log(String(o.method));
console.log(plain.toString().length, K.toString().length);
console.log(String(function () { return 日本; }));
var 日本 = "é";
