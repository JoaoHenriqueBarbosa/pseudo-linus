# ul-archive: estado

Dono: agente arquivos. Alvo: GNU tar 1.35, gzip 1.13, bzip2 1.0.8, xz-utils 5.8.1, lzip 1.25,
zstd 1.5.7, Info-ZIP zip 3.0 e unzip 6.0.

## Pronto

- Esqueleto do crate e `programs()` com tar, gzip/gunzip/zcat, bzip2/bunzip2/bzcat,
  xz/unxz/xzcat/lzma/unlzma/lzcat, zstd/unzstd/zstdcat, lzip, zip e unzip.
- `codec`: codecs em Rust puro (flate2 com zlib-rs, bzip2 com libbz2-rs-sys, lzma-rust2,
  structured-zstd), enquadramento gzip e laço de fluxos bzip2 nossos, erros classificados, compressão
  e descompressão em fluxo; roundtrip verde em todos os formatos.
- `getopt`, `sysutil`, `tz` (iguais aos do ul-diff).

## Em andamento

- `tar` (GNU tar 1.35), compressores e zip/unzip.

## Placar

A preencher na primeira rodada completa.
