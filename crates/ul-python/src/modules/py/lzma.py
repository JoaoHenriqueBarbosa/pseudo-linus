"""Compressão xz e lzma sobre os codecs do sandbox (formatos XZ e ALONE; sem filtros brutos)."""

import io
import _archive
from _archivefile import CompressedFile

CHECK_NONE = 0
CHECK_CRC32 = 1
CHECK_CRC64 = 4
CHECK_SHA256 = 10
CHECK_ID_MAX = 15
CHECK_UNKNOWN = 16
FILTER_LZMA1 = 0x4000000000000001
FILTER_LZMA2 = 0x21
FILTER_DELTA = 0x03
FILTER_X86 = 0x04
FILTER_IA64 = 0x06
FILTER_ARM = 0x07
FILTER_ARMTHUMB = 0x08
FILTER_SPARC = 0x09
FILTER_POWERPC = 0x05
FORMAT_AUTO = 0
FORMAT_XZ = 1
FORMAT_ALONE = 2
FORMAT_RAW = 3
MF_HC3 = 0x03
MF_HC4 = 0x04
MF_BT2 = 0x12
MF_BT3 = 0x13
MF_BT4 = 0x14
MODE_FAST = 1
MODE_NORMAL = 2
PRESET_DEFAULT = 6
PRESET_EXTREME = 0x80000000

__all__ = [n for n in dir() if n.isupper()] + [
    'LZMACompressor', 'LZMADecompressor', 'LZMAFile', 'LZMAError', 'open', 'compress',
    'decompress', 'is_check_supported']


class LZMAError(Exception):
    pass


def is_check_supported(check_id):
    return check_id in (CHECK_NONE, CHECK_CRC32, CHECK_CRC64, CHECK_SHA256)


def _preset(preset):
    if preset is None:
        return 6
    return preset & 0x1f


def _is_xz(data):
    return data[:6] == b'\xfd7zXZ\x00'


def compress(data, format=FORMAT_XZ, check=-1, preset=None, filters=None):
    if filters is not None or format == FORMAT_RAW:
        raise LZMAError('filter chains are not supported')
    if format == FORMAT_ALONE:
        return _archive.lzma_compress(bytes(data), _preset(preset))
    if format != FORMAT_XZ:
        raise ValueError('Invalid container format: %d' % format)
    return _archive.xz_compress(bytes(data), _preset(preset))


def decompress(data, format=FORMAT_AUTO, memlimit=None, filters=None):
    data = bytes(data)
    if filters is not None or format == FORMAT_RAW:
        raise LZMAError('filter chains are not supported')
    if not data:
        raise LZMAError('Compressed data ended before the end-of-stream marker was reached')
    try:
        if format == FORMAT_XZ or (format == FORMAT_AUTO and _is_xz(data)):
            return _archive.xz_decompress(data)
        return _archive.lzma_decompress(data)
    except EOFError:
        raise LZMAError('Compressed data ended before the end-of-stream marker was reached')
    except OSError as e:
        raise LZMAError('Input format not supported by decoder') from None


class LZMACompressor:

    def __init__(self, format=FORMAT_XZ, check=-1, preset=None, filters=None):
        self._format = format
        self._preset = preset
        self._filters = filters
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
        return compress(b''.join(self._chunks), self._format, preset=self._preset, filters=self._filters)


class LZMADecompressor:

    def __init__(self, format=FORMAT_AUTO, memlimit=None, filters=None):
        self._format = format
        self._buf = b''
        self.eof = False
        self.unused_data = b''
        self.check = CHECK_UNKNOWN

    @property
    def needs_input(self):
        return not self.eof

    def decompress(self, data, max_length=-1):
        if self.eof:
            raise EOFError('Already at end of stream')
        self._buf += bytes(data)
        try:
            out = decompress(self._buf, self._format)
        except LZMAError:
            if self._buf and self._format != FORMAT_ALONE and _is_xz(self._buf) is False and len(self._buf) > 13:
                raise
            return b''
        self.eof = True
        self._buf = b''
        if max_length is not None and 0 <= max_length < len(out):
            out = out[:max_length]
        return out


class LZMAFile(CompressedFile):

    def __init__(self, filename=None, mode='r', *, format=None, check=-1, preset=None, filters=None):
        if mode.replace('b', '') in ('r', ''):
            fmt = FORMAT_AUTO if format is None else format
            CompressedFile.__init__(self, filename, mode, None, lambda d: decompress(d, fmt), 'lzma')
        else:
            fmt = FORMAT_XZ if format is None else format
            CompressedFile.__init__(self, filename, mode, lambda d: compress(d, fmt, check, preset), None, 'lzma')


def open(filename, mode='rb', *, format=None, check=-1, preset=None, filters=None,
         encoding=None, errors=None, newline=None):
    if 't' in mode:
        if 'b' in mode:
            raise ValueError('Invalid mode: %r' % (mode,))
    elif encoding is not None or errors is not None or newline is not None:
        raise ValueError("Argument 'encoding' not supported in binary mode")
    lz_mode = mode.replace('t', '')
    binary_file = LZMAFile(filename, lz_mode, format=format, check=check, preset=preset, filters=filters)
    if 't' in mode:
        encoding = io.text_encoding(encoding)
        return io.TextIOWrapper(binary_file, encoding, errors, newline)
    return binary_file
