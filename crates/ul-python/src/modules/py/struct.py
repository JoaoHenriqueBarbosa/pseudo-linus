"""struct: as funções vêm do módulo nativo `_struct`; `Struct` guarda o formato já validado."""

from _struct import error, pack, unpack, pack_into, unpack_from, calcsize
from _struct import iter_unpack as _iter_unpack

__all__ = ['calcsize', 'pack', 'pack_into', 'unpack', 'unpack_from', 'iter_unpack', 'Struct', 'error']


def iter_unpack(format, buffer, /):
    """Como no CPython, devolve um iterador (o nativo devolve a lista já decodificada)."""
    return iter(_iter_unpack(format, buffer))


def _clearcache():
    """Clear the internal cache."""
    return None


class Struct:
    __module__ = '_struct'

    def __init__(self, format):
        if isinstance(format, (bytes, bytearray)):
            format = bytes(format).decode('ascii')
        elif not isinstance(format, str):
            raise TypeError("Struct() argument 1 must be a str or bytes object, not %s" % type(format).__name__)
        self.format = format
        self.size = calcsize(format)

    def pack(self, *values):
        return pack(self.format, *values)

    def unpack(self, buffer):
        return unpack(self.format, buffer)

    def pack_into(self, buffer, offset, *values):
        return pack_into(self.format, buffer, offset, *values)

    def unpack_from(self, buffer, offset=0):
        return unpack_from(self.format, buffer, offset)

    def iter_unpack(self, buffer):
        return iter_unpack(self.format, buffer)

    def __repr__(self):
        return 'Struct(%r)' % (self.format,)
