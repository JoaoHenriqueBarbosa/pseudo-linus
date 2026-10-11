enum Color { Red, Green = 4, Blue }
enum Mixed { A = 1, B = "b", C = A + 1, D = "x".length }
declare enum Ambient { A }
export enum Exported { X = 1 }
export const enum Inlined { A = 1, B = 2 }
console.log(Color.Red, Mixed.A, Exported.X, Inlined.B, Color[0]);
