function inner(n: number): number {
  if (n > 2) throw new RangeError('too big: ' + n);
  return inner(n + 1);
}
function outer() { return inner(0); }
outer();
