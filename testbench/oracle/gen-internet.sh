#!/bin/sh
# Gera o golden do conjunto da internet: para cada fita, roda o chromium real duas vezes, sem rede, sobre o replay.
# As duas saídas iguais viram testbench/golden/chromium-internet/<id>.dom; saídas diferentes (a página depende de
# aleatório, relógio ou ordem de rede) vão para unstable.txt e saem do placar, porque nem o oráculo se repete.
# Uso: gen-internet.sh [id ...]   (sem argumento, todas)
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
list=$root/testbench/corpus/internet/pages.txt
golden=$root/testbench/golden/chromium-internet
mkdir -p "$golden"
tmp=$(mktemp -d)
grep -v '^#' "$list" | while IFS='|' read -r id group url; do
  if [ "$#" -gt 0 ] && ! printf '%s\n' "$@" | grep -qx "$id"; then continue; fi
  [ -f "$root/testbench/corpus/internet/$id/tape.json" ] || continue
  for run in 1 2; do
    docker run --rm --network none -v "$root/testbench/oracle":/oracle:ro -v "$root/testbench/corpus/internet/$id":/tape:ro \
      pl-oracle-chromium sh /oracle/replay-dump.sh "$url" > "$tmp/$id.$run" || true
  done
  if cmp -s "$tmp/$id.1" "$tmp/$id.2" && [ -s "$tmp/$id.1" ]; then
    cp "$tmp/$id.1" "$golden/$id.dom"
    sed -i "/^$id\$/d" "$root/testbench/corpus/internet/unstable.txt" 2>/dev/null || true
    echo "golden: $id"
  else
    rm -f "$golden/$id.dom"
    grep -qx "$id" "$root/testbench/corpus/internet/unstable.txt" 2>/dev/null || echo "$id" >> "$root/testbench/corpus/internet/unstable.txt"
    echo "instável ou vazio: $id"
  fi
done
rm -rf "$tmp"
