import greenlet, sys
from greenlet import greenlet as G

log = []

def tr(ev, args):
    o, t = args
    log.append((ev, 'cur=' + ('main' if greenlet.getcurrent() is main else 'g' if greenlet.getcurrent() is g else 'other'),
                'o=' + ('main' if o is main else 'g'), 't=' + ('main' if t is main else 'g')))
    if raise_in[0]:
        raise_in[0] -= 1
        if raise_in[0] == 0:
            raise ValueError('boom')

main = greenlet.getcurrent()
raise_in = [0]

def run(x):
    log.append(('in run', x))
    v = main.switch('from g')
    log.append(('g resumed', v))
    return 'done'

g = G(run)
greenlet.settrace(tr)
r = g.switch(1)
log.append(('main got', r))
r = g.switch(2)
log.append(('main got', r))
greenlet.settrace(None)
for l in log:
    print(l)

print('--- raising in start')
log.clear()
g = G(run)
raise_in[0] = 1
greenlet.settrace(tr)
try:
    g.switch(1)
except BaseException as e:
    log.append(('main exc', repr(e)))
log.append(('g', g.dead, bool(g), greenlet.gettrace()))
greenlet.settrace(None)
for l in log:
    print(l)

print('--- raising on return to main')
log.clear()
g = G(run)
raise_in[0] = 2
greenlet.settrace(tr)
try:
    g.switch(1)
except BaseException as e:
    log.append(('main exc', repr(e)))
log.append(('g', g.dead, bool(g), greenlet.gettrace()))
greenlet.settrace(None)
for l in log:
    print(l)
