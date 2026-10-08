#!/bin/sh
# Build estática (musl) do osh e do pseudo-linusd, com a libsqlite3 3.46.1 compilada com as
# mesmas opções do pacote do Debian 13 (as que o `pragma compile_options` do oráculo lista).
#
# Roda dentro de `rust:1-slim-trixie`, com a worktree do commit em /work:
#
#   docker run --rm -v "$WORKTREE":/work -v rel-trixie:/target -v rel-sqlite:/sq \
#     -v "$PWD/scripts/build-musl.sh":/build.sh:ro -v "$HOME/.cargo/registry":/usr/local/cargo/registry \
#     -e OWNER="$(id -u):$(id -g)" rust:1-slim-trixie sh /build.sh
#
# Os binários saem em /target/x86_64-unknown-linux-musl/release/{osh,pseudo-linusd}.
set -eu
apt-get update -qq >/dev/null && apt-get install -y -qq musl-tools libclang-dev clang curl make >/dev/null
rustup target add x86_64-unknown-linux-musl >/dev/null
mkdir -p /sq && cd /sq
if [ ! -f libsqlite3.a ]; then
    curl -sSfL https://www.sqlite.org/2024/sqlite-autoconf-3460100.tar.gz | tar xz
    cd sqlite-autoconf-3460100
    musl-gcc -O2 -c sqlite3.c -o sqlite3.o \
        -DSQLITE_ALLOW_ROWID_IN_VIEW -DSQLITE_DEFAULT_RECURSIVE_TRIGGERS \
        -DSQLITE_DEFAULT_WAL_SYNCHRONOUS=2 -DSQLITE_DIRECT_OVERFLOW_READ \
        -DSQLITE_ENABLE_COLUMN_METADATA -DSQLITE_ENABLE_DBPAGE_VTAB -DSQLITE_ENABLE_DBSTAT_VTAB \
        -DSQLITE_ENABLE_FTS3 -DSQLITE_ENABLE_FTS3_PARENTHESIS -DSQLITE_ENABLE_FTS3_TOKENIZER \
        -DSQLITE_ENABLE_FTS4 -DSQLITE_ENABLE_FTS5 -DSQLITE_ENABLE_MATH_FUNCTIONS \
        -DSQLITE_ENABLE_PREUPDATE_HOOK -DSQLITE_ENABLE_RTREE -DSQLITE_ENABLE_SESSION \
        -DSQLITE_ENABLE_STMTVTAB -DSQLITE_ENABLE_UNLOCK_NOTIFY -DSQLITE_ENABLE_UPDATE_DELETE_LIMIT \
        -DHAVE_ISNAN -DSQLITE_LIKE_DOESNT_MATCH_BLOBS -DSQLITE_MAX_SCHEMA_RETRY=25 \
        -DSQLITE_MAX_VARIABLE_NUMBER=250000 -DSQLITE_SECURE_DELETE -DSQLITE_SOUNDEX \
        -DSQLITE_TEMP_STORE=1 -DSQLITE_THREADSAFE=1 -DSQLITE_USE_URI -DSQLITE_DEFAULT_AUTOVACUUM=0 \
        -DSQLITE_DEFAULT_MMAP_SIZE=0 -DSQLITE_MAX_MMAP_SIZE=0x7fff0000 -DSQLITE_DEFAULT_FILE_FORMAT=4 \
        -DSQLITE_DEFAULT_SECTOR_SIZE=4096 -DSQLITE_DEFAULT_SYNCHRONOUS=2 \
        -DSQLITE_DEFAULT_JOURNAL_SIZE_LIMIT=-1 -DSQLITE_DEFAULT_WORKER_THREADS=0 -DSQLITE_MAX_WORKER_THREADS=8 \
        -DSQLITE_ENABLE_LOAD_EXTENSION -DSQLITE_MAX_DEFAULT_PAGE_SIZE=32768
    ar rcs ../libsqlite3.a sqlite3.o
    cp sqlite3.h sqlite3ext.h ..
    cd ..
fi
cd /work
export SQLITE3_LIB_DIR=/sq SQLITE3_INCLUDE_DIR=/sq SQLITE3_STATIC=1
export RUSTFLAGS="-C target-feature=+crt-static -C relocation-model=static --remap-path-prefix=/usr/local/cargo/registry/src=/cargo --remap-path-prefix=/work=/src --remap-path-prefix=/usr/local/rustup=/rustup"
CARGO_TARGET_DIR=/target cargo clean -q --release --target x86_64-unknown-linux-musl -p libsqlite3-sys
CARGO_TARGET_DIR=/target cargo build --release --target x86_64-unknown-linux-musl -p host --bin osh --bin pseudo-linusd
chown -R "$OWNER" /target /sq
