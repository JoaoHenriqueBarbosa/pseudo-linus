"""A cadeia de f_back de um quadro numa thread e num greenlet acaba na função dela: o `<module>` do script é só o fundo da principal."""
import sys, threading, greenlet

def chain(f):
    out = []
    while f is not None:
        out.append(f.f_code.co_name)
        f = f.f_back
    return out

res = []
def th():
    res.append(chain(sys._getframe(0)))
t = threading.Thread(target=th); t.start(); t.join()
print('thread', res)

main = greenlet.getcurrent()
def body():
    main.switch(sys._getframe(0))
g = greenlet.greenlet(body)
fr = g.switch()
print('greenlet', chain(fr))
def outer():
    inner()
def inner():
    main.switch(sys._getframe(0))
h = greenlet.greenlet(outer)
fr = h.switch()
print('nested', chain(fr), chain(h.gr_frame))
print('main', chain(sys._getframe(0)))
