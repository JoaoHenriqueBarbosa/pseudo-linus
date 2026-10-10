#!/bin/sh
# Mede a árvore do perfil que o chromium do oráculo cria no disco.
# Uso: testbench/oracle/measure-profile.sh <modo> [página.html]
#   dump     --user-data-dir=/tmp/profile --dump-dom (o processo sai sozinho)
#   cdp      --user-data-dir=/tmp/profile com DevTools remoto, listada depois de 8 s com o navegador vivo e de novo
#            depois do SIGTERM
#   copy     como cdp, mas copia os bytes do perfil para $OUT/live (navegador vivo, 8 s) e $OUT/profile (depois do
#            SIGTERM); OUT=diretório do host
#   tmp      sem --user-data-dir, com DevTools remoto: /tmp e /root/.config/chromium listados com o navegador vivo e
#            depois do SIGTERM
# Saída: a árvore em JSON (profile-tree.py), uma entrada por linha, separada por "== <rótulo>".
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
mode=${1:?modo: dump, cdp ou tmp}
page=${2:-}
mounts="-v $root/testbench/oracle/profile-tree.py:/usr/local/bin/profile-tree.py:ro -v $root/testbench/corpus/headless:/pg:ro"
case "$mode" in
dump)
  script='chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --dump-dom "$PAGE" >/dev/null 2>&1; echo "== depois"; profile-tree.py /tmp/profile'
  ;;
cdp)
  script='chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --remote-debugging-port=9222 --remote-allow-origins=* "$PAGE" >/dev/null 2>&1 & pid=$!; sleep 8; echo "== vivo"; profile-tree.py /tmp/profile; kill -TERM $pid; wait $pid 2>/dev/null || true; echo "== depois do SIGTERM"; profile-tree.py /tmp/profile'
  ;;
tmp)
  script='chromium --headless --no-sandbox --disable-gpu --remote-debugging-port=9222 --remote-allow-origins=* "$PAGE" >/dev/null 2>&1 & pid=$!; sleep 8; echo "== vivo /tmp"; profile-tree.py /tmp; echo "== vivo .config"; profile-tree.py /root/.config/chromium; kill -TERM $pid; wait $pid 2>/dev/null || true; echo "== depois do SIGTERM /tmp"; profile-tree.py /tmp; echo "== depois .config"; profile-tree.py /root/.config/chromium'
  ;;
copy)
  # Copia o perfil inteiro (bytes) para $OUT (diretório do host), depois do SIGTERM, para gerar o molde.
  : "${OUT:?OUT: diretório do host que recebe a cópia}"
  mkdir -p "$OUT"
  mounts="$mounts -v $OUT:/out"
  script='chromium --headless --no-sandbox --disable-gpu --user-data-dir=/tmp/profile --remote-debugging-port=9222 --remote-allow-origins=* "$PAGE" >/dev/null 2>&1 & pid=$!; sleep 8; cp -a /tmp/profile /out/live; profile-tree.py /tmp/profile > /out/live-tree.jsonl; kill -TERM $pid; wait $pid 2>/dev/null || true; cp -a /tmp/profile /out/profile; profile-tree.py /tmp/profile > /out/profile-tree.jsonl; (cp -a /root/.config /out/config 2>/dev/null || true); profile-tree.py /root/.config > /out/config-tree.jsonl; profile-tree.py /tmp > /out/tmp-tree.jsonl; chmod -R a+rX /out'
  ;;
*) echo "modo desconhecido: $mode" >&2; exit 2 ;;
esac
PAGE=${page:+file:///pg/$page}
PAGE=${PAGE:-about:blank}
docker run --rm --network none $mounts -e PAGE="$PAGE" pl-oracle-chromium sh -c "$script"
