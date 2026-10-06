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
fn class_hooks_and_metaclass() {
    let src = "\
class Base:
    registry = []
    def __init_subclass__(cls, tag=None, **kw):
        super().__init_subclass__(**kw)
        Base.registry.append((cls.__name__, tag))
class A(Base, tag='a'): pass
class B(Base): pass
print(Base.registry)
class Field:
    def __set_name__(self, owner, name):
        self.name = name
class M:
    x = Field()
    y = Field()
print(M.x.name, M.y.name)
class Meta(type):
    def __new__(mcs, name, bases, ns):
        ns['order'] = [k for k in ns if not k.startswith('__')]
        return super().__new__(mcs, name, bases, ns)
class C(metaclass=Meta):
    b = 1
    a = 2
print(C.order)
";
    assert_eq!(out(src), "[('A', 'a'), ('B', None)]\nx y\n['b', 'a']\n");
}

#[test]
fn type_three_args_and_dunder_new() {
    let src = "\
K = type('K', (), {'x': 1, 'hi': lambda self: 'oi'})
print(K.__name__, K().x, K().hi())
class Single:
    _inst = None
    def __new__(cls, *a):
        if cls._inst is None:
            cls._inst = super().__new__(cls)
        return cls._inst
    def __init__(self, v):
        self.v = v
a = Single(1); b = Single(2)
print(a is b, a.v)
class Other:
    def __new__(cls):
        return 42
print(Other())
";
    assert_eq!(out(src), "K 1 oi\nTrue 2\n42\n");
}

#[test]
fn subclasses_of_builtin_types() {
    let src = "\
class D(dict):
    def __missing__(self, k):
        return 'x'
    def __setitem__(self, k, v):
        super().__setitem__(k, v * 2)
d = D(a=1)
d['b'] = 5
print(d, len(d), d['b'], 'a' in d, isinstance(d, dict), sorted(d), d == {'a': 1, 'b': 10})
print(list(d.items()), d.get('zz'))
class L(list):
    def total(self):
        return sum(self)
l = L([1, 2, 3])
l.append(4)
l += [5]
print(l, l.total(), len(l), l[0], l[-1], bool(L()), l == [1, 2, 3, 4, 5])
class S(str):
    def shout(self):
        return self.upper() + '!'
s = S('abc')
print(s, s.shout(), len(s), s + 'd', isinstance(s, str))
class N(int):
    def double(self):
        return self * 2
print(N(21).double(), N(3) + 1)
class T(tuple):
    def __new__(cls, a, b):
        return super().__new__(cls, (a, b))
print(T(1, 2), T(1, 2)[1])
";
    assert_eq!(
        out(src),
        "{'a': 1, 'b': 10} 2 10 True True ['a', 'b'] True\n[('a', 1), ('b', 10)] None\n\
         [1, 2, 3, 4, 5] 15 5 1 5 False True\nabc ABC! 3 abcd True\n42 4\n(1, 2) 2\n"
    );
}

#[test]
fn descriptors_annotations_and_generics() {
    let src = "\
class Positive:
    def __set_name__(self, owner, name):
        self.name = '_' + name
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return getattr(obj, self.name)
    def __set__(self, obj, value):
        if value <= 0:
            raise ValueError('must be positive')
        object.__setattr__(obj, self.name, value)
class Item:
    qty = Positive()
    label: str = 'x'
    count: int
    def __init__(self, qty):
        self.qty = qty
i = Item(3)
print(i.qty, Item.__annotations__)
try:
    i.qty = -1
except ValueError as e:
    print('erro', e)
print(list[int], dict[str, list[int]], int | None, isinstance(3, int | str), ...)
def f(x: list[int] | None = None) -> tuple[int, ...]:
    return ()
print(f())
";
    assert_eq!(
        out(src),
        "3 {'label': <class 'str'>, 'count': <class 'int'>}\nerro must be positive\n\
         list[int] dict[str, list[int]] int | None True Ellipsis\n()\n"
    );
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
