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


# O `run` que nunca foi posto (o `run=None` explícito é um `run` que vale `None`, e chamá-lo é `TypeError`).
class _NoRun:
    pass


_NO_RUN = _NoRun()


class _State:
    """O estado de um greenlet, separado do objeto: o segmento suspenso o segura sem segurar o greenlet (o objeto só
    morre quando o programa solta a última referência, e aí o `__del__` mata o segmento)."""

    def __init__(self, owner, parent, main):
        self.owner = owner
        self.parent = parent
        self.main = main
        self.call_run = False
        self.run = _NO_RUN
        self.tid = None
        self.started = main
        self.dead = False
        self.frame = None
        self.context = None
        # O `_stack_saved` (ver `_save_stacks`): onde a pilha do greenlet começa e onde ela parou, em bytes de pilha de
        # máquina contados do início da do principal, o que o corpo dele gasta antes do código do usuário, e quanto
        # dela já foi copiado para o heap.
        self.start = 0
        self.base = 0
        self.sp = None
        self.saved = 0


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
    # Na limpeza final do interpretador (o `__del__` de um ciclo que só a finalização coleta) o greenlet principal
    # já se foi: o C responde com um erro em vez de devolver um greenlet.
    if _sysmod.is_finalizing():
        raise RuntimeError('greenlet is being finalized')
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


# O `_stack_saved` conta bytes da pilha de máquina que o greenlet do C copiou para o heap. Aqui não há pilha de máquina:
# os números são os do CPython 3.13 com o greenlet 3.5.6 da wheel no oráculo, e cada um vale um caminho de C.
# O que o C empilha para chegar a um quadro Python chamado por nativa ou por slot (`_sys._gt_c_frames` dá o nome de
# cada um que espera no segmento): a chamada Python comum não gasta pilha de máquina no 3.13.
_C_FRAME_BYTES = {
    # slots dos operadores e das instruções
    'binary:__add__': 656, 'binary:__radd__': 656, 'binary:__sub__': 656, 'binary:__mul__': 656,
    'binary_inplace:__iadd__': 608, 'push:__neg__': 576, 'push:__invert__': 576, 'push:__getitem__': 576,
    'push:__enter__': 592, 'discard:__setitem__': 592, 'discard:__delitem__': 592,
    'equal:__eq__': 592, 'equal:__ne__': 592, 'order:__lt__': 592, 'order:__ge__': 592,
    'boolean:__contains__': 528, 'truth:__bool__': 544, 'iter:__iter__': 544, 'for_next:__next__': 576, 'next:__next__': 704,
    'init:__init__': 672, 'call:__call__': 736, 'attr:__getattr__': 656, 'attr': 624, 'exec:': 928,
    'builtin:__abs__': 656, 'builtin:__bool__': 624, 'builtin:__float__': 656, 'builtin:__int__': 720,
    'builtin:__len__': 720, 'builtin:__round__': 688, 'text:__str__': 704, 'text:__repr__': 704,
    'scan_list_in:__eq__': 608, 'scan_tuple_in:__eq__': 608, 'scan_list_count:__eq__': 768,
    'scan_list_index:__eq__': 784, 'scan_list_remove:__eq__': 688,
    # nativas repetidas
    'replay_call_ascii:__repr__': 736, 'replay_call_bin:__index__': 704, 'replay_call_hex:__index__': 704,
    'replay_call_oct:__index__': 704, 'replay_call_range:__index__': 672, 'replay_call_divmod:__divmod__': 720,
    'replay_call_format:__format__': 736, 'replay_call_format:__str__': 896, 'replay_call_print:__str__': 912,
    'replay_call_getattr:__getattr__': 768, 'replay_call_hasattr:__getattr__': 768, 'replay_call_hash:__hash__': 640,
    'replay_call_dict.get:__hash__': 656, 'replay_call_dict.pop:__hash__': 672,
    'replay_call_dict.setdefault:__hash__': 736, 'replay_call_dict.__setitem__:__hash__': 768,
    'replay_call_dict.__delitem__:__hash__': 800, 'replay_call_dict:__hash__': 672,
    'replay_call_set.add:__hash__': 704, 'replay_call_set.discard:__hash__': 656,
    'replay_call_set.remove:__hash__': 672, 'replay_call_set.union:__hash__': 896, 'replay_call_set:__hash__': 848,
    'replay_call_frozenset:__hash__': 848, 'replay_call_sorted:__lt__': 5152, 'replay_call_min:__lt__': 720,
    'replay_call_max:__lt__': 720, 'replay_call_sum:__radd__': 848,
    'replay_call_str.format_map:__getitem__': 1328, 'replay_call_str.format:__format__': 1520, 'replay_call_str.format:__str__': 1680,
    'replay_step_binary_str_obj:__str__': 1040, 'replay_step_binary_str_obj:__repr__': 992,
    'replay_step_binary_str_obj:__int__': 1056, 'replay_step_build_dict_obj_int:__hash__': 560,
    'replay_step_build_set_obj:__hash__': 624, 'replay_step_compare_obj_dict:__hash__': 560,
    'replay_step_compare_obj_set:__hash__': 576, 'replay_step_compare_list_list:__eq__': 672,
    'replay_step_compare_tuple_tuple:__eq__': 704, 'replay_step_format_value_obj:__str__': 816,
    'replay_step_format_value_obj:__format__': 656, 'replay_step_format_value_obj:__repr__': 624,
    'replay_step_format_value_obj_str:__format__': 656, 'replay_step_subscript_dict_obj:__hash__': 560,
    'replay_step_subscript_list_obj:__index__': 688, 'replay_step_subscript_str_slice:__index__': 656,
    'replay_step_store_subscript_int_dict_obj:__hash__': 560,
    # callbacks das nativas (o método é do usuário: vale o caminho)
    'callback_advance_collect_list': 800, 'callback_advance_fold_set': 912, 'callback_advance_fold_sum': 768,
    'callback_advance_collect_list:__getitem__': 896, 'callback_kept_collect_list': 736, 'callback_kept_for': 512,
    'callback_keyed_fold_max': 656, 'callback_keyed_fold_min': 656, 'callback_keyed_fold_sorted': 5040,
    'callback_keyed_fold_list_sort': 4928, 'callback_mapped_collect_list': 768, 'callback_mapped_for': 544,
    'callback_start_collect_list': 768, 'callback_start_collect_tuple': 656,
    # geradores retomados por quem os consome
    'resume_next:': 544, 'resume_for:': 448, 'resume_delegate:': 0, 'resume_collect_list:': 672,
    'resume_collect_list_enumerate:': 752, 'resume_collect_list_map:': 800, 'resume_collect_list_zip:': 720,
    'resume_collect_native:': 576, 'resume_collect_tuple:': 560, 'resume_collect_str.join:': 736,
    'resume_fold_all:': 560, 'resume_fold_any:': 624, 'resume_fold_dict:': 608, 'resume_fold_extend:': 672,
    'resume_fold_min:': 640, 'resume_fold_set:': 784, 'resume_fold_sorted:': 736, 'resume_fold_sum:': 640,
    # funções embutidas que no CPython são C (o que elas chamam vem por cima)
    'pyc_reduce:reduce': 592, 'pyc_setitem:setitem': 96, 'pyc_starmap.__next__:__next__': 144,
}
# Os quadros deste módulo são o C do próprio greenlet, já contados nas constantes do corpo e da troca.
_OWN_FRAMES = frozenset(('pyc_greenlet.switch:switch', 'pyc_greenlet.throw:throw', 'pyc__switch_to:_switch_to',
                         'pyc__enter:_enter', 'pyc__enter.<locals>.<lambda>:<lambda>', 'pyc__entry:_entry'))
# Caminho fora da tabela: a mediana dos medidos da mesma família (o começo do nome), e, sem família, a de todos.
_C_FAMILY = ('replay_step_', 'replay_call_', 'callback_', 'resume_', 'scan_', 'pyc_')


def _c_frame_bytes(name):
    got = _C_FRAME_BYTES.get(name)
    if got is None:
        got = _C_FRAME_BYTES.get(name.rpartition(':')[0])
    if got is None:
        family = next((f for f in _C_FAMILY if name.startswith(f)), '')
        known = sorted(v for k, v in _C_FRAME_BYTES.items() if k.startswith(family))
        got = known[len(known) // 2]
    return got


# O corpo do greenlet até o código do usuário (`g_initialstub` e a chamada do `run`): função Python, método ligado,
# objeto chamável e `functools.partial`; com argumentos nomeados no primeiro `switch` o C monta o dicionário.
_RUN_FUNCTION = 1512
_RUN_METHOD = 1656
_RUN_CALLABLE = 1800
_RUN_PARTIAL = 1544
_RUN_GREENLET_SWITCH = 984
_RUN_KWARGS = 96
# Onde o `slp_switch` deixa a pilha de quem troca: para o principal, para outro greenlet já iniciado, para um novo
# (o `g_initialstub` fica no caminho); e quanto acima desse ponto a pilha do greenlet novo começa.
_TO_MAIN = 0
_TO_STARTED = 64
_TO_NEW = 336
_NEW_STOP = 552


def _run_base(run, kwargs):
    from types import FunctionType, MethodType
    if isinstance(run, MethodType):
        base = _RUN_GREENLET_SWITCH if run.__func__ is greenlet.switch else _RUN_METHOD
    elif isinstance(run, FunctionType) or type(run).__name__ in ('builtin_function_or_method', 'method_descriptor'):
        base = _RUN_FUNCTION
    elif type(run).__name__ == 'partial':
        base = _RUN_PARTIAL
    else:
        base = _RUN_CALLABLE
    return base + (_RUN_KWARGS if kwargs else 0)


def _save_stacks(green, origin_state, state, new, finishing, run, payload):
    """A cópia preguiçosa de pilha do greenlet: ao trocar para `state`, cada greenlet suspenso desta thread cuja pilha
    passa do começo da de `state` tem essa parte copiada (a do principal começa no topo, então trocar para ele copia
    tudo); a de quem é retomado volta inteira."""
    live = getattr(green, 'gstacks', None)
    if live is None:
        live = green.gstacks = []
    if not finishing:
        off = _TO_NEW if new else (_TO_MAIN if state.main else _TO_STARTED)
        names = [n for n in _sys._gt_c_frames() if n not in _OWN_FRAMES]
        # O `__call__` do `run` objeto chamável já está na base do corpo (`_RUN_CALLABLE`).
        if names and names[0] == 'call:__call__' and origin_state.call_run:
            del names[0]
        native = sum(_c_frame_bytes(n) for n in names)
        origin_state.sp = origin_state.start + origin_state.base + native + off
        if not any(x is origin_state for x in live):
            live.append(origin_state)
    if new:
        state.start = (origin_state.sp - _NEW_STOP) if not finishing else origin_state.start
        kwargs = payload[2] if payload[0] == 'switch' else None
        state.base = _run_base(run, kwargs)
        state.call_run = state.base - (_RUN_KWARGS if kwargs else 0) in (_RUN_CALLABLE, _RUN_PARTIAL)
    for other in live:
        if other is state:
            continue
        # A pilha do principal começa no topo da pilha da thread: nada limita a parte dela a copiar.
        full = None if other.main else other.sp - other.start
        part = max(0, other.sp - state.start) if full is None else full if state.main else min(full, max(0, other.sp - state.start))
        if part > other.saved:
            other.saved = part
    live[:] = [x for x in live if x is not state and not x.dead]
    state.saved = 0
    state.sp = None


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
    _save_stacks(green, origin_state, state, not state.started, finishing, run, payload)
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
    state.run = _NO_RUN
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
    if state.started or state.run is _NO_RUN:
        raise AttributeError('run')
    return state.run


def _set_run(glet, value):
    state = _st(glet)
    if state.started:
        raise AttributeError('run cannot be set after the start of the greenlet')
    state.run = value


def _del_run(glet):
    _set_run(glet, _NO_RUN)


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

    def __init__(self, run=_NO_RUN, parent=None):
        if run is not _NO_RUN:
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
    def _stack_saved(self):
        state = _st(self)
        if state.dead or not state.started or self is getattr(_thread(), 'glet', None):
            return 0
        return state.saved

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

    @gr_context.deleter
    def gr_context(self):
        raise AttributeError("can't delete context attribute")

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
