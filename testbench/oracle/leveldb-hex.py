#!/usr/bin/env python3
"""Imprime as operações de escrita dos `.log` de um IndexedDB com as chaves e os valores COMPLETOS em hexadecimal.

Ferramenta do oráculo (Python só aqui). Complementa leveldb-dump.py, que abrevia valores longos: aqui sai uma linha
por operação, `put <chave hex> <valor hex>` ou `del <chave hex>`, com uma linha `batch seq=N` antes de cada lote.
Uso: leveldb-hex.py <arquivo.log> [...]
"""
import importlib.util
import os
import struct
import sys

_spec = importlib.util.spec_from_file_location("leveldb_dump", os.path.join(os.path.dirname(os.path.abspath(__file__)), "leveldb-dump.py"))
_dump = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_dump)


def main(paths):
    for path in paths:
        for record in _dump.records(open(path, "rb").read()):
            sequence, count = struct.unpack_from("<QI", record, 0)
            print("batch seq=%d" % sequence)
            pos = 12
            for _ in range(count):
                kind = record[pos]
                pos += 1
                length, pos = _dump.varint(record, pos)
                key = record[pos : pos + length]
                pos += length
                if kind == 1:
                    length, pos = _dump.varint(record, pos)
                    value = record[pos : pos + length]
                    pos += length
                    print("put %s %s" % (key.hex(), value.hex()))
                else:
                    print("del %s" % key.hex())


if __name__ == "__main__":
    main(sys.argv[1:])
