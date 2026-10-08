"""SHA-3 hash algorithms. A shim sobre o `_hashimpl` nativo, com a superfície do `_sha3` do CPython."""

import _hashimpl
from _hashbase import _adopt, _input

implementation = 'HACL'


@_adopt
class sha3_224:
    """Return a new SHA3 hash object with a hashbit length of 28 bytes."""
    _algorithm = 'sha3_224'

    def __init__(self, data=None, *, usedforsecurity=True, string=None):
        self._h = _hashimpl.new(self._algorithm, _input(data, string))


@_adopt
class sha3_256:
    """Return a new SHA3 hash object with a hashbit length of 32 bytes."""
    _algorithm = 'sha3_256'

    def __init__(self, data=None, *, usedforsecurity=True, string=None):
        self._h = _hashimpl.new(self._algorithm, _input(data, string))


@_adopt
class sha3_384:
    """Return a new SHA3 hash object with a hashbit length of 48 bytes."""
    _algorithm = 'sha3_384'

    def __init__(self, data=None, *, usedforsecurity=True, string=None):
        self._h = _hashimpl.new(self._algorithm, _input(data, string))


@_adopt
class sha3_512:
    """Return a new SHA3 hash object with a hashbit length of 64 bytes."""
    _algorithm = 'sha3_512'

    def __init__(self, data=None, *, usedforsecurity=True, string=None):
        self._h = _hashimpl.new(self._algorithm, _input(data, string))


@_adopt
class shake_128:
    """Return a new SHAKE hash object with a hashbit length of 16 bytes."""
    _algorithm = 'shake_128'

    def __init__(self, data=None, *, usedforsecurity=True, string=None):
        self._h = _hashimpl.new(self._algorithm, _input(data, string))

    def digest(self, length):
        """Return the digest value as a bytes object."""
        return self._h.digest(length)

    def hexdigest(self, length):
        """Return the digest value as a string of hexadecimal digits."""
        return self._h.hexdigest(length)


@_adopt
class shake_256:
    """Return a new SHAKE hash object with a hashbit length of 32 bytes."""
    _algorithm = 'shake_256'

    def __init__(self, data=None, *, usedforsecurity=True, string=None):
        self._h = _hashimpl.new(self._algorithm, _input(data, string))

    def digest(self, length):
        """Return the digest value as a bytes object."""
        return self._h.digest(length)

    def hexdigest(self, length):
        """Return the digest value as a string of hexadecimal digits."""
        return self._h.hexdigest(length)
