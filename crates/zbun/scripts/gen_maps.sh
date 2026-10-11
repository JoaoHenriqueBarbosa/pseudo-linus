#!/bin/sh
# Gera, para cada entrada de wip/zbun/tests/golden/maps, o que o `bun build` do oráculo (bun 1.4.2) imprime num bundle de
# um arquivo só com mapa externo: `<entrada>.bundle.expected` (o código do chunk, sem o comentário de cabeçalho nem o
# debugId) e `<entrada>.mappings.expected` (o campo `mappings` do mapa, com o `;` do cabeçalho). O `--no-bundle` do bun
# 1.4.2 ignora `--sourcemap`, então o mapa de posições só se mede pelo bundle; o algoritmo do mapa é o mesmo do printer.
# Uso: gen_maps.sh [ARQ...]   (sem argumentos: todas as entradas)
HERE="$(cd "$(dirname "$0")" && pwd)"
DIR="$HERE/../tests/golden/maps"
OUT=/tmp/zbun-maps-out
rm -rf "$OUT" && mkdir -p "$OUT"
if [ "$#" -gt 0 ]; then FILES="$*"; else FILES="$(cd "$DIR" && ls | grep -v 'expected$')"; fi
for f in $FILES; do
  docker run --rm --entrypoint "" -v "$DIR":/w -v "$OUT":/o pl-oracle-bun-check \
    bun build "/w/$f" --sourcemap=external --outdir=/o/"$f".d > /dev/null || echo "bun recusou $f"
done
python3 "$HERE/extract_maps.py" "$OUT" "$DIR"
