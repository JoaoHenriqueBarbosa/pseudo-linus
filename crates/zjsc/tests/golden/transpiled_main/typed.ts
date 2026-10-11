enum Color { Red, Green = 5 }
interface Shape { n: number }
type Id = string | number;
function area(s: Shape, scale: number = 2): number {
  // comentário
  return s.n * scale;
}
class P<T> {
  constructor(public v: T, private w: number = 1) {}
  get(): T { return this.v; }
}
const p = new P<string>("x");
console.log(Color.Green, area({ n: 3 }), p.get(), String(area));
console.log(String(P));
try { (p as any).nope(); } catch (e) { console.log((e as Error).message); }
console.log(new Error("ts").stack!.split("\n")[1]);
function thrower(): never { throw new Error("t"); }
try { thrower(); } catch (e) { console.log((e as Error).stack!.split("\n").slice(0, 3).join("\n")); }
