interface I { a: number; b?: string; m(): void }
type T = { a: number } | string;
type G<X> = X extends string ? 1 : 2;
declare const dc: number;
declare function df(): void;
const x: number = 1, y = x as unknown as string, z = <any>x, nn = x!, sat = { a: 1 } satisfies I;
function f<T>(a: T, b?: number, ...rest: string[]): T { return a; }
class A<T> implements I {
  private p: number = 1;
  public readonly q = 2;
  protected static s?: string;
  constructor(private a: number, public b = 2, readonly c?: string) { }
  abstract_?: void;
  m(this: A<T>, x: number): void {}
  get g(): number { return 1; }
}
abstract class AB { abstract m(): void; abstract p: number; }
enum Color { Red, Green = 4, Blue }
const enum CE { A = 1, B = A << 1 }
enum Str { A = "a", B = "b" }
namespace NS { export const a = 1; export function f() {} export namespace Inner { export const b = 2; } }
module M2 { export const c = 3; }
declare module "x" { export const y: number; }
export type { T };
export { type I };
import type { Foo } from "./foo";
import { type Bar, baz } from "./bar";
export const arrow = <T,>(a: T): T => a;
let v = x satisfies number;
for (const k of [1] as number[]) {}
console.log(x, y, z, nn, sat, f, A, AB, Color, CE, Str, NS, M2, v, baz, Color.Red, CE.B, Str.A);
