#!/bin/sh
# Baixa as wheels do wheels.lock para wheels/ com o pip do oráculo e confere os sha256.
set -eu
cd "$(dirname "$0")"
mkdir -p wheels
pkgs=$(sed -n 's/^[0-9a-f]\{64\}  \([^-]*\)-\([^-]*\)-.*/\1==\2/p' wheels.lock)
docker run --rm -v "$PWD/wheels":/w pseudo-linus-oracle:158760c4b91c bash -c \
  "python3 -m venv /v && /v/bin/pip download -q --no-deps --only-binary=:all: -d /w $pkgs; chown -R $(id -u):$(id -g) /w"
grep -v '^#' wheels.lock | (cd wheels && sha256sum -c --quiet)
