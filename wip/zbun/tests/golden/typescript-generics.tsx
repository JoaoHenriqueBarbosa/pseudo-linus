const f = <T,>(a: T): T => a;
const g = <T extends object>(a: T) => a;
const h = async <T,>(a: T) => a;
function k<T, U extends T[] = T[]>(a: T, b: U): [T, U] { return [a, b]; }
class P<T> { constructor(public v: T) {} m<U>(u: U): U { return u; } }
const x = new P<number>(1), y = f<string>("a"), z = k<number, number[]>(1, [2]);
const el = <P<number> v={1} />;
const a = f as <T>(a: T) => T;
console.log(x, y, z, el, a, g, h);
