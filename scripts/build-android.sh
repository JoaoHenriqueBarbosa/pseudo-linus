#!/bin/sh
# Build do osh para Android (bionic, executável PIE dinâmico), por ABI, com o NDK. A libsqlite3
# 3.46.1 é compilada estática com o clang do NDK e as mesmas opções do pacote do Debian 13: a lista
# de `-D` é lida do build-musl.sh, que continua sendo o único lugar onde ela está escrita.
#
#   scripts/build-android.sh x86_64        # emulador
#   scripts/build-android.sh arm64-v8a     # aparelho
#
# Roda no host (sem Docker), a partir de qualquer diretório. Precisa do NDK e do alvo do rustup
# (`rustup target add x86_64-linux-android aarch64-linux-android`). Variáveis:
#
#   ANDROID_NDK_HOME   raiz do NDK (padrão: o mais novo em $ANDROID_HOME/ndk ou ~/Android/Sdk/ndk)
#   ANDROID_API        nível de API do alvo (padrão 30, Android 11)
#   CARGO_BUILD_JOBS   paralelismo do cargo (o próprio cargo lê)
#
# O binário sai em target/<triple>/release/osh, com as tabelas de linha do perfil release; para
# empacotar, `llvm-strip` do NDK tira os símbolos.
set -eu

case "${1:-}" in
    x86_64) triple=x86_64-linux-android ;;
    arm64-v8a) triple=aarch64-linux-android ;;
    *)
        echo "uso: $0 x86_64|arm64-v8a" >&2
        exit 2
        ;;
esac
api=${ANDROID_API:-30}
root=$(cd "$(dirname "$0")/.." && pwd)

if [ -z "${ANDROID_NDK_HOME:-}" ]; then
    ANDROID_NDK_HOME=$(ls -d "${ANDROID_HOME:-$HOME/Android/Sdk}"/ndk/*/ 2>/dev/null | sort -V | tail -1)
fi
bin=${ANDROID_NDK_HOME%/}/toolchains/llvm/prebuilt/linux-x86_64/bin
cc=$bin/$triple$api-clang
if [ ! -x "$cc" ]; then
    echo "$0: não achei $cc (ANDROID_NDK_HOME, ANDROID_API)" >&2
    exit 1
fi

# libsqlite3.a por ABI e por API, em target/android/sqlite-<triple><api>.
sq=$root/target/android/sqlite-$triple$api
if [ ! -f "$sq/libsqlite3.a" ]; then
    flags=$(sed -n 's/^[[:space:]]*\(-D[^\\]*[^\\ ]\)[[:space:]]*\\\{0,1\}$/\1/p' "$root/scripts/build-musl.sh")
    if [ -z "$flags" ]; then
        echo "$0: não achei as opções da sqlite no build-musl.sh" >&2
        exit 1
    fi
    mkdir -p "$sq"
    curl -sSfL https://www.sqlite.org/2024/sqlite-autoconf-3460100.tar.gz | tar xz -C "$sq"
    # shellcheck disable=SC2086 # a lista de -D é separada por espaços de propósito
    "$cc" -O2 -fPIC -c "$sq/sqlite-autoconf-3460100/sqlite3.c" -o "$sq/sqlite3.o" $flags
    "$bin/llvm-ar" rcs "$sq/libsqlite3.a" "$sq/sqlite3.o"
    cp "$sq/sqlite-autoconf-3460100/sqlite3.h" "$sq/sqlite-autoconf-3460100/sqlite3ext.h" "$sq"
fi

env_triple=$(echo "$triple" | tr - _)
env_upper=$(echo "$env_triple" | tr '[:lower:]' '[:upper:]')
export "CC_$env_triple=$cc" "AR_$env_triple=$bin/llvm-ar" "CARGO_TARGET_${env_upper}_LINKER=$cc"
export SQLITE3_LIB_DIR="$sq" SQLITE3_INCLUDE_DIR="$sq" SQLITE3_STATIC=1
# Sem caminho da máquina de quem compila dentro do binário.
export RUSTFLAGS="--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}/registry/src=/cargo --remap-path-prefix=$root=/src --remap-path-prefix=${RUSTUP_HOME:-$HOME/.rustup}=/rustup"
cd "$root"
cargo build --release --target "$triple" -p host --bin osh
ls -l "$root/target/$triple/release/osh"
