#!/bin/sh
# Gera o `.expected.txt` de cada programa de `tests/golden/transpiled_main` com o bun 1.4.2 do oráculo (imagem
# pl-oracle-bun-check): o programa NOME.EXT roda como `/app/main.EXT` (`bun run`), e o stdout é o esperado. O código
# de saída tem de ser 0. O teste (`tests/transpiled_main_bun_golden.rs`) roda o MESMO fonte original no zjsc, com o
# transpilador do zbun instalado.
# Uso: gen-transpiled-main-golden.sh            gera só o que ainda não existe
#      gen-transpiled-main-golden.sh --all      refaz tudo
HERE="$(cd "$(dirname "$0")" && pwd)"
DIR="$HERE/../tests/golden/transpiled_main"
for file in "$DIR"/*; do
  case "$file" in *.expected.txt) continue ;; esac
  name="$(basename "$file")"
  if [ "$1" != "--all" ] && [ -f "$file.expected.txt" ]; then continue; fi
  ext="${name##*.}"
  tmp="$(mktemp -d)"
  cp "$file" "$tmp/main.$ext"
  if docker run --rm --entrypoint "" -v "$tmp":/app -w /app pl-oracle-bun-check bun run /app/main.$ext > "$file.expected.txt" 2> "$tmp/err.txt"; then
    echo "gerado: $name"
  else
    echo "bun recusou $name:"; cat "$tmp/err.txt"; rm -f "$file.expected.txt"
  fi
  rm -rf "$tmp"
done
