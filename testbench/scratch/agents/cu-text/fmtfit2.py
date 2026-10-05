import json, itertools, sys
from fmtsim import wrap, mk2
cases = json.load(open('fmtcases2.json'))
outs = open('fmtout2.txt').read().split('@@\n')
outs = [o.rstrip('\n').split('\n') for o in outs[:len(cases)]]
def score(P, show=0):
    ok = 0; bad = []
    for (w, t), exp in zip(cases, outs):
        got = wrap(mk2(t), 0, 0, w, w * 187 // 200, P)
        if got == exp: ok += 1
        else: bad.append((w, t, exp, got))
    for b in bad[:show]:
        print('w', b[0]); print('  ORA', b[2]); print('  SIM', b[3])
    return ok
if __name__ == '__main__':
    base = dict(short=1, rdiv=2, widow=200, orphan=150, base=0, sentence=50, nobreak=600, punct=40, paren=40, lone=0)
    print(score(base), len(cases))
