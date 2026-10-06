"""`_random`: a base `Random` do módulo `random`, sobre o MT19937 nativo (`_mt`). A semente de entropia só é
lida no primeiro uso do gerador (importar `random` não toca o sistema), e `seed(n)`, `getstate()` e
`setstate()` seguem o `_randommodule.c` do CPython."""

import _mt
import _os

__all__ = ['Random']


def _key(n):
    """Palavras de 32 bits, menos significativa primeiro, de `abs(n)` (ao menos uma)."""
    n = abs(n)
    key = []
    while n:
        key.append(n & 0xffffffff)
        n >>= 32
    return key or [0]


class Random:

    def __new__(cls, *args, **kwargs):
        if cls is Random and kwargs:
            raise TypeError('Random() takes no keyword arguments')
        if len(args) > 1:
            raise TypeError('Random() expected at most 1 argument, got %d' % len(args))
        self = object.__new__(cls)
        self._rng = _mt.new()
        if args:
            Random.seed(self, args[0])
        return self

    def _seed_entropy(self):
        words = []
        data = _os.urandom(32)
        for i in range(0, len(data), 4):
            words.append(int.from_bytes(data[i:i + 4], 'little'))
        self._rng.init_by_array(words)

    def seed(self, n=None, /):
        if n is None:
            # Descarta o estado: o gerador volta a ser semeado com entropia na próxima leitura.
            self._rng = _mt.new()
            return None
        if not isinstance(n, int):
            n = hash(n) & 0xffffffffffffffff
        self._rng.init_by_array(_key(n))
        return None

    def random(self):
        try:
            return self._rng.random()
        except RuntimeError:
            self._seed_entropy()
            return self._rng.random()

    def getrandbits(self, k, /):
        if k <= 62:
            try:
                return self._rng.getrandbits(k)
            except RuntimeError:
                self._seed_entropy()
                return self._rng.getrandbits(k)
        if not isinstance(k, int):
            raise TypeError("'%s' object cannot be interpreted as an integer" % type(k).__name__)
        result = 0
        shift = 0
        for _ in range((k - 1) // 32 + 1):
            try:
                r = self._rng.genrand32()
            except RuntimeError:
                self._seed_entropy()
                r = self._rng.genrand32()
            if k < 32:
                r >>= 32 - k
            result |= r << shift
            shift += 32
            k -= 32
        return result

    def getstate(self):
        if not self._rng.seeded():
            self._seed_entropy()
        return self._rng.getstate()

    def setstate(self, state, /):
        if not isinstance(state, tuple):
            raise TypeError('state vector must be a tuple')
        self._rng.setstate(state)
        return None
