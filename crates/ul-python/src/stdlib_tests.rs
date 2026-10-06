//! Testes dos módulos da stdlib em Python embutido. Como os de `lang_tests`, rodam Python de verdade
//! e comparam com o que o CPython 3.13 imprime. Não há pseudo-processo nos testes unitários, então
//! tudo que toca o VFS (`open`, `os.listdir`...) é testado pela bancada do `osh`.

use crate::run_source;

fn out(src: &str) -> String {
    let o = run_source(src);
    assert_eq!(o.stderr, "", "stderr inesperado para:\n{src}");
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn every_embedded_module_imports() {
    // `logging` lê o relógio ao importar, e o relógio é do pseudo-processo (só existe na bancada).
    const NEEDS_PROCESS: &[&str] = &["logging"];
    for name in crate::modules::pysrc::names() {
        if NEEDS_PROCESS.contains(&name) {
            continue;
        }
        let o = run_source(&format!("import {name}\nprint('ok')"));
        assert_eq!(o.stderr, "", "módulo {name}");
        assert_eq!(o.stdout, b"ok\n", "módulo {name}");
    }
}

#[test]
fn posixpath_functions() {
    let src = "\
import os.path as p
print(p.join('a', 'b', '/c', 'd'), p.split('/x/y/z.txt'), p.splitext('a/b.tar.gz'))
print(p.normpath('/a/./b/../c//d'), p.basename('/a/b/'), p.dirname('/a/b/c'))
print(p.relpath('/a/b/c', '/a'), p.commonpath(['/a/b/c', '/a/b/d']), p.isabs('x'))
";
    assert_eq!(
        out(src),
        "/c/d ('/x/y', 'z.txt') ('a/b.tar', '.gz')\n/a/c/d  /a/b\nb/c /a/b False\n"
    );
}

#[test]
fn itertools_basics() {
    let src = "\
import itertools as it
print(list(it.islice(it.count(5), 3)), list(it.chain([1], 'ab')), list(it.accumulate([1, 2, 3])))
print(list(it.product('ab', repeat=2))[:3], list(it.permutations([1, 2, 3], 2))[:3])
print(list(it.combinations('abc', 2)), list(it.zip_longest('ab', [1], fillvalue=0)))
print([(k, list(g)) for k, g in it.groupby('aabbbc')])
print(list(it.pairwise([1, 2, 3])), list(it.batched(range(5), 2)))
";
    assert_eq!(
        out(src),
        "[5, 6, 7] [1, 'a', 'b'] [1, 3, 6]\n[('a', 'a'), ('a', 'b'), ('b', 'a')] [(1, 2), (1, 3), (2, 1)]\n\
         [('a', 'b'), ('a', 'c'), ('b', 'c')] [('a', 1), ('b', 0)]\n\
         [('a', ['a', 'a']), ('b', ['b', 'b', 'b']), ('c', ['c'])]\n[(1, 2), (2, 3)] [(0, 1), (2, 3), (4,)]\n"
    );
}

#[test]
fn functools_basics() {
    let src = "\
import functools
print(functools.reduce(lambda a, b: a + b, [1, 2, 3], 10))
add3 = functools.partial(lambda a, b, c: a + b + c, 1, 2)
print(add3(3))
@functools.lru_cache(maxsize=None)
def fib(n):
    return n if n < 2 else fib(n - 1) + fib(n - 2)
print(fib(30), fib.cache_info().hits > 0)
def deco(f):
    @functools.wraps(f)
    def w(*a):
        return f(*a)
    return w
@deco
def named(x):
    'doc'
    return x
print(named.__name__, named(4))
print(sorted(['bb', 'a', 'ccc'], key=functools.cmp_to_key(lambda a, b: len(b) - len(a))))
";
    assert_eq!(out(src), "16\n6\n832040 True\nnamed 4\n['ccc', 'bb', 'a']\n");
}

#[test]
fn contextlib_basics() {
    let src = "\
import contextlib
@contextlib.contextmanager
def tag(name):
    print('<' + name + '>')
    try:
        yield name
    finally:
        print('</' + name + '>')
with tag('b') as t:
    print('inside', t)
with contextlib.suppress(KeyError):
    {}['x']
print('after')
try:
    with tag('i'):
        raise ValueError('boom')
except ValueError as e:
    print('caught', e)
";
    assert_eq!(out(src), "<b>\ninside b\n</b>\nafter\n<i>\n</i>\ncaught boom\n");
}

#[test]
fn abc_blocks_abstract_instantiation() {
    let src = "\
from abc import ABC, abstractmethod
class Shape(ABC):
    @abstractmethod
    def area(self):
        pass
class Sq(Shape):
    def __init__(self, s):
        self.s = s
    def area(self):
        return self.s ** 2
print(Sq(3).area())
try:
    Shape()
except TypeError as e:
    print(e)
";
    assert_eq!(
        out(src),
        "9\nCan't instantiate abstract class Shape without an implementation for abstract method 'area'\n"
    );
}

#[test]
fn set_dict_operators_and_percent() {
    let src = "\
print({1, 2, 3} - {2}, {1, 2} | {3}, {1, 2} & {2, 3}, {1, 2} ^ {2, 3})
print({'a': 1} | {'b': 2}, '%s-%d-%.1f' % ('x', 3, 2.55))
s = {1}
s |= {2}
print(s)
";
    assert_eq!(out(src), "{1, 3} {1, 2, 3} {2} {1, 3}\n{'a': 1, 'b': 2} x-3-2.5\n{1, 2}\n");
}

#[test]
fn dataclasses_module() {
    let src = r#"
from dataclasses import dataclass, field, fields, asdict, astuple, replace, is_dataclass, FrozenInstanceError, KW_ONLY
from typing import List, Optional, ClassVar
import copy

@dataclass
class Point:
    x: int
    y: int = 0
    tags: List[str] = field(default_factory=list)
    count: ClassVar[int] = 0

p = Point(1, tags=['a'])
print(p, p == Point(1, 0, ['a']), p.tags, Point.count)
print(asdict(p), astuple(p), replace(p, y=5), is_dataclass(p), [f.name for f in fields(p)])

@dataclass(frozen=True, order=True)
class V:
    a: int
    b: str = 'z'
v = V(1)
try:
    v.a = 2
except FrozenInstanceError as e:
    print('frozen:', e)
print(v, v < V(2), hash(v) == hash(V(1)), {v: 1}[V(1)])

@dataclass
class Q:
    n: int
    def __post_init__(self):
        self.double = self.n * 2
print(Q(4).double, Q(4))

try:
    Point()
except TypeError as e:
    print(e)
try:
    Point(1, 2, [], 4)
except TypeError as e:
    print(e)

@dataclass
class Child(Point):
    z: int = 9
print(Child(1, 2), Child.__mro__[1].__name__)

@dataclass
class N:
    items: list = field(default_factory=list)
    name: str = field(default='n', repr=False)
a = N(); b = N(); a.items.append(1)
print(a, b, a == b)
c = copy.deepcopy(a); c.items.append(2)
print(a.items, c.items)
@dataclass
class K:
    a: int
    _: KW_ONLY
    b: int = 3
print(K(1, b=4), K(1))
try:
    @dataclass
    class Bad:
        a: int = 1
        b: int
except TypeError as e:
    print(e)
"#;
    assert_eq!(
        out(src),
        "Point(x=1, y=0, tags=['a']) True ['a'] 0\n\
         {'x': 1, 'y': 0, 'tags': ['a']} (1, 0, ['a']) Point(x=1, y=5, tags=['a']) True ['x', 'y', 'tags']\n\
         frozen: cannot assign to field 'a'\n\
         V(a=1, b='z') True True 1\n\
         8 Q(n=4)\n\
         Point.__init__() missing 1 required positional argument: 'x'\n\
         Point.__init__() takes from 2 to 4 positional arguments but 5 were given\n\
         Child(x=1, y=2, tags=[], z=9) Point\n\
         N(items=[1]) N(items=[]) False\n\
         [1] [1, 2]\n\
         K(a=1, b=4) K(a=1, b=3)\n\
         non-default argument 'b' follows default argument 'a'\n"
    );
}

#[test]
fn typing_module() {
    let src = r#"
from typing import List, Dict, Optional, Union, Any, Tuple, Callable, TypeVar, Generic, NamedTuple, TypedDict, Iterable, ClassVar, Literal, cast, get_type_hints
T = TypeVar('T')
print(List[int], Dict[str, int], Optional[int], Union[int, str], Tuple[int, ...], Callable[[int], str], Any, T)
print(Optional[List[str]], Union[int, None], List, Callable[..., int], Literal['a', 'b'])
class Box(Generic[T]):
    def __init__(self, item: T):
        self.item = item
b = Box[int](3)
print(b.item, Box[int])
class P(NamedTuple):
    x: int
    y: int = 0
p = P(1)
print(p, p.x, p._asdict(), P._fields)
class M(TypedDict):
    name: str
m = M(name='a')
print(m, type(m), M.__annotations__)
def f(a: int, b: Optional[str] = None) -> List[int]:
    return [a]
print(cast(int, '3'), get_type_hints(P))
x: ClassVar[int] = 3
print(isinstance([], list), List[int].__origin__, List[int].__args__)
"#;
    assert_eq!(
        out(src),
        "typing.List[int] typing.Dict[str, int] typing.Optional[int] typing.Union[int, str] typing.Tuple[int, ...] typing.Callable[[int], str] typing.Any ~T\n\
         typing.Optional[typing.List[str]] typing.Optional[int] typing.List typing.Callable[..., int] typing.Literal['a', 'b']\n\
         3 __main__.Box[int]\n\
         P(x=1, y=0) 1 {'x': 1, 'y': 0} ('x', 'y')\n\
         {'name': 'a'} <class 'dict'> {'name': <class 'str'>}\n\
         3 {'x': <class 'int'>, 'y': <class 'int'>}\n\
         True <class 'list'> (<class 'int'>,)\n"
    );
}

#[test]
fn enum_module() {
    let src = r#"
from enum import Enum, IntEnum, StrEnum, Flag, IntFlag, auto, unique
class Color(Enum):
    RED = 1
    GREEN = 2
    BLUE = 3
    ALIAS = 1
    def describe(self):
        return f'{self.name}={self.value}'
print(Color.RED, repr(Color.GREEN), Color.RED.name, Color.BLUE.value, Color(2), Color['BLUE'])
print(list(Color), len(Color), Color.RED in Color, Color.ALIAS is Color.RED, Color.RED.describe())
print(list(Color.__members__))
for c in Color:
    print(c.name, end=' ')
print()
try:
    Color(9)
except ValueError as e:
    print('erro:', e)
class Num(IntEnum):
    A = 1
    B = 2
print(Num.A + 1, Num.B == 2, Num(1), repr(Num.A), str(Num.A), f'{Num.B}', sorted([Num.B, Num.A]), isinstance(Num.A, int))
class S(StrEnum):
    X = auto()
    Y = auto()
print(S.X, S.Y.value, S('x'), S.X == 'x', repr(S.X))
class P(Flag):
    R = auto()
    W = auto()
    X = auto()
print(P.R | P.W, repr(P.R | P.W), P.R in (P.R | P.W), bool(P.R & P.W), (P.R | P.W | P.X).value)
class N(Enum):
    A = auto()
    B = auto()
print(N.A.value, N.B.value, {Color.RED: 'r'}[Color.RED])
E = Enum('E', 'ONE TWO THREE')
print(list(E), E.TWO.value)
"#;
    assert_eq!(
        out(src),
        "Color.RED <Color.GREEN: 2> RED 3 Color.GREEN Color.BLUE\n\
         [<Color.RED: 1>, <Color.GREEN: 2>, <Color.BLUE: 3>] 3 True True RED=1\n\
         ['RED', 'GREEN', 'BLUE', 'ALIAS']\n\
         RED GREEN BLUE \n\
         erro: 9 is not a valid Color\n\
         2 True 1 <Num.A: 1> 1 2 [<Num.A: 1>, <Num.B: 2>] True\n\
         x y x True <S.X: 'x'>\n\
         P.R|W <P.R|W: 3> True False 7\n\
         1 2 r\n\
         [<E.ONE: 1>, <E.TWO: 2>, <E.THREE: 3>] 2\n"
    );
}

#[test]
fn collections_module() {
    let src = "\
from collections import namedtuple, defaultdict, Counter, OrderedDict, deque, ChainMap
P = namedtuple('P', 'x y')
p = P(1, y=2)
print(p, p.x, p[1], p._asdict(), p._replace(x=9), P._fields, tuple(p), len(p))
x, y = p
print(x, y, p == (1, 2), isinstance(p, tuple))
dd = defaultdict(list)
dd['a'].append(1); dd['b'].append(2); dd['a'].append(3)
print(dd, dict(dd), len(dd))
c = Counter('abracadabra')
print(c, c.most_common(2), c['z'], sum(c.values()), list(c.elements())[:3])
print(Counter(a=2) + Counter(a=1, b=1), Counter(a=3) - Counter(a=1))
od = OrderedDict(); od['x'] = 1; od['y'] = 2; od.move_to_end('x')
print(od, list(od), od.popitem())
d = deque([1, 2, 3], maxlen=3); d.append(4); d.appendleft(0)
print(d, d.popleft(), d.pop(), len(d), list(d), d[0])
d.rotate(1); print(d)
cm = ChainMap({'a': 1}, {'a': 2, 'b': 3})
print(cm['a'], cm['b'], len(cm), sorted(cm), cm)
";
    assert_eq!(
        out(src),
        "P(x=1, y=2) 1 2 {'x': 1, 'y': 2} P(x=9, y=2) ('x', 'y') (1, 2) 2\n\
         1 2 True True\n\
         defaultdict(<class 'list'>, {'a': [1, 3], 'b': [2]}) {'a': [1, 3], 'b': [2]} 2\n\
         Counter({'a': 5, 'b': 2, 'r': 2, 'c': 1, 'd': 1}) [('a', 5), ('b', 2)] 0 11 ['a', 'a', 'a']\n\
         Counter({'a': 3, 'b': 1}) Counter({'a': 2})\n\
         OrderedDict({'y': 2}) ['y', 'x'] ('x', 1)\n\
         deque([2], maxlen=3) 0 3 1 [2] 2\n\
         deque([2], maxlen=3)\n\
         1 3 2 ['a', 'b'] ChainMap({'a': 1}, {'a': 2, 'b': 3})\n"
    );
}

#[test]
fn datetime_module() {
    let src = r#"
from datetime import *
import datetime as dtm
d = date(2024, 3, 15)
print(d, repr(d), d.weekday(), d.isoweekday(), d.isocalendar(), d.toordinal(), d.ctime())
print(d + timedelta(days=20), d - timedelta(days=75), date(2024, 12, 31) - d)
print(d.strftime('%d/%m/%Y %A %B'), f'{d:%Y}', d.replace(day=1), date.fromisoformat('2023-02-28'))
dt = datetime(2024, 3, 15, 13, 45, 10, 123456)
print(dt, repr(dt), dt.isoformat(), dt.isoformat(' ', 'seconds'), dt.date(), dt.time())
print(dt + timedelta(hours=12, minutes=30), dt - datetime(2024, 1, 1), (dt - datetime(2023, 1, 1)).total_seconds())
print(dt.strftime('%Y-%m-%d %H:%M:%S.%f %j %p %I'), dt.timestamp())
aware = datetime(2024, 3, 15, 13, 45, tzinfo=timezone.utc)
print(aware, repr(aware), aware.astimezone(timezone(timedelta(hours=-3))), aware.timestamp())
print(datetime.fromisoformat('2024-03-15T10:20:30'), datetime.fromisoformat('2024-03-15 10:20:30+02:00'))
print(datetime.fromtimestamp(1700000000), datetime.strptime('15/03/2024 08:09', '%d/%m/%Y %H:%M'))
td = timedelta(days=1, hours=2, minutes=3, seconds=4, microseconds=5)
print(td, repr(td), td * 2, td / 2, td // timedelta(hours=1), -td, abs(-td), timedelta(0), timedelta(seconds=-1))
print(sorted([dt, datetime(2020, 1, 1)]), d < date(2025, 1, 1), time(10, 30), repr(time(1, 2, 3)))
print(datetime.min, datetime.max, date.max, timedelta.max)
try:
    date(2024, 2, 30)
except ValueError as e:
    print(e)
"#;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r#"2024-03-15 datetime.date(2024, 3, 15) 4 5 datetime.IsoCalendarDate(year=2024, week=11, weekday=5) 738960 Fri Mar 15 00:00:00 2024
2024-04-04 2023-12-31 291 days, 0:00:00
15/03/2024 Friday March 2024 2024-03-01 2023-02-28
2024-03-15 13:45:10.123456 datetime.datetime(2024, 3, 15, 13, 45, 10, 123456) 2024-03-15T13:45:10.123456 2024-03-15 13:45:10 2024-03-15 13:45:10.123456
2024-03-16 02:15:10.123456 74 days, 13:45:10.123456 37979110.123456
2024-03-15 13:45:10.123456 075 PM 01 1710510310.123456
2024-03-15 13:45:00+00:00 datetime.datetime(2024, 3, 15, 13, 45, tzinfo=datetime.timezone.utc) 2024-03-15 10:45:00-03:00 1710510300.0
2024-03-15 10:20:30 2024-03-15 10:20:30+02:00
2023-11-14 22:13:20 2024-03-15 08:09:00
1 day, 2:03:04.000005 datetime.timedelta(days=1, seconds=7384, microseconds=5) 2 days, 4:06:08.000010 13:01:32.000002 26 -2 days, 21:56:55.999995 1 day, 2:03:04.000005 0:00:00 -1 day, 23:59:59
[datetime.datetime(2020, 1, 1, 0, 0), datetime.datetime(2024, 3, 15, 13, 45, 10, 123456)] True 10:30:00 datetime.time(1, 2, 3)
0001-01-01 00:00:00 9999-12-31 23:59:59.999999 9999-12-31 999999999 days, 23:59:59.999999
day is out of range for month
"#
    );
}

#[test]
fn random_module_matches_cpython() {
    let src = r#"
import random
random.seed(42)
print(random.random(), random.randint(1, 100), random.randrange(0, 50, 5), random.choice('abcdef'))
l = list(range(10)); random.shuffle(l); print(l)
print(random.sample(range(100), 5), random.sample('abcdefgh', 3), random.uniform(1, 5))
print(random.getrandbits(8), random.getrandbits(40), random.randbytes(4))
print(random.choices(['a', 'b', 'c'], k=5), random.choices([1, 2, 3], weights=[10, 1, 1], k=5))
print(random.gauss(0, 1), random.normalvariate(10, 2), random.expovariate(0.5), random.triangular(0, 10, 3))
r = random.Random('hello'); print(r.random(), r.randint(0, 10**6))
r = random.Random(12345678901234); print(r.random(), r.randrange(10))
st = r.getstate(); a = r.random(); r.setstate(st); print(a == r.random())
print(random.betavariate(2, 3), random.gammavariate(2.0, 1.5), random.gammavariate(0.5, 1))
"#;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r#"0.6394267984578837 4 20 b
[9, 7, 8, 0, 4, 6, 5, 1, 2, 3]
[4, 3, 11, 27, 29] ['a', 'e', 'b'] 3.864078451689614
179 461902006518 b'\xe0\xcbn8'
['b', 'a', 'c', 'c', 'a'] [1, 1, 1, 1, 1]
-0.6871759112629331 9.272043816080757 0.08884536119027676 3.848556395205186
0.3537754404730722 695414
0.02504137575317178 2
True
0.3940634165047749 4.065437491299427 0.18331590922815402
"#
    );
}

#[test]
fn pathlib_pure_paths_and_stat() {
    let src = r#"
from pathlib import PurePosixPath, Path
p = PurePosixPath('/usr/local/lib/python3.13/site.py')
print(p, repr(p), p.name, p.stem, p.suffix, p.parent, p.parts, list(p.parents)[:2], p.parents[1])
print(p.with_suffix('.txt'), p.with_name('x.py'), p.relative_to('/usr'), p.is_absolute(), p.match('*.py'), p.match('lib/*/site.py'))
print(PurePosixPath('a//b/./c/'), PurePosixPath('a', 'b', '/c', 'd'), PurePosixPath('a') / 'b' / PurePosixPath('c'), PurePosixPath(''))
import stat
print(stat.filemode(0o100644), stat.filemode(0o040755), stat.S_ISDIR(0o040755))
print(Path('/a/b').joinpath('c', 'd'), Path('x/y').with_stem('z'), Path('/').parent, Path('a/b').parents[0], Path('a/b/c').relative_to('a'))
"#;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r#"/usr/local/lib/python3.13/site.py PurePosixPath('/usr/local/lib/python3.13/site.py') site.py site .py /usr/local/lib/python3.13 ('/', 'usr', 'local', 'lib', 'python3.13', 'site.py') [PurePosixPath('/usr/local/lib/python3.13'), PurePosixPath('/usr/local/lib')] /usr/local/lib
/usr/local/lib/python3.13/site.txt /usr/local/lib/python3.13/x.py local/lib/python3.13/site.py True True True
a/b/c /c/d a/b/c .
-rw-r--r-- drwxr-xr-x True
/a/b/c/d x/z / a b/c
"#
    );
}

#[test]
fn zlib_module_matches_cpython() {
    let src = r#"
import zlib
data = b'hello world ' * 200 + bytes(range(256)) * 8
for lvl in (0, 1, 6, 9, -1):
    c = zlib.compress(data, lvl)
    print(lvl, len(c), c[:2].hex(), zlib.decompress(c) == data, zlib.crc32(c), zlib.adler32(c))
r = zlib.compress(data, 6, -15); print(len(r), zlib.decompress(r, -15) == data)
g = zlib.compress(data, 6, 31); print(len(g), g[:3].hex(), zlib.decompress(g, 31) == data, zlib.decompress(g, 47) == data)
co = zlib.compressobj(6, zlib.DEFLATED, -15)
parts = [co.compress(data[i:i + 700]) for i in range(0, len(data), 700)] + [co.flush()]
blob = b''.join(parts); print(len(blob), zlib.decompress(blob, -15) == data)
do = zlib.decompressobj(-15); out = b''
for i in range(0, len(blob), 100): out += do.decompress(blob[i:i + 100])
print(out == data, do.eof, do.unused_data)
do = zlib.decompressobj(); z = zlib.compress(b'abc' * 50) + b'TRAIL'
print(do.decompress(z), do.eof, do.unused_data)
try:
    zlib.decompress(b'not zlib at all')
except zlib.error as e:
    print(e)
try:
    zlib.decompress(zlib.compress(b'x' * 1000)[:-6])
except zlib.error as e:
    print(e)
print(zlib.crc32(b''), zlib.crc32(b'a'), zlib.adler32(b''), zlib.adler32(b'abc'), zlib.crc32(b'abc', zlib.crc32(b'x')))
"#;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r#"0 4459 7801 True 2373319531 3193209236
1 335 7801 True 1316960334 1180939190
6 327 789c True 1175475321 3885348811
9 327 78da True 4096918293 915978249
-1 327 789c True 1175475321 3885348811
321 True
339 1f8b08 True True
321 True
True True b''
b'abcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabc' True b'TRAIL'
Error -3 while decompressing data: incorrect header check
Error -5 while decompressing data: incomplete or truncated stream
0 3904355907 1 38600999 1168822207
"#
    );
}

#[test]
fn zipfile_in_memory_matches_cpython() {
    let src = r#"
import zipfile, io, os, hashlib
buf = io.BytesIO()
with zipfile.ZipFile(buf, 'w', zipfile.ZIP_DEFLATED) as z:
    for name, data in (('a.txt', b'hello ' * 100), ('dir/b.bin', bytes(range(256)) * 4), ('dir/', b''), ('ç.txt', 'ação'.encode())):
        zi = zipfile.ZipInfo(name, (2024, 3, 15, 13, 45, 10))
        zi.compress_type = zipfile.ZIP_DEFLATED if not name.endswith('/') else zipfile.ZIP_STORED
        zi.external_attr = (0o644 << 16) if not name.endswith('/') else (0o40755 << 16) | 0x10
        z.writestr(zi, data)
    z.comment = b'meu comentario'
raw = buf.getvalue()
print(len(raw), hashlib.sha256(raw).hexdigest())
print(zipfile.is_zipfile(io.BytesIO(raw)), zipfile.is_zipfile(io.BytesIO(b'nope')))
with zipfile.ZipFile(io.BytesIO(raw)) as z:
    print(z.namelist(), z.comment, z.testzip())
    for i in z.infolist():
        print(i.filename, i.file_size, i.compress_size, i.compress_type, i.date_time, oct(i.external_attr >> 16), i.CRC, i.is_dir())
    print(z.read('a.txt')[:12], len(z.read('dir/b.bin')), z.getinfo('a.txt'))
    with z.open('a.txt') as f: print(f.read(5), f.read(7))
    try: z.read('missing')
    except KeyError as e: print('KeyError', e)
"#;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r#"690 7899bce14d5d831ea47a52335042928d8343c5cd9ddc8ee94a8bb39fe040632b
True False
['a.txt', 'dir/b.bin', 'dir/', 'ç.txt'] b'meu comentario' None
a.txt 600 14 8 (2024, 3, 15, 13, 45, 10) 0o644 228733427 False
dir/b.bin 1024 280 8 (2024, 3, 15, 13, 45, 10) 0o644 3070970918 False
dir/ 0 0 0 (2024, 3, 15, 13, 45, 10) 0o40755 0 True
ç.txt 6 8 8 (2024, 3, 15, 13, 45, 10) 0o644 3350033681 False
b'hello hello ' 1024 <ZipInfo filename='a.txt' compress_type=deflate filemode='?rw-r--r--' file_size=600 compress_size=14>
b'hello' b' hello '
KeyError "There is no item named 'missing' in the archive"
"#
    );
}

#[test]
fn bytes_methods_match_cpython() {
    let src = r#"
b = b'  Hello, World hello  \n'
print(b.rfind(b'ello'), b.rfind(b'zz'), b.rfind(b'l', 0, 10), b.index(b'W'), b.rindex(b'o'))
print(b.partition(b','), b.rpartition(b'l'), b'abc'.partition(b'x'), b'abc'.rpartition(b'x'))
print(b'a,b,c,d'.rsplit(b',', 1), b'a b  c d '.rsplit(None, 2), b'a b c'.rsplit(), b'a,b'.rsplit(b','))
print(b'a\nb\r\nc\rd'.splitlines(), b'a\nb\r\nc\n'.splitlines(True), b''.splitlines())
print(b'abcdef'.removeprefix(b'abc'), b'abcdef'.removesuffix(b'def'), b'abc'.removeprefix(b'x'))
print(b'ab'.center(7, b'*'), b'ab'.ljust(5), b'ab'.rjust(5, b'-'), b'ab'.center(6), b'-42'.zfill(6), b'42'.zfill(5), b'abc'.center(2))
print(b'abc'.isalpha(), b'a1'.isalpha(), b'123'.isdigit(), b'a1'.isalnum(), b' \t'.isspace(), b'AB'.isupper(), b'ab'.islower(), b'\xff'.isascii(), b''.isalpha())
print(b'Hello World'.swapcase(), b'hELLO'.capitalize(), b'hello wORLD-x y2z'.title(), b'a\tb\tc'.expandtabs(4))
print(b'hello'.translate(bytes(range(256))), b'hello'.translate(None, b'l'), b'hello'.translate(bytes([ord('h') if i == ord('e') else i for i in range(256)])))
"#;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r#"16 -1 5 9 19
(b'  Hello', b',', b' World hello  \n') (b'  Hello, World hel', b'l', b'o  \n') (b'abc', b'', b'') (b'', b'', b'abc')
[b'a,b,c', b'd'] [b'a b', b'c', b'd'] [b'a', b'b', b'c'] [b'a', b'b']
[b'a', b'b', b'c', b'd'] [b'a\n', b'b\r\n', b'c\n'] []
b'def' b'abc' b'abc'
b'***ab**' b'ab   ' b'---ab' b'  ab  ' b'-00042' b'00042' b'abc'
True False True True True True True False False
b'hELLO wORLD' b'Hello' b'Hello World-X Y2Z' b'a   b   c'
b'hello' b'heo' b'hhllo'
"#
    );
}

#[test]
fn traceback_objects_and_exception_hierarchy() {
    let src = r#"
import traceback, sys

def inner(n):
    raise ValueError("boom %d" % n)

def middle(n):
    inner(n)

class MyErr(Exception):
    pass

try:
    middle(3)
except ValueError as e:
    tb = e.__traceback__
    print([(f.f_code.co_name, ln) for f, ln in traceback.walk_tb(tb)])
    print(tb.tb_lineno, tb.tb_frame.f_code.co_name, tb.tb_next.tb_lineno, tb.tb_next.tb_next.tb_next)
    print(traceback.format_exception_only(e))
    print(sys.exc_info()[2] is not None, sys.exc_info()[1] is e)
    print([(x.name, x.lineno) for x in traceback.extract_tb(tb)])

try:
    raise KeyError('k')
except KeyError as e:
    print(traceback.format_exception_only(type(e), e), traceback.format_exception_only(e))
try:
    raise MyErr()
except MyErr as e:
    print(traceback.format_exception_only(e), issubclass(UnicodeDecodeError, UnicodeError), issubclass(BrokenPipeError, OSError), IOError is OSError)
print(traceback.format_exc())
"#;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r#"[('<module>', 14), ('middle', 8), ('inner', 5)]
14 <module> 8 None
['ValueError: boom 3\n']
True True
[('<module>', 14), ('middle', 8), ('inner', 5)]
["KeyError: 'k'\n"] ["KeyError: 'k'\n"]
['MyErr\n'] True True True
NoneType: None

"#
    );
}

#[test]
fn warnings_and_getframe() {
    let src = r#"
import warnings, sys

def f():
    warnings.warn("careful", UserWarning)

base = sys._getframe().f_lineno
with warnings.catch_warnings(record=True) as w:
    warnings.simplefilter("always")
    f(); f()
    warnings.warn("dep", DeprecationWarning)
    print(len(w), w[0].category.__name__, str(w[0].message), w[0].lineno - base, w[2].category.__name__)
with warnings.catch_warnings():
    warnings.simplefilter("error")
    try:
        f()
    except UserWarning as e:
        print("raised", e)
def deep():
    fr = sys._getframe(1)
    return fr.f_code.co_name, fr.f_lineno - base, sys._getframe().f_back.f_lineno - base, sys._getframe(0).f_code.co_name
print(deep(), sys._getframe(0).f_code.co_name)
try:
    sys._getframe(5)
except ValueError as e:
    print(e)
"#;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r#"3 UserWarning careful -2 DeprecationWarning
raised careful
('<module>', 15, 15, 'deep') <module>
call stack is not deep enough
"#
    );
}

#[test]
fn gzip_module_matches_cpython() {
    let src = r#"
import gzip, io
data = b"hello gzip world\n" * 50 + bytes(range(256))
for lvl in (1, 6, 9):
    c = gzip.compress(data, lvl, mtime=0)
    print(lvl, len(c), c[:10].hex(), c[-8:].hex(), gzip.decompress(c) == data)
buf = io.BytesIO()
with gzip.GzipFile(filename="a.txt.gz", mode="wb", fileobj=buf, mtime=12345) as f:
    f.write(b"linha1\nlinha2\n")
    f.write(b"linha3\n")
raw = buf.getvalue()
print(raw.hex())
with gzip.GzipFile(fileobj=io.BytesIO(raw)) as f:
    print(f.readline(), f.read(3), f.readlines())
print(gzip.decompress(raw + raw))
try:
    gzip.decompress(b"nope nope nope")
except gzip.BadGzipFile as e:
    print("bad", e)
try:
    gzip.decompress(raw[:-5])
except EOFError as e:
    print("eof", e)
"#;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r#"1 318 1f8b08000000000004ff 2ce5cdd852040000 True
6 316 1f8b08000000000000ff 2ce5cdd852040000 True
9 316 1f8b08000000000002ff 2ce5cdd852040000 True
1f8b08083930000002ff612e74787400cbc9cccb4834e4ca01514610ca980b00a8bc074c15000000
b'linha1\n' b'lin' [b'ha2\n', b'linha3\n']
b'linha1\nlinha2\nlinha3\nlinha1\nlinha2\nlinha3\n'
bad Not a gzipped file (b'no')
eof Compressed file ended before the end-of-stream marker was reached
"#
    );
}

#[test]
fn argparse_module_matches_cpython() {
    let src = r#"
import argparse, sys

p = argparse.ArgumentParser(prog="tool", description="Faz coisas com arquivos.", epilog="fim")
p.add_argument("input", help="arquivo de entrada")
p.add_argument("extra", nargs="*", help="outros")
p.add_argument("-o", "--output", default="out.txt", help="saída (padrão: %(default)s)")
p.add_argument("-v", "--verbose", action="count", default=0)
p.add_argument("-q", action="store_true", help="quieto")
p.add_argument("-n", type=int, choices=[1, 2, 3], metavar="N")
p.add_argument("--tag", action="append", default=[])
p.add_argument("--mode", choices=["fast", "slow"], required=False)
g = p.add_mutually_exclusive_group()
g.add_argument("--yes", action="store_true")
g.add_argument("--no", action="store_true")
p.add_argument("--version", action="version", version="tool 1.2")
print(p.parse_args(["a.txt", "b", "c", "-vv", "-n", "2", "--tag", "x", "--tag=y", "-o", "res"]))
print(p.parse_args(["a.txt", "--yes"]))
print(p.parse_known_args(["a.txt", "--zzz", "q"]))
print(p.format_usage(), end="")
print(p.format_help())

sub = argparse.ArgumentParser(prog="git")
sp = sub.add_subparsers(dest="cmd", help="comandos")
a = sp.add_parser("add", help="adiciona")
a.add_argument("paths", nargs="+")
a.add_argument("-f", action="store_true")
c = sp.add_parser("commit", aliases=["ci"], help="comita")
c.add_argument("-m", required=True)
print(sub.parse_args(["add", "-f", "x", "y"]))
print(sub.parse_args(["ci", "-m", "msg"]))
sub.print_help()
c.print_help()
b = argparse.ArgumentParser(prog="b")
b.add_argument("--color", action=argparse.BooleanOptionalAction, default=True)
b.add_argument("pos", nargs="?", default="dflt")
print(b.parse_args(["--no-color"]), b.parse_args([]))
b.print_help()
"#;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r#"Namespace(input='a.txt', extra=['b', 'c'], output='res', verbose=2, q=False, n=2, tag=['x', 'y'], mode=None, yes=False, no=False)
Namespace(input='a.txt', extra=[], output='out.txt', verbose=0, q=False, n=None, tag=[], mode=None, yes=True, no=False)
(Namespace(input='a.txt', extra=['q'], output='out.txt', verbose=0, q=False, n=None, tag=[], mode=None, yes=False, no=False), ['--zzz'])
usage: tool [-h] [-o OUTPUT] [-v] [-q] [-n N] [--tag TAG] [--mode {fast,slow}]
            [--yes | --no] [--version]
            input [extra ...]
usage: tool [-h] [-o OUTPUT] [-v] [-q] [-n N] [--tag TAG] [--mode {fast,slow}]
            [--yes | --no] [--version]
            input [extra ...]

Faz coisas com arquivos.

positional arguments:
  input                arquivo de entrada
  extra                outros

options:
  -h, --help           show this help message and exit
  -o, --output OUTPUT  saída (padrão: out.txt)
  -v, --verbose
  -q                   quieto
  -n N
  --tag TAG
  --mode {fast,slow}
  --yes
  --no
  --version            show program's version number and exit

fim

Namespace(cmd='add', paths=['x', 'y'], f=True)
Namespace(cmd='ci', m='msg')
usage: git [-h] {add,commit,ci} ...

positional arguments:
  {add,commit,ci}  comandos
    add            adiciona
    commit (ci)    comita

options:
  -h, --help       show this help message and exit
usage: git commit [-h] -m M

options:
  -h, --help  show this help message and exit
  -m M
Namespace(color=False, pos='dflt') Namespace(color=True, pos='dflt')
usage: b [-h] [--color | --no-color] [pos]

positional arguments:
  pos

options:
  -h, --help           show this help message and exit
  --color, --no-color
"#
    );
}

#[test]
fn stdlib_batch_heapq_difflib_types() {
    let src = r#"
import heapq, colorsys, keyword, graphlib, reprlib, getopt, difflib, types, builtins
h = []
for x in [5, 1, 8, 3, 2]:
    heapq.heappush(h, x)
print([heapq.heappop(h) for _ in range(5)], heapq.nlargest(2, [4, 9, 1, 7]), heapq.nsmallest(2, [4, 9, 1, 7]))
print(heapq.merge([1, 4], [2, 3]).__class__.__name__, list(heapq.merge([1, 4], [2, 3])))
print(colorsys.rgb_to_hsv(0.2, 0.4, 0.4), colorsys.hls_to_rgb(0.5, 0.5, 0.5))
print(keyword.iskeyword("for"), keyword.iskeyword("foo"), len(keyword.kwlist))
ts = graphlib.TopologicalSorter({"b": ["a"], "c": ["a", "b"]})
print(list(ts.static_order()))
print(reprlib.repr(list(range(100))), reprlib.repr("x" * 100))
print(getopt.getopt(["-a", "-b", "val", "--long=3", "rest"], "ab:", ["long="]))
print(list(difflib.unified_diff(["a\n", "b\n", "c\n"], ["a\n", "x\n", "c\n"], "f1", "f2")))
print(difflib.SequenceMatcher(None, "abcd", "bcde").ratio(), difflib.get_close_matches("appel", ["apple", "ape", "peach"]))
print(types.FunctionType is type(lambda: 0), isinstance(len, types.BuiltinFunctionType), isinstance(types, types.ModuleType))
print(types.SimpleNamespace(a=1, b=2), builtins.len([1, 2]), builtins.int("7"))
print([1].__len__(), {1}.__contains__(1), "x".__class__.__name__, (3).__add__(4))
def f():
    a = 1
    b = 2
    return locals()
print(f(), callable(int), callable(f), callable(5))
class P:
    def __init__(self):
        self.z = 1
        self.a = 2
print(vars(P()), list(P().__dict__))
"#;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r#"[1, 2, 3, 5, 8] [9, 7] [1, 4]
generator [1, 2, 3, 4]
(0.5, 0.5, 0.4) (0.25, 0.7499999999999999, 0.75)
True False 35
['a', 'b', 'c']
[0, 1, 2, 3, 4, 5, ...] 'xxxxxxxxxxxx...xxxxxxxxxxxxx'
([('-a', ''), ('-b', 'val'), ('--long', '3')], ['rest'])
['--- f1\n', '+++ f2\n', '@@ -1,3 +1,3 @@\n', ' a\n', '-b\n', '+x\n', ' c\n']
0.75 ['apple', 'ape']
True True True
namespace(a=1, b=2) 2 7
1 True str 7
{'a': 1, 'b': 2} True True False
{'z': 1, 'a': 2} ['z', 'a']
"#
    );
}

#[test]
fn weakref_module_matches_cpython() {
    let src = r#"
import weakref

class C:
    def m(self):
        return 1

o = C()
r = weakref.ref(o)
print(r() is o, r() is not None)
called = []
r2 = weakref.ref(o, lambda x: called.append("cb"))
print(r == weakref.ref(o), hash(r) == hash(weakref.ref(o)))
s = weakref.WeakSet()
s.add(o)
print(len(s), o in s)
d = weakref.WeakValueDictionary()
d["k"] = o
print(list(d.keys()), d["k"] is o)
wk = weakref.WeakKeyDictionary()
wk[o] = 5
print(len(wk), wk[o])
wm = weakref.WeakMethod(o.m)
print(wm()())
del o
print(r(), len(s), len(d), len(wk), wm())
print(r2(), called)
try:
    weakref.ref([1])
except TypeError as e:
    print(e)
f = weakref.finalize(C(), print, "fin")
print(f.alive)
"#;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r#"True True
True True
1 True
['k'] True
1 5
1
None 0 0 0 None
None ['cb']
cannot create weak reference to 'list' object
fin
False
"#
    );
}

#[test]
fn threading_and_futures_serial() {
    let src = r#"
import threading, time
from concurrent.futures import ThreadPoolExecutor, as_completed, wait
results = []
lock = threading.Lock()
def work(n):
    with lock:
        results.append(n * n)
ts = [threading.Thread(target=work, args=(i,)) for i in range(4)]
for t in ts:
    t.start()
for t in ts:
    t.join()
print(sorted(results), threading.current_thread().name, threading.active_count())
ev = threading.Event()
ev.set()
print(ev.wait(0.01), ev.is_set())
with ThreadPoolExecutor(max_workers=3) as ex:
    fs = [ex.submit(pow, 2, i) for i in range(5)]
    print([f.result() for f in fs])
    print(list(ex.map(lambda x: x + 1, [1, 2, 3])))
    bad = ex.submit(lambda: 1 / 0)
    print(type(bad.exception()).__name__, bad.done())
    done, pending = wait(fs)
    print(len(done), len(pending), sorted(f.result() for f in as_completed(fs)))
t = threading.Thread(target=lambda: print("in", threading.current_thread().name), name="worker")
t.start()
print(t.is_alive(), repr(t).split()[0])
class Counter:
    def __init__(self):
        self.n = 0
        self.lock = threading.RLock()
    def inc(self):
        with self.lock:
            self.n += 1
c = Counter()
for _ in range(3):
    threading.Thread(target=c.inc).start()
print(c.n)
"#;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r#"[0, 1, 4, 9] MainThread 1
True True
[1, 2, 4, 8, 16]
[2, 3, 4]
ZeroDivisionError True
5 0 [1, 2, 4, 8, 16]
in worker
False <Thread(worker,
3
"#
    );
}

#[test]
fn xml_etree_and_xlsx_reading() {
    let src = r#"
import xml.etree.ElementTree as ET, io, zipfile
doc = '''<?xml version="1.0" encoding="UTF-8"?>
<root xmlns="http://x/ns" xmlns:a="http://a/ns" id="1">
  <!-- comment -->
  <item a:k="v" n="2">text &amp; more &#233; &lt;</item>
  <item n="3"><![CDATA[raw <b>]]></item>
  <empty/>
  <nested><deep>1</deep><deep>2</deep></nested>
</root>'''
root = ET.fromstring(doc)
print(root.tag, root.attrib)
for it in root.iter("{http://x/ns}item"):
    print(it.attrib, repr(it.text))
print([e.tag for e in root], root.find("{http://x/ns}nested/{http://x/ns}deep").text)
print(ET.tostring(ET.fromstring("<a x='1'><b>t</b><c/></a>")))
a = ET.Element("sheet", name="s1")
r = ET.SubElement(a, "row", n="1")
c = ET.SubElement(r, "c")
c.text = "v<&>"
print(ET.tostring(a, encoding="unicode"))
ET.indent(a)
print(ET.tostring(a, encoding="unicode"))
tree = ET.ElementTree(a)
buf = io.BytesIO()
tree.write(buf, encoding="utf-8", xml_declaration=True)
print(buf.getvalue())
print([ (e.tag, e.get("n")) for e in ET.fromstring("<a><b n='1'/><b n='2'/></a>").findall("b")])
try:
    ET.fromstring("<a><b></a>")
except ET.ParseError as e:
    print("ParseError", e)
try:
    ET.fromstring("")
except ET.ParseError as e:
    print("ParseError", e)
# xlsx minimo
z = io.BytesIO()
with zipfile.ZipFile(z, "w", zipfile.ZIP_DEFLATED) as zf:
    zf.writestr(zipfile.ZipInfo("xl/sharedStrings.xml", (2024, 1, 1, 0, 0, 0)), '<?xml version="1.0"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>Nome</t></si><si><t>Ana</t></si></sst>')
    zf.writestr(zipfile.ZipInfo("xl/worksheets/sheet1.xml", (2024, 1, 1, 0, 0, 0)), '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1"><v>42</v></c></row><row r="2"><c r="A2" t="s"><v>1</v></c><c r="B2"><v>3.5</v></c></row></sheetData></worksheet>')
ns = {"m": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}
with zipfile.ZipFile(io.BytesIO(z.getvalue())) as zf:
    strings = [t.text for t in ET.fromstring(zf.read("xl/sharedStrings.xml")).iterfind(".//m:t", ns)]
    sheet = ET.fromstring(zf.read("xl/worksheets/sheet1.xml"))
    for row in sheet.iterfind(".//m:row", ns):
        vals = []
        for c in row.findall("m:c", ns):
            v = c.find("m:v", ns).text
            vals.append(strings[int(v)] if c.get("t") == "s" else float(v))
        print(vals)
for ev, el in ET.iterparse(io.BytesIO(b"<a><b>1</b><b>2</b></a>"), events=("start", "end")):
    print(ev, el.tag)
"#;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r#"{http://x/ns}root {'id': '1'}
{'{http://a/ns}k': 'v', 'n': '2'} 'text & more é <'
{'n': '3'} 'raw <b>'
['{http://x/ns}item', '{http://x/ns}item', '{http://x/ns}empty', '{http://x/ns}nested'] 1
b'<a x="1"><b>t</b><c /></a>'
<sheet name="s1"><row n="1"><c>v&lt;&amp;&gt;</c></row></sheet>
<sheet name="s1">
  <row n="1">
    <c>v&lt;&amp;&gt;</c>
  </row>
</sheet>
b'<?xml version=\'1.0\' encoding=\'utf-8\'?>\n<sheet name="s1">\n  <row n="1">\n    <c>v&lt;&amp;&gt;</c>\n  </row>\n</sheet>'
[('b', '1'), ('b', '2')]
ParseError mismatched tag: line 1, column 8
ParseError no element found: line 1, column 0
['Nome', 42.0]
['Ana', 3.5]
start a
start b
end b
start b
end b
end a
"#
    );
}
