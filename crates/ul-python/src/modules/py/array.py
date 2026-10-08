"""Vetores tipados compactos, guardados numa lista e serializados por `struct`."""

import struct
from types import GenericAlias as _GenericAlias

typecodes = 'bBuwhHiIlLqQfd'
_FORMATS = {'b': 'b', 'B': 'B', 'h': 'h', 'H': 'H', 'i': 'i', 'I': 'I', 'l': 'q', 'L': 'Q',
            'q': 'q', 'Q': 'Q', 'f': 'f', 'd': 'd'}
_SIZES = {'b': 1, 'B': 1, 'u': 4, 'w': 4, 'h': 2, 'H': 2, 'i': 4, 'I': 4, 'l': 8, 'L': 8,
          'q': 8, 'Q': 8, 'f': 4, 'd': 8}
_RANGES = {'b': (-128, 127), 'B': (0, 255), 'h': (-32768, 32767), 'H': (0, 65535),
           'i': (-2**31, 2**31 - 1), 'I': (0, 2**32 - 1), 'l': (-2**63, 2**63 - 1),
           'L': (0, 2**64 - 1), 'q': (-2**63, 2**63 - 1), 'Q': (0, 2**64 - 1)}


class array:
    __class_getitem__ = classmethod(_GenericAlias)

    # Como no `array_new` do CPython, tudo acontece no `__new__` e o `__init__` é o de `object`: uma
    # subclasse que reescreve `__new__` com outra assinatura (`StyleArray` do openpyxl) funciona.
    def __new__(cls, *args, **kwargs):
        if kwargs and cls is array:
            raise TypeError('array.array() takes no keyword arguments')
        if not args:
            raise TypeError('array() takes at least 1 argument (0 given)')
        if len(args) > 2:
            raise TypeError('array() takes at most 2 arguments (%d given)' % len(args))
        typecode = args[0]
        initializer = args[1] if len(args) > 1 else None
        if not isinstance(typecode, str) or len(typecode) != 1:
            raise TypeError('array() argument 1 must be a unicode character, not %s' % type(typecode).__name__)
        if typecode not in typecodes:
            raise ValueError('bad typecode (must be b, B, u, w, h, H, i, I, l, L, q, Q, f or d)')
        self = object.__new__(cls)
        self.typecode = typecode
        self.itemsize = _SIZES[typecode]
        self._items = []
        if initializer is None:
            return self
        if isinstance(initializer, (bytes, bytearray)):
            self.frombytes(initializer)
        elif isinstance(initializer, str):
            if typecode not in 'uw':
                raise TypeError('cannot use a str to initialize an array with typecode %r' % typecode)
            self._items = list(initializer)
        elif isinstance(initializer, array):
            self.extend(initializer)
        else:
            for x in initializer:
                self.append(x)
        return self

    def _check(self, x):
        t = self.typecode
        if t in 'uw':
            if not isinstance(x, str) or len(x) != 1:
                raise TypeError('array item must be a unicode character, not %s' % type(x).__name__)
            return x
        if t in 'fd':
            if isinstance(x, bool) or not isinstance(x, (int, float)):
                raise TypeError('must be real number, not %s' % type(x).__name__)
            return float(x)
        if not isinstance(x, int):
            raise TypeError("'%s' object cannot be interpreted as an integer" % type(x).__name__)
        lo, hi = _RANGES[t]
        if x < lo:
            raise OverflowError('signed integer is less than minimum' if lo < 0 else 'unsigned byte integer is less than minimum')
        if x > hi:
            raise OverflowError('signed integer is greater than maximum' if lo < 0 else 'unsigned byte integer is greater than maximum')
        return int(x)

    def append(self, x):
        self._items.append(self._check(x))

    def extend(self, it):
        if isinstance(it, array) and it.typecode != self.typecode:
            raise TypeError('can only extend with array of same kind')
        for x in it:
            self.append(x)

    def insert(self, i, x):
        self._items.insert(i, self._check(x))

    def pop(self, i=-1):
        return self._items.pop(i)

    def remove(self, x):
        self._items.remove(x)

    def index(self, x, *args):
        return self._items.index(x, *args)

    def count(self, x):
        return self._items.count(x)

    def reverse(self):
        self._items.reverse()

    def tolist(self):
        return list(self._items)

    def fromlist(self, lst):
        if not isinstance(lst, list):
            raise TypeError('arg must be list')
        for x in lst:
            self.append(x)

    def tobytes(self):
        t = self.typecode
        if t in 'uw':
            return ''.join(self._items).encode('utf-32-le')
        return struct.pack('<%d%s' % (len(self._items), _FORMATS[t]), *self._items)

    def frombytes(self, data):
        data = bytes(data)
        if len(data) % self.itemsize:
            raise ValueError('bytes length not a multiple of item size')
        t = self.typecode
        if t in 'uw':
            self._items.extend(data.decode('utf-32-le'))
            return
        n = len(data) // self.itemsize
        self._items.extend(struct.unpack('<%d%s' % (n, _FORMATS[t]), data))

    def tofile(self, f):
        f.write(self.tobytes())

    def fromfile(self, f, n):
        data = f.read(n * self.itemsize)
        if len(data) != n * self.itemsize:
            self.frombytes(data[:len(data) - len(data) % self.itemsize])
            raise EOFError('read() didn\'t return enough bytes')
        self.frombytes(data)

    def tounicode(self):
        if self.typecode not in 'uw':
            raise ValueError('tounicode() may only be called on unicode type arrays')
        return ''.join(self._items)

    def fromunicode(self, s):
        if self.typecode not in 'uw':
            raise ValueError('fromunicode() may only be called on unicode type arrays')
        self._items.extend(s)

    def byteswap(self):
        if self.itemsize == 1:
            return
        raw = self.tobytes()
        sz = self.itemsize
        swapped = b''.join(raw[i:i + sz][::-1] for i in range(0, len(raw), sz))
        self._items = []
        self.frombytes(swapped)

    def buffer_info(self):
        return (0, len(self._items))

    def __len__(self):
        return len(self._items)

    def __iter__(self):
        return iter(self._items)

    def __contains__(self, x):
        return x in self._items

    def __getitem__(self, i):
        if isinstance(i, slice):
            new = array(self.typecode)
            new._items = self._items[i]
            return new
        return self._items[i]

    def __setitem__(self, i, x):
        if isinstance(i, slice):
            if not isinstance(x, array) or x.typecode != self.typecode:
                raise TypeError('can only assign array (not "%s") to array slice' % type(x).__name__)
            self._items[i] = list(x._items)
            return
        self._items[i] = self._check(x)

    def __delitem__(self, i):
        del self._items[i]

    def __add__(self, other):
        if not isinstance(other, array):
            raise TypeError('can only append array (not "%s") to array' % type(other).__name__)
        if other.typecode != self.typecode:
            raise TypeError('bad argument type for built-in operation')
        new = array(self.typecode)
        new._items = self._items + other._items
        return new

    def __iadd__(self, other):
        self.extend(other)
        return self

    def __mul__(self, n):
        new = array(self.typecode)
        new._items = self._items * n
        return new

    __rmul__ = __mul__

    def __imul__(self, n):
        self._items = self._items * n
        return self

    def __eq__(self, other):
        if not isinstance(other, array):
            return NotImplemented
        return self._items == other._items

    def __ne__(self, other):
        if not isinstance(other, array):
            return NotImplemented
        return self._items != other._items

    def __lt__(self, other):
        return self._items < other._items

    def __le__(self, other):
        return self._items <= other._items

    def __gt__(self, other):
        return self._items > other._items

    def __ge__(self, other):
        return self._items >= other._items

    __hash__ = None

    def __copy__(self):
        new = array(self.typecode)
        new._items = list(self._items)
        return new

    def __deepcopy__(self, memo):
        return self.__copy__()

    def __reduce__(self):
        return (array, (self.typecode, self.tolist()))

    def __repr__(self):
        name = type(self).__name__
        if not self._items:
            return "%s('%s')" % (name, self.typecode)
        if self.typecode in 'uw':
            return "%s('%s', %r)" % (name, self.typecode, ''.join(self._items))
        return "%s('%s', %r)" % (name, self.typecode, self._items)


ArrayType = array

# `enum machine_format_code` do `arraymodule.c`: o formato de `struct` de cada código (os dois últimos pares são
# UTF-16 e UTF-32, decodificados pelo codec) e o código nativo de cada typecode no x86-64.
_MACHINE_FORMATS = ('<B', '<b', '<H', '>H', '<h', '>h', '<I', '>I', '<i', '>i', '<Q', '>Q', '<q', '>q', '<f', '>f',
                    '<d', '>d', 'utf-16-le', 'utf-16-be', 'utf-32-le', 'utf-32-be')
_NATIVE_MACHINE_FORMAT = {'b': 1, 'B': 0, 'h': 4, 'H': 2, 'i': 8, 'I': 6, 'l': 12, 'L': 10, 'q': 12, 'Q': 10,
                          'f': 14, 'd': 16, 'u': 20, 'w': 20}


def _array_reconstructor(arraytype, typecode, mformat_code, items, /):
    """Internal. Used for pickling support."""
    if not isinstance(arraytype, type):
        raise TypeError('first argument must be a type object, not %.200s' % type(arraytype).__name__)
    if not issubclass(arraytype, array):
        raise TypeError('%.200s is not a subtype of %.200s' % (arraytype.__name__, array.__name__))
    if not isinstance(typecode, str):
        raise TypeError('_array_reconstructor() argument 2 must be a unicode character, not %s'
                        % type(typecode).__name__)
    if len(typecode) != 1:
        raise TypeError('_array_reconstructor() argument 2 must be a unicode character, not a string of length %d'
                        % len(typecode))
    if typecode not in typecodes:
        raise ValueError('second argument must be a valid type code')
    if not 0 <= mformat_code <= 21:
        raise ValueError('third argument must be a valid machine format code.')
    if not isinstance(items, bytes):
        raise TypeError('fourth argument should be bytes, not %.200s' % type(items).__name__)
    result = arraytype(typecode)
    # Sem conversão: o código de máquina é o do próprio typecode.
    if _NATIVE_MACHINE_FORMAT[typecode] == mformat_code:
        result.frombytes(items)
        return result
    fmt = _MACHINE_FORMATS[mformat_code]
    if mformat_code >= 18:
        result.fromunicode(items.decode(fmt))
        return result
    if len(items) % struct.calcsize(fmt):
        raise ValueError('string length not a multiple of item size')
    for (value,) in struct.iter_unpack(fmt, items):
        result.append(value)
    return result

del _GenericAlias
