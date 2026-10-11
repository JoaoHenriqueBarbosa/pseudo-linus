#!/bin/sh
# Roda scripts/dry-check.sh num container do rust-mold, sem sujar o host. O similarity-rs mora em
# $DRY_CACHE/similarity (instalação única, persistente):
#   docker run --rm -v "$DRY_CACHE/similarity":/opt/sim rust-mold cargo install similarity-rs --root /opt/sim
# O repositório inteiro é montado no mesmo caminho do host para o git do worktree funcionar lá dentro.
# O dry-check só olha crates/; o que mora em wip/ (zjsc, zbun) se confere direto com o similarity-rs, no mesmo container:
#   docker run --rm -v "$PWD":/w -v "$DRY_CACHE/similarity":/opt/sim -w /w rust-mold /opt/sim/bin/similarity-rs ARQ... --min-lines 3 --threshold 0.90 --skip-test
# Uso, de dentro do repositório ou de um worktree: sh scripts/dry-docker.sh [ARQ...]   (sem argumentos, o índice)
DRY_CACHE="${DRY_CACHE:-$HOME/cache}"
here=$(pwd)
common=$(cd "$(git rev-parse --git-common-dir)/.." && pwd)
exec docker run --rm --name "dry-$$" --memory 8g \
  -v "$common":"$common" -v "$DRY_CACHE/similarity":/opt/sim -w "$here" \
  -e PATH=/opt/sim/bin:/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  rust-mold sh -c 'git config --global --add safe.directory "*"; exec bash scripts/dry-check.sh "$@"' sh "$@"
