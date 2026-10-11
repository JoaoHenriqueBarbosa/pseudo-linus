namespace B { export namespace C { export const D = 1; } }
export import E = B.C;
console.log(E);
