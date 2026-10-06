"""marshal do sandbox (Python embutido): serialização binária dos tipos básicos, no formato do CPython.

Escreve sem as marcas de referência (`FLAG_REF`), que no CPython dependem da contagem de referências dos
objetos; lê as duas formas, então aceita dados produzidos pelo CPython."""

import struct

version = 5

_FLAG_REF = 0x80


def _w_long(out, n):
    out.append(struct.pack('<i', n))


def _w_object(out, obj, depth):
    if depth > 2000:
        raise ValueError('object too deeply nested to marshal')
    if obj is None:
        out.append(b'N')
    elif obj is True:
        out.append(b'T')
    elif obj is False:
        out.append(b'F')
    elif obj is StopIteration:
        out.append(b'S')
    elif obj is Ellipsis:
        out.append(b'.')
    elif isinstance(obj, int):
        if -0x80000000 <= obj <= 0x7fffffff:
            out.append(b'i')
            _w_long(out, obj)
        else:
            out.append(b'l')
            digits = []
            n = abs(obj)
            while n:
                digits.append(n & 0x7fff)
                n >>= 15
            _w_long(out, -len(digits) if obj < 0 else len(digits))
            for d in digits:
                out.append(struct.pack('<H', d))
    elif isinstance(obj, float):
        out.append(b'g')
        out.append(struct.pack('<d', obj))
    elif isinstance(obj, complex):
        out.append(b'y')
        out.append(struct.pack('<dd', obj.real, obj.imag))
    elif isinstance(obj, str):
        data = obj.encode('utf-8', 'surrogatepass')
        if obj.isascii():
            if len(data) < 256:
                out.append(b'z')
                out.append(bytes([len(data)]))
            else:
                out.append(b'a')
                _w_long(out, len(data))
        else:
            out.append(b'u')
            _w_long(out, len(data))
        out.append(data)
    elif isinstance(obj, (bytes, bytearray, memoryview)):
        data = bytes(obj)
        out.append(b's')
        _w_long(out, len(data))
        out.append(data)
    elif isinstance(obj, tuple):
        if len(obj) < 256:
            out.append(b')')
            out.append(bytes([len(obj)]))
        else:
            out.append(b'(')
            _w_long(out, len(obj))
        for item in obj:
            _w_object(out, item, depth + 1)
    elif isinstance(obj, list):
        out.append(b'[')
        _w_long(out, len(obj))
        for item in obj:
            _w_object(out, item, depth + 1)
    elif isinstance(obj, dict):
        out.append(b'{')
        for key, value in obj.items():
            _w_object(out, key, depth + 1)
            _w_object(out, value, depth + 1)
        out.append(b'0')
    elif isinstance(obj, (set, frozenset)):
        out.append(b'>' if isinstance(obj, frozenset) else b'<')
        _w_long(out, len(obj))
        for item in obj:
            _w_object(out, item, depth + 1)
    else:
        raise ValueError('unmarshallable object')


def dumps(value, version=version, /, *, allow_code=True):
    out = []
    _w_object(out, value, 0)
    return b''.join(out)


def dump(value, file, version=version, /, *, allow_code=True):
    file.write(dumps(value, version))


class _Reader:
    def __init__(self, data):
        self.data = data
        self.pos = 0
        self.refs = []

    def take(self, n):
        end = self.pos + n
        if end > len(self.data):
            raise EOFError('marshal data too short')
        chunk = self.data[self.pos:end]
        self.pos = end
        return chunk

    def long(self):
        return struct.unpack('<i', self.take(4))[0]

    def obj(self):
        code = self.take(1)[0]
        flag = code & _FLAG_REF
        t = chr(code & ~_FLAG_REF)
        slot = None
        if flag:
            slot = len(self.refs)
            self.refs.append(None)

        def done(value):
            if slot is not None:
                self.refs[slot] = value
            return value

        if t == 'N':
            return done(None)
        if t == 'T':
            return done(True)
        if t == 'F':
            return done(False)
        if t == 'S':
            return done(StopIteration)
        if t == '.':
            return done(Ellipsis)
        if t == 'i':
            return done(self.long())
        if t == 'l':
            n = self.long()
            count = abs(n)
            value = 0
            for i in range(count):
                value |= struct.unpack('<H', self.take(2))[0] << (15 * i)
            return done(-value if n < 0 else value)
        if t == 'g':
            return done(struct.unpack('<d', self.take(8))[0])
        if t == 'y':
            re, im = struct.unpack('<dd', self.take(16))
            return done(complex(re, im))
        if t in 'uat':
            return done(self.take(self.long()).decode('utf-8', 'surrogatepass'))
        if t in 'zZ':
            return done(self.take(self.take(1)[0]).decode('utf-8', 'surrogatepass'))
        if t == 'A':
            return done(self.take(self.long()).decode('utf-8'))
        if t == 's':
            return done(bytes(self.take(self.long())))
        if t == ')':
            n = self.take(1)[0]
            items = tuple(self.obj() for _ in range(n))
            return done(items)
        if t == '(':
            n = self.long()
            items = tuple(self.obj() for _ in range(n))
            return done(items)
        if t == '[':
            result = []
            done(result)
            for _ in range(self.long()):
                result.append(self.obj())
            return result
        if t == '{':
            result = {}
            done(result)
            while True:
                if self.data[self.pos:self.pos + 1] == b'0':
                    self.pos += 1
                    return result
                key = self.obj()
                result[key] = self.obj()
        if t == '<':
            result = set()
            done(result)
            for _ in range(self.long()):
                result.add(self.obj())
            return result
        if t == '>':
            n = self.long()
            return done(frozenset(self.obj() for _ in range(n)))
        if t == 'r':
            return self.refs[self.long()]
        raise ValueError('bad marshal data (unknown type code)')


def loads(data, /, *, allow_code=True):
    return _Reader(bytes(data)).obj()


def load(file, /, *, allow_code=True):
    data = file.read()
    reader = _Reader(bytes(data))
    value = reader.obj()
    rest = len(data) - reader.pos
    if rest and hasattr(file, 'seek'):
        file.seek(-rest, 1)
    return value
