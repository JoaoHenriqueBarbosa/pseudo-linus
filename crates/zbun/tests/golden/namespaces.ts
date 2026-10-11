namespace A { export const x = 1; export function f() { return x; } export class C {} export enum E { P } }
namespace A.B.C { export const deep = 2; }
namespace A { export const merged = 3; }
export namespace Exp { export let v = 1; }
console.log(A.x, A.f(), A.B.C.deep, A.merged, Exp.v);
