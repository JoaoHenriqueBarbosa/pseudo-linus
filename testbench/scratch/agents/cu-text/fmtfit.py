import json, itertools, sys
from fmtsim import wrap, mk
cases = json.load(open('fmtcases.json'))
outs = open('fmtout.txt').read().split('@@\n')
outs = [o.rstrip('\n').split('\n') for o in outs[:len(cases)]]
def goalf(w): return w * 187 // 200
def score(P, gf=goalf):
    ok = 0
    for (w, t), exp in zip(cases, outs):
        got = wrap(mk(t), 0, 0, w, gf(w), P)
        ok += got == exp
    return ok
if __name__ == '__main__':
    base = dict(short=1, ragged=0.5, widow=200, orphan=150, base=0, sentence=0, nobreak=0, punct=0, paren=0)
    print('baseline', score(base), len(cases))
    for wd in (0, 50, 100, 200, 400, 800):
        for rg in (0, 0.25, 0.5, 1):
            P = dict(base, widow=wd, ragged=rg)
            print(wd, rg, score(P))
