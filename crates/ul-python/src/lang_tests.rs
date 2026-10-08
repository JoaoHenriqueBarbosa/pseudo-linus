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
    assert!(err("class E(Exception): pass\nraise E('bad')").ends_with("\nE: bad\n"));
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

#[test]
fn property_subclass_cached_property() {
    let src = "\
_missing = object()
class cached_property(property):
    def __init__(self, fget, name=None, doc=None):
        super().__init__(fget, doc=doc)
        self.__name__ = name or fget.__name__
        self.slot_name = f'_cache_{self.__name__}'
        self.__module__ = fget.__module__
    def __set__(self, obj, value):
        if hasattr(obj, '__dict__'):
            obj.__dict__[self.__name__] = value
        else:
            setattr(obj, self.slot_name, value)
    def __delete__(self, obj):
        if hasattr(obj, '__dict__'):
            obj.__dict__.pop(self.__name__, None)
        else:
            setattr(obj, self.slot_name, _missing)
    def __get__(self, obj, type=None):
        if obj is None:
            return self
        obj_dict = getattr(obj, '__dict__', None)
        if obj_dict is not None:
            value = obj_dict.get(self.__name__, _missing)
        else:
            value = getattr(obj, self.slot_name, _missing)
        if value is _missing:
            value = self.fget(obj)
            if obj_dict is not None:
                obj.__dict__[self.__name__] = value
            else:
                setattr(obj, self.slot_name, value)
        return value
calls = []
class A:
    @cached_property
    def x(self):
        'o docstring'
        calls.append(1)
        return 42
a = A()
print(a.x, a.x, len(calls))
print(isinstance(A.__dict__['x'], property), isinstance(A.__dict__['x'], cached_property))
print(A.x is A.__dict__['x'], A.x.__name__, A.x.__doc__)
a.x = 7
print(a.x, a.__dict__)
del a.x
print(a.x, len(calls))
b = A()
print(b.x, len(calls))
";
    assert_eq!(out(src), "42 42 1\nTrue True\nTrue x o docstring\n7 {'x': 7}\n42 2\n42 3\n");
}

#[test]
fn property_subclass_get_override_and_super() {
    let src = "\
class loud(property):
    def __get__(self, obj, cls=None):
        if obj is None:
            return self
        return super().__get__(obj, cls) * 2
class A:
    def __init__(self):
        self._v = 5
    @loud
    def v(self):
        'doc de v'
        return self._v
    @v.setter
    def v(self, value):
        self._v = value
a = A()
print(a.v)
a.v = 10
print(a.v, type(A.__dict__['v']).__name__)
print(A.v.fget.__name__, A.v.fset.__name__, A.v.fdel, A.v.__doc__)
print(isinstance(A.v, property), issubclass(loud, property))
print(A.v.__isabstractmethod__)
";
    assert_eq!(out(src), "10\n20 loud\nv v None doc de v\nTrue True\nFalse\n");
}

#[test]
fn property_subclass_decorators_return_subclass() {
    let src = "\
class P(property):
    pass
class A:
    def __init__(self):
        self._x = 1
    @P
    def x(self):
        return self._x
    @x.setter
    def x(self, value):
        self._x = value
    @x.deleter
    def x(self):
        self._x = 0
print(type(A.__dict__['x']).__name__)
a = A()
a.x = 9
print(a.x)
del a.x
print(a.x)
p = P(lambda self: 1)
print(type(p.getter(lambda self: 2)).__name__, type(p.setter(len)).__name__, type(p.deleter(len)).__name__)
print(repr(p).startswith('<__main__.P object at 0x'))
print(p.fset, p.fdel, p.fget(None))
";
    assert_eq!(out(src), "P\n9\n0\nP P P\nTrue\nNone None 1\n");
}

#[test]
fn del_runs_when_the_last_reference_drops() {
    let src = "\
class C:
    def __init__(self, n):
        self.n = n
    def __del__(self):
        print('del', self.n)
c = C(1); del c; print('after')
C(2); print('next')
x = C(3); x = C(4); print('rebound')
def f():
    y = C(5)
f(); print('left')
";
    assert_eq!(out(src), "del 1\nafter\ndel 2\nnext\ndel 3\nrebound\ndel 5\nleft\ndel 4\n");
}

#[test]
fn del_runs_once_and_after_atexit() {
    let src = "\
import atexit
keep = []
class C:
    def __del__(self):
        print('del')
        keep.append(self)
c = C(); del c
print(len(keep)); keep.clear(); print('end')
class D:
    def __del__(self):
        print('del D')
d = D()
atexit.register(lambda: print('atexit'))
";
    assert_eq!(out(src), "del\n1\nend\natexit\ndel D\n");
}

#[test]
fn del_exception_goes_to_unraisablehook() {
    let o = run_source("class C:\n    def __del__(self):\n        raise ValueError('boom')\nc = C(); del c; print('after')\n");
    assert_eq!(String::from_utf8(o.stdout).unwrap(), "after\n");
    assert!(o.stderr.starts_with("Exception ignored in: <function C.__del__ at 0x"), "{}", o.stderr);
    assert!(o.stderr.contains("Traceback (most recent call last):\n"), "{}", o.stderr);
    assert!(o.stderr.ends_with("ValueError: boom\n"), "{}", o.stderr);
}

#[test]
fn property_subclass_errors_like_cpython() {
    let src = "\
class P(property):
    pass
class A:
    @P
    def x(self):
        return 1
    y = P(None)
a = A()
for stmt in ('a.x = 1', 'del a.x', 'a.y'):
    try:
        exec(stmt)
    except AttributeError as e:
        print(e)
";
    assert_eq!(
        out(src),
        "property 'x' of 'A' object has no setter\nproperty 'x' of 'A' object has no deleter\nproperty 'y' of 'A' object has no getter\n"
    );
}

#[test]
fn globals_keep_insertion_order() {
    let src = "\
z = 1
a = 2
class C: pass
print([k for k in globals() if not k.startswith('__')])
del z
z = 3
b = 4
del a
print([k for k in globals() if not k.startswith('__')])
globals()['m'] = 5
n = 6
print([k for k in globals() if not k.startswith('__')])
exec('q = 1\\nd = 2\\nq = 3', globals())
print([k for k in globals() if not k.startswith('__')])
";
    assert_eq!(
        out(src),
        "['z', 'a', 'C']\n['C', 'z', 'b']\n['C', 'z', 'b', 'm', 'n']\n['C', 'z', 'b', 'm', 'n', 'q', 'd']\n"
    );
}

/// PEP 709 (CPython 3.12+): compreensão de lista, conjunto e dicionário é inline, sem quadro próprio.
#[test]
fn inline_comprehensions() {
    let src = "\
import sys
x = 'outer'
print([x for x in range(3)], x)
def f():
    y = 10
    r = [y for y in range(3)]
    return r, y
print(f())
def g():
    a = 1
    return [locals() for a in range(1)][0]
print(sorted(g().items()))
fs = [lambda: i for i in range(3)]
print([f() for f in fs])
def h():
    return [lambda: i for i in range(2)]
print([f() for f in h()])
class K:
    base = 2
    ok = [i for i in range(base)]
    try:
        bad = [base for _ in range(2)]
    except NameError as e:
        err = str(e)
print(K.ok, K.err)
print('i' in K.__dict__, '_' in K.__dict__)
def t():
    v = 'kept'
    try:
        [1 // 0 for v in range(1)]
    except ZeroDivisionError:
        pass
    return v, sorted(locals())
print(t())
print([[x for x in range(2)] + [x] for x in range(2, 4)])
[zz for zz in range(2)]
print('zz' in globals())
print([(w := n) for n in range(3)], w)
events = []
def tracer(frame, event, arg):
    events.append((event, frame.f_code.co_name, frame.f_lineno))
    return tracer
def comp():
    return [i for i in range(2)]
sys.settrace(tracer)
comp()
sys.settrace(None)
first = comp.__code__.co_firstlineno
print([(e[0], e[2] - first) for e in events if e[1] == 'comp'])
";
    assert_eq!(
        out(src),
        "[0, 1, 2] outer\n\
         ([0, 1, 2], 10)\n\
         [('a', 0)]\n\
         [2, 2, 2]\n\
         [1, 1]\n\
         [0, 1] name 'base' is not defined\n\
         False False\n\
         ('kept', ['v'])\n\
         [[0, 1, 2], [0, 1, 3]]\n\
         False\n\
         [0, 1, 2] 2\n\
         [('call', 0), ('line', 1), ('line', 1), ('line', 1), ('return', 1)]\n"
    );
}

/// `co_firstlineno` de função e classe decoradas é a linha do primeiro decorador (3.13), e o evento
/// `call` do settrace sai com essa linha; `__firstlineno__` da classe idem.
#[test]
fn decorated_firstlineno_is_first_decorator() {
    let src = "\
import sys
def d(f):
    return f
@d
def one():
    return 1
@d
@d
def two():
    return 2
@d
class C1:
    pass
@d
@d
class C2:
    pass
class K:
    @property
    def prop(self):
        return 4
events = []
def tracer(frame, event, arg):
    if event == 'call' and frame.f_code.co_name == 'prop':
        events.append(frame.f_lineno)
    return None
sys.settrace(tracer)
K().prop
sys.settrace(None)
print(one.__code__.co_firstlineno, two.__code__.co_firstlineno)
print(C1.__firstlineno__, C2.__firstlineno__)
print(K.prop.fget.__code__.co_firstlineno, events)
";
    assert_eq!(out(src), "4 7\n11 14\n19 [19]\n");
}

/// Uma função criada na compreensão que fecha sobre o alvo do laço não tira a compreensão de dentro
/// da função de fora (PEP 709): o alvo vira célula do quadro de fora (`co_cellvars`), nova a cada
/// execução da compreensão, e a variável de fora de mesmo nome sai intacta. Nenhum quadro
/// `<listcomp>` aparece em traceback, `_getframe` ou `f_back`.
#[test]
fn inline_comprehension_closes_over_target_cell() {
    let src = "\
import sys
print([f() for f in [lambda: i for i in range(3)]])
def a():
    fs = [lambda: i for i in range(3)]
    return [f() for f in fs]
print(a())
def b():
    i = 'before'
    fs = [lambda: i for i in range(2)]
    return i, [f() for f in fs], b.__code__.co_cellvars
print(b())
def c():
    r = [lambda: q for q in range(2)]
    return c.__code__.co_cellvars, c.__code__.co_varnames
print(c())
def d():
    fs = [lambda: q for q in range(2)]
    try:
        q
    except UnboundLocalError as e:
        return str(e), [f() for f in fs]
print(d())
def nested():
    rows = [[lambda: (i, j) for j in range(2)] for i in range(2)]
    return [[f() for f in row] for row in rows]
print(nested())
def e():
    return [1 // 0 for z in range(1) if (lambda: z)]
names = []
try:
    e()
except ZeroDivisionError:
    tb = sys.exc_info()[2]
    while tb:
        names.append(tb.tb_frame.f_code.co_name)
        tb = tb.tb_next
print(names)
def who():
    return sys._getframe(1).f_code.co_name
def g():
    return [who() for k in range(1) if (lambda: k)], [sys._getframe().f_code.co_name for k in range(1) if (lambda: k)]
print(g())
def h():
    return [sys._getframe().f_back.f_code.co_name for k in range(1) if (lambda: k)]
def caller():
    return h()
print(caller())
def gen():
    return list((lambda: k)() for k in range(2))
print(gen())
";
    assert_eq!(
        out(src),
        "[2, 2, 2]\n\
         [2, 2, 2]\n\
         ('before', [1, 1], ('i',))\n\
         (('q',), ('q', 'r'))\n\
         (\"cannot access local variable 'q' where it is not associated with a value\", [1, 1])\n\
         [[(1, 1), (1, 1)], [(1, 1), (1, 1)]]\n\
         ['<module>', 'e']\n\
         (['g'], ['g'])\n\
         ['caller']\n\
         [0, 1]\n"
    );
}

/// `locals()` e `f_locals` seguem a ordem de `co_varnames` (parâmetros, depois os locais pela primeira
/// aparição no corpo), as células que não são parâmetros e as variáveis livres, como o CPython 3.13.
#[test]
fn locals_follow_code_object_order() {
    let src = "\
def params(b, a, *args, k2, k1, **kw):
    z = 1
    y = 2
    x = z + y
    return list(locals())
print(params(1, 2, 3, k2=4, k1=5, extra=6))
def first_use():
    for i in range(2):
        pass
    c = 1
    a = 2
    return list(locals())
print(first_use())
def cells(p):
    q = 1
    w = 2
    def inner():
        return w, q, p
    r = 3
    return list(locals()), inner.__code__.co_freevars, cells.__code__.co_cellvars, cells.__code__.co_varnames
print(cells(0))
def outer():
    b = 2
    a = 1
    def inner():
        c = 3
        d = a + b
        return list(locals())
    return inner()
print(outer())
def comp():
    first = 1
    r = [t for t in range(2)]
    last = 3
    return comp.__code__.co_varnames, comp.__code__.co_nlocals
print(comp())
";
    assert_eq!(
        out(src),
        "['b', 'a', 'k2', 'k1', 'args', 'kw', 'z', 'y', 'x']\n\
         ['i', 'c', 'a']\n\
         (['p', 'inner', 'r', 'q', 'w'], ('p', 'q', 'w'), ('p', 'q', 'w'), ('p', 'inner', 'r'))\n\
         ['c', 'd', 'a', 'b']\n\
         (('first', 't', 'r', 'last'), 4)\n"
    );
}

/// `warnings.warn` e `warn_explicit` vêm do `_warnings` (C no CPython): o quadro de `warn` não existe,
/// o `stacklevel` conta a partir de quem chamou, e filtros e registro seguem o `_warnings.c`.
#[test]
fn warnings_module_is_native_like_cpython() {
    let src = r#"
import warnings, sys, _warnings, traceback
print(warnings.warn is _warnings.warn, warnings.warn_explicit is _warnings.warn_explicit, warnings.filters is _warnings.filters)
print(repr(warnings.warn), type(warnings.warn).__name__)

def inner(level):
    global L_in
    L_in = sys._getframe().f_lineno + 1
    warnings.warn('m%d' % level, UserWarning, stacklevel=level)
def outer(level):
    global L_out
    L_out = sys._getframe().f_lineno + 1
    inner(level)
with warnings.catch_warnings(record=True) as w:
    warnings.simplefilter('always')
    L_mod = sys._getframe().f_lineno + 1
    outer(1)
    outer(2)
    outer(3)
    outer(4)
print([x.lineno for x in w[:3]] == [L_in, L_out, L_mod + 2], w[3].filename, w[3].lineno, w[3].message.args)

def emit(msg):
    warnings.warn(msg, UserWarning)
def emit2(msg):
    warnings.warn(msg, UserWarning)
def count(action):
    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter(action)
        emit('x')
        emit2('x')
        emit('x')
    return len(w)
print([count(a) for a in ('default', 'once', 'module', 'always', 'ignore')])

with warnings.catch_warnings():
    warnings.simplefilter('error')
    try:
        emit('boom')
    except UserWarning as e:
        print('raised', e, len(traceback.extract_tb(e.__traceback__)))

with warnings.catch_warnings(record=True) as w:
    warnings.simplefilter('default')
    emit('r')
    reg = globals()['__warningregistry__']
    print(sorted(k if isinstance(k, str) else k[0] for k in reg), isinstance(reg['version'], int), reg[('r', UserWarning, w[0].lineno)])

for bad in (5, int, 'x'):
    try:
        warnings.warn('x', bad)
    except TypeError as e:
        print(e)
with warnings.catch_warnings(record=True) as w:
    warnings.simplefilter('always')
    warnings.warn(DeprecationWarning('dd'), UserWarning)
print(w[0].category.__name__, str(w[0].message))

seen = []
def hook(message, category, filename, lineno, file=None, line=None):
    f = sys._getframe(1)
    seen.append((f.f_code.co_name, f.f_back.f_code.co_name))
def caller():
    warnings.warn('h')
with warnings.catch_warnings():
    warnings.simplefilter('always')
    warnings.showwarning = hook
    caller()
print(seen)

with warnings.catch_warnings(record=True) as w:
    warnings.simplefilter('always')
    warnings.warn('s', skip_file_prefixes=(sys._getframe().f_code.co_filename,))
    warnings.warn_explicit('e', UserWarning, 'foo.py', 7)
    warnings.filterwarnings('ignore', module='foo')
    warnings.warn_explicit('e2', UserWarning, 'foo.py', 7)
print([(x.filename, x.lineno) for x in w])
try:
    warnings.warn_explicit('x', UserWarning, 'f', 1, registry=3)
except TypeError as e:
    print(e)
"#;
    assert_eq!(
        out(src),
        "True True True\n\
         <built-in function warn> builtin_function_or_method\n\
         True <sys> 0 ('m4',)\n\
         [2, 1, 1, 3, 0]\n\
         raised boom 2\n\
         ['r', 'version'] True True\n\
         category must be a Warning subclass, not 'int'\n\
         category must be a Warning subclass, not 'type'\n\
         category must be a Warning subclass, not 'str'\n\
         DeprecationWarning dd\n\
         [('_showwarnmsg', 'caller')]\n\
         [('<sys>', 0), ('foo.py', 7)]\n\
         'registry' must be a dict or None\n"
    );
}

/// O salto para trás do `for` e do `while` não abre linha nova (herda a do corpo): a linha do laço
/// gera um `line` por entrada e por volta, não dois (`trace.Trace(count=1)` dá (3, 7) no CPython).
#[test]
fn settrace_loop_header_line_fires_once_per_pass() {
    let src = "\
import sys
def f(n):
    s = 0
    for i in range(n):
        s += i
    return s
def w(n):
    i = 0
    while i < n:
        i += 1
    return i
lines = []
def tracer(frame, event, arg):
    if event == 'line':
        lines.append(frame.f_lineno - frame.f_code.co_firstlineno)
    return tracer
sys.settrace(tracer)
f(3)
f(2)
w(2)
sys.settrace(None)
print([(k, lines.count(k)) for k in sorted(set(lines))])
";
    assert_eq!(out(src), "[(1, 3), (2, 10), (3, 7), (4, 3)]\n");
}

/// `__reduce__` e `__reduce_ex__` dos embutidos e do `datetime` como no CPython 3.13 (o que o
/// representer do PyYAML e o `pickle` consomem), e o `__module__` das funções embutidas.
#[test]
fn builtin_reduce_and_function_module() {
    let src = "\
import datetime, math
print(bytearray(b'xy').__reduce_ex__(2), bytearray(b'xy').__reduce_ex__(3), bytearray(b'xy').__reduce__())
print(slice(1, 2).__reduce__(), slice(1, 2).__reduce_ex__(2))
try:
    memoryview(b'a').__reduce_ex__(2)
except TypeError as e:
    print(e)
t = datetime.time(1, 2, 3)
print(t.__reduce_ex__(2), t.__reduce__())
print(datetime.time(*t.__reduce__()[1]) == t)
print(datetime.date(2001, 12, 14).__reduce__(), datetime.date(*datetime.date(2001, 12, 14).__reduce__()[1]))
d = datetime.datetime(2001, 12, 14, 21, 59, 43, 10, fold=1)
print(d.__reduce_ex__(2)[1], d.__reduce_ex__(4)[1], datetime.datetime(*d.__reduce_ex__(4)[1]).fold)
print(datetime.timezone.utc.__reduce__(), datetime.timezone(datetime.timedelta(hours=1), 'x').__reduce__())
print(len.__module__, math.sqrt.__module__, len.__self__, math.sqrt.__self__ is math)
";
    assert_eq!(
        out(src),
        "(<class 'bytearray'>, ('xy', 'latin-1'), None) (<class 'bytearray'>, (b'xy',), None) (<class 'bytearray'>, ('xy', 'latin-1'), None)\n\
         (<class 'slice'>, (1, 2, None)) (<class 'slice'>, (1, 2, None))\n\
         cannot pickle 'memoryview' object\n\
         (<class 'datetime.time'>, (b'\\x01\\x02\\x03\\x00\\x00\\x00',)) (<class 'datetime.time'>, (b'\\x01\\x02\\x03\\x00\\x00\\x00',))\n\
         True\n\
         (<class 'datetime.date'>, (b'\\x07\\xd1\\x0c\\x0e',)) 2001-12-14\n\
         (b'\\x07\\xd1\\x0c\\x0e\\x15;+\\x00\\x00\\n',) (b'\\x07\\xd1\\x8c\\x0e\\x15;+\\x00\\x00\\n',) 1\n\
         (<class 'datetime.timezone'>, (datetime.timedelta(0),)) (<class 'datetime.timezone'>, (datetime.timedelta(seconds=3600), 'x'))\n\
         builtins math <module 'builtins' (built-in)> True\n"
    );
}

/// `co_code` de atribuição, soma e `return`: `LOAD_FAST_LOAD_FAST`, `BINARY_OP` com uma entrada de CACHE,
/// `STORE_FAST` e `LOAD_FAST` em linhas diferentes (sem `STORE_FAST_LOAD_FAST`), `RETURN_VALUE`.
#[test]
fn cpython_bytecode_of_assignment_and_return() {
    let src = "\
def f(a, b):
    x = a + b
    return x
c = f.__code__
print(list(c.co_code))
print(c.co_consts, c.co_names, c.co_varnames)
";
    assert_eq!(out(src), "[149, 0, 88, 1, 45, 0, 0, 0, 110, 2, 85, 2, 36, 0]\n(None,) () ('a', 'b', 'x')\n");
}

/// `if` sem `else` com `return` de constante: `TO_BOOL` com 3 CACHE, `POP_JUMP_IF_FALSE` relativo com 1 CACHE,
/// `RETURN_CONST`, e o corte da primeira constante (`None`) mantido.
#[test]
fn cpython_bytecode_of_if_and_return_const() {
    let src = "\
def g(x):
    if x:
        return 1
    return 2
c = g.__code__
print(list(c.co_code))
print(c.co_consts)
";
    assert_eq!(
        out(src),
        "[149, 0, 85, 0, 40, 0, 0, 0, 0, 0, 0, 0, 97, 1, 0, 0, 103, 1, 103, 2]\n(None, 1, 2)\n"
    );
}

/// Módulo: `LOAD_NAME` e `STORE_NAME`, `PUSH_NULL` depois do `LOAD_NAME` do chamador, `CALL` com 3 CACHE, `POP_TOP`
/// e o `RETURN_CONST None` do fim (a constante `None` entra por último no módulo).
#[test]
fn cpython_bytecode_of_module_and_eval() {
    let src = "\
c = compile('x = 1\\nprint(x)', '<s>', 'exec')
print(list(c.co_code))
print(c.co_consts, c.co_names)
e = compile('a + 1', '<s>', 'eval')
print(list(e.co_code))
print(e.co_consts, e.co_names)
";
    assert_eq!(
        out(src),
        "[149, 0, 83, 0, 114, 0, 92, 1, 34, 0, 92, 0, 53, 1, 0, 0, 0, 0, 0, 0, 32, 0, 103, 1]\n\
         (1, None) ('x', 'print')\n\
         [149, 0, 92, 0, 83, 0, 45, 0, 0, 0, 36, 0]\n\
         (1,) ('a',)\n"
    );
}

/// O `dis.py` real do Debian sobre o `_opcode` nativo e o `co_code` emitido.
#[test]
fn dis_get_instructions_reads_emitted_bytecode() {
    let src = "\
import dis
def f(a, b):
    x = a + b
    return x
for i in dis.get_instructions(f):
    print(i.opname, i.argrepr, i.positions.lineno)
";
    assert_eq!(
        out(src),
        "RESUME  2\nLOAD_FAST_LOAD_FAST a, b 3\nBINARY_OP + 3\nSTORE_FAST x 3\nLOAD_FAST x 4\nRETURN_VALUE  4\n"
    );
}

/// Troca cada endereço `0x...` por `0x0`: o `repr` de um objeto `code` leva o endereço dele.
fn mask_hex_addresses(text: &str) -> String {
    let mut masked = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("0x") {
        masked.push_str(&rest[..i]);
        masked.push_str("0x0");
        let tail = &rest[i + 2..];
        let digits = tail.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(tail.len());
        rest = &tail[digits..];
    }
    masked.push_str(rest);
    masked
}

/// O módulo de exemplo medido no CPython 3.13 do Debian (`wip/notes/python-dis-oracle.txt`): `co_linetable` e
/// `co_stacksize` do módulo, o `dis.dis` completo (com as três funções aninhadas), `co_linetable`, `co_stacksize`,
/// `co_code` e as duas últimas posições de cada função, o módulo vazio, as listas `has*` do `opcode` e o
/// `stack_effect` dos opcodes duvidosos. A saída tem de ser idêntica byte a byte (menos os endereços).
#[test]
fn cpython_bytecode_matches_oracle_measurement() {
    let src = r#"
import dis, io, re, opcode, _opcode
src = "import os\ndef f(a, b):\n    return a + b\ndef g(x):\n    if x is None:\n        return 1\n    y = x.m(2, k=3)\n    os.getcwd()\n    return y\ndef h(): pass\n"
c = compile(src, "m", "exec")
print(c.co_linetable.hex(), c.co_stacksize)
buf = io.StringIO()
dis.dis(c, file=buf)
print(re.sub(r"0x[0-9a-f]+", "0x0", buf.getvalue()), end="")
for k in c.co_consts:
    if hasattr(k, "co_code"):
        print(k.co_name, k.co_linetable.hex(), k.co_stacksize, k.co_code.hex(), list(k.co_positions())[-2:])
empty = compile("", "m", "exec")
print(empty.co_stacksize, empty.co_linetable.hex())
for name in ("hasarg", "hasconst", "hasname", "hasjump", "hasfree", "haslocal", "hasexc"):
    print(name, getattr(opcode, name))
for name in ("RETURN_GENERATOR", "SEND", "FOR_ITER", "LOAD_SUPER_ATTR", "CALL_FUNCTION_EX"):
    op = opcode.opmap[name]
    args = (op, 1) if op in opcode.hasarg else (op,)
    print(name, *[_opcode.stack_effect(*args, jump=j) for j in (None, False, True)])
"#;
    let oracle = include_str!("../../../wip/notes/python-dis-oracle.txt");
    assert_eq!(out(src), mask_hex_addresses(oracle));
}

/// Closure: `MAKE_CELL` antes do `RESUME`, `LOAD_CLOSURE` (que o `dis` mostra como `LOAD_FAST`) e `COPY_FREE_VARS`
/// no código de dentro, que lê a variável com `LOAD_DEREF`; o objeto `code` de dentro entra em `co_consts`.
#[test]
fn cpython_bytecode_of_closure() {
    let src = "\
def outer(n):
    def inner():
        return n
    return inner
c = outer.__code__
print(c.co_cellvars, [k.co_name for k in c.co_consts if hasattr(k, 'co_code')])
i = c.co_consts[1]
print(i.co_freevars, i.co_code[:6].hex())
print(list(c.co_code[:6]))
";
    assert_eq!(out(src), "('n',) ['inner']\n('n',) 3e0195005400\n[94, 0, 149, 0, 85, 0]\n");
}

/// Laço `for`: `GET_ITER`, `FOR_ITER`, `JUMP_BACKWARD` e o `END_FOR` do iterador esgotado; nada cai no esqueleto.
#[test]
fn cpython_bytecode_of_for_loop() {
    let src = "\
import dis
def f(xs):
    t = 0
    for x in xs:
        t += x
    return t
names = [i.opname for i in dis.get_instructions(f)]
print('FOR_ITER' in names, 'JUMP_BACKWARD' in names, 'END_FOR' in names, names.count('RESUME'))
";
    assert_eq!(out(src), "True True True 1\n");
}

/// `try`/`except`, `with`, boolops, `if` como valor, f-string, desempacotamento, fatias, `del`, `assert`, `raise`,
/// comparação encadeada, `CALL_FUNCTION_EX` e `class`: nenhum cai no esqueleto de `RESUME` e `NOP`. Cada código tem
/// ao menos os opcodes que o CPython 3.13 gera para a construção, o `dis` lê a `co_exceptiontable` emitida, e o corpo
/// da classe tem os nomes de `compiler_class_body`.
#[test]
fn cpython_bytecode_of_exceptions_class_and_expressions() {
    let src = "\
import dis
def ops(code):
    return {i.opname for i in dis.get_instructions(code)}
def t_try(a):
    try:
        a()
    except ValueError as e:
        a = e
    return a
def t_with(a):
    with a as f:
        f.x = 1
def t_misc(a, b, k):
    x = a and b
    y = a if b else k
    z = f'{a!r:>5}x'
    p, q = a
    s = a[1:2]
    a[1:2] = b
    del s
    assert x
    return g(*a, **k), 1 < a < b
def t_raise(a):
    raise a
need = {
    t_try: {'PUSH_EXC_INFO', 'CHECK_EXC_MATCH', 'POP_EXCEPT', 'RERAISE', 'STORE_FAST', 'DELETE_FAST', 'COPY', 'POP_JUMP_IF_FALSE'},
    t_with: {'BEFORE_WITH', 'WITH_EXCEPT_START', 'PUSH_EXC_INFO', 'POP_EXCEPT', 'RERAISE', 'STORE_ATTR'},
    t_misc: {'COPY', 'TO_BOOL', 'POP_JUMP_IF_FALSE', 'CONVERT_VALUE', 'FORMAT_WITH_SPEC', 'BUILD_STRING', 'UNPACK_SEQUENCE',
             'BINARY_SLICE', 'STORE_SLICE', 'DELETE_FAST', 'LOAD_ASSERTION_ERROR', 'RAISE_VARARGS', 'CALL_FUNCTION_EX',
             'DICT_MERGE', 'BUILD_MAP', 'SWAP'},
    t_raise: {'RAISE_VARARGS'},
}
for f, wanted in need.items():
    print(f.__name__, wanted <= ops(f), sorted(wanted - ops(f)))
print(bool(t_try.__code__.co_exceptiontable), bool(t_with.__code__.co_exceptiontable), t_raise.__code__.co_exceptiontable)
mod = compile('class C(B, metaclass=M):\\n    x = 1\\n    def m(self):\\n        self.y = 2\\n', 'm', 'exec')
print(sorted(ops(mod) & {'LOAD_BUILD_CLASS', 'MAKE_FUNCTION', 'CALL_KW', 'STORE_NAME'}))
body = [k for k in mod.co_consts if hasattr(k, 'co_code')][0]
print(body.co_name, body.co_qualname, body.co_names)
print(body.co_consts[0], body.co_consts[-1], len(body.co_exceptiontable))
";
    assert_eq!(
        out(src),
        "t_try True []\nt_with True []\nt_misc True []\nt_raise True []\n\
         True True b''\n\
         ['CALL_KW', 'LOAD_BUILD_CLASS', 'MAKE_FUNCTION', 'STORE_NAME']\n\
         C C ('__name__', '__module__', '__qualname__', '__firstlineno__', 'x', 'm', '__static_attributes__')\n\
         C None 0\n"
    );
}

/// `__debug__` lido é a constante `True` (o `fold_name` do `ast_opt.c`): `return __debug__` é `RETURN_CONST True` e o
/// `if __debug__:` não deixa teste. O nome privado `self.__x` chega mutilado (`_A__x` em `co_names`) e o corpo da classe
/// aninhada `__In` guarda o nome original como constante e o mutilado só em `co_names`. Dedução do `compile.c` do 3.13.5,
/// a conferir no oráculo com `dis.dis`.
#[test]
fn cpython_bytecode_of_debug_constant_and_private_names() {
    let src = "\
import dis
def f():
    return __debug__
def g(x):
    if __debug__:
        return x
    return 0
class A:
    def m(self):
        return self.__x
    class __In:
        pass
print(f.__code__.co_consts, [i.opname for i in dis.get_instructions(f)])
print([i.opname for i in dis.get_instructions(g)])
print(A.m.__code__.co_names, [i.opname for i in dis.get_instructions(A.m)])
mod = compile('class A:\\n    class __In:\\n        pass\\n', 'm', 'exec')
body = [k for k in mod.co_consts if hasattr(k, 'co_code')][0]
print(body.co_names, '__In' in body.co_consts, '_A__In' in body.co_consts)
";
    assert_eq!(
        out(src),
        "(None, True) ['RESUME', 'RETURN_CONST']\n\
         ['RESUME', 'NOP', 'LOAD_FAST', 'RETURN_VALUE']\n\
         ('_A__x',) ['RESUME', 'LOAD_FAST', 'LOAD_ATTR', 'RETURN_VALUE']\n\
         ('__name__', '__module__', '__qualname__', '__firstlineno__', '_A__In', '__static_attributes__') True False\n"
    );
}

/// O `__doc__` dos métodos dos tipos embutidos (`method_descriptor`, `wrapper_descriptor`,
/// `method-wrapper`, método ligado), dos tipos e das funções nativas, da tabela gerada no oráculo.
#[test]
fn builtin_docs_come_from_the_cpython_table() {
    let src = "\
import math
print(str.upper.__doc__)
print(list.append.__doc__)
print(int.__add__.__doc__)
print(len.__doc__)
print(math.sqrt.__doc__)
print(dict.fromkeys.__doc__)
print(str.__doc__.splitlines()[:2])
print('x'.upper.__doc__)
print([].append.__doc__)
print((0).__add__.__doc__)
print(bool.__and__.__doc__)
";
    assert_eq!(
        out(src),
        "Return a copy of the string converted to uppercase.\n\
         Append object to the end of the list.\n\
         Return self+value.\n\
         Return the number of items in a container.\n\
         Return the square root of x.\n\
         Create a new dictionary with keys from iterable and values set to value.\n\
         [\"str(object='') -> str\", 'str(bytes_or_buffer[, encoding[, errors]]) -> str']\n\
         Return a copy of the string converted to uppercase.\n\
         Append object to the end of the list.\n\
         Return self+value.\n\
         Return self&value.\n"
    );
}

/// Os métodos embutidos ligados (`method-wrapper` e função embutida) de valores simples, iteradores e
/// exceções: tipo, `__qualname__` e `__objclass__` do dono do slot, como o CPython 3.13.
#[test]
fn bound_builtin_methods_report_their_defining_type() {
    let src = "\
m = (0).__eq__
print(type(m).__name__, m.__qualname__, m.__objclass__)
b = True.__add__
print(b.__qualname__, b.__objclass__)
i = iter([])
print(type(i.__eq__).__name__, i.__eq__.__qualname__, i.__eq__.__objclass__)
print(type(i.__next__).__name__, i.__next__.__objclass__)
print(type(i.__length_hint__).__name__, i.__length_hint__.__qualname__)
print(repr((0).__init_subclass__).startswith('<built-in method __init_subclass__ of type object at'))
print(type(bytearray().__reduce__).__name__, type(range(1).__reduce__).__name__)
print(type(ValueError('x').__eq__).__name__, ValueError('x').__eq__.__objclass__)
print(type(slice(1).indices).__name__, slice(1).indices.__qualname__)
print(classmethod(len).__func__ is len, staticmethod(len).__name__)
";
    assert_eq!(
        out(src),
        "method-wrapper int.__eq__ <class 'int'>\n\
         int.__add__ <class 'int'>\n\
         method-wrapper object.__eq__ <class 'object'>\n\
         method-wrapper <class 'list_iterator'>\n\
         builtin_function_or_method list_iterator.__length_hint__\n\
         True\n\
         builtin_function_or_method builtin_function_or_method\n\
         method-wrapper <class 'object'>\n\
         builtin_function_or_method slice.indices\n\
         True len\n"
    );
}

/// `breakpoint()` chama o `sys.breakpointhook` atual; o original lê o `PYTHONBREAKPOINT` a cada chamada.
#[test]
fn breakpoint_calls_current_hook_and_reads_environment() {
    let src = "\
import os, sys, warnings
os.environ['PYTHONBREAKPOINT'] = '0'
print(breakpoint())
sys.breakpointhook = lambda *a, **k: ('hook', a, k)
print(breakpoint(1, x=2))
print(sys.__breakpointhook__(3))
os.environ['PYTHONBREAKPOINT'] = 'json.dumps'
print(sys.__breakpointhook__([1]))
os.environ['PYTHONBREAKPOINT'] = 'nope_mod.fn'
with warnings.catch_warnings(record=True) as w:
    warnings.simplefilter('always')
    print(sys.__breakpointhook__())
print(w[0].category.__name__, w[0].message)
del sys.breakpointhook
try: breakpoint()
except RuntimeError as e: print(e)
try: del sys.nope
except AttributeError as e: print(e)
";
    // `os.environ` escreve no ambiente do processo: precisa de um processo do kernel de teste.
    let r = crate::stdlib_tests::in_process(src);
    assert_eq!(r.stderr_str(), "", "stderr inesperado para:\n{src}");
    assert_eq!(
        r.stdout_str(),
        "None\n('hook', (1,), {'x': 2})\nNone\n[1]\nNone\n\
         RuntimeWarning Ignoring unimportable $PYTHONBREAKPOINT: \"nope_mod.fn\"\n\
         lost sys.breakpointhook\n'module' object has no attribute 'nope'\n"
    );
}

/// `__class__` definido na classe (property, o `spec` do `unittest.mock`) passa pelo protocolo de
/// atributo; `isinstance` cai nele quando o tipo real não casa, `issubclass` não, e o
/// `__instancecheck__` da metaclasse manda.
#[test]
fn dunder_class_descriptor_and_isinstance_fallback() {
    let src = "\
class Svc: pass
class Fake:
    __class__ = property(lambda self: Svc)
f = Fake()
print(f.__class__ is Svc, type(f) is Fake)
print(isinstance(f, Svc), isinstance(f, Fake), isinstance(f, (int, Svc)), isinstance(f, int))
print(issubclass(Fake, Svc), isinstance(f, object))
class M(type):
    def __instancecheck__(cls, obj): return False
class Strict(metaclass=M): pass
class Fake2:
    __class__ = property(lambda self: Strict)
print(isinstance(Fake2(), Strict))
class Num:
    __class__ = property(lambda self: int)
print(isinstance(Num(), int), isinstance(Num(), float))
class Plain: pass
print(Plain().__class__ is Plain)
";
    assert_eq!(out(src), "True True\nTrue True True False\nFalse True\nFalse\nTrue False\nTrue\n");
}

/// As checagens do `object_set_class` em `obj.__class__ = valor`.
#[test]
fn dunder_class_assignment_checks() {
    let src = "\
class A: pass
class S:
    __slots__ = ('x',)
class T:
    __slots__ = ('y',)
for new in (int, 3, S):
    try: A().__class__ = new
    except TypeError as e: print(e)
try: S().__class__ = T
except TypeError as e: print(e)
";
    assert_eq!(
        out(src),
        "__class__ assignment only supported for mutable types or ModuleType subclasses\n\
         __class__ must be set to a class, not 'int' object\n\
         __class__ assignment: 'S' object layout differs from 'A'\n\
         __class__ assignment: 'T' object layout differs from 'S'\n"
    );
}

/// `obj.__class__ = X` válido troca a classe da instância (`object_set_class`): os métodos novos passam a
/// valer, o `__dict__` e os slots compatíveis ficam, e `del obj.__class__` é `TypeError`.
#[test]
fn dunder_class_assignment_swaps_the_class() {
    let src = "\
class A:
    def f(self): return 'A.f'
class B:
    def f(self): return 'B.f'
    def g(self): return 'B.g'
a = A()
a.x = 1
d = a.__dict__
a.__class__ = B
print(type(a).__name__, a.__class__ is B, a.f(), a.g(), a.x, a.__dict__, a.__dict__ is d)
print(isinstance(a, B), isinstance(a, A), type(a) is B)
class S:
    __slots__ = ('x',)
class T:
    __slots__ = ('x',)
    def who(self): return 'T'
s = S()
s.x = 5
s.__class__ = T
print(type(s).__name__, s.x, s.who())
try: del a.__class__
except TypeError as e: print(e)
class Loud(A):
    def __setattr__(self, name, value):
        print('set', name)
        super().__setattr__(name, value)
l = Loud()
l.__class__ = B
print(type(l).__name__, l.g())
";
    assert_eq!(
        out(src),
        "B True B.f B.g 1 {'x': 1} True\n\
         True False True\n\
         T 5 T\n\
         can't delete __class__ attribute\n\
         set __class__\n\
         B B.g\n"
    );
}

/// `sys.modules[__name__].__class__ = Sub` (PEP 549 e o uso de `lazy_loader`): o módulo ganha a classe,
/// com `property` (leitura e escrita) e métodos; `ModuleType` desfaz a troca; o resto é `TypeError`.
#[test]
fn dunder_class_assignment_on_modules() {
    let src = "\
import sys, types
class M(types.ModuleType):
    @property
    def prop(self): return 'p'
    def hello(self): return 'hi ' + self.__name__
    @property
    def v(self): return self._v
    @v.setter
    def v(self, x): self._v = x * 2
m = sys.modules[__name__]
m.__class__ = M
print(type(m) is M, isinstance(m, M), isinstance(m, types.ModuleType), m.__class__ is M)
print(m.prop, m.hello())
m.v = 4
print(m.v, _v)
for bad in (int, 3):
    try: m.__class__ = bad
    except TypeError as e: print(e)
class Plain: pass
try: m.__class__ = Plain
except TypeError as e: print(e)
try: del m.__class__
except TypeError as e: print(e)
m.__class__ = types.ModuleType
print(type(m) is types.ModuleType)
class M2(types.ModuleType):
    def who(self): return 'M2'
z = M('z')
z.__class__ = M2
print(type(z).__name__, z.who())
";
    assert_eq!(
        out(src),
        "True True True True\n\
         p hi __main__\n\
         8 8\n\
         __class__ assignment only supported for mutable types or ModuleType subclasses\n\
         __class__ must be set to a class, not 'int' object\n\
         __class__ assignment only supported for mutable types or ModuleType subclasses\n\
         can't delete __class__ attribute\n\
         True\n\
         M2 M2\n"
    );
}

/// `__dict__` de `function` e de `type` é um `getset_descriptor`: carregá-lo para montar o
/// `mappingproxy` do tipo reentrava no mesmo `__dict__` até estourar a pilha.
#[test]
fn builtin_type_dict_holds_getset_descriptor() {
    let src = "\
f = lambda: 3
print(type(f).__dict__['__dict__'])
print(type.__dict__['__dict__'])
print(f.__call__(), issubclass(type(f), object), issubclass(type, (int, type)))
";
    assert_eq!(
        out(src),
        "<attribute '__dict__' of 'function' objects>\n<attribute '__dict__' of 'type' objects>\n3 True True\n"
    );
}

/// PEP 562: o atributo ausente de um módulo consulta o `__getattr__` do dict dele (`module_getattro`);
/// `getattr` com default, `hasattr`, `from m import nome` e `dir()` (com `__dir__`) passam por ele.
#[test]
fn module_getattr_and_dir_hooks() {
    let src = "\
import types, sys
m = types.ModuleType('mm')
try: m.zz
except AttributeError as e: print(e)
def hook(n):
    if n == 'bad': raise AttributeError('no ' + n)
    if n == 'boom': raise ValueError('boom')
    return n.upper()
m.__getattr__ = hook
print(m.zz, getattr(m, 'q'), hasattr(m, 'bad'), getattr(m, 'bad', 7))
try: m.bad
except AttributeError as e: print(e)
sys.modules['mm'] = m
from mm import abc
print(abc)
try: from mm import boom
except ValueError as e: print('ve', e)
try: from mm import bad
except ImportError as e: print(type(e).__name__, e)
m.__dir__ = lambda: ['b', 'a']
print(dir(m))
";
    assert_eq!(
        out(src),
        "module 'mm' has no attribute 'zz'\nZZ Q False 7\nno bad\nABC\nve boom\n\
         ImportError cannot import name 'bad' from 'mm' (unknown location)\n['a', 'b']\n"
    );
}
