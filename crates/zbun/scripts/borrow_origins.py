#!/usr/bin/env python3
"""Agrupa os erros E0499/E0502 do log completo do rustc pela linha do primeiro empréstimo (a origem), para atacar a
origem que mais pesa. Uso: python3 -I borrow_origins.py LOG [N]"""
import collections
import re
import sys

text = open(sys.argv[1]).read()
count = collections.Counter()
for block in re.split(r"\n(?=error)", text):
    if not re.match(r"error\[E0(502|499)\]", block):
        continue
    lines = [l for l in block.split("\n") if re.match(r"\s*\d+ \|", l)]
    if not lines:
        continue
    where = re.search(r"--> (\S+):(\d+)", block)
    first = re.sub(r"^\s*\d+ \|\s*", "", lines[0])[:100]
    if first.startswith("impl<") and len(lines) > 1:
        first = re.sub(r"^\s*\d+ \|\s*", "", lines[1])[:100]
    count[re.sub(r"\b\d+\b", "N", first)] += 1
for key, value in count.most_common(int(sys.argv[2]) if len(sys.argv) > 2 else 15):
    print(value, key)
