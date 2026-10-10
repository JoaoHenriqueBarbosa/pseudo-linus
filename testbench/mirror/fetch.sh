#!/bin/sh
# Baixa os arquivos do wheels.lock para wheels/ com o pip do oráculo e confere os sha256.
# Uma chamada do pip por arquivo: o lock pode ter duas versões do mesmo pacote (platformdirs, pyee, urllib3) e o
# resolvedor do pip recusa `pkg==1 pkg==2` na mesma linha. Wheel: `--only-binary`. Sdist (`.tar.gz`, como o
# websockets 10.4, que não tem wheel para cp313 nem pura no PyPI): `--no-binary` e o arquivo é o do próprio PyPI.
set -eu
cd "$(dirname "$0")"
mkdir -p wheels
wheels=$(sed -n 's/^[0-9a-f]\{64\}  \([^-]*\)-\([^-]*\)-.*\.whl$/\1==\2/p' wheels.lock | tr '\n' ' ')
sdists=$(sed -n 's/^[0-9a-f]\{64\}  \([^-]*\)-\(.*\)\.tar\.gz$/\1==\2/p' wheels.lock | tr '\n' ' ')
docker run --rm -v "$PWD/wheels":/w "${PL_MIRROR_IMAGE:-pseudo-linus-oracle:158760c4b91c}" bash -c \
  "python3 -m venv /v && for p in $wheels; do /v/bin/pip download -q --no-deps --only-binary=:all: -d /w \$p; done; \
   for p in $sdists; do /v/bin/pip download -q --no-deps --no-binary=:all: -d /w \$p; done; chown -R $(id -u):$(id -g) /w"
grep -v '^#' wheels.lock | (cd wheels && sha256sum -c --quiet)
