import re, threading, greenlet
from greenlet import greenlet as G


def norm(g):
    return re.sub(r'0x[0-9a-f]+', 'X', repr(g)) + ' | otid_nil=' + str('(nil)' in repr(g))


class Sub(G):
    pass


g = G(lambda: 1)
print(norm(g))
print(norm(Sub()))
print(norm(G()))
started = G(lambda: greenlet.getcurrent().parent.switch())
started.switch()
print(norm(started))
done = G(lambda: 1)
done.switch()
print(norm(done))
print(norm(greenlet.getcurrent()))
made = []
t = threading.Thread(target=lambda: made.append(G(lambda: 1)))
t.start()
t.join()
print(norm(made[0]))
other_thread_pending = made[0]
print(other_thread_pending.dead, bool(other_thread_pending))
