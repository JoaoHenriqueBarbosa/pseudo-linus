"""Arquivo comprimido aberto inteiro na memória: lê e descomprime de uma vez, ou acumula e comprime ao fechar."""

import io


class CompressedFile(io.BufferedIOBase):

    def __init__(self, filename, mode, compress, decompress, error_name):
        mode = mode.replace('t', '')
        if mode in ('', 'r', 'rb'):
            self._mode = 'r'
        elif mode in ('w', 'wb', 'x', 'xb', 'a', 'ab'):
            self._mode = 'w'
        else:
            raise ValueError('Invalid mode: %r' % (mode,))
        self._compress = compress
        self._decompress = decompress
        self._own = False
        if isinstance(filename, (str, bytes)) or hasattr(filename, '__fspath__'):
            open_mode = {'r': 'rb', 'w': 'ab' if mode.startswith('a') else ('xb' if mode.startswith('x') else 'wb')}[self._mode]
            self._fp = io.open(filename, open_mode)
            self._own = True
        elif hasattr(filename, 'read') or hasattr(filename, 'write'):
            self._fp = filename
        else:
            raise TypeError('filename must be a str, bytes, file or PathLike object')
        self.name = getattr(self._fp, 'name', filename if isinstance(filename, (str, bytes)) else '')
        self._closed = False
        self._loaded = self._mode != 'r'
        self._data = io.BytesIO()

    @property
    def _buf(self):
        if not self._loaded:
            raw = self._fp.read()
            self._data = io.BytesIO(self._decompress(raw) if raw else b'')
            self._loaded = True
        return self._data

    @property
    def closed(self):
        return self._closed

    @property
    def mode(self):
        return 'rb' if self._mode == 'r' else 'wb'

    def readable(self):
        return self._mode == 'r'

    def writable(self):
        return self._mode == 'w'

    def seekable(self):
        return self._mode == 'r'

    def fileno(self):
        return self._fp.fileno()

    def _check_read(self):
        self._check_closed()
        if self._mode != 'r':
            raise io.UnsupportedOperation('File not open for reading')

    def read(self, size=-1):
        self._check_read()
        return self._buf.read(-1 if size is None else size)

    def read1(self, size=-1):
        return self.read(size)

    def peek(self, size=0):
        self._check_read()
        pos = self._buf.tell()
        data = self._buf.read(max(size, 1))
        self._buf.seek(pos)
        return data

    def readinto(self, b):
        data = self.read(len(b))
        b[:len(data)] = data
        return len(data)

    def readline(self, size=-1):
        self._check_read()
        return self._buf.readline(-1 if size is None else size)

    def readlines(self, hint=-1):
        self._check_read()
        return self._buf.readlines(hint)

    def __iter__(self):
        self._check_read()
        return iter(self._buf)

    def tell(self):
        self._check_closed()
        return self._buf.tell()

    def seek(self, offset, whence=0):
        self._check_read()
        return self._buf.seek(offset, whence)

    def write(self, data):
        self._check_closed()
        if self._mode != 'w':
            raise io.UnsupportedOperation('File not open for writing')
        return self._buf.write(bytes(data))

    def flush(self):
        pass

    def close(self):
        if self._closed:
            return
        self._closed = True
        try:
            if self._mode == 'w':
                self._fp.write(self._compress(self._buf.getvalue()))
        finally:
            if self._own:
                self._fp.close()
