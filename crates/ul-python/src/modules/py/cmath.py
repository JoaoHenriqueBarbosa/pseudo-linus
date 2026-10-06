"""cmath em Python sobre `math` (funções matemáticas para números complexos)."""
import math as _m

pi = _m.pi
e = _m.e
tau = _m.tau
inf = _m.inf
nan = _m.nan
infj = complex(0.0, inf)
nanj = complex(0.0, nan)


def _c(z):
    return complex(z)


def phase(z):
    z = _c(z)
    return _m.atan2(z.imag, z.real)


def polar(z):
    z = _c(z)
    return abs(z), _m.atan2(z.imag, z.real)


def rect(r, phi):
    if _m.isinf(r) and _m.isfinite(phi) and phi != 0.0:
        return complex(_m.copysign(inf, _m.cos(phi)), _m.copysign(inf, _m.sin(phi)))
    return complex(r * _m.cos(phi), r * _m.sin(phi))


def isnan(z):
    z = _c(z)
    return _m.isnan(z.real) or _m.isnan(z.imag)


def isinf(z):
    z = _c(z)
    return _m.isinf(z.real) or _m.isinf(z.imag)


def isfinite(z):
    z = _c(z)
    return _m.isfinite(z.real) and _m.isfinite(z.imag)


def isclose(a, b, *, rel_tol=1e-09, abs_tol=0.0):
    a, b = _c(a), _c(b)
    if rel_tol < 0.0 or abs_tol < 0.0:
        raise ValueError("tolerances must be non-negative")
    if a == b:
        return True
    if isinf(a) or isinf(b):
        return False
    diff = abs(a - b)
    return diff <= rel_tol * abs(b) or diff <= rel_tol * abs(a) or diff <= abs_tol


def sqrt(z):
    z = _c(z)
    a, b = z.real, z.imag
    if a == 0.0 and b == 0.0:
        return complex(0.0, b)
    if _m.isinf(b):
        return complex(inf, b)
    if _m.isnan(a):
        return complex(a, a if not _m.isinf(b) else b)
    if _m.isinf(a):
        if a > 0:
            return complex(a, _m.copysign(0.0, b) if not _m.isnan(b) else b)
        return complex(abs(b) if not _m.isnan(b) else b, _m.copysign(inf, b))
    ax, ay = abs(a), abs(b)
    s = _m.sqrt(0.5 * (ax + _m.hypot(ax, ay)))
    d = ay / (2.0 * s)
    if a >= 0:
        return complex(s, _m.copysign(d, b))
    return complex(d, _m.copysign(s, b))


def exp(z):
    z = _c(z)
    if _m.isinf(z.real) and z.real > 0 and z.imag == 0:
        return complex(inf, z.imag)
    try:
        r = _m.exp(z.real)
    except OverflowError:
        raise OverflowError("math range error") from None
    return complex(r * _m.cos(z.imag), r * _m.sin(z.imag))


def log(z, base=None):
    z = _c(z)
    if z == 0:
        raise ValueError("math domain error")
    r = complex(_m.log(_m.hypot(z.real, z.imag)), _m.atan2(z.imag, z.real))
    if base is not None:
        return r / log(base)
    return r


def log10(z):
    r = log(z)
    return complex(r.real / _m.log(10.0), r.imag / _m.log(10.0))


def sin(z):
    z = _c(z)
    return complex(_m.sin(z.real) * _m.cosh(z.imag), _m.cos(z.real) * _m.sinh(z.imag))


def cos(z):
    z = _c(z)
    return complex(_m.cos(z.real) * _m.cosh(z.imag), -_m.sin(z.real) * _m.sinh(z.imag))


def tan(z):
    z = _c(z)
    r = tanh(complex(-z.imag, z.real))
    return complex(r.imag, -r.real)


def sinh(z):
    z = _c(z)
    return complex(_m.sinh(z.real) * _m.cos(z.imag), _m.cosh(z.real) * _m.sin(z.imag))


def cosh(z):
    z = _c(z)
    return complex(_m.cosh(z.real) * _m.cos(z.imag), _m.sinh(z.real) * _m.sin(z.imag))


def tanh(z):
    z = _c(z)
    if abs(z.real) > 354:
        return complex(_m.copysign(1.0, z.real), 4.0 * _m.sin(z.imag) * _m.cos(z.imag) * _m.exp(-2.0 * abs(z.real)))
    tx = _m.tanh(z.real)
    ty = _m.tan(z.imag)
    cx = 1.0 / _m.cosh(z.real)
    txty = tx * ty
    denom = 1.0 + txty * txty
    return complex(tx * (1.0 + ty * ty) / denom, ((ty / denom) * cx) * cx)


def asinh(z):
    z = _c(z)
    s1 = sqrt(complex(1.0 + z.imag, -z.real))
    s2 = sqrt(complex(1.0 - z.imag, z.real))
    return complex(_m.asinh(s1.real * s2.imag - s2.real * s1.imag),
                   _m.atan2(z.imag, s1.real * s2.real - s1.imag * s2.imag))


def acosh(z):
    z = _c(z)
    s1 = sqrt(complex(z.real - 1.0, z.imag))
    s2 = sqrt(complex(z.real + 1.0, z.imag))
    return complex(_m.asinh(s1.real * s2.real + s1.imag * s2.imag), 2.0 * _m.atan2(s1.imag, s2.real))


def atanh(z):
    z = _c(z)
    if z.real < 0.0:
        r = atanh(complex(-z.real, -z.imag))
        return complex(-r.real, -r.imag)
    ay = abs(z.imag)
    if z.real == 1.0 and ay == 0.0:
        raise ValueError("math domain error")
    return complex(_m.log1p(4.0 * z.real / ((1 - z.real) * (1 - z.real) + ay * ay)) / 4.0,
                   _m.copysign(-_m.atan2(-2.0 * ay, (1 - z.real) * (1 + z.real) - ay * ay) / 2.0, z.imag))


def asin(z):
    z = _c(z)
    s1 = sqrt(complex(1.0 - z.real, -z.imag))
    s2 = sqrt(complex(1.0 + z.real, z.imag))
    return complex(_m.atan2(z.real, s1.real * s2.real - s1.imag * s2.imag),
                   _m.asinh(s2.imag * s1.real - s2.real * s1.imag))


def acos(z):
    z = _c(z)
    s1 = sqrt(complex(1.0 - z.real, -z.imag))
    s2 = sqrt(complex(1.0 + z.real, z.imag))
    return complex(2.0 * _m.atan2(s1.real, s2.real), _m.asinh(s2.real * s1.imag - s2.imag * s1.real))


def atan(z):
    z = _c(z)
    r = atanh(complex(-z.imag, z.real))
    return complex(r.imag, -r.real)
