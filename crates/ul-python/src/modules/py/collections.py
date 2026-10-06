"""collections do sandbox (Python embutido)."""

from itertools import chain, repeat


def namedtuple(typename, field_names, *, rename=False, defaults=None, module=None):
    """Returns a new subclass of tuple with named fields.

    >>> Point = namedtuple('Point', ['x', 'y'])
    >>> Point.__doc__                   # docstring for the new class
    'Point(x, y)'
    >>> p = Point(11, y=22)             # instantiate with positional args or keywords
    >>> p[0] + p[1]                     # indexable like a plain tuple
    33
    >>> x, y = p                        # unpack like a regular tuple
    >>> x, y
    (11, 22)
    >>> p.x + p.y                       # fields also accessible by name
    33
    >>> d = p._asdict()                 # convert to a dictionary
    >>> d['x']
    11
    >>> Point(**d)                      # convert from a dictionary
    Point(x=11, y=22)
    >>> p._replace(x=100)               # _replace() is like str.replace() but targets named fields
    Point(x=100, y=22)

    """
    if isinstance(field_names, str):
        field_names = field_names.replace(',', ' ').split()
    fields = tuple(field_names)
    if rename:
        seen = set()
        renamed = []
        for i, name in enumerate(fields):
            if (not name.isidentifier()) or name.startswith('_') or name in seen:
                name = '_%d' % i
            seen.add(name)
            renamed.append(name)
        fields = tuple(renamed)
    for name in (typename,) + fields:
        if not isinstance(name, str):
            raise TypeError('Type names and field names must be strings')
        if not name.isidentifier():
            raise ValueError('Type names and field names must be valid identifiers: %r' % (name,))
    seen = set()
    for name in fields:
        if name.startswith('_'):
            raise ValueError('Field names cannot start with an underscore: %r' % (name,))
        if name in seen:
            raise ValueError('Encountered duplicate field name: %r' % (name,))
        seen.add(name)
    defaults = tuple(defaults) if defaults is not None else ()
    if len(defaults) > len(fields):
        raise TypeError('Got more default values than field names')
    default_map = dict(zip(fields[len(fields) - len(defaults):], defaults))
    holder = []

    def __new__(cls, *args, **kwargs):
        if len(args) > len(fields):
            raise TypeError('%s.__new__() takes %d positional arguments but %d were given'
                            % (typename, len(fields) + 1, len(args) + 1))
        values = list(args)
        missing = []
        for name in fields[len(args):]:
            if name in kwargs:
                values.append(kwargs.pop(name))
            elif name in default_map:
                values.append(default_map[name])
            else:
                missing.append(name)
        if kwargs:
            raise TypeError("%s.__new__() got an unexpected keyword argument '%s'"
                            % (typename, next(iter(kwargs))))
        if missing:
            names = ["'%s'" % m for m in missing]
            if len(names) == 1:
                text = names[0]
            elif len(names) == 2:
                text = names[0] + ' and ' + names[1]
            else:
                text = ', '.join(names[:-1]) + ', and ' + names[-1]
            raise TypeError('%s.__new__() missing %d required positional argument%s: %s'
                            % (typename, len(missing), '' if len(missing) == 1 else 's', text))
        return super(holder[0], cls).__new__(cls, tuple(values))

    def _make(cls, iterable):
        values = tuple(iterable)
        if len(values) != len(fields):
            raise TypeError('Expected %d arguments, got %d' % (len(fields), len(values)))
        return cls(*values)

    def _replace(self, **kwargs):
        values = dict(zip(fields, self))
        for key in kwargs:
            if key not in values:
                raise ValueError('Got unexpected field names: %r' % (list(kwargs),))
        values.update(kwargs)
        return self.__class__(**values)

    def _asdict(self):
        return dict(zip(fields, self))

    def __repr__(self):
        return '%s(%s)' % (self.__class__.__name__,
                           ', '.join('%s=%r' % (f, v) for f, v in zip(fields, self)))

    def __getnewargs__(self):
        return tuple(self)

    namespace = {
        '__slots__': (),
        '_fields': fields,
        '_field_defaults': default_map,
        '__new__': __new__,
        '_make': classmethod(_make),
        '_replace': _replace,
        '_asdict': _asdict,
        '__repr__': __repr__,
        '__getnewargs__': __getnewargs__,
        'index': lambda self, value, *a: tuple(self).index(value, *a),
        'count': lambda self, value: tuple(self).count(value),
    }
    for index, name in enumerate(fields):
        namespace[name] = property(lambda self, i=index: self[i], None, None)
    result = type(typename, (tuple,), namespace)
    holder.append(result)
    return result


class deque:
    def __init__(self, iterable=(), maxlen=None):
        if maxlen is not None and maxlen < 0:
            raise ValueError('maxlen must be non-negative')
        self._items = []
        self.maxlen = maxlen
        self.extend(iterable)

    def _trim_right(self):
        if self.maxlen is not None:
            while len(self._items) > self.maxlen:
                self._items.pop(0)

    def _trim_left(self):
        if self.maxlen is not None:
            while len(self._items) > self.maxlen:
                self._items.pop()

    def append(self, x):
        self._items.append(x)
        self._trim_right()

    def appendleft(self, x):
        self._items.insert(0, x)
        self._trim_left()

    def pop(self):
        if not self._items:
            raise IndexError('pop from an empty deque')
        return self._items.pop()

    def popleft(self):
        if not self._items:
            raise IndexError('pop from an empty deque')
        return self._items.pop(0)

    def extend(self, iterable):
        if iterable is self:
            iterable = list(iterable)
        for x in iterable:
            self.append(x)

    def extendleft(self, iterable):
        if iterable is self:
            iterable = list(iterable)
        for x in iterable:
            self.appendleft(x)

    def clear(self):
        self._items.clear()

    def copy(self):
        return deque(self._items, self.maxlen)

    __copy__ = copy

    def count(self, x):
        return self._items.count(x)

    def index(self, x, *args):
        try:
            return self._items.index(x, *args)
        except ValueError:
            raise ValueError('%r is not in deque' % (x,)) from None

    def insert(self, i, x):
        if self.maxlen is not None and len(self._items) >= self.maxlen:
            raise IndexError('deque already at its maximum size')
        self._items.insert(i, x)

    def remove(self, value):
        try:
            self._items.remove(value)
        except ValueError:
            raise ValueError('deque.remove(x): x not in deque') from None

    def reverse(self):
        self._items.reverse()

    def rotate(self, n=1):
        size = len(self._items)
        if size == 0:
            return
        n = n % size
        if n:
            self._items[:] = self._items[-n:] + self._items[:-n]

    def __len__(self):
        return len(self._items)

    def __iter__(self):
        return iter(self._items)

    def __reversed__(self):
        return iter(self._items[::-1])

    def __contains__(self, x):
        return x in self._items

    def __getitem__(self, i):
        try:
            return self._items[i]
        except IndexError:
            raise IndexError('deque index out of range') from None

    def __setitem__(self, i, x):
        try:
            self._items[i] = x
        except IndexError:
            raise IndexError('deque index out of range') from None

    def __delitem__(self, i):
        try:
            del self._items[i]
        except IndexError:
            raise IndexError('deque index out of range') from None

    def __eq__(self, other):
        if not isinstance(other, deque):
            return NotImplemented
        return self._items == other._items

    def __add__(self, other):
        if not isinstance(other, deque):
            raise TypeError('can only concatenate deque (not "%s") to deque' % type(other).__name__)
        return deque(self._items + other._items, self.maxlen)

    def __iadd__(self, other):
        self.extend(other)
        return self

    def __mul__(self, n):
        return deque(self._items * n, self.maxlen)

    def __bool__(self):
        return bool(self._items)

    def __repr__(self):
        if self.maxlen is None:
            return 'deque(%r)' % (self._items,)
        return 'deque(%r, maxlen=%d)' % (self._items, self.maxlen)

    __hash__ = None


class defaultdict(dict):
    def __init__(self, default_factory=None, /, *args, **kwargs):
        if default_factory is not None and not callable(default_factory):
            raise TypeError('first argument must be callable or None')
        self.default_factory = default_factory
        super().__init__(*args, **kwargs)

    def __missing__(self, key):
        if self.default_factory is None:
            raise KeyError(key)
        value = self.default_factory()
        self[key] = value
        return value

    def copy(self):
        return defaultdict(self.default_factory, self)

    __copy__ = copy

    def __repr__(self):
        return 'defaultdict(%r, %r)' % (self.default_factory, {k: v for k, v in self.items()})


class OrderedDict(dict):
    def move_to_end(self, key, last=True):
        value = self.pop(key)
        if last:
            self[key] = value
        else:
            rest = list(self.items())
            self.clear()
            self[key] = value
            for k, v in rest:
                self[k] = v

    def popitem(self, last=True):
        if not self:
            raise KeyError('dictionary is empty')
        key = list(self)[-1] if last else next(iter(self))
        return key, self.pop(key)

    def copy(self):
        return OrderedDict(self)

    def __repr__(self):
        if not self:
            return 'OrderedDict()'
        return 'OrderedDict(%r)' % ({k: v for k, v in self.items()},)

    def __eq__(self, other):
        if isinstance(other, OrderedDict):
            return list(self.items()) == list(other.items())
        return {k: v for k, v in self.items()} == other

    __hash__ = None


class Counter(dict):
    def __init__(self, iterable=None, /, **kwds):
        super().__init__()
        self.update(iterable, **kwds)

    def __missing__(self, key):
        return 0

    def total(self):
        return sum(self.values())

    def most_common(self, n=None):
        ordered = sorted(self.items(), key=lambda item: item[1], reverse=True)
        if n is None:
            return ordered
        return ordered[:n]

    def elements(self):
        return chain.from_iterable(repeat(elem, count) for elem, count in self.items())

    def update(self, iterable=None, /, **kwds):
        if iterable is not None:
            if hasattr(iterable, 'items'):
                for elem, count in iterable.items():
                    self[elem] = self.get(elem, 0) + count
            else:
                for elem in iterable:
                    self[elem] = self.get(elem, 0) + 1
        if kwds:
            self.update(kwds)

    def subtract(self, iterable=None, /, **kwds):
        if iterable is not None:
            if hasattr(iterable, 'items'):
                for elem, count in iterable.items():
                    self[elem] = self.get(elem, 0) - count
            else:
                for elem in iterable:
                    self[elem] = self.get(elem, 0) - 1
        if kwds:
            self.subtract(kwds)

    def copy(self):
        return Counter(self)

    def __delitem__(self, elem):
        if elem in self:
            super().__delitem__(elem)

    def __repr__(self):
        if not self:
            return '%s()' % self.__class__.__name__
        try:
            items = ', '.join('%r: %r' % item for item in self.most_common())
            return '%s({%s})' % (self.__class__.__name__, items)
        except TypeError:
            return '%s(%r)' % (self.__class__.__name__, {k: v for k, v in self.items()})

    def __eq__(self, other):
        if not isinstance(other, dict):
            return NotImplemented
        return all(self[e] == other[e] for c in (self, other) for e in c)

    def __ne__(self, other):
        result = self.__eq__(other)
        return result if result is NotImplemented else not result

    def __add__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        result = Counter()
        for elem, count in self.items():
            new = count + other[elem]
            if new > 0:
                result[elem] = new
        for elem, count in other.items():
            if elem not in self and count > 0:
                result[elem] = count
        return result

    def __sub__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        result = Counter()
        for elem, count in self.items():
            new = count - other[elem]
            if new > 0:
                result[elem] = new
        for elem, count in other.items():
            if elem not in self and count < 0:
                result[elem] = 0 - count
        return result

    def __or__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        result = Counter()
        for elem, count in self.items():
            other_count = other[elem]
            new = other_count if count < other_count else count
            if new > 0:
                result[elem] = new
        for elem, count in other.items():
            if elem not in self and count > 0:
                result[elem] = count
        return result

    def __and__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        result = Counter()
        for elem, count in self.items():
            other_count = other[elem]
            new = count if count < other_count else other_count
            if new > 0:
                result[elem] = new
        return result

    __hash__ = None


class ChainMap:
    def __init__(self, *maps):
        self.maps = list(maps) or [{}]

    def __missing__(self, key):
        raise KeyError(key)

    def __getitem__(self, key):
        for mapping in self.maps:
            try:
                return mapping[key]
            except KeyError:
                pass
        return self.__missing__(key)

    def get(self, key, default=None):
        return self[key] if key in self else default

    def __len__(self):
        return len(set().union(*self.maps))

    def __iter__(self):
        d = {}
        for mapping in reversed(self.maps):
            d.update(dict.fromkeys(mapping))
        return iter(d)

    def __contains__(self, key):
        return any(key in m for m in self.maps)

    def __bool__(self):
        return any(self.maps)

    def __repr__(self):
        return '%s(%s)' % (self.__class__.__name__, ', '.join(map(repr, self.maps)))

    def keys(self):
        return list(self)

    def values(self):
        return [self[k] for k in self]

    def items(self):
        return [(k, self[k]) for k in self]

    def copy(self):
        return self.__class__(self.maps[0].copy(), *self.maps[1:])

    __copy__ = copy

    def new_child(self, m=None):
        if m is None:
            m = {}
        return self.__class__(m, *self.maps)

    @property
    def parents(self):
        return self.__class__(*self.maps[1:])

    def __setitem__(self, key, value):
        self.maps[0][key] = value

    def __delitem__(self, key):
        try:
            del self.maps[0][key]
        except KeyError:
            raise KeyError('Key not found in the first mapping: %r' % (key,))

    def popitem(self):
        try:
            return self.maps[0].popitem()
        except KeyError:
            raise KeyError('No keys found in the first mapping.')

    def pop(self, key, *args):
        try:
            return self.maps[0].pop(key, *args)
        except KeyError:
            raise KeyError('Key not found in the first mapping: %r' % (key,))

    def clear(self):
        self.maps[0].clear()


class UserDict:
    def __init__(self, dict=None, /, **kwargs):
        self.data = {}
        if dict is not None:
            self.update(dict)
        if kwargs:
            self.update(kwargs)

    def __len__(self):
        return len(self.data)

    def __getitem__(self, key):
        if key in self.data:
            return self.data[key]
        if hasattr(self.__class__, '__missing__'):
            return self.__class__.__missing__(self, key)
        raise KeyError(key)

    def __setitem__(self, key, item):
        self.data[key] = item

    def __delitem__(self, key):
        del self.data[key]

    def __iter__(self):
        return iter(self.data)

    def __contains__(self, key):
        return key in self.data

    def __repr__(self):
        return repr(self.data)

    def keys(self):
        return self.data.keys()

    def values(self):
        return self.data.values()

    def items(self):
        return self.data.items()

    def get(self, key, default=None):
        return self[key] if key in self else default

    def pop(self, key, *args):
        return self.data.pop(key, *args)

    def setdefault(self, key, default=None):
        if key not in self:
            self[key] = default
        return self[key]

    def update(self, other=(), /, **kwds):
        if hasattr(other, 'items'):
            for key, value in other.items():
                self[key] = value
        else:
            for key, value in other:
                self[key] = value
        for key, value in kwds.items():
            self[key] = value

    def copy(self):
        return self.__class__(self.data)


class UserList:
    def __init__(self, initlist=None):
        self.data = []
        if initlist is not None:
            self.data[:] = list(initlist)

    def __repr__(self):
        return repr(self.data)

    def __len__(self):
        return len(self.data)

    def __getitem__(self, i):
        return self.data[i]

    def __setitem__(self, i, item):
        self.data[i] = item

    def __delitem__(self, i):
        del self.data[i]

    def __iter__(self):
        return iter(self.data)

    def __contains__(self, item):
        return item in self.data

    def __eq__(self, other):
        return self.data == (other.data if isinstance(other, UserList) else other)

    def append(self, item):
        self.data.append(item)

    def extend(self, other):
        self.data.extend(other)

    def insert(self, i, item):
        self.data.insert(i, item)

    def pop(self, i=-1):
        return self.data.pop(i)

    def remove(self, item):
        self.data.remove(item)

    def sort(self, *args, **kwds):
        self.data.sort(*args, **kwds)

    def reverse(self):
        self.data.reverse()

    def count(self, item):
        return self.data.count(item)

    def index(self, item, *args):
        return self.data.index(item, *args)

    def copy(self):
        return self.__class__(self)


from collections.abc import Sequence as _Sequence
import sys as _sys


class UserString(_Sequence):

    def __init__(self, seq):
        if isinstance(seq, str):
            self.data = seq
        elif isinstance(seq, UserString):
            self.data = seq.data[:]
        else:
            self.data = str(seq)

    def __str__(self):
        return str(self.data)

    def __repr__(self):
        return repr(self.data)

    def __int__(self):
        return int(self.data)

    def __float__(self):
        return float(self.data)

    def __complex__(self):
        return complex(self.data)

    def __hash__(self):
        return hash(self.data)

    def __getnewargs__(self):
        return (self.data[:],)

    def __eq__(self, string):
        if isinstance(string, UserString):
            return self.data == string.data
        return self.data == string

    def __lt__(self, string):
        if isinstance(string, UserString):
            return self.data < string.data
        return self.data < string

    def __le__(self, string):
        if isinstance(string, UserString):
            return self.data <= string.data
        return self.data <= string

    def __gt__(self, string):
        if isinstance(string, UserString):
            return self.data > string.data
        return self.data > string

    def __ge__(self, string):
        if isinstance(string, UserString):
            return self.data >= string.data
        return self.data >= string

    def __contains__(self, char):
        if isinstance(char, UserString):
            char = char.data
        return char in self.data

    def __len__(self):
        return len(self.data)

    def __getitem__(self, index):
        return self.__class__(self.data[index])

    def __add__(self, other):
        if isinstance(other, UserString):
            return self.__class__(self.data + other.data)
        elif isinstance(other, str):
            return self.__class__(self.data + other)
        return self.__class__(self.data + str(other))

    def __radd__(self, other):
        if isinstance(other, str):
            return self.__class__(other + self.data)
        return self.__class__(str(other) + self.data)

    def __mul__(self, n):
        return self.__class__(self.data * n)

    __rmul__ = __mul__

    def __mod__(self, args):
        return self.__class__(self.data % args)

    def __rmod__(self, template):
        return self.__class__(str(template) % self)

    # the following methods are defined in alphabetical order:
    def capitalize(self):
        return self.__class__(self.data.capitalize())

    def casefold(self):
        return self.__class__(self.data.casefold())

    def center(self, width, *args):
        return self.__class__(self.data.center(width, *args))

    def count(self, sub, start=0, end=_sys.maxsize):
        if isinstance(sub, UserString):
            sub = sub.data
        return self.data.count(sub, start, end)

    def removeprefix(self, prefix, /):
        if isinstance(prefix, UserString):
            prefix = prefix.data
        return self.__class__(self.data.removeprefix(prefix))

    def removesuffix(self, suffix, /):
        if isinstance(suffix, UserString):
            suffix = suffix.data
        return self.__class__(self.data.removesuffix(suffix))

    def encode(self, encoding='utf-8', errors='strict'):
        encoding = 'utf-8' if encoding is None else encoding
        errors = 'strict' if errors is None else errors
        return self.data.encode(encoding, errors)

    def endswith(self, suffix, start=0, end=_sys.maxsize):
        return self.data.endswith(suffix, start, end)

    def expandtabs(self, tabsize=8):
        return self.__class__(self.data.expandtabs(tabsize))

    def find(self, sub, start=0, end=_sys.maxsize):
        if isinstance(sub, UserString):
            sub = sub.data
        return self.data.find(sub, start, end)

    def format(self, /, *args, **kwds):
        return self.data.format(*args, **kwds)

    def format_map(self, mapping):
        return self.data.format_map(mapping)

    def index(self, sub, start=0, end=_sys.maxsize):
        return self.data.index(sub, start, end)

    def isalpha(self):
        return self.data.isalpha()

    def isalnum(self):
        return self.data.isalnum()

    def isascii(self):
        return self.data.isascii()

    def isdecimal(self):
        return self.data.isdecimal()

    def isdigit(self):
        return self.data.isdigit()

    def isidentifier(self):
        return self.data.isidentifier()

    def islower(self):
        return self.data.islower()

    def isnumeric(self):
        return self.data.isnumeric()

    def isprintable(self):
        return self.data.isprintable()

    def isspace(self):
        return self.data.isspace()

    def istitle(self):
        return self.data.istitle()

    def isupper(self):
        return self.data.isupper()

    def join(self, seq):
        return self.data.join(seq)

    def ljust(self, width, *args):
        return self.__class__(self.data.ljust(width, *args))

    def lower(self):
        return self.__class__(self.data.lower())

    def lstrip(self, chars=None):
        return self.__class__(self.data.lstrip(chars))

    maketrans = str.maketrans

    def partition(self, sep):
        return self.data.partition(sep)

    def replace(self, old, new, maxsplit=-1):
        if isinstance(old, UserString):
            old = old.data
        if isinstance(new, UserString):
            new = new.data
        return self.__class__(self.data.replace(old, new, maxsplit))

    def rfind(self, sub, start=0, end=_sys.maxsize):
        if isinstance(sub, UserString):
            sub = sub.data
        return self.data.rfind(sub, start, end)

    def rindex(self, sub, start=0, end=_sys.maxsize):
        return self.data.rindex(sub, start, end)

    def rjust(self, width, *args):
        return self.__class__(self.data.rjust(width, *args))

    def rpartition(self, sep):
        return self.data.rpartition(sep)

    def rstrip(self, chars=None):
        return self.__class__(self.data.rstrip(chars))

    def split(self, sep=None, maxsplit=-1):
        return self.data.split(sep, maxsplit)

    def rsplit(self, sep=None, maxsplit=-1):
        return self.data.rsplit(sep, maxsplit)

    def splitlines(self, keepends=False):
        return self.data.splitlines(keepends)

    def startswith(self, prefix, start=0, end=_sys.maxsize):
        return self.data.startswith(prefix, start, end)

    def strip(self, chars=None):
        return self.__class__(self.data.strip(chars))

    def swapcase(self):
        return self.__class__(self.data.swapcase())

    def title(self):
        return self.__class__(self.data.title())

    def translate(self, *args):
        return self.__class__(self.data.translate(*args))

    def upper(self):
        return self.__class__(self.data.upper())

    def zfill(self, width):
        return self.__class__(self.data.zfill(width))
