"""Tipo `complex`: número complexo imutável (`Objects/complexobject.c`), em Python.

O interpretador resolve o nome `complex` para esta classe e compila literais como `3j` em
`complex(0.0, 3.0)`.
"""

import math as _math


def _fmt(x):
    """repr de um componente, sem o `.0` final de inteiros (`1.0` vira `1`), como o CPython."""
    s = repr(x)
    if s.endswith('.0'):
        s = s[:-2]
    return s


class complex:
    """complex(real=0, imag=0): número complexo."""

    __slots__ = ('real', 'imag')
    __module__ = 'builtins'

    def __new__(cls, real=0, imag=None):
        if imag is None:
            imag = 0
            if isinstance(real, complex) and type(real) is cls:
                return real
            only_real = True
        else:
            only_real = False
        if isinstance(real, str):
            if not only_real:
                raise TypeError("complex() can't take second arg if first is a string")
            return cls._parse(real)
        if isinstance(imag, str):
            raise TypeError("complex() second arg can't be a string")
        re, im = _split(real)
        re2, im2 = _split(imag)
        self = object.__new__(cls)
        # real + imag * 1j
        object.__setattr__(self, 'real', float(re) if only_real or not isinstance(imag, complex) else float(re - im2))
        object.__setattr__(self, 'imag', float(im) if only_real else (float(re2) if not isinstance(real, complex) else float(im + re2)))
        return self

    @classmethod
    def _parse(cls, text):
        t = text.strip()
        if t.startswith('(') and t.endswith(')'):
            t = t[1:-1].strip()
        bad = ValueError("complex() arg is a malformed string")
        if not t:
            raise bad
        try:
            if t[-1] in 'jJ':
                body = t[:-1]
                # divide em parte real e imaginária no último sinal que não segue um expoente
                cut = -1
                for i in range(len(body) - 1, 0, -1):
                    if body[i] in '+-' and body[i - 1] not in 'eE':
                        cut = i
                        break
                if cut == -1:
                    re, im = 0.0, _imag_part(body)
                else:
                    re, im = float(body[:cut]), _imag_part(body[cut:])
            else:
                re, im = float(t), 0.0
        except ValueError:
            raise bad from None
        return cls(re, im)

    def __repr__(self):
        re, im = self.real, self.imag
        if re == 0 and _math.copysign(1.0, re) > 0:
            return _fmt(im) + 'j'
        sign = '-' if (im < 0 or (im == 0 and _math.copysign(1.0, im) < 0)) else '+'
        return '(' + _fmt(re) + sign + _fmt(abs(im)) + 'j)'

    __str__ = __repr__

    def __hash__(self):
        h = hash(self.imag) * 1000003 + hash(self.real)
        return h if h != -1 else -2

    def __eq__(self, other):
        if isinstance(other, complex):
            return self.real == other.real and self.imag == other.imag
        if isinstance(other, (int, float)):
            return self.imag == 0 and self.real == other
        return NotImplemented

    def __ne__(self, other):
        r = self.__eq__(other)
        return r if r is NotImplemented else not r

    def __bool__(self):
        return self.real != 0 or self.imag != 0

    def __neg__(self):
        return complex(-self.real, -self.imag)

    def __pos__(self):
        return self

    def __abs__(self):
        return _math.hypot(self.real, self.imag)

    def conjugate(self):
        return complex(self.real, -self.imag)

    def __add__(self, other):
        o = _coerce(other)
        if o is None:
            return NotImplemented
        return complex(self.real + o[0], self.imag + o[1])

    __radd__ = __add__

    def __sub__(self, other):
        o = _coerce(other)
        if o is None:
            return NotImplemented
        return complex(self.real - o[0], self.imag - o[1])

    def __rsub__(self, other):
        o = _coerce(other)
        if o is None:
            return NotImplemented
        return complex(o[0] - self.real, o[1] - self.imag)

    def __mul__(self, other):
        o = _coerce(other)
        if o is None:
            return NotImplemented
        a, b, c, d = self.real, self.imag, o[0], o[1]
        return complex(a * c - b * d, a * d + b * c)

    __rmul__ = __mul__

    def __truediv__(self, other):
        o = _coerce(other)
        if o is None:
            return NotImplemented
        return _div(self.real, self.imag, o[0], o[1])

    def __rtruediv__(self, other):
        o = _coerce(other)
        if o is None:
            return NotImplemented
        return _div(o[0], o[1], self.real, self.imag)

    def __pow__(self, other, mod=None):
        if mod is not None:
            raise ValueError('complex modulo')
        o = _coerce(other)
        if o is None:
            return NotImplemented
        c, d = o
        a, b = self.real, self.imag
        if c == 0 and d == 0:
            return complex(1.0, 0.0)
        if a == 0 and b == 0:
            if d != 0 or c < 0:
                raise ZeroDivisionError('zero to a negative or complex power')
            return complex(0.0, 0.0)
        if d == 0 and c == int(c) and abs(c) <= 100:
            n = int(abs(c))
            result = complex(1.0, 0.0)
            base = self
            while n:
                if n & 1:
                    result = result * base
                base = base * base
                n >>= 1
            return result if c >= 0 else complex(1.0, 0.0) / result
        vabs = _math.hypot(a, b)
        length = vabs ** c
        at = _math.atan2(b, a)
        phase = at * c
        if d != 0.0:
            length /= _math.exp(at * d)
            phase += d * _math.log(vabs)
        return complex(length * _math.cos(phase), length * _math.sin(phase))

    def __rpow__(self, other, mod=None):
        o = _coerce(other)
        if o is None:
            return NotImplemented
        return complex(o[0], o[1]) ** self

    def __format__(self, spec):
        if not spec:
            return repr(self)
        return format(self.real, spec) + ('+' if self.imag >= 0 or self.imag != self.imag else '') + format(self.imag, spec) + 'j'

    def __complex__(self):
        return self

    def __getnewargs__(self):
        return (self.real, self.imag)


def _imag_part(s):
    if s in ('', '+'):
        return 1.0
    if s == '-':
        return -1.0
    return float(s)


def _split(x):
    if isinstance(x, complex):
        return x.real, x.imag
    if isinstance(x, (int, float)):
        return x, 0.0
    f = getattr(x, '__complex__', None)
    if f is not None:
        c = f()
        return c.real, c.imag
    f = getattr(x, '__float__', None)
    if f is not None:
        return f(), 0.0
    raise TypeError("complex() first argument must be a string or a number, not '%s'" % type(x).__name__)


def _coerce(x):
    if isinstance(x, complex):
        return x.real, x.imag
    if isinstance(x, (int, float)):
        return float(x), 0.0
    return None


def _div(a, b, c, d):
    if c == 0 and d == 0:
        raise ZeroDivisionError('complex division by zero')
    if abs(c) >= abs(d):
        ratio = d / c
        denom = c + d * ratio
        return complex((a + b * ratio) / denom, (b - a * ratio) / denom)
    ratio = c / d
    denom = c * ratio + d
    return complex((a * ratio + b) / denom, (b * ratio - a) / denom)
