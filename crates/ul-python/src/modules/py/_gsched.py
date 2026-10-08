"""Escalonador das threads verdes (desenho em `wip/notes/python-threads.md`).

Cada thread de `threading`/`_thread` é uma pilha de quadros da VM (`_sys._gt_*`). Aqui fica só a política: quem
roda a seguir. A regra é a do CPython com a GIL, de forma determinística:

- `start()` roda a thread nova na hora, até ela bloquear ou acabar (a GIL passa a quem acabou de nascer);
- uma thread que bloqueia (travão, condição, `join`, `sleep`) fica suspensa e volta quando a condição vale
  ou o prazo vence;
- entre as que podem rodar, a que espera há mais tempo vai primeiro;
- sem ninguém para rodar, o processo espera de verdade (o prazo mais próximo, ou as fontes externas), e sem
  prazo nenhum bloqueia para sempre, como um `acquire` sem saída no CPython (só um sinal o interrompe).
"""

import _os
import _sys
import _thread
from time import monotonic as _monotonic


class _Green:
    """Uma thread verde: o identificador nativo, o objeto `threading.Thread` e o que a espera."""

    def __init__(self, tid, thread):
        self.tid = tid
        self.thread = thread
        self.cond = None
        self.deadline = None
        self.idents = [_thread._MAIN_IDENT]
        self.hooks = (None, None)


# `main` é a thread principal (tid 0); `_cur[0]`, a que roda agora.
main = _Green(0, None)
_cur = [main]
# Quem espera para rodar, em ordem de chegada: uma thread pronta tem `deadline = 0.0`; uma dormindo, só
# `deadline`; uma bloqueada, `cond` (e talvez `deadline`).
_queue = []
# Quantas threads além da principal existem (começaram e não acabaram).
_alive = [0]
# O `threading._state` (`_state['current']` é o objeto da thread que roda) e as fontes externas de eventos.
_state = {}
pollers = []
# O escalonador roda inteiro, como o do CPython com a GIL: com `_busy[0]` ligado a fatia de tempo que vence não
# troca de thread (uma troca pela metade, o `cond()` de uma espera). Quem liga guarda o valor anterior e o
# devolve ao sair; quem ganha a vez numa troca sai pelo `finally` dele, ou começa do zero no `_entry`.
_busy = _thread._busy


def _atomic(func):
    def run(*args):
        with _thread._Atomic():
            return func(*args)
    run.__name__ = func.__name__
    run.__doc__ = func.__doc__
    return run


def can_switch():
    """A VM pode trocar de thread aqui (só com o laço mais externo rodando)."""
    return _sys._gt_can_switch()


def current():
    return _cur[0]


def others():
    """Há outra thread viva além da que roda."""
    return _alive[0] > 0


def _enqueue(green, cond, deadline):
    green.cond = cond
    green.deadline = deadline
    _queue.append(green)


def _eligible(green, now):
    if green.cond is not None:
        try:
            if green.cond():
                return True
        except BaseException:
            # O erro nasce na thread dona da condição, quando ela rodar e a reavaliar.
            return True
    return green.deadline is not None and green.deadline <= now


def _idle(now):
    """Ninguém pode rodar: espera o prazo mais próximo ou as fontes externas."""
    deadlines = [g.deadline for g in _queue if g.deadline is not None]
    left = max(min(deadlines) - now, 0.0) if deadlines else None
    if pollers:
        for poller in list(pollers):
            poller(left)
    elif left is None:
        # Nada vai destravar ninguém: como o CPython, o processo bloqueia (um sinal ainda o interrompe).
        _os.sleep(3600.0)
    elif left > 0:
        _os.sleep(left)


def _pick():
    """Tira da fila a thread que roda a seguir, esperando se nenhuma puder."""
    while True:
        now = _monotonic()
        for green in _queue:
            if _eligible(green, now):
                _queue.remove(green)
                return green
        _idle(now)


def _install(target):
    """Põe de pé o contexto de `target`: a thread corrente do `threading`, os identificadores e os ganchos."""
    _cur[0] = target
    _state['current'] = target.thread
    _thread._idents[:] = target.idents


def _transfer(me, target):
    """Suspende `me` e retoma `target`. Volta quando alguém retomar `me`: quem o fizer já instalou o contexto dele."""
    me.idents = list(_thread._idents)
    me.hooks = _sys._swap_hooks(*target.hooks)
    _install(target)
    try:
        _sys._gt_switch(target.tid)
    except BaseException:
        _install(me)
        target.hooks = _sys._swap_hooks(*me.hooks)
        raise


def _run_next(me):
    """`me` já está na fila: passa o controle a quem for a vez (talvez ela mesma)."""
    target = _pick()
    if target is not me:
        _transfer(me, target)


@_atomic
def wait(cond, timeout, what):
    """Bloqueia a thread atual até `cond()` valer (`timeout` em segundos, `None` sem prazo). Devolve `cond()`."""
    if cond():
        return True
    if timeout is not None and timeout <= 0:
        return False
    me = _cur[0]
    _enqueue(me, cond, None if timeout is None else _monotonic() + timeout)
    try:
        _run_next(me)
    finally:
        # Um erro na espera (o tratador de um sinal que levanta, dentro do `poll`) não deixa a thread na fila.
        if me in _queue:
            _queue.remove(me)
    me.cond = None
    me.deadline = None
    return cond()


def preempt():
    """A fatia de tempo da thread que roda venceu (a VM chama entre instruções): como a GIL ao fim do
    `sys.getswitchinterval()`, a vez passa à thread pronta que espera há mais tempo. Sem ninguém pronto, a
    thread segue sem esperar; no meio do escalonador (`_busy`), a troca fica para a próxima fatia."""
    if _busy[0] or not _queue or not can_switch():
        return
    _yield_slice()


@_atomic
def _yield_slice():
    now = _monotonic()
    for target in _queue:
        if _eligible(target, now):
            break
    else:
        return
    me = _cur[0]
    _queue.remove(target)
    _enqueue(me, None, 0.0)
    try:
        _transfer(me, target)
    finally:
        if me in _queue:
            _queue.remove(me)
    me.deadline = None


@_atomic
def sleep(secs):
    """`time.sleep` com as outras threads rodando. Devolve quanto ainda falta dormir de verdade."""
    if not others() or not isinstance(secs, (int, float)) or secs < 0:
        return secs
    me = _cur[0]
    _enqueue(me, None, _monotonic() + secs)
    _run_next(me)
    me.deadline = None
    return 0.0


def serve(step):
    """A thread atual roda os passos de `step` (um `serve_forever` fatiado) até ele devolver `None`, e acaba."""
    from time import sleep as _sleep
    while True:
        result = step()
        if result is None:
            raise SystemExit
        _sleep(0.0 if result else 0.001)


def _entry(green):
    # A thread nova ganhou a vez no meio de uma troca: o escalonador já terminou o que fazia por ela.
    _busy[0] = False
    try:
        green.thread._bootstrap()
    except BaseException:
        pass
    _finish(green)


@_atomic
def _finish(green):
    """A thread acabou: a pilha dela se descarta e o controle passa a quem for a vez."""
    _alive[0] -= 1
    target = _pick()
    _sys._swap_hooks(*target.hooks)
    _install(target)
    _sys._gt_finish(target.tid)


def _spawn(thread):
    """Cria a thread verde de `thread`, ainda sem rodar."""
    green = _Green(None, thread)
    green.idents = [thread._ident]
    green.tid = _sys._gt_spawn(lambda: _entry(green))
    thread._green = green
    _alive[0] += 1
    return green


@_atomic
def defer(thread):
    """Cria a thread verde de `thread` e a deixa pronta na fila: quem iniciou segue rodando (o `_thread.start_new_thread`
    devolve na hora) e ela roda quando a corrente bloquear."""
    _enqueue(_spawn(thread), None, 0.0)


@_atomic
def start(thread):
    """Cria a thread verde de `thread` e a roda já, até bloquear ou acabar. Quem iniciou volta à fila."""
    green = _spawn(thread)
    me = _cur[0]
    _enqueue(me, None, 0.0)
    _transfer(me, green)
    me.cond = None
    me.deadline = None


def after_fork():
    """No filho de um `fork`: só a thread que o chamou sobrevive e vira a principal (tid 0 da VM nova)."""
    global main
    me = _cur[0]
    me.tid = 0
    main = me
    del _queue[:]
    _alive[0] = 0
