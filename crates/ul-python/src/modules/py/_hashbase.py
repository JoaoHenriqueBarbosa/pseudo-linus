"""Base dos objetos de resumo escritos em Python (`_blake2`, `_md5`, `_sha1`, `_sha2`, `_sha3`): cada um guarda o
objeto nativo do `_hashimpl` e repassa a ele. É módulo de apoio: só código embutido o importa."""


def _input(data, string):
    """O dado inicial: `data` ou o `string` de palavra-chave (os dois juntos são recusados, como no CPython)."""
    if string is not None:
        if data is not None and data != b'':
            raise TypeError("'data' and 'string' are mutually exclusive "
                            "and support for 'string' keyword parameter is slated for removal in a future version.")
        return string
    return b'' if data is None else data


class _Hash:
    """Os métodos comuns dos objetos de resumo."""

    def update(self, data, /):
        """Update this hash object's state with the provided string."""
        self._h.update(data)

    def digest(self):
        """Return the digest value as a bytes object."""
        return self._h.digest()

    def hexdigest(self):
        """Return the digest value as a string of hexadecimal digits."""
        return self._h.hexdigest()

    def copy(self):
        """Return a copy of the hash object."""
        other = self.__class__.__new__(self.__class__)
        other._h = self._h.copy()
        return other

    @property
    def name(self):
        return self._h.name

    @property
    def digest_size(self):
        return self._h.digest_size

    @property
    def block_size(self):
        return self._h.block_size


def _adopt(cls):
    """Decorador: copia os métodos de `_Hash` que a classe não define, sem pôr `_Hash` na herança (o `__mro__` do
    tipo de C é só `(tipo, object)`)."""
    for key in ('update', 'digest', 'hexdigest', 'copy', 'name', 'digest_size', 'block_size'):
        if key not in cls.__dict__:
            setattr(cls, key, _Hash.__dict__[key])
    return cls
