"""SHA-2 hash algorithms. A shim sobre o `_hashimpl` nativo, com a superfície do `_sha2` do CPython."""

import _hashimpl
from _hashbase import _adopt, _input


@_adopt
class SHA224Type:
    """Type for sha224 hash objects."""

    def __init__(self, data=None, string=None):
        self._h = _hashimpl.new('sha224', _input(data, string))


@_adopt
class SHA256Type:
    """Type for sha256 hash objects."""

    def __init__(self, data=None, string=None):
        self._h = _hashimpl.new('sha256', _input(data, string))


@_adopt
class SHA384Type:
    """Type for sha384 hash objects."""

    def __init__(self, data=None, string=None):
        self._h = _hashimpl.new('sha384', _input(data, string))


@_adopt
class SHA512Type:
    """Type for sha512 hash objects."""

    def __init__(self, data=None, string=None):
        self._h = _hashimpl.new('sha512', _input(data, string))


def sha224(data=b'', *, usedforsecurity=True, string=None):
    """Return a new SHA-224 hash object; optionally initialized with a bytes-like object."""
    return SHA224Type(data, string)


def sha256(data=b'', *, usedforsecurity=True, string=None):
    """Return a new SHA-256 hash object; optionally initialized with a bytes-like object."""
    return SHA256Type(data, string)


def sha384(data=b'', *, usedforsecurity=True, string=None):
    """Return a new SHA-384 hash object; optionally initialized with a bytes-like object."""
    return SHA384Type(data, string)


def sha512(data=b'', *, usedforsecurity=True, string=None):
    """Return a new SHA-512 hash object; optionally initialized with a bytes-like object."""
    return SHA512Type(data, string)
