#!/bin/sh
# Imprime, de cada run medido, os lotes de IndexedDB a partir de um número de sequência, sem as chaves de escopo.
# Uso: idb-tail.sh <dir das medições> <seq mínimo> <run> ...
set -eu
dir=$1; min=$2; shift 2
here=$(cd "$(dirname "$0")" && pwd)
for n in "$@"; do
  echo "##### run$n"
  ls "$dir/run$n/IndexedDB/"*/ | tr '\n' ' '; echo
  for f in "$dir/run$n/IndexedDB/"*/*.log; do
    python3 -I "$here/leveldb-hex.py" "$f" | awk -v min="$min" '/^== /{next} /^batch seq=/{split($2,a,"="); p=(a[2]>=min)} p' | grep -v 'x7f.xff.xff.xff.xff.xff.xff' | cut -c1-175
  done
done
