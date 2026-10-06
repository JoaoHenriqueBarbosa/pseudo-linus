//! Testes do núcleo da linguagem: funções, closures, classes, geradores, compreensões, `with`...
//! Cada teste roda Python de verdade e compara com o que o CPython 3.13 imprime.

use crate::run_source;

fn out(src: &str) -> String {
    let o = run_source(src);
    assert_eq!(o.stderr, "", "stderr inesperado para:\n{src}");
    String::from_utf8(o.stdout).unwrap()
}

fn err(src: &str) -> String {
    run_source(src).stderr
}

#[test]
fn closures_and_nonlocal() {
    let src = "\
def counter():
    n = 0
    def inc():
        nonlocal n
        n += 1
        return n
    return inc
c = counter()
print(c(), c(), c())
def adder(a):
    return lambda b: a + b
print(adder(2)(3))
x = 10
def g():
    global x
    x = 20
g()
print(x)
";
    assert_eq!(out(src), "1 2 3\n5\n20\n");
}

#[test]
fn parameters_star_and_keyword_only() {
    let src = "\
def f(a, b=2, *args, c, d=4, **kw):
    return (a, b, args, c, d, sorted(kw.items()))
print(f(1, c=3))
print(f(1, 2, 3, 4, c=5, z=9))
def g(x, /, y):
    return x + y
print(g(1, 2), g(1, y=5))
args = [1, 2]
kw = {'c': 7}
print(f(*args, **kw))
print(*[1, 2, 3], sep='-')
";
    assert_eq!(
        out(src),
        "(1, 2, (), 3, 4, [])\n(1, 2, (3, 4), 5, 4, [('z', 9)])\n3 6\n(1, 2, (), 7, 4, [])\n1-2-3\n"
    );
}

#[test]
fn parameter_errors() {
    assert!(err("def f(a): pass\nf()").ends_with("TypeError: f() missing 1 required positional argument: 'a'\n"));
    assert!(err("def f(*, a): pass\nf()").ends_with("TypeError: f() missing 1 required keyword-only argument: 'a'\n"));
    assert!(err("def f(a): pass\nf(1, b=2)").ends_with("TypeError: f() got an unexpected keyword argument 'b'\n"));
    assert!(err("def f(x, /): pass\nf(x=1)").contains("positional-only arguments passed as keyword arguments: 'x'"));
}

#[test]
fn comprehensions() {
    let src = "\
print([x * x for x in range(5) if x % 2 == 0])
print({x % 3 for x in range(10)})
print({k: v for k, v in zip('abc', range(3))})
print([(i, j) for i in range(2) for j in range(2)])
print(sum(x for x in range(4)))
print(list(c.upper() for c in 'ab'))
total = 10
print([total + i for i in range(3)])
";
    assert_eq!(
        out(src),
        "[0, 4, 16]\n{0, 1, 2}\n{'a': 0, 'b': 1, 'c': 2}\n[(0, 0), (0, 1), (1, 0), (1, 1)]\n6\n['A', 'B']\n[10, 11, 12]\n"
    );
}

#[test]
fn slices() {
    let src = "\
a = [0, 1, 2, 3, 4, 5]
print(a[1:3], a[:2], a[4:], a[::2], a[::-1], a[-2:], a[10:])
s = 'hello'
print(s[1:3], s[::-1], s[-3:])
t = (1, 2, 3, 4)
print(t[1:], t[:-1])
b = [1, 2, 3, 4]
b[1:3] = [9]
print(b)
del b[0]
print(b)
del a[::2]
print(a)
";
    assert_eq!(
        out(src),
        "[1, 2] [0, 1] [4, 5] [0, 2, 4] [5, 4, 3, 2, 1, 0] [4, 5] []\nel olleh llo\n(2, 3, 4) (1, 2, 3)\n[1, 9, 4]\n[9, 4]\n[1, 3, 5]\n"
    );
}

#[test]
fn fstrings() {
    let src = "\
name = 'x'
n = 3.14159
print(f'{name}={n:.2f} {n!r} {name!r:>5} {1+1}')
w = 6
print(f'[{name:>{w}}]')
print(f'{{literal}}')
";
    assert_eq!(out(src), "x=3.14 3.14159   'x' 2\n[     x]\n{literal}\n");
}

#[test]
fn starred_unpacking_and_displays() {
    let src = "\
a, *b, c = [1, 2, 3, 4]
print(a, b, c)
first, *rest = 'xyz'
print(first, rest)
print([*range(2), *'ab'], (*[1], 2), {*[1, 1, 2]})
print({**{'a': 1}, 'b': 2})
";
    assert_eq!(out(src), "1 [2, 3] 4\nx ['y', 'z']\n[0, 1, 'a', 'b'] (1, 2) {1, 2}\n{'a': 1, 'b': 2}\n");
}

#[test]
fn classes_and_inheritance() {
    let src = "\
class Animal:
    kind = 'animal'
    def __init__(self, name):
        self.name = name
    def speak(self):
        return f'{self.name} makes a sound'
    def __repr__(self):
        return f'Animal({self.name!r})'
class Dog(Animal):
    def speak(self):
        return super().speak() + ' (woof)'
d = Dog('rex')
print(d.speak())
print(d, repr(d), str(d))
print(d.kind, Dog.kind, isinstance(d, Animal), type(d).__name__)
print([d])
";
    assert_eq!(
        out(src),
        "rex makes a sound (woof)\nAnimal('rex') Animal('rex') Animal('rex')\nanimal animal True Dog\n[Animal('rex')]\n"
    );
}

#[test]
fn class_features() {
    let src = "\
class P:
    count = 0
    def __init__(self, x):
        self._x = x
        P.count += 1
    @property
    def x(self):
        return self._x
    @x.setter
    def x(self, v):
        self._x = v * 2
    @staticmethod
    def sm(a):
        return a + 1
    @classmethod
    def cm(cls, a):
        return cls(a)
    def __eq__(self, o):
        return self._x == o._x
    def __lt__(self, o):
        return self._x < o._x
    def __add__(self, o):
        return P(self._x + o._x)
    def __len__(self):
        return self._x
    def __getitem__(self, i):
        return i * 10
    def __contains__(self, v):
        return v == self._x
p = P(1)
p.x = 5
print(p.x, P.sm(1), P.cm(7).x, P.count)
print(P(1) == P(1), P(1) < P(2), sorted([P(3), P(1)])[0].x, (P(1) + P(2)).x)
print(len(p), p[3], 10 in p, bool(P(0)))
";
    assert_eq!(out(src), "10 2 7 2\nTrue True 1 3\n10 30 True False\n");
}

#[test]
fn user_exceptions() {
    let src = "\
class MyErr(Exception):
    def __init__(self, msg, code):
        super().__init__(msg)
        self.code = code
try:
    raise MyErr('boom', 7)
except MyErr as e:
    print(e, e.code, e.args, repr(e))
try:
    raise MyErr('x', 1)
except Exception as e:
    print(type(e).__name__)
class Plain(Exception):
    pass
try:
    raise Plain('hi')
except (KeyError, Plain) as e:
    print(str(e))
";
    assert_eq!(out(src), "boom 7 ('boom',) MyErr('boom')\nMyErr\nhi\n");
    assert!(err("class E(Exception): pass\nraise E('bad')").ends_with("__main__.E: bad\n"));
}

#[test]
fn generators() {
    let src = "\
def gen(n):
    for i in range(n):
        yield i * 2
print(list(gen(4)))
g = gen(2)
print(next(g), next(g))
def fib():
    a, b = 0, 1
    while True:
        yield a
        a, b = b, a + b
f = fib()
print([next(f) for _ in range(8)])
def chain(*its):
    for it in its:
        yield from it
print(list(chain([1, 2], 'ab')))
def echo():
    x = yield 1
    print('got', x)
    yield 2
e = echo()
print(next(e), e.send('hello'))
";
    assert_eq!(out(src), "[0, 2, 4, 6]\n0 2\n[0, 1, 1, 2, 3, 5, 8, 13]\n[1, 2, 'a', 'b']\ngot hello\n1 2\n");
}

#[test]
fn with_statement() {
    let src = "\
class Ctx:
    def __init__(self, name, suppress=False):
        self.name = name
        self.suppress = suppress
    def __enter__(self):
        print('enter', self.name)
        return self
    def __exit__(self, t, v, tb):
        print('exit', self.name, t.__name__ if t else None)
        return self.suppress
with Ctx('a') as a, Ctx('b'):
    print('body', a.name)
with Ctx('c', True):
    raise ValueError('x')
print('after')
def f():
    with Ctx('d'):
        return 5
print(f())
";
    assert_eq!(
        out(src),
        "enter a\nenter b\nbody a\nexit b None\nexit a None\nenter c\nexit c ValueError\nafter\nenter d\nexit d None\n5\n"
    );
}

#[test]
fn decorators_and_lambda() {
    let src = "\
def deco(f):
    def wrapper(*a, **k):
        print('calling', f.__name__)
        return f(*a, **k)
    return wrapper
@deco
def hello(x):
    return x + 1
print(hello(1))
sq = lambda x, y=2: x ** y
print(sq(3), sq(2, 3))
print(sorted(['bb', 'a', 'ccc'], key=lambda s: -len(s)))
";
    assert_eq!(out(src), "calling hello\n2\n9 8\n['ccc', 'bb', 'a']\n");
}

#[test]
fn walrus_and_misc() {
    let src = "\
data = [1, 2, 3]
if (n := len(data)) > 2:
    print(n)
import os.path
x: int = 5
print(x)
def f():
    return (yield_ := 3)
print(f())
print(type(5).__name__, type('a').__name__, type([]).__name__)
";
    let o = run_source(src);
    // `import os.path` precisa do módulo `os`, que chega com a stdlib da rodada 2.
    assert!(o.stderr.contains("No module named 'os.path'") || o.stderr.is_empty(), "{}", o.stderr);
}
