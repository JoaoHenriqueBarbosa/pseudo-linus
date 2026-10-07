#!/usr/bin/env bash
#
# Aponta o git deste clone para os hooks versionados em .githooks/.
#
# O .git/hooks não é versionado, então um hook colocado lá vale para uma máquina só e some no
# clone seguinte. core.hooksPath resolve isso: os hooks viram parte do repositório.
#
# Rode uma vez por clone: scripts/install-hooks.sh

set -euo pipefail

root=$(git rev-parse --show-toplevel)
cd "$root"

chmod +x .githooks/* scripts/dry-check.sh scripts/install-hooks.sh
git config core.hooksPath .githooks

echo "core.hooksPath = $(git config core.hooksPath)"
echo 'Hooks instalados. O commit passa a recusar duplicação nova.'

if ! command -v similarity-rs &>/dev/null; then
	echo >&2
	echo 'Atenção: similarity-rs não está instalado, e sem ele a checagem é pulada.' >&2
	echo 'Instale com: cargo install similarity-rs' >&2
fi
