# pseudo-linus

Um Linux de mentira, escrito em Rust com `forbid(unsafe_code)`, que se comporta como um Debian 13
(trixie) byte a byte: kernel com processos, sinais, VFS em memória, `/proc` e escalonador EEVDF, e
o userland reescrito (coreutils, util-linux, procps, ncurses, git, tar/gzip/bzip2/zip, awk, sed,
grep, jq, yq, sqlite3, bc, file, diff/patch, findutils, entre outros). Nada toca o host.

## v1

Conformância contra o oráculo (um contêiner `debian:trixie` de verdade), em `docs/conformance.md`:
**7460/7483 casos idênticos (99,7%)**. Os 23 de fora são conhecidos: 21 casos de CSV que usam o
`python3` como referência (o Python não é portado) e 2 do sqlite (`generate_series` e o conteúdo
do journal depois de uma saída abrupta).

## Rodar

```sh
cargo build --release -p host --bin osh
target/release/osh                         # shell interativo no sandbox
target/release/osh -c 'uname -a; ls /usr/bin | wc -l'
target/release/osh scripts/v1-tour.sh      # passeio curto: texto, git, tar/zip, jq, sqlite, ps, tput
```

O diretório de trabalho é `/work` (um tmpfs próprio, como o `--tmpfs /work:exec` do oráculo); o
sandbox some ao sair, a menos que o `osh` aponte pra um `pseudo-linusd` com `--remote` e `--keep`.

## Painel

```sh
cargo run -p pl-conformance --release           # todas as suítes, regrava docs/conformance.md
cargo run -p pl-conformance --release -- git    # uma suíte
```

Os goldens saem do oráculo com `cargo run -p oracle -- gen` dentro de `testbench/` (precisa do
Docker e da imagem do oráculo).

## Documentos

- `docs/design.md`: arquitetura e decisões.
- `docs/implementation.md`: o que cada crate implementa e de onde veio.
- `docs/conformance.md`: o placar por suíte.
- `docs/LICENSING.md`: licenças das fontes portadas.
