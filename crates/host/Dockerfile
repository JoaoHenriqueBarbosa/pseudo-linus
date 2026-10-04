# syntax=docker/dockerfile:1.7
#
# Imagem do pseudo-linusd. Contexto de build: a raiz do repositório.
#
#   docker build -f crates/host/Dockerfile -t registry.johnenrique.tech/pseudo-linus:1 .
#
# O arquivo de ignore ao lado (Dockerfile.dockerignore) manda só o workspace Rust e o que o build dele
# lê da bancada (o manifesto, os crates de apoio e o golden de onde o sysabi gera errno e sinais).

ARG RUST_IMAGE=rust:1.98.1-slim-trixie
ARG RUNTIME_IMAGE=debian:trixie-slim

FROM ${RUST_IMAGE} AS build
# libsqlite3-dev: o sqlite3 do pseudo-linus linka com a libsqlite3 do Debian 13 (fidelidade de versão).
RUN apt-get update \
    && apt-get install -y --no-install-recommends libsqlite3-dev pkg-config ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
# Features extras só pra diagnóstico local (ex.: CARGO_FEATURES=fake-backend); a imagem de produção
# sai sem nenhuma.
ARG CARGO_FEATURES=""
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked -p host --bins ${CARGO_FEATURES:+--features ${CARGO_FEATURES}} \
    && install -D -m 0755 target/release/pseudo-linusd /out/pseudo-linusd \
    && install -D -m 0755 target/release/osh /out/osh

FROM ${RUNTIME_IMAGE}
RUN apt-get update \
    && apt-get install -y --no-install-recommends libsqlite3-0 ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --user-group --home-dir /var/lib/pseudo-linus --shell /usr/sbin/nologin pl \
    && install -d -o pl -g pl -m 0700 /var/lib/pseudo-linus \
    && install -d -m 0755 /etc/pseudo-linus
COPY --from=build /out/pseudo-linusd /out/osh /usr/local/bin/
COPY crates/host/deploy/config.toml /etc/pseudo-linus/config.toml
ENV PL_CONFIG=/etc/pseudo-linus/config.toml \
    PL_LOG=info \
    PL_LOG_FORMAT=json
USER pl
WORKDIR /var/lib/pseudo-linus
VOLUME ["/var/lib/pseudo-linus"]
EXPOSE 8080
# O próprio binário faz o GET no /healthz (a imagem não tem curl).
HEALTHCHECK --interval=15s --timeout=5s --start-period=40s --retries=3 \
    CMD ["pseudo-linusd", "healthcheck", "--url", "http://127.0.0.1:8080/healthz"]
# O supervisor é o PID 1: trata SIGTERM (desliga os workers e grava o último uso das chaves) e recolhe
# os workers que ele mesmo cria.
STOPSIGNAL SIGTERM
ENTRYPOINT ["pseudo-linusd"]
CMD ["serve"]
