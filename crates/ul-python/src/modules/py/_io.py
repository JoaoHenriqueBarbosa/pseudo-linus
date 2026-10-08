"""Módulo `_io` do sandbox: `open`, arquivos binários e de texto sobre descritores, StringIO e BytesIO.

O `io` público é o `io.py` do Debian, que reexporta daqui e monta as ABCs (`io.IOBase`...) sobre as bases `_IOBase`,
`_RawIOBase`, `_BufferedIOBase` e `_TextIOBase`, como no CPython.
"""

import _net
import _os
import _sys

BlockingIOError = BlockingIOError

DEFAULT_BUFFER_SIZE = 8192


class UnsupportedOperation(OSError):
    __module__ = 'io'


def _type_name(obj):
    """O `tp_name` do C: `_io.X` nos tipos embutidos, só o nome nas subclasses definidas em Python."""
    t = type(obj)
    return '_io.' + t.__name__ if t.__module__ == '_io' else t.__name__


class _BytesIOBuffer:
    """O tipo do `BytesIO.getbuffer()`: não se cria de fora, como no C."""

    def __new__(cls, *args, **kwargs):
        raise TypeError("cannot create '_io._BytesIOBuffer' instances")


class _IOBase:
    """Base de todos os objetos de arquivo."""

    _closed = False

    def __del__(self):
        # O `iobase_finalize` do C: fecha o que ainda está aberto (um `closed` ilegível conta como objeto
        # inutilizável e fica como está). O erro do `close()` é engolido: na saída do interpretador as
        # globais já podem ter ido embora, e um traceback aí só assusta.
        try:
            if self.closed:
                return
        except BaseException:
            return
        try:
            self.close()
        except BaseException:
            pass

    @property
    def closed(self):
        return self._closed

    def _check_closed(self):
        if self._closed:
            raise ValueError('I/O operation on closed file.')

    def _checkClosed(self, msg=None):
        if self.closed:
            raise ValueError('I/O operation on closed file.' if msg is None else msg)

    def _checkReadable(self, msg=None):
        if not self.readable():
            raise UnsupportedOperation('File or stream is not readable.' if msg is None else msg)

    def _checkWritable(self, msg=None):
        if not self.writable():
            raise UnsupportedOperation('File or stream is not writable.' if msg is None else msg)

    def _checkSeekable(self, msg=None):
        if not self.seekable():
            raise UnsupportedOperation('File or stream is not seekable.' if msg is None else msg)

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

    def seek(self, pos, whence=0):
        raise UnsupportedOperation('seek')

    def tell(self):
        return self.seek(0, 1)

    def truncate(self, pos=None):
        raise UnsupportedOperation('truncate')

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


class _RawIOBase(_IOBase):
    pass


class _BufferedIOBase(_IOBase):
    def read(self, size=-1):
        raise UnsupportedOperation('read')

    def read1(self, size=-1):
        raise UnsupportedOperation('read1')

    def write(self, b):
        raise UnsupportedOperation('write')

    def detach(self):
        raise UnsupportedOperation('detach')

    def readinto(self, b):
        data = self.read(len(b))
        b[:len(data)] = data
        return len(data)

    def readinto1(self, b):
        data = self.read1(len(b))
        b[:len(data)] = data
        return len(data)


class _TextIOBase(_IOBase):
    encoding = None
    errors = None
    newlines = None


class FileIO(_RawIOBase):
    """Arquivo binário sobre um descritor, com buffer de leitura para `readline`."""

    def __init__(self, file, mode='r', closefd=True, opener=None):
        # Como o `_io_FileIO___init___impl`: `file` é um descritor ou um caminho, e o modo escolhe os flags de `open`.
        if isinstance(file, float):
            raise TypeError('integer argument expected, got float')
        fd = -1
        if isinstance(file, int):
            fd = file
            if fd < 0:
                raise ValueError('negative file descriptor')
            name = file
        else:
            name = file
            if not isinstance(file, (str, bytes)):
                fspath = getattr(type(file), '__fspath__', None)
                if fspath is None:
                    raise TypeError('expected str, bytes or os.PathLike object, not ' + type(file).__name__)
                file = fspath(file)
        plus = False
        self._created = self._append = False
        self._readable = self._writable = False
        flags = 0
        kinds = 0
        for c in mode:
            if c == 'x':
                kinds += 1
                self._created = self._writable = True
                flags |= _os.O_EXCL | _os.O_CREAT
            elif c == 'r':
                kinds += 1
                self._readable = True
            elif c == 'w':
                kinds += 1
                self._writable = True
                flags |= _os.O_CREAT | _os.O_TRUNC
            elif c == 'a':
                kinds += 1
                self._writable = self._append = True
                flags |= _os.O_APPEND | _os.O_CREAT
            elif c == '+':
                if plus:
                    kinds = 2
                plus = True
                self._readable = self._writable = True
            elif c != 'b':
                raise ValueError('invalid mode: ' + repr(mode))
        if kinds != 1:
            raise ValueError('Must have exactly one of create/read/write/append mode and at most one plus')
        flags |= _os.O_RDWR if self._readable and self._writable else _os.O_RDONLY if self._readable else _os.O_WRONLY
        if fd < 0:
            if not closefd:
                raise ValueError('Cannot use closefd=False with file name')
            if opener is None:
                fd = _os.open(file, flags, 0o666)
            else:
                # O opener recebe nome e flags (com `O_CLOEXEC`) e devolve o descritor.
                fd = opener(file, flags | 0o2000000)
                if not isinstance(fd, int):
                    raise TypeError('expected integer from opener')
                if fd < 0:
                    raise ValueError(f'opener returned {fd}')
        self.name = name
        self._fd = fd
        self._closefd = closefd
        self._rbuf = b''
        if self._append:
            # Como o `_io_FileIO___init___impl`: em modo 'a' o deslocamento começa no fim do arquivo.
            try:
                _os.lseek(fd, 0, 2)
            except OSError:
                pass

    @property
    def mode(self):
        # Como o getset do C (`mode_string`): deriva dos flags, não do texto passado.
        if self._created:
            return 'xb+' if self._readable else 'xb'
        if self._append:
            return 'ab+' if self._readable else 'ab'
        if self._readable:
            return 'rb+' if self._writable else 'rb'
        return 'wb'

    @property
    def closefd(self):
        return self._closefd

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
            raise UnsupportedOperation('File not open for reading')
        if size is None or size < 0:
            data = self._rbuf + _net.read(self._fd, -1)
            self._rbuf = b''
            return data
        if len(self._rbuf) >= size:
            data = self._rbuf[:size]
            self._rbuf = self._rbuf[size:]
            return data
        data = self._rbuf + _net.read(self._fd, size - len(self._rbuf))
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
            raise UnsupportedOperation('File not open for reading')
        while True:
            i = self._rbuf.find(b'\n')
            if i >= 0:
                end = i + 1
                break
            chunk = _net.read(self._fd, DEFAULT_BUFFER_SIZE)
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
            raise UnsupportedOperation('File not open for writing')
        if isinstance(data, str):
            raise TypeError("a bytes-like object is required, not 'str'")
        self._drop_buffer()
        return _net.write(self._fd, bytes(data))

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
        return '<%s name=%r mode=%r closefd=%r>' % (_type_name(self), self.name, self.mode, self._closefd)


class _BufferedBase(_BufferedIOBase):
    """Camada de buffer sobre um objeto bruto (`read`/`readinto`, `write`, `seek`...).

    Como no C, o funcionamento interno usa os campos privados (`_raw`, `_buffer_size`): uma subclasse
    que redefina a propriedade `raw` não o afeta, e `buffer_size` não existe como atributo público."""

    __module__ = '_io'

    def __init__(self, raw, buffer_size=DEFAULT_BUFFER_SIZE):
        self._check_raw(raw)
        if buffer_size <= 0:
            raise ValueError('buffer size must be strictly positive')
        self._raw = raw
        self._buffer_size = buffer_size
        self._rbuf = b''
        self._wbuf = bytearray()

    def _check_raw(self, raw):
        pass

    @property
    def raw(self):
        return self._raw

    @property
    def closed(self):
        if self._raw is None:
            raise ValueError('raw stream has been detached')
        return self._closed or self._raw.closed

    def _check_closed(self):
        if self.closed:
            raise ValueError('I/O operation on closed file.')

    @property
    def name(self):
        return self._raw.name

    @property
    def mode(self):
        return self._raw.mode

    def fileno(self):
        return self._raw.fileno()

    def isatty(self):
        return self._raw.isatty()

    def seekable(self):
        return self._raw.seekable()

    def readable(self):
        return self._raw.readable()

    def writable(self):
        return self._raw.writable()

    def detach(self):
        self.flush()
        raw, self._raw = self._raw, None
        return raw

    def close(self):
        if not self.closed:
            try:
                self.flush()
            finally:
                self._closed = True
                self._raw.close()

    def flush(self):
        self._check_closed()
        if self._wbuf:
            data = bytes(self._wbuf)
            self._wbuf = bytearray()
            while data:
                n = self._raw.write(data)
                data = data[n:] if n is not None else b''
        if hasattr(self._raw, 'flush'):
            self._raw.flush()

    def _drop_rbuf(self):
        if self._rbuf:
            self._raw.seek(-len(self._rbuf), 1)
            self._rbuf = b''

    def tell(self):
        return self._raw.tell() - len(self._rbuf) + len(self._wbuf)

    def seek(self, offset, whence=0):
        self._check_closed()
        if self._wbuf:
            self.flush()
        if whence == 1:
            offset -= len(self._rbuf)
        self._rbuf = b''
        return self._raw.seek(offset, whence)

    def __repr__(self):
        try:
            name = self.name
        except AttributeError:
            return '<%s>' % _type_name(self)
        return '<%s name=%r>' % (_type_name(self), name)


class _BufferedReading:
    """Lado de leitura dos buffers (`BufferedReader` e `BufferedRandom`)."""

    def _raw_read(self, n):
        raw = self._raw
        if hasattr(raw, 'read'):
            data = raw.read(n)
            return b'' if data is None else bytes(data)
        buf = bytearray(n)
        got = raw.readinto(buf)
        return bytes(buf[:got or 0])

    def read(self, size=-1):
        self._check_closed()
        if self._wbuf:
            self.flush()
        if size is None or size < 0:
            data = self._rbuf
            self._rbuf = b''
            if hasattr(self._raw, 'readall'):
                rest = self._raw.readall()
                return data + (b'' if rest is None else bytes(rest))
            chunks = [data]
            while True:
                chunk = self._raw_read(self._buffer_size)
                if not chunk:
                    break
                chunks.append(chunk)
            return b''.join(chunks)
        while len(self._rbuf) < size:
            data = self._raw_read(max(size - len(self._rbuf), self._buffer_size))
            if not data:
                break
            self._rbuf += data
        out, self._rbuf = self._rbuf[:size], self._rbuf[size:]
        return out

    def read1(self, size=-1):
        self._check_closed()
        if size is None or size < 0:
            size = self._buffer_size
        if not self._rbuf:
            self._rbuf = self._raw_read(max(size, 1))
        out, self._rbuf = self._rbuf[:size], self._rbuf[size:]
        return out

    def peek(self, size=0):
        self._check_closed()
        if not self._rbuf:
            self._rbuf = self._raw_read(self._buffer_size)
        return self._rbuf

    def readline(self, size=-1):
        self._check_closed()
        if size is None:
            size = -1
        out = b''
        while size < 0 or len(out) < size:
            if not self._rbuf:
                self._rbuf = self._raw_read(self._buffer_size)
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


class _BufferedWriting:
    """Lado de escrita dos buffers (`BufferedWriter` e `BufferedRandom`): o que não chegou ao
    objeto bruto é descarregado quando o buffer fecha (explicitamente ou no `__del__` do `_IOBase`)."""

    def write(self, b):
        self._check_closed()
        self._drop_rbuf()
        data = bytes(b)
        self._wbuf += data
        if len(self._wbuf) >= self._buffer_size:
            self.flush()
        return len(data)

    def truncate(self, pos=None):
        self.flush()
        self._drop_rbuf()
        if pos is None:
            pos = self.tell()
        return self._raw.truncate(pos)


def _raw_check(raw, readable, writable, seekable=False):
    if seekable and not raw.seekable():
        raise UnsupportedOperation('File or stream is not seekable.')
    if readable and not raw.readable():
        raise UnsupportedOperation('File or stream is not readable.')
    if writable and not raw.writable():
        raise UnsupportedOperation('File or stream is not writable.')


class BufferedReader(_BufferedReading, _BufferedBase):
    __module__ = '_io'

    def _check_raw(self, raw):
        _raw_check(raw, True, False)


class BufferedWriter(_BufferedWriting, _BufferedBase):
    __module__ = '_io'

    def _check_raw(self, raw):
        _raw_check(raw, False, True)

    def readline(self, size=-1):
        raise UnsupportedOperation('read')


class BufferedRandom(_BufferedWriting, _BufferedReading, _BufferedBase):
    __module__ = '_io'

    def _check_raw(self, raw):
        _raw_check(raw, True, True, True)


class BufferedRWPair(_BufferedIOBase):
    __module__ = '_io'

    def __init__(self, reader, writer, buffer_size=DEFAULT_BUFFER_SIZE):
        self._reader = BufferedReader(reader, buffer_size)
        self._writer = BufferedWriter(writer, buffer_size)

    def read1(self, size=-1):
        return self._reader.read1(size)

    def read(self, size=-1):
        return self._reader.read(size)

    def peek(self, size=0):
        return self._reader.peek(size)

    def readline(self, size=-1):
        return self._reader.readline(size)

    def write(self, b):
        return self._writer.write(b)

    def flush(self):
        self._writer.flush()

    def readable(self):
        return True

    def writable(self):
        return True

    def close(self):
        try:
            self._writer.close()
        finally:
            self._reader.close()

    @property
    def closed(self):
        return self._writer.closed


def _translate_newlines(text):
    if '\r' in text:
        text = text.replace('\r\n', '\n').replace('\r', '\n')
    return text


class TextIOWrapper(_TextIOBase):
    """Texto sobre um arquivo binário: decodificação, quebras de linha universais e `readline`."""

    __module__ = '_io'

    def __init__(self, buffer, encoding=None, errors=None, newline=None, line_buffering=False, write_through=False):
        if newline is not None and newline not in ('', '\n', '\r', '\r\n'):
            raise ValueError('illegal newline value: ' + repr(newline))
        # Como o C: buffer, encoding, errors, line_buffering, write_through e name são getsets
        # somente leitura, então o __init__ guarda em campos privados (uma subclasse pode
        # redefinir a propriedade sem quebrar a atribuição). `mode` é atributo de instância
        # que só o open() grava.
        self._buffer = buffer
        self._encoding = encoding or 'utf-8'
        self._errors = errors or 'strict'
        self._wenc = None
        self._newline = newline
        self._tbuf = ''
        self._tpos = 0
        self._loaded = False
        self._line_buffering = bool(line_buffering)
        self._write_through = bool(write_through)

    @property
    def buffer(self):
        return self._buffer

    @property
    def encoding(self):
        return self._encoding

    @property
    def errors(self):
        return self._errors

    @property
    def line_buffering(self):
        return self._line_buffering

    @property
    def write_through(self):
        return self._write_through

    @property
    def name(self):
        return self._buffer.name

    @property
    def closed(self):
        return self._buffer.closed

    @property
    def newlines(self):
        return None

    def readable(self):
        return self._buffer.readable()

    def writable(self):
        return self._buffer.writable()

    def seekable(self):
        return self._buffer.seekable()

    def fileno(self):
        return self._buffer.fileno()

    def isatty(self):
        return self._buffer.isatty()

    def _load(self):
        if not self._loaded:
            data = self._buffer.read()
            text = data.decode(self._encoding, self._errors)
            if self._newline is None:
                text = _translate_newlines(text)
            self._tbuf = text
            self._tpos = 0
            self._loaded = True

    def read(self, size=-1):
        self._check_closed()
        if not self._buffer.readable():
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
        if not self._buffer.readable():
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
        if not self._buffer.writable():
            raise UnsupportedOperation('not writable')
        if self._loaded:
            self._unload()
        if self._newline in (None, '\n'):
            pass
        elif self._newline == '\r' or self._newline == '\r\n':
            text = text.replace('\n', self._newline)
        self._buffer.write(text.encode(self._write_encoding(), self._errors))
        if self._line_buffering and ('\n' in text or '\r' in text):
            self._buffer.flush()
        return len(text)

    def _write_encoding(self):
        # Os codecs com BOM (utf-8-sig, utf-16, utf-32) escrevem o BOM só no começo do arquivo.
        name = self._wenc
        if name is None:
            name = self._encoding.lower().replace('_', '-')
            bare = {'utf-8-sig': 'utf-8', 'utf8-sig': 'utf-8', 'utf-16': 'utf-16-le', 'utf16': 'utf-16-le',
                    'u16': 'utf-16-le', 'utf-32': 'utf-32-le', 'utf32': 'utf-32-le', 'u32': 'utf-32-le'}.get(name)
            if bare is None:
                self._wenc = self._encoding
                return self._encoding
            try:
                fresh = self._buffer.seekable() and self._buffer.tell() == 0
            except (OSError, ValueError):
                fresh = True
            if not fresh:
                self._wenc = bare
                return bare
            self._wenc = bare
            return self._encoding
        return name

    def _unload(self):
        # Descarta o texto lido e devolve o buffer binário à posição lógica do texto consumido.
        pos = self.tell()
        self._loaded = False
        self._tbuf = ''
        self._tpos = 0
        self._buffer.seek(pos)

    def flush(self):
        self._check_closed()
        self._buffer.flush()

    def seek(self, pos, whence=0):
        self._check_closed()
        self._loaded = False
        self._tbuf = ''
        self._tpos = 0
        return self._buffer.seek(pos, whence)

    def tell(self):
        self._check_closed()
        if self._loaded:
            return self._buffer.tell() - len(self._tbuf[self._tpos:].encode(self._encoding, self._errors))
        return self._buffer.tell()

    def truncate(self, size=None):
        self._check_closed()
        return self._buffer.truncate(size)

    def detach(self):
        buf = self._buffer
        self._buffer = None
        return buf

    def close(self):
        if not self._closed:
            try:
                self._buffer.close()
            finally:
                self._closed = True

    def __repr__(self):
        out = '<' + _type_name(self)
        try:
            out += ' name=%r' % (self.name,)
        except (AttributeError, ValueError):
            pass
        try:
            out += ' mode=%r' % (self.mode,)
        except AttributeError:
            pass
        return out + ' encoding=%r>' % (self._encoding,)


class BytesIO(_BufferedIOBase):
    __module__ = '_io'

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
        if not isinstance(b, (bytes, bytearray, memoryview)) and not hasattr(b, 'tobytes'):
            raise TypeError("a bytes-like object is required, not '%s'" % type(b).__name__)
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


class IncrementalNewlineDecoder:
    """Decodificador que traduz `\\r\\n` e `\\r` em `\\n` (opcional) e lembra quais fins de linha viu."""

    __module__ = '_io'

    def __init__(self, decoder, translate, errors='strict'):
        self.decoder = decoder
        self.translate = translate
        self.errors = errors
        self.seennl = 0
        self.pendingcr = False

    def decode(self, input, final=False):
        if self.decoder is not None:
            output = self.decoder.decode(input, final=final)
        else:
            output = input
        if self.pendingcr and (output or final):
            output = '\r' + output
            self.pendingcr = False
        if output.endswith('\r') and not final:
            output = output[:-1]
            self.pendingcr = True
        crlf = output.count('\r\n')
        cr = output.count('\r') - crlf
        lf = output.count('\n') - crlf
        self.seennl |= (1 if lf else 0) | (2 if cr else 0) | (4 if crlf else 0)
        if self.translate:
            if crlf:
                output = output.replace('\r\n', '\n')
            if cr:
                output = output.replace('\r', '\n')
        return output

    def getstate(self):
        buf, flag = (b'', 0) if self.decoder is None else self.decoder.getstate()
        return buf, (flag << 1) | int(self.pendingcr)

    def setstate(self, state):
        buf, flag = state
        self.pendingcr = bool(flag & 1)
        if self.decoder is not None:
            self.decoder.setstate((buf, flag >> 1))

    def reset(self):
        self.seennl = 0
        self.pendingcr = False
        if self.decoder is not None:
            self.decoder.reset()

    @property
    def newlines(self):
        return (None, '\n', '\r', ('\r', '\n'), '\r\n', ('\n', '\r\n'), ('\r', '\r\n'),
                ('\r', '\n', '\r\n'))[self.seennl]


class StringIO(_TextIOBase):
    __module__ = '_io'

    def __init__(self, initial_value='', newline='\n'):
        self._data = initial_value or ''
        self._pos = 0
        self._newline = newline
        if newline is None:
            self._data = _translate_newlines(self._data)

    @property
    def line_buffering(self):
        return False

    def _check_closed(self):
        # `CHECK_CLOSED` do stringio.c: a mensagem não leva ponto final, ao contrário do iobase.c.
        if self._closed:
            raise ValueError('I/O operation on closed file')

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
    if not isinstance(file, (int, str, bytes)):
        if hasattr(file, '__fspath__'):
            file = file.__fspath__()
        if not isinstance(file, (str, bytes)):
            raise TypeError('expected str, bytes or os.PathLike object, not ' + type(file).__name__)
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
    raw_mode = ('r' if reading else 'w' if writing else 'a' if appending else 'x') + ('+' if updating else '')
    raw = FileIO(file, raw_mode + ('b' if binary else ''), closefd, opener)
    try:
        # Como o `_io_open_impl`: tty ou buffering=1 liga o line buffering; o tamanho padrão do buffer
        # é o st_blksize do descritor (DEFAULT_BUFFER_SIZE quando ele não passa de 1).
        if binary and buffering == 1:
            import warnings
            warnings.warn("line buffering (buffering=1) isn't supported in binary mode, "
                          "the default buffer size will be used", RuntimeWarning, 2)
        isatty = buffering < 0 and raw.isatty()
        line_buffering = buffering == 1 or isatty
        if line_buffering:
            buffering = -1
        if buffering < 0:
            import os
            buffering = os.fstat(raw.fileno()).st_blksize
            if buffering <= 1:
                buffering = DEFAULT_BUFFER_SIZE
        if buffering == 0:
            if binary:
                return raw
            raise ValueError("can't have unbuffered text I/O")
        if updating:
            buffer = BufferedRandom(raw, buffering)
        elif creating or writing or appending:
            buffer = BufferedWriter(raw, buffering)
        else:
            buffer = BufferedReader(raw, buffering)
        if binary:
            return buffer
        text = TextIOWrapper(buffer, encoding, errors, newline, line_buffering)
        text.mode = mode
        return text
    except BaseException:
        raw.close()
        raise


_sys._builtin(open)


def open_code(path):
    return open(path, 'rb')


def text_encoding(encoding, stacklevel=2):
    return encoding or 'utf-8'
