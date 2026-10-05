#!/bin/bash
# uso: p.sh arquivo_de_script   (roda no oráculo com o ambiente fixo)
docker run --rm -i --tmpfs /work:exec pseudo-linus-oracle:719900900623 bash -c '
cd /work
export GIT_AUTHOR_NAME=Agent GIT_AUTHOR_EMAIL=agent@example.com GIT_COMMITTER_NAME=Agent GIT_COMMITTER_EMAIL=agent@example.com
export GIT_AUTHOR_DATE="1768478400 +0000" GIT_COMMITTER_DATE="1768478400 +0000"
T=1768478400
tick() { T=$((T+60)); export GIT_AUTHOR_DATE="$T +0000" GIT_COMMITTER_DATE="$T +0000"; }
ex() { echo "\$ $*"; "$@"; echo "[exit=$?]"; }
ex2() { echo "\$ $*"; "$@" 2>/tmp/e >/tmp/o; rc=$?; echo "[out]"; cat /tmp/o; echo "[err]"; cat /tmp/e; echo "[exit=$rc]"; }
# mk NOME: repo com 3 commits (a.txt, b.txt, c.txt), ramo main
mk() { rm -rf "$1"; git init -q -b main "$1"; cd "$1"; for f in a b c; do echo $f > $f.txt; git add $f.txt; tick; git commit -qm "add $f"; done; }
exec 2>&1
'"$(cat "$1")"
