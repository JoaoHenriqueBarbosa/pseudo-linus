"""O `greenlet._greenlet` do greenlet 3.5.6 (o `greenlet.cpp` e o `PyGreenlet.cpp`) sobre as threads verdes da VM.

Cada greenlet iniciado é um segmento da pilha explícita da VM (`_sys._gt_spawn`, `_sys._gt_switch`, `_sys._gt_finish`,
o mesmo mecanismo das threads de `threading`): trocar de greenlet é trocar de segmento, sem recursão Rust nem pilha de
C. O valor de cada troca passa pela caixa `_box`, porque o segmento novo começa sem receber valor.

Os greenlets pertencem à thread que os criou. A thread de `threading` que roda (`_gsched.current()`) guarda o
greenlet corrente dela (`glet`) e o gancho de `settrace` (`gtrace`); o `tid` dela acompanha o segmento do greenlet
que roda, para o escalonador das threads retomar a thread no greenlet certo.

O módulo é entregue ao programa com o nome `greenlet._greenlet`; este arquivo não existe para ele. Como no C, as
funções de nível de módulo são `builtin_function_or_method` (`C_EXTENSION_MODULES`) e o `greenlet.greenlet` é um tipo
estático (`C_EXTENSION_TYPES`, com as linhas das tabelas do oráculo): o estado de cada greenlet mora no campo
`_greenlet__st` do `__slots__`, fora do `__dict__`, e o `__del__` só existe para a finalização da VM.
"""

__name__ = 'greenlet._greenlet'

import sys as _sysmod
import _sys
import _gsched
from _contextvars import Context as _Context

CLOCKS_PER_SEC = 1000000
GREENLET_USE_GC = 1
GREENLET_USE_TRACING = 1
GREENLET_USE_CONTEXT_VARS = 1
GREENLET_USE_STANDARD_THREADING = 1


# Como o `PyErr_NewException("greenlet.error", ...)`: o dicionário só com `__module__`, sem o que o `class` acrescenta.
error = type('error', (Exception,), {'__module__': 'greenlet'})
GreenletExit = type('GreenletExit', (BaseException,), {'__module__': 'greenlet'})

# A caixa da troca: `('switch', args, kwargs)` ou `('throw', exceção)`, posta por quem troca e tirada por quem retoma.
_box = [None]
# Os greenlets que perderam a última referência suspensos: a morte deles (o `GreenletExit`) roda no ponto seguro.
_doomed = []
_getattr = object.__getattribute__
_setattr = object.__setattr__


class _State:
    """O estado de um greenlet, separado do objeto: o segmento suspenso o segura sem segurar o greenlet (o objeto só
    morre quando o programa solta a última referência, e aí o `__del__` mata o segmento)."""

    def __init__(self, owner, parent, main):
        self.owner = owner
        self.parent = parent
        self.main = main
        self.run = None
        self.tid = None
        self.started = main
        self.dead = False
        self.frame = None
        self.context = None


def _st(glet):
    """O estado de `glet`, lido como o C lê o campo da struct: sem passar por `__getattribute__` de subclasse."""
    return _getattr(glet, '_greenlet__st')


def _thread():
    return _gsched.current()


def _current():
    """O greenlet corrente da thread que roda; o principal dela nasce na primeira consulta."""
    green = _thread()
    glet = getattr(green, 'glet', None)
    if glet is None:
        glet = object.__new__(greenlet)
        state = _State(green, None, True)
        state.tid = green.tid
        _setattr(glet, '_greenlet__st', state)
        green.glet = glet
        green.gtrace = None
    return glet


def getcurrent():
    return _current()


def gettrace():
    _current()
    return _thread().gtrace


def settrace(callback):
    _current()
    green = _thread()
    previous = green.gtrace
    green.gtrace = None if callback is None else callback
    return previous


def _check(value):
    if not isinstance(value, greenlet):
        raise TypeError('GreenletChecker: Expected any type of greenlet, not %s' % type(value).__name__)


def _exited(owner):
    return owner is not _gsched.main and owner.thread is not None and not owner.thread.is_alive()


def _check_thread(target):
    owner = _st(target).owner
    if owner is not _thread():
        if _exited(owner):
            raise error('cannot switch to a different thread (which happens to have exited)')
        raise error('cannot switch to a different thread')


def _unpack(args, kwargs):
    if kwargs:
        return kwargs if not args else (args, kwargs)
    if len(args) == 1:
        return args[0]
    return args


def _exception(typ, val, tb):
    if isinstance(typ, type) and issubclass(typ, BaseException):
        if isinstance(val, typ):
            exc = val
        elif val is None:
            exc = typ()
        elif isinstance(val, tuple):
            exc = typ(*val)
        else:
            exc = typ(val)
    elif isinstance(typ, BaseException):
        if val is not None:
            raise TypeError('instance exception may not have a separate value')
        exc = typ
    else:
        raise TypeError('exceptions must be classes, or instances, not %s' % type(typ).__name__)
    if tb is not None:
        try:
            exc = exc.with_traceback(tb)
        except TypeError:
            raise TypeError('throw() third argument must be a traceback object') from None
    return exc


def _resolve(target):
    """O greenlet que de fato recebe a troca: um morto passa adiante ao primeiro ancestral vivo."""
    while _st(target).dead:
        target = _st(target).parent
    return target


def _trace(event, origin, target):
    tracer = _thread().gtrace
    if tracer is not None:
        tracer(event, (origin, target))


def _land():
    """Tira da caixa o que a troca trouxe e o entrega como o resultado do `switch` (ou levanta, num `throw`)."""
    payload = _box[0]
    _box[0] = None
    if payload[0] == 'throw':
        raise payload[1]
    return _unpack(payload[1], payload[2])


def _enter(target, payload, run, finishing):
    """Passa o controle do greenlet corrente a `target`; `run` é o corpo, quando `target` ainda não começou, e
    `finishing` diz que o segmento corrente acabou. Nenhum quadro daqui segura o greenlet que sai: suspenso, ele só
    vive enquanto o programa o segurar (e o `__del__` o mata quando soltar)."""
    state = _st(target)
    green = _thread()
    origin_state = _st(green.glet)
    if not _sys._gt_can_switch():
        # Dentro de um callback de nativa a VM não troca de segmento (`gthread.rs`): nada muda antes do erro.
        raise RuntimeError('cannot switch threads inside a callback of the interpreter')
    if not state.started:
        state.started = True
        state.tid = _sys._gt_spawn(lambda state=state, run=run: _entry(state, run))
    del run
    if not finishing:
        origin_state.frame = _sysmod._getframe(0)
    _box[0] = payload
    # O `ts->context` da thread troca de dono: o de quem sai fica guardado no greenlet dele, e o de quem entra (vazio,
    # num greenlet que ainda não rodou) passa a ser o corrente.
    origin_state.context = green.ctx
    green.ctx = state.context
    state.context = None
    green.glet = target
    green.tid = state.tid
    state.frame = None
    del target, payload
    if finishing:
        # O segmento corrente se descarta aqui: nada depois desta linha roda nele.
        _sys._gt_finish(state.tid)
    _sys._gt_switch(state.tid)
    origin_state.frame = None
    return _land()


def _switch_to(target, payload, event):
    _check_thread(target)
    target = _resolve(target)
    origin = _current()
    if target is origin:
        if payload[0] == 'throw':
            raise payload[1]
        return _unpack(payload[1], payload[2])
    # O `PyRequireAttr(self, "run")` do `g_initialstub`: o `run` de uma subclasse vence o campo do tipo.
    run = None if _st(target).started else getattr(target, 'run')
    _trace(event, origin, target)
    del origin
    return _enter(target, payload, run, False)


def _entry(state, run):
    """O corpo do segmento de um greenlet: roda o `run` e entrega o resultado (ou a exceção) ao pai."""
    payload = _box[0]
    _box[0] = None
    try:
        if payload[0] == 'throw':
            raise payload[1]
        result = ('switch', (run(*payload[1], **payload[2]),), {})
    except GreenletExit as exit_:
        result = ('switch', (exit_,), {})
    except BaseException as exc:
        result = ('throw', exc)
    del run, payload
    state.dead = True
    state.run = None
    state.frame = None
    me = _thread().glet
    target = _resolve(state.parent)
    while True:
        parent_state = _st(target)
        try:
            run = None if parent_state.started else getattr(target, 'run')
            _trace('switch' if result[0] == 'switch' else 'throw', me, target)
            break
        except BaseException as exc:
            # O pai não pôde receber (sem `run`): a falha sobe ao avô, como a propagação do C.
            result = ('throw', exc)
            parent_state.started = True
            parent_state.dead = True
            target = _resolve(parent_state.parent)
    del me
    _enter(target, result, run, True)


def _get_parent(glet):
    return _st(glet).parent


def _set_parent(glet, value):
    _check(value)
    state = _st(glet)
    if state.main:
        raise AttributeError('cannot set the parent of a main greenlet')
    probe = value
    while probe is not None:
        if probe is glet:
            raise ValueError('cyclic parent chain')
        probe = _st(probe).parent
    if _st(value).owner is not state.owner:
        raise ValueError('parent cannot be on a different thread')
    state.parent = value


def _no_delete_parent(glet):
    raise AttributeError("can't delete attribute")


def _get_run(glet):
    state = _st(glet)
    if state.main:
        raise AttributeError('Main greenlets do not have a run attribute.')
    if state.started or state.run is None:
        raise AttributeError('run')
    return state.run


def _set_run(glet, value):
    state = _st(glet)
    if state.started:
        raise AttributeError('run cannot be set after the start of the greenlet')
    state.run = value


def _del_run(glet):
    _set_run(glet, None)


class greenlet:
    """greenlet(run=None, parent=None) -> greenlet

Creates a new greenlet object (without running it).

 - *run* -- The callable to invoke.
 - *parent* -- The parent greenlet. The default is the current greenlet."""

    __slots__ = ('__st', '__dict__', '__weakref__')

    def __new__(cls, *args, **kwargs):
        self = object.__new__(cls)
        current = _current()
        _setattr(self, '_greenlet__st', _State(_thread(), current, False))
        return self

    def __init__(self, run=None, parent=None):
        if run is not None:
            _set_run(self, run)
        if parent is not None:
            _set_parent(self, parent)

    def switch(self, *args, **kwargs):
        """switch(*args, **kwargs)

Switch execution to this greenlet.

If this greenlet has never been run, then this greenlet
will be switched to using the body of ``self.run(*args, **kwargs)``.

If the greenlet is active (has been run, but was switch()'ed
out before leaving its run function), then this greenlet will
be resumed and the return value to its switch call will be
None if no arguments are given, the given argument if one
argument is given, or the args tuple and keyword args dict if
multiple arguments are given.

If the greenlet is dead, or is the current greenlet then this
function will simply return the arguments using the same rules as
above."""
        return _switch_to(self, ('switch', args, kwargs), 'switch')

    def throw(self, typ=GreenletExit, val=None, tb=None):
        """Switches execution to this greenlet, but immediately raises the
given exception in this greenlet.  If no argument is provided, the exception
defaults to `greenlet.GreenletExit`.  The normal exception
propagation rules apply, as described for `switch`.  Note that calling this
method is almost equivalent to the following::

    def raiser():
        raise typ, val, tb
    g_raiser = greenlet(raiser, parent=g)
    g_raiser.switch()

except that this trick does not work for the
`greenlet.GreenletExit` exception, which would not propagate
from ``g_raiser`` to ``g``.
"""
        return _switch_to(self, ('throw', _exception(typ, val, tb)), 'throw')

    def __bool__(self):
        state = _st(self)
        return state.started and not state.dead

    @property
    def dead(self):
        state = _st(self)
        return state.dead and not state.main

    @property
    def gr_frame(self):
        state = _st(self)
        if not state.started or state.dead or self is getattr(_thread(), 'glet', None):
            return None
        return state.frame

    @property
    def gr_context(self):
        # O do greenlet que roda é o contexto corrente da thread; o dos outros, o guardado na última troca.
        if self is getattr(_thread(), 'glet', None):
            return _thread().ctx
        return _st(self).context

    @gr_context.setter
    def gr_context(self, value):
        if value is not None and type(value) is not _Context:
            raise TypeError('greenlet context must be a contextvars.Context or None')
        if self is getattr(_thread(), 'glet', None):
            _thread().ctx = value
        else:
            _st(self).context = value

    parent = property(_get_parent, _set_parent, _no_delete_parent)
    run = property(_get_run, _set_run, _del_run)

    def __getstate__(self):
        raise TypeError("cannot serialize '%s' object" % _tp_name(self))

    def __repr__(self):
        state = _st(self)
        name = _tp_name(self)
        otid = id(state.owner) & 0xffffffffffff
        if state.dead and not state.main:
            return '<%s object at 0x%x (otid=0x%x) %sdead>' % (
                name, id(self) & 0xffffffffffff, otid, '(thread exited) ' if _exited(state.owner) else '')
        if _exited(state.owner):
            where = ' (thread exited)'
        elif self is getattr(_thread(), 'glet', None):
            where = ' current'
        else:
            where = ' suspended' if state.started else ''
        return '<%s object at 0x%x (otid=0x%x)%s%s%s%s>' % (
            name, id(self) & 0xffffffffffff, otid, where, ' active' if bool(self) else '',
            ' started' if state.started else ' pending', ' main' if state.main else '')

    def __del__(self):
        try:
            state = _st(self)
        except AttributeError:
            return
        if state.main or not state.started or state.dead:
            return
        if state.owner is not _thread():
            return
        _doomed.append(self)
        _gsched.at_safe_point(_reap)


def _reap():
    """A morte dos greenlets soltos (`del g` de um suspenso): o `GreenletExit` levantado neles, com o corrente como pai,
    como o `kill` do C no descarte. Um erro aqui vira `sys.unraisablehook`."""
    while _doomed:
        glet = _doomed.pop(0)
        state = _st(glet)
        if state.dead:
            continue
        state.parent = _current()
        try:
            _switch_to(glet, ('throw', GreenletExit()), 'throw')
        except BaseException as exc:
            import _unraisable
            _sysmod.unraisablehook(_unraisable.UnraisableHookArgs((type(exc), exc, exc.__traceback__, None, glet)))


greenlet.GreenletExit = GreenletExit
greenlet.error = error
# As mesmas funções de módulo, como no `tp_methods` com `METH_STATIC`: `greenlet.getcurrent is greenlet.greenlet.getcurrent`.
greenlet.getcurrent = _sys._builtin(getcurrent)
greenlet.gettrace = _sys._builtin(gettrace)
greenlet.settrace = _sys._builtin(settrace)
greenlet.__module__ = 'greenlet'


def _tp_name(glet):
    """O `tp_name` do tipo de `glet`: `greenlet.greenlet` no estático, o nome da classe numa subclasse."""
    return 'greenlet.greenlet' if type(glet) is greenlet else type(glet).__name__


class UnswitchableGreenlet(greenlet):
    force_switch_error = False
    force_slp_switch_error = False


def set_thread_local(key, value):
    import threading
    setattr(threading.local(), key, value)


def get_pending_cleanup_count():
    return 0


def get_total_main_greenlets():
    return sum(1 for green in [_gsched.main] if getattr(green, 'glet', None) is not None)


_cleanup = [False]


def enable_optional_cleanup(enabled):
    _cleanup[0] = bool(enabled)


def get_clocks_used_doing_optional_cleanup():
    return 0 if _cleanup[0] else None


import _capsule
_C_API = _capsule.make('greenlet._C_API')
del _capsule
