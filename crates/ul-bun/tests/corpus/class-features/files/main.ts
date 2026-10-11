abstract class Base { abstract id(): string; static #count = 0; static next() { return ++Base.#count; } }
class Impl extends Base { #secret = 42; id() { return 'impl' + this.#secret; } get s() { return this.#secret; } static { Impl.registered = true; } static registered = false; }
console.log(new Impl().id(), Base.next(), Base.next(), Impl.registered, new Impl().s);
const o = { a: 1, b: { c: [1, 2, { d: 3 }] } } as const;
console.log(o, o?.b?.c?.[2]?.d ?? 'none');
