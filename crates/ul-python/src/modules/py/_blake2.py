"""Hash functions (blake2b and blake2s). A shim sobre o `_hashimpl` nativo, com a superfície do `_blake2` do CPython."""

import _hashimpl
from _hashbase import _adopt, _input

BLAKE2B_MAX_DIGEST_SIZE = 64
BLAKE2B_MAX_KEY_SIZE = 64
BLAKE2B_PERSON_SIZE = 16
BLAKE2B_SALT_SIZE = 16
BLAKE2S_MAX_DIGEST_SIZE = 32
BLAKE2S_MAX_KEY_SIZE = 32
BLAKE2S_PERSON_SIZE = 8
BLAKE2S_SALT_SIZE = 8


def _make(self, kind, name, limits, data, digest_size, key, salt, person, fanout, depth, leaf_size, node_offset,
          node_depth, inner_size, last_node, string):
    """Confere os parâmetros com as mensagens do `_blake2module.c` e cria o objeto nativo."""
    max_digest, max_key, person_size, salt_size = limits
    if digest_size < 1 or digest_size > max_digest:
        raise ValueError("digest_size for %s must be between 1 and %d bytes, got %d" % (name, max_digest, digest_size))
    if len(key) > max_key:
        raise ValueError("maximum key length is %d bytes" % max_key)
    if len(salt) > salt_size:
        raise ValueError("maximum salt length is %d bytes" % salt_size)
    if len(person) > person_size:
        raise ValueError("maximum person length is %d bytes" % person_size)
    if fanout < 0 or fanout > 255:
        raise ValueError("fanout must be between 0 and 255")
    if depth <= 0 or depth > 255:
        raise ValueError("depth must be between 1 and 255")
    if leaf_size < 0 or leaf_size > 0xFFFFFFFF:
        raise OverflowError("leaf_size is too large")
    if node_offset < 0 or node_offset > (0xFFFFFFFFFFFFFFFF if kind == 'b' else 0xFFFFFFFFFFFF):
        raise OverflowError("node_offset is too large")
    if node_depth < 0 or node_depth > 255:
        raise ValueError("node_depth must be between 0 and 255")
    if inner_size < 0 or inner_size > max_digest:
        raise ValueError("inner_size must be between 0 and is %d" % max_digest)
    self._h = _hashimpl.blake2(kind, _input(data, string), digest_size, key, salt, person, fanout, depth, leaf_size,
                               node_offset, node_depth, inner_size, bool(last_node))


@_adopt
class blake2b:
    """Return a new BLAKE2b hash object."""

    MAX_DIGEST_SIZE = BLAKE2B_MAX_DIGEST_SIZE
    MAX_KEY_SIZE = BLAKE2B_MAX_KEY_SIZE
    PERSON_SIZE = BLAKE2B_PERSON_SIZE
    SALT_SIZE = BLAKE2B_SALT_SIZE

    def __init__(self, data=None, *, digest_size=BLAKE2B_MAX_DIGEST_SIZE, key=b'', salt=b'', person=b'', fanout=1,
                 depth=1, leaf_size=0, node_offset=0, node_depth=0, inner_size=0, last_node=False,
                 usedforsecurity=True, string=None):
        _make(self, 'b', 'blake2b', (64, 64, 16, 16), data, digest_size, key, salt, person, fanout, depth, leaf_size,
              node_offset, node_depth, inner_size, last_node, string)


@_adopt
class blake2s:
    """Return a new BLAKE2s hash object."""

    MAX_DIGEST_SIZE = BLAKE2S_MAX_DIGEST_SIZE
    MAX_KEY_SIZE = BLAKE2S_MAX_KEY_SIZE
    PERSON_SIZE = BLAKE2S_PERSON_SIZE
    SALT_SIZE = BLAKE2S_SALT_SIZE

    def __init__(self, data=None, *, digest_size=BLAKE2S_MAX_DIGEST_SIZE, key=b'', salt=b'', person=b'', fanout=1,
                 depth=1, leaf_size=0, node_offset=0, node_depth=0, inner_size=0, last_node=False,
                 usedforsecurity=True, string=None):
        _make(self, 's', 'blake2s', (32, 32, 8, 8), data, digest_size, key, salt, person, fanout, depth, leaf_size,
              node_offset, node_depth, inner_size, last_node, string)
