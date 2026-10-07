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
    // `unittest.__main__` roda o `unittest.main()` ao ser importado: só vale como `python3 -m unittest`.
    // `this` imprime o Zen do Python ao ser importado. `PIL.ImageShow` procura os visualizadores no
    // `PATH` (`shutil.which`) ao ser importado; `PIL.__main__` e `PIL.report` imprimem o relatório
    // do `python3 -m PIL`.
    const NEEDS_PROCESS: &[&str] = &["unittest.__main__", "this", "PIL.ImageShow", "PIL.__main__", "PIL.report"];
    // `tkinter` precisa do Tk nativo (`_tkinter`), que o sandbox não tem: o oráculo instala o
    // `python3-tk` e importa, então este é um buraco conhecido do nosso lado, não do oráculo.
    const MISSING_DEPS: &[&str] = &["PIL._tkinter_finder"];
    // Importam o `sysconfig` ou o `pkgutil` reais, que vivem no disco da imagem e não existem no
    // teste unitário: a bancada (`python/stdlib-disk.toml` e os casos de datas) cobre esses.
    const NEEDS_DISK: &[&str] = &["trace", "pydoc", "zoneinfo", "zoneinfo._tzpath", "zoneinfo._common", "zoneinfo._zoneinfo", "unittest.mock"];
    for name in crate::modules::pysrc::names() {
        if NEEDS_PROCESS.contains(&name) || MISSING_DEPS.contains(&name) || NEEDS_DISK.contains(&name) {
            continue;
        }
        // Os módulos de apoio não existem para o programa: o import deles falha como no CPython.
        if crate::modules::INTERNAL.contains(&name) {
            let o = run_source(&format!("import {name}"));
            assert!(o.stderr.ends_with(&format!("ModuleNotFoundError: No module named '{name}'\n")), "módulo {name}");
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
t.join()
print(t.is_alive(), repr(t).split()[0])
class Counter:
    def __init__(self):
        self.n = 0
        self.lock = threading.RLock()
    def inc(self):
        with self.lock:
            self.n += 1
c = Counter()
workers = [threading.Thread(target=c.inc) for _ in range(3)]
for w in workers:
    w.start()
for w in workers:
    w.join()
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

#[test]
fn bytes_percent_re_bytes_and_json_package() {
    let src = r##"
import re, json, os
from pathlib import Path

print(b"len=%d name=%s hex=%02x %b %%" % (7, b"ab", 255, b"zz"))
print(b"%(a)s-%(b)d" % {b"a": b"x", b"b": 3} if False else b"%5.1f|%-4d|" % (3.14159, 42))

m = re.search(rb"(?P<k>\w+)=(\d+)", b"xx key=42 yy")
print(m, m.group(0), m.group("k"), m[2], m.span(2), m.groups(), m.groupdict())
print(re.findall(rb"\d+", b"a1b22c333"), re.findall(rb"(a)(\d)", b"a1 a2"))
print(re.sub(rb"\s+", b" ", b"a  b\n\tc"), re.sub(rb"(\d)", rb"<\1>", b"a1b2"))
print(re.sub(rb"\d", lambda mo: b"#" * int(mo.group()), b"a2b3"))
print(re.split(rb",\s*", b"a, b,c"), re.escape(b"a.b*c"))
print(re.compile(rb"ab+", re.I), re.compile(rb"ab+").flags, re.compile(b"x").pattern)
print([x.group() for x in re.finditer(rb"stream\r?\n(.*?)\r?\nendstream", b"stream\nAB\nendstream stream\nCD\nendstream", re.S)])
print(re.match(rb"\xe9", b"\xe9") is not None, re.match(rb"\w", b"\xe9"))
for bad in ((rb"a", "a"), ("a", b"a")):
    try:
        re.search(*bad)
    except TypeError as e:
        print(e)

d = {"b": [1, 2.5, {"z": None}], "a": "ação", "t": True}
print(json.dumps(d, indent=2, sort_keys=True, ensure_ascii=False))
print(json.dumps(d, separators=(",", ":"), sort_keys=True))
print(json.dumps({"x": (1, 2)}, indent="\t"))
print(json.loads('{"a": [1, 2, {"b": null}], "c": "\\u00e7"}', object_pairs_hook=list))
print(json.loads(b'{"k": 1.5e2}'))
try:
    json.loads("{bad")
except json.JSONDecodeError as e:
    print(type(e).__name__, e.msg, e.pos, e.lineno, e.colno, e)
print(json.dumps(Path("/a"), default=str), json.dumps(set(), default=sorted))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"b'len=7 name=ab hex=ff zz %'
b'  3.1|42  |'
<re.Match object; span=(3, 9), match=b'key=42'> b'key=42' b'key' b'42' (7, 9) (b'key', b'42') {'k': b'key'}
[b'1', b'22', b'333'] [(b'a', b'1'), (b'a', b'2')]
b'a b c' b'a<1>b<2>'
b'a##b###'
[b'a', b'b', b'c'] b'a\\.b\\*c'
re.compile(b'ab+', re.IGNORECASE) 0 b'x'
[b'stream\nAB\nendstream', b'stream\nCD\nendstream']
True None
cannot use a bytes pattern on a string-like object
cannot use a string pattern on a bytes-like object
{
  "a": "ação",
  "b": [
    1,
    2.5,
    {
      "z": null
    }
  ],
  "t": true
}
{"a":"a\u00e7\u00e3o","b":[1,2.5,{"z":null}],"t":true}
{
	"x": [
		1,
		2
	]
}
[('a', [1, 2, [('b', None)]]), ('c', 'ç')]
{'k': 150.0}
JSONDecodeError Expecting property name enclosed in double quotes 1 1 2 Expecting property name enclosed in double quotes: line 1 column 2 (char 1)
"/a" []
"##
    );
}

#[test]
fn bigint_arithmetic_matches_cpython() {
    let src = r##"
a = 2 ** 100
b = 3 ** 70
print(a, b, a * b, a + b, b - a, -a, abs(-a), a // 7, a % 7, divmod(b, a), a / 3, b / a)
print(a == 2 ** 100, a != b, a < b, a >= b, a == float(a), a < 1e40, hash(a), hash(-a), type(a), isinstance(a, int))
print(a & (2 ** 70 - 1), a | 1, a ^ (a - 1), a >> 90, 1 << 100, ~a, -7 >> 100, 7 << 70)
print(int("123456789012345678901234567890"), int("-0xffffffffffffffffffff", 16), int(1e30), int("1" * 40, 2) if False else 0)
print(float(a), float(10 ** 30), 10 ** 30 / 3, 10 ** 25 // 3, (10 ** 25) % 7, round(10 ** 25 + 5, -1), round(2 ** 70, -3))
print(hex(a), oct(2 ** 70), bin(2 ** 65), hex(-a))
print(str(a), repr(b), "%d|%s|%x|%o" % (a, a, a, a), f"{a}|{a:,}|{a:x}|{a:#o}|{a:>40}|{a:_d}|{a:e}|{-a:+}")
print("{:d} {:b} {:X} {:030d}".format(a, a, a, a))
print(pow(a, 3), pow(3, 200, 10 ** 20 + 7), pow(a, -1, 10 ** 20 + 7), a ** 0, (-2) ** 101, 2 ** -3)
print(sum([2 ** 63, 2 ** 63, 5]), max(a, b), min(a, b), sorted([b, a, 5, 2 ** 64]), 2 ** 63, -2 ** 63, 2 ** 63 - 1, 9223372036854775807 + 1, -9223372036854775808 - 1)
print(9223372036854775807 * 9223372036854775807, (2 ** 64) // (2 ** 32), (2 ** 64) % 1000, 2 ** 64 // -3, -(2 ** 64) // 3, -(2 ** 64) % 7)
print(a.bit_length(), (-a).bit_length(), a.to_bytes(13, "big"), int.from_bytes(b"\x01" + b"\x00" * 15, "big"), a.to_bytes(16, "little").hex())
print(int.from_bytes(b"\xff" * 16, "little", signed=True), (-a).to_bytes(14, "big", signed=True).hex(), a.real, a.imag, a.numerator, a.denominator, a.conjugate())
print({a: 1}[2 ** 100], a in {2 ** 100}, [a, b].index(b), a is a, bool(a), a.__class__.__name__, a.is_integer() if hasattr(a, "is_integer") else "-")
import math
print(math.factorial(25), math.factorial(30), math.comb(100, 50), math.perm(30, 15), math.gcd(2 ** 80, 6 ** 40), math.lcm(2 ** 70, 3 ** 40), math.isqrt(10 ** 40), math.prod([2 ** 40, 2 ** 40]))
print(math.floor(1e30), math.ceil(1e30), math.trunc(-1e30), math.sqrt(a), math.log2(a), math.log10(10 ** 40), math.log(a), math.fsum([1e30, 1]))
print(int(str(a)) == a, [int(c) for c in str(2 ** 70)][:5], len(str(3 ** 1000)), sum(map(int, str(2 ** 1000))))
import json, struct
print(json.dumps({"n": a}), json.loads('{"n": 123456789012345678901234567890}'))
print(divmod(-a, 7), divmod(a, -7), (-a) // 7, a.__add__(1) if hasattr(a, "__add__") else "", a.__mul__(2), int.__repr__(a))
x = 1
for i in range(1, 40):
    x *= i
print(x, x % 1000007, x // 10 ** 20, str(x)[::-1])
f1, f2 = 0, 1
for _ in range(300):
    f1, f2 = f2, f1 + f2
print(f1, len(str(f1)))
print(range(3)[1], 5 ** 30 % 97, 7 ** 77 % 1000, (a + 1) % 2, bytes([a % 256]), "x" * (a % 5), [0] * (a % 3))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"1267650600228229401496703205376 2503155504993241601315571986085849 3173126578369279394610431020106175019306389315838160670214324224 2504423155593469830717068689291225 2501887854393013371914075282880473 -1267650600228229401496703205376 1267650600228229401496703205376 181092942889747057356671886482 2 (1974, 813220142716762761079858673625) 4.2255020007607644e+29 1974.6415175779275
True True True False True True 549755813888 -549755813888 <class 'int'> True
0 1267650600228229401496703205377 2535301200456458802993406410751 1024 1267650600228229401496703205376 -1267650600228229401496703205377 -1 8264141345021879123968
123456789012345678901234567890 -1208925819614629174706175 1000000000000000019884624838656 0
1.2676506002282294e+30 1e+30 3.333333333333333e+29 3333333333333333333333333 3 10000000000000000000000000 1180591620717411303000
0x10000000000000000000000000 0o200000000000000000000000 0b100000000000000000000000000000000000000000000000000000000000000000 -0x10000000000000000000000000
1267650600228229401496703205376 2503155504993241601315571986085849 1267650600228229401496703205376|1267650600228229401496703205376|10000000000000000000000000|2000000000000000000000000000000000 1267650600228229401496703205376|1,267,650,600,228,229,401,496,703,205,376|10000000000000000000000000|0o2000000000000000000000000000000000|         1267650600228229401496703205376|1_267_650_600_228_229_401_496_703_205_376|1.267651e+30|-1267650600228229401496703205376
1267650600228229401496703205376 10000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000 10000000000000000000000000 1267650600228229401496703205376
2037035976334486086268445688409378161051468393665936250636140449354381299763336706183397376 68354989064495052399 88697245551632721 1 -2535301200456458802993406410752 0.125
18446744073709551621 2503155504993241601315571986085849 1267650600228229401496703205376 [5, 18446744073709551616, 1267650600228229401496703205376, 2503155504993241601315571986085849] 9223372036854775808 -9223372036854775808 9223372036854775807 9223372036854775808 -9223372036854775809
85070591730234615847396907784232501249 4294967296 616 -6148914691236517206 -6148914691236517206 5
101 101 b'\x10\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00' 1329227995784915872903807060280344576 00000000000000000000000010000000
-1 fff0000000000000000000000000 1267650600228229401496703205376 0 1267650600228229401496703205376 1 1267650600228229401496703205376
1 True 1 True True int True
15511210043330985984000000 265252859812191058636308480000000 100891344545564193334812497256 202843204931727360000 1099511627776 14353237968448109868972222216943775514624 100000000000000000000 1208925819614629174706176
1000000000000000019884624838656 1000000000000000019884624838656 -1000000000000000019884624838656 1125899906842624.0 100.0 40.0 69.31471805599453 1e+30
True [1, 1, 8, 0, 5] 478 1366
{"n": 1267650600228229401496703205376} {'n': 123456789012345678901234567890}
(-181092942889747057356671886483, 5) (-181092942889747057356671886483, -5) -181092942889747057356671886483 1267650600228229401496703205377 2535301200456458802993406410752 1267650600228229401496703205376
20397882081197443358640281739902897356800000000 327758 203978820811974433586402817 00000000865379820993718204685334479118028879302
222232244629420445529739893461909967206666939096499764990979600 63
1 79 207 1 b'\x00' x [0]
"##
    );
}

#[test]
fn complex_type_matches_cpython() {
    let src = r##"
a = 3 + 4j
b = complex(1, -2)
print(a, b, repr(1j), 2.5j, -3j, complex(0, 0), complex(-0.0, 1), complex("1+2j"), complex(" (3-4j) "), complex("2j"), complex(1.5))
print(a + b, a - b, a * b, a / b, -a, +a, abs(a), a.conjugate(), a.real, a.imag, a == complex(3, 4), a != b, a == 3, complex(2, 0) == 2)
print(a ** 2, a ** 0, 1j ** 2, 2 ** 1j, a ** -1, a ** 0.5, 5 + a, 5 - a, 5 * a, 1 / a, bool(0j), bool(a), type(a), isinstance(a, complex))
print(hash(1j) == hash(complex(0, 1)), hash(complex(3, 0)) == hash(3), {a: 1}[3 + 4j], f"{a}", f"{a:.2f}", format(b, ".1e"), str(a), [a, b])
try:
    a < b
except TypeError as e:
    print(e)
try:
    1j / 0
except ZeroDivisionError as e:
    print(e)
try:
    complex("abc")
except ValueError as e:
    print(e)
import numbers
print(isinstance(a, numbers.Complex), isinstance(3, numbers.Complex), isinstance(3, numbers.Rational), isinstance(2.5, numbers.Rational), isinstance(a, numbers.Real))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"(3+4j) (1-2j) 1j 2.5j (-0-3j) 0j (-0+1j) (1+2j) (3-4j) 2j (1.5+0j)
(4+2j) (2+6j) (11-2j) (-1+2j) (-3-4j) (3+4j) 5.0 (3-4j) 3.0 4.0 True True False True
(-7+24j) (1+0j) (-1+0j) (0.7692389013639721+0.6389612763136348j) (0.12-0.16j) (2+1j) (8+4j) (2-4j) (15+20j) (0.12-0.16j) False True <class 'complex'> True
True True 1 (3+4j) 3.00+4.00j 1.0e+00-2.0e+00j (3+4j) [(3+4j), (1-2j)]
'<' not supported between instances of 'complex' and 'complex'
complex division by zero
complex() arg is a malformed string
True True True False False
"##
    );
}

#[test]
fn config_queue_calendar_uuid_urllib_matches_cpython() {
    let src = r##"
import configparser, io, queue, calendar, uuid, hmac, hashlib, secrets, numbers
from urllib.parse import urlparse, urlsplit, parse_qs, parse_qsl, urlencode, quote, unquote, urljoin, quote_plus, urlunparse

cfg = configparser.ConfigParser()
cfg.read_string("""
[server]
host = example.com
port = 8080
debug = yes
path = /srv/%(host)s

[db]
url = postgres://u:p@h/db
timeout = 2.5
""")
print(cfg.sections(), cfg["server"]["host"], cfg.getint("server", "port"), cfg.getboolean("server", "debug"))
print(cfg.getfloat("db", "timeout"), cfg.get("server", "path"), cfg.has_option("db", "x"), dict(cfg["db"]))
cfg["new"] = {"a": "1", "b": "two"}
buf = io.StringIO()
cfg.write(buf)
print(buf.getvalue())
try:
    cfg.get("nope", "x")
except configparser.NoSectionError as e:
    print(type(e).__name__, e)

q = queue.Queue()
for i in range(3):
    q.put(i)
print(q.qsize(), q.get(), q.get_nowait(), q.empty(), q.full())
pq = queue.PriorityQueue()
for v in (5, 1, 3):
    pq.put(v)
print([pq.get() for _ in range(3)])
lq = queue.LifoQueue()
lq.put("a"); lq.put("b")
print(lq.get(), lq.get())
try:
    q.get_nowait(); q.get_nowait()
except queue.Empty:
    print("Empty")

print(calendar.isleap(2024), calendar.monthrange(2025, 2), calendar.weekday(2025, 1, 31), calendar.month_name[3], calendar.day_abbr[0])
print(calendar.month(2025, 2))
print(calendar.TextCalendar().formatmonth(2024, 12).splitlines()[1])

u = uuid.UUID("12345678-1234-5678-1234-567812345678")
print(u, u.hex, u.int, u.version, repr(u), u.bytes[:4], str(u.urn))
print(uuid.uuid5(uuid.NAMESPACE_DNS, "example.com"), uuid.uuid3(uuid.NAMESPACE_URL, "http://x/"))

print(hmac.new(b"key", b"msg", hashlib.sha256).hexdigest())
print(hmac.compare_digest("abc", "abc"), hmac.digest(b"k", b"m", "md5").hex())
print(isinstance(3, numbers.Integral), isinstance(2.5, numbers.Real), isinstance(1, numbers.Number), isinstance("a", numbers.Number))

p = urlparse("https://user:pw@example.com:8443/a/b;p?x=1&y=2#frag")
print(p, p.hostname, p.port, p.username, p.path, p.query, p.fragment)
print(parse_qs("a=1&a=2&b=%C3%A7"), parse_qsl("x=1&y=&z"))
print(urlencode({"q": "a b", "n": [1, 2]}, doseq=True), quote("/ã b?"), quote_plus("a b/c"), unquote("%E2%9C%93+x"))
print(urljoin("http://a/b/c/d;p?q", "../g"), urlunparse(("http", "h", "/p", "", "q=1", "")), urlsplit("//h/p").netloc)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"['server', 'db'] example.com 8080 True
2.5 /srv/example.com False {'url': 'postgres://u:p@h/db', 'timeout': '2.5'}
[server]
host = example.com
port = 8080
debug = yes
path = /srv/%(host)s

[db]
url = postgres://u:p@h/db
timeout = 2.5

[new]
a = 1
b = two


NoSectionError No section: 'nope'
3 0 1 False False
[1, 3, 5]
b a
Empty
True (calendar.SATURDAY, 28) 4 March Mon
   February 2025
Mo Tu We Th Fr Sa Su
                1  2
 3  4  5  6  7  8  9
10 11 12 13 14 15 16
17 18 19 20 21 22 23
24 25 26 27 28

Mo Tu We Th Fr Sa Su
12345678-1234-5678-1234-567812345678 12345678123456781234567812345678 24197857161011715162171839636988778104 None UUID('12345678-1234-5678-1234-567812345678') b'\x124Vx' urn:uuid:12345678-1234-5678-1234-567812345678
cfbff0d1-9375-5685-968c-48ce8b15ae17 c96e1d5f-9e80-3fac-af64-6996b5d74334
2d93cbc1be167bcb1637a4a23cbff01a7878f0c50ee833954ea5221bb1b8c628
True ed7e724d3a91554aaa2043041d9c5305
True True True False
ParseResult(scheme='https', netloc='user:pw@example.com:8443', path='/a/b', params='p', query='x=1&y=2', fragment='frag') example.com 8443 user /a/b x=1&y=2 frag
{'a': ['1', '2'], 'b': ['ç']} [('x', '1')]
q=a+b&n=1&n=2 /%C3%A3%20b%3F a+b%2Fc ✓+x
http://a/b/g http://h/p?q=1 h
"##
    );
}

#[test]
fn match_statement_matches_cpython() {
    let src = r##"
from dataclasses import dataclass


@dataclass
class Point:
    x: int
    y: int


class Box:
    __match_args__ = ("w", "h")

    def __init__(self, w, h):
        self.w, self.h = w, h


def f(v):
    match v:
        case 0 | 1:
            return "small"
        case int(n) if n > 100:
            return f"big {n}"
        case int():
            return "int"
        case str() as s:
            return "str " + s
        case [] | ():
            return "empty"
        case [x]:
            return f"one {x}"
        case [1, 2, *rest]:
            return f"12 then {rest}"
        case [first, *mid, last]:
            return f"{first} {mid} {last}"
        case {"k": 1, **kw}:
            return f"k1 {kw}"
        case {"name": name, "age": age}:
            return f"{name} {age}"
        case Point(x=0, y=0):
            return "origin"
        case Point(x, y):
            return f"pt {x},{y}"
        case Box(w, h):
            return f"box {w}x{h}"
        case None:
            return "none"
        case True:
            return "true"
        case float(z):
            return f"float {z}"
        case _:
            return "other"


for v in [0, 1, 5, 500, "hi", [], (), [9], [1, 2, 3, 4], [7, 8, 9, 10], {"k": 1, "z": 2}, {"name": "a", "age": 3},
          Point(0, 0), Point(1, 2), Box(2, 3), None, 2.5, {1}, [1, 2]]:
    print(type(v).__name__, "->", f(v))

cmd = "go north"
match cmd.split():
    case ["go", direction]:
        print("going", direction)
    case ["quit"]:
        print("bye")

match (1, (2, 3)):
    case (a, (b, c)):
        print(a, b, c)

Color = type("Color", (), {"RED": 1})
match 1:
    case Color.RED:
        print("red")
try:
    match 3:
        case Point(1, 2, 3):
            pass
except TypeError as e:
    print(e)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"int -> small
int -> small
int -> int
int -> big 500
str -> str hi
list -> empty
tuple -> empty
list -> one 9
list -> 12 then [3, 4]
list -> 7 [8, 9] 10
dict -> k1 {'z': 2}
dict -> a 3
Point -> origin
Point -> pt 1,2
Box -> box 2x3
NoneType -> none
float -> float 2.5
set -> other
list -> 12 then []
going north
1 2 3
red
"##
    );
}

#[test]
fn decimal_fractions_statistics_pprint_match_cpython() {
    let src = r##"
from decimal import Decimal, getcontext, localcontext, ROUND_HALF_UP, ROUND_DOWN, InvalidOperation, DivisionByZero, Context, ROUND_HALF_EVEN
from fractions import Fraction
import statistics, pprint, math

getcontext().prec = 28
print(Decimal("0.1") + Decimal("0.2"), Decimal(1) / Decimal(3), Decimal("1.10") * 3, Decimal("2.5").quantize(Decimal("1"), rounding=ROUND_HALF_UP), Decimal("2.5").quantize(Decimal("1")))
print(Decimal("10.456").quantize(Decimal("0.01")), Decimal("-3.7") // 2, Decimal("7") % 3, Decimal("1e3"), Decimal("1E+3").normalize(), Decimal(0.5), Decimal("NaN"), Decimal("-Infinity"))
print(repr(Decimal("3.14")), str(Decimal("100")), float(Decimal("2.5")), int(Decimal("9.99")), round(Decimal("2.675"), 2), Decimal("1.5") == Decimal("1.50"), Decimal("1.5") < 2, hash(Decimal("1.5")) == hash(1.5))
print(Decimal("123.456").as_tuple(), Decimal("2").sqrt(), Decimal("100").ln(), Decimal(1).exp(), Decimal("2") ** 10, abs(Decimal("-1.5")), -Decimal("1.5"), Decimal("1.5").to_integral_value(), sum([Decimal("0.1")] * 10))
with localcontext() as ctx:
    ctx.prec = 5
    print(Decimal(1) / Decimal(7), Decimal("123456789") * 1)
try:
    Decimal("abc")
except InvalidOperation as e:
    print(type(e).__name__)
try:
    Decimal(1) / Decimal(0)
except DivisionByZero as e:
    print(type(e).__name__)
print(f"{Decimal('1234.5678'):,.2f}", format(Decimal("0.000001234"), "e"), "%.3f" % Decimal("2.0005"))

f = Fraction(3, 4)
print(f, repr(f), f + Fraction(1, 4), f * 2, f / 3, f ** 2, Fraction("2/6"), Fraction(0.75), Fraction(1.5).limit_denominator(2), float(f), f.numerator, f.denominator, Fraction(7, 3).__floor__(), round(Fraction(7, 2)), f < 1, f == 0.75, hash(Fraction(1, 2)) == hash(0.5), Fraction(10 ** 30, 3))
print(Fraction("1.25"), Fraction(-3, 6), Fraction(5) - Fraction(1, 3), math.floor(Fraction(7, 2)), Fraction(1, 3) + 1, 1 / Fraction(3), divmod(Fraction(7, 2), 1), Fraction(1, 3).as_integer_ratio())

data = [2, 3, 5, 7, 7, 11, 13]
print(statistics.mean(data), statistics.median(data), statistics.mode(data), statistics.pstdev(data), statistics.stdev(data), statistics.variance(data), statistics.pvariance(data), statistics.median_low(data), statistics.median_high([1, 2, 3, 4]), statistics.harmonic_mean([1, 2, 4]), statistics.fmean(data), statistics.geometric_mean([1, 4, 16]))
print(statistics.mean([Fraction(1, 2), Fraction(3, 4)]), statistics.mean([Decimal("1.5"), Decimal("2.5")]), statistics.quantiles(data, n=4), statistics.multimode([1, 1, 2, 2, 3]), statistics.correlation([1, 2, 3, 4], [2, 4, 5, 9]), statistics.linear_regression([1, 2, 3], [2, 4, 7]))
try:
    statistics.mean([])
except statistics.StatisticsError as e:
    print(e)
nd = statistics.NormalDist(10, 2)
print(nd.mean, nd.stdev, round(nd.pdf(10), 6), round(nd.cdf(12), 6), round(nd.inv_cdf(0.9), 6))

pprint.pprint({"alpha": list(range(30)), "beta": {"x": "a" * 40, "y": [1, 2, {"z": (1, 2, 3)}]}, "gamma": "g" * 70})
print(pprint.pformat([1, 2, [3, 4]], width=10), pprint.pformat("x" * 5), pprint.isreadable({"a": 1}), pprint.saferepr([1, "a"]))
pprint.pprint(list(range(100)), compact=True, width=60)
pprint.pprint({"b": 1, "a": 2}, sort_dicts=False)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"0.3 0.3333333333333333333333333333 3.30 3 2
10.46 -1 1 1E+3 1E+3 0.5 NaN -Infinity
Decimal('3.14') 100 2.5 9 2.68 True True True
DecimalTuple(sign=0, digits=(1, 2, 3, 4, 5, 6), exponent=-3) 1.414213562373095048801688724 4.605170185988091368035982909 2.718281828459045235360287471 1024 1.5 -1.5 2 1.0
0.14286 1.2346E+8
InvalidOperation
DivisionByZero
1,234.57 1.234e-6 2.001
3/4 Fraction(3, 4) 1 3/2 1/4 9/16 1/3 3/4 3/2 0.75 3 4 2 4 True True True 1000000000000000000000000000000/3
5/4 -1/2 14/3 3 4/3 1/3 (3, Fraction(1, 2)) (1, 3)
6.857142857142857 7 7 3.719776161797582 4.0178174601214955 16.142857142857142 13.83673469387755 7 3 1.7142857142857142 6.857142857142857 4.0
5/8 2 [3.0, 7.0, 11.0] [1, 2] 0.9647638212377322 LinearRegression(slope=2.5, intercept=-0.666666666666667)
mean requires at least one data point
10.0 2.0 0.199471 0.841345 12.563103
{'alpha': [0,
           1,
           2,
           3,
           4,
           5,
           6,
           7,
           8,
           9,
           10,
           11,
           12,
           13,
           14,
           15,
           16,
           17,
           18,
           19,
           20,
           21,
           22,
           23,
           24,
           25,
           26,
           27,
           28,
           29],
 'beta': {'x': 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          'y': [1, 2, {'z': (1, 2, 3)}]},
 'gamma': 'gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg'}
[1,
 2,
 [3, 4]] 'xxxxx' True [1, 'a']
[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46,
 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61,
 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76,
 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91,
 92, 93, 94, 95, 96, 97, 98, 99]
{'b': 1, 'a': 2}
"##
    );
}

#[test]
fn bytearray_memoryview_match_cpython() {
    let src = r##"
import struct, hashlib, base64, binascii, io

ba = bytearray(b"hello")
ba.append(33)
ba.extend(b" world")
ba[0] = 72
ba += b"!!"
print(ba, len(ba), bytes(ba), ba[1:3], ba.decode(), type(ba).__name__, isinstance(ba, bytes))
ba.insert(0, 62)
print(ba.pop(), ba.pop(0), ba.index(b"o"), ba.find(b"w"), ba.upper(), ba.hex())
del ba[0:2]
print(ba, ba == b"llo world!", bytearray(3), bytearray([1, 2, 3]), bytearray("é", "utf-8"))
ba.reverse()
print(ba, ba.count(b"l"), ba.startswith(b"!"), ba.replace(b"l", b"L"), ba.split(b"o"))
ba.clear()
print(ba, bool(ba))

buf = bytearray(8)
struct.pack_into("<I", buf, 0, 0xDEADBEEF)
struct.pack_into(">H", buf, 4, 513)
print(buf, struct.unpack_from("<I", buf, 0), struct.unpack_from(">H", buf, 4), struct.calcsize("<IH"))

mv = memoryview(b"abcdef")
print(mv[1], bytes(mv[1:3]), bytes(mv[2:4]), len(mv), mv.tobytes(), list(mv[:3]), mv.nbytes, mv.readonly)
mb = memoryview(bytearray(b"abcdef"))
mb[0] = 65
mb[1:3] = b"XY"
print(bytes(mb), mb.tolist(), mb.readonly)

h = hashlib.sha256()
h.update(bytearray(b"abc"))
print(h.hexdigest()[:16], base64.b64encode(bytearray(b"hi")), binascii.hexlify(bytearray(b"hi")))
b = io.BytesIO()
b.write(bytearray(b"xyz"))
print(b.getvalue(), b.getbuffer().nbytes)
for x in bytearray(b"ab"):
    print(x, end=" ")
print()
print(bytes(bytearray(b"ab")) + b"c", bytearray(b"ab") + b"c", b"c" + bytearray(b"ab"), bytearray(b"ab") * 2)
print(bytearray(b"abc") < bytearray(b"abd"), sorted(bytearray(b"cab")), max(bytearray(b"cab")))
print(bytearray.fromhex("4142"), bytearray(b"a").join([b"x", b"y"]), bytearray(b" a ").strip(), bytearray(b"ab").zfill(5) if hasattr(bytearray, "zfill") else "")
try:
    ba2 = bytearray(b"a")
    ba2[0] = 300
except ValueError as e:
    print(e)
try:
    hash(bytearray(b"a"))
except TypeError as e:
    print(e)
d = {}
try:
    d[bytearray(b"a")] = 1
except TypeError as e:
    print(e)
print(repr(bytearray(b"a\x00\xff")), str(bytearray(b"ab")), bytearray(b"ab").__class__)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"bytearray(b'Hello! world!!') 14 b'Hello! world!!' bytearray(b'el') Hello! world!! bytearray False
33 62 4 7 bytearray(b'HELLO! WORLD!') 48656c6c6f2120776f726c6421
bytearray(b'llo! world!') False bytearray(b'\x00\x00\x00') bytearray(b'\x01\x02\x03') bytearray(b'\xc3\xa9')
bytearray(b'!dlrow !oll') 3 True bytearray(b'!dLrow !oLL') [bytearray(b'!dlr'), bytearray(b'w !'), bytearray(b'll')]
bytearray(b'') False
bytearray(b'\xef\xbe\xad\xde\x02\x01\x00\x00') (3735928559,) (513,) 6
98 b'bc' b'cd' 6 b'abcdef' [97, 98, 99] 6 True
b'AXYdef' [65, 88, 89, 100, 101, 102] False
ba7816bf8f01cfea b'aGk=' b'6869'
b'xyz' 3
97 98 
b'abc' bytearray(b'abc') b'cab' bytearray(b'abab')
True [97, 98, 99] 99
bytearray(b'AB') bytearray(b'xay') bytearray(b'a') bytearray(b'000ab')
byte must be in range(0, 256)
unhashable type: 'bytearray'
unhashable type: 'bytearray'
bytearray(b'a\x00\xff') bytearray(b'ab') <class 'bytearray'>
"##
    );
}

#[test]
fn docstrings_csv_unittest_signal_match_cpython() {
    let src = r##"
"""Doc do módulo."""
import csv, io, unittest, signal


class A:
    """Classe A."""
    def m(self):
        """método m"""


class B(A): pass


print(__doc__, A.__doc__, B.__doc__, A().m.__doc__, A.m.__doc__, (lambda: 1).__doc__)
buf = io.StringIO()
w = csv.DictWriter(buf, fieldnames=["a", "b"], lineterminator="\n")
w.writeheader()
w.writerow({"a": 1, "b": "x,y"})
print(repr(buf.getvalue()), list(csv.DictReader(io.StringIO(buf.getvalue()))))
print(csv.list_dialects() == ['excel', 'excel-tab', 'unix'], csv.Sniffer().sniff("a;b\n1;2\n").delimiter)


class T(unittest.TestCase):
    def test_a(self):
        self.assertEqual(1 + 1, 2)
        with self.assertRaises(ZeroDivisionError):
            1 / 0

    def test_b(self):
        self.assertEqual([1, 2], [1, 2])

    @unittest.skip("não")
    def test_c(self):
        pass


s = io.StringIO()
r = unittest.TextTestRunner(stream=s, verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(T))
print(r.testsRun, len(r.failures), len(r.skipped), r.wasSuccessful())
print("\n".join(l for l in s.getvalue().splitlines() if not l.startswith(("Ran ", "File ", "  File")) and "~~" not in l and "^^" not in l and "assertEqual" not in l))
print(signal.SIGINT, int(signal.SIGTERM), signal.getsignal(signal.SIGINT).__name__)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"Doc do módulo. Classe A. None método m método m None
'a,b\n1,"x,y"\n' [{'a': '1', 'b': 'x,y'}]
True ;
3 0 1 True
test_a (__main__.T.test_a) ... ok
test_b (__main__.T.test_b) ... ok
test_c (__main__.T.test_c) ... skipped 'não'

----------------------------------------------------------------------

OK (skipped=1)
2 15 default_int_handler
"##
    );
}

#[test]
fn exec_eval_namespaces_match_cpython() {
    let src = r##"
ns = {"a": 2}
exec("b = a * 3\ndef f(x): return x + b\nclass K: v = 1", ns)
print(sorted(k for k in ns if not k.startswith("__")), ns["b"], ns["f"](1), ns["K"].v)
print(eval("a + b", ns), eval("[i * a for i in range(3)]", {"a": 5}))
loc = {}
exec("q = 7\nr = q + 1", {"z": 1}, loc)
print(loc)
g = {"base": 10}
print(eval("base + t", g, {"t": 5}))
try:
    exec("raise ValueError('boom')", {})
except ValueError as e:
    print("ve", e)
d = {"n": 1}
try:
    exec("n = 2\n1/0", d)
except ZeroDivisionError:
    print(d["n"])
exec("del n", d)
print("n" in d)
try:
    eval("1 +", {})
except SyntaxError as e:
    print("syntax")
def outer():
    exec("w = 1")
    return "ok"
print(outer())
x = 10
print(eval("x * 2"))
code = "total = sum(range(n))"
env = {"n": 5}
exec(code, env)
print(env["total"])
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"['K', 'a', 'b', 'f'] 6 7 1
8 [0, 5, 10]
{'q': 7, 'r': 8}
15
ve boom
2
False
syntax
ok
20
10
"##
    );
}

#[test]
fn module_type_and_sys_modules_match_cpython() {
    let src = r##"
import types, sys
m = types.ModuleType("m", "doc!")
print(m, m.__name__, m.__doc__, type(m) is type(sys), types.ModuleType.__name__)
m.x = 5
print(m.x, "x" in vars(m), sys.modules.get("m"))
sys.modules["m"] = m
import m as m2
print(m2 is m, m2.x)
exec("y = x + 1\ndef f(): return y * 2", m.__dict__)
print(m.y, m.f())
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"<module 'm'> m doc! True module
5 True None
True 5
6 12
"##
    );
}

#[test]
fn coroutines_async_generators_delegation_match_cpython() {
    let src = r##"
class Fut:
    def __init__(self): self.v = None
    def __await__(self):
        got = yield self
        return got

async def leaf(x):
    r = await Fut()
    return x + r

async def mid():
    a = await leaf(1)
    b = await leaf(10)
    return a + b

c = mid()
print(type(c).__name__)
f = c.send(None)
print(type(f).__name__)
f = c.send(100)
print(type(f).__name__)
try:
    c.send(1000)
except StopIteration as e:
    print("ret", e.value)

def sub():
    x = yield 1
    y = yield x * 2
    return x + y

def outer():
    r = yield from sub()
    print("sub returned", r)
    yield r * 10

g = outer()
print(next(g), g.send(5), g.send(7), list(g))

def thrower():
    try:
        yield 1
    except ValueError as e:
        print("caught in sub", e)
        yield 99
    return "end"

def deleg():
    r = yield from thrower()
    yield r

g = deleg()
print(next(g), g.throw(ValueError("boom")), next(g))

async def raiser():
    try:
        await Fut()
    except KeyError as e:
        print("coro caught", e)
        return "handled"

c = raiser()
c.send(None)
try:
    c.throw(KeyError("k"))
except StopIteration as e:
    print(e.value)

async def agen():
    for i in range(3):
        await Fut()
        yield i

async def consume():
    out = []
    async for v in agen():
        out.append(v)
    return out

c = consume()
c.send(None)
c.send(None)
c.send(None)
try:
    c.send(None)
except StopIteration as e:
    print(e.value)

class CM:
    async def __aenter__(self):
        print("aenter"); return 7
    async def __aexit__(self, t, v, tb):
        print("aexit", t.__name__ if t else None); return True

async def uses_cm():
    async with CM() as v:
        print("body", v)
        raise ValueError("x")
    async with CM() as v:
        return v

c = uses_cm()
try:
    c.send(None)
except StopIteration as e:
    print("cm ret", e.value)

async def not_started():
    return 1
c = not_started()
print(repr(c)[:28])
c.close()
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"coroutine
Fut
Fut
ret 1111
sub returned 12
1 10 120 []
caught in sub boom
1 99 end
coro caught 'k'
handled
[0, 1, 2]
aenter
body 7
aexit ValueError
aenter
aexit None
cm ret 7
<coroutine object not_starte
"##
    );
}

#[test]
fn exception_groups_and_except_star_match_cpython() {
    let src = r##"
try:
    raise ExceptionGroup("g", [ValueError(1), TypeError(2), KeyError(3)])
except* ValueError as e:
    print("V", repr(e))
except* (TypeError, KeyError) as e:
    print("TK", [type(x).__name__ for x in e.exceptions])
try:
    try:
        raise ExceptionGroup("g", [ValueError(1), OSError(2)])
    except* ValueError:
        print("v")
except ExceptionGroup as e:
    print("left", repr(e))
try:
    raise ValueError("naked")
except* ValueError as e:
    print(type(e).__name__, e.exceptions)
eg = ExceptionGroup("m", [ValueError(1), ExceptionGroup("n", [TypeError(2), ValueError(3)])])
m, r = eg.split(ValueError)
print(repr(m), repr(r), str(eg), eg.message)
print(isinstance(eg, Exception), isinstance(BaseExceptionGroup("b", [KeyboardInterrupt()]), Exception), type(BaseExceptionGroup("b", [ValueError()])).__name__)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"V ExceptionGroup('g', [ValueError(1)])
TK ['TypeError', 'KeyError']
v
left ExceptionGroup('g', [OSError(2)])
ExceptionGroup (ValueError('naked'),)
ExceptionGroup('m', [ValueError(1), ExceptionGroup('n', [ValueError(3)])]) ExceptionGroup('m', [ExceptionGroup('n', [TypeError(2)])]) m (2 sub-exceptions) m
True False ExceptionGroup
"##
    );
}

#[test]
fn array_module() {
    let src = r##"
import array
a = array.array('i', [1, 2, 3]); a.append(4); a.extend([5])
print(a, len(a), a[1], a.tolist(), a.typecode, a.itemsize, a.tobytes())
b = array.array('d', [1.5, 2.5]); print(b, sum(b), b[::-1], b + b)
c = array.array('B', b'abc'); print(c, bytes(c)); c.frombytes(b'd'); print(c)
a.insert(0, 9); a.pop(); a.reverse(); print(a, a.index(9), a.count(1), 3 in a)
for i, x in enumerate(array.array('H', range(3))): print(i, x)
print(array.array('i', [1,2]) == array.array('i', [1,2]), array.typecodes)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"array('i', [1, 2, 3, 4, 5]) 5 2 [1, 2, 3, 4, 5] i 4 b'\x01\x00\x00\x00\x02\x00\x00\x00\x03\x00\x00\x00\x04\x00\x00\x00\x05\x00\x00\x00'
array('d', [1.5, 2.5]) 4.0 array('d', [2.5, 1.5]) array('d', [1.5, 2.5, 1.5, 2.5])
array('B', [97, 98, 99]) b'abc'
array('B', [97, 98, 99, 100])
array('i', [4, 3, 2, 1, 9]) 4 1 True
0 0
1 1
2 2
True bBuwhHiIlLqQfd
"##
    );
}

#[test]
fn unicodedata_module() {
    let src = r##"
import unicodedata as u
print(u.normalize('NFD', 'café'), len(u.normalize('NFD', 'é')), u.normalize('NFC', 'é') == 'é')
print(u.normalize('NFKD', 'ﬁ½'), ''.join(c for c in u.normalize('NFKD', 'João Ação') if not u.combining(c)))
print(u.category('a'), u.category('A'), u.category('1'), u.category(' '), u.category('é'), u.category('€'), u.category('́'))
print(u.name('a'), u.name('é'), u.name('€'), u.lookup('GREEK SMALL LETTER ALPHA'), u.name('\x00', 'none'))
print(u.decimal('7'), u.digit('٣'), u.numeric('½'), u.numeric('Ⅷ'), u.decimal('x', -1))
print(u.is_normalized('NFC', 'é'), u.combining('́'), u.combining('a'))
try: u.lookup('NOT A NAME')
except KeyError as e: print('KeyError', e)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"café 2 True
fi1⁄2 Joao Acao
Ll Lu Nd Zs Ll Sc Mn
LATIN SMALL LETTER A LATIN SMALL LETTER E WITH ACUTE EURO SIGN α none
7 3 0.5 8.0 -1
True 230 0
KeyError "undefined character name 'NOT A NAME'"
"##
    );
}

#[test]
fn xml_minidom_sax() {
    let src = r##"
from xml.dom import minidom
import xml.sax, io
from xml.sax.saxutils import escape, quoteattr
doc = minidom.parseString('<root a="1"><item id="x">hello &amp; bye</item><item id="y"/><!-- c --></root>')
r = doc.documentElement
print(r.tagName, r.getAttribute('a'), [i.getAttribute('id') for i in r.getElementsByTagName('item')])
print(r.getElementsByTagName('item')[0].firstChild.data, r.childNodes.length)
n = doc.createElement('new'); n.setAttribute('k', 'v<'); n.appendChild(doc.createTextNode('t&')); r.appendChild(n)
print(doc.toxml())
print(doc.toprettyxml(indent='  '))
class H(xml.sax.ContentHandler):
    def startElement(self, name, attrs): print('start', name, dict(attrs))
    def characters(self, c): print('chars', repr(c))
    def endElement(self, name): print('end', name)
xml.sax.parseString(b'<a x="1"><b>t</b></a>', H())
print(escape('<&>'), quoteattr('a"b'))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"root 1 ['x', 'y']
hello & bye 3
<?xml version="1.0" ?><root a="1"><item id="x">hello &amp; bye</item><item id="y"/><!-- c --><new k="v&lt;">t&amp;</new></root>
<?xml version="1.0" ?>
<root a="1">
  <item id="x">hello &amp; bye</item>
  <item id="y"/>
  <!-- c -->
  <new k="v&lt;">t&amp;</new>
</root>

start a {'x': '1'}
start b {}
chars 't'
end b
end a
&lt;&amp;&gt; 'a"b'
"##
    );
}

#[test]
fn sqlite3_dbapi() {
    let src = r##"
import sqlite3
db = sqlite3.connect(':memory:')
db.execute('create table t(id integer primary key, name text, score real, data blob)')
db.executemany('insert into t(name, score, data) values (?, ?, ?)', [('a', 1.5, b'x'), ('b', 2.5, None), ('c', None, b'zz')])
print(db.execute('select * from t').fetchall())
cur = db.execute('select name, score from t where score > :m order by id', {'m': 1})
print(cur.description, cur.fetchone(), cur.fetchmany(5), cur.rowcount)
c = db.cursor(); c.execute('insert into t(name) values (?)', ('d',)); print(c.lastrowid, c.rowcount, db.total_changes, db.in_transaction)
db.commit(); print(db.in_transaction)
db.row_factory = sqlite3.Row
r = db.execute('select id, name from t where name = ?', ('a',)).fetchone()
print(r['name'], r[0], r.keys(), len(r), tuple(r))
db.row_factory = None
try: db.execute('insert into t(id, name) values (1, "dup")')
except sqlite3.IntegrityError as e: print('IntegrityError', e)
try: db.execute('selec 1')
except sqlite3.OperationalError as e: print('OperationalError', e)
try: db.execute('select ?', (1, 2))
except sqlite3.ProgrammingError as e: print('ProgrammingError', e)
with db: db.execute("update t set score = 9 where name = 'a'")
print(db.execute('select count(*), sum(score) from t').fetchone())
db.create_function('twice', 1, lambda x: x * 2)
print(db.execute('select twice(21), twice(name) from t limit 1').fetchone())
class Cat:
    def __init__(self): self.n = 0
    def step(self, v): self.n += len(v or '')
    def finalize(self): return self.n
db.create_aggregate('totlen', 1, Cat); print(db.execute('select totlen(name) from t').fetchone())
db.create_collation('rev', lambda a, b: (a < b) - (a > b)); print(db.execute('select name from t order by name collate rev').fetchall())
print('\n'.join(db.iterdump()))
db.close()
try: db.execute('select 1')
except sqlite3.ProgrammingError as e: print(e)
print(sqlite3.complete_statement('select 1;'), sqlite3.complete_statement('select 1'))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"[(1, 'a', 1.5, b'x'), (2, 'b', 2.5, None), (3, 'c', None, b'zz')]
(('name', None, None, None, None, None, None), ('score', None, None, None, None, None, None)) ('a', 1.5) [('b', 2.5)] -1
4 1 4 True
False
a 1 ['id', 'name'] 2 (1, 'a')
IntegrityError UNIQUE constraint failed: t.id
OperationalError near "selec": syntax error
ProgrammingError Incorrect number of bindings supplied. The current statement uses 1, and there are 2 supplied.
(4, 11.5)
(42, 'aa')
(4,)
[('d',), ('c',), ('b',), ('a',)]
BEGIN TRANSACTION;
CREATE TABLE t(id integer primary key, name text, score real, data blob);
INSERT INTO "t" VALUES(1,'a',9.0,X'78');
INSERT INTO "t" VALUES(2,'b',2.5,NULL);
INSERT INTO "t" VALUES(3,'c',NULL,X'7A7A');
INSERT INTO "t" VALUES(4,'d',NULL,NULL);
COMMIT;
Cannot operate on a closed database.
True False
"##
    );
}

#[test]
fn keyerror_subclass_str() {
    let src = r##"
class E(KeyError): pass
print(str(KeyError('a')), str(E('a')), repr(E('a')), str(E('a', 'b')), str(E()))
try: {}['x']
except KeyError as e: print(e)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"'a' 'a' E('a') ('a', 'b') 
'x'
"##
    );
}

#[test]
fn ast_module() {
    let src = r##"
import ast
src = '''
"""mod doc"""
import os, sys as s
from a.b import c as d, e

@deco(1)
def f(a, /, b: int = 2, *args, k=None, **kw) -> str:
    """fdoc"""
    x = [i * 2 for i in range(a) if i % 2]
    y = {k: v for k, v in kw.items()}
    with open(b) as fh, other():
        pass
    try:
        raise ValueError("bad") from None
    except (KeyError, ValueError) as exc:
        return f"{a!r:>10} {b}"
    finally:
        del x
    lam = lambda q, *r: q if r else -q
    async def g():
        await h()
    match a:
        case [1, 2, *rest]: pass
        case {"k": v, **kw2}: pass
        case Point(x=0) | None: pass
        case _: pass
    return a < b <= 3 and not c or d[1:2, ::3]

class C(Base, metaclass=M):
    z: int = 5
    def m(self): return self.z ** 2 @ other
'''
t = ast.parse(src)
print(ast.get_docstring(t), ast.get_docstring(t.body[3]))
f = t.body[3]
print(f.name, f.lineno, f.end_lineno, f.col_offset, [a.arg for a in f.args.args], f.args.vararg.arg, f.returns.id)
print(ast.dump(f.body[1], indent=1))
class V(ast.NodeVisitor):
    def __init__(self): self.names = []
    def visit_Name(self, n): self.names.append(n.id)
    def visit_Call(self, n): self.generic_visit(n)
v = V(); v.visit(t); print(v.names)
class T(ast.NodeTransformer):
    def visit_Constant(self, n):
        if isinstance(n.value, int): return ast.copy_location(ast.Constant(n.value + 100), n)
        return n
t2 = ast.fix_missing_locations(T().visit(ast.parse("a = 1 + 2\nb = 'x'")))
print(ast.unparse(t2))
print(ast.unparse(t))
tree = ast.parse("def sq(n):\n    return n * n\nresult = sq(7)")
ns = {}
exec(compile(tree, '<ast>', 'exec'), ns); print(ns['result'])
e = ast.Expression(ast.BinOp(ast.Constant(6), ast.Mult(), ast.Constant(7)))
print(eval(compile(ast.fix_missing_locations(e), '<e>', 'eval')))
print(ast.literal_eval('[1, -2.5, 3j, "a" "b", (1,), {}, set(), None, ...]') if hasattr(ast, 'literal_eval') else '')
print([type(n).__name__ for n in ast.iter_child_nodes(ast.parse('a.b(c)').body[0].value)])
try: ast.parse('def (:')
except SyntaxError as e: print('SyntaxError', e.msg, e.lineno)
print(ast.Constant.__match_args__, ast.Name._fields, ast.Constant(5).value, ast.Return().value)
print(ast.dump(ast.parse('x: int = 1; y += 2; print(*a, **k)'), annotate_fields=False))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"mod doc fdoc
f 7 27 0 ['b'] args str
Assign(
 targets=[
  Name(id='x', ctx=Store())],
 value=ListComp(
  elt=BinOp(
   left=Name(id='i', ctx=Load()),
   op=Mult(),
   right=Constant(value=2)),
  generators=[
   comprehension(
    target=Name(id='i', ctx=Store()),
    iter=Call(
     func=Name(id='range', ctx=Load()),
     args=[
      Name(id='a', ctx=Load())]),
    ifs=[
     BinOp(
      left=Name(id='i', ctx=Load()),
      op=Mod(),
      right=Constant(value=2))],
    is_async=0)]))
['int', 'x', 'i', 'i', 'range', 'a', 'i', 'y', 'k', 'v', 'k', 'v', 'kw', 'open', 'b', 'fh', 'other', 'ValueError', 'KeyError', 'ValueError', 'a', 'b', 'x', 'lam', 'r', 'q', 'q', 'h', 'a', 'Point', 'a', 'b', 'c', 'd', 'deco', 'str', 'Base', 'M', 'z', 'int', 'self', 'other']
a = 101 + 102
b = 'x'
"""mod doc"""
import os, sys as s
from a.b import c as d, e

@deco(1)
def f(a, /, b: int=2, *args, k=None, **kw) -> str:
    """fdoc"""
    x = [i * 2 for i in range(a) if i % 2]
    y = {k: v for k, v in kw.items()}
    with open(b) as fh, other():
        pass
    try:
        raise ValueError('bad') from None
    except (KeyError, ValueError) as exc:
        return f'{a!r:>10} {b}'
    finally:
        del x
    lam = lambda q, *r: q if r else -q

    async def g():
        await h()
    match a:
        case [1, 2, *rest]:
            pass
        case {'k': v, **kw2}:
            pass
        case Point(x=0) | None:
            pass
        case _:
            pass
    return a < b <= 3 and (not c) or d[1:2, ::3]

class C(Base, metaclass=M):
    z: int = 5

    def m(self):
        return self.z ** 2 @ other
49
42
[1, -2.5, 3j, 'ab', (1,), {}, set(), None, Ellipsis]
['Attribute', 'Name']
SyntaxError invalid syntax 1
('value', 'kind') ('id', 'ctx') 5 None
Module([AnnAssign(Name('x', Store()), Name('int', Load()), Constant(1), 1), AugAssign(Name('y', Store()), Add(), Constant(2)), Expr(Call(Name('print', Load()), [Starred(Name('a', Load()), Load())], [keyword(value=Name('k', Load()))]))])
"##
    );
}

#[test]
fn enum_simple_enum_auto() {
    let src = r##"
from enum import IntEnum, auto, _simple_enum, Enum
@_simple_enum(IntEnum)
class P:
    """doc"""
    A = auto()
    B = auto()
print(type(P), P.A, P.B, P.A < P.B, int(P.B), list(P))
class Q(IntEnum):
    A = auto()
    B = auto()
print(Q.A < Q.B, repr(Q.B))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"<class 'enum.EnumType'> 1 2 True 2 [<P.A: 1>, <P.B: 2>]
True <Q.B: 2>
"##
    );
}

#[test]
fn text_codecs() {
    let src = r##"
import io, codecs
for enc in ('cp1252', 'iso-8859-1', 'utf-16', 'utf-8-sig', 'utf-16-be', 'utf-32-le', 'cp437', 'koi8-r'):
    for err in ('strict', 'ignore', 'replace', 'backslashreplace', 'xmlcharrefreplace', 'namereplace'):
        try: print(enc, err, 'ação €→日'.encode(enc, err))
        except Exception as e: print(enc, err, type(e).__name__, e)
for enc, data in [('cp1252', b'a\x81\x80z'), ('utf-16-le', b'a\x00b'), ('utf-16', b'\xff\xfea\x00\x00\xd8'), ('utf-16-le', b'\x00\xdc'), ('utf-32', b'abc'), ('utf-8-sig', b'\xef\xbb\xbfhi'), ('iso8859-2', b'\xa1\xb1'), ('unicode_escape', b'a\\n\\x41\\u20ac\\N{BULLET}\\101'), ('raw_unicode_escape', b'a\\u20ac\\n'), ('unicode_escape', b'\\xZ'), ('cp1252', b'\x9f\x8e')]:
    for err in ('strict', 'replace', 'ignore', 'backslashreplace'):
        try: print(enc, err, repr(data.decode(enc, err)))
        except Exception as e: print(enc, err, type(e).__name__, e)
print(codecs.encode('é', 'cp1252'), codecs.decode(b'\xe9', 'latin-1'), codecs.lookup('Windows-1252').name, codecs.lookup('latin1').name)
print(io.TextIOWrapper(io.BytesIO('ñ'.encode('cp850')), encoding='cp850').read())
print(str(b'\xe9', 'cp1252'), bytes('é', 'cp1252'), 'é'.encode('cp1252').decode('mac_roman'))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"cp1252 strict UnicodeEncodeError 'charmap' codec can't encode characters in position 6-7: character maps to <undefined>
cp1252 ignore b'a\xe7\xe3o \x80'
cp1252 replace b'a\xe7\xe3o \x80??'
cp1252 backslashreplace b'a\xe7\xe3o \x80\\u2192\\u65e5'
cp1252 xmlcharrefreplace b'a\xe7\xe3o \x80&#8594;&#26085;'
cp1252 namereplace b'a\xe7\xe3o \x80\\N{RIGHTWARDS ARROW}\\N{CJK UNIFIED IDEOGRAPH-65E5}'
iso-8859-1 strict UnicodeEncodeError 'latin-1' codec can't encode characters in position 5-7: ordinal not in range(256)
iso-8859-1 ignore b'a\xe7\xe3o '
iso-8859-1 replace b'a\xe7\xe3o ???'
iso-8859-1 backslashreplace b'a\xe7\xe3o \\u20ac\\u2192\\u65e5'
iso-8859-1 xmlcharrefreplace b'a\xe7\xe3o &#8364;&#8594;&#26085;'
iso-8859-1 namereplace b'a\xe7\xe3o \\N{EURO SIGN}\\N{RIGHTWARDS ARROW}\\N{CJK UNIFIED IDEOGRAPH-65E5}'
utf-16 strict b'\xff\xfea\x00\xe7\x00\xe3\x00o\x00 \x00\xac \x92!\xe5e'
utf-16 ignore b'\xff\xfea\x00\xe7\x00\xe3\x00o\x00 \x00\xac \x92!\xe5e'
utf-16 replace b'\xff\xfea\x00\xe7\x00\xe3\x00o\x00 \x00\xac \x92!\xe5e'
utf-16 backslashreplace b'\xff\xfea\x00\xe7\x00\xe3\x00o\x00 \x00\xac \x92!\xe5e'
utf-16 xmlcharrefreplace b'\xff\xfea\x00\xe7\x00\xe3\x00o\x00 \x00\xac \x92!\xe5e'
utf-16 namereplace b'\xff\xfea\x00\xe7\x00\xe3\x00o\x00 \x00\xac \x92!\xe5e'
utf-8-sig strict b'\xef\xbb\xbfa\xc3\xa7\xc3\xa3o \xe2\x82\xac\xe2\x86\x92\xe6\x97\xa5'
utf-8-sig ignore b'\xef\xbb\xbfa\xc3\xa7\xc3\xa3o \xe2\x82\xac\xe2\x86\x92\xe6\x97\xa5'
utf-8-sig replace b'\xef\xbb\xbfa\xc3\xa7\xc3\xa3o \xe2\x82\xac\xe2\x86\x92\xe6\x97\xa5'
utf-8-sig backslashreplace b'\xef\xbb\xbfa\xc3\xa7\xc3\xa3o \xe2\x82\xac\xe2\x86\x92\xe6\x97\xa5'
utf-8-sig xmlcharrefreplace b'\xef\xbb\xbfa\xc3\xa7\xc3\xa3o \xe2\x82\xac\xe2\x86\x92\xe6\x97\xa5'
utf-8-sig namereplace b'\xef\xbb\xbfa\xc3\xa7\xc3\xa3o \xe2\x82\xac\xe2\x86\x92\xe6\x97\xa5'
utf-16-be strict b'\x00a\x00\xe7\x00\xe3\x00o\x00  \xac!\x92e\xe5'
utf-16-be ignore b'\x00a\x00\xe7\x00\xe3\x00o\x00  \xac!\x92e\xe5'
utf-16-be replace b'\x00a\x00\xe7\x00\xe3\x00o\x00  \xac!\x92e\xe5'
utf-16-be backslashreplace b'\x00a\x00\xe7\x00\xe3\x00o\x00  \xac!\x92e\xe5'
utf-16-be xmlcharrefreplace b'\x00a\x00\xe7\x00\xe3\x00o\x00  \xac!\x92e\xe5'
utf-16-be namereplace b'\x00a\x00\xe7\x00\xe3\x00o\x00  \xac!\x92e\xe5'
utf-32-le strict b'a\x00\x00\x00\xe7\x00\x00\x00\xe3\x00\x00\x00o\x00\x00\x00 \x00\x00\x00\xac \x00\x00\x92!\x00\x00\xe5e\x00\x00'
utf-32-le ignore b'a\x00\x00\x00\xe7\x00\x00\x00\xe3\x00\x00\x00o\x00\x00\x00 \x00\x00\x00\xac \x00\x00\x92!\x00\x00\xe5e\x00\x00'
utf-32-le replace b'a\x00\x00\x00\xe7\x00\x00\x00\xe3\x00\x00\x00o\x00\x00\x00 \x00\x00\x00\xac \x00\x00\x92!\x00\x00\xe5e\x00\x00'
utf-32-le backslashreplace b'a\x00\x00\x00\xe7\x00\x00\x00\xe3\x00\x00\x00o\x00\x00\x00 \x00\x00\x00\xac \x00\x00\x92!\x00\x00\xe5e\x00\x00'
utf-32-le xmlcharrefreplace b'a\x00\x00\x00\xe7\x00\x00\x00\xe3\x00\x00\x00o\x00\x00\x00 \x00\x00\x00\xac \x00\x00\x92!\x00\x00\xe5e\x00\x00'
utf-32-le namereplace b'a\x00\x00\x00\xe7\x00\x00\x00\xe3\x00\x00\x00o\x00\x00\x00 \x00\x00\x00\xac \x00\x00\x92!\x00\x00\xe5e\x00\x00'
cp437 strict UnicodeEncodeError 'charmap' codec can't encode character '\xe3' in position 2: character maps to <undefined>
cp437 ignore b'a\x87o '
cp437 replace b'a\x87?o ???'
cp437 backslashreplace b'a\x87\\xe3o \\u20ac\\u2192\\u65e5'
cp437 xmlcharrefreplace b'a\x87&#227;o &#8364;&#8594;&#26085;'
cp437 namereplace b'a\x87\\N{LATIN SMALL LETTER A WITH TILDE}o \\N{EURO SIGN}\\N{RIGHTWARDS ARROW}\\N{CJK UNIFIED IDEOGRAPH-65E5}'
koi8-r strict UnicodeEncodeError 'charmap' codec can't encode characters in position 1-2: character maps to <undefined>
koi8-r ignore b'ao '
koi8-r replace b'a??o ???'
koi8-r backslashreplace b'a\\xe7\\xe3o \\u20ac\\u2192\\u65e5'
koi8-r xmlcharrefreplace b'a&#231;&#227;o &#8364;&#8594;&#26085;'
koi8-r namereplace b'a\\N{LATIN SMALL LETTER C WITH CEDILLA}\\N{LATIN SMALL LETTER A WITH TILDE}o \\N{EURO SIGN}\\N{RIGHTWARDS ARROW}\\N{CJK UNIFIED IDEOGRAPH-65E5}'
cp1252 strict UnicodeDecodeError 'charmap' codec can't decode byte 0x81 in position 1: character maps to <undefined>
cp1252 replace 'a�€z'
cp1252 ignore 'a€z'
cp1252 backslashreplace 'a\\x81€z'
utf-16-le strict UnicodeDecodeError 'utf-16-le' codec can't decode byte 0x62 in position 2: truncated data
utf-16-le replace 'a�'
utf-16-le ignore 'a'
utf-16-le backslashreplace 'a\\x62'
utf-16 strict UnicodeDecodeError 'utf-16-le' codec can't decode bytes in position 4-5: unexpected end of data
utf-16 replace 'a�'
utf-16 ignore 'a'
utf-16 backslashreplace 'a\\x00\\xd8'
utf-16-le strict UnicodeDecodeError 'utf-16-le' codec can't decode bytes in position 0-1: illegal encoding
utf-16-le replace '�'
utf-16-le ignore ''
utf-16-le backslashreplace '\\x00\\xdc'
utf-32 strict UnicodeDecodeError 'utf-32-le' codec can't decode bytes in position 0-2: truncated data
utf-32 replace '�'
utf-32 ignore ''
utf-32 backslashreplace '\\x61\\x62\\x63'
utf-8-sig strict 'hi'
utf-8-sig replace 'hi'
utf-8-sig ignore 'hi'
utf-8-sig backslashreplace 'hi'
iso8859-2 strict 'Ąą'
iso8859-2 replace 'Ąą'
iso8859-2 ignore 'Ąą'
iso8859-2 backslashreplace 'Ąą'
unicode_escape strict 'a\nA€•A'
unicode_escape replace 'a\nA€•A'
unicode_escape ignore 'a\nA€•A'
unicode_escape backslashreplace 'a\nA€•A'
raw_unicode_escape strict 'a€\\n'
raw_unicode_escape replace 'a€\\n'
raw_unicode_escape ignore 'a€\\n'
raw_unicode_escape backslashreplace 'a€\\n'
unicode_escape strict UnicodeDecodeError 'unicodeescape' codec can't decode bytes in position 0-1: truncated \xXX escape
unicode_escape replace '�Z'
unicode_escape ignore 'Z'
unicode_escape backslashreplace '\\x5c\\x78Z'
cp1252 strict 'ŸŽ'
cp1252 replace 'ŸŽ'
cp1252 ignore 'ŸŽ'
cp1252 backslashreplace 'ŸŽ'
b'\xe9' é cp1252 iso8859-1
ñ
é b'\xe9' È
"##
    );
}

#[test]
fn stdlib_pickle_toml_ip_html() {
    let src = r##"
import pickle, copyreg, tomllib, ipaddress, mimetypes, fileinput
from html.parser import HTMLParser
data = {'a': [1, 2.5, (3, None)], 'b': {'x', 'y'}, 'c': b'zz', 'd': 'é', 'e': 10**30, 'f': True}
for proto in range(0, 6):
    s = pickle.dumps(data, protocol=proto)
    assert pickle.loads(s) == data, proto
print(len(pickle.dumps(data)), pickle.dumps([1, 'a']))
class P:
    def __init__(self, x): self.x = x
    def __eq__(self, o): return isinstance(o, P) and o.x == self.x
print(pickle.loads(pickle.dumps(P([1, 2]))).x)
import collections, datetime
print(pickle.loads(pickle.dumps(collections.OrderedDict(a=1))), pickle.loads(pickle.dumps(datetime.date(2024, 1, 2))))
print(tomllib.loads('''
title = "x"
[owner]
name = "Tom"
dob = 1979-05-27T07:32:00-08:00
[[items]]
a = 1
[[items]]
a = 2.5
tags = ["u", "v"]
'''))
n = ipaddress.ip_network('192.168.1.0/24'); print(n.num_addresses, ipaddress.ip_address('192.168.1.7') in n, list(n.hosts())[:2], ipaddress.ip_address('::1').is_loopback)
class H(HTMLParser):
    def handle_starttag(self, t, a): print('start', t, a)
    def handle_endtag(self, t): print('end', t)
    def handle_data(self, d): print('data', repr(d))
H().feed('<div class="a"><a href="/x">hi &amp; bye</a><br/></div>')
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"98 b'\x80\x04\x95\x0b\x00\x00\x00\x00\x00\x00\x00]\x94(K\x01\x8c\x01a\x94e.'
[1, 2]
OrderedDict({'a': 1}) 2024-01-02
{'title': 'x', 'owner': {'name': 'Tom', 'dob': datetime.datetime(1979, 5, 27, 7, 32, tzinfo=datetime.timezone(datetime.timedelta(days=-1, seconds=57600)))}, 'items': [{'a': 1}, {'a': 2.5, 'tags': ['u', 'v']}]}
256 True [IPv4Address('192.168.1.1'), IPv4Address('192.168.1.2')] True
start div [('class', 'a')]
start a [('href', '/x')]
data 'hi & bye'
end a
start br []
end br
end div
"##
    );
}

#[test]
fn pickle_protocols() {
    let src = r##"
import pickle, copy, io, dataclasses, collections, enum, fractions, decimal, datetime, functools, array
@dataclasses.dataclass
class Pt:
    x: int
    y: int = 2
    tags: list = dataclasses.field(default_factory=list)
NT = collections.namedtuple('NT', 'a b')
class Color(enum.Enum):
    RED = 1
    BLUE = 2
class Slotted:
    __slots__ = ('a', 'b')
    def __init__(self, a, b): self.a, self.b = a, b
class WithState:
    def __init__(self): self.v = 1; self.cache = {'big': 1}
    def __getstate__(self): return {'v': self.v}
    def __setstate__(self, s): self.v = s['v']; self.cache = {}
class Node:
    def __init__(self, name): self.name = name; self.next = None
objs = [Pt(1, tags=['a']), NT(1, 'z'), Color.BLUE, Slotted(1, 'q'), WithState(), fractions.Fraction(3, 7), decimal.Decimal('1.25'),
        datetime.datetime(2024, 5, 6, 7, 8, 9, 10), datetime.timedelta(days=2, seconds=3), datetime.timezone.utc,
        collections.Counter('abca'), collections.defaultdict(list, {'k': [1]}), collections.deque([1, 2, 3]),
        frozenset({1, 2}), bytearray(b'xy'), range(5), complex(1, 2), None, ..., 1.5e300, -2**70, 'üñí', (1, (2, [3]))]
for o in objs:
    for proto in (0, 1, 2, 3, 4, 5):
        try: r = pickle.loads(pickle.dumps(o, protocol=proto))
        except Exception as e: print('ERR', type(o).__name__, proto, type(e).__name__, str(e)[:80]); break
        if type(o).__name__ in ('Slotted', 'WithState'):
            ok = (r.a, r.b) == (o.a, o.b) if isinstance(o, Slotted) else (r.v, r.cache) == (1, {})
        else: ok = r == o
        if not ok: print('DIFF', type(o).__name__, proto, r)
print('roundtrip done')
a = Node('a'); b = Node('b'); a.next = b; b.next = a
c = pickle.loads(pickle.dumps(a)); print(c.name, c.next.name, c.next.next is c)
d = copy.deepcopy(a); print(d.next.next is d, d is not a)
e = copy.copy(Pt(1, 2, [3])); print(e)
buf = io.BytesIO(); pickle.dump({'x': [1, 2]}, buf); pickle.dump('second', buf); buf.seek(0)
print(pickle.load(buf), pickle.load(buf))
try: pickle.dumps(lambda x: x)
except Exception as ex: print(type(ex).__name__)
try: pickle.loads(b'garbage')
except Exception as ex: print(type(ex).__name__)
print(pickle.dumps(1), pickle.dumps('a', protocol=2), pickle.HIGHEST_PROTOCOL, pickle.DEFAULT_PROTOCOL)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"ERR Slotted 0 TypeError a class that defines __slots__ without defining __getstate__ cannot be pickled
roundtrip done
a b True
True True
Pt(x=1, y=2, tags=[3])
{'x': [1, 2]} second
PicklingError
UnpicklingError
b'\x80\x04K\x01.' b'\x80\x02X\x01\x00\x00\x00aq\x00.' 5 4
"##
    );
}

#[test]
fn quopri_plist_code() {
    let src = r##"
import quopri, plistlib, codeop, code
print(quopri.encodestring(b"caf\xc3\xa9 = ok\n"))
print(quopri.decodestring(b"caf=C3=A9\n"))
d = {"a": 1, "b": [1.5, "x", True], "c": b"zz"}
s = plistlib.dumps(d)
print(plistlib.loads(s) == d, plistlib.dumps(d, fmt=plistlib.FMT_BINARY)[:8])
print(plistlib.loads(plistlib.dumps(d, fmt=plistlib.FMT_BINARY)) == d)
c = code.InteractiveInterpreter()
c.runsource("x = 2 + 3")
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"b'caf=C3=A9 =3D ok\n'
b'caf\xc3\xa9\n'
True b'bplist00'
True
"##
    );
}

#[test]
fn tokenize_basic() {
    let src = r##"
import tokenize, io, token, keyword
src = "x = 1 + 2  # c\nif x:\n    print('a')\n"
for t in tokenize.generate_tokens(io.StringIO(src).readline):
    print(token.tok_name[t.type], repr(t.string), t.start)
print(keyword.iskeyword("if"), len(keyword.kwlist))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"NAME 'x' (1, 0)
OP '=' (1, 2)
NUMBER '1' (1, 4)
OP '+' (1, 6)
NUMBER '2' (1, 8)
COMMENT '# c' (1, 11)
NEWLINE '\n' (1, 14)
NAME 'if' (2, 0)
NAME 'x' (2, 3)
OP ':' (2, 4)
NEWLINE '\n' (2, 5)
INDENT '    ' (3, 0)
NAME 'print' (3, 4)
OP '(' (3, 9)
STRING "'a'" (3, 10)
OP ')' (3, 13)
NEWLINE '\n' (3, 14)
DEDENT '' (4, 0)
ENDMARKER '' (4, 0)
True 35
"##
    );
}

#[test]
fn cmath_funcs() {
    let src = r##"
import cmath
z = complex(3, 4)
for f in (cmath.sqrt, cmath.exp, cmath.log, cmath.log10, cmath.sin, cmath.cos, cmath.tan, cmath.sinh, cmath.cosh, cmath.tanh, cmath.asin, cmath.acos, cmath.atan, cmath.asinh, cmath.acosh, cmath.atanh):
    print(f.__name__, f(z))
print(cmath.sqrt(-4), cmath.sqrt(-1j), cmath.polar(1j), cmath.rect(2, cmath.pi/2), cmath.phase(-1), cmath.isclose(1+1j, 1+1.0000000001j), cmath.log(8, 2))
print(cmath.exp(1j*cmath.pi), cmath.sqrt(complex(-1, -0.0)))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"sqrt (2+1j)
exp (-13.128783081462158-15.200784463067954j)
log (1.6094379124341003+0.9272952180016122j)
log10 (0.6989700043360187+0.4027191962733731j)
sin (3.853738037919377-27.016813258003932j)
cos (-27.034945603074224-3.851153334811777j)
tan (-0.0001873462046294784+0.999355987381473j)
sinh (-6.5481200409110025-7.61923172032141j)
cosh (-6.580663040551157-7.581552742746545j)
tanh (1.000709536067233+0.00490825806749606j)
asin (0.6339838656391766+2.305509031243477j)
acos (0.9368124611557198-2.305509031243477j)
atan (1.4483069952314644+0.15899719167999918j)
asinh (2.2999140408792695+0.9176168533514787j)
acosh (2.305509031243477+0.9368124611557198j)
atanh (0.1175009073114339+1.4099210495965755j)
2j (0.7071067811865476-0.7071067811865475j) (1.0, 1.5707963267948966) (1.2246467991473532e-16+2j) 3.141592653589793 True (3+0j)
(-1+1.2246467991473532e-16j) -1j
"##
    );
}

#[test]
fn wave_cmd_metadata() {
    let src = r##"
import importlib.resources, wave, io, cmd, webbrowser
w = io.BytesIO()
with wave.open(w, "wb") as f:
    f.setnchannels(1); f.setsampwidth(2); f.setframerate(8000); f.writeframes(b"\x00\x01" * 100)
w.seek(0)
with wave.open(w, "rb") as f:
    print(f.getnchannels(), f.getframerate(), f.getnframes())
class C(cmd.Cmd):
    def do_hi(self, a): print("hi", a)
C().onecmd("hi x")
"##;
    // O `importlib.metadata` lê os `.dist-info` do disco: é coberto pelo caso `python/pip.toml`
    // da bancada, que roda num pseudo-processo e compara com o oráculo.
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"1 8000 100
hi x
"##
    );
}

#[test]
fn exception_chaining() {
    let src = r##"
import sys
def f():
    try:
        {}["k"]
    except KeyError as e:
        raise ValueError("bad") from e
def g():
    try:
        f()
    except ValueError:
        raise RuntimeError("wrapped")
try:
    g()
except RuntimeError as e:
    print(repr(e.__context__), repr(e.__context__.__cause__), e.__suppress_context__)
import traceback
try:
    g()
except RuntimeError as e:
    lines = traceback.format_exception(e)
    print([l for l in lines if "exception" in l.lower()])
class MyErr(Exception): pass
try:
    try: raise KeyError(1)
    except KeyError as k: raise MyErr("m") from k
except MyErr as m:
    print(repr(m.__cause__), m.__context__ is m.__cause__, m.__suppress_context__)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"ValueError('bad') KeyError('k') False
['\nThe above exception was the direct cause of the following exception:\n\n', '\nDuring handling of the above exception, another exception occurred:\n\n']
KeyError(1) True True
"##
    );
}

#[test]
fn annotations_struct_base64_re() {
    let src = r##"
import re, struct, base64
def g(a: int, b: str = "x", *c: float, d: list = None, **e: dict) -> bool: ...
print(g.__annotations__, (lambda: 1).__annotations__)
s = struct.Struct("<I2sf")
print(s.size, s.unpack(s.pack(1, b"hi", 1.5)), base64.b85encode(b"hello"), base64.b85decode(b"Xk~0{Zv"))
print(base64.a85encode(b"hello"), bytes.maketrans(b"ab", b"xy"), b"abc".translate(bytes.maketrans(b"ab", b"xy")))
print(isinstance(re.match("a", "a"), re.Match), isinstance(re.compile("a"), re.Pattern), isinstance(1, re.Match))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"{'a': <class 'int'>, 'b': <class 'str'>, 'c': <class 'float'>, 'd': <class 'list'>, 'e': <class 'dict'>, 'return': <class 'bool'>} {}
10 (1, b'hi', 1.5) b'Xk~0{Zv' b'hello'
b'BOu!rDZ' b'\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f !"#$%&\'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`xycdefghijklmnopqrstuvwxyz{|}~\x7f\x80\x81\x82\x83\x84\x85\x86\x87\x88\x89\x8a\x8b\x8c\x8d\x8e\x8f\x90\x91\x92\x93\x94\x95\x96\x97\x98\x99\x9a\x9b\x9c\x9d\x9e\x9f\xa0\xa1\xa2\xa3\xa4\xa5\xa6\xa7\xa8\xa9\xaa\xab\xac\xad\xae\xaf\xb0\xb1\xb2\xb3\xb4\xb5\xb6\xb7\xb8\xb9\xba\xbb\xbc\xbd\xbe\xbf\xc0\xc1\xc2\xc3\xc4\xc5\xc6\xc7\xc8\xc9\xca\xcb\xcc\xcd\xce\xcf\xd0\xd1\xd2\xd3\xd4\xd5\xd6\xd7\xd8\xd9\xda\xdb\xdc\xdd\xde\xdf\xe0\xe1\xe2\xe3\xe4\xe5\xe6\xe7\xe8\xe9\xea\xeb\xec\xed\xee\xef\xf0\xf1\xf2\xf3\xf4\xf5\xf6\xf7\xf8\xf9\xfa\xfb\xfc\xfd\xfe\xff' b'xyc'
True True False
"##
    );
}

#[test]
fn slots_genericalias_singledispatch() {
    let src = r##"
import functools, types
class S:
    __slots__ = ("a", "b")
    def __init__(self): self.a = 1
x = S()
x.b = 2
try:
    x.c = 3
except AttributeError as e:
    print(e)
class D(S):
    pass
D().zz = 1
class W:
    __slots__ = ("a", "__dict__")
w = W(); w.q = 1; print(w.q)
print(list[int], types.GenericAlias(dict, (str, int)), isinstance(list[int], types.GenericAlias))
@functools.singledispatch
def show(x): return "obj"
@show.register
def _(x: int): return "int"
@show.register(list)
def _(x): return "list"
@show.register
def _(x: str | bytes): return "text"
print(show(1), show([1]), show(2.5), show("s"), show(b"b"), sorted(k.__name__ for k in show.registry))
class C:
    @functools.singledispatchmethod
    def f(self, x): return "any"
    @f.register
    def _(self, x: int): return "int"
print(C().f(1), C().f("a"))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"'S' object has no attribute 'c' and no __dict__ for setting new attributes
1
list[int] dict[str, int] True
int list obj text text ['bytes', 'int', 'list', 'object', 'str']
int any
"##
    );
}

#[test]
fn strftime_week_numbers() {
    let src = r##"
import datetime as dt, time
for d in (dt.datetime(2024, 1, 1), dt.datetime(2024, 2, 29, 13, 5), dt.datetime(2023, 1, 1), dt.datetime(2021, 1, 3), dt.datetime(2020, 12, 31)):
    print(d.strftime("%Y-%m-%d %a %U %W %V %G %g %j %u %w"))
print(time.strftime("%U %W", time.gmtime(86400 * 400)))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"2024-01-01 Mon 00 01 01 2024 24 001 1 1
2024-02-29 Thu 08 09 09 2024 24 060 4 4
2023-01-01 Sun 01 00 52 2022 22 001 7 0
2021-01-03 Sun 01 00 53 2020 20 003 7 0
2020-12-31 Thu 52 52 53 2020 20 366 4 4
05 05
"##
    );
}

#[test]
fn intflag_flag_repr() {
    let src = r##"
import enum
class P(enum.IntFlag):
    R = 4; W = 2; X = 1
p = P.R | P.W
print(p, repr(p), f'{p:03b}', 1 | P.X, P.R & 6, p in P.R | P.W | P.X, bool(P(0)), repr(P(0)), repr(P(8)), int(p), p == 6)
print(sorted([P.X, P.R]), [m.name for m in P], P['W'], P(2), ~P.R)
class F(enum.Flag):
    A = enum.auto(); B = enum.auto()
print(F.A | F.B, repr(F.A | F.B), repr(F(0)), F.A in (F.A | F.B))
print(repr(P(12)), str(P(12)), repr(P(9)), P(8) | P.X, str(F(0)), str(P(0)))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"6 <P.R|W: 6> 110 1 4 True False <P: 0> <P: 8> 6 True
[<P.X: 1>, <P.R: 4>] ['R', 'W', 'X'] 2 2 3
F.A|B <F.A|B: 3> <F: 0> True
<P.R|8: 12> 12 <P.X|8: 9> 9 F(0) 0
"##
    );
}

#[test]
fn traceback_reraise_contextmanager() {
    let src = r##"import sys, contextlib
def g(): raise ValueError('x')
def f(): g()
def names(tb):
    out = []
    while tb: out.append((tb.tb_frame.f_code.co_name, tb.tb_lineno)); tb = tb.tb_next
    return out
def mid():
    try: f()
    except ValueError: raise
def top():
    try: mid()
    finally: pass
try: top()
except ValueError as e: print(names(e.__traceback__))
@contextlib.contextmanager
def cm():
    try: yield
    except Exception: raise
try:
    with cm(): f()
except ValueError as e: print(names(e.__traceback__))
try:
    try: f()
    except ValueError as e1: raise KeyError('k') from e1
except KeyError as e2: print(names(e2.__traceback__), names(e2.__cause__.__traceback__))
e = ValueError('z'); e.__traceback__ = None; print(e.__traceback__)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"[('<module>', 14), ('top', 12), ('mid', 9), ('f', 3), ('g', 2)]
[('<module>', 21), ('f', 3), ('g', 2)]
[('<module>', 25)] [('<module>', 24), ('f', 3), ('g', 2)]
None
"##
    );
}

#[test]
fn cooperative_threading() {
    let src = r##"
import threading, queue, concurrent.futures as cf
q = queue.Queue(); out = []
def consumer():
    while True:
        item = q.get()
        if item is None: break
        out.append(item * 2); q.task_done()
t = threading.Thread(target=consumer); t.start()
for i in range(5): q.put(i)
q.put(None); t.join(); print(out, t.is_alive())
jobs = queue.Queue(); results = []
def worker():
    while True:
        n = jobs.get(); results.append(n * n); jobs.task_done()
workers = [threading.Thread(target=worker, daemon=True) for _ in range(3)]
for w in workers: w.start()
for n in range(6): jobs.put(n)
jobs.join(); print(sorted(results))
lock = threading.Lock(); n = [0]
def inc():
    for _ in range(500):
        with lock: n[0] += 1
ts = [threading.Thread(target=inc) for _ in range(4)]; [x.start() for x in ts]; [x.join() for x in ts]; print(n[0])
with cf.ThreadPoolExecutor(2) as ex:
    print([f.result() for f in [ex.submit(lambda x: x + 1, i) for i in range(4)]], list(ex.map(str, range(3))))
cond = threading.Condition(); box = []
def cons():
    with cond:
        while not box: cond.wait()
        print('got', box)
def prod():
    with cond: box.append(1); cond.notify()
c = threading.Thread(target=cons); p = threading.Thread(target=prod); c.start(); p.start(); c.join(); p.join()
ev = threading.Event(); order = []
def waiter(): order.append('wait'); ev.wait(); order.append('go')
def setter(): order.append('set'); ev.set()
a = threading.Thread(target=waiter); b = threading.Thread(target=setter); a.start(); b.start(); a.join(); b.join(); print(order)
sem = threading.Semaphore(2); bar = threading.Barrier(3); seen = []
def party(i): bar.wait(); seen.append(i)
ps = [threading.Thread(target=party, args=(i,)) for i in range(3)]; [x.start() for x in ps]; [x.join() for x in ps]; print(sorted(seen))
loc = threading.local(); loc.v = 'main'; got = []
def readloc(): loc.v = 'child'; got.append(loc.v)
x = threading.Thread(target=readloc); x.start(); x.join(); print(loc.v, got)
names = []
th = threading.Thread(target=lambda: names.append(threading.current_thread().name), name='custom'); th.start(); th.join()
print(names, threading.current_thread().name, threading.active_count())
def boom(): raise ValueError('in thread')
threading.excepthook = lambda args: print('hook', args.exc_type.__name__, args.thread.name)
bt = threading.Thread(target=boom, name='B'); bt.start(); bt.join()
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"[0, 2, 4, 6, 8] False
[0, 1, 4, 9, 16, 25]
2000
[1, 2, 3, 4] ['0', '1', '2']
got [1]
['wait', 'set', 'go']
[0, 1, 2]
main ['child']
['custom'] MainThread 4
hook ValueError B
"##
    );
}

#[test]
fn re_flags_error_scenario() {
    let src = r##"
import re
print(re.I, re.IGNORECASE | re.M, repr(re.X | re.S), int(re.I), re.A.value, re.compile('a', re.I | re.M).flags, re.compile('a').flags)
p = re.compile(r'''(?P<year>\d{4})-(?P<mon>\d\d)  # date
                  (?:-(?P<day>\d\d))?''', re.VERBOSE)
m = p.search('on 2026-10-06 and 2027-01'); print(m.groupdict(), m.span('mon'), m.lastgroup, m.lastindex, m.group(0, 'year'), m[2])
print([mm.groupdict() for mm in p.finditer('2026-10-06 2027-01')], p.groups, p.groupindex)
print(re.sub(r'(?P<w>\w+)@(\w+)', lambda m: m.group('w').upper() + '#' + m.group(2), 'bob@home x@y'), re.sub(r'(a)(b)?', r'[\1|\2]', 'ab a'), re.subn('a', 'b', 'aaa', count=2))
print(re.split(r'(,)\s*', 'a, b,c'), re.split(r'\s+', ' a  b '), re.split('x*', 'axb'), re.findall(r'(\d)(\w)', '1a 2b'), re.findall(r'\d', 'a1b22'))
print(re.match(r'(?<=a)b', 'ab'), re.search(r'(?<=a)b', 'ab').start(), re.search(r'(?<!a)b', 'ab cb').start(), re.fullmatch(r'a+', 'aaa'), re.fullmatch(r'a+', 'aab'))
print(re.escape('a.b*c[d]'), re.escape('é ñ'), re.match(r'^(\w+)\s(?=\d)', 'abc 123').group(), re.search(r'(\w+) \1', 'hello hello world').group())
print(re.findall(r'^\w+', 'one\ntwo', re.M), re.search(r'a.b', 'a\nb', re.S) is not None, re.search('A', 'a', re.I).group(), re.findall(r'(?i)ab', 'AB ab'))
print(re.match(r'(?P<n>a)|(?P<m>b)', 'b').groupdict(), re.match(r'(a)?b', 'b').groups(), re.match(r'(a)?b', 'b').groups('d'), re.match(r'(a)*', 'aaa').group(1))
try: re.compile('(')
except re.error as e: print(type(e).__name__, e.msg, e.pattern, e.pos)
try: re.compile('a{2,1}')
except re.error as e: print(e)
print(re.compile(r'\d+').pattern, repr(re.compile(r'\d+', re.I)), re.compile('x') is re.compile('x'))
print(re.search(r'(\d+)', 'ab 123 cd').expand(r'<\1>'), re.match(r'(.)(.)', 'xy').regs, re.match('a', 'a').string, re.match('a', 'a').re.pattern, bool(re.match('', '')))
print(re.findall(r'\bfoo\b', 'foo food foo.'), re.sub(r'\s+', ' ', 'a \n\t b'), re.sub(r'^', '> ', 'a\nb', flags=re.M), re.findall(r'[^\W\d_]+', 'ab_1cd'))
print(re.match(r'(?:(?:a|b)+c)+', 'abcbac').group(), re.findall(r'a*?', 'aa'), re.findall(r'(a|ab)(c|bcd)(d*)', 'abcd'), re.sub('x*', '-', 'abxd'))
print(re.search(r'\d{2,3}?', '12345').group(), re.search(r'(?x) a b # c', 'ab').group(), re.match(r'\A\d+\Z', '12\n'), re.match(r'\d+$', '12\n').group(), re.match(r'\N{LATIN SMALL LETTER A}', 'a') is not None if hasattr(re, 'NOFLAG') else 0)
print(re.NOFLAG, re.RegexFlag, re.DOTALL.name, sorted(f.name for f in re.RegexFlag)[:3])
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"re.IGNORECASE re.IGNORECASE|re.MULTILINE re.DOTALL|re.VERBOSE 2 256 42 32
{'year': '2026', 'mon': '10', 'day': '06'} (8, 10) day 3 ('2026-10-06', '2026') 10
[{'year': '2026', 'mon': '10', 'day': '06'}, {'year': '2027', 'mon': '01', 'day': None}] 3 {'year': 1, 'mon': 2, 'day': 3}
BOB#home X#y [a|b] [a|] ('bba', 2)
['a', ',', 'b', ',', 'c'] ['', 'a', 'b', ''] ['', 'a', '', 'b', ''] [('1', 'a'), ('2', 'b')] ['1', '2', '2']
None 1 4 <re.Match object; span=(0, 3), match='aaa'> None
a\.b\*c\[d\] é\ ñ abc  hello hello
['one', 'two'] True a ['AB', 'ab']
{'n': None, 'm': 'b'} (None,) ('d',) a
PatternError missing ), unterminated subpattern ( 0
min repeat greater than max repeat at position 2
\d+ re.compile('\\d+', re.IGNORECASE) True
<123> ((0, 2), (0, 1), (1, 2)) a a True
['foo', 'foo'] a b > a
> b ['ab', 'cd']
abcbac ['', 'a', '', 'a', ''] [('a', 'bcd', '')] -a-b--d-
12 ab None 12 True
re.NOFLAG <flag 'RegexFlag'> DOTALL ['ASCII', 'DEBUG', 'DOTALL']
"##
    );
}

#[test]
fn async_comprehensions() {
    let src = r##"
def run(coro):
    try:
        while True:
            coro.send(None)
    except StopIteration as e:
        return e.value
async def agen(n):
    for i in range(n):
        yield i
async def dbl(x):
    return x * 2
async def many(*cs):
    return [await c for c in cs]
async def main():
    print([x async for x in agen(4)])
    print([x async for x in agen(6) if x % 2 == 0])
    print({x: x * x async for x in agen(3)}, {x % 2 async for x in agen(5)})
    print([await dbl(x) for x in range(4)])
    print([await dbl(x) for x in [1, 2, 3] if x > 1], {await dbl(1): 2})
    print([(a, b) async for a in agen(2) async for b in agen(2)])
    print([(a, b) for a in range(2) async for b in agen(2)])
    print(sum([await dbl(i) for i in range(5)]))
    g = (x async for x in agen(3)); print(type(g).__name__, [y async for y in g])
    g2 = (await dbl(x) for x in range(3)); print([y async for y in g2])
    print([[await dbl(y) for y in range(x)] for x in range(3)])
    print(await many(*[dbl(i) for i in range(3)]), await many(*(dbl(i) for i in range(3))))
    r = [x async for x in agen(5) if await dbl(x) > 4]; print(r)
    async def inner():
        return [i async for i in agen(2)]
    print(await inner())
run(main())
try:
    compile("def f():\n    return [x async for x in y]\n", "t", "exec")
except SyntaxError as e: print('SyntaxError', e.msg)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"[0, 1, 2, 3]
[0, 2, 4]
{0: 0, 1: 1, 2: 4} {0, 1}
[0, 2, 4, 6]
[4, 6] {2: 2}
[(0, 0), (0, 1), (1, 0), (1, 1)]
[(0, 0), (0, 1), (1, 0), (1, 1)]
20
async_generator [0, 1, 2]
[0, 2, 4]
[[], [0], [0, 2]]
[0, 2, 4] [0, 2, 4]
[3, 4]
[0, 1]
SyntaxError asynchronous comprehension outside of an asynchronous function
"##
    );
}

#[test]
fn csv_list_reader_and_getattribute() {
    let src = r##"
import csv, sys, builtins
r = csv.reader(["a,b", "1,2", '3,"x,y"'])
print(next(r), list(r))
print(list(csv.reader(("x,y",))), list(csv.reader(iter(["p,q"]))), 'ok', flush=True)
sys.stdout.flush()
print(type(builtins).__name__, 'count' in tuple.__dict__, '__await__' in list.__dict__, 'send' in tuple.__dict__)
class L(tuple):
    def __getattribute__(self, a):
        if a == 'secret':
            return 42
        return tuple.__getattribute__(self, a)
    def __getattr__(self, a):
        return 'fallback:' + a
x = L((1, 2)); print(x.secret, x.count(1), x.missing, len(x))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"['a', 'b'] [['1', '2'], ['3', 'x,y']]
[['x', 'y']] [['p', 'q']] ok
module True False False
42 1 fallback:missing 2
"##
    );
}

#[test]
fn did_you_mean_suggestions() {
    let src = r##"
import traceback

class A:
    def __init__(self):
        self.value = 1

def f(alpha):
    return alphaa

def show(e):
    print("".join(traceback.format_exception_only(e)).strip())

for fn in (lambda: A().valeu, lambda: [].apend(1), lambda: "x".uper(), lambda: {}.itemz()):
    try:
        fn()
    except AttributeError as e:
        show(e)
        print(e.name)
try:
    f(1)
except NameError as e:
    print("".join(traceback.format_exception(e)).splitlines()[-1], e.name)
    show(e)
try:
    sys.exit
except NameError as e:
    print("".join(traceback.format_exception(e)).splitlines()[-1])
try:
    from os import pathh
except ImportError as e:
    show(e)
    print(e.name, e.name_from)
try:
    zzzzzz.foo
except NameError as e:
    print(str(e))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"AttributeError: 'A' object has no attribute 'valeu'. Did you mean: 'value'?
valeu
AttributeError: 'list' object has no attribute 'apend'. Did you mean: 'append'?
apend
AttributeError: 'str' object has no attribute 'uper'. Did you mean: 'upper'?
uper
AttributeError: 'dict' object has no attribute 'itemz'. Did you mean: 'items'?
itemz
NameError: name 'alphaa' is not defined. Did you mean: 'alpha'? alphaa
NameError: name 'alphaa' is not defined
NameError: name 'sys' is not defined. Did you forget to import 'sys'?
ImportError: cannot import name 'pathh' from 'os' (/usr/lib/python3.13/os.py). Did you mean: 'path'?
os pathh
name 'zzzzzz' is not defined
"##
    );
}

#[test]
fn builtin_base_methods_skip_overrides() {
    let src = r##"
class D(dict):
    def __getitem__(self, key):
        v = dict.__getitem__(self, key)
        return v * 2
    def __setitem__(self, key, value):
        dict.__setitem__(self, key, value + 1)
    def __contains__(self, k):
        return dict.__contains__(self, k)
    def __len__(self):
        return dict.__len__(self) + 100
    def __iter__(self):
        return dict.__iter__(self)
    def get(self, k, d=None):
        return dict.get(self, k, d)
    def __repr__(self):
        return "D" + dict.__repr__(self)
    def pop(self, k, *a):
        return dict.pop(self, k, *a)
    def update(self, *a, **k):
        dict.update(self, *a, **k)
    def __delitem__(self, k):
        dict.__delitem__(self, k)
d = D()
d["a"] = 1
print(d["a"], "a" in d, len(d), list(d), d.get("a"), repr(d))
d.update(b=5)
print(dict.items(d), d.pop("b"))
del d["a"]
print(dict.__len__(d))
class L(list):
    def __getitem__(self, i):
        return list.__getitem__(self, i) * 10
    def append(self, x):
        list.append(self, x + 1)
    def __len__(self):
        return list.__len__(self) + 1
    def __iter__(self):
        return list.__iter__(self)
    def __setitem__(self, i, v):
        list.__setitem__(self, i, v)
l = L()
l.append(1)
l[0] = 7
print(l[0], len(l), list(l), list.__len__(l))
class S(str):
    def __getitem__(self, i):
        return str.__getitem__(self, i).upper()
    def __len__(self):
        return str.__len__(self) * 2
print(S("abc")[1], len(S("abc")))
class T(tuple):
    def __getitem__(self, i):
        return tuple.__getitem__(self, i) + 1
print(T((1, 2))[1])
class St(set):
    def __contains__(self, x):
        return set.__contains__(self, x) or x == "magic"
print("magic" in St(), 1 in St({1}))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"4 True 101 ['a'] 2 D{'a': 2}
dict_items([('a', 2)]) 5
0
70 2 [7] 1
B 6
3
True True
"##
    );
}

#[test]
fn builtin_functions_do_not_bind_in_class() {
    let src = r##"
import time

class Clock:
    conv = time.localtime
    fmt = time.strftime

    def go(self):
        return self.conv(0).tm_year, self.fmt("%Y", self.conv(0))

c = Clock()
print(c.go(), Clock.conv(86400).tm_mday, c.conv(86400 * 2).tm_mday)

def plain(x=None):
    return "plain", x

class K:
    f = plain
print(K().f()[0])
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"(1970, '1970') 2 3
plain
"##
    );
}

#[test]
fn qualname_in_messages_and_repr() {
    let src = r##"
class A:
    def __init__(self, x):
        self.x = x

    def m(self, y):
        def inner(z):
            pass
        return inner

    @staticmethod
    def s(a):
        pass


def outer():
    def f(a):
        pass
    return f


print(A.m.__qualname__, A.s.__qualname__, outer().__qualname__, outer.__qualname__)
print(A.__qualname__, (lambda: 0).__qualname__)
print(A(1).m(2).__qualname__)
print(repr(A.m).split(" at ")[0], repr(outer()).split(" at ")[0])
for call in (lambda: A(), lambda: A(1).m(), lambda: A.s(), lambda: outer()()):
    try:
        call()
    except TypeError as e:
        print(e)


class B:
    class C:
        def n(self):
            pass


print(B.C.__qualname__, B.C.n.__qualname__)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"A.m A.s outer.<locals>.f outer
A <lambda>
A.m.<locals>.inner
<function A.m <function outer.<locals>.f
A.__init__() missing 1 required positional argument: 'x'
A.m() missing 1 required positional argument: 'y'
A.s() missing 1 required positional argument: 'a'
outer.<locals>.f() missing 1 required positional argument: 'a'
B.C B.C.n
"##
    );
}

#[test]
fn bounded_queue_with_producer_thread() {
    let src = r##"
import queue
import threading

q = queue.Queue(maxsize=2)
prod = threading.Thread(target=lambda: ([q.put(i) for i in range(5)], q.put(None)))
prod.start()
got = []
while (v := q.get()) is not None:
    got.append(v)
print(got)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"[0, 1, 2, 3, 4]
"##
    );
}

#[test]
fn repr_errors_namedtuple_percent_lazy_iter() {
    let src = r##"
import collections
import io

P = collections.namedtuple("P", "a b")
print("%s-%s" % P(1, 2))
f = io.StringIO("a\nb\n\nc\n")
print(list(iter(f.readline, "")))
n = [0]


def tick():
    n[0] += 1
    return n[0]


it = iter(tick, 3)
print(next(it), next(it), list(it), n[0])


class R:
    def __repr__(self):
        raise ValueError("boom")


for g in (repr, str, lambda o: "%r" % (o,), lambda o: f"{o!r}", lambda o: [o].__repr__(), print):
    try:
        g(R())
    except ValueError as e:
        print("raised", e)
print(1, 2, end="|")
try:
    print("x", R(), "y")
except ValueError:
    print("<fail>")
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"1-2
['a\n', 'b\n', '\n', 'c\n']
1 2 [] 3
raised boom
raised boom
raised boom
raised boom
raised boom
raised boom
1 2|x <fail>
"##
    );
}

#[test]
fn inspect_signature_abc_annotated_weakref_slots() {
    let src = r##"
import abc
import dataclasses
import inspect
import weakref
from typing import Annotated, get_args, get_origin


def f(x: int, *a: str, k: 'list[int]' = None, **kw: float) -> 'T':
    pass


class C:
    def __init__(self, a: int, b=2):
        pass


@dataclasses.dataclass(kw_only=True)
class Cfg:
    host: str = 'h'
    port: int = 80
    tags: list = dataclasses.field(default_factory=list)


class Shape(abc.ABC):
    @abc.abstractmethod
    def area(self):
        ...


class Sq(Shape):
    def area(self):
        return 1


class Slots:
    __slots__ = ('a',)


print(inspect.signature(f))
print(inspect.signature(C))
print(inspect.signature(lambda x, /, y=1, *, z: 0))
print(inspect.signature(Cfg))
print(inspect.isabstract(Shape), inspect.isabstract(Sq), Shape.__abstractmethods__, Sq.__abstractmethods__)
print(inspect.getdoc(Sq) == inspect.getdoc(abc.ABC))
print(C.__mro__, object in Sq.__mro__)
print(Annotated[int, 'm'].__metadata__, frozenset().__doc__ is None or True)
bound = inspect.signature(f).bind(1, 'a', 'b', k=[1], z=2.0)
print(bound.args, bound.kwargs)
try:
    weakref.ref(Slots())
except TypeError as e:
    print(e)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"(x: int, *a: str, k: 'list[int]' = None, **kw: float) -> 'T'
(a: int, b=2)
(x, /, y=1, *, z)
(*, host: str = 'h', port: int = 80, tags: list = <factory>) -> None
True False frozenset({'area'}) frozenset()
True
(<class '__main__.C'>, <class 'object'>) True
('m',) True
(1, 'a', 'b') {'k': [1], 'z': 2.0}
cannot create weak reference to 'Slots' object
"##
    );
}

#[test]
fn shlex_official_module_with_lexer_class() {
    let src = r##"
import shlex
print(shlex.split("a 'b c' \"d e\" f\\ g # c", comments=True), shlex.quote("it's"), shlex.quote(""), shlex.quote("safe-1.txt"), shlex.join(['a b', 'c']))
lex = shlex.shlex("x = 'a b' ; y", posix=True)
lex.whitespace_split = False
print(list(lex))
lx = shlex.shlex("foo bar # baz\nqux", posix=True)
lx.whitespace_split = True
print(list(lx))
try:
    shlex.split("a 'b")
except ValueError as e:
    print(e)
print(shlex.split("a=1 b", posix=False), shlex.split(""), shlex.split("  x   y  "))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"['a', 'b c', 'd e', 'f g'] 'it'"'"'s' '' safe-1.txt 'a b' c
['x', '=', 'a b', ';', 'y']
['foo', 'bar', 'qux']
No closing quotation
['a=1', 'b'] [] ['x', 'y']
"##
    );
}

#[test]
fn marshal_resource_fcntl_readline_protocol_modules() {
    let src = r##"
import fcntl
import ftplib
import marshal
import netrc
import readline
import resource
import smtplib
import wsgiref.util
import xmlrpc.client

blob = marshal.dumps((1, 'a', [2.5, None, True], {'k': b'v'}, {1, 2}, frozenset({3}), 2 ** 70, -(2 ** 40), 'é', 1 + 2j, ...))
print(marshal.loads(blob))
print(marshal.loads(b'\xa9\x02\xfa\x01x\xdf\xe9\x01\x00\x00\x00'[:0] or marshal.dumps(('x', 'x'))))
print(marshal.dumps(None), [marshal.loads(marshal.dumps(v)) for v in (1, (1, 2), [1], 1.5, 'x' * 300, b'zz', -5, 2 ** 31)])
print(marshal.loads(b'\xe9\x01\x00\x00\x00'), marshal.loads(b'\xa9\x02\xfa\x01x\x72\x01\x00\x00\x00'), marshal.loads(b'\xe7\x00\x00\x00\x00\x00\x00\xf8?'))
print(resource.RLIMIT_NOFILE, resource.getrlimit(resource.RLIMIT_CORE), resource.getpagesize(), resource.getrusage(resource.RUSAGE_SELF).ru_utime >= 0)
fcntl.flock(1, fcntl.LOCK_EX | fcntl.LOCK_NB)
fcntl.flock(1, fcntl.LOCK_UN)
print(fcntl.LOCK_SH, fcntl.LOCK_EX, fcntl.LOCK_NB, fcntl.LOCK_UN, fcntl.F_GETFL)
readline.add_history('one')
readline.add_history('two')
print(readline.get_current_history_length(), readline.get_history_item(2), readline.get_history_item(9))
readline.parse_and_bind('tab: complete')
print(smtplib.SMTP_PORT, smtplib.CRLF.encode(), ftplib.FTP.port if hasattr(ftplib.FTP, 'port') else None, ftplib.MAXLINE)
print(xmlrpc.client.dumps((1, 'a', [2.5]), 'm'))
print(xmlrpc.client.loads('<methodResponse><params><param><value><int>7</int></value></param></params></methodResponse>'))
env = {'wsgi.url_scheme': 'http', 'HTTP_HOST': 'x.org:8080', 'SCRIPT_NAME': '/app', 'PATH_INFO': '/a b', 'QUERY_STRING': 'q=1'}
print(wsgiref.util.request_uri(env), wsgiref.util.application_uri(env))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"(1, 'a', [2.5, None, True], {'k': b'v'}, {1, 2}, frozenset({3}), 1180591620717411303424, -1099511627776, 'é', (1+2j), Ellipsis)
('x', 'x')
b'N' [1, (1, 2), [1], 1.5, 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx', b'zz', -5, 2147483648]
1 ('x', 'x') 1.5
7 (-1, -1) 4096 True
1 2 4 8 3
2 two None
25 b'\r\n' 21 8192
<?xml version='1.0'?>
<methodCall>
<methodName>m</methodName>
<params>
<param>
<value><int>1</int></value>
</param>
<param>
<value><string>a</string></value>
</param>
<param>
<value><array><data>
<value><double>2.5</double></value>
</data></array></value>
</param>
</params>
</methodCall>

((7,), None)
http://x.org:8080/app/a%20b?q=1 http://x.org:8080/app
"##
    );
}

#[test]
fn live_globals_and_oserror_errno_subclasses() {
    let src = r##"
import enum
import sys

globals()['A'] = 1
globals().update(B=2)
print(A, B)
g = globals()
C = 3
print(g['C'], 'C' in g)
del g['C']
print('C' in globals())
m = sys.modules[__name__]
m.D = 4
print(D, m.__dict__['D'], m.__dict__ is vars(m))
vars(m)['E'] = 5
print(E, sys._getframe(0).f_globals is globals())
Q_A = 1
Q_B = 2
enum.IntEnum._convert_('X', __name__, lambda n: n.startswith('Q_'))
print(list(X), X.Q_A)
exec('F = A + B', globals())
print(F)
import errno
print(type(OSError(errno.ECONNREFUSED, 'x')).__name__, type(OSError(errno.ENOENT, 'x')).__name__, type(OSError(errno.EAGAIN, 'x')).__name__)


class E(OSError):
    pass


e = E(5, 'x')
print(e.args, str(e), e.errno, e.strerror, e.filename)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"1 2
3 True
False
4 4 True
5 True
[<X.Q_A: 1>, <X.Q_B: 2>] 1
3
ConnectionRefusedError FileNotFoundError BlockingIOError
(5, 'x') [Errno 5] x 5 x None
"##
    );
}

#[test]
fn official_pathlib_ntpath_fnmatch_glob_pickletools() {
    let src = r##"
import fnmatch
import genericpath
import glob
import ntpath
import pathlib
import pickle
import pickletools

print(ntpath.join('C:\\a', 'b', '..\\c'), ntpath.normpath('C:/a/./b/../c'), ntpath.splitdrive('D:\\x\\y'), ntpath.basename('C:\\a\\b.txt'))
p = pathlib.PureWindowsPath('C:/Users/me/file.tar.gz')
print(p, p.drive, p.parts, p.suffixes, p.parent, p.as_posix(), p.name)
print(pathlib.PureWindowsPath('a/b') / 'c', pathlib.PureWindowsPath('A:/x') == pathlib.PureWindowsPath('a:/X'))
print(genericpath.commonprefix(['/usr/lib', '/usr/local']))
pickletools.dis(pickle.dumps({'a': [1, 2]}, protocol=2))
print(fnmatch.fnmatch('a.PY', '*.py'), fnmatch.fnmatchcase('a.PY', '*.py'), fnmatch.filter(['a.py', 'b.txt', 'c.py'], '*.py'))
print(fnmatch.translate('*.[ch]'), glob.translate('**/*.py', recursive=True, include_hidden=True))
print(glob.has_magic('a*'), glob.escape('a*b[c]'))
q = pathlib.PurePosixPath('/usr/local/lib/python3.13/site.py')
print(q.match('*.py'), q.match('lib/*/site.py'), q.full_match('/usr/**/*.py'), q.parents[1], q.with_stem('x'))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"C:\a\b\..\c C:\a\c ('D:', '\\x\\y') b.txt
C:\Users\me\file.tar.gz C: ('C:\\', 'Users', 'me', 'file.tar.gz') ['.tar', '.gz'] C:\Users\me C:/Users/me/file.tar.gz file.tar.gz
a\b\c True
/usr/l
    0: \x80 PROTO      2
    2: }    EMPTY_DICT
    3: q    BINPUT     0
    5: X    BINUNICODE 'a'
   11: q    BINPUT     1
   13: ]    EMPTY_LIST
   14: q    BINPUT     2
   16: (    MARK
   17: K        BININT1    1
   19: K        BININT1    2
   21: e        APPENDS    (MARK at 16)
   22: s    SETITEM
   23: .    STOP
highest protocol among opcodes = 2
False False ['a.py', 'c.py']
(?s:.*\.[ch])\Z (?s:(?:.+/)?[^/]*\.py)\Z
True a[*]b[[]c]
True True True /usr/local/lib /usr/local/lib/python3.13/x.py
"##
    );
}

#[test]
fn str_slice_and_regex_cache_on_long_text() {
    let src = r##"
import re
s = 'abcdefghij' * 200
print(s[3:8], s[-5:], s[:3], s[5:2] == '', s[::7][:5], s[::-97][:4], s[1:100:13][:3], s[-3:-1], len(s[:]))
u = 'áéíóú' * 300
print(u[1:4], u[::600], u[-2:], u[2:1000:301])
big = '{"k": [1, 2, 3], "s": "x"}' * 40
m = re.compile(r'\d+')
pos = 0
found = []
while True:
    mm = m.search(big, pos)
    if not mm:
        break
    found.append(mm.group())
    pos = mm.end()
print(len(found), found[:6])
b = (b'ab12' * 300)
print(len(re.findall(rb'\d+', b)), re.compile(rb'[a-z]+').match(b, 4).group())
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"defgh fghij abc True ahebi jcfi beh hi 2000
éíó ááá óú íóúá
120 ['1', '2', '3', '1', '2', '3']
300 b'ab'
"##
    );
}

#[test]
fn random_native_twister_matches_cpython() {
    let src = r##"
import random
random.seed(42)
print(random.random(), random.randint(1, 100), random.getrandbits(5), random.getrandbits(32), random.getrandbits(33), random.getrandbits(62), random.getrandbits(100))
random.seed('abc'); print(random.random(), random.choice(range(1000)), random.sample(range(50), 5))
random.seed(b'xyz'); l = list(range(20)); random.shuffle(l); print(l)
r = random.Random(7)
print([r.randrange(10) for _ in range(8)], r.gauss(0, 1), r.uniform(1, 2), r.betavariate(2, 3), r.randbytes(5))
st = r.getstate(); a = r.random(); r.setstate(st); print(a == r.random(), len(st[1]), st[1][-1], st[0])
class R2(random.Random):
    def random(self):
        return 0.25
print(R2(1).randint(1, 4), R2().random())
random.seed(0); print(random.random(), random.random())
random.seed(2**70 + 5); print(random.random(), random.getrandbits(64))
print(random.choices(range(5), k=6), random.randrange(10**20), random.getrandbits(0))
try:
    random.getrandbits(-1)
except ValueError as e:
    print(e)
random.seed(99); print(random.triangular(), random.expovariate(1.5), random.normalvariate(0, 1), random.binomialvariate(10, .5))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"0.6394267984578837 4 23 1181241943 1051802512 3396374007001956841 1167876019479626170561453694887
0.7720246314157545 571 [29, 45, 22, 49, 10]
[5, 0, 17, 1, 11, 3, 2, 13, 18, 4, 14, 16, 6, 15, 12, 7, 19, 9, 10, 8]
[5, 2, 6, 0, 1, 8, 1, 5] -1.9029547557688855 1.2146981808356618 0.22156998794866342 b'\xb6\xdd!\x0f\xd3'
True 625 27 3
1 0.25
0.8444218515250481 0.7579544029403025
0.46679953776226335 11542059036137560337
[1, 1, 2, 1, 4, 0] 87701585912082931622 0
number of bits must be non-negative
0.4494319052668971 0.14882524098663694 -0.7331645826020126 7
"##
    );
}

#[test]
fn json_native_fast_path_matches_cpython() {
    let src = r##"
import json

seed = [12345]


def rnd(n):
    seed[0] = (seed[0] * 1103515245 + 12345) & 0x7fffffff
    return (seed[0] >> 8) % n


STRS = ['', 'a', 'hello world', 'ação', '日本語', '😀 emoji', 'q"uote', 'back\\slash', 'tab\there', 'nl\nx', '\x00\x1f', '\x7f', '  ', '/', 'é' * 20, '', 'à', '\U0010ffff']
FLOATS = [0.0, -0.0, 0.1, 1.5, -2.25, 1e22, 1e-7, 1e21, 123456789.123456789, 5e-324, 1.7976931348623157e308, 3.14159, 100.0, 1e16, 1e15, 0.00001, 12345678901234567890.0, 2.5e-5]
INTS = [0, 1, -1, 42, 2**31, -2**31, 2**53, 2**63 - 1, -2**63, 2**63, 10**30, -10**25]


def gen(d):
    k = rnd(9) if d < 4 else rnd(6)
    if k == 0:
        return None
    if k == 1:
        return bool(rnd(2))
    if k == 2:
        return INTS[rnd(len(INTS))]
    if k == 3:
        return FLOATS[rnd(len(FLOATS))]
    if k == 4:
        return STRS[rnd(len(STRS))] + str(rnd(10))
    if k == 5:
        return STRS[rnd(len(STRS))]
    if k == 6:
        return [gen(d + 1) for _ in range(rnd(5))]
    if k == 7:
        return {STRS[rnd(len(STRS))] + str(i): gen(d + 1) for i in range(rnd(5))}
    return tuple(gen(d + 1) for _ in range(rnd(4)))


bad = 0
for i in range(400):
    o = gen(0)
    for ea in (True, False):
        s = json.dumps(o, ensure_ascii=ea)
        r = json.loads(s)
        if i < 6:
            print(ea, s[:90])
        print(i, ea, len(s), hash(s) & 0xffff if False else sum(map(ord, s)) % 100003, repr(r)[:60] if i < 40 else len(repr(r)))

special = [
    '{"a": 1, "a": 2}', '[1, 2,3 ]', ' \n\t[ ] ', '-0', '-0.0', '1e400', '-1e400', '1E5', '0.5e-3', '123456789012345678901234567890',
    '"\\ud83d\\ude00"', '"\\u00e9\\n\\/"', 'NaN', '-Infinity', 'Infinity', '[NaN, Infinity]', 'true', 'null',
    '{"k": {"k": {"k": [[[]]]}}}', '"x"', '12', '3.0', '[1,]', '{"a":}', '', ' ', '[1] x', '"abc', '01', '1.', '.5', "'a'", '{"a" 1}', '[1 2]', '"\t"', 'nul', '﻿[1]',
]
for sp in special:
    try:
        r = json.loads(sp)
        print('ok', repr(sp)[:30], repr(r)[:50], type(r).__name__)
    except Exception as e:
        print('err', repr(sp)[:30], type(e).__name__, e)

for o in ({1: 'a', 2.5: 'b', True: 'c', None: 'd', 'e': 'f'}, {(1, 2): 3}, [object()], {'a': {1, 2}}, float('nan'), [float('inf'), -float('inf')], {'x': b'by'}, 10**40, [[1, [2, [3, {'k': (4, 5)}]]]]):
    try:
        print(json.dumps(o), json.dumps(o, ensure_ascii=False))
    except Exception as e:
        print('err', type(e).__name__, e)
a = []
a.append(a)
try:
    json.dumps(a)
except Exception as e:
    print(type(e).__name__, e)
deep = []
cur = deep
for _ in range(600):
    n = []
    cur.append(n)
    cur = n
print(len(json.dumps(deep)), json.loads(json.dumps(deep)) == deep)
print(json.dumps({'a': 1, 'b': [1, 2]}, sort_keys=True), json.dumps([1, {'a': 2}], indent=2), json.dumps({'a': 1}, separators=(',', ':')))
class D(dict):
    pass
from collections import OrderedDict
print(json.dumps(D(a=1)), json.dumps(OrderedDict([('z', 1), ('a', 2)])), json.dumps(True), json.dumps('é'), json.dumps(1.0))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"True {"tab\there0": 0.1, "1": {}}
0 True 28 2067 {'tab\there0': 0.1, '1': {}}
False {"tab\there0": 0.1, "1": {}}
0 False 28 2067 {'tab\there0': 0.1, '1': {}}
True null
1 True 4 443 None
False null
1 False 4 443 None
True null
2 True 4 443 None
False null
2 False 4 443 None
True [[[false]], true, 1.7976931348623157e+308]
3 True 42 2918 [[[False]], True, 1.7976931348623157e+308]
False [[[false]], true, 1.7976931348623157e+308]
3 False 42 2918 [[[False]], True, 1.7976931348623157e+308]
True ["\u65e5\u672c\u8a9e2", {"\u65e5\u672c\u8a9e0": ["a\u00e7\u00e3o6", null, "tab\there"], "\
4 True 249 18479 ['日本語2', {'日本語0': ['ação6', None, 'tab\there'], '\ue0001': '
False ["日本語2", {"日本語0": ["ação6", null, "tab\there"], "1": "q\"uote", "2": ""}, "ééééééééééééé
4 False 99 242 ['日本語2', {'日本語0': ['ação6', None, 'tab\there'], '\ue0001': '
True false
5 True 5 523 False
False false
5 False 5 523 False
6 True 10 522 [0.0, '4']
6 False 10 522 [0.0, '4']
7 True 27 1294 -10000000000000000000000000
7 False 27 1294 -10000000000000000000000000
8 True 19 1000 9223372036854775807
8 False 19 1000 9223372036854775807
9 True 67 4873 {'0': True, 'ação1': 1e-07, '日本語2': False}
9 False 42 90939 {'0': True, 'ação1': 1e-07, '日本語2': False}
10 True 217 15917 {'à0': None, 'a1': 9223372036854775808, '日本語2': ['ééééééééé
10 False 97 98207 {'à0': None, 'a1': 9223372036854775808, '日本語2': ['ééééééééé
11 True 4 443 None
11 False 4 443 None
12 True 20 1046 -9223372036854775808
12 False 20 1046 -9223372036854775808
13 True 2 248 {}
13 False 2 248 {}
14 True 5 239 100.0
14 False 5 239 100.0
15 True 4 443 None
15 False 4 443 None
16 True 21 1574 '日本語1'
16 False 6 88100 '日本語1'
17 True 37 3215 [False, 'tab\there7', '\U0010ffff']
17 False 26 16067 [False, 'tab\there7', '\U0010ffff']
18 True 4 448 True
18 False 4 448 True
19 True 3 142 0.0
19 False 3 142 0.0
20 True 4 448 True
20 False 4 448 True
21 True 63 4809 {'tab\there0': {'\ue0000': 'hello world'}, '\x00\x1f1': []}
21 False 58 61699 {'tab\there0': {'\ue0000': 'hello world'}, '\x00\x1f1': []}
22 True 122 9328 'éééééééééééééééééééé'
22 False 22 4728 'éééééééééééééééééééé'
23 True 34 2066 [42, 1.2345678901234567e+19, None]
23 False 34 2066 [42, 1.2345678901234567e+19, None]
24 True 186 13622 {'0': [1e+21, '\x7f4', 'tab\there0'], 'nl\nx1': 5e-324, 'ééé
24 False 81 8687 {'0': [1e+21, '\x7f4', 'tab\there0'], 'nl\nx1': 5e-324, 'ééé
25 True 5 523 False
25 False 5 523 False
26 True 4 215 'a2'
26 False 4 215 'a2'
27 True 14 1294 '\U0010ffff'
27 False 3 14146 '\U0010ffff'
28 True 3 165 'a'
28 False 3 165 'a'
29 True 42 3111 [[None, ['\ue000', []], '\x7f5'], False]
29 False 32 59666 [[None, ['\ue000', []], '\x7f5'], False]
30 True 243 18071 [[{'nl\nx0': 100.0, 'hello world1': -0.0, 'back\\slash2': '\
30 False 127 25988 [[{'nl\nx0': 100.0, 'hello world1': -0.0, 'back\\slash2': '\
31 True 2 248 {}
31 False 2 248 {}
32 True 7 396 2.5e-05
32 False 7 396 2.5e-05
33 True 4 443 None
33 False 4 443 None
34 True 20 1046 -9223372036854775808
34 False 20 1046 -9223372036854775808
35 True 45 2825 [None, None, 2147483648, 9223372036854775807]
35 False 45 2825 [None, None, 2147483648, 9223372036854775807]
36 True 4 443 None
36 False 4 443 None
37 True 5 523 False
37 False 5 523 False
38 True 5 523 False
38 False 5 523 False
39 True 5 523 False
39 False 5 523 False
40 True 31 2468 21
40 False 21 2008 21
41 True 5 292 5
41 False 5 292 5
42 True 4 443 4
42 False 4 443 4
43 True 39 3201 39
43 False 34 60091 39
44 True 4 443 4
44 False 4 443 4
45 True 11 1007 11
45 False 11 1007 11
46 True 157 11590 131
46 False 131 25600 131
47 True 7 608 7
47 False 7 608 7
48 True 3 148 3
48 False 3 148 3
49 True 61 3589 61
49 False 61 3589 61
50 True 4 443 4
50 False 4 443 4
51 True 4 443 4
51 False 4 443 4
52 True 31 2398 31
52 False 31 2398 31
53 True 14 1247 14
53 False 14 1247 14
54 True 141 11008 39
54 False 30 19260 39
55 True 26 2113 26
55 False 26 2113 26
56 True 4 443 4
56 False 4 443 4
57 True 21 1580 6
57 False 6 88106 6
58 True 2 248 2
58 False 2 248 2
59 True 21 1892 21
59 False 21 1892 21
60 True 5 523 5
60 False 5 523 5
61 True 5 244 5
61 False 5 244 5
62 True 20 1046 20
62 False 20 1046 20
63 True 15 951 15
63 False 5 16589 15
64 True 4 448 4
64 False 4 448 4
65 True 6 632 6
65 False 6 632 6
66 True 16 845 16
66 False 16 845 16
67 True 1 49 1
67 False 1 49 1
68 True 4 222 4
68 False 4 222 4
69 True 122 9328 22
69 False 22 4728 22
70 True 119 8580 119
70 False 119 8580 119
71 True 11 572 11
71 False 11 572 11
72 True 18 944 18
72 False 18 944 18
73 True 30 2612 28
73 False 19 15464 28
74 True 4 448 4
74 False 4 448 4
75 True 31 1489 31
75 False 31 1489 31
76 True 12 1058 12
76 False 12 1058 12
77 True 19 1001 19
77 False 19 1001 19
78 True 4 166 4
78 False 4 166 4
79 True 5 296 5
79 False 5 296 5
80 True 27 1294 27
80 False 27 1294 27
81 True 4 219 4
81 False 4 219 4
82 True 4 187 4
82 False 4 187 4
83 True 4 252 4
83 False 4 252 4
84 True 4 443 4
84 False 4 443 4
85 True 20 1046 20
85 False 20 1046 20
86 True 8 576 8
86 False 8 576 8
87 True 203 13609 175
87 False 176 68947 175
88 True 16 1194 6
88 False 6 734 6
89 True 4 443 4
89 False 4 443 4
90 True 4 443 4
90 False 4 443 4
91 True 4 187 4
91 False 4 187 4
92 True 249 18459 146
92 False 134 27301 146
93 True 6 352 6
93 False 6 352 6
94 True 20 1046 20
94 False 20 1046 20
95 True 2 184 2
95 False 2 184 2
96 True 4 443 4
96 False 4 443 4
97 True 16 1194 6
97 False 6 734 6
98 True 4 443 4
98 False 4 443 4
99 True 212 16390 104
99 False 96 24307 104
100 True 2 184 2
100 False 2 184 2
101 True 7 396 7
101 False 7 396 7
102 True 4 443 4
102 False 4 443 4
103 True 14 925 10
103 False 14 925 10
104 True 26 2012 21
104 False 21 2376 21
105 True 20 1525 5
105 False 5 88051 5
106 True 1 49 1
106 False 1 49 1
107 True 5 523 5
107 False 5 523 5
108 True 4 443 4
108 False 4 443 4
109 True 15 982 11
109 False 15 982 11
110 True 5 239 5
110 False 5 239 5
111 True 4 448 4
111 False 4 448 4
112 True 5 523 5
112 False 5 523 5
113 True 448 32323 229
113 False 196 51290 229
114 True 123 8281 97
114 False 87 37929 97
115 True 19 1390 19
115 False 14 58280 19
116 True 4 448 4
116 False 4 448 4
117 True 213 16106 99
117 False 98 67936 99
118 True 2 184 2
118 False 2 184 2
119 True 5 523 5
119 False 5 523 5
120 True 17 1248 7
120 False 7 788 7
121 True 2 68 2
121 False 2 68 2
122 True 2 184 2
122 False 2 184 2
123 True 83 5030 83
123 False 83 5030 83
124 True 4 443 4
124 False 4 443 4
125 True 26 1549 26
125 False 26 1549 26
126 True 5 523 5
126 False 5 523 5
127 True 18 863 18
127 False 18 863 18
128 True 19 1000 19
128 False 19 1000 19
129 True 4 443 4
129 False 4 443 4
130 True 5 293 5
130 False 5 293 5
131 True 27 1294 27
131 False 27 1294 27
132 True 23 1243 23
132 False 23 1243 23
133 True 2 102 2
133 False 2 102 2
134 True 4 443 4
134 False 4 443 4
135 True 8 530 6
135 False 3 195 6
136 True 14 1240 14
136 False 14 1240 14
137 True 33 2605 27
137 False 22 15457 27
138 True 11 1007 11
138 False 11 1007 11
139 True 17 1486 17
139 False 17 1486 17
140 True 4 286 4
140 False 4 286 4
141 True 23 1243 23
141 False 23 1243 23
142 True 9 569 4
142 False 4 933 4
143 True 4 443 4
143 False 4 443 4
144 True 5 523 5
144 False 5 523 5
145 True 12 1056 12
145 False 12 1056 12
146 True 5 332 5
146 False 5 332 5
147 True 4 443 4
147 False 4 443 4
148 True 116 7984 100
148 False 85 36014 100
149 True 207 14184 153
149 False 127 31363 153
150 True 8 661 8
150 False 8 661 8
151 True 17 1248 7
151 False 7 788 7
152 True 32 2370 30
152 False 27 2035 30
153 True 2 184 2
153 False 2 184 2
154 True 8 658 8
154 False 8 658 8
155 True 88 5568 76
155 False 67 17960 76
156 True 70 5683 70
156 False 70 5683 70
157 True 48 2805 44
157 False 38 2135 44
158 True 4 448 4
158 False 4 448 4
159 True 122 9328 22
159 False 22 4728 22
160 True 5 296 5
160 False 5 296 5
161 True 196 14444 96
161 False 86 25482 96
162 True 5 523 5
162 False 5 523 5
163 True 7 707 7
163 False 7 707 7
164 True 5 292 5
164 False 5 292 5
165 True 122 8236 104
165 False 101 35752 104
166 True 20 1046 20
166 False 20 1046 20
167 True 4 443 4
167 False 4 443 4
168 True 2 102 2
168 False 2 102 2
169 True 19 1000 19
169 False 19 1000 19
170 True 4 448 4
170 False 4 448 4
171 True 73 5340 62
171 False 57 89717 62
172 True 71 4908 71
172 False 71 4908 71
173 True 385 27689 272
173 False 223 36650 272
174 True 4 448 4
174 False 4 448 4
175 True 192 14046 92
175 False 92 9446 92
176 True 17 1243 7
176 False 7 783 7
177 True 4 443 4
177 False 4 443 4
178 True 14 1294 12
178 False 3 14146 12
179 True 27 1294 27
179 False 27 1294 27
180 True 4 443 4
180 False 4 443 4
181 True 148 11256 46
181 False 43 6321 46
182 True 28 2217 28
182 False 28 2217 28
183 True 122 9328 22
183 False 22 4728 22
184 True 7 608 7
184 False 7 608 7
185 True 47 3660 47
185 False 47 3660 47
186 True 85 6147 60
186 False 60 92213 60
187 True 4 368 4
187 False 4 368 4
188 True 11 1007 11
188 False 11 1007 11
189 True 18 944 18
189 False 18 944 18
190 True 5 523 5
190 False 5 523 5
191 True 23 1243 23
191 False 23 1243 23
192 True 4 187 4
192 False 4 187 4
193 True 4 448 4
193 False 4 448 4
194 True 4 448 4
194 False 4 448 4
195 True 3 143 3
195 False 3 143 3
196 True 23 1243 23
196 False 23 1243 23
197 True 14 1241 14
197 False 14 1241 14
198 True 15 975 11
198 False 15 975 11
199 True 23 1243 23
199 False 23 1243 23
200 True 4 443 4
200 False 4 443 4
201 True 2 248 2
201 False 2 248 2
202 True 4 213 4
202 False 4 213 4
203 True 350 24927 212
203 False 198 75873 212
204 True 351 25140 239
204 False 215 36190 239
205 True 5 523 5
205 False 5 523 5
206 True 7 396 7
206 False 7 396 7
207 True 19 1000 19
207 False 19 1000 19
208 True 5 296 5
208 False 5 296 5
209 True 29 1888 29
209 False 29 1888 29
210 True 286 21921 164
210 False 143 70541 164
211 True 21 1707 10
211 False 10 29194 10
212 True 39 2192 39
212 False 39 2192 39
213 True 62 4871 55
213 False 46 18087 55
214 True 2 248 2
214 False 2 248 2
215 True 78 4422 78
215 False 78 4422 78
216 True 31 1489 31
216 False 31 1489 31
217 True 19 1001 19
217 False 19 1001 19
218 True 5 523 5
218 False 5 523 5
219 True 7 357 7
219 False 7 357 7
220 True 4 443 4
220 False 4 443 4
221 True 5 296 5
221 False 5 296 5
222 True 5 239 5
222 False 5 239 5
223 True 18 944 18
223 False 18 944 18
224 True 7 608 7
224 False 7 608 7
225 True 18 1134 18
225 False 13 58024 18
226 True 1 48 1
226 False 1 48 1
227 True 40 2552 40
227 False 40 2552 40
228 True 16 845 16
228 False 16 845 16
229 True 5 298 5
229 False 5 298 5
230 True 7 357 7
230 False 7 357 7
231 True 5 523 5
231 False 5 523 5
232 True 48 3500 46
232 False 37 16352 46
233 True 15 1350 13
233 False 4 14202 13
234 True 20 1525 5
234 False 5 88051 5
235 True 5 523 5
235 False 5 523 5
236 True 6 352 6
236 False 6 352 6
237 True 9 573 9
237 False 4 57463 9
238 True 4 443 4
238 False 4 443 4
239 True 7 357 7
239 False 7 357 7
240 True 11 757 11
240 False 6 57647 11
241 True 85 5474 80
241 False 80 5838 80
242 True 11 654 11
242 False 11 654 11
243 True 5 523 5
243 False 5 523 5
244 True 18 944 18
244 False 18 944 18
245 True 383 27325 250
245 False 242 81028 250
246 True 90 5991 59
246 False 60 92421 59
247 True 8 522 8
247 False 3 57412 8
248 True 31 2580 31
248 False 31 2580 31
249 True 38 2567 38
249 False 38 2567 38
250 True 15 946 15
250 False 5 16584 15
251 True 4 443 4
251 False 4 443 4
252 True 4 448 4
252 False 4 448 4
253 True 65 4471 63
253 False 54 17323 63
254 True 9 752 8
254 False 9 752 8
255 True 22 2026 22
255 False 22 2026 22
256 True 43 3316 43
256 False 43 3316 43
257 True 2 248 2
257 False 2 248 2
258 True 20 1046 20
258 False 20 1046 20
259 True 221 16125 109
259 False 95 80807 109
260 True 1 48 1
260 False 1 48 1
261 True 19 1000 19
261 False 19 1000 19
262 True 4 443 4
262 False 4 443 4
263 True 27 1294 27
263 False 27 1294 27
264 True 5 296 5
264 False 5 296 5
265 True 11 1007 11
265 False 11 1007 11
266 True 4 448 4
266 False 4 448 4
267 True 2 248 2
267 False 2 248 2
268 True 23 1759 19
268 False 23 1759 19
269 True 2 68 2
269 False 2 68 2
270 True 19 1000 19
270 False 19 1000 19
271 True 48 3609 33
271 False 33 90135 33
272 True 2 184 2
272 False 2 184 2
273 True 2 68 2
273 False 2 68 2
274 True 11 1007 11
274 False 11 1007 11
275 True 4 220 4
275 False 4 220 4
276 True 8 656 8
276 False 8 656 8
277 True 4 286 4
277 False 4 286 4
278 True 17 1244 7
278 False 7 784 7
279 True 8 530 6
279 False 3 195 6
280 True 9 569 4
280 False 4 933 4
281 True 13 1184 13
281 False 13 1184 13
282 True 31 2122 31
282 False 31 2122 31
283 True 4 448 4
283 False 4 448 4
284 True 15 1021 15
284 False 10 57911 15
285 True 4 443 4
285 False 4 443 4
286 True 3 119 3
286 False 3 119 3
287 True 67 4293 62
287 False 62 4657 62
288 True 62 4216 58
288 False 52 3546 58
289 True 5 523 5
289 False 5 523 5
290 True 4 448 4
290 False 4 448 4
291 True 41 3482 30
291 False 30 30969 30
292 True 4 448 4
292 False 4 448 4
293 True 4 448 4
293 False 4 448 4
294 True 17 1245 7
294 False 7 785 7
295 True 3 143 3
295 False 3 143 3
296 True 4 443 4
296 False 4 443 4
297 True 20 1525 5
297 False 5 88051 5
298 True 20 1654 9
298 False 9 29141 9
299 True 9 578 9
299 False 4 57468 9
300 True 11 1007 11
300 False 11 1007 11
301 True 4 443 4
301 False 4 443 4
302 True 16 1194 6
302 False 6 734 6
303 True 15 943 15
303 False 5 16581 15
304 True 4 443 4
304 False 4 443 4
305 True 21 1703 10
305 False 10 29190 10
306 True 9 752 8
306 False 9 752 8
307 True 10 808 9
307 False 10 808 9
308 True 2 184 2
308 False 2 184 2
309 True 6 352 6
309 False 6 352 6
310 True 15 973 11
310 False 15 973 11
311 True 37 2231 37
311 False 37 2231 37
312 True 4 187 4
312 False 4 187 4
313 True 161 12017 61
313 False 61 7417 61
314 True 6 632 6
314 False 6 632 6
315 True 4 443 4
315 False 4 443 4
316 True 10 802 9
316 False 10 802 9
317 True 47 3427 37
317 False 37 2967 37
318 True 5 523 5
318 False 5 523 5
319 True 19 1000 19
319 False 19 1000 19
320 True 9 577 9
320 False 4 57467 9
321 True 13 1184 13
321 False 13 1184 13
322 True 20 1525 5
322 False 5 88051 5
323 True 4 443 4
323 False 4 443 4
324 True 2 68 2
324 False 2 68 2
325 True 26 1459 26
325 False 26 1459 26
326 True 14 1240 14
326 False 14 1240 14
327 True 14 1236 14
327 False 14 1236 14
328 True 2 184 2
328 False 2 184 2
329 True 21 1465 21
329 False 11 17103 21
330 True 13 1184 13
330 False 13 1184 13
331 True 27 1908 25
331 False 22 1573 25
332 True 2 184 2
332 False 2 184 2
333 True 22 1185 22
333 False 22 1185 22
334 True 5 298 5
334 False 5 298 5
335 True 118 8386 81
335 False 62 10286 81
336 True 218 16464 97
336 False 87 54529 97
337 True 2 184 2
337 False 2 184 2
338 True 8 522 8
338 False 3 57412 8
339 True 20 1654 9
339 False 9 29141 9
340 True 10 951 10
340 False 10 951 10
341 True 5 523 5
341 False 5 523 5
342 True 7 357 7
342 False 7 357 7
343 True 27 1294 27
343 False 27 1294 27
344 True 5 244 5
344 False 5 244 5
345 True 112 7852 99
345 False 80 61968 99
346 True 74 5863 61
346 False 42 61840 61
347 True 122 9328 22
347 False 22 4728 22
348 True 40 2394 40
348 False 30 18032 40
349 True 1 49 1
349 False 1 49 1
350 True 14 1247 14
350 False 14 1247 14
351 True 2 184 2
351 False 2 184 2
352 True 4 187 4
352 False 4 187 4
353 True 2 184 2
353 False 2 184 2
354 True 214 14808 163
354 False 144 31591 163
355 True 20 1654 9
355 False 9 29141 9
356 True 2 94 2
356 False 2 94 2
357 True 40 2580 30
357 False 30 2120 30
358 True 18 863 18
358 False 18 863 18
359 True 27 1294 27
359 False 27 1294 27
360 True 19 1000 19
360 False 19 1000 19
361 True 4 187 4
361 False 4 187 4
362 True 4 219 4
362 False 4 219 4
363 True 8 530 6
363 False 3 195 6
364 True 14 895 14
364 False 4 16533 14
365 True 139 10524 39
365 False 39 5924 39
366 True 9 792 9
366 False 9 792 9
367 True 58 3777 54
367 False 58 3777 54
368 True 4 443 4
368 False 4 443 4
369 True 4 443 4
369 False 4 443 4
370 True 19 1000 19
370 False 19 1000 19
371 True 17 1242 7
371 False 7 782 7
372 True 52 4413 41
372 False 41 31900 41
373 True 9 580 7
373 False 4 245 7
374 True 4 443 4
374 False 4 443 4
375 True 1 48 1
375 False 1 48 1
376 True 4 172 4
376 False 4 172 4
377 True 14 1294 12
377 False 3 14146 12
378 True 6 352 6
378 False 6 352 6
379 True 65 4521 65
379 False 65 4521 65
380 True 4 443 4
380 False 4 443 4
381 True 11 572 11
381 False 11 572 11
382 True 27 1294 27
382 False 27 1294 27
383 True 4 448 4
383 False 4 448 4
384 True 172 12569 71
384 False 72 7969 71
385 True 4 443 4
385 False 4 443 4
386 True 3 165 3
386 False 3 165 3
387 True 5 523 5
387 False 5 523 5
388 True 5 292 5
388 False 5 292 5
389 True 5 299 5
389 False 5 299 5
390 True 4 165 4
390 False 4 165 4
391 True 4 165 4
391 False 4 165 4
392 True 13 1192 13
392 False 13 1192 13
393 True 20 1046 20
393 False 20 1046 20
394 True 14 1294 12
394 False 3 14146 12
395 True 5 523 5
395 False 5 523 5
396 True 2 248 2
396 False 2 248 2
397 True 5 292 5
397 False 5 292 5
398 True 2 184 2
398 False 2 184 2
399 True 19 1520 19
399 False 19 1520 19
ok '{"a": 1, "a": 2}' {'a': 2} dict
ok '[1, 2,3 ]' [1, 2, 3] list
ok ' \n\t[ ] ' [] list
ok '-0' 0 int
ok '-0.0' -0.0 float
ok '1e400' inf float
ok '-1e400' -inf float
ok '1E5' 100000.0 float
ok '0.5e-3' 0.0005 float
ok '12345678901234567890123456789 123456789012345678901234567890 int
ok '"\\ud83d\\ude00"' '😀' str
ok '"\\u00e9\\n\\/"' 'é\n/' str
ok 'NaN' nan float
ok '-Infinity' -inf float
ok 'Infinity' inf float
ok '[NaN, Infinity]' [nan, inf] list
ok 'true' True bool
ok 'null' None NoneType
ok '{"k": {"k": {"k": [[[]]]}}}' {'k': {'k': {'k': [[[]]]}}} dict
ok '"x"' 'x' str
ok '12' 12 int
ok '3.0' 3.0 float
err '[1,]' JSONDecodeError Illegal trailing comma before end of array: line 1 column 3 (char 2)
err '{"a":}' JSONDecodeError Expecting value: line 1 column 6 (char 5)
err '' JSONDecodeError Expecting value: line 1 column 1 (char 0)
err ' ' JSONDecodeError Expecting value: line 1 column 2 (char 1)
err '[1] x' JSONDecodeError Extra data: line 1 column 5 (char 4)
err '"abc' JSONDecodeError Unterminated string starting at: line 1 column 1 (char 0)
err '01' JSONDecodeError Extra data: line 1 column 2 (char 1)
err '1.' JSONDecodeError Extra data: line 1 column 2 (char 1)
err '.5' JSONDecodeError Expecting value: line 1 column 1 (char 0)
err "'a'" JSONDecodeError Expecting value: line 1 column 1 (char 0)
err '{"a" 1}' JSONDecodeError Expecting ':' delimiter: line 1 column 6 (char 5)
err '[1 2]' JSONDecodeError Expecting ',' delimiter: line 1 column 4 (char 3)
err '"\t"' JSONDecodeError Invalid control character at: line 1 column 2 (char 1)
err 'nul' JSONDecodeError Expecting value: line 1 column 1 (char 0)
err '\ufeff[1]' JSONDecodeError Unexpected UTF-8 BOM (decode using utf-8-sig): line 1 column 1 (char 0)
{"1": "c", "2.5": "b", "null": "d", "e": "f"} {"1": "c", "2.5": "b", "null": "d", "e": "f"}
err TypeError keys must be str, int, float, bool or None, not tuple
err TypeError Object of type object is not JSON serializable
err TypeError Object of type set is not JSON serializable
NaN NaN
[Infinity, -Infinity] [Infinity, -Infinity]
err TypeError Object of type bytes is not JSON serializable
10000000000000000000000000000000000000000 10000000000000000000000000000000000000000
[[1, [2, [3, {"k": [4, 5]}]]]] [[1, [2, [3, {"k": [4, 5]}]]]]
ValueError Circular reference detected
1202 True
{"a": 1, "b": [1, 2]} [
  1,
  {
    "a": 2
  }
] {"a":1}
{"a": 1} {"z": 1, "a": 2} true "\u00e9" 1.0
"##
    );
}

#[test]
fn class_subclasses_registry() {
    let src = r##"
class Base:
    registry = []
    def __init_subclass__(cls, **kw):
        super().__init_subclass__(**kw)
        Base.registry.append(cls.__name__)
class A(Base): pass
class B(Base): pass
class C(A): pass
print(Base.registry)
print(Base.__subclasses__())
print(A.__subclasses__(), C.__subclasses__())
def walk(c):
    for s in c.__subclasses__():
        yield s
        yield from walk(s)
print([k.__name__ for k in walk(Base)])
print(issubclass(C, Base))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"['A', 'B', 'C']
[<class '__main__.A'>, <class '__main__.B'>]
[<class '__main__.C'>] []
['A', 'C', 'B']
True
"##
    );
}

#[test]
fn lone_surrogates_and_surrogateescape() {
    let src = r##"
import os
raw = b'ok \xff\xfe bad \xc3\x28 end'
s = raw.decode('utf-8', 'surrogateescape')
print(len(s), ascii(s))
print(s.encode('utf-8', 'surrogateescape') == raw)
print(raw.decode('utf-8', 'replace'), raw.decode('utf-8', 'backslashreplace'), raw.decode('utf-8', 'ignore'))
fn = os.fsdecode(b'caf\xe9.txt'); print(ascii(fn), os.fsencode(fn))
print(ascii(chr(0xd83d)), len(chr(0xd800) + 'a'), '\ud800'.encode('utf-16', 'surrogatepass'), ord('\udc80'), '\udcff' == chr(0xdcff))
print('\ud800'.encode('utf-8', 'surrogatepass'), repr('\udfff x'), ('a\udc80b').encode('utf-8', 'backslashreplace'), ('a\udc80b').encode('ascii', 'surrogateescape'))
try: '\ud800'.encode()
except UnicodeEncodeError as e: print(e)
import json
print(json.dumps('\ud83d'), json.dumps('\ud83d', ensure_ascii=False) == '"\ud83d"', json.loads('"\\ud83d"') == '\ud83d', json.loads('"\\ud83d\\ude00"'))
print(sorted(['\udc80', 'z', 'a']) == ['a', 'z', '\udc80'] or 'order')
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"16 'ok \udcff\udcfe bad \udcc3( end'
True
ok �� bad �( end ok \xff\xfe bad \xc3( end ok  bad ( end
'caf\udce9.txt' b'caf\xe9.txt'
'\ud83d' 2 b'\xff\xfe\x00\xd8' 56448 True
b'\xed\xa0\x80' '\udfff x' b'a\\udc80b' b'a\x80b'
'utf-8' codec can't encode character '\ud800' in position 0: surrogates not allowed
"\ud83d" True True 😀
True
"##
    );
}

#[test]
fn slice_indices_for_custom_sequences() {
    let src = r##"
s = slice(None, None, -1); print(s.indices(5), slice(1, 10).indices(5), slice(-3, None).indices(10), slice(None, -20).indices(4), slice(2, 8, 3).indices(7), slice(None, None, -2).indices(0))
class Seq:
    def __init__(self, d): self.d = d
    def __len__(self): return len(self.d)
    def __getitem__(self, k):
        if isinstance(k, slice):
            return [self.d[i] for i in range(*k.indices(len(self)))]
        return self.d[k]
q = Seq(list(range(10)))
print(q[2:5], q[::-3], q[-2:], q[:100], q[5:2])
try: slice(1, 2, 0).indices(3)
except ValueError as e: print(e)
try: slice(1, 2).indices(-1)
except ValueError as e: print(e)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"(4, -1, -1) (1, 5, 1) (7, 10, 1) (0, 0, 1) (2, 7, 3) (-1, -1, -2)
[2, 3, 4] [9, 6, 3, 0] [8, 9] [0, 1, 2, 3, 4, 5, 6, 7, 8, 9] []
slice step cannot be zero
length should not be negative
"##
    );
}

#[test]
fn hashlib_sha3_matches_cpython() {
    let src = r##"
import hashlib, hmac
for n in ('sha3_224', 'sha3_256', 'sha3_384', 'sha3_512'):
    for m in (b'', b'abc', b'a' * 135, b'a' * 136, b'a' * 137, b'x' * 1000):
        print(n, len(m), getattr(hashlib, n)(m).hexdigest()[:24], hashlib.new(n, m).digest_size, getattr(hashlib, n)().block_size)
print(hmac.new(b'k', b'm', 'sha3_256').hexdigest(), hashlib.pbkdf2_hmac('sha3_256', b'p', b's', 3).hex())
h = hashlib.sha3_256(); h.update(b'ab'); h.update(b'c'); print(h.name, h.hexdigest()[:16], h.copy().hexdigest() == h.hexdigest())
print('sha3_256' in hashlib.algorithms_guaranteed, 'md5' in hashlib.algorithms_available)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"sha3_224 0 6b4e03423667dbb73b6e1545 28 144
sha3_224 3 e642824c3f8cf24ad09234ee 28 144
sha3_224 135 f9f28c21a2b0884bbd3594ca 28 144
sha3_224 136 96136a6a094433b4aa855f16 28 144
sha3_224 137 d0c9e8452b199b5149b9d06e 28 144
sha3_224 1000 2459c65a000e217e65e99924 28 144
sha3_256 0 a7ffc6f8bf1ed76651c14756 32 136
sha3_256 3 3a985da74fe225b2045c172d 32 136
sha3_256 135 8094bb53c44cfb1e67b7c304 32 136
sha3_256 136 3fc5559f14db8e453a0a3091 32 136
sha3_256 137 f8d6846cedd2ccfadf15c587 32 136
sha3_256 1000 9392d9e39b54fd1ee9f46551 32 136
sha3_384 0 0c63a75b845e4f7d01107d85 48 104
sha3_384 3 ec01498288516fc926459f58 48 104
sha3_384 135 a2d51907c0611e25c058f067 48 104
sha3_384 136 cbbcb466417a2f6d466479bb 48 104
sha3_384 137 8a9e401af96cfcdc6ee9e848 48 104
sha3_384 1000 95a2b7f72e41a03e3ca1f012 48 104
sha3_512 0 a69f73cca23a9ac5c8b567dc 64 72
sha3_512 3 b751850b1a57168a5693cd92 64 72
sha3_512 135 4be1e70276f9122f470a54c2 64 72
sha3_512 136 e50392c91ed95768c8dcf52a 64 72
sha3_512 137 c1a51bff785ff8443c873d0f 64 72
sha3_512 1000 71a4118f314ec6f5bf8ba025 64 72
1b92d4a22666154356c30c31595306c7db17a27a0fb02efae867f69018d3a876 4f24af1949a4edb7253a1913084c2688ec45787f06afb2ed9ba5d87d246dc739
sha3_256 3a985da74fe225b2 True
True True
"##
    );
}

#[test]
fn mmap_anonymous_and_int_bit_count() {
    let src = r##"
import mmap
m = mmap.mmap(-1, 32)
m.write(b'hello\nworld\n'); m.seek(0)
print(m.readline(), m.tell(), m.find(b'world'), m.rfind(b'o'), len(m), m[0], m[1:4], m[-1])
m[0:5] = b'HELLO'; m[6] = ord('W'); m.seek(0); print(m.read(12), m.read_byte())
m.move(0, 6, 5); print(m[:12], m.closed)
try: m.size()
except OSError as e: print(type(e).__name__, e)
try: m[0:2] = b'abc'
except IndexError as e: print('IndexError', e)
try: m.seek(100)
except ValueError as e: print('ValueError', e)
try: m.write(b'x' * 40)
except ValueError as e: print('ValueError', e)
with m: pass
try: m[0]
except ValueError as e: print('ValueError', e)
print((255).bit_count(), (-7).bit_count(), (2**70 + 1).bit_count(), True.bit_count())
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"b'hello\n' 6 6 7 32 104 b'ell' 0
b'HELLO\nWorld\n' 0
b'World\nWorld\n' False
OSError [Errno 9] Bad file descriptor
IndexError mmap slice assignment is wrong size
ValueError seek out of range
ValueError data out of range
ValueError mmap closed or invalid
8 3 2 1
"##
    );
}

#[test]
fn idna_punycode_and_titlecase() {
    let src = r##"
for s in ('münchen.de', 'bücher.example', 'ação.com.br', '日本語.jp', 'ASCII.com', 'Ünï.Çom.', 'пример.рф', '😀.fm'):
    e = s.encode('idna'); print(s, e, e.decode('idna'))
for s in ('münchen', 'ação', '日本語', 'abc', 'a-b', '😀', 'Hello, 世界'):
    p = s.encode('punycode'); print(s, p, p.decode('punycode') == s)
print('xn--MNCHEN-3YA.de'.encode().decode('idna'), 'ǅ'.istitle(), 'ǆa'.title(), int('١٢٣') + 1, int(' ٤٢ '), 'ﬁx'.title())
try: 'a..b'.encode('idna')
except UnicodeError as e: print('UnicodeError', e)
import urllib.parse as up
print(up.urlsplit('http://münchen.de/x').hostname)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"münchen.de b'xn--mnchen-3ya.de' münchen.de
bücher.example b'xn--bcher-kva.example' bücher.example
ação.com.br b'xn--ao-siap.com.br' ação.com.br
日本語.jp b'xn--wgv71a119e.jp' 日本語.jp
ASCII.com b'ASCII.com' ASCII.com
Ünï.Çom. b'xn--n-nga1b.xn--om-3ia.' ünï.çom.
пример.рф b'xn--e1afmkfd.xn--p1ai' пример.рф
😀.fm b'xn--e28h.fm' 😀.fm
münchen b'mnchen-3ya' True
ação b'ao-siap' True
日本語 b'wgv71a119e' True
abc b'abc-' True
a-b b'a-b-' True
😀 b'e28h' True
Hello, 世界 b'Hello, -dz3ki820a' True
MüNCHEN.de True ǅa 124 42 Fix
UnicodeError 'idna' codec can't encode character '\x2e' in position 2: label empty
münchen.de
"##
    );
}

#[test]
fn regex_text_and_unicode_properties() {
    let src = r##"
import re, string, textwrap, unicodedata, locale, difflib
t = 'Olá, João! Preço: R$ 1.234,56 (20% off) em 2026-10-06; e-mail: ana.silva+x@ex.com.br, tel (11) 98765-4321. ÇÃO ção İstanbul ß'
print(re.findall(r'(?<=R\$ )[\d.,]+', t), re.findall(r'\b\w+@\w+(?:\.\w+)+', t), re.sub(r'(\d+)-(\d+)-(\d+)', r'\3/\2/\1', t)[:60])
print(re.findall(r'(?i)ção', t), re.findall(r'[^\W\d_]+', t)[:6], re.split(r'[,;]\s*', t)[:3], re.findall(r'(\w)\1', 'aabbcd'))
print(re.match(r'(?P<d>\d{4})-(?P<m>\d\d)', '2026-10-06').groupdict(), re.fullmatch(r'\w+', 'ação'), re.search(r'(?<!\d)\d{2}(?!\d)', 'a1 22 333').group())
print(re.sub(r'\s+', ' ', 'a \n\t b'), re.escape('a.b*c'), re.compile(r'x*', re.M).sub('-', 'abc'), re.findall(r'^\w', 'ab\ncd', re.M), re.subn('a', 'b', 'aaa', count=2))
print([m.span() for m in re.finditer(r'\b[A-ZÀ-Ý]{2,}\b', t)], re.findall(r'(?x) (\d+) \s* % ', t), re.sub(r'(?P<w>\w+)', lambda m: m['w'][::-1], 'ab cd'))
print('{a:>8}|{b:<6}|{c:^7.2f}|{d!r}'.format_map({'a': 'x', 'b': 'y', 'c': 3.14159, 'd': 'q'}), string.Formatter().parse('a{b!r:>3}c').__next__())
print(t.upper(), t.lower()[-8:], t.casefold()[-8:], 'ß'.upper(), 'İ'.lower().encode(), 'ǆ'.title(), 'ﬁ'.upper(), 'Σας'.lower(), 'ΑΣ'.lower())
print(unicodedata.normalize('NFKC', 'ﬁ²①'), unicodedata.category('ç'), unicodedata.east_asian_width('日'), unicodedata.numeric('½'), unicodedata.mirrored("("), unicodedata.bidirectional('א'), unicodedata.combining('́'))
print(textwrap.shorten('palavra ' * 20, 30), textwrap.indent('a\nb', '> '), textwrap.wrap('日本語' * 10, 8), len(textwrap.dedent('   a\n    b')))
print('a\tb'.expandtabs(4), 'abc'.center(9, '*'), '%-5s|%05d|%x|%e|%c|%%' % ('ab', 42, 255, 12345.678, 65), 'x'.zfill(4), '-5'.zfill(4), 'a,b,,c'.split(','), 'a b  c'.split(None, 1), 'abc'.partition('b'), 'aXbXc'.rsplit('X', 1))
print(sorted(['é', 'e', 'z', 'a', 'É', 'Z'], key=str.casefold), sorted(['b10', 'b9', 'b2'], key=lambda s: (s[0], int(s[1:]))), 'ação'.isalpha(), '²'.isdigit(), '²'.isdecimal(), '٣'.isdecimal(), int('٣'), 'a1'.isalnum(), ' '.isspace(), 'ǅ'.istitle())
print(f'{"x":*^9}', f'{3.0:g}', f'{1e-5}', f'{12345.6789:_.2f}', f'{255:08b}', f'{-3:+}', f'{"a" "b"!r:>6}', f'{0.1+0.2:.17g}', f'{10**20:,}', f'{1/3:.3%}', f'{12:c}')
print(repr('á'), repr('\x00\x7f​\U0001F600'), ascii('é'), 'é'.encode('idna') if False else '', 'münchen.de'.encode('idna'), b'xn--mnchen-3ya.de'.decode('idna'))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"['1.234,56'] ['x@ex.com.br'] Olá, João! Preço: R$ 1.234,56 (20% off) em 06/10/2026; e-mai
['ÇÃO', 'ção'] ['Olá', 'João', 'Preço', 'R', 'off', 'em'] ['Olá', 'João! Preço: R$ 1.234', '56 (20% off) em 2026-10-06'] ['a', 'b']
{'d': '2026', 'm': '10'} <re.Match object; span=(0, 4), match='ação'> 22
a b a\.b\*c -a-b-c- ['a', 'c'] ('bba', 2)
[(107, 110)] ['20'] ba dc
       x|y     | 3.14  |'q' ('a', 'b', '>3', 'r')
OLÁ, JOÃO! PREÇO: R$ 1.234,56 (20% OFF) EM 2026-10-06; E-MAIL: ANA.SILVA+X@EX.COM.BR, TEL (11) 98765-4321. ÇÃO ÇÃO İSTANBUL SS tanbul ß anbul ss SS b'i\xcc\x87' ǅ FI σας ας
fi21 Ll W 0.5 1 R 230
palavra palavra palavra [...] > a
> b ['日本語日本語日本', '語日本語日本語日', '本語日本語日本語', '日本語日本語'] 4
a   b ***abc*** ab   |00042|ff|1.234568e+04|A|% 000x -005 ['a', 'b', '', 'c'] ['a', 'b  c'] ('a', 'b', 'c') ['aXb', 'c']
['a', 'e', 'z', 'Z', 'é', 'É'] ['b2', 'b9', 'b10'] True True False True 3 True True True
****x**** 3 1e-05 12_345.68 11111111 -3   'ab' 0.30000000000000004 100,000,000,000,000,000,000 33.333% 
'á' '\x00\x7f\u200b😀' '\xe9'  b'xn--mnchen-3ya.de' münchen.de
"##
    );
}

#[test]
fn float_unicode_digits_and_decomposition() {
    let src = r##"
import timeit, unicodedata
print(float('١٢.٥'), float(' ٣ '), unicodedata.decomposition('é'), unicodedata.decomposition('a'), unicodedata.decomposition('ﬁ'))
print(len(timeit.repeat("1+1", number=10, repeat=3)))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"12.5 3.0 0065 0301  <compat> 0066 0069
3
"##
    );
}

#[test]
fn exec_separate_locals_does_not_leak() {
    let src = r##"
g = {}
for i in range(3):
    ns = {}
    exec(compile('def inner(): return 1', '<s>', 'exec'), g, ns)
    print(i, sorted(ns), 'inner' in g)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"0 ['inner'] False
1 ['inner'] False
2 ['inner'] False
"##
    );
}

#[test]
fn cprofile_counts_python_functions() {
    let src = r##"
import cProfile, pstats, io
def leaf(n): return sum(range(n))
def mid(n):
    t = 0
    for _ in range(20): t += leaf(n)
    return t
def fib(n): return n if n < 2 else fib(n - 1) + fib(n - 2)
def main():
    mid(100); fib(12); return mid(10)
pr = cProfile.Profile(); pr.runcall(main); pr.create_stats()
print(sorted((k[2], v[0], v[1]) for k, v in pr.stats.items() if k[2] in ('leaf', 'mid', 'fib', 'main')))
print(sorted((k[2], sorted(kk[2] for kk in v[4])) for k, v in pr.stats.items() if k[2] in ('leaf', 'fib')))
s = io.StringIO(); pstats.Stats(pr, stream=s).sort_stats('cumulative').print_stats(3)
print('ncalls' in s.getvalue(), 'cumtime' in s.getvalue())
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"[('fib', 1, 465), ('leaf', 40, 40), ('main', 1, 1), ('mid', 2, 2)]
[('fib', ['fib', 'main']), ('leaf', ['mid'])]
True True
"##
    );
}

#[test]
fn settrace_call_line_exception_return() {
    let src = r##"
import sys
BASE = sys._getframe().f_lineno
n = []
def tr(frame, event, arg):
    n.append((event, frame.f_code.co_name, frame.f_lineno - BASE, arg if event == 'return' else (arg[0].__name__ if event == 'exception' else None)))
    return tr
def f(a):
    b = a + 1
    if b > 1:
        b *= 2
    return b
def g():
    x = f(1)
    try:
        1 / 0
    except ZeroDivisionError:
        pass
    return x
def boom():
    raise ValueError('x')
sys.settrace(tr); g()
try: boom()
except ValueError: pass
sys.settrace(None)
for e in n: print(e)
print(sys.gettrace())
only_calls = []
def gl(frame, event, arg):
    only_calls.append(frame.f_code.co_name)
sys.settrace(gl); f(5); g(); sys.settrace(None)
print(only_calls)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"('call', 'g', 10, None)
('line', 'g', 11, None)
('call', 'f', 5, None)
('line', 'f', 6, None)
('line', 'f', 7, None)
('line', 'f', 8, None)
('line', 'f', 9, None)
('return', 'f', 9, 4)
('line', 'g', 12, None)
('line', 'g', 13, None)
('exception', 'g', 13, 'ZeroDivisionError')
('line', 'g', 14, None)
('line', 'g', 15, None)
('line', 'g', 16, None)
('return', 'g', 16, 4)
('call', 'boom', 17, None)
('line', 'boom', 18, None)
('exception', 'boom', 18, 'ValueError')
('return', 'boom', 18, None)
None
['f', 'g', 'f']
"##
    );
}
#[test]
fn dataclass_slots_and_get_overloads() {
    let src = r##"
import dataclasses, typing
@dataclasses.dataclass(slots=True)
class E:
    a: int
    b: int = 2
e = E(1); print(e, E.__slots__, hasattr(e, '__dict__'))
try: e.c = 1
except AttributeError: print('no c')
@typing.overload
def f(x: int) -> int: ...
def f(x): return x
print(len(typing.get_overloads(f)), f(3))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"E(a=1, b=2) ('a', 'b') False
no c
1 3
"##
    );
}

#[test]
fn counter_unary_ordereddict_fromkeys_iter_unpack() {
    let src = r##"
import collections, struct
c = collections.Counter(a=2, b=-1)
print(+c, -c, collections.OrderedDict.fromkeys('ab', 0))
it = struct.iter_unpack('<H', b'\x01\x00\x02\x00'); print(next(it), list(it))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"Counter({'a': 2}) Counter({'b': 1}) OrderedDict({'a': 0, 'b': 0})
(1,) [(2,)]
"##
    );
}

#[test]
fn builtin_subclass_alt_constructors() {
    let src = r##"
class D(dict): pass
d = D.fromkeys('ab', 1); print(type(d).__name__, d)
class L(list):
    def total(self): return sum(self)
l = L([1, 2]); l += [3]; print(type(l).__name__, l.total(), l[1:], type(l + [4]).__name__)
class S(str):
    def shout(self): return self.upper() + '!'
print(S('hi').shout(), type(S('a') + 'b').__name__, S.maketrans('a', 'b'))
class I(int):
    def __repr__(self): return f'I({int(self)})'
print(I(3), I(3) + 1, I.from_bytes(b'\x01', 'big'), type(I.from_bytes(b'\x01', 'big')).__name__)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"D {'a': 1, 'b': 1}
L 6 [2, 3] list
HI! str {97: 98}
I(3) 4 I(1) I
"##
    );
}

#[test]
fn exception_notes_and_traceback_exception_chain() {
    let src = r##"
import traceback
class M(Exception): pass
for e in (ValueError('x'), M('y')):
    e.add_note('n1'); e.add_note('dois\nlinhas')
    print(e.__notes__, traceback.format_exception_only(e))
try:
    try: {}['k']
    except KeyError as e: raise RuntimeError('wrap') from e
except RuntimeError as e:
    tb = traceback.TracebackException.from_exception(e)
    print(type(tb.__cause__).__name__, tb.__cause__.exc_type.__name__, tb.__context__ is not None, tb.__suppress_context__)
try: raise M('fim')
except M as e: print(''.join(traceback.format_exception_only(e)).strip())
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"['n1', 'dois\nlinhas'] ['ValueError: x\n', 'n1\n', 'dois\n', 'linhas\n']
['n1', 'dois\nlinhas'] ['M: y\n', 'n1\n', 'dois\n', 'linhas\n']
TracebackException KeyError False True
M: fim
"##
    );
}

#[test]
fn http_server_forever_in_thread() {
    let src = r##"
import http.server, threading, json, urllib.request, urllib.error, urllib.parse, socketserver, http.client, socket
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def _send(self, code, obj, headers=()):
        body = json.dumps(obj).encode(); self.send_response(code); self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        for k, v in headers: self.send_header(k, v)
        self.end_headers(); self.wfile.write(body)
    def do_GET(self):
        u = urllib.parse.urlsplit(self.path)
        if u.path == '/items': self._send(200, {'q': urllib.parse.parse_qs(u.query), 'ua': self.headers.get('User-Agent', '')[:6]})
        elif u.path == '/redir': self.send_response(302); self.send_header('Location', '/items?r=1'); self.end_headers()
        else: self._send(404, {'err': 'nf'})
    def do_POST(self):
        n = int(self.headers['Content-Length']); data = json.loads(self.rfile.read(n))
        self._send(201, {'got': data, 'ct': self.headers.get_content_type()}, [('X-Id', '7')])
srv = socketserver.ThreadingTCPServer(('127.0.0.1', 0), H); srv.daemon_threads = True
port = srv.server_address[1]; t = threading.Thread(target=srv.serve_forever, daemon=True); t.start()
base = f'http://127.0.0.1:{port}'
with urllib.request.urlopen(base + '/items?a=1&a=2') as r: print(r.status, r.headers['Content-Type'], json.load(r))
req = urllib.request.Request(base + '/items', data=json.dumps({'x': [1, 2]}).encode(), headers={'Content-Type': 'application/json'}, method='POST')
with urllib.request.urlopen(req, timeout=5) as r: print(r.status, r.getheader('X-Id'), json.loads(r.read()))
with urllib.request.urlopen(base + '/redir') as r: print(r.status, r.url.endswith('/items?r=1'), json.load(r)['q'])
try: urllib.request.urlopen(base + '/nada')
except urllib.error.HTTPError as e: print('HTTPError', e.code, e.reason, json.loads(e.read()))
try: urllib.request.urlopen('http://127.0.0.1:1/x', timeout=2)
except urllib.error.URLError as e: print('URLError', type(e.reason).__name__)
c = http.client.HTTPConnection('127.0.0.1', port, timeout=5); c.request('GET', '/items?z=9', headers={'User-Agent': 'agente/1'}); resp = c.getresponse()
print(resp.status, resp.reason, json.loads(resp.read())); c.close()
s = socket.create_connection(('127.0.0.1', port)); s.sendall(b'GET /items HTTP/1.0\r\nHost: x\r\n\r\n'); data = b''
while (chunk := s.recv(4096)): data += chunk
s.close(); print(data.split(b'\r\n')[0], data.endswith(b'}'))
srv.shutdown(); srv.server_close(); print('fim')
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"200 application/json {'q': {'a': ['1', '2']}, 'ua': 'Python'}
201 7 {'got': {'x': [1, 2]}, 'ct': 'application/json'}
200 True {'r': ['1']}
HTTPError 404 Not Found {'err': 'nf'}
URLError ConnectionRefusedError
200 OK {'q': {'z': ['9']}, 'ua': 'agente'}
b'HTTP/1.0 200 OK' True
fim
"##
    );
}

#[test]
fn csv_stats_report_decimal() {
    let src = r##"
import csv, io, statistics, itertools, operator, textwrap, decimal, fractions, heapq, bisect, json, pathlib, tempfile

data = """nome,dept,salario,entrada
Ana,TI,8500.50,2020-03-01
Bruno,RH,4200,2019-07-15
Carla,TI,9100,2021-01-10
Davi,Vendas,5300.25,2018-11-30
Eva,RH,4800,2022-05-05
Fábio,Vendas,6100,2020-09-09
"""
rows = list(csv.DictReader(io.StringIO(data)))
for r in rows:
    r['salario'] = decimal.Decimal(r['salario'])
rows.sort(key=operator.itemgetter('dept'))
for dept, grp in itertools.groupby(rows, key=operator.itemgetter('dept')):
    g = list(grp)
    sal = [float(x['salario']) for x in g]
    print(f"{dept:<8}|{len(g):>3}|{statistics.mean(sal):>10.2f}|{statistics.median(sal):>10,.2f}|{sum(x['salario'] for x in g)}")
print(statistics.stdev(float(r['salario']) for r in rows).__round__(3))
print(statistics.quantiles([float(r['salario']) for r in rows], n=4))
print(heapq.nlargest(2, rows, key=lambda r: r['salario'])[0]['nome'])
s = sorted(float(r['salario']) for r in rows)
print(bisect.bisect_left(s, 5000), fractions.Fraction('0.125') + fractions.Fraction(1, 3))
out = io.StringIO()
w = csv.DictWriter(out, fieldnames=['nome', 'salario'], extrasaction='ignore', quoting=csv.QUOTE_NONNUMERIC)
w.writeheader()
w.writerows(rows[:2])
print(repr(out.getvalue()))
print(textwrap.fill('O relatório consolidado de salários por departamento mostra variação ' * 2, width=40, initial_indent='> ', subsequent_indent='  '))
print(textwrap.shorten('um texto bem longo que precisa ser encurtado agora', width=25, placeholder=' [...]'))
f = io.StringIO(newline='')
csv.writer(f, delimiter=';', lineterminator='\n').writerows([['a', 'b;c'], [1, 'x"y']])
print(repr(f.getvalue()))
print(list(csv.reader(io.StringIO(f.getvalue()), delimiter=';')))
print(json.dumps({'total': str(sum(r['salario'] for r in rows))}, indent=2, ensure_ascii=False))
print(csv.Sniffer().sniff('a;b;c\n1;2;3\n').delimiter, csv.Sniffer().has_header(data))
print(decimal.Decimal('8500.50').quantize(decimal.Decimal('1'), rounding=decimal.ROUND_HALF_EVEN), statistics.mode(['a', 'b', 'a']))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"RH      |  2|   4500.00|  4,500.00|9000
TI      |  2|   8800.25|  8,800.25|17600.50
Vendas  |  2|   5700.12|  5,700.12|11400.25
2018.662
[4650.0, 5700.125, 8650.375]
Carla
2 11/24
'"nome","salario"\r\n"Bruno",4200\r\n"Eva",4800\r\n'
> O relatório consolidado de salários
  por departamento mostra variação O
  relatório consolidado de salários por
  departamento mostra variação
um texto bem longo [...]
'a;"b;c"\n1;"x""y"\n'
[['a', 'b;c'], ['1', 'x"y']]
{
  "total": "38000.75"
}
; True
8500 a
"##
    );
}

#[test]
fn core_closures_match_flag_dispatch() {
    let src = r##"
from dataclasses import dataclass, field, asdict, replace
from functools import reduce, partial, cached_property, singledispatch, total_ordering, wraps
from typing import NamedTuple
import enum, contextlib

def counter():
    n = 0
    def inc(step=1):
        nonlocal n
        n += step
        return n
    return inc
c = counter(); c(); c(5); print(c(), c(0))

def acc():
    total = 0
    while True:
        x = yield total
        if x is None:
            return total
        total += x
def wrap():
    r = yield from acc()
    yield f'fim {r}'
g = wrap(); next(g); g.send(3); print(g.send(4), g.send(None))

class Cor(enum.Flag):
    R = enum.auto(); G = enum.auto(); B = enum.auto()
print(Cor.R | Cor.B, list(Cor.R | Cor.G), Cor(6).name, ~Cor.R)

@dataclass(order=True, frozen=True)
class Item:
    prioridade: int
    nome: str = field(compare=False)
    tags: tuple = ()
it = [Item(2, 'b'), Item(1, 'a', ('x',))]
print(sorted(it)[0], asdict(it[1]), replace(it[0], nome='z'), hash(it[0]) == hash(Item(2, 'q')))

def descr(obj):
    match obj:
        case {'tipo': 'pt', 'x': int(x), 'y': int(y)} if x == y:
            return f'diag {x}'
        case Item(prioridade=p, nome=n) if p > 1:
            return f'item {n}'
        case [first, *rest] if rest:
            return f'lista {first}+{len(rest)}'
        case str() | bytes() as s:
            return f'texto {s!r}'
        case _:
            return 'outro'
for o in [{'tipo': 'pt', 'x': 2, 'y': 2}, Item(5, 'k'), [1, 2, 3], b'ab', 3.5]:
    print(descr(o))

class P(NamedTuple):
    x: int
    y: int = 0
    def norma(self): return (self.x ** 2 + self.y ** 2) ** .5
p = P(3, 4); print(p, p.norma(), p._replace(y=0), P._fields, P._field_defaults)

@singledispatch
def fmt(v): return 'gen'
@fmt.register
def _(v: int): return 'int'
@fmt.register(list)
def _(v): return 'list'
print(fmt(1), fmt([1]), fmt('a'), fmt(True))

@total_ordering
class V:
    def __init__(s, v): s.v = v
    def __eq__(s, o): return s.v == o.v
    def __lt__(s, o): return s.v < o.v
print(V(1) >= V(1), V(2) > V(1), max(V(3), V(7)).v)

class Lazy:
    @cached_property
    def big(self):
        print('calc'); return 42
z = Lazy(); print(z.big, z.big, 'big' in z.__dict__)

def deco(f):
    @wraps(f)
    def w(*a, **k):
        return f(*a, **k) * 2
    return w
@deco
def soma(a, b=1, *, c=0):
    """doc"""
    return a + b + c
print(soma(1, c=2), soma.__name__, soma.__doc__, soma.__wrapped__(1))
print(reduce(lambda a, b: a * b, range(1, 6)), partial(int, base=2)('101'))
with contextlib.suppress(KeyError), contextlib.ExitStack() as st:
    st.callback(print, 'saindo')
    {}['x']
print([(i, j) for i in range(3) for j in range(i) if (i + j) % 2], {k: v for k, v in zip('ab', 'cd')})
print((lambda *a, **k: (a, sorted(k)))(1, 2, z=1, a=2), [*range(2), *'ab'], {**{'a': 1}, 'b': 2})
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"7 7
7 fim 7
Cor.R|B [<Cor.R: 1>, <Cor.G: 2>] G|B Cor.G|B
Item(prioridade=1, nome='a', tags=('x',)) {'prioridade': 1, 'nome': 'a', 'tags': ('x',)} Item(prioridade=2, nome='z', tags=()) True
diag 2
item k
lista 1+2
texto b'ab'
outro
P(x=3, y=4) 5.0 P(x=3, y=0) ('x', 'y') {'y': 0}
int list gen int
True True 7
calc
42 42 True
8 soma doc 2
120 5
saindo
[(1, 0), (2, 1)] {'a': 'c', 'b': 'd'}
((1, 2), ['a', 'z']) [0, 1, 'a', 'b'] {'a': 1, 'b': 2}
"##
    );
}

#[test]
fn cli_argparse_logs_config() {
    let src = r##"
import argparse, logging, sys, json, re, subprocess, shlex, os, textwrap, configparser, io, string, pprint
from collections import Counter, defaultdict, OrderedDict, ChainMap
from itertools import chain, islice, accumulate, pairwise, batched, zip_longest, product, combinations

p = argparse.ArgumentParser(prog='ferramenta', description='Analisa logs.')
p.add_argument('arquivos', nargs='*', default=['-'])
p.add_argument('-n', '--top', type=int, default=3, help='quantos mostrar (padrão: %(default)s)')
p.add_argument('--nivel', choices=['INFO', 'WARN', 'ERROR'], action='append')
p.add_argument('-v', '--verbose', action='count', default=0)
sub = p.add_subparsers(dest='cmd')
r = sub.add_parser('resumo'); r.add_argument('--json', action='store_true')
a = p.parse_args(['-n', '2', '--nivel', 'ERROR', '--nivel', 'WARN', '-vv', 'x.log', 'resumo', '--json'])
print(a)
print(p.format_usage().strip())
print(repr(p.parse_args([]).nivel))
log = """2026-10-06 10:00:01 INFO api GET /users 200 12ms
2026-10-06 10:00:02 ERROR db timeout após 3000ms
2026-10-06 10:00:03 WARN api GET /items 429 5ms
2026-10-06 10:00:04 ERROR api POST /orders 500 87ms
2026-10-06 10:00:05 INFO api GET /users 200 9ms"""
pat = re.compile(r'(?P<ts>\S+ \S+) (?P<lvl>\w+) (?P<src>\w+) (?P<msg>.*)')
recs = [m.groupdict() for m in map(pat.match, log.splitlines()) if m]
c = Counter(r['lvl'] for r in recs); print(c.most_common(), c.total())
by = defaultdict(list)
for r in recs: by[r['src']].append(r['msg'])
pprint.pprint(dict(by), width=60)
ms = [int(x) for x in re.findall(r'(\d+)ms', log)]
print(list(accumulate(ms)), list(pairwise(ms[:3])), list(batched(ms, 2)), max(ms, key=abs))
print(list(zip_longest('ab', [1], fillvalue='-')), len(list(product('ab', repeat=3))), list(combinations(range(4), 2))[:3])
cfg = configparser.ConfigParser()
cfg.read_string('[db]\nhost = localhost\nport = 5432\n[api]\ntimeout = 3.5\ndebug = yes\n')
print(cfg['db'].getint('port'), cfg.getfloat('api', 'timeout'), cfg.getboolean('api', 'debug'), cfg.sections())
s = io.StringIO(); cfg.write(s); print(s.getvalue().strip())
logging.basicConfig(stream=sys.stdout, level=logging.DEBUG, format='%(levelname)-5s %(name)s: %(message)s')
lg = logging.getLogger('ferr.sub')
lg.debug('d %s', 1); lg.warning('w %d itens', 3)
try:
    1 / 0
except ZeroDivisionError:
    lg.exception('falhou')
print(shlex.split("sh -c 'echo $0 \"$1\"; exit 3' a 'b c'")[2], shlex.join(['a b', "c'd"]), shlex.quote('x y'))
print(string.Template('$a-${b}').safe_substitute(a=1), ChainMap({'a': 1}, {'a': 2, 'b': 3})['b'])
print(textwrap.dedent('''\
    linha 1
      linha 2
'''), end='')
print(json.dumps(OrderedDict(z=1, a=[1, {'k': None}]), separators=(',', ':')))
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"Namespace(arquivos=['x.log'], top=2, nivel=['ERROR', 'WARN'], verbose=2, cmd='resumo', json=True)
usage: ferramenta [-h] [-n TOP] [--nivel {INFO,WARN,ERROR}] [-v]
                  [arquivos ...] {resumo} ...
None
[('INFO', 2), ('ERROR', 2), ('WARN', 1)] 5
{'api': ['GET /users 200 12ms',
         'GET /items 429 5ms',
         'POST /orders 500 87ms',
         'GET /users 200 9ms'],
 'db': ['timeout após 3000ms']}
[12, 3012, 3017, 3104, 3113] [(12, 3000), (3000, 5)] [(12, 3000), (5, 87), (9,)] 3000
[('a', 1), ('b', '-')] 8 [(0, 1), (0, 2), (0, 3)]
5432 3.5 True ['db', 'api']
[db]
host = localhost
port = 5432

[api]
timeout = 3.5
debug = yes
DEBUG ferr.sub: d 1
WARNING ferr.sub: w 3 itens
ERROR ferr.sub: falhou
Traceback (most recent call last):
  File "<string>", line 39, in <module>
ZeroDivisionError: division by zero
echo $0 "$1"; exit 3 'a b' 'c'"'"'d' 'x y'
1-${b} 3
linha 1
  linha 2
{"z":1,"a":[1,{"k":null}]}
"##
    );
}

#[test]
fn private_name_mangling() {
    let src = r##"
class A:
    __cls = 1
    def __init__(self, __p=5):
        self.__x = __p
        self.__y__ = 2
        self._A__z = 3
    def __m(self): return 'm'
    def call(self, **kw): return kw
    def use(self):
        import os as __os
        __loc = 7
        return self.__m(), self.call(__k=1), __loc, __os.sep, A.__cls, getattr(self, '__x', 'nao')
    class __Inner:
        def f(self): self.__q = 1; return vars(self)
class ___:
    def g(self): self.__w = 1; return vars(self)
class _B:
    def g(self): self.__w = 1; return vars(self)
a = A()
print(sorted(vars(a)), sorted(k for k in vars(A) if 'A' in k or k.startswith('__c')))
print(a.use())
print(A._A__Inner().f(), ___().g(), _B().g())
print(A.__init__.__code__.co_varnames)
def outer():
    class C:
        def h(self): return lambda: self.__v
    return C
print(outer().h.__code__.co_names if hasattr(outer().h, '__code__') else '')
c = outer()(); c._C__v = 9; print(c.h()())
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"['_A__x', '_A__z', '__y__'] ['_A__Inner', '_A__cls', '_A__m']
('m', {'__k': 1}, 7, '/', 1, 'nao')
{'_Inner__q': 1} {'__w': 1} {'_B__w': 1}
('self', '_A__p')
()
9
"##
    );
}

#[test]
fn dict_view_set_comparisons() {
    let src = r##"
d = {'a': 1, 'b': 2}
k = d.keys()
print(k >= {'a'}, k > {'a'}, k <= {'a', 'b', 'c'}, k < {'a', 'b'}, {'a'} <= k, {'a', 'b', 'c'} >= k, k == {'b', 'a'})
print(d.items() >= {('a', 1)}, d.items() <= {('a', 1)}, k >= d.keys(), k < frozenset('abc'))
for bad in ([1], 'ab'):
    try:
        k < bad
    except TypeError as e:
        print(e)
print(k == ['a', 'b'], k != 'ab')
try:
    d.values() < {1}
except TypeError as e:
    print(e)
"##;
    let o = crate::run_source(src);
    assert_eq!(o.status, 0, "{}", o.stderr);
    assert_eq!(
        String::from_utf8(o.stdout).unwrap(),
        r##"True True True False True True True
True False True True
'<' not supported between instances of 'dict_keys' and 'list'
'<' not supported between instances of 'dict_keys' and 'str'
False True
'<' not supported between instances of 'dict_values' and 'set'
"##
    );
}
