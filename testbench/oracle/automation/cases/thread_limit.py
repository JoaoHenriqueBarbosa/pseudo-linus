"""O limite de threads: com `RLIMIT_NPROC` baixo, `Thread.start()` e `_thread.start_new_thread` falham com
`RuntimeError: can't start new thread`, a thread que falhou não existe, e as vagas voltam quando as threads acabam.
O limite é posto em relação às tarefas que o usuário já tem (os processos ao redor variam de uma máquina para outra)."""

import _thread
import os
import resource
import threading

uid = os.getuid()
ambient = sum(1 for name in os.listdir('/proc') if name.isdigit() and os.stat('/proc/' + name).st_uid == uid)
resource.setrlimit(resource.RLIMIT_NPROC, (ambient + 4, ambient + 4))
print('limit', resource.getrlimit(resource.RLIMIT_NPROC)[0] - ambient)

release = threading.Event()


def work():
    release.wait()


started = []
failure = None
for _ in range(20):
    t = threading.Thread(target=work)
    try:
        t.start()
    except RuntimeError as exc:
        failure = (type(exc).__name__, str(exc), exc.args, t.is_alive(), t._started.is_set(), t.ident)
        break
    started.append(t)
print('started', len(started))
print('failure', failure)
try:
    _thread.start_new_thread(work, ())
except RuntimeError as exc:
    print('start_new_thread', type(exc).__name__, exc, exc.args, repr(exc))
print('active', threading.active_count())
try:
    t.join()
except RuntimeError as exc:
    print('join_failed', exc)
release.set()
for t in started:
    t.join()
print('active_after_join', threading.active_count())
for _ in range(3):
    t = threading.Thread(target=lambda: None)
    t.start()
    t.join()
print('restart_ok', not t.is_alive())
