"""O argumento de `sys.unraisablehook`: o `UnraisableHookArgs` do CPython, um tipo que não é atributo de `sys`."""


class UnraisableHookArgs(tuple):
    _fields = ('exc_type', 'exc_value', 'exc_traceback', 'err_msg', 'object')
    n_fields = 5
    n_sequence_fields = 5
    n_unnamed_fields = 0

    exc_type = property(lambda self: self[0])
    exc_value = property(lambda self: self[1])
    exc_traceback = property(lambda self: self[2])
    err_msg = property(lambda self: self[3])
    object = property(lambda self: self[4])

    def __repr__(self):
        return 'UnraisableHookArgs(%s)' % ', '.join('%s=%r' % kv for kv in zip(self._fields, self))


UnraisableHookArgs.__module__ = 'sys'
