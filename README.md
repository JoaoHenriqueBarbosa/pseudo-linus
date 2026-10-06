# pseudo-linus

Um Linux de mentira, escrito em Rust com `forbid(unsafe_code)`, feito para ser o computador de um
agente de IA. Por fora ele é um Debian 13 (trixie): os programas respondem byte a byte como os do
Debian, com as mesmas mensagens de erro, os mesmos códigos de saída e os mesmos arquivos gerados.
Por dentro é tudo simulado: kernel com processos, sinais, pipes, sockets TCP de loopback, VFS em
memória, `/proc` e escalonador EEVDF, e um userland reescrito por cima disso. Nada toca o host.

O que não existe responde `command not found`, como num Debian de verdade sem o pacote.

## O que tem dentro

- **Shell e texto:** `sh`/`bash`, coreutils, util-linux, findutils, grep, sed, awk (gawk), diff,
  patch, xargs, column, iconv, envsubst, less/more, tree, which.
- **Dados:** jq, yq, sqlite3, bc/dc, file, xxd, hexdump, strings.
- **Arquivos compactados:** tar, gzip, bzip2, xz, zstd, lzip, zip/unzip (com o deflate do zlib
  1.3.1 e o do gzip reproduzidos byte a byte), e os scripts `zgrep`, `zless`, `zcat` e afins.
- **Sistema:** procps (`ps`, `top`, `kill`, `free`...), ncurses e `tput`, git, shadow, data e fuso
  (`date`, `zic`, `zdump`, `faketime`).
- **Rede local:** `curl` 8.14.1 e `wget` 1.25.0 contra servidores que o próprio agente sobe no
  sandbox (sem saída para a internet).
- **Python 3.13.5:** um interpretador compatível com o CPython, com classes, metaclasses,
  geradores, `async`/`await`, inteiros arbitrários, `bytearray`/`memoryview`, imports de arquivos
  do usuário e uma stdlib grande (`os`, `pathlib`, `subprocess`, `json`, `csv`, `re`, `sqlite3`,
  `datetime`, `decimal`, `zipfile`, `tarfile`, `gzip`, `http.server`, `urllib`, `socket`,
  `asyncio`, `argparse`, `logging`, `email`, entre outros). Os tracebacks saem iguais aos do
  CPython. Em andamento: o Pillow (PIL), para o agente gerar gráficos e imagens.

## Conformância

Cada caso da bancada roda no pseudo-linus e num contêiner `debian:trixie` de verdade (o oráculo), e
o resultado só conta se stdout, stderr, código de saída e arquivos forem idênticos. O placar
completo está em `docs/conformance.md`:

**9264/9507 casos idênticos (97,4%)**, em 45 suítes e 400 programas. A maior parte das suítes está
em 100%; as que puxam a média para baixo são as de ferramentas de empacotamento (`dpkg`) e de
binários ELF (`binutils`), que estão fora do foco do projeto.

## Rodar

```sh
cargo build --release -p host --bin osh
target/release/osh                         # shell interativo no sandbox
target/release/osh -c 'uname -a; ls /usr/bin | wc -l'
target/release/osh scripts/v1-tour.sh      # passeio curto: texto, git, tar/zip, jq, sqlite, ps, tput
target/release/osh -c 'python3 -c "import sys; print(sys.version)"'
```

O diretório de trabalho é `/work` (um tmpfs próprio, como o `--tmpfs /work:exec` do oráculo); o
sandbox some ao sair, a menos que o `osh` aponte para um `pseudo-linusd` com `--remote` e `--keep`.

## Bancada

```sh
cargo run -p pl-conformance --release           # todas as suítes, regrava docs/conformance.md
cargo run -p pl-conformance --release -- git    # uma suíte
```

Os gabaritos saem do oráculo com `cargo run -p oracle -- gen` dentro de `testbench/` (precisa do
Docker e da imagem do oráculo, `testbench/oracle/Dockerfile`).

## Documentos

- `docs/design.md`: arquitetura e decisões.
- `docs/implementation.md`: o que cada crate implementa e de onde veio.
- `docs/conformance.md`: o placar por suíte.
- `docs/LICENSING.md`: licenças das fontes portadas.
