"""Registro de codecs sobre `str.encode`/`bytes.decode`, mais as transformações de bytes comuns."""

import io as _io
import sys

BOM_UTF8 = b'\xef\xbb\xbf'
BOM_LE = BOM_UTF16_LE = b'\xff\xfe'
BOM_BE = BOM_UTF16_BE = b'\xfe\xff'
BOM_UTF32_LE = b'\xff\xfe\x00\x00'
BOM_UTF32_BE = b'\x00\x00\xfe\xff'
BOM = BOM_UTF16 = BOM_LE if sys.byteorder == 'little' else BOM_BE
BOM32_LE = BOM_UTF32_LE
BOM32_BE = BOM_UTF32_BE
BOM_UTF32 = BOM_UTF32_LE

_CANON = {
    'utf8': 'utf-8', 'utf-8': 'utf-8', 'u8': 'utf-8', 'utf': 'utf-8', 'utf_8': 'utf-8',
    'ascii': 'ascii', 'us-ascii': 'ascii', '646': 'ascii',
    'latin-1': 'iso8859-1', 'latin1': 'iso8859-1', 'latin': 'iso8859-1', 'l1': 'iso8859-1',
    'iso-8859-1': 'iso8859-1', 'iso8859-1': 'iso8859-1', '8859': 'iso8859-1', 'cp819': 'iso8859-1',
    'utf-16': 'utf-16', 'utf16': 'utf-16', 'utf-16-le': 'utf-16-le', 'utf-16le': 'utf-16-le',
    'utf-16-be': 'utf-16-be', 'utf-16be': 'utf-16-be',
    'utf-32': 'utf-32', 'utf32': 'utf-32', 'utf-32-le': 'utf-32-le', 'utf-32le': 'utf-32-le',
    'utf-32-be': 'utf-32-be', 'utf-32be': 'utf-32-be',
    'utf-8-sig': 'utf-8-sig', 'utf8-sig': 'utf-8-sig',
    'cp1252': 'cp1252', 'windows-1252': 'cp1252',
    'rot13': 'rot-13', 'rot-13': 'rot-13', 'rot_13': 'rot-13',
    'hex': 'hex', 'hex_codec': 'hex', 'base64': 'base64', 'base64_codec': 'base64',
    'zlib': 'zlib', 'zip': 'zlib', 'zlib_codec': 'zlib',
    'unicode_escape': 'unicode-escape', 'unicode-escape': 'unicode-escape',
    'raw_unicode_escape': 'raw-unicode-escape',
    'idna': 'idna', 'punycode': 'punycode',
}
_BYTES_TO_BYTES = {'hex', 'base64', 'zlib'}
_ERRORS = {}


class CodecInfo(tuple):

    def __new__(cls, encode, decode, streamreader=None, streamwriter=None,
                incrementalencoder=None, incrementaldecoder=None, name=None, *, _is_text_encoding=None):
        self = tuple.__new__(cls, (encode, decode, streamreader, streamwriter))
        self.name = name
        self.encode = encode
        self.decode = decode
        self.incrementalencoder = incrementalencoder
        self.incrementaldecoder = incrementaldecoder
        self.streamwriter = streamwriter
        self.streamreader = streamreader
        self._is_text_encoding = _is_text_encoding
        return self

    def __repr__(self):
        return '<%s.%s object for encoding %s at %#x>' % (
            self.__class__.__module__, self.__class__.__name__, self.name, id(self))


def _norm(encoding):
    if not isinstance(encoding, str):
        raise TypeError('lookup() argument must be str, not %s' % type(encoding).__name__)
    key = encoding.lower().replace(' ', '-').replace('_', '-')
    found = _CANON.get(key) or _CANON.get(encoding.lower()) or _CANON.get(key.replace('-', '_'))
    if found is None:
        # Os codecs de texto do núcleo (cp125x, iso8859-x, koi8...): se `str.encode` conhece, vale.
        try:
            ''.encode(encoding)
        except LookupError:
            return None
        found = key
    return found


def _hex_enc(data, errors='strict'):
    return bytes(data).hex().encode('ascii'), len(data)


def _hex_dec(data, errors='strict'):
    return bytes.fromhex(bytes(data).decode('ascii')), len(data)


def _b64_enc(data, errors='strict'):
    import base64
    return base64.encodebytes(bytes(data)), len(data)


def _b64_dec(data, errors='strict'):
    import base64
    return base64.decodebytes(bytes(data)), len(data)


def _zlib_enc(data, errors='strict'):
    import zlib
    return zlib.compress(bytes(data)), len(data)


def _zlib_dec(data, errors='strict'):
    import zlib
    return zlib.decompress(bytes(data)), len(data)


_ROT = str.maketrans('ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz',
                     'NOPQRSTUVWXYZABCDEFGHIJKLMnopqrstuvwxyzabcdefghijklm')


def _rot_enc(text, errors='strict'):
    return str.translate(text, _ROT), len(text)


def _make(name):
    if name == 'hex':
        enc, dec = _hex_enc, _hex_dec
    elif name == 'base64':
        enc, dec = _b64_enc, _b64_dec
    elif name == 'zlib':
        enc, dec = _zlib_enc, _zlib_dec
    elif name == 'rot-13':
        enc, dec = _rot_enc, _rot_enc
    else:
        def enc(text, errors='strict', _n=name):
            data = text.encode(_n if _n != 'iso8859-1' else 'latin-1', errors)
            return data, len(text)

        def dec(data, errors='strict', _n=name):
            text = bytes(data).decode(_n if _n != 'iso8859-1' else 'latin-1', errors)
            return text, len(data)
    return CodecInfo(enc, dec, name=name,
                     streamreader=_stream_reader(name), streamwriter=_stream_writer(name),
                     incrementalencoder=_inc_encoder(name), incrementaldecoder=_inc_decoder(name))


_cache = {}


def lookup(encoding):
    name = _norm(encoding)
    if name is None:
        raise LookupError('unknown encoding: %s' % encoding)
    if name not in _cache:
        _cache[name] = _make(name)
    return _cache[name]


def register(search_function):
    pass


def unregister(search_function):
    pass


def getencoder(encoding):
    return lookup(encoding).encode


def getdecoder(encoding):
    return lookup(encoding).decode


def getincrementalencoder(encoding):
    return lookup(encoding).incrementalencoder


def getincrementaldecoder(encoding):
    return lookup(encoding).incrementaldecoder


def getreader(encoding):
    return lookup(encoding).streamreader


def getwriter(encoding):
    return lookup(encoding).streamwriter


def encode(obj, encoding='utf-8', errors='strict'):
    name = _norm(encoding)
    if name is None:
        raise LookupError('unknown encoding: %s' % encoding)
    if name in _BYTES_TO_BYTES or name == 'rot-13':
        return lookup(name).encode(obj, errors)[0]
    return obj.encode(encoding, errors) if isinstance(obj, str) else _bad_encode(obj, encoding)


def _bad_encode(obj, encoding):
    raise TypeError("'%s' object cannot be interpreted as str" % type(obj).__name__)


def decode(obj, encoding='utf-8', errors='strict'):
    name = _norm(encoding)
    if name is None:
        raise LookupError('unknown encoding: %s' % encoding)
    if name in _BYTES_TO_BYTES:
        if isinstance(obj, str):
            obj = obj.encode('ascii')
        return lookup(name).decode(obj, errors)[0]
    if name == 'rot-13':
        return lookup(name).decode(obj, errors)[0]
    return bytes(obj).decode(encoding, errors)


def register_error(name, handler):
    _ERRORS[name] = handler


def lookup_error(name):
    if name in _ERRORS:
        return _ERRORS[name]
    if name in ('strict', 'ignore', 'replace', 'backslashreplace', 'xmlcharrefreplace', 'namereplace', 'surrogateescape', 'surrogatepass'):
        def handler(exc, _n=name):
            raise exc
        handler.__name__ = name + '_errors'
        return handler
    raise LookupError('unknown error handler name %r' % name)


def strict_errors(exc):
    raise exc


def iterencode(iterator, encoding, errors='strict', **kwargs):
    for chunk in iterator:
        out = encode(chunk, encoding, errors)
        if out:
            yield out


def iterdecode(iterator, encoding, errors='strict', **kwargs):
    d = getincrementaldecoder(encoding)(errors)
    for chunk in iterator:
        out = d.decode(chunk)
        if out:
            yield out
    out = d.decode(b'', True)
    if out:
        yield out


class IncrementalEncoder:
    def __init__(self, errors='strict'):
        self.errors = errors
        self.buffer = ''

    def encode(self, input, final=False):
        raise NotImplementedError

    def reset(self):
        pass

    def getstate(self):
        return 0

    def setstate(self, state):
        pass


class BufferedIncrementalEncoder(IncrementalEncoder):
    def __init__(self, errors='strict'):
        IncrementalEncoder.__init__(self, errors)
        self.buffer = ''

    def _buffer_encode(self, input, errors, final):
        raise NotImplementedError

    def encode(self, input, final=False):
        data = self.buffer + input
        (result, consumed) = self._buffer_encode(data, self.errors, final)
        self.buffer = data[consumed:]
        return result

    def reset(self):
        IncrementalEncoder.reset(self)
        self.buffer = ''

    def getstate(self):
        return self.buffer or 0

    def setstate(self, state):
        self.buffer = state or ''


class IncrementalDecoder:
    def __init__(self, errors='strict'):
        self.errors = errors

    def decode(self, input, final=False):
        raise NotImplementedError

    def reset(self):
        pass

    def getstate(self):
        return (b'', 0)

    def setstate(self, state):
        pass


class BufferedIncrementalDecoder(IncrementalDecoder):
    def __init__(self, errors='strict'):
        IncrementalDecoder.__init__(self, errors)
        self.buffer = b''

    def _buffer_decode(self, input, errors, final):
        raise NotImplementedError

    def decode(self, input, final=False):
        data = self.buffer + bytes(input)
        result, consumed = self._buffer_decode(data, self.errors, final)
        self.buffer = data[consumed:]
        return result

    def reset(self):
        IncrementalDecoder.reset(self)
        self.buffer = b''

    def getstate(self):
        return (self.buffer, 0)

    def setstate(self, state):
        self.buffer = state[0]


def _inc_encoder(name):
    class _Enc(IncrementalEncoder):
        def encode(self, input, final=False):
            return encode(input, name, self.errors)
    _Enc.__name__ = 'IncrementalEncoder'
    return _Enc


def _inc_decoder(name):
    class _Dec(BufferedIncrementalDecoder):
        def _buffer_decode(self, input, errors, final):
            if final or name in _BYTES_TO_BYTES:
                return decode(input, name, errors), len(input)
            end = len(input)
            while end > 0:
                try:
                    return bytes(input[:end]).decode(name if name != 'iso8859-1' else 'latin-1', errors), end
                except UnicodeDecodeError:
                    if end < len(input) - 4:
                        raise
                    end -= 1
            return '', 0
    _Dec.__name__ = 'IncrementalDecoder'
    return _Dec


class Codec:
    def encode(self, input, errors='strict'):
        raise NotImplementedError

    def decode(self, input, errors='strict'):
        raise NotImplementedError


class StreamWriter(Codec):
    def __init__(self, stream, errors='strict'):
        self.stream = stream
        self.errors = errors

    def write(self, object):
        data, _ = self.encode(object, self.errors)
        self.stream.write(data)

    def writelines(self, list):
        self.write(''.join(list))

    def reset(self):
        pass

    def __getattr__(self, name):
        return getattr(self.stream, name)

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.stream.close()


class StreamReader(Codec):
    def __init__(self, stream, errors='strict'):
        self.stream = stream
        self.errors = errors
        self._text = None

    def _load(self):
        if self._text is None:
            self._text = self.decode(self.stream.read(), self.errors)[0]
            self._pos = 0

    def read(self, size=-1, chars=-1, firstline=False):
        self._load()
        n = chars if chars >= 0 else size
        if n is None or n < 0:
            out = self._text[self._pos:]
            self._pos = len(self._text)
        else:
            out = self._text[self._pos:self._pos + n]
            self._pos += len(out)
        return out

    def readline(self, size=None, keepends=True):
        self._load()
        i = self._text.find('\n', self._pos)
        end = len(self._text) if i < 0 else i + 1
        line = self._text[self._pos:end]
        self._pos = end
        return line if keepends else line.rstrip('\r\n')

    def readlines(self, sizehint=None, keepends=True):
        return self.read().splitlines(keepends)

    def __iter__(self):
        return self

    def __next__(self):
        line = self.readline()
        if line:
            return line
        raise StopIteration

    def reset(self):
        self._text = None

    def __getattr__(self, name):
        return getattr(self.stream, name)

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.stream.close()


def _stream_reader(name):
    info = {}

    class _R(StreamReader):
        def decode(self, input, errors='strict'):
            return lookup(name).decode(input, errors)
    _R.__name__ = 'StreamReader'
    return _R


def _stream_writer(name):
    class _W(StreamWriter):
        def encode(self, input, errors='strict'):
            return lookup(name).encode(input, errors)
    _W.__name__ = 'StreamWriter'
    return _W


class StreamReaderWriter:
    def __init__(self, stream, Reader, Writer, errors='strict'):
        self.stream = stream
        self.reader = Reader(stream, errors)
        self.writer = Writer(stream, errors)
        self.errors = errors

    def read(self, size=-1):
        return self.reader.read(size)

    def readline(self, size=None):
        return self.reader.readline(size)

    def readlines(self, sizehint=None):
        return self.reader.readlines(sizehint)

    def __next__(self):
        return next(self.reader)

    def __iter__(self):
        return self

    def write(self, data):
        return self.writer.write(data)

    def writelines(self, list):
        return self.writer.writelines(list)

    def __getattr__(self, name):
        return getattr(self.stream, name)

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.stream.close()


def open(filename, mode='r', encoding=None, errors='strict', buffering=-1):
    if encoding is not None and 'b' not in mode:
        mode = mode + 'b'
    file = _io.open(filename, mode, buffering)
    if encoding is None:
        return file
    info = lookup(encoding)
    srw = StreamReaderWriter(file, info.streamreader, info.streamwriter, errors)
    srw.encoding = encoding
    return srw


def EncodedFile(file, data_encoding, file_encoding=None, errors='strict'):
    if file_encoding is None:
        file_encoding = data_encoding
    return StreamReaderWriter(file, getreader(file_encoding), getwriter(file_encoding), errors)


def make_identity_dict(rng):
    return {i: i for i in rng}
