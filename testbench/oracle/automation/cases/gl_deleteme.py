"""O greenlet que outra thread soltou só morre na thread dona dele, na próxima vez que ela consulta o greenlet
corrente (o `deleteme` do C): o `GreenletExit` dele e o `__del__` de quem ele segura rodam na dona."""
import threading
import greenlet
from greenlet import greenlet as G

seen = []
refs = []
created = threading.Event()
released = threading.Event()
cleared = threading.Event()
done = threading.Event()


def body():
    try:
        greenlet.getcurrent().parent.switch()
    except BaseException as e:
        seen.append((type(e).__name__, threading.current_thread().name))
        raise


def bg():
    g = G(body)
    g.switch()
    refs.append(g)
    del g
    created.set()
    released.wait(5)
    G()
    cleared.set()
    done.wait(5)


t = threading.Thread(target=bg, name='Bg')
t.start()
created.wait(5)
print('after creation', seen)
del refs[:]
print('after main drop', seen)
released.set()
cleared.wait(5)
print('after bg alloc', seen)
done.set()
t.join()


class Bomb:
    def __del__(self):
        print('bomb del in', threading.current_thread().name, flush=True)


holder = []


def deallocator():
    greenlet.getcurrent().parent.switch()


g = G(deallocator)
g.bomb = Bomb()
g.switch()
holder.append(g)
del g


def other():
    del holder[:]
    print('other dropped the holder', flush=True)


t2 = threading.Thread(target=other, name='Other')
t2.start()
t2.join()
print('main before getcurrent', flush=True)
greenlet.getcurrent()
print('main after getcurrent', flush=True)
