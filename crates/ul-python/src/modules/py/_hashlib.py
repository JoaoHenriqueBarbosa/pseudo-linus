"""OpenSSL interface for hashlib module. A shim sobre o `_hashimpl` nativo, com a superfície do `_hashlib` do CPython
3.13 (OpenSSL 3.5 do Debian 13)."""

import _hashimpl

_GIL_MINSIZE = 2048

openssl_md_meth_names = frozenset({
    'blake2b512', 'blake2s256', 'md5', 'md5-sha1', 'ripemd160', 'sha1', 'sha224', 'sha256', 'sha384', 'sha3_224',
    'sha3_256', 'sha3_384', 'sha3_512', 'sha512', 'sha512_224', 'sha512_256', 'shake_128', 'shake_256', 'sm3',
})

_INT_MAX = 2147483647


class UnsupportedDigestmodError(ValueError):
    pass


class HASH:
    """A hash is an object used to calculate a checksum of a string of information.

Methods:

update() -- updates the current digest with an additional string
digest() -- return the current digest value
hexdigest() -- return the current digest as a string of hexadecimal digits
copy() -- return a copy of the current hash object

Attributes:

name -- the hash algorithm being used by this object
digest_size -- number of bytes in this hashes output"""

    def __new__(cls, *args, **kwargs):
        raise TypeError("cannot create '_hashlib.HASH' instances")


class HASHXOF(HASH):
    """A hash is an object used to calculate a checksum of a string of information.

Methods:

update() -- updates the current digest with an additional string
digest(length) -- return the current digest value
hexdigest(length) -- return the current digest as a string of hexadecimal digits
copy() -- return a copy of the current hash object

Attributes:

name -- the hash algorithm being used by this object
digest_size -- number of bytes in this hashes output"""

    def __new__(cls, *args, **kwargs):
        raise TypeError("cannot create '_hashlib.HASHXOF' instances")


class HMAC:
    """The object used to calculate HMAC of a message.

Methods:

update() -- updates the current digest with an additional string
digest() -- return the current digest value
hexdigest() -- return the current digest as a string of hexadecimal digits
copy() -- return a copy of the current hash object

Attributes:

name -- the name, including the hash algorithm used by this object
digest_size -- number of bytes in digest() output"""

    def __new__(cls, *args, **kwargs):
        raise TypeError("cannot create '_hashlib.HMAC' instances")


def _data(data, string):
    """O dado inicial: `data` ou o `string` de palavra-chave (os dois juntos são recusados, como no CPython)."""
    if string is not None:
        if data != b'':
            raise TypeError("'data' and 'string' are mutually exclusive "
                            "and support for 'string' keyword parameter is slated for removal in a future version.")
        return string
    return data


def new(name, data=b'', *, usedforsecurity=True, string=None):
    """Return a new hash object using the named algorithm.

An optional string argument may be provided and will be automatically hashed.

The MD5 and SHA1 algorithms are always supported."""
    return _hashimpl.new(name, _data(data, string))


def openssl_md5(data=b'', *, usedforsecurity=True, string=None):
    """Returns a md5 hash object; optionally initialized with a string"""
    return _hashimpl.new('md5', _data(data, string))


def openssl_sha1(data=b'', *, usedforsecurity=True, string=None):
    """Returns a sha1 hash object; optionally initialized with a string"""
    return _hashimpl.new('sha1', _data(data, string))


def openssl_sha224(data=b'', *, usedforsecurity=True, string=None):
    """Returns a sha224 hash object; optionally initialized with a string"""
    return _hashimpl.new('sha224', _data(data, string))


def openssl_sha256(data=b'', *, usedforsecurity=True, string=None):
    """Returns a sha256 hash object; optionally initialized with a string"""
    return _hashimpl.new('sha256', _data(data, string))


def openssl_sha384(data=b'', *, usedforsecurity=True, string=None):
    """Returns a sha384 hash object; optionally initialized with a string"""
    return _hashimpl.new('sha384', _data(data, string))


def openssl_sha512(data=b'', *, usedforsecurity=True, string=None):
    """Returns a sha512 hash object; optionally initialized with a string"""
    return _hashimpl.new('sha512', _data(data, string))


def openssl_sha3_224(data=b'', *, usedforsecurity=True, string=None):
    """Returns a sha3-224 hash object; optionally initialized with a string"""
    return _hashimpl.new('sha3_224', _data(data, string))


def openssl_sha3_256(data=b'', *, usedforsecurity=True, string=None):
    """Returns a sha3-256 hash object; optionally initialized with a string"""
    return _hashimpl.new('sha3_256', _data(data, string))


def openssl_sha3_384(data=b'', *, usedforsecurity=True, string=None):
    """Returns a sha3-384 hash object; optionally initialized with a string"""
    return _hashimpl.new('sha3_384', _data(data, string))


def openssl_sha3_512(data=b'', *, usedforsecurity=True, string=None):
    """Returns a sha3-512 hash object; optionally initialized with a string"""
    return _hashimpl.new('sha3_512', _data(data, string))


def openssl_shake_128(data=b'', *, usedforsecurity=True, string=None):
    """Returns a shake-128 variable hash object; optionally initialized with a string"""
    return _hashimpl.new('shake_128', _data(data, string))


def openssl_shake_256(data=b'', *, usedforsecurity=True, string=None):
    """Returns a shake-256 variable hash object; optionally initialized with a string"""
    return _hashimpl.new('shake_256', _data(data, string))


def get_fips_mode():
    """Determine the OpenSSL FIPS mode of operation.

For OpenSSL 3.0.0 and newer it returns the state of the default provider
in the default OSSL context. It's not quite the same as FIPS_mode() but good
enough for unittests.

Effectively any non-zero return value indicates FIPS mode;
values other than 1 may have additional significance."""
    return 0


def compare_digest(a, b, /):
    """Return 'a == b'.

This function uses an approach designed to prevent
timing analysis, making it appropriate for cryptography.

a and b must both be of the same type: either str (ASCII only),
or any bytes-like object.

Note: If a and b are of different lengths, or if an error occurs,
a timing attack could theoretically reveal information about the
types and lengths of a and b--but not their values."""
    if isinstance(a, str) and isinstance(b, str):
        if not (a.isascii() and b.isascii()):
            raise TypeError("comparing strings with non-ASCII characters is not supported")
        a, b = a.encode(), b.encode()
    elif not (isinstance(a, (bytes, bytearray, memoryview)) and isinstance(b, (bytes, bytearray, memoryview))):
        raise TypeError(
            "unsupported operand types(s) or combination of types: "
            f"'{type(a).__name__}' and '{type(b).__name__}'")
    a, b = bytes(a), bytes(b)
    result = len(a) ^ len(b)
    for x, y in zip(a, b):
        result |= x ^ y
    return result == 0


def _digest_name(digestmod):
    """O nome do algoritmo de um `digestmod` (texto ou construtor `openssl_*` do `hashlib`)."""
    if isinstance(digestmod, str):
        return digestmod
    name = getattr(digestmod, '__name__', '')
    if name.startswith('openssl_') and getattr(digestmod, '__module__', None) == '_hashlib':
        return name[len('openssl_'):]
    raise UnsupportedDigestmodError('Unsupported digestmod %r' % (digestmod,))


def hmac_new(key, msg=b'', digestmod=None):
    """Return a new hmac object."""
    if digestmod is None:
        raise TypeError("Missing required parameter 'digestmod'.")
    if msg is None:
        msg = b''
    try:
        return _hashimpl.hmac_new(_digest_name(digestmod), key, msg)
    except UnsupportedDigestmodError:
        raise
    except ValueError as e:
        raise UnsupportedDigestmodError(str(e)) from None


def hmac_digest(key, msg, digest):
    """Single-shot HMAC."""
    try:
        return _hashimpl.hmac_digest(_digest_name(digest), key, msg)
    except UnsupportedDigestmodError:
        raise
    except ValueError as e:
        raise UnsupportedDigestmodError(str(e)) from None


def pbkdf2_hmac(hash_name, password, salt, iterations, dklen=None):
    """Password based key derivation function 2 (PKCS #5 v2.0) with HMAC as pseudorandom function."""
    digest_size = _hashimpl.new(hash_name).digest_size
    if iterations < 1:
        raise ValueError("iteration value must be greater than 0.")
    if iterations > _INT_MAX:
        raise OverflowError("iteration value is too great.")
    if dklen is None:
        dklen = digest_size
    if dklen < 1:
        raise ValueError("key length must be greater than 0.")
    if dklen > _INT_MAX:
        raise OverflowError("key length is too great.")
    return _hashimpl.pbkdf2(hash_name, password, salt, iterations, dklen)


def scrypt(password, *, salt=None, n=None, r=None, p=None, maxmem=0, dklen=64):
    """scrypt password-based key derivation function."""
    if salt is None:
        raise TypeError("salt is required")
    if n is None:
        raise TypeError("n is required and must be an unsigned int")
    if r is None:
        raise TypeError("r is required and must be an unsigned int")
    if p is None:
        raise TypeError("p is required and must be an unsigned int")
    if n < 2 or n & (n - 1):
        raise ValueError("n must be a power of 2.")
    if maxmem < 0 or maxmem > _INT_MAX:
        raise ValueError("maxmem must be positive and smaller than %d" % _INT_MAX)
    if dklen < 1 or dklen > _INT_MAX:
        raise ValueError("dklen must be greater than 0 and smaller than %d" % _INT_MAX)
    # Os limites do `EVP_PBE_scrypt` (RFC 7914 e a memória permitida, 32 MiB sem `maxmem`).
    limit = maxmem or 32 * 1024 * 1024
    memory = 128 * r * p + 128 * r * (n + 2) if r > 0 and p > 0 else 0
    if r < 1 or p < 1 or r * p >= 1 << 30 or (16 * r <= 63 and n >= 1 << (16 * r)) or memory > limit:
        raise ValueError("Invalid parameter combination for n, r, p, maxmem.")
    return _hashimpl.scrypt(password, salt, n, r, p, dklen)
