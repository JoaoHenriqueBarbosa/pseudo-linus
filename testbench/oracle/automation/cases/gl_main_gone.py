import threading, time, greenlet


class T(threading.Thread):
    main_glet = None

    def run(self):
        self.main_glet = greenlet.getcurrent()


t = T()
t.start()
t.join(10)
time.sleep(0.05)
m = t.main_glet
print(m.dead, bool(m), m.parent, m.gr_frame, m.started if hasattr(m, 'started') else '-')
print(repr(m)[repr(m).index("(otid"):])
try:
    m.switch()
except BaseException as e:
    print(type(e).__name__, e)
try:
    m.throw()
except BaseException as e:
    print(type(e).__name__, e)
print(m.dead, m.gr_context)
for _ in range(3):
    print(m.dead, repr(m).endswith(' (thread exited) dead>'))
