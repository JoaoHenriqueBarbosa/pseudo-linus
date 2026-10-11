#!/bin/sh
# Gera tests/cli_errors/*.expected.stderr (stderr e código de saída) no oráculo (bun 1.4.2).
# Os arquivos rodam em /w/x.js, então o `at` do diagnóstico é `/w/x.js:L:C`.
# uso: sh scripts/gen_cli_errors_golden.sh   (rode da raiz de wip/zbun)
set -e
for f in tests/cli_errors/*.js; do
  name=$(basename "$f")
  docker run --rm --entrypoint "" -v "$PWD/tests/cli_errors":/w pl-oracle-bun-check sh -c "
    bun build --no-bundle /w/$name > /dev/null 2> /w/$name.expected.stderr; echo \$? > /w/$name.expected.code"
done
