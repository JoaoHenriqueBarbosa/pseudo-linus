#!/usr/bin/env python3
"""Resume o TSV do `bench-js.sh`: mediana por cenário e ferramenta, razão zjsc/bun e se a saída bate."""
import collections
import statistics
import sys

rows = collections.defaultdict(list)
for line in open(sys.argv[1], encoding="utf-8"):
    name, tool, _run, ms, code, checksum = line.rstrip("\n").split("\t")
    rows[(name, tool)].append((int(ms), int(code), checksum))

names = []
for name, _ in rows:
    if name not in names:
        names.append(name)
print("| Cenário | zjsc (ms) | bun (ms) | razão zjsc/bun | saída igual | saída do zjsc |")
print("|---|---|---|---|---|---|")
for name in names:
    z, b = rows.get((name, "zjsc"), []), rows.get((name, "bun"), [])
    if not z or not b:
        continue
    zm, bm = statistics.median(x[0] for x in z), statistics.median(x[0] for x in b)
    same = "sim" if {x[2] for x in z} == {x[2] for x in b} else "não"
    status = "ok" if all(x[1] == 0 for x in z) else "saiu com código %s" % z[0][1]
    ratio = "%.1f" % (zm / bm) if bm else "-"
    print("| %s | %d | %d | %s | %s | %s |" % (name, zm, bm, ratio, same, status))
