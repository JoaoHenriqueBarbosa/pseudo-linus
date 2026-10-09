#!/bin/sh
# Gera o golden do conjunto da internet: para cada fita, roda o chromium real RUNS vezes, sem rede, sobre o replay.
# Cada saída distinta é um comportamento legítimo do Chromium (a ordem em que respostas paralelas chegam varia), e
# todas viram golden: testbench/golden/chromium-internet/<id>.dom e, havendo mais, <id>.<n>.dom. O placar aceita
# qualquer uma. Só sai do placar (unstable.txt) a página cujo despejo vem vazio.
# Uso: gen-internet.sh [id ...]   (sem argumento, todas)
set -eu
RUNS=${RUNS:-4}
root=$(cd "$(dirname "$0")/../.." && pwd)
list=$root/testbench/corpus/internet/pages.txt
golden=$root/testbench/golden/chromium-internet
unstable=$root/testbench/corpus/internet/unstable.txt
mkdir -p "$golden"
touch "$unstable"
tmp=$(mktemp -d)
grep -v '^#' "$list" | while IFS='|' read -r id group url; do
  if [ "$#" -gt 0 ] && ! printf '%s\n' "$@" | grep -qx "$id"; then continue; fi
  [ -f "$root/testbench/corpus/internet/$id/tape.json" ] || continue
  run=1
  while [ "$run" -le "$RUNS" ]; do
    docker run --rm --network none -v "$root/testbench/oracle":/oracle:ro -v "$root/testbench/corpus/internet/$id":/tape:ro \
      pl-oracle-chromium sh /oracle/replay-dump.sh "$url" > "$tmp/$id.$run" || true
    run=$((run + 1))
  done
  rm -f "$golden/$id.dom" "$golden/$id".*.dom
  variants=0
  for out in "$tmp/$id".*; do
    [ -s "$out" ] || continue
    seen=no
    for kept in "$golden/$id.dom" "$golden/$id".*.dom; do
      [ -f "$kept" ] && cmp -s "$out" "$kept" && seen=yes
    done
    [ "$seen" = yes ] && continue
    if [ "$variants" -eq 0 ]; then cp "$out" "$golden/$id.dom"; else cp "$out" "$golden/$id.$variants.dom"; fi
    variants=$((variants + 1))
  done
  sed -i "/^$id\$/d" "$unstable"
  if [ "$variants" -eq 0 ]; then
    echo "$id" >> "$unstable"
    echo "vazio: $id"
  else
    echo "golden: $id ($variants variante(s))"
  fi
done
rm -rf "$tmp"
