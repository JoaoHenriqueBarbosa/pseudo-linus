"""pyexpat: analisador XML incremental com a interface do expat (escrito em Python).

Cobre o que `xml.etree`, `xml.dom.minidom` e `xml.sax` usam: manipuladores de elemento, texto,
comentário, instrução de processamento, seção CDATA, declaração XML e doctype, espaços de nomes com
`namespace_separator`, entidades predefinidas, referências numéricas e entidades internas do DTD.
O DTD externo não é lido."""

import re as _re

__all__ = ['ParserCreate', 'ExpatError', 'error', 'XMLParserType', 'errors', 'model', 'version_info', 'EXPAT_VERSION',
           'ErrorString', 'native_encoding', 'features', 'XML_PARAM_ENTITY_PARSING_NEVER',
           'XML_PARAM_ENTITY_PARSING_UNLESS_STANDALONE', 'XML_PARAM_ENTITY_PARSING_ALWAYS']

EXPAT_VERSION = 'expat_2.7.1'
version_info = (2, 7, 1)
native_encoding = 'UTF-8'
features = [('sizeof(XML_Char)', 1), ('sizeof(XML_LChar)', 1), ('XML_DTD', 0), ('XML_CONTEXT_BYTES', 1024), ('XML_NS', 0)]
XML_PARAM_ENTITY_PARSING_NEVER = 0
XML_PARAM_ENTITY_PARSING_UNLESS_STANDALONE = 1
XML_PARAM_ENTITY_PARSING_ALWAYS = 2


class ExpatError(Exception):
    pass


error = ExpatError


class _Errors:
    XML_ERROR_NO_MEMORY = 'out of memory'
    XML_ERROR_SYNTAX = 'syntax error'
    XML_ERROR_NO_ELEMENTS = 'no element found'
    XML_ERROR_INVALID_TOKEN = 'not well-formed (invalid token)'
    XML_ERROR_UNCLOSED_TOKEN = 'unclosed token'
    XML_ERROR_PARTIAL_CHAR = 'partial character'
    XML_ERROR_TAG_MISMATCH = 'mismatched tag'
    XML_ERROR_DUPLICATE_ATTRIBUTE = 'duplicate attribute'
    XML_ERROR_JUNK_AFTER_DOC_ELEMENT = 'junk after document element'
    XML_ERROR_UNDEFINED_ENTITY = 'undefined entity'
    XML_ERROR_BAD_CHAR_REF = 'reference to invalid character number'
    XML_ERROR_UNBOUND_PREFIX = 'unbound prefix'
    XML_ERROR_UNCLOSED_CDATA_SECTION = 'unclosed CDATA section'
    XML_ERROR_FINISHED = 'parsing finished'
    codes = {}
    messages = {}


for _i, _name in enumerate([n for n in dir(_Errors) if n.startswith('XML_ERROR_')], 1):
    _Errors.codes[getattr(_Errors, _name)] = _i
    _Errors.messages[_i] = getattr(_Errors, _name)


class _Model:
    XML_CTYPE_EMPTY = 1
    XML_CTYPE_ANY = 2
    XML_CTYPE_MIXED = 3
    XML_CTYPE_NAME = 4
    XML_CTYPE_CHOICE = 5
    XML_CTYPE_SEQ = 6


model = _Model()

_NAME_START = r'[A-Za-z_:À-￿]'
_NAME_CHAR = r'[A-Za-z0-9_:.\-·À-￿]'
_NAME = _re.compile(_NAME_START + _NAME_CHAR + '*')
_PREDEFINED = {'lt': '<', 'gt': '>', 'amp': '&', 'quot': '"', 'apos': "'"}
_WS = ' \t\r\n'


class XMLParserType:
    def __init__(self, encoding, namespace_separator, intern):
        self.encoding = encoding
        self.intern = {} if intern is None else intern
        self._sep = namespace_separator
        self.buffer_text = False
        self.buffer_size = 8192
        self.buffer_used = 0
        self.ordered_attributes = False
        self.specified_attributes = False
        self.returns_unicode = True
        self.namespace_prefixes = False
        self.ErrorCode = 0
        self.ErrorLineNumber = 1
        self.ErrorColumnNumber = 0
        self.ErrorByteIndex = 0
        self.CurrentLineNumber = 1
        self.CurrentColumnNumber = 0
        self.CurrentByteIndex = 0
        self.StartElementHandler = None
        self.EndElementHandler = None
        self.CharacterDataHandler = None
        self.ProcessingInstructionHandler = None
        self.CommentHandler = None
        self.StartCdataSectionHandler = None
        self.EndCdataSectionHandler = None
        self.DefaultHandler = None
        self.DefaultHandlerExpand = None
        self.XmlDeclHandler = None
        self.StartDoctypeDeclHandler = None
        self.EndDoctypeDeclHandler = None
        self.StartNamespaceDeclHandler = None
        self.EndNamespaceDeclHandler = None
        self.NotStandaloneHandler = None
        self.ExternalEntityRefHandler = None
        self.UnparsedEntityDeclHandler = None
        self.NotationDeclHandler = None
        self.EntityDeclHandler = None
        self.ElementDeclHandler = None
        self.AttlistDeclHandler = None
        self.SkippedEntityHandler = None
        self._buf = ''
        self._bytes = b''
        self._stack = []
        self._ns_stack = []
        self._text = []
        self._started = False
        self._root_done = False
        self._finished = False
        self._entities = {}
        self._line = 1
        self._col = 0
        self._standalone = -1

    # -- erros e posição

    def _fail(self, message, line=None, col=None):
        self.ErrorLineNumber = self._line if line is None else line
        self.ErrorColumnNumber = self._col if col is None else col
        self.ErrorCode = _Errors.codes.get(message, 1)
        self._finished = True
        err = ExpatError('%s: line %d, column %d' % (message, self.ErrorLineNumber, self.ErrorColumnNumber))
        err.code = self.ErrorCode
        err.lineno = self.ErrorLineNumber
        err.offset = self.ErrorColumnNumber
        raise err

    def _advance(self, text):
        n = text.count('\n')
        if n:
            self._line += n
            self._col = len(text) - text.rfind('\n') - 1
        else:
            self._col += len(text)

    # -- emissão

    def _flush_text(self):
        if self._text:
            data = ''.join(self._text)
            self._text = []
            h = self.CharacterDataHandler
            if h is not None:
                h(data)
            elif self.DefaultHandler is not None:
                self.DefaultHandler(data)

    def _characters(self, data):
        if not data:
            return
        if self.buffer_text:
            self._text.append(data)
        else:
            h = self.CharacterDataHandler
            if h is not None:
                h(data)

    # -- interface pública

    def SetBase(self, base):
        self._base = base

    def GetBase(self):
        return getattr(self, '_base', None)

    def SetParamEntityParsing(self, flag):
        return 1

    def UseForeignDTD(self, flag=True):
        pass

    def GetInputContext(self):
        return None

    def ExternalEntityParserCreate(self, context, encoding=None):
        return XMLParserType(encoding, self._sep, None)

    def Parse(self, data, isfinal=False):
        if isinstance(data, str):
            text = data
        else:
            data = self._bytes + bytes(data)
            self._bytes = b''
            enc = (self.encoding or self._sniff(data) or 'utf-8').lower()
            if enc in ('utf-8', 'utf8', 'us-ascii', 'ascii'):
                try:
                    text = data.decode('utf-8')
                except UnicodeDecodeError as e:
                    if not isfinal and e.start >= len(data) - 3:
                        self._bytes = data[e.start:]
                        text = data[:e.start].decode('utf-8')
                    else:
                        self._fail('not well-formed (invalid token)')
            else:
                text = data.decode(enc)
        if self._finished and text:
            self._fail('parsing finished')
        self._buf += text
        self._pump(isfinal)
        if isfinal:
            if self._stack:
                self._fail('no element found' if not self._started else 'unclosed token')
            if not self._started:
                self._fail('no element found')
            self._flush_text()
            self._finished = True
        else:
            self._flush_text()
        return 1

    def ParseFile(self, file):
        data = file.read()
        return self.Parse(data, True)

    @staticmethod
    def _sniff(data):
        if data[:3] == b'\xef\xbb\xbf':
            return 'utf-8'
        if data[:2] in (b'\xff\xfe', b'\xfe\xff'):
            return 'utf-16'
        m = _re.match(r'<\?xml[^>]*encoding=["\']([A-Za-z0-9._\-]+)["\']', data[:200].decode('ascii', 'ignore'))
        if m:
            return m.group(1)
        return None

    # -- núcleo

    def _pump(self, final):
        buf = self._buf
        i = 0
        n = len(buf)
        if not self._started and not self._stack and buf[:1] == '﻿':
            i = 1
        while i < n:
            lt = buf.find('<', i)
            if lt < 0:
                text = buf[i:]
                amp = text.rfind('&')
                if amp >= 0 and ';' not in text[amp:] and not final:
                    # referência cortada no fim do bloco: espera o resto
                    self._text_chunk(text[:amp])
                    i += amp
                    break
                self._text_chunk(text)
                i = n
                break
            if lt > i:
                self._text_chunk(buf[i:lt])
                i = lt
            # buf[i] == '<'
            if buf.startswith('<!--', i):
                end = buf.find('-->', i + 4)
                if end < 0:
                    break
                body = buf[i + 4:end]
                if '--' in body:
                    self._fail('not well-formed (invalid token)')
                self._flush_text()
                if self.CommentHandler is not None:
                    self.CommentHandler(body)
                self._advance(buf[i:end + 3])
                i = end + 3
            elif buf.startswith('<![CDATA[', i):
                end = buf.find(']]>', i + 9)
                if end < 0:
                    break
                if not self._stack:
                    self._fail('syntax error')
                self._flush_text()
                if self.StartCdataSectionHandler is not None:
                    self.StartCdataSectionHandler()
                data = buf[i + 9:end]
                if self.CharacterDataHandler is not None:
                    if self.buffer_text:
                        self._text.append(data)
                        self._flush_text()
                    else:
                        self.CharacterDataHandler(data)
                if self.EndCdataSectionHandler is not None:
                    self.EndCdataSectionHandler()
                self._advance(buf[i:end + 3])
                i = end + 3
            elif buf.startswith('<?', i):
                end = buf.find('?>', i + 2)
                if end < 0:
                    break
                self._pi(buf[i + 2:end], buf[i:end + 2])
                self._advance(buf[i:end + 2])
                i = end + 2
            elif buf.startswith('<!DOCTYPE', i):
                end = self._doctype_end(buf, i)
                if end < 0:
                    break
                self._doctype(buf[i:end])
                self._advance(buf[i:end])
                i = end
            elif buf.startswith('</', i):
                end = buf.find('>', i)
                if end < 0:
                    break
                name = buf[i + 2:end].rstrip(_WS)
                self._end_tag(name)
                self._advance(buf[i:end + 1])
                i = end + 1
            elif i + 1 >= n:
                break
            elif buf[i + 1] == '!':
                if not final and len(buf) - i < 9:
                    break
                self._fail('not well-formed (invalid token)')
            else:
                end = self._tag_end(buf, i)
                if end < 0:
                    break
                self._start_tag(buf[i + 1:end])
                self._advance(buf[i:end + 1])
                i = end + 1
        self._buf = buf[i:]
        if final and self._buf.strip(_WS):
            rest = self._buf
            if rest.startswith('<'):
                self._fail('unclosed token')
            self._fail('junk after document element' if self._root_done else 'unclosed token')

    @staticmethod
    def _tag_end(buf, i):
        quote = None
        j = i + 1
        n = len(buf)
        while j < n:
            c = buf[j]
            if quote:
                if c == quote:
                    quote = None
            elif c == '"' or c == "'":
                quote = c
            elif c == '>':
                return j
            j += 1
        return -1

    @staticmethod
    def _doctype_end(buf, i):
        depth = 0
        j = i
        n = len(buf)
        quote = None
        while j < n:
            c = buf[j]
            if quote:
                if c == quote:
                    quote = None
            elif c in '"\'':
                quote = c
            elif c == '[':
                depth += 1
            elif c == ']':
                depth -= 1
            elif c == '>' and depth <= 0:
                return j + 1
            j += 1
        return -1

    def _text_chunk(self, text):
        if not self._stack:
            if text.strip(_WS):
                if self._root_done:
                    self._fail('junk after document element')
                self._fail('syntax error')
            self._advance(text)
            return
        out = []
        k = 0
        while True:
            amp = text.find('&', k)
            if amp < 0:
                out.append(text[k:])
                break
            out.append(text[k:amp])
            semi = text.find(';', amp)
            if semi < 0:
                self._fail('not well-formed (invalid token)')
            out.append(self._reference(text[amp + 1:semi]))
            k = semi + 1
        data = ''.join(out)
        if '\r' in data:
            data = data.replace('\r\n', '\n').replace('\r', '\n')
        self._characters(data)
        self._advance(text)

    def _reference(self, name):
        if name.startswith('#'):
            try:
                code = int(name[2:], 16) if name[1:2] in ('x', 'X') else int(name[1:])
                return chr(code)
            except (ValueError, OverflowError):
                self._fail('reference to invalid character number')
        if name in _PREDEFINED:
            return _PREDEFINED[name]
        if name in self._entities:
            return self._entities[name]
        self._fail('undefined entity')

    def _pi(self, body, whole):
        m = _NAME.match(body)
        if m is None:
            self._fail('not well-formed (invalid token)')
        target = m.group(0)
        data = body[m.end():].lstrip(_WS)
        if target.lower() == 'xml':
            if self._started or self._stack:
                self._fail('XML or text declaration not at start of entity')
            attrs = dict(_re.findall(r'(\w+)\s*=\s*["\']([^"\']*)["\']', data))
            if self.XmlDeclHandler is not None:
                standalone = {'yes': 1, 'no': 0}.get(attrs.get('standalone'), -1)
                self.XmlDeclHandler(attrs.get('version'), attrs.get('encoding'), standalone)
            return
        self._flush_text()
        if self.ProcessingInstructionHandler is not None:
            self.ProcessingInstructionHandler(target, data)

    def _doctype(self, text):
        m = _re.match(r'<!DOCTYPE\s+(\S+?)(?:\s+(?:SYSTEM\s+["\']([^"\']*)["\']|PUBLIC\s+["\']([^"\']*)["\']\s*(?:["\']([^"\']*)["\'])?))?\s*(\[.*\])?\s*>$', text, _re.S)
        if m is None:
            self._fail('syntax error')
        name, sysid, pubid, sysid2, internal = m.groups()
        if self.StartDoctypeDeclHandler is not None:
            self.StartDoctypeDeclHandler(name, sysid or sysid2, pubid, 1 if internal else 0)
        if internal:
            for em in _re.finditer(r'<!ENTITY\s+([^\s%]+)\s+(?:"([^"]*)"|\'([^\']*)\')\s*>', internal):
                self._entities[em.group(1)] = em.group(2) if em.group(2) is not None else em.group(3)
        if self.EndDoctypeDeclHandler is not None:
            self.EndDoctypeDeclHandler()

    # -- tags

    def _split_name(self, qname):
        if ':' in qname:
            prefix, local = qname.split(':', 1)
            return prefix, local
        return None, qname

    def _resolve(self, qname, is_attr):
        if self._sep is None:
            return qname
        prefix, local = self._split_name(qname)
        if prefix is None:
            if is_attr:
                return local
            for scope in reversed(self._ns_stack):
                if None in scope:
                    uri = scope[None]
                    return (uri + self._sep + local) if uri else local
            return local
        if prefix == 'xml':
            return 'http://www.w3.org/XML/1998/namespace' + self._sep + local
        for scope in reversed(self._ns_stack):
            if prefix in scope:
                return scope[prefix] + self._sep + local
        self._fail('unbound prefix')

    def _parse_attrs(self, body):
        attrs = []
        i = 0
        n = len(body)
        while True:
            while i < n and body[i] in _WS:
                i += 1
            if i >= n:
                break
            m = _NAME.match(body, i)
            if m is None:
                self._fail('not well-formed (invalid token)')
            name = m.group(0)
            i = m.end()
            while i < n and body[i] in _WS:
                i += 1
            if i >= n or body[i] != '=':
                self._fail('not well-formed (invalid token)')
            i += 1
            while i < n and body[i] in _WS:
                i += 1
            if i >= n or body[i] not in '"\'':
                self._fail('not well-formed (invalid token)')
            q = body[i]
            end = body.find(q, i + 1)
            if end < 0:
                self._fail('unclosed token')
            raw = body[i + 1:end]
            if '<' in raw:
                self._fail('not well-formed (invalid token)')
            i = end + 1
            if i < n and body[i] not in _WS:
                self._fail('not well-formed (invalid token)')
            out = []
            k = 0
            while True:
                amp = raw.find('&', k)
                if amp < 0:
                    out.append(raw[k:])
                    break
                out.append(raw[k:amp])
                semi = raw.find(';', amp)
                if semi < 0:
                    self._fail('not well-formed (invalid token)')
                out.append(self._reference(raw[amp + 1:semi]))
                k = semi + 1
            value = ''.join(out).replace('\t', ' ').replace('\n', ' ').replace('\r', ' ')
            attrs.append((name, value))
        return attrs

    def _start_tag(self, inner):
        empty = inner.endswith('/')
        if empty:
            inner = inner[:-1]
        m = _NAME.match(inner)
        if m is None:
            self._fail('not well-formed (invalid token)')
        qname = m.group(0)
        if self._root_done and not self._stack:
            self._fail('junk after document element')
        attrs = self._parse_attrs(inner[m.end():])
        seen = set()
        for k, _ in attrs:
            if k in seen:
                self._fail('duplicate attribute')
            seen.add(k)
        self._flush_text()
        self._started = True
        declared = []
        scope = {}
        if self._sep is not None:
            rest = []
            for k, v in attrs:
                if k == 'xmlns':
                    scope[None] = v
                    declared.append((None, v))
                elif k.startswith('xmlns:'):
                    scope[k[6:]] = v
                    declared.append((k[6:], v))
                else:
                    rest.append((k, v))
            attrs = rest
            self._ns_stack.append(scope)
            if self.StartNamespaceDeclHandler is not None:
                for prefix, uri in declared:
                    self.StartNamespaceDeclHandler(prefix, uri or None)
        name = self._resolve(qname, False)
        resolved = [(self._resolve(k, True), v) for k, v in attrs]
        self._stack.append((qname, name, declared))
        if self.StartElementHandler is not None:
            if self.ordered_attributes:
                flat = []
                for k, v in resolved:
                    flat.append(k)
                    flat.append(v)
                self.StartElementHandler(name, flat)
            else:
                self.StartElementHandler(name, dict(resolved))
        if empty:
            self._end_tag(qname)

    def _end_tag(self, qname):
        if not self._stack:
            self._fail('junk after document element' if self._root_done else 'syntax error')
        open_q, name, declared = self._stack[-1]
        if qname != open_q:
            self._fail('mismatched tag', col=self._col + 2)
        self._flush_text()
        self._stack.pop()
        if self.EndElementHandler is not None:
            self.EndElementHandler(name)
        if self._sep is not None:
            self._ns_stack.pop()
            if self.EndNamespaceDeclHandler is not None:
                for prefix, _ in reversed(declared):
                    self.EndNamespaceDeclHandler(prefix)
        if not self._stack:
            self._root_done = True


def ParserCreate(encoding=None, namespace_separator=None, intern=None):
    if namespace_separator is not None and not isinstance(namespace_separator, str):
        raise TypeError('ParserCreate() argument 2 must be str or None, not %s' % type(namespace_separator).__name__)
    return XMLParserType(encoding, namespace_separator, intern)


def ErrorString(code):
    return _Errors.messages.get(code, 'unknown error')


class _ErrorsModule:
    codes = _Errors.codes
    messages = _Errors.messages


errors = _ErrorsModule
for _n in dir(_Errors):
    if _n.startswith('XML_ERROR_'):
        setattr(_ErrorsModule, _n, getattr(_Errors, _n))
