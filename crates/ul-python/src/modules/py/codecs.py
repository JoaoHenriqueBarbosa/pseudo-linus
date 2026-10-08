"""Registro de codecs sobre `str.encode`/`bytes.decode`, mais as transformações de bytes comuns."""

import builtins
import sys

from _codecs import *

__all__ = ["register", "lookup", "open", "EncodedFile", "BOM", "BOM_BE",
           "BOM_LE", "BOM32_BE", "BOM32_LE", "BOM64_BE", "BOM64_LE",
           "BOM_UTF8", "BOM_UTF16", "BOM_UTF16_LE", "BOM_UTF16_BE",
           "BOM_UTF32", "BOM_UTF32_LE", "BOM_UTF32_BE",
           "CodecInfo", "Codec", "IncrementalEncoder", "IncrementalDecoder",
           "StreamReader", "StreamWriter",
           "StreamReaderWriter", "StreamRecoder",
           "getencoder", "getdecoder", "getincrementalencoder",
           "getincrementaldecoder", "getreader", "getwriter",
           "encode", "decode", "iterencode", "iterdecode",
           "strict_errors", "ignore_errors", "replace_errors",
           "xmlcharrefreplace_errors",
           "backslashreplace_errors", "namereplace_errors",
           "register_error", "lookup_error"]

BOM_UTF8 = b'\xef\xbb\xbf'
BOM_LE = BOM_UTF16_LE = b'\xff\xfe'
BOM_BE = BOM_UTF16_BE = b'\xfe\xff'
BOM_UTF32_LE = b'\xff\xfe\x00\x00'
BOM_UTF32_BE = b'\x00\x00\xfe\xff'
if sys.byteorder == 'little':
    BOM = BOM_UTF16 = BOM_UTF16_LE
    BOM_UTF32 = BOM_UTF32_LE
else:
    BOM = BOM_UTF16 = BOM_UTF16_BE
    BOM_UTF32 = BOM_UTF32_BE
BOM32_LE = BOM_UTF16_LE
BOM32_BE = BOM_UTF16_BE
BOM64_LE = BOM_UTF32_LE
BOM64_BE = BOM_UTF32_BE

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
    'utf-7': 'utf-7', 'utf7': 'utf-7', 'u7': 'utf-7', 'unicode-1-1-utf-7': 'utf-7',
    'cp1252': 'cp1252', 'windows-1252': 'cp1252',
    'rot13': 'rot-13', 'rot-13': 'rot-13', 'rot_13': 'rot-13',
    'hex': 'hex', 'hex_codec': 'hex', 'base64': 'base64', 'base64_codec': 'base64',
    'zlib': 'zlib', 'zip': 'zlib', 'zlib_codec': 'zlib',
    'unicode_escape': 'unicode-escape', 'unicode-escape': 'unicode-escape',
    'raw_unicode_escape': 'raw-unicode-escape',
    'idna': 'idna', 'punycode': 'punycode',
}
_BYTES_TO_BYTES = {'hex', 'base64', 'zlib'}
_NOT_TEXT = {'hex', 'base64', 'zlib', 'rot-13'}

# Os codecs cujas funções sem estado são as do `_codecs`: `(codificador, decodificador, aceita final)`.
_STATELESS = {
    'utf-8': (utf_8_encode, utf_8_decode, True),
    'ascii': (ascii_encode, ascii_decode, False),
    'iso8859-1': (latin_1_encode, latin_1_decode, False),
    'utf-16': (utf_16_encode, utf_16_decode, True),
    'utf-16-le': (utf_16_le_encode, utf_16_le_decode, True),
    'utf-16-be': (utf_16_be_encode, utf_16_be_decode, True),
    'utf-32': (utf_32_encode, utf_32_decode, True),
    'utf-32-le': (utf_32_le_encode, utf_32_le_decode, True),
    'utf-32-be': (utf_32_be_encode, utf_32_be_decode, True),
    'utf-7': (utf_7_encode, utf_7_decode, True),
    'unicode-escape': (unicode_escape_encode, unicode_escape_decode, True),
    'raw-unicode-escape': (raw_unicode_escape_encode, raw_unicode_escape_decode, True),
}


### Codec base classes (defining the API)

class CodecInfo(tuple):
    """Codec details when looking up the codec registry"""

    # Private API to allow Python 3.4 to denylist the known non-Unicode
    # codecs in the standard library. A more general mechanism to
    # reliably distinguish test encodings from other codecs will hopefully
    # be defined for Python 3.5
    #
    # See http://bugs.python.org/issue19619
    _is_text_encoding = True # Assume codecs are text encodings by default

    def __new__(cls, encode, decode, streamreader=None, streamwriter=None,
        incrementalencoder=None, incrementaldecoder=None, name=None,
        *, _is_text_encoding=None):
        self = tuple.__new__(cls, (encode, decode, streamreader, streamwriter))
        self.name = name
        self.encode = encode
        self.decode = decode
        self.incrementalencoder = incrementalencoder
        self.incrementaldecoder = incrementaldecoder
        self.streamwriter = streamwriter
        self.streamreader = streamreader
        if _is_text_encoding is not None:
            self._is_text_encoding = _is_text_encoding
        return self

    def __repr__(self):
        return "<%s.%s object for encoding %s at %#x>" % \
                (self.__class__.__module__, self.__class__.__qualname__,
                 self.name, id(self))

    def __getnewargs__(self):
        return tuple(self)


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


def _buffer(data):
    """Os bytes de um objeto com buffer, com a recusa do `Py_buffer` do CPython."""
    if isinstance(data, str):
        raise TypeError("a bytes-like object is required, not 'str'")
    try:
        return bytes(memoryview(data))
    except TypeError:
        raise TypeError("a bytes-like object is required, not '%s'" % type(data).__name__) from None


def _ascii_buffer(data):
    return data.encode('ascii') if isinstance(data, str) else _buffer(data)


def _hex_enc(data, errors='strict'):
    return _buffer(data).hex().encode('ascii'), len(data)


def _hex_dec(data, errors='strict'):
    return bytes.fromhex(_ascii_buffer(data).decode('ascii')), len(data)


def _b64_enc(data, errors='strict'):
    import base64
    return base64.encodebytes(_buffer(data)), len(data)


def _b64_dec(data, errors='strict'):
    import base64
    return base64.decodebytes(_ascii_buffer(data)), len(data)


def _zlib_enc(data, errors='strict'):
    import zlib
    return zlib.compress(_buffer(data)), len(data)


def _zlib_dec(data, errors='strict'):
    import zlib
    return zlib.decompress(_ascii_buffer(data)), len(data)


_ROT = str.maketrans('ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz',
                     'NOPQRSTUVWXYZABCDEFGHIJKLMnopqrstuvwxyzabcdefghijklm')


def _rot_enc(text, errors='strict'):
    return str.translate(text, _ROT), len(text)


def _stateless(encode_fn, decode_fn, with_final):
    """O par `(encode, decode)` de `encodings/*.py` sobre as funções do `_codecs`."""
    def encoder(text, errors='strict'):
        return encode_fn(text, errors)

    if with_final:
        def decoder(data, errors='strict'):
            return decode_fn(data, errors, True)
    else:
        def decoder(data, errors='strict'):
            return decode_fn(data, errors)
    return encoder, decoder


def _charmap_pair(name):
    """O par de um codec de texto que o núcleo conhece (cp125x, iso8859-x, koi8, utf-8-sig...)."""
    label = 'utf_8_encode' if name == 'utf-8-sig' else 'charmap_encode'

    def enc(text, errors='strict'):
        if not isinstance(text, str):
            raise TypeError('%s() argument 1 must be str, not %s' % (label, type(text).__name__))
        return text.encode(name, errors), len(text)

    def dec(data, errors='strict'):
        return _buffer(data).decode(name, errors), len(data)
    return enc, dec


def _make(name):
    if name == 'hex':
        enc, dec = _hex_enc, _hex_dec
    elif name == 'base64':
        enc, dec = _b64_enc, _b64_dec
    elif name == 'zlib':
        enc, dec = _zlib_enc, _zlib_dec
    elif name == 'rot-13':
        enc, dec = _rot_enc, _rot_enc
    elif name in _STATELESS:
        enc, dec = _stateless(*_STATELESS[name])
    else:
        enc, dec = _charmap_pair(name)
    return CodecInfo(enc, dec, name=name,
                     streamreader=_stream_reader(name), streamwriter=_stream_writer(name),
                     incrementalencoder=_inc_encoder(name), incrementaldecoder=_inc_decoder(name),
                     _is_text_encoding=name not in _NOT_TEXT)


_cache = {}


def _search(name):
    """A função de busca dos codecs da imagem, a que o `encodings` registra no CPython."""
    canonical = _norm(name)
    if canonical is None:
        return None
    if canonical not in _cache:
        _cache[canonical] = _make(canonical)
    return _cache[canonical]


register(_search)


### Shortcuts

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


def _idna_split(text):
    """`dots.split(text)` de `encodings/idna.py`: separa em U+002E, U+3002, U+FF0E e U+FF61."""
    labels = []
    start = 0
    for i, ch in enumerate(text):
        if ch in '.。．｡':
            labels.append(text[start:i])
            start = i + 1
    labels.append(text[start:])
    return labels


def _idna_incremental_encoder():
    import _idna

    class IncrementalEncoder(BufferedIncrementalEncoder):
        def _buffer_encode(self, input, errors, final):
            if errors != 'strict':
                raise UnicodeError(f"Unsupported error handling: {errors}")
            if not input:
                return (b'', 0)
            labels = _idna_split(input)
            trailing_dot = b''
            if labels:
                if not labels[-1]:
                    trailing_dot = b'.'
                    del labels[-1]
                elif not final:
                    # Mantém o rótulo possivelmente incompleto até a próxima chamada.
                    del labels[-1]
                    if labels:
                        trailing_dot = b'.'
            result = bytearray()
            size = 0
            for label in labels:
                if size:
                    result.extend(b'.')
                    size += 1
                try:
                    result.extend(_idna.to_ascii(label))
                except (UnicodeEncodeError, UnicodeDecodeError) as exc:
                    raise UnicodeEncodeError("idna", input, size + exc.start, size + exc.end, exc.reason)
                size += len(label)
            result += trailing_dot
            size += len(trailing_dot)
            return (bytes(result), size)
    return IncrementalEncoder


def _idna_incremental_decoder():
    import _idna

    class IncrementalDecoder(BufferedIncrementalDecoder):
        def _buffer_decode(self, input, errors, final):
            if errors != 'strict':
                raise UnicodeError("Unsupported error handling: {errors}")
            if not input:
                return ("", 0)
            if isinstance(input, str):
                labels = _idna_split(input)
            else:
                try:
                    input = str(input, "ascii")
                except (UnicodeEncodeError, UnicodeDecodeError) as exc:
                    raise UnicodeDecodeError("idna", input, exc.start, exc.end, exc.reason)
                labels = input.split(".")
            trailing_dot = ''
            if labels:
                if not labels[-1]:
                    trailing_dot = '.'
                    del labels[-1]
                elif not final:
                    del labels[-1]
                    if labels:
                        trailing_dot = '.'
            result = []
            size = 0
            for label in labels:
                try:
                    u_label = _idna.to_unicode(label)
                except (UnicodeEncodeError, UnicodeDecodeError) as exc:
                    raise UnicodeDecodeError("idna", input.encode("ascii", errors="backslashreplace"),
                                             size + exc.start, size + exc.end, exc.reason)
                else:
                    result.append(u_label)
                if size:
                    size += 1
                size += len(label)
            result = ".".join(result) + trailing_dot
            size += len(trailing_dot)
            return (result, size)
    return IncrementalDecoder


def _punycode_incremental_encoder():
    class _Enc(IncrementalEncoder):
        def encode(self, input, final=False):
            return input.encode('punycode')
    _Enc.__name__ = 'IncrementalEncoder'
    return _Enc


def _punycode_incremental_decoder():
    class _Dec(IncrementalDecoder):
        def decode(self, input, final=False):
            if self.errors not in ('strict', 'replace', 'ignore'):
                raise UnicodeError(f"Unsupported error handling: {self.errors}")
            if isinstance(input, str):
                input = input.encode('ascii')
            return bytes(input).decode('punycode', self.errors)
    _Dec.__name__ = 'IncrementalDecoder'
    return _Dec


def _inc_encoder(name):
    if name == 'idna':
        return _idna_incremental_encoder()
    if name == 'punycode':
        return _punycode_incremental_encoder()

    class _Enc(IncrementalEncoder):
        def encode(self, input, final=False):
            return encode(input, name, self.errors)
    _Enc.__name__ = 'IncrementalEncoder'
    return _Enc


def _inc_decoder(name):
    if name == 'idna':
        return _idna_incremental_decoder()
    if name == 'punycode':
        return _punycode_incremental_decoder()
    if name == 'utf-7':
        class IncrementalDecoder(BufferedIncrementalDecoder):
            def _buffer_decode(self, input, errors, final):
                return utf_7_decode(input, errors, final)
        return IncrementalDecoder

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


class StreamRecoder:

    """ StreamRecoder instances translate data from one encoding to another.

        They use the complete set of APIs returned by the
        codecs.lookup() function to implement their task.

        Data written to the StreamRecoder is first decoded into an
        intermediate format (depending on the "decode" codec) and then
        written to the underlying stream using an instance of the provided
        Writer class.

        In the other direction, data is read from the underlying stream using
        a Reader instance and then encoded and returned to the caller.

    """
    # Optional attributes set by the file wrappers below
    data_encoding = 'unknown'
    file_encoding = 'unknown'

    def __init__(self, stream, encode, decode, Reader, Writer,
                 errors='strict'):
        self.stream = stream
        self.encode = encode
        self.decode = decode
        self.reader = Reader(stream, errors)
        self.writer = Writer(stream, errors)
        self.errors = errors

    def read(self, size=-1):

        data = self.reader.read(size)
        data, bytesencoded = self.encode(data, self.errors)
        return data

    def readline(self, size=None):

        if size is None:
            data = self.reader.readline()
        else:
            data = self.reader.readline(size)
        data, bytesencoded = self.encode(data, self.errors)
        return data

    def readlines(self, sizehint=None):

        data = self.reader.read()
        data, bytesencoded = self.encode(data, self.errors)
        return data.splitlines(keepends=True)

    def __next__(self):

        """ Return the next decoded line from the input stream."""
        data = next(self.reader)
        data, bytesencoded = self.encode(data, self.errors)
        return data

    def __iter__(self):
        return self

    def write(self, data):

        data, bytesdecoded = self.decode(data, self.errors)
        return self.writer.write(data)

    def writelines(self, list):

        data = b''.join(list)
        data, bytesdecoded = self.decode(data, self.errors)
        return self.writer.write(data)

    def reset(self):

        self.reader.reset()
        self.writer.reset()

    def seek(self, offset, whence=0):
        # Seeks must be propagated to both the readers and writers
        # as they might need to reset their internal buffers.
        self.reader.seek(offset, whence)
        self.writer.seek(offset, whence)

    def __getattr__(self, name,
                    getattr=getattr):

        """ Inherit all other methods from the underlying stream.
        """
        return getattr(self.stream, name)

    def __enter__(self):
        return self

    def __exit__(self, type, value, tb):
        self.stream.close()

    def __reduce_ex__(self, proto):
        raise TypeError("can't serialize %s" % self.__class__.__name__)


def open(filename, mode='r', encoding=None, errors='strict', buffering=-1):
    if encoding is not None and 'b' not in mode:
        mode = mode + 'b'
    file = builtins.open(filename, mode, buffering)
    if encoding is None:
        return file
    try:
        info = lookup(encoding)
        srw = StreamReaderWriter(file, info.streamreader, info.streamwriter, errors)
        srw.encoding = encoding
        return srw
    except:
        file.close()
        raise


def EncodedFile(file, data_encoding, file_encoding=None, errors='strict'):
    if file_encoding is None:
        file_encoding = data_encoding
    data_info = lookup(data_encoding)
    file_info = lookup(file_encoding)
    sr = StreamRecoder(file, data_info.encode, data_info.decode,
                       file_info.streamreader, file_info.streamwriter, errors)
    sr.data_encoding = data_encoding
    sr.file_encoding = file_encoding
    return sr


def make_identity_dict(rng):
    return {i: i for i in rng}


def make_encoding_map(decoding_map):
    """ Creates an encoding map from a decoding map.

        If a target mapping in the decoding map occurs multiple
        times, then that target is mapped to None (undefined mapping),
        causing an exception when encountered by the charmap codec
        during translation.

        One example where this happens is cp875.py which decodes
        multiple character to \\u001a.

    """
    m = {}
    for k,v in decoding_map.items():
        if not v in m:
            m[v] = k
        else:
            m[v] = None
    return m


### error handlers

try:
    strict_errors = lookup_error("strict")
    ignore_errors = lookup_error("ignore")
    replace_errors = lookup_error("replace")
    xmlcharrefreplace_errors = lookup_error("xmlcharrefreplace")
    backslashreplace_errors = lookup_error("backslashreplace")
    namereplace_errors = lookup_error("namereplace")
except LookupError:
    # In --disable-unicode builds, these error handler are missing
    strict_errors = None
    ignore_errors = None
    replace_errors = None
    xmlcharrefreplace_errors = None
    backslashreplace_errors = None
    namereplace_errors = None

# Tell modulefinder that using codecs probably needs the encodings
# package
_false = 0
if _false:
    import encodings
