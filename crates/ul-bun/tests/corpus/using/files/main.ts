class Res implements Disposable { constructor(public n: string) { console.log('open', n); } [Symbol.dispose]() { console.log('close', this.n); } }
function f() { using a = new Res('a'); using b = new Res('b'); console.log('body'); return 1; }
console.log(f());
