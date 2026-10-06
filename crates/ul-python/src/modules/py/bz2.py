"""Compressão bzip2 sobre os codecs do sandbox."""

import io
import _archive
from _archivefile import CompressedFile

__all__ = ['BZ2File', 'BZ2Compressor', 'BZ2Decompressor', 'open', 'compress', 'decompress']


def compress(data, compresslevel=9):
    if not 1 <= compresslevel <= 9:
        raise ValueError('compresslevel must be between 1 and 9')
    return _archive.bz2_compress(data, compresslevel)


def decompress(data):
    out = b''
    if not data:
        return out
    return _archive.bz2_decompress(data)


class BZ2Compressor:

    def __init__(self, compresslevel=9):
        if not 1 <= compresslevel <= 9:
            raise ValueError('compresslevel must be between 1 and 9')
        self._level = compresslevel
        self._chunks = []
        self._done = False

    def compress(self, data):
        if self._done:
            raise ValueError('Compressor has been flushed')
        self._chunks.append(bytes(data))
        return b''

    def flush(self):
        if self._done:
            raise ValueError('Repeated call to flush()')
        self._done = True
        return _archive.bz2_compress(b''.join(self._chunks), self._level)


class BZ2Decompressor:

    def __init__(self):
        self._buf = b''
        self.eof = False
        self.unused_data = b''

    @property
    def needs_input(self):
        return not self.eof

    def decompress(self, data, max_length=-1):
        if self.eof:
            raise EOFError('End of stream already reached')
        self._buf += bytes(data)
        try:
            out = _archive.bz2_decompress(self._buf)
        except EOFError:
            return b''
        self.eof = True
        self._buf = b''
        if max_length is not None and 0 <= max_length < len(out):
            out = out[:max_length]
        return out


class BZ2File(CompressedFile):

    def __init__(self, filename, mode='r', *, compresslevel=9):
        CompressedFile.__init__(self, filename, mode,
                                lambda d: compress(d, compresslevel), decompress, 'bz2')


def open(filename, mode='rb', compresslevel=9, encoding=None, errors=None, newline=None):
    if 't' in mode:
        if 'b' in mode:
            raise ValueError('Invalid mode: %r' % (mode,))
    elif encoding is not None or errors is not None or newline is not None:
        raise ValueError("Argument 'encoding' not supported in binary mode")
    bz_mode = mode.replace('t', '')
    binary_file = BZ2File(filename, bz_mode, compresslevel=compresslevel)
    if 't' in mode:
        encoding = io.text_encoding(encoding)
        return io.TextIOWrapper(binary_file, encoding, errors, newline)
    return binary_file
