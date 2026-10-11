#!/bin/sh
# Grava as fitas do conjunto da internet (precisa de rede): para cada linha de pages.txt, o chromium real navega e
# a fita vai para testbench/corpus/internet/<id>/. Uso: record-internet.sh [id ...]   (sem argumento, todas)
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
list=$root/testbench/corpus/internet/pages.txt
grep -v '^#' "$list" | while IFS='|' read -r id group url; do
  if [ "$#" -gt 0 ] && ! printf '%s\n' "$@" | grep -qx "$id"; then continue; fi
  rm -rf "$root/testbench/corpus/internet/$id"
  mkdir -p "$root/testbench/corpus/internet/$id"
  echo "gravando $id ($url)"
  docker run --rm -v "$root/testbench/oracle":/oracle:ro -v "$root/testbench/corpus/internet/$id":/out pl-oracle-chromium \
    python3 /oracle/tape-record.py /out "$url" || echo "falhou: $id"
done
