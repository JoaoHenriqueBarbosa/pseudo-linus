"""MD5 hash algorithm. A shim sobre o `_hashimpl` nativo, com a superfície do `_md5` do CPython."""

import _hashimpl
from _hashbase import _adopt, _input


@_adopt
class MD5Type:
    """Type for md5 hash objects."""

    def __init__(self, data=None, string=None):
        self._h = _hashimpl.new('md5', _input(data, string))


def md5(data=b'', *, usedforsecurity=True, string=None):
    """Return a new MD5 hash object; optionally initialized with a bytes-like object."""
    return MD5Type(data, string)
