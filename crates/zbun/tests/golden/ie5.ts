import A = B.C.D;
export import E = B.C;
namespace B { export namespace C { export const D = 1; } }
console.log(A, E);
