function boom(): never {
  throw new Error('x');
}
try { boom(); } catch (e) { console.log((e as Error).stack!.split('\n').slice(0, 3).join('|')); }
