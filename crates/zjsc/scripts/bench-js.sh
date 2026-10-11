#!/bin/sh
# Baseline de desempenho: os MESMOS arquivos JS de `wip/zjsc/bench/js` no zjsc (`bench_run`, release) e no bun 1.4.2 do
# oráculo, três execuções por cenário, tempo de parede do processo (sem o início do container), mediana e razão zjsc/bun.
#
# Uso (a partir da raiz do repositório, com o `bench_run` já compilado em release):
#   wip/zjsc/scripts/bench-js.sh [RESULTADO.tsv]
# Variáveis: ZJSC_TARGET (alvo do cargo com o release, padrão $HOME/cache/target-bench), PW_PKG (a pasta
# `playwright/driver/package`, com `lib` e `package.json`, padrão $HOME/cache/pw-pkg), BUN_IMAGE (padrão
# pl-oracle-bun-check:latest), RUST_IMAGE (padrão rust-mold:latest, só para rodar o `bench_run`), RUNS (padrão 3).
set -eu

root=$(cd "$(dirname "$0")/../../.." && pwd)
target=${ZJSC_TARGET:-$HOME/cache/target-bench}
pw=${PW_PKG:-$HOME/cache/pw-pkg}
bun_image=${BUN_IMAGE:-pl-oracle-bun-check:latest}
rust_image=${RUST_IMAGE:-rust-mold:latest}
runs=${RUNS:-3}
out=${1:-/tmp/bench-js.tsv}
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

# O laço de medição roda DENTRO do container: `date +%s%N` em volta de cada processo.
cat > "$scratch/inner.sh" <<'EOF'
#!/bin/sh
# $1 = ferramenta (zjsc ou bun), $2 = comando do motor; os cenários vêm de /bench/js e /pw.
tool=$1; engine=$2; runs=$3
run_case() {
  name=$1; file=$2; shift 2
  i=1
  while [ "$i" -le "$runs" ]; do
    start=$(date +%s%N)
    code=0
    timeout 900 $engine "$file" "$@" > /tmp/stdout.txt 2> /tmp/stderr.txt || code=$?
    end=$(date +%s%N)
    sum=$(cksum < /tmp/stdout.txt | cut -d' ' -f1)
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$tool" "$i" "$(( (end - start) / 1000000 ))" "$code" "$sum"
    i=$((i + 1))
  done
}
echo 'void 0;' > /tmp/empty.js
run_case empty /tmp/empty.js 
for name in richards deltablue fib strconcat sort json regex decodeuri; do
  run_case "$name" "/bench/js/$name.js" 
done
run_case pw_parse_core /bench/js/parse_bundle.js /pw/lib/coreBundle.js
run_case pw_parse_utils /bench/js/parse_bundle.js /pw/lib/utilsBundle.js
run_case pw_require_core /bench/js/require_bundle.js /pw/lib/coreBundle.js
run_case pw_require_utils /bench/js/require_bundle.js /pw/lib/utilsBundle.js
# O que o `require` do bundle disse (uma execução, para o relatório).
for name in pw_require_core pw_require_utils; do :; done
EOF
chmod +x "$scratch/inner.sh"

docker run --rm --memory 16g -v "$scratch":/s:ro -v "$root/wip/zjsc/bench":/bench:ro -v "$pw":/pw:ro -v "$target":/target:ro \
  "$rust_image" sh /s/inner.sh zjsc /target/release/examples/bench_run "$runs" > "$scratch/zjsc.tsv"
docker run --rm --memory 16g -v "$scratch":/s:ro -v "$root/wip/zjsc/bench":/bench:ro -v "$pw":/pw:ro \
  "$bun_image" sh /s/inner.sh bun bun "$runs" > "$scratch/bun.tsv"
cat "$scratch/zjsc.tsv" "$scratch/bun.tsv" > "$out"
echo "gravado em $out"
