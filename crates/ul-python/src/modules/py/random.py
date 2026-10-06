"""random: Mersenne Twister idêntico ao do CPython (mesmas sequências para a mesma semente)."""

import _os
from math import floor as _floor, log as _log, exp as _exp, sqrt as _sqrt, cos as _cos, sin as _sin
from math import pi as _pi, acos as _acos

__all__ = ['Random', 'SystemRandom', 'seed', 'random', 'uniform', 'randint', 'choice', 'sample',
           'randrange', 'shuffle', 'normalvariate', 'lognormvariate', 'expovariate', 'vonmisesvariate',
           'gammavariate', 'triangular', 'gauss', 'betavariate', 'paretovariate', 'weibullvariate',
           'getstate', 'setstate', 'getrandbits', 'choices', 'randbytes', 'binomialvariate']

_N = 624
_M = 397
_MATRIX_A = 0x9908b0df
_UPPER = 0x80000000
_LOWER = 0x7fffffff
_RECIP_BPF = 1.0 / 9007199254740992.0


def _key_from_int(n):
    n = abs(n)
    key = []
    while n:
        key.append(n & 0xffffffff)
        n >>= 32
    return key or [0]


def _key_from_bytes(data):
    """Palavras de 32 bits, little-endian, do inteiro cujos bytes (big-endian) são `data`."""
    data = bytes(data)
    key = []
    i = len(data)
    while i > 0:
        lo = max(0, i - 4)
        word = 0
        for b in data[lo:i]:
            word = (word << 8) | b
        key.append(word)
        i = lo
    while len(key) > 1 and key[-1] == 0:
        key.pop()
    return key or [0]


class Random:
    VERSION = 3

    def __init__(self, x=None):
        self.seed(x)

    def seed(self, a=None, version=2):
        if a is None:
            key = _key_from_bytes(_os.urandom(32))
        elif isinstance(a, (str, bytes, bytearray)):
            if isinstance(a, str):
                a = a.encode()
            if version == 2:
                import hashlib
                a = bytes(a) + hashlib.sha512(a).digest()
            key = _key_from_bytes(a)
        elif isinstance(a, int):
            key = _key_from_int(a)
        else:
            key = _key_from_int(hash(a))
        self._init_by_array(key)
        self.gauss_next = None

    def _init_genrand(self, s):
        mt = [0] * _N
        mt[0] = s & 0xffffffff
        for i in range(1, _N):
            prev = mt[i - 1]
            mt[i] = (1812433253 * (prev ^ (prev >> 30)) + i) & 0xffffffff
        self._mt = mt
        self._index = _N

    def _init_by_array(self, key):
        self._init_genrand(19650218)
        mt = self._mt
        i = 1
        j = 0
        klen = len(key)
        k = _N if _N > klen else klen
        while k:
            prev = mt[i - 1]
            mt[i] = ((mt[i] ^ ((prev ^ (prev >> 30)) * 1664525)) + key[j] + j) & 0xffffffff
            i += 1
            j += 1
            if i >= _N:
                mt[0] = mt[_N - 1]
                i = 1
            if j >= klen:
                j = 0
            k -= 1
        k = _N - 1
        while k:
            prev = mt[i - 1]
            mt[i] = ((mt[i] ^ ((prev ^ (prev >> 30)) * 1566083941)) - i) & 0xffffffff
            i += 1
            if i >= _N:
                mt[0] = mt[_N - 1]
                i = 1
            k -= 1
        mt[0] = 0x80000000
        self._index = _N

    def _genrand_uint32(self):
        mt = self._mt
        if mt is None:
            # Instância global: a semente de entropia só é lida no primeiro uso.
            self.seed()
            mt = self._mt
        if self._index >= _N:
            for kk in range(_N):
                y = (mt[kk] & _UPPER) | (mt[(kk + 1) % _N] & _LOWER)
                v = mt[(kk + _M) % _N] ^ (y >> 1)
                if y & 1:
                    v ^= _MATRIX_A
                mt[kk] = v
            self._index = 0
        y = mt[self._index]
        self._index += 1
        y ^= y >> 11
        y ^= (y << 7) & 0x9d2c5680
        y ^= (y << 15) & 0xefc60000
        y ^= y >> 18
        return y

    def random(self):
        a = self._genrand_uint32() >> 5
        b = self._genrand_uint32() >> 6
        return (a * 67108864.0 + b) * _RECIP_BPF

    def getrandbits(self, k):
        if k < 0:
            raise ValueError('number of bits must be non-negative')
        if k == 0:
            return 0
        if k <= 32:
            return self._genrand_uint32() >> (32 - k)
        words = (k - 1) // 32 + 1
        result = 0
        shift = 0
        for i in range(words):
            r = self._genrand_uint32()
            if k < 32:
                r >>= 32 - k
            result |= r << shift
            shift += 32
            k -= 32
        return result

    def randbytes(self, n):
        out = bytearray()
        while len(out) < n:
            r = self._genrand_uint32()
            out += bytes([r & 255, (r >> 8) & 255, (r >> 16) & 255, (r >> 24) & 255])
        return bytes(out[:n])

    def getstate(self):
        return (self.VERSION, tuple(self._mt) + (self._index,), self.gauss_next)

    def setstate(self, state):
        version, internal, gauss = state
        self._mt = list(internal[:_N])
        self._index = internal[_N]
        self.gauss_next = gauss

    def _randbelow(self, n):
        if n <= 0:
            return 0
        k = n.bit_length()
        r = self.getrandbits(k)
        while r >= n:
            r = self.getrandbits(k)
        return r

    _randbelow_with_getrandbits = _randbelow

    def randrange(self, start, stop=None, step=1):
        istart = int(start)
        if istart != start:
            raise ValueError('non-integer arg 1 for randrange()')
        if stop is None:
            if istart > 0:
                return self._randbelow(istart)
            raise ValueError('empty range for randrange()')
        istop = int(stop)
        if istop != stop:
            raise ValueError('non-integer stop for randrange()')
        width = istop - istart
        istep = int(step)
        if istep != step:
            raise ValueError('non-integer step for randrange()')
        if istep == 1:
            if width > 0:
                return istart + self._randbelow(width)
            raise ValueError('empty range in randrange(%d, %d)' % (istart, istop))
        if istep > 0:
            n = (width + istep - 1) // istep
        elif istep < 0:
            n = (width + istep + 1) // istep
        else:
            raise ValueError('zero step for randrange()')
        if n <= 0:
            raise ValueError('empty range in randrange(%d, %d, %d)' % (istart, istop, istep))
        return istart + istep * self._randbelow(n)

    def randint(self, a, b):
        return self.randrange(a, b + 1)

    def choice(self, seq):
        if not len(seq):
            raise IndexError('Cannot choose from an empty sequence')
        return seq[self._randbelow(len(seq))]

    def shuffle(self, x):
        for i in reversed(range(1, len(x))):
            j = self._randbelow(i + 1)
            x[i], x[j] = x[j], x[i]

    def sample(self, population, k, *, counts=None):
        if not hasattr(population, '__len__') or isinstance(population, (set, frozenset)):
            if isinstance(population, (set, frozenset)):
                raise TypeError('Population must be a sequence.  For dicts or sets, use sorted(d).')
        n = len(population)
        if counts is not None:
            cum = []
            total = 0
            for c in counts:
                total += c
                cum.append(total)
            if total <= 0:
                raise ValueError('Total of counts must be greater than zero')
            selections = self.sample(range(total), k)
            import bisect
            return [population[bisect.bisect(cum, s)] for s in selections]
        if not 0 <= k <= n:
            raise ValueError('Sample larger than population or is negative')
        result = [None] * k
        setsize = 21
        if k > 5:
            setsize += 4 ** _ceil_log4(k * 3)
        if n <= setsize:
            pool = list(population)
            for i in range(k):
                j = self._randbelow(n - i)
                result[i] = pool[j]
                pool[j] = pool[n - i - 1]
        else:
            selected = set()
            for i in range(k):
                j = self._randbelow(n)
                while j in selected:
                    j = self._randbelow(n)
                selected.add(j)
                result[i] = population[j]
        return result

    def choices(self, population, weights=None, *, cum_weights=None, k=1):
        n = len(population)
        if cum_weights is None:
            if weights is None:
                n_f = float(n)
                return [population[int(self.random() * n_f)] for i in range(k)]
            cum = []
            total = 0
            for w in weights:
                total += w
                cum.append(total)
            cum_weights = cum
        elif weights is not None:
            raise TypeError('Cannot specify both weights and cumulative weights')
        if len(cum_weights) != n:
            raise ValueError('The number of weights does not match the population')
        import bisect
        total = cum_weights[-1] + 0.0
        hi = n - 1
        return [population[bisect.bisect(cum_weights, self.random() * total, 0, hi)] for i in range(k)]

    def uniform(self, a, b):
        return a + (b - a) * self.random()

    def triangular(self, low=0.0, high=1.0, mode=None):
        u = self.random()
        try:
            c = 0.5 if mode is None else (mode - low) / (high - low)
        except ZeroDivisionError:
            return low
        if u > c:
            u = 1.0 - u
            c = 1.0 - c
            low, high = high, low
        return low + (high - low) * _sqrt(u * c)

    def normalvariate(self, mu=0.0, sigma=1.0):
        while True:
            u1 = self.random()
            u2 = 1.0 - self.random()
            z = 1.7155277699214135 * (u1 - 0.5) / u2
            zz = z * z / 4.0
            if zz <= -_log(u2):
                break
        return mu + z * sigma

    def gauss(self, mu=0.0, sigma=1.0):
        z = self.gauss_next
        self.gauss_next = None
        if z is None:
            x2pi = self.random() * 2.0 * _pi
            g2rad = _sqrt(-2.0 * _log(1.0 - self.random()))
            z = _cos(x2pi) * g2rad
            self.gauss_next = _sin(x2pi) * g2rad
        return mu + z * sigma

    def lognormvariate(self, mu, sigma):
        return _exp(self.normalvariate(mu, sigma))

    def expovariate(self, lambd=1.0):
        return -_log(1.0 - self.random()) / lambd

    def paretovariate(self, alpha):
        u = 1.0 - self.random()
        return u ** (-1.0 / alpha)

    def weibullvariate(self, alpha, beta):
        u = 1.0 - self.random()
        return alpha * (-_log(u)) ** (1.0 / beta)

    def vonmisesvariate(self, mu, kappa):
        if kappa <= 1e-6:
            return 2.0 * _pi * self.random()
        s = 0.5 / kappa
        r = s + _sqrt(1.0 + s * s)
        while True:
            u1 = self.random()
            z = _cos(_pi * u1)
            d = z / (r + z)
            u2 = self.random()
            if u2 < 1.0 - d * d or u2 <= (1.0 - d) * _exp(d):
                break
        q = 1.0 / r
        f = (q + z) / (1.0 + q * z)
        u3 = self.random()
        if u3 > 0.5:
            theta = (mu + _acos(f)) % (2.0 * _pi)
        else:
            theta = (mu - _acos(f)) % (2.0 * _pi)
        return theta

    def gammavariate(self, alpha, beta):
        if alpha <= 0.0 or beta <= 0.0:
            raise ValueError('gammavariate: alpha and beta must be > 0.0')
        if alpha > 1.0:
            ainv = _sqrt(2.0 * alpha - 1.0)
            bbb = alpha - 1.3862943611198906
            ccc = alpha + ainv
            while True:
                u1 = self.random()
                if not 1e-7 < u1 < 0.9999999:
                    continue
                u2 = 1.0 - self.random()
                v = _log(u1 / (1.0 - u1)) / ainv
                x = alpha * _exp(v)
                z = u1 * u1 * u2
                r = bbb + ccc * v - x
                if r + 2.504077396776274 - 4.5 * z >= 0.0 or r >= _log(z):
                    return x * beta
        elif alpha == 1.0:
            return -_log(1.0 - self.random()) * beta
        else:
            while True:
                u = self.random()
                b = (_e + alpha) / _e
                p = b * u
                if p <= 1.0:
                    x = p ** (1.0 / alpha)
                else:
                    x = -_log((b - p) / alpha)
                u1 = self.random()
                if p > 1.0:
                    if u1 <= x ** (alpha - 1.0):
                        break
                elif u1 <= _exp(-x):
                    break
            return x * beta

    def betavariate(self, alpha, beta):
        y = self.gammavariate(alpha, 1.0)
        if y:
            return y / (y + self.gammavariate(beta, 1.0))
        return 0.0

    def binomialvariate(self, n=1, p=0.5):
        return sum(1 for _ in range(n) if self.random() < p)


_e = 2.718281828459045


def _ceil_log4(x):
    # ceil(log(x, 4)) sem logaritmo em ponto flutuante
    n = 0
    v = 1
    while v < x:
        v *= 4
        n += 1
    return n


class SystemRandom(Random):
    def random(self):
        return (int.from_bytes(_os.urandom(7), 'big') >> 3) * _RECIP_BPF

    def getrandbits(self, k):
        if k < 0:
            raise ValueError('number of bits must be non-negative')
        numbytes = (k + 7) // 8
        x = int.from_bytes(_os.urandom(numbytes), 'big')
        return x >> (numbytes * 8 - k)

    def randbytes(self, n):
        return _os.urandom(n)

    def seed(self, *args, **kwds):
        return None

    def getstate(self, *args, **kwds):
        raise NotImplementedError('System entropy source does not have state.')

    setstate = getstate


_inst = Random.__new__(Random)
_inst._mt = None
_inst._index = _N
_inst.gauss_next = None
seed = _inst.seed
random = _inst.random
uniform = _inst.uniform
triangular = _inst.triangular
randint = _inst.randint
choice = _inst.choice
randrange = _inst.randrange
sample = _inst.sample
shuffle = _inst.shuffle
choices = _inst.choices
normalvariate = _inst.normalvariate
lognormvariate = _inst.lognormvariate
expovariate = _inst.expovariate
vonmisesvariate = _inst.vonmisesvariate
gammavariate = _inst.gammavariate
gauss = _inst.gauss
betavariate = _inst.betavariate
paretovariate = _inst.paretovariate
weibullvariate = _inst.weibullvariate
getstate = _inst.getstate
setstate = _inst.setstate
getrandbits = _inst.getrandbits
randbytes = _inst.randbytes
binomialvariate = _inst.binomialvariate
