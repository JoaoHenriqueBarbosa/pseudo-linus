switch (a) {
  case 1:
  case 2: {
    break;
  }
  case 3:
    b();
    // fallthrough
  case 4: c(); break;
  default:
    d();
}
switch (a) { default: }
switch (a) { case 1: }
switch (a) { case (1, 2): case a ? 1 : 2: case function () {}: }
switch (a) { case 1: let x = 1; { break; } }
var a, b, c, d;
