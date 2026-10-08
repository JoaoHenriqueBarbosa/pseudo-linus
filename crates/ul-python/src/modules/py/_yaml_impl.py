"""Camada em Python do binding `yaml._yaml` (o `_yaml.pyx` do PyYAML 6.0.3 sobre o libyaml 0.2.5).

O scanner, o parser e o emissor são o `_yaml_core`, em Rust: os objetos dele devolvem tuplas simples.
Aqui ficam as classes `Mark`, `CParser` e `CEmitter`, que montam os objetos de token, evento e nó do
pacote `yaml` e fazem a composição e a serialização de nós como o código Cython faz. O módulo é
entregue ao programa com o nome `yaml._yaml`; `_yaml_core` e este arquivo não existem para ele.
"""

__name__ = 'yaml._yaml'

import _yaml_core
from yaml.error import YAMLError
from yaml.reader import ReaderError
from yaml.scanner import ScannerError
from yaml.parser import ParserError
from yaml.composer import ComposerError
from yaml.constructor import ConstructorError
from yaml.emitter import EmitterError
from yaml.serializer import SerializerError
from yaml.representer import RepresenterError
from yaml.tokens import (
    DirectiveToken, DocumentStartToken, DocumentEndToken, StreamStartToken, StreamEndToken,
    BlockSequenceStartToken, BlockMappingStartToken, BlockEndToken, FlowSequenceStartToken,
    FlowMappingStartToken, FlowSequenceEndToken, FlowMappingEndToken, KeyToken, ValueToken,
    BlockEntryToken, FlowEntryToken, AliasToken, AnchorToken, TagToken, ScalarToken)
from yaml.events import (
    StreamStartEvent, StreamEndEvent, DocumentStartEvent, DocumentEndEvent, AliasEvent,
    ScalarEvent, SequenceStartEvent, SequenceEndEvent, MappingStartEvent, MappingEndEvent)
from yaml.nodes import ScalarNode, SequenceNode, MappingNode


def get_version_string():
    return '0.2.5'


def get_version():
    return (0, 2, 5)


class Mark:

    def __init__(self, name, index, line, column, buffer, pointer):
        self._name = name
        self._index = index
        self._line = line
        self._column = column
        self._buffer = buffer
        self._pointer = pointer

    name = property(lambda self: self._name)
    index = property(lambda self: self._index)
    line = property(lambda self: self._line)
    column = property(lambda self: self._column)
    buffer = property(lambda self: self._buffer)
    pointer = property(lambda self: self._pointer)

    def get_snippet(self):
        return None

    def __str__(self):
        where = "  in \"%s\", line %d, column %d" % (self._name, self._line + 1, self._column + 1)
        return where


# Tokens sem dados, pelo código que o `_yaml_core` usa.
_SIMPLE_TOKENS = {
    5: DocumentStartToken, 6: DocumentEndToken, 7: BlockSequenceStartToken,
    8: BlockMappingStartToken, 9: BlockEndToken, 10: FlowSequenceStartToken,
    11: FlowSequenceEndToken, 12: FlowMappingStartToken, 13: FlowMappingEndToken,
    14: BlockEntryToken, 15: FlowEntryToken, 16: KeyToken, 17: ValueToken,
}

_STREAM_START = 1
_STREAM_END = 2
_DOCUMENT_START = 3
_DOCUMENT_END = 4
_ALIAS = 5
_SCALAR = 6
_SEQUENCE_START = 7
_SEQUENCE_END = 8
_MAPPING_START = 9
_MAPPING_END = 10


class CParser:

    def __init__(self, stream):
        if hasattr(stream, 'read'):
            self._stream = stream
            try:
                self._stream_name = stream.name
            except AttributeError:
                self._stream_name = '<file>'
            self._h = _yaml_core.parser_from_reader(stream.read)
        else:
            unicode_source = False
            if type(stream) is str:
                stream = stream.encode('utf-8')
                self._stream_name = '<unicode string>'
                unicode_source = True
            else:
                self._stream_name = '<byte string>'
            if type(stream) is not bytes:
                raise TypeError("a string or stream input is required")
            self._stream = stream
            self._h = _yaml_core.parser_from_bytes(stream, unicode_source)
        self._current_token = None
        self._current_event = None
        self._anchors = {}
        self._parsed_event = None

    def dispose(self):
        pass

    def _mark(self, position):
        return Mark(self._stream_name, position[0], position[1], position[2], None, None)

    def _raise_error(self, r):
        kind = r[1]
        if kind == 'reader':
            raise ReaderError(self._stream_name, r[3], r[4], '?', r[2])
        context = r[2]
        context_mark = None
        if context is not None:
            context_mark = self._mark(r[3])
        problem_mark = self._mark(r[5])
        if kind == 'scanner':
            raise ScannerError(context, context_mark, r[4], problem_mark)
        raise ParserError(context, context_mark, r[4], problem_mark)

    def _scan(self):
        r = self._h.scan()
        if r is None:
            return None
        if r[0] < 0:
            self._raise_error(r)
        return self._token_to_object(r)

    def _token_to_object(self, r):
        kind = r[0]
        start_mark = self._mark(r[1])
        end_mark = self._mark(r[2])
        if kind == 1:
            return StreamStartToken(start_mark, end_mark, r[3])
        if kind == 2:
            return StreamEndToken(start_mark, end_mark)
        if kind == 3:
            return DirectiveToken('YAML', (r[3], r[4]), start_mark, end_mark)
        if kind == 4:
            return DirectiveToken('TAG', (r[3], r[4]), start_mark, end_mark)
        if kind in _SIMPLE_TOKENS:
            return _SIMPLE_TOKENS[kind](start_mark, end_mark)
        if kind == 18:
            return AliasToken(r[3], start_mark, end_mark)
        if kind == 19:
            return AnchorToken(r[3], start_mark, end_mark)
        if kind == 20:
            handle = r[3]
            if not handle:
                handle = None
            return TagToken((handle, r[4]), start_mark, end_mark)
        if kind == 21:
            style = r[4]
            return ScalarToken(r[3], style == '', start_mark, end_mark, style)
        raise ValueError("unknown token type")

    def get_token(self):
        if self._current_token is not None:
            value = self._current_token
            self._current_token = None
        else:
            value = self._scan()
        return value

    def peek_token(self):
        if self._current_token is None:
            self._current_token = self._scan()
        return self._current_token

    def check_token(self, *choices):
        if self._current_token is None:
            self._current_token = self._scan()
        if self._current_token is None:
            return False
        if not choices:
            return True
        token_class = self._current_token.__class__
        for choice in choices:
            if token_class is choice:
                return True
        return False

    def _parse(self):
        r = self._h.parse()
        if r is None:
            return None
        if r[0] < 0:
            self._raise_error(r)
        return self._event_to_object(r)

    def _event_to_object(self, r):
        kind = r[0]
        start_mark = self._mark(r[1])
        end_mark = self._mark(r[2])
        if kind == _STREAM_START:
            return StreamStartEvent(start_mark, end_mark, r[3])
        if kind == _STREAM_END:
            return StreamEndEvent(start_mark, end_mark)
        if kind == _DOCUMENT_START:
            tags = None
            if r[4] is not None:
                tags = {}
                for handle, prefix in r[4]:
                    tags[handle] = prefix
            return DocumentStartEvent(start_mark, end_mark, r[5], r[3], tags)
        if kind == _DOCUMENT_END:
            return DocumentEndEvent(start_mark, end_mark, r[3])
        if kind == _ALIAS:
            return AliasEvent(r[3], start_mark, end_mark)
        if kind == _SCALAR:
            return ScalarEvent(r[3], r[4], (r[6], r[7]), r[5], start_mark, end_mark, r[8])
        if kind == _SEQUENCE_START:
            return SequenceStartEvent(r[3], r[4], r[5], start_mark, end_mark, flow_style=r[6])
        if kind == _SEQUENCE_END:
            return SequenceEndEvent(start_mark, end_mark)
        if kind == _MAPPING_START:
            return MappingStartEvent(r[3], r[4], r[5], start_mark, end_mark, flow_style=r[6])
        if kind == _MAPPING_END:
            return MappingEndEvent(start_mark, end_mark)
        raise ValueError("unknown event type")

    def get_event(self):
        if self._current_event is not None:
            value = self._current_event
            self._current_event = None
        else:
            value = self._parse()
        return value

    def peek_event(self):
        if self._current_event is None:
            self._current_event = self._parse()
        return self._current_event

    def check_event(self, *choices):
        if self._current_event is None:
            self._current_event = self._parse()
        if self._current_event is None:
            return False
        if not choices:
            return True
        event_class = self._current_event.__class__
        for choice in choices:
            if event_class is choice:
                return True
        return False

    # Composição: trabalha direto sobre as tuplas cruas do `_yaml_core`, como o Cython sobre
    # o `yaml_event_t`.

    def _parse_next_event(self):
        if self._parsed_event is None:
            r = self._h.parse()
            if r is not None and r[0] < 0:
                self._raise_error(r)
            self._parsed_event = r

    def _parsed_kind(self):
        if self._parsed_event is None:
            return 0
        return self._parsed_event[0]

    def check_node(self):
        self._parse_next_event()
        if self._parsed_kind() == _STREAM_START:
            self._parsed_event = None
            self._parse_next_event()
        if self._parsed_kind() != _STREAM_END:
            return True
        return False

    def get_node(self):
        self._parse_next_event()
        if self._parsed_kind() != _STREAM_END:
            return self._compose_document()

    def get_single_node(self):
        self._parse_next_event()
        self._parsed_event = None
        self._parse_next_event()
        document = None
        if self._parsed_kind() != _STREAM_END:
            document = self._compose_document()
        self._parse_next_event()
        if self._parsed_kind() != _STREAM_END:
            mark = self._mark(self._parsed_event[1])
            raise ComposerError("expected a single document in the stream",
                    document.start_mark, "but found another document", mark)
        return document

    def _compose_document(self):
        self._parsed_event = None
        node = self._compose_node(None, None)
        self._parse_next_event()
        self._parsed_event = None
        self._anchors = {}
        return node

    def _compose_node(self, parent, index):
        self._parse_next_event()
        r = self._parsed_event
        kind = self._parsed_kind()
        if kind == _ALIAS:
            anchor = r[3]
            if anchor not in self._anchors:
                mark = self._mark(r[1])
                raise ComposerError(None, None, "found undefined alias", mark)
            self._parsed_event = None
            return self._anchors[anchor]
        anchor = None
        if kind == _SCALAR or kind == _SEQUENCE_START or kind == _MAPPING_START:
            anchor = r[3]
        if anchor is not None:
            if anchor in self._anchors:
                mark = self._mark(r[1])
                raise ComposerError("found duplicate anchor; first occurrence",
                        self._anchors[anchor].start_mark, "second occurrence", mark)
        self.descend_resolver(parent, index)
        if kind == _SCALAR:
            node = self._compose_scalar_node(anchor)
        elif kind == _SEQUENCE_START:
            node = self._compose_sequence_node(anchor)
        elif kind == _MAPPING_START:
            node = self._compose_mapping_node(anchor)
        self.ascend_resolver()
        return node

    def _compose_scalar_node(self, anchor):
        r = self._parsed_event
        start_mark = self._mark(r[1])
        end_mark = self._mark(r[2])
        value = r[5]
        plain_implicit = r[6]
        quoted_implicit = r[7]
        if r[4] is None or r[4] == '!':
            tag = self.resolve(ScalarNode, value, (plain_implicit, quoted_implicit))
        else:
            tag = r[4]
        node = ScalarNode(tag, value, start_mark, end_mark, r[8])
        if anchor is not None:
            self._anchors[anchor] = node
        self._parsed_event = None
        return node

    def _compose_sequence_node(self, anchor):
        r = self._parsed_event
        start_mark = self._mark(r[1])
        if r[4] is None or r[4] == '!':
            tag = self.resolve(SequenceNode, None, r[5])
        else:
            tag = r[4]
        value = []
        node = SequenceNode(tag, value, start_mark, None, r[6])
        if anchor is not None:
            self._anchors[anchor] = node
        self._parsed_event = None
        index = 0
        self._parse_next_event()
        while self._parsed_kind() != _SEQUENCE_END:
            value.append(self._compose_node(node, index))
            index = index + 1
            self._parse_next_event()
        node.end_mark = self._mark(self._parsed_event[2])
        self._parsed_event = None
        return node

    def _compose_mapping_node(self, anchor):
        r = self._parsed_event
        start_mark = self._mark(r[1])
        if r[4] is None or r[4] == '!':
            tag = self.resolve(MappingNode, None, r[5])
        else:
            tag = r[4]
        value = []
        node = MappingNode(tag, value, start_mark, None, r[6])
        if anchor is not None:
            self._anchors[anchor] = node
        self._parsed_event = None
        self._parse_next_event()
        while self._parsed_kind() != _MAPPING_END:
            item_key = self._compose_node(node, None)
            item_value = self._compose_node(node, item_key)
            value.append((item_key, item_value))
            self._parse_next_event()
        node.end_mark = self._mark(self._parsed_event[2])
        self._parsed_event = None
        return node

    def raw_parse(self):
        count = 0
        while True:
            r = self._h.parse()
            if r is None:
                break
            if r[0] < 0:
                self._raise_error(r)
            count = count + 1
        return count

    def raw_scan(self):
        count = 0
        while True:
            r = self._h.scan()
            if r is None:
                break
            if r[0] < 0:
                self._raise_error(r)
            count = count + 1
        return count


def _text(value, message):
    # `str` vira UTF-8; `bytes` passa como está; o resto é erro.
    if type(value) is str:
        return value
    if type(value) is bytes:
        return value.decode('utf-8', 'replace')
    raise TypeError(message)


def _optional_text(value, message):
    if value is None:
        return None
    return _text(value, message)


def _unpack_pair(item):
    # O `for chave, valor in ...` do Cython: os erros de desempacotamento têm as mensagens dele,
    # não as do CPython.
    if type(item) is tuple or type(item) is list:
        count = len(item)
        if count > 2:
            raise ValueError("too many values to unpack (expected 2)")
        if count < 2:
            raise ValueError("need more than %d value%s to unpack" % (count, '' if count == 1 else 's'))
        return item[0], item[1]
    iterator = iter(item)
    missing = object()
    first = next(iterator, missing)
    if first is missing:
        raise ValueError("need more than 0 values to unpack")
    second = next(iterator, missing)
    if second is missing:
        raise ValueError("need more than 1 value to unpack")
    if next(iterator, missing) is not missing:
        raise ValueError("too many values to unpack (expected 2)")
    return first, second


def _scalar_style(style):
    if style == "'" or style == "\"" or style == "|" or style == ">":
        return style
    return ''


class CEmitter:

    def __init__(self, stream, canonical=None, indent=None, width=None,
            allow_unicode=None, line_break=None, encoding=None,
            explicit_start=None, explicit_end=None, version=None, tags=None):
        self._h = _yaml_core.emitter(bool(canonical), indent, width, bool(allow_unicode), line_break)
        self._stream = stream
        self._dump_unicode = 0
        if hasattr(stream, 'encoding'):
            self._dump_unicode = 1
        self._use_encoding = encoding
        self._document_start_implicit = 1
        if explicit_start:
            self._document_start_implicit = 0
        self._document_end_implicit = 1
        if explicit_end:
            self._document_end_implicit = 0
        self._use_version = version
        self._use_tags = tags
        self._serialized_nodes = {}
        self._anchors = {}
        self._last_alias_id = 0
        self._closed = -1

    def dispose(self):
        pass

    def _write(self, chunks):
        for chunk in chunks:
            if self._dump_unicode == 0:
                value = chunk
            else:
                value = chunk.decode('utf-8')
            self._stream.write(value)

    def _emit_tuple(self, event):
        problem, chunks = self._h.emit(event)
        self._write(chunks)
        if problem is not None:
            raise EmitterError(problem)

    def _tag_directives(self, tags):
        if len(tags) > 128:
            raise ValueError("too many tags")
        result = []
        for handle in tags:
            prefix = tags[handle]
            result.append((_text(handle, "tag handle must be a string"),
                           _text(prefix, "tag prefix must be a string")))
        return result

    def _object_to_event(self, event_object):
        event_class = event_object.__class__
        if event_class is StreamStartEvent:
            return (_STREAM_START, event_object.encoding)
        if event_class is StreamEndEvent:
            return (_STREAM_END,)
        if event_class is DocumentStartEvent:
            version = None
            if event_object.version:
                version = (event_object.version[0], event_object.version[1])
            tags = None
            if event_object.tags:
                tags = self._tag_directives(event_object.tags)
            implicit = True
            if event_object.explicit:
                implicit = False
            return (_DOCUMENT_START, version, tags, implicit)
        if event_class is DocumentEndEvent:
            implicit = True
            if event_object.explicit:
                implicit = False
            return (_DOCUMENT_END, implicit)
        if event_class is AliasEvent:
            return (_ALIAS, _text(event_object.anchor, "anchor must be a string"))
        if event_class is ScalarEvent:
            anchor = _optional_text(event_object.anchor, "anchor must be a string")
            tag = _optional_text(event_object.tag, "tag must be a string")
            value = _text(event_object.value, "value must be a string")
            plain_implicit = False
            quoted_implicit = False
            if event_object.implicit is not None:
                plain_implicit = bool(event_object.implicit[0])
                quoted_implicit = bool(event_object.implicit[1])
            return (_SCALAR, anchor, tag, value, plain_implicit, quoted_implicit,
                    _scalar_style(event_object.style))
        if event_class is SequenceStartEvent or event_class is MappingStartEvent:
            anchor = _optional_text(event_object.anchor, "anchor must be a string")
            tag = _optional_text(event_object.tag, "tag must be a string")
            implicit = False
            if event_object.implicit:
                implicit = True
            flow = False
            if event_object.flow_style:
                flow = True
            kind = _SEQUENCE_START
            if event_class is MappingStartEvent:
                kind = _MAPPING_START
            return (kind, anchor, tag, implicit, flow)
        if event_class is SequenceEndEvent:
            return (_SEQUENCE_END,)
        if event_class is MappingEndEvent:
            return (_MAPPING_END,)
        raise TypeError("invalid event %s" % event_object)

    def emit(self, event_object):
        self._emit_tuple(self._object_to_event(event_object))

    def open(self):
        if self._closed == -1:
            encoding = 'utf-8'
            if self._use_encoding == 'utf-16-le':
                encoding = 'utf-16-le'
            elif self._use_encoding == 'utf-16-be':
                encoding = 'utf-16-be'
            if self._use_encoding is None:
                self._dump_unicode = 1
            if self._dump_unicode == 1:
                encoding = 'utf-8'
            self._emit_tuple((_STREAM_START, encoding))
            self._closed = 0
        elif self._closed == 1:
            raise SerializerError("serializer is closed")
        else:
            raise SerializerError("serializer is already opened")

    def close(self):
        if self._closed == -1:
            raise SerializerError("serializer is not opened")
        elif self._closed == 0:
            self._emit_tuple((_STREAM_END,))
            self._closed = 1

    def serialize(self, node):
        if self._closed == -1:
            raise SerializerError("serializer is not opened")
        elif self._closed == 1:
            raise SerializerError("serializer is closed")
        version = None
        if self._use_version:
            version = (self._use_version[0], self._use_version[1])
        tags = None
        if self._use_tags:
            tags = self._tag_directives(self._use_tags)
        self._emit_tuple((_DOCUMENT_START, version, tags, bool(self._document_start_implicit)))
        self._anchor_node(node)
        self._serialize_node(node, None, None)
        self._emit_tuple((_DOCUMENT_END, bool(self._document_end_implicit)))
        self._serialized_nodes = {}
        self._anchors = {}
        self._last_alias_id = 0

    def _anchor_node(self, node):
        if node in self._anchors:
            if self._anchors[node] is None:
                self._last_alias_id = self._last_alias_id + 1
                self._anchors[node] = "id%03d" % self._last_alias_id
        else:
            self._anchors[node] = None
            node_class = node.__class__
            if node_class is SequenceNode:
                for item in node.value:
                    self._anchor_node(item)
            elif node_class is MappingNode:
                for item in node.value:
                    key, value = _unpack_pair(item)
                    self._anchor_node(key)
                    self._anchor_node(value)

    def _serialize_node(self, node, parent, index):
        anchor = _optional_text(self._anchors[node], "anchor must be a string")
        if node in self._serialized_nodes:
            self._emit_tuple((_ALIAS, anchor))
            return
        node_class = node.__class__
        self._serialized_nodes[node] = True
        self.descend_resolver(parent, index)
        if node_class is ScalarNode:
            plain_implicit = False
            quoted_implicit = False
            tag_object = node.tag
            if self.resolve(ScalarNode, node.value, (True, False)) == tag_object:
                plain_implicit = True
            if self.resolve(ScalarNode, node.value, (False, True)) == tag_object:
                quoted_implicit = True
            tag = _optional_text(tag_object, "tag must be a string")
            value = _text(node.value, "value must be a string")
            self._emit_tuple((_SCALAR, anchor, tag, value, plain_implicit, quoted_implicit,
                              _scalar_style(node.style)))
        elif node_class is SequenceNode:
            implicit = False
            tag_object = node.tag
            if self.resolve(SequenceNode, node.value, True) == tag_object:
                implicit = True
            tag = _optional_text(tag_object, "tag must be a string")
            flow = False
            if node.flow_style:
                flow = True
            self._emit_tuple((_SEQUENCE_START, anchor, tag, implicit, flow))
            item_index = 0
            for item in node.value:
                self._serialize_node(item, node, item_index)
                item_index = item_index + 1
            self._emit_tuple((_SEQUENCE_END,))
        elif node_class is MappingNode:
            implicit = False
            tag_object = node.tag
            if self.resolve(MappingNode, node.value, True) == tag_object:
                implicit = True
            tag = _optional_text(tag_object, "tag must be a string")
            flow = False
            if node.flow_style:
                flow = True
            self._emit_tuple((_MAPPING_START, anchor, tag, implicit, flow))
            for item in node.value:
                item_key, item_value = _unpack_pair(item)
                self._serialize_node(item_key, node, None)
                self._serialize_node(item_value, node, item_key)
            self._emit_tuple((_MAPPING_END,))
        self.ascend_resolver()
