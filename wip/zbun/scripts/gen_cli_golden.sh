#!/bin/sh
# Gera tests/cli/*.expected.{build,minify,target,both} no oráculo (bun 1.4.2).
# uso: sh scripts/gen_cli_golden.sh   (rode da raiz de wip/zbun)
set -e
for f in tests/cli/*.js; do
  docker run --rm --entrypoint "" -v "$PWD/tests/cli":/w pl-oracle-bun-check sh -c "
    n=/w/$(basename "$f")
    bun build --no-bundle \$n > /w/$(basename "$f").expected.build
    bun build --no-bundle --minify-syntax \$n > /w/$(basename "$f").expected.minify
    bun build --no-bundle --target=bun \$n > /w/$(basename "$f").expected.target
    bun build --no-bundle --target=bun --minify-syntax \$n > /w/$(basename "$f").expected.both"
done
