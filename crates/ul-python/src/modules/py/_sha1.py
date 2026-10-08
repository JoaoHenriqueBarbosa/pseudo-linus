"""SHA-1 hash algorithm. A shim sobre o `_hashimpl` nativo, com a superfície do `_sha1` do CPython."""

import _hashimpl
from _hashbase import _adopt, _input


@_adopt
class SHA1Type:
    """Type for sha1 hash objects."""

    def __init__(self, data=None, string=None):
        self._h = _hashimpl.new('sha1', _input(data, string))


def sha1(data=b'', *, usedforsecurity=True, string=None):
    """Return a new SHA1 hash object; optionally initialized with a bytes-like object."""
    return SHA1Type(data, string)
