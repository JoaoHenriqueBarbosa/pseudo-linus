#!/bin/sh
# Roda `cargo test` num container rust-mold (16 GiB), com alvo de build só deste worktree.
# Uso: scripts/container-test.sh <argumentos do cargo test...>
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
target=${TARGET_DIR:-/home/john/cache/target-chromium-disk3}
exec docker run --rm --memory 16g -v "$root:/work" -v "$target:/target" -e CARGO_TARGET_DIR=/target -w /work rust-mold cargo test "$@"
