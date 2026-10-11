#!/usr/bin/env python3
"""Imprime a linha em que fecha o bloco aberto na linha dada (balanceamento de chaves, sem strings).

Uso: python3 -I block_end.py ARQUIVO LINHA_DE_ABERTURA
"""
import sys

lines = open(sys.argv[1], "rb").read().split(b"\n")
depth = 0
for i in range(int(sys.argv[2]) - 1, len(lines)):
    for ch in lines[i]:
        if ch == ord("{"):
            depth += 1
        elif ch == ord("}"):
            depth -= 1
            if depth == 0:
                print("fecha na linha", i + 1)
                raise SystemExit
print("sem fecho")
