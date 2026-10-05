#!/bin/bash
# Uso: run.sh comandos.txt [diretório]  -> roda cada linha no oráculo e no osh, grava oracle.out e
# ours.out e mostra o diff. Cada comando sai como "=== cmd", a saída (stdout e stderr juntos) e
# "rc=N". Com um diretório, os arquivos dele vão por tar no stdin (o argumento do bash -c tem limite
# de 128 KiB) e são extraídos no diretório de trabalho antes dos comandos.
here=$(cd "$(dirname "$0")" && pwd)
cmds=$1
extra=$2
script=$(
  echo 'export TZ=UTC; mkdir -p /work/uzp && cd /work/uzp'
  [ -n "$extra" ] && echo 'tar xf -; exec </dev/null'
  while read -r name b64; do echo "echo $b64 | base64 -d > $name.zip"; done < "$here/fixtures.txt"
  while IFS= read -r c; do
    [ -z "$c" ] && continue
    printf 'echo %q\n' "=== $c"
    printf '%s 2>&1; echo "rc=$?"\n' "$c"
  done < "$cmds"
)
feed() { if [ -n "$extra" ]; then tar -C "$extra" -cf - .; fi; }
feed | docker run --rm -i --tmpfs /work:exec pseudo-linus-oracle:719900900623 bash -c "$script" > "$here/oracle.out" 2>&1
feed | "$here/../../../target/release/osh" -c "$script" > "$here/ours.out" 2>&1
diff "$here/oracle.out" "$here/ours.out" && echo "IGUAIS ($(grep -c '^===' "$here/oracle.out") comandos)"
