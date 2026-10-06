"""gzip: contêiner gzip (RFC 1952) sobre o `zlib` deflate cru."""

import io
import os
import struct
import time
import zlib

__all__ = ['BadGzipFile', 'GzipFile', 'open', 'compress', 'decompress']

FTEXT, FHCRC, FEXTRA, FNAME, FCOMMENT = 1, 2, 4, 8, 16
READ = 'rb'
WRITE = 'wb'
_COMPRESS_LEVEL_FAST = 1
_COMPRESS_LEVEL_TRADEOFF = 6
_COMPRESS_LEVEL_BEST = 9


class BadGzipFile(OSError):
    """Arquivo gzip inválido."""


def _header(fname, mtime, level):
    flags = FNAME if fname else 0
    xfl = 2 if level == _COMPRESS_LEVEL_BEST else 4 if level == _COMPRESS_LEVEL_FAST else 0
    head = b'\x1f\x8b\x08' + bytes([flags]) + struct.pack('<L', int(mtime) & 0xffffffff) + bytes([xfl, 255])
    if fname:
        head += fname + b'\x00'
    return head


def _trailer(crc, size):
    return struct.pack('<LL', crc & 0xffffffff, size & 0xffffffff)


def compress(data, compresslevel=_COMPRESS_LEVEL_BEST, *, mtime=None):
    if mtime is None:
        mtime = time.time()
    co = zlib.compressobj(compresslevel, zlib.DEFLATED, -zlib.MAX_WBITS)
    body = co.compress(data) + co.flush()
    return _header(b'', mtime, compresslevel) + body + _trailer(zlib.crc32(data), len(data))


def _read_member(data, pos):
    """Lê um membro a partir de `pos`; devolve (bytes, posição seguinte)."""
    if len(data) - pos < 10:
        raise EOFError('Compressed file ended before the end-of-stream marker was reached')
    if data[pos:pos + 2] != b'\x1f\x8b':
        raise BadGzipFile('Not a gzipped file (%r)' % data[pos:pos + 2])
    if data[pos + 2] != 8:
        raise BadGzipFile('Unknown compression method')
    flag = data[pos + 3]
    p = pos + 10
    if flag & FEXTRA:
        xlen = data[p] | (data[p + 1] << 8)
        p += 2 + xlen
    if flag & FNAME:
        while data[p] != 0:
            p += 1
        p += 1
    if flag & FCOMMENT:
        while data[p] != 0:
            p += 1
        p += 1
    if flag & FHCRC:
        p += 2
    d = zlib.decompressobj(-zlib.MAX_WBITS)
    try:
        out = d.decompress(data[p:])
    except zlib.error as e:
        raise BadGzipFile(str(e))
    if not d.eof:
        raise EOFError('Compressed file ended before the end-of-stream marker was reached')
    rest = d.unused_data
    if len(rest) < 8:
        raise EOFError('Compressed file ended before the end-of-stream marker was reached')
    crc, size = struct.unpack('<LL', rest[:8])
    if crc != zlib.crc32(out):
        raise BadGzipFile('CRC check failed %s != %s' % (hex(crc), hex(zlib.crc32(out))))
    if size != (len(out) & 0xffffffff):
        raise BadGzipFile('Incorrect length of data produced')
    return out, len(data) - len(rest) + 8


def decompress(data):
    out = []
    pos = 0
    while pos < len(data):
        chunk, pos = _read_member(data, pos)
        out.append(chunk)
        # preenchimento de zeros entre membros é aceito
        while pos < len(data) and data[pos] == 0:
            pos += 1
    return b''.join(out)


class GzipFile(io.BufferedIOBase):
    myfileobj = None

    def __init__(self, filename=None, mode=None, compresslevel=_COMPRESS_LEVEL_BEST, fileobj=None, mtime=None):
        if mode and ('t' in mode or 'U' in mode):
            raise ValueError('Invalid mode: {!r}'.format(mode))
        if mode and 'b' not in mode:
            mode += 'b'
        if fileobj is None:
            fileobj = self.myfileobj = io.open(filename, mode or 'rb')
        if filename is None:
            filename = getattr(fileobj, 'name', '')
            if not isinstance(filename, (str, bytes)):
                filename = ''
        else:
            filename = os.fspath(filename)
        origmode = mode
        if mode is None:
            mode = getattr(fileobj, 'mode', 'rb')
        if mode.startswith('r'):
            self.mode = READ
            self._buf = None
            self._pos = 0
            self._data = fileobj.read()
        elif mode.startswith(('w', 'a', 'x')):
            if origmode is None:
                import warnings
                warnings.warn("GzipFile was opened for writing, but this will change in future Python "
                              "releases.  Specify the mode argument for opening it for writing.",
                              FutureWarning, 2)
            self.mode = WRITE
            self._chunks = []
            self._size = 0
            self._crc = 0
            self._level = compresslevel
        else:
            raise ValueError('Invalid mode: {!r}'.format(mode))
        self.name = filename
        self.fileobj = fileobj
        self.mtime = mtime
        self._closed = False
        if self.mode == WRITE:
            self._write_header = True

    @property
    def closed(self):
        return self._closed

    def readable(self):
        return self.mode == READ

    def writable(self):
        return self.mode == WRITE

    def seekable(self):
        return True

    def _load(self):
        if self._buf is None:
            self._buf = decompress(self._data)

    def read(self, size=-1):
        if self.mode != READ:
            raise OSError(9, 'read() on write-only GzipFile object')
        self._load()
        if size is None or size < 0:
            out = self._buf[self._pos:]
        else:
            out = self._buf[self._pos:self._pos + size]
        self._pos += len(out)
        return out

    read1 = read

    def peek(self, n):
        self._load()
        return self._buf[self._pos:self._pos + n]

    def readline(self, size=-1):
        self._load()
        end = self._buf.find(b'\n', self._pos)
        end = len(self._buf) if end < 0 else end + 1
        if size is not None and size >= 0:
            end = min(end, self._pos + size)
        out = self._buf[self._pos:end]
        self._pos = end
        return out

    def __iter__(self):
        while True:
            line = self.readline()
            if not line:
                return
            yield line

    def readlines(self, hint=-1):
        return list(self)

    def tell(self):
        if self.mode == WRITE:
            return self._size
        return self._pos

    def seek(self, offset, whence=0):
        if self.mode == WRITE:
            raise OSError('Seek from end not supported' if whence else 'Negative seek in write mode')
        self._load()
        if whence == 1:
            offset += self._pos
        elif whence == 2:
            offset += len(self._buf)
        self._pos = max(0, offset)
        return self._pos

    def write(self, data):
        if self.mode != WRITE:
            raise OSError(9, 'write() on read-only GzipFile object')
        if self._closed:
            raise ValueError('write() on closed GzipFile object')
        if isinstance(data, str):
            raise TypeError("a bytes-like object is required, not 'str'")
        data = bytes(data)
        self._chunks.append(data)
        self._size += len(data)
        self._crc = zlib.crc32(data, self._crc)
        return len(data)

    def flush(self, zlib_mode=None):
        pass

    def close(self):
        if self._closed:
            return
        self._closed = True
        try:
            if self.mode == WRITE:
                mtime = self.mtime if self.mtime is not None else time.time()
                fname = os.path.basename(self.name)
                if fname.endswith('.gz'):
                    fname = fname[:-3]
                fname = fname.encode('latin-1') if isinstance(fname, str) else fname
                co = zlib.compressobj(self._level, zlib.DEFLATED, -zlib.MAX_WBITS)
                body = b''.join(co.compress(c) for c in self._chunks) + co.flush()
                self.fileobj.write(_header(fname, mtime, self._level) + body + _trailer(self._crc, self._size))
        finally:
            if self.myfileobj:
                self.myfileobj.close()
                self.myfileobj = None
            self.fileobj = None

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def fileno(self):
        return self.fileobj.fileno()


def open(filename, mode='rb', compresslevel=_COMPRESS_LEVEL_BEST, encoding=None, errors=None, newline=None):
    if 't' in mode:
        if 'b' in mode:
            raise ValueError('Invalid mode: %r' % (mode,))
    else:
        if encoding is not None:
            raise ValueError("Argument 'encoding' not supported in binary mode")
        if errors is not None:
            raise ValueError("Argument 'errors' not supported in binary mode")
        if newline is not None:
            raise ValueError("Argument 'newline' not supported in binary mode")
    gz_mode = mode.replace('t', '')
    if isinstance(filename, (str, bytes, os.PathLike)):
        binary_file = GzipFile(filename, gz_mode, compresslevel)
    elif hasattr(filename, 'read') or hasattr(filename, 'write'):
        binary_file = GzipFile(None, gz_mode, compresslevel, filename)
    else:
        raise TypeError('filename must be a str or bytes object, or a file')
    if 't' in mode:
        return io.TextIOWrapper(binary_file, encoding, errors, newline)
    return binary_file
