"""Cápsulas de ponteiro (`PyCapsule`) que módulos em C do CPython expõem, como `_socket.CAPI` e
`datetime.datetime_CAPI`: a API de C que o Python entrega a extensões; aqui só o objeto opaco."""


def make(name):
    """A cápsula `name`: um objeto opaco cujo `repr` mostra só o nome."""
    def capsule_repr(self):
        return '<capsule object "%s" at 0x%x>' % (name, id(self) & 0xffffffffffff)
    # O nome entra pelo `type()` porque atribuir `__name__` à classe não troca o nome do tipo.
    return type('PyCapsule', (), {'__repr__': capsule_repr, '__module__': 'builtins'})()
