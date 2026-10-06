"""zlib: compressão deflate sobre o nativo `_zlib`."""

import _zlib
from binascii import crc32 as _crc32

MAX_WBITS = 15
DEFLATED = 8
DEF_MEM_LEVEL = 8
DEF_BUF_SIZE = 16384
Z_NO_COMPRESSION = 0
Z_BEST_SPEED = 1
Z_BEST_COMPRESSION = 9
Z_DEFAULT_COMPRESSION = -1
Z_FILTERED = 1
Z_HUFFMAN_ONLY = 2
Z_RLE = 3
Z_FIXED = 4
Z_DEFAULT_STRATEGY = 0
Z_NO_FLUSH = 0
Z_PARTIAL_FLUSH = 1
Z_SYNC_FLUSH = 2
Z_FULL_FLUSH = 3
Z_FINISH = 4
Z_BLOCK = 5
Z_TREES = 6
ZLIB_VERSION = '1.3.1'
ZLIB_RUNTIME_VERSION = '1.3.1'


class error(Exception):
    pass


def _wrap(fn, *args):
    try:
        return fn(*args)
    except ValueError as e:
        msg = str(e)
        if msg.startswith('Error ') or msg.startswith('Invalid init') or msg.startswith('Bad compression'):
            raise error(msg) from None
        raise


class _Compress:
    def __init__(self, raw):
        self._raw = raw

    def compress(self, data):
        return _wrap(self._raw.compress, data)

    def flush(self, mode=Z_FINISH):
        return _wrap(self._raw.flush, mode)

    def copy(self):
        raise NotImplementedError('Compress.copy() is not supported')


class _Decompress:
    def __init__(self, raw):
        self._raw = raw

    unused_data = property(lambda self: self._raw.unused_data)
    unconsumed_tail = property(lambda self: self._raw.unconsumed_tail)
    eof = property(lambda self: self._raw.eof)

    def decompress(self, data, max_length=0):
        return _wrap(self._raw.decompress, data, max_length)

    def flush(self, length=DEF_BUF_SIZE):
        return _wrap(self._raw.flush)

    def copy(self):
        raise NotImplementedError('Decompress.copy() is not supported')


def compress(data, /, level=-1, wbits=MAX_WBITS):
    return _wrap(_zlib.compress, data, level, wbits)


def decompress(data, /, wbits=MAX_WBITS, bufsize=DEF_BUF_SIZE):
    return _wrap(_zlib.decompress, data, wbits, bufsize)


def compressobj(level=-1, method=DEFLATED, wbits=MAX_WBITS, memLevel=DEF_MEM_LEVEL,
                strategy=Z_DEFAULT_STRATEGY, zdict=None):
    return _Compress(_wrap(_zlib.compressobj, level, method, wbits, memLevel, strategy))


def decompressobj(wbits=MAX_WBITS, zdict=b''):
    return _Decompress(_wrap(_zlib.decompressobj, wbits))


def crc32(data, value=0):
    return _crc32(data, value)


def adler32(data, value=1):
    return _zlib.adler32(data, value)
