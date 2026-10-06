"""Módulo io do sandbox: `open`, arquivos binários e de texto sobre descritores, StringIO e BytesIO."""

import _os

SEEK_SET = 0
SEEK_CUR = 1
SEEK_END = 2
DEFAULT_BUFFER_SIZE = 8192


class UnsupportedOperation(OSError):
    pass


class IOBase:
    """Base de todos os objetos de arquivo."""

    _closed = False

    @property
    def closed(self):
        return self._closed

    def _check_closed(self):
        if self._closed:
            raise ValueError('I/O operation on closed file.')

    def close(self):
        if not self._closed:
            try:
                self.flush()
            finally:
                self._closed = True

    def flush(self):
        self._check_closed()

    def readable(self):
        return False

    def writable(self):
        return False

    def seekable(self):
        return False

    def isatty(self):
        return False

    def fileno(self):
        raise UnsupportedOperation('fileno')

    def __enter__(self):
        self._check_closed()
        return self

    def __exit__(self, *exc):
        self.close()
        return False

    def __iter__(self):
        self._check_closed()
        return self

    def __next__(self):
        line = self.readline()
        if not line:
            raise StopIteration
        return line

    def readlines(self, hint=-1):
        lines = []
        total = 0
        for line in self:
            lines.append(line)
            total += len(line)
            if 0 < hint <= total:
                break
        return lines

    def writelines(self, lines):
        self._check_closed()
        for line in lines:
            self.write(line)


class RawIOBase(IOBase):
    pass


class BufferedIOBase(IOBase):
    pass


class TextIOBase(IOBase):
    encoding = None
    errors = None
    newlines = None


class FileIO(RawIOBase):
    """Arquivo binário sobre um descritor, com buffer de leitura para `readline`."""

    def __init__(self, name, mode, fd, closefd=True):
        self.name = name
        self.mode = mode
        self._fd = fd
        self._closefd = closefd
        self._rbuf = b''
        self._readable = 'r' in mode or '+' in mode
        self._writable = 'w' in mode or 'a' in mode or 'x' in mode or '+' in mode
        self._append = 'a' in mode

    def readable(self):
        return self._readable

    def writable(self):
        return self._writable

    def seekable(self):
        return True

    def fileno(self):
        self._check_closed()
        return self._fd

    def isatty(self):
        return _os.isatty(self._fd)

    def _drop_buffer(self):
        if self._rbuf:
            _os.lseek(self._fd, -len(self._rbuf), 1)
            self._rbuf = b''

    def read(self, size=-1):
        self._check_closed()
        if not self._readable:
            raise UnsupportedOperation('not readable')
        if size is None or size < 0:
            data = self._rbuf + _os.read(self._fd, -1)
            self._rbuf = b''
            return data
        if len(self._rbuf) >= size:
            data = self._rbuf[:size]
            self._rbuf = self._rbuf[size:]
            return data
        data = self._rbuf + _os.read(self._fd, size - len(self._rbuf))
        self._rbuf = b''
        return data

    readall = read

    def readinto(self, b):
        data = self.read(len(b))
        b[:len(data)] = data
        return len(data)

    def readline(self, size=-1):
        self._check_closed()
        if not self._readable:
            raise UnsupportedOperation('not readable')
        while True:
            i = self._rbuf.find(b'\n')
            if i >= 0:
                end = i + 1
                break
            chunk = _os.read(self._fd, DEFAULT_BUFFER_SIZE)
            if not chunk:
                end = len(self._rbuf)
                break
            self._rbuf += chunk
        if size is not None and 0 <= size < end:
            end = size
        line = self._rbuf[:end]
        self._rbuf = self._rbuf[end:]
        return line

    def write(self, data):
        self._check_closed()
        if not self._writable:
            raise UnsupportedOperation('not writable')
        if isinstance(data, str):
            raise TypeError("a bytes-like object is required, not 'str'")
        self._drop_buffer()
        return _os.write(self._fd, bytes(data))

    def seek(self, pos, whence=0):
        self._check_closed()
        if whence == 1:
            pos -= len(self._rbuf)
        self._rbuf = b''
        return _os.lseek(self._fd, pos, whence)

    def tell(self):
        self._check_closed()
        return _os.lseek(self._fd, 0, 1) - len(self._rbuf)

    def truncate(self, size=None):
        self._check_closed()
        if size is None:
            size = self.tell()
        _os.ftruncate(self._fd, size)
        return size

    def close(self):
        if not self._closed:
            self._closed = True
            if self._closefd:
                _os.close(self._fd)

    def __repr__(self):
        return "<_io.FileIO name=%r mode=%r closefd=%r>" % (self.name, self.mode, self._closefd)


class _BufferedBase(BufferedIOBase):
    """Camada de buffer sobre um objeto bruto (`read`/`readinto`, `write`, `seek`...)."""

    def __init__(self, raw, buffer_size=DEFAULT_BUFFER_SIZE):
        if buffer_size <= 0:
            raise ValueError('buffer size must be strictly positive')
        self.raw = raw
        self.buffer_size = buffer_size
        self._rbuf = b''
        self._wbuf = bytearray()

    @property
    def closed(self):
        return self._closed or self.raw.closed

    @property
    def name(self):
        return self.raw.name

    @property
    def mode(self):
        return self.raw.mode

    def fileno(self):
        return self.raw.fileno()

    def isatty(self):
        return self.raw.isatty()

    def seekable(self):
        return self.raw.seekable()

    def readable(self):
        return self.raw.readable()

    def writable(self):
        return self.raw.writable()

    def detach(self):
        self.flush()
        raw, self.raw = self.raw, None
        return raw

    def close(self):
        if not self._closed:
            try:
                self.flush()
            finally:
                self._closed = True
                self.raw.close()

    def _raw_read(self, n):
        raw = self.raw
        if hasattr(raw, 'read'):
            data = raw.read(n)
            return b'' if data is None else bytes(data)
        buf = bytearray(n)
        got = raw.readinto(buf)
        return bytes(buf[:got or 0])

    def _drop_rbuf(self):
        if self._rbuf:
            self.raw.seek(-len(self._rbuf), 1)
            self._rbuf = b''

    def flush(self):
        self._check_closed()
        if self._wbuf:
            data = bytes(self._wbuf)
            self._wbuf = bytearray()
            while data:
                n = self.raw.write(data)
                data = data[n:] if n is not None else b''
        if hasattr(self.raw, 'flush'):
            self.raw.flush()

    def tell(self):
        return self.raw.tell() - len(self._rbuf) + len(self._wbuf)

    def seek(self, offset, whence=0):
        self._check_closed()
        if self._wbuf:
            self.flush()
        if whence == 1:
            offset -= len(self._rbuf)
        self._rbuf = b''
        return self.raw.seek(offset, whence)

    def read(self, size=-1):
        self._check_closed()
        if self._wbuf:
            self.flush()
        if size is None or size < 0:
            chunks = [self._rbuf]
            self._rbuf = b''
            while True:
                data = self._raw_read(self.buffer_size)
                if not data:
                    break
                chunks.append(data)
            return b''.join(chunks)
        while len(self._rbuf) < size:
            data = self._raw_read(max(size - len(self._rbuf), self.buffer_size))
            if not data:
                break
            self._rbuf += data
        out, self._rbuf = self._rbuf[:size], self._rbuf[size:]
        return out

    def read1(self, size=-1):
        self._check_closed()
        if size is None or size < 0:
            size = self.buffer_size
        if not self._rbuf:
            self._rbuf = self._raw_read(max(size, 1))
        out, self._rbuf = self._rbuf[:size], self._rbuf[size:]
        return out

    def peek(self, size=0):
        self._check_closed()
        if not self._rbuf:
            self._rbuf = self._raw_read(self.buffer_size)
        return self._rbuf

    def readinto(self, b):
        data = self.read(len(b))
        b[:len(data)] = data
        return len(data)

    def readinto1(self, b):
        data = self.read1(len(b))
        b[:len(data)] = data
        return len(data)

    def readline(self, size=-1):
        self._check_closed()
        out = b''
        while size < 0 or len(out) < size:
            if not self._rbuf:
                self._rbuf = self._raw_read(self.buffer_size)
                if not self._rbuf:
                    break
            i = self._rbuf.find(b'\n')
            take = len(self._rbuf) if i < 0 else i + 1
            if size >= 0:
                take = min(take, size - len(out))
            out += self._rbuf[:take]
            self._rbuf = self._rbuf[take:]
            if i >= 0 and take == i + 1:
                break
        return out

    def write(self, b):
        self._check_closed()
        self._drop_rbuf()
        data = bytes(b)
        self._wbuf += data
        if len(self._wbuf) >= self.buffer_size:
            self.flush()
        return len(data)

    def truncate(self, pos=None):
        self.flush()
        self._drop_rbuf()
        if pos is None:
            pos = self.tell()
        return self.raw.truncate(pos)

    def __repr__(self):
        name = getattr(self.raw, 'name', None)
        if name is None:
            return '<_io.%s>' % type(self).__name__
        return '<_io.%s name=%r>' % (type(self).__name__, name)


class BufferedReader(_BufferedBase):
    pass


class BufferedWriter(_BufferedBase):
    pass


class BufferedRandom(_BufferedBase):
    pass


class BufferedRWPair(BufferedIOBase):
    def __init__(self, reader, writer, buffer_size=DEFAULT_BUFFER_SIZE):
        self.reader = BufferedReader(reader, buffer_size)
        self.writer = BufferedWriter(writer, buffer_size)

    def read(self, size=-1):
        return self.reader.read(size)

    def peek(self, size=0):
        return self.reader.peek(size)

    def readline(self, size=-1):
        return self.reader.readline(size)

    def write(self, b):
        return self.writer.write(b)

    def flush(self):
        self.writer.flush()

    def readable(self):
        return True

    def writable(self):
        return True

    def close(self):
        try:
            self.writer.close()
        finally:
            self.reader.close()

    @property
    def closed(self):
        return self.writer.closed


def _translate_newlines(text):
    if '\r' in text:
        text = text.replace('\r\n', '\n').replace('\r', '\n')
    return text


class TextIOWrapper(TextIOBase):
    """Texto sobre um arquivo binário: decodificação, quebras de linha universais e `readline`."""

    def __init__(self, buffer, encoding=None, errors=None, newline=None, line_buffering=False, write_through=False):
        if newline is not None and newline not in ('', '\n', '\r', '\r\n'):
            raise ValueError('illegal newline value: ' + repr(newline))
        self.buffer = buffer
        self.encoding = encoding or 'utf-8'
        self.errors = errors or 'strict'
        self._wenc = None
        self._newline = newline
        self._tbuf = ''
        self._tpos = 0
        self._loaded = False
        self.line_buffering = line_buffering
        self.name = getattr(buffer, 'name', None)
        self.mode = getattr(buffer, 'mode', 'r')

    @property
    def newlines(self):
        return None

    def readable(self):
        return self.buffer.readable()

    def writable(self):
        return self.buffer.writable()

    def seekable(self):
        return self.buffer.seekable()

    def fileno(self):
        return self.buffer.fileno()

    def isatty(self):
        return self.buffer.isatty()

    def _load(self):
        if not self._loaded:
            data = self.buffer.read()
            text = data.decode(self.encoding, self.errors)
            if self._newline is None:
                text = _translate_newlines(text)
            self._tbuf = text
            self._tpos = 0
            self._loaded = True

    def read(self, size=-1):
        self._check_closed()
        if not self.buffer.readable():
            raise UnsupportedOperation('not readable')
        self._load()
        if size is None or size < 0:
            text = self._tbuf[self._tpos:]
            self._tpos = len(self._tbuf)
        else:
            text = self._tbuf[self._tpos:self._tpos + size]
            self._tpos += len(text)
        return text

    def readline(self, size=-1):
        self._check_closed()
        if not self.buffer.readable():
            raise UnsupportedOperation('not readable')
        self._load()
        text = self._tbuf
        start = self._tpos
        if self._newline in (None, '', '\n'):
            i = text.find('\n', start)
            end = i + 1 if i >= 0 else len(text)
        elif self._newline == '\r':
            i = text.find('\r', start)
            end = i + 1 if i >= 0 else len(text)
        else:
            i = text.find('\r\n', start)
            end = i + 2 if i >= 0 else len(text)
        if size is not None and size >= 0 and start + size < end:
            end = start + size
        self._tpos = end
        return text[start:end]

    def write(self, text):
        self._check_closed()
        if not isinstance(text, str):
            raise TypeError('write() argument must be str, not ' + type(text).__name__)
        if self._loaded:
            self._unload()
        if self._newline in (None, '\n'):
            pass
        elif self._newline == '\r' or self._newline == '\r\n':
            text = text.replace('\n', self._newline)
        self.buffer.write(text.encode(self._write_encoding(), self.errors))
        return len(text)

    def _write_encoding(self):
        # Os codecs com BOM (utf-8-sig, utf-16, utf-32) escrevem o BOM só no começo do arquivo.
        name = self._wenc
        if name is None:
            name = self.encoding.lower().replace('_', '-')
            bare = {'utf-8-sig': 'utf-8', 'utf8-sig': 'utf-8', 'utf-16': 'utf-16-le', 'utf16': 'utf-16-le',
                    'u16': 'utf-16-le', 'utf-32': 'utf-32-le', 'utf32': 'utf-32-le', 'u32': 'utf-32-le'}.get(name)
            if bare is None:
                self._wenc = self.encoding
                return self.encoding
            try:
                fresh = self.buffer.seekable() and self.buffer.tell() == 0
            except (OSError, ValueError):
                fresh = True
            if not fresh:
                self._wenc = bare
                return bare
            self._wenc = bare
            return self.encoding
        return name

    def _unload(self):
        # Descarta o texto lido e devolve o buffer binário à posição lógica do texto consumido.
        pos = self.tell()
        self._loaded = False
        self._tbuf = ''
        self._tpos = 0
        self.buffer.seek(pos)

    def flush(self):
        self._check_closed()
        self.buffer.flush()

    def seek(self, pos, whence=0):
        self._check_closed()
        self._loaded = False
        self._tbuf = ''
        self._tpos = 0
        return self.buffer.seek(pos, whence)

    def tell(self):
        self._check_closed()
        if self._loaded:
            return self.buffer.tell() - len(self._tbuf[self._tpos:].encode(self.encoding, self.errors))
        return self.buffer.tell()

    def truncate(self, size=None):
        self._check_closed()
        return self.buffer.truncate(size)

    def detach(self):
        buf = self.buffer
        self.buffer = None
        return buf

    def close(self):
        if not self._closed:
            try:
                self.buffer.close()
            finally:
                self._closed = True

    def __repr__(self):
        return "<_io.TextIOWrapper name=%r mode=%r encoding=%r>" % (self.name, self.mode, self.encoding)


class BytesIO(BufferedIOBase):
    def __init__(self, initial_bytes=b''):
        self._data = bytes(initial_bytes)
        self._pos = 0

    def readable(self):
        return True

    def writable(self):
        return True

    def seekable(self):
        return True

    def getvalue(self):
        self._check_closed()
        return self._data

    def getbuffer(self):
        return memoryview(self._data)

    def read(self, size=-1):
        self._check_closed()
        if size is None or size < 0:
            end = len(self._data)
        else:
            end = min(self._pos + size, len(self._data))
        data = self._data[self._pos:end]
        self._pos = max(self._pos, end)
        return data

    read1 = read

    def readinto(self, b):
        data = self.read(len(b))
        b[:len(data)] = data
        return len(data)

    readinto1 = readinto

    def readline(self, size=-1):
        self._check_closed()
        i = self._data.find(b'\n', self._pos)
        end = i + 1 if i >= 0 else len(self._data)
        if size is not None and size >= 0 and self._pos + size < end:
            end = self._pos + size
        data = self._data[self._pos:end]
        self._pos = end
        return data

    def write(self, b):
        self._check_closed()
        b = bytes(b)
        if self._pos > len(self._data):
            self._data += b'\x00' * (self._pos - len(self._data))
        self._data = self._data[:self._pos] + b + self._data[self._pos + len(b):]
        self._pos += len(b)
        return len(b)

    def seek(self, pos, whence=0):
        self._check_closed()
        if whence == 0:
            if pos < 0:
                raise ValueError('negative seek value %d' % pos)
            self._pos = pos
        elif whence == 1:
            self._pos = max(0, self._pos + pos)
        else:
            self._pos = max(0, len(self._data) + pos)
        return self._pos

    def tell(self):
        self._check_closed()
        return self._pos

    def truncate(self, size=None):
        self._check_closed()
        if size is None:
            size = self._pos
        self._data = self._data[:size]
        return size


class StringIO(TextIOBase):
    def __init__(self, initial_value='', newline='\n'):
        self._data = initial_value or ''
        self._pos = 0
        self._newline = newline
        if newline is None:
            self._data = _translate_newlines(self._data)

    def readable(self):
        return True

    def writable(self):
        return True

    def seekable(self):
        return True

    def getvalue(self):
        self._check_closed()
        return self._data

    def read(self, size=-1):
        self._check_closed()
        if size is None or size < 0:
            end = len(self._data)
        else:
            end = min(self._pos + size, len(self._data))
        text = self._data[self._pos:end]
        self._pos = max(self._pos, end)
        return text

    def readline(self, size=-1):
        self._check_closed()
        i = self._data.find('\n', self._pos)
        end = i + 1 if i >= 0 else len(self._data)
        if size is not None and size >= 0 and self._pos + size < end:
            end = self._pos + size
        text = self._data[self._pos:end]
        self._pos = end
        return text

    def write(self, s):
        self._check_closed()
        if not isinstance(s, str):
            raise TypeError('string argument expected, got ' + repr(type(s).__name__))
        if self._newline is None or self._newline == '':
            pass
        elif self._newline != '\n':
            s = s.replace('\n', self._newline)
        if self._pos > len(self._data):
            self._data += '\x00' * (self._pos - len(self._data))
        self._data = self._data[:self._pos] + s + self._data[self._pos + len(s):]
        self._pos += len(s)
        return len(s)

    def seek(self, pos, whence=0):
        self._check_closed()
        if whence == 0:
            if pos < 0:
                raise ValueError('Negative seek position %d' % pos)
            self._pos = pos
        elif whence == 1:
            if pos != 0:
                raise OSError('Can\'t do nonzero cur-relative seeks')
        else:
            if pos != 0:
                raise OSError('Can\'t do nonzero end-relative seeks')
            self._pos = len(self._data)
        return self._pos

    def tell(self):
        self._check_closed()
        return self._pos

    def truncate(self, size=None):
        self._check_closed()
        if size is None:
            size = self._pos
        self._data = self._data[:size]
        return size


def open(file, mode='r', buffering=-1, encoding=None, errors=None, newline=None, closefd=True, opener=None):
    if isinstance(file, int):
        fd = file
        name = file
    else:
        if hasattr(file, '__fspath__'):
            file = file.__fspath__()
        if not isinstance(file, (str, bytes)):
            raise TypeError('expected str, bytes or os.PathLike object, not ' + type(file).__name__)
        name = file
        fd = None
    modes = set(mode)
    if modes - set('axrwb+tU') or len(mode) > len(modes):
        raise ValueError('invalid mode: ' + repr(mode))
    creating = 'x' in modes
    reading = 'r' in modes
    writing = 'w' in modes
    appending = 'a' in modes
    updating = '+' in modes
    text = 't' in modes
    binary = 'b' in modes
    if text and binary:
        raise ValueError("can't have text and binary mode at once")
    if creating + reading + writing + appending > 1:
        raise ValueError('must have exactly one of create/read/write/append mode')
    if not (creating or reading or writing or appending):
        raise ValueError('Must have exactly one of create/read/write/append mode and at most one plus')
    if binary and encoding is not None:
        raise ValueError("binary mode doesn't take an encoding argument")
    if binary and errors is not None:
        raise ValueError("binary mode doesn't take an errors argument")
    if binary and newline is not None:
        raise ValueError("binary mode doesn't take a newline argument")
    flags = 0
    if updating:
        flags = _os.O_RDWR
    elif reading:
        flags = _os.O_RDONLY
    else:
        flags = _os.O_WRONLY
    if creating:
        flags |= _os.O_CREAT | _os.O_EXCL
    elif writing:
        flags |= _os.O_CREAT | _os.O_TRUNC
    elif appending:
        flags |= _os.O_CREAT | _os.O_APPEND
    if fd is None:
        fd = _os.open(name, flags, 0o666)
    raw_mode = ('r' if reading else 'w' if writing else 'a' if appending else 'x') + ('+' if updating else '')
    raw = FileIO(name, raw_mode + ('b' if binary else ''), fd, closefd)
    if binary:
        return raw
    return TextIOWrapper(raw, encoding, errors, newline)


def open_code(path):
    return open(path, 'rb')


def text_encoding(encoding, stacklevel=2):
    return encoding or 'utf-8'
