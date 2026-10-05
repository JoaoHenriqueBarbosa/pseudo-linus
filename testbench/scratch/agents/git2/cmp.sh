#!/bin/bash
# uso: cmp.sh script.sh...  -> roda cada script no oráculo (p.sh) e no osh com o mesmo prelúdio e
# mostra o diff; as saídas ficam em out/<nome>.oracle e out/<nome>.ours.
here=$(cd "$(dirname "$0")" && pwd)
osh="$here/../../../../target/release/osh"
mkdir -p "$here/out"
prelude='
mkdir -p /work /tmp; cd /work
export GIT_AUTHOR_NAME=Agent GIT_AUTHOR_EMAIL=agent@example.com GIT_COMMITTER_NAME=Agent GIT_COMMITTER_EMAIL=agent@example.com
export GIT_AUTHOR_DATE="1768478400 +0000" GIT_COMMITTER_DATE="1768478400 +0000"
T=1768478400
tick() { T=$((T+60)); export GIT_AUTHOR_DATE="$T +0000" GIT_COMMITTER_DATE="$T +0000"; }
ex() { echo "\$ $*"; "$@"; echo "[exit=$?]"; }
ex2() { echo "\$ $*"; "$@" 2>/tmp/e >/tmp/o; rc=$?; echo "[out]"; cat /tmp/o; echo "[err]"; cat /tmp/e; echo "[exit=$rc]"; }
# mk NOME: repo com 3 commits (a.txt, b.txt, c.txt), ramo main
mk() { rm -rf "$1"; git init -q -b main "$1"; cd "$1"; for f in a b c; do echo $f > $f.txt; git add $f.txt; tick; git commit -qm "add $f"; done; }
exec 2>&1
'
for s in "$@"; do
  n=$(basename "$s" .sh)
  bash "$here/p.sh" "$here/$n.sh" > "$here/out/$n.oracle" 2>&1 </dev/null
  env -i HOME=/root PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
    "$osh" -c "$prelude$(cat "$here/$n.sh")" > "$here/out/$n.ours" 2>&1 </dev/null
  if diff -q "$here/out/$n.oracle" "$here/out/$n.ours" >/dev/null; then
    echo "== $n: IGUAL"
  else
    echo "== $n: DIFERE ($(diff "$here/out/$n.oracle" "$here/out/$n.ours" | grep -c '^[<>]') linhas)"
  fi
done
