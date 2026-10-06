"""Laço de eventos "Unix" do sandbox: o `BaseEventLoop` sem I/O, e a política padrão que o `asyncio.run`
usa."""

from . import base_events
from . import events

__all__ = (
    'SelectorEventLoop', 'DefaultEventLoopPolicy', 'EventLoop',
)


class SelectorEventLoop(base_events.BaseEventLoop):
    """Laço de tarefas, tempo, executores e rede em loopback (sockets em processo, sem seletor)."""


class _UnixDefaultEventLoopPolicy(events.BaseDefaultEventLoopPolicy):
    """Política que cria `SelectorEventLoop`; sem observador de processos filhos."""
    _loop_factory = SelectorEventLoop


DefaultEventLoopPolicy = _UnixDefaultEventLoopPolicy
EventLoop = SelectorEventLoop
