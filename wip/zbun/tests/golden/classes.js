class A {}
class B extends A { constructor() { super(); this.x = 1; } }
class C { static s = 1; static #p = 2; #priv = 3; pub = 4; static { C.init = 1; } get g() { return this.#priv; } set g(v) { this.#priv = v; } static m() {} async am() {} *gm() {} async *agm() {} #pm() {} static #spm() {} ["computed"]() {} [`t`] = 1; "str" = 2; 3 = 4; accessor acc = 1; }
const D = class {}, E = class Named extends (A) {}, F = class extends A.B {};
class G extends (class {}) {}
class H { constructor(a, b) { this.a = a; } static async *[Symbol.asyncIterator]() {} }
class I { x = this.#y; #y = 1; static has(o) { return #y in o; } }
new A, new B(), new (class {})();
console.log(A, B, C, D, E, F, G, H, I);
