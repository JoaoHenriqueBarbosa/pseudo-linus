import re, sys
from sim import *
blocks = parse('ptxdata.txt')
P = dict(db=0, dk=0, da=0, dt=-1, dh=0, hang=1)
flt = eval(sys.argv[1]) if len(sys.argv)>1 else (lambda W,g,f,t,mode: True)
lim = int(sys.argv[2]) if len(sys.argv)>2 else 3
n=0
for hdr, lines in blocks.items():
    m = re.match(r'@@ w=(\d+) g=(\d+) f=\[(.*)\] t=\[(.*)\] opt=(\w+)', hdr)
    W, g, f, t, mode = int(m[1]), int(m[2]), m[3], m[4], m[5]
    if mode=='A' or not flt(W,g,f,t,mode): continue
    sim=run(t,W,g,f,mode,P)
    if sorted(sim)==sorted(lines): continue
    n+=1
    print(hdr)
    for a in sorted(set(sim)-set(lines)): print('  SIM|'+a)
    for a in sorted(set(lines)-set(sim)): print('  ORA|'+a)
    if n>=lim: break
