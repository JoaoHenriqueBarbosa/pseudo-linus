#!/bin/sh
# Gera o golden de cada entrada de wip/zbun/tests/golden com o `bun build --no-bundle` do oráculo (bun 1.4.2, imagem
# pl-oracle-bun-check). A entrada é montada em /w dentro do contêiner, então o texto gerado (o nome do jsxDEV, por
# exemplo) não carrega caminho da máquina de quem desenvolve.
#
# Saídas por entrada `x`: `x.expected` (sem flag) e `x.expected.<variante>` para cada variante de minificação:
#   minify-whitespace, minify-syntax, minify-identifiers, minify (as três juntas).
# Uso: gen_golden.sh            gera só o que ainda não existe
#      gen_golden.sh --all      refaz tudo
#      gen_golden.sh ARQ...     refaz os arquivos escolhidos
HERE="$(cd "$(dirname "$0")" && pwd)"
DIR="$HERE/../tests/golden"
if [ "$1" = "--all" ]; then
  FILES="$(cd "$DIR" && ls -p | grep -v '/$' | grep -v 'expected')"
  FORCE=1
elif [ "$#" -gt 0 ]; then
  FILES="$*"
  FORCE=1
else
  FILES="$(cd "$DIR" && ls -p | grep -v '/$' | grep -v 'expected')"
  FORCE=0
fi

# gen VARIANTE FLAGS: uma execução do contêiner para todos os arquivos da variante.
gen() {
  variant="$1"; flags="$2"; todo=""
  for f in $FILES; do
    if [ "$variant" = "default" ]; then out="$f.expected"; else out="$f.expected.$variant"; fi
    if [ "$FORCE" = 1 ] || [ ! -f "$DIR/$out" ]; then todo="$todo $f"; fi
  done
  [ -z "$todo" ] && return
  docker run --rm --entrypoint "" -v "$DIR":/w pl-oracle-bun-check sh -c '
    for f in '"$todo"'; do
      if [ "'"$variant"'" = default ]; then out="$f.expected"; else out="$f.expected.'"$variant"'"; fi
      if ! bun build --no-bundle '"$flags"' "/w/$f" > "/w/$out" 2> "/w/$f.err"; then
        echo "bun recusou $f ('"$variant"'):"; cat "/w/$f.err"; rm -f "/w/$out"
      fi
      rm -f "/w/$f.err"
    done'
}

gen default ""
gen minify-whitespace "--minify-whitespace"
gen minify-syntax "--minify-syntax"
gen minify-identifiers "--minify-identifiers"
gen minify "--minify"
