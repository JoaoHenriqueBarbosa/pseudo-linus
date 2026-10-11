#!/bin/sh
# Gera o golden do conjunto da internet: para cada fita, roda o chromium real RUNS vezes (8 por padrão), sem rede,
# sobre o replay.
# - Cada saída distinta é um comportamento legítimo do Chromium (a ordem em que respostas paralelas chegam varia), e
#   todas viram golden: testbench/golden/chromium-internet/<id>.dom e, havendo mais, <id>.<n>.dom.
# - O que o oráculo não repete nem com o relógio virtual (o retrato de uma animação no meio, porque o trabalho de CPU
#   do carregamento anda o relógio de verdade e cada rodada para num instante diferente) vira intervalo: o número que
#   muda entre as rodadas, sempre com o mesmo texto logo antes e logo depois, guarda o mínimo e o máximo observados.
#   Cada texto distinto com algum número assim ganha <id>.ranges.json (e <id>.ranges.<n>.json); o resto continua
#   exato. Ver golden-ranges.py.
# O placar aceita qualquer variante exata, ou o despejo que bate com o texto de um grupo e cai nos intervalos dele.
# Só sai do placar (unstable.txt) a página cujo despejo vem vazio.
# Uso: gen-internet.sh [id ...]   (sem argumento, todas)
set -eu
RUNS=${RUNS:-8}
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
  rm -f "$golden/$id.dom" "$golden/$id".*.dom "$golden/$id".ranges*.json
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
  ranges=$(python3 -I "$root/testbench/oracle/golden-ranges.py" "$golden/$id.ranges.json" "$tmp/$id".*)
  sed -i "/^$id\$/d" "$unstable"
  if [ "$variants" -eq 0 ]; then
    echo "$id" >> "$unstable"
    echo "vazio: $id"
  else
    echo "golden: $id ($variants variante(s), $ranges grupo(s) com intervalo)"
  fi
  rm -f "$tmp/$id".*
done
rm -rf "$tmp"
