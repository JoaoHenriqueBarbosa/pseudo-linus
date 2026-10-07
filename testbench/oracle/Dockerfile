# Oráculo da bancada: um Debian 13 de verdade com as ferramentas GNU que o pseudo-linus imita.
# A imagem é pinada por digest pra que o golden seja reprodutível.
FROM debian:trixie-slim@sha256:a99cfc517144bc59b1978475ec53b46ecabec7e43635402ee5b77cc54cd1b20a

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      bash coreutils findutils diffutils patch sed grep gawk mawk jq \
      tar gzip bzip2 xz-utils zstd zip unzip lzip \
      file bc git sqlite3 yq faketime busybox procps util-linux bsdextrautils \
      ca-certificates locales-all \
 && rm -rf /var/lib/apt/lists/*

# Ferramentas acrescentadas na implementação (camada separada, pra não mexer nas versões de cima):
# referência do ul-misc (dc, tree, xxd, strings, envsubst, less, killall) e do ul-net (curl, wget,
# observados sem rede, já que o oráculo roda com --network none).
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      dc tree xxd binutils gettext-base less psmisc curl wget python3-pil python3-olefile python3-tk \
 && rm -rf /var/lib/apt/lists/*

# awk alvo é o gawk (decisão do design v2).
RUN update-alternatives --set awk /usr/bin/gawk

# Fase 1 do roadmap: o pip do Debian (python3-pip, com as bibliotecas vendorizadas).
RUN apt-get update \
 && apt-get install -y --no-install-recommends python3-pip python3-venv \
 && rm -rf /var/lib/apt/lists/*

ENV LC_ALL=C.UTF-8 TZ=UTC HOME=/root
WORKDIR /work
