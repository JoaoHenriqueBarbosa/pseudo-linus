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
__version__ = '1.0'


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
    return _Decompress(_wrap(_zlib.decompressobj, wbits, zdict))


class _ZlibDecompressor:
    """Create a decompressor object for decompressing data incrementally.

  wbits
    The window buffer size and container format.
  zdict
    The predefined compression dictionary.  This is a sequence of bytes
    (such as a bytes object) containing subsequences that are expected
    to occur frequently in the data that is to be compressed.  Those
    subsequences that are expected to be most common should come at the
    end of the dictionary.  This must be the same dictionary as used by the
    compressor that produced the input data."""

    def __init__(self, wbits=MAX_WBITS, zdict=b''):
        self._raw = decompressobj(wbits, zdict)
        self._pending = b''
        self.eof = False
        self.unused_data = b''
        self.needs_input = True

    def decompress(self, data, max_length=-1):
        """Decompress *data*, returning uncompressed data as bytes.

If *max_length* is nonnegative, returns at most *max_length* bytes of
decompressed data. If this limit is reached and further output can be
produced, *self.needs_input* will be set to ``False``. In this case, the next
call to *decompress()* may provide *data* as b'' to obtain more of the output.

If all of the input data was decompressed and returned (either because this
was less than *max_length* bytes, or because *max_length* was negative),
*self.needs_input* will be set to True.

Attempting to decompress data after the end of stream is reached raises an
EOFError.  Any data found after the end of the stream is ignored and saved in
the unused_data attribute."""
        if self.eof:
            raise EOFError('End of stream already reached')
        data = self._pending + bytes(data)
        self._pending = b''
        if max_length == 0:
            # Sem espaço para saída: a entrada espera a próxima chamada.
            self._pending = data
            out = b''
        elif max_length < 0:
            out = self._raw.decompress(data)
        else:
            # O que o limite deixou de fora fica no `unconsumed_tail` do descompressor cru.
            out = self._raw.decompress(data, max_length)
        if self._raw.eof:
            self.eof = True
            self.unused_data = self._raw.unused_data
            self._pending = b''
        more_input = bool(self._pending or self._raw.unconsumed_tail)
        self.needs_input = not self.eof and not more_input and (max_length < 0 or len(out) < max_length)
        return out


def crc32(data, value=0):
    return _crc32(data, value)


def adler32(data, value=1):
    return _zlib.adler32(data, value)
