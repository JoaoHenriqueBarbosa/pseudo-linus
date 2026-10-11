import fs = require("fs");
import A = B.C.D;
namespace B { export namespace C { export const D = 1; } }
console.log(fs, A);
