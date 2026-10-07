"""Apoio do `anext(iterador, padrão)` embutido."""


class anext_awaitable:
    # O objeto que o `anext` com valor padrão devolve (`anextawaitable_*` do CPython): aguarda o
    # `__anext__` do iterador e troca o `StopAsyncIteration` pelo padrão.
    __module__ = 'builtins'

    def __init__(self, awaitable, default):
        self._awaitable = awaitable
        self._default = default

    def __await__(self):
        awaitable = self._awaitable
        if not hasattr(awaitable, '__await__'):
            raise TypeError(f"'{type(awaitable).__name__}' object can't be awaited")
        try:
            return (yield from awaitable.__await__())
        except StopAsyncIteration:
            return self._default

    def __repr__(self):
        return f'<anext_awaitable object at {hex(id(self))}>'
