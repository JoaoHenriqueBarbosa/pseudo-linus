# F02: grep e sed

## Hipóteses

- **H24** (v1): as crates do ripgrep dão um grep GNU-compatível com um front-end de flags. Critério:
  conformidade do grep montado com `grep-searcher`/`grep-matcher` contra o golden de grep.
- **H25** (v1): existe sed em Rust adotável (uutils/sed, sed-rs). Critério: conformidade de cada
  candidato no golden de sed e esforço pra rodar sobre bytes em memória.

## Método

**Oráculo.** GNU grep 3.11 e GNU sed 4.9 do Debian 13 (`LC_ALL=C.UTF-8`, umask 022), comparação byte a
byte de stdout, stderr, exit e árvore de arquivos (estrito) ou sem stderr (leniente).

**Corpus** (golden em `golden/{grep,sed}`):

| arquivo | casos | origem |
|---|---|---|
| `corpus/cases/grep/agent-style.toml` | 105 | à mão: `-E -F -i -v -n -c -l -L -o -w -x -r -R -A -B -C -NUM -h -H -q -s -e -f -m -b -T -Z --include --exclude --exclude-dir --label --binary-files`, binário, UTF-8 inválido, vários arquivos, stdin, exit 0/1/2, erros de sintaxe e de uso |
| `corpus/cases/grep/upstream-foad1.toml` | 39 | GNU grep 3.11 `tests/foad1` (linhas com argumentos literais, sem `--color=always`) |
| `corpus/cases/grep/upstream-yesno.toml` | 41 | GNU grep 3.11 `tests/yesno` (combinações de `-m`, `-v`, `-o`, `-C`, `--group-separator`) |
| `corpus/cases/sed/agent-style.toml` | 117 | à mão: `s` com `g p I N w`, `-n -E -r -i -i.bak -s -z`, endereços e intervalos (`N`, `$`, `/re/`, `/re/I`, `\%re%`, `N,M`, `addr,+N`, `first~step`, `0,/re/`, `!`), `d p q Q a i c y = l F r w z`, hold space, `N D P`, `b t T`, vários `-e`, `-f`, `\U \L \u`, scripts de agente, erros |
| `corpus/cases/sed/upstream-misc.toml` | 52 | GNU sed 4.9 `testsuite/misc.pl` (a lista `@Tests` inteira, lida com `perl` + `JSON::PP`) |
| `corpus/cases/sed/upstream-scripts.toml` | 3 | GNU sed 4.9 `madding`, `uniq`, `mac-mf` |

Os importados são gerados pelo binário `f02-gen-cases` a partir de `corpus/upstream/{grep,sed}`; as
saídas esperadas das suítes não são usadas, o golden sai do oráculo. Os testes `.sh` do sed que dependem
do framework (`init.sh`, `compare`, `returns_`) não foram convertidos.

A ordem da saída de `grep -r` no GNU segue o readdir do sistema de arquivos do container (não é
ordenada): casos com a tag `order-insensitive` comparam as linhas ordenadas dos dois lados.

**Como cada candidato roda.**

- grep montado (nosso): front-end de flags do GNU (`src/grep/args.rs`: `getopt_long` com permutação,
  `-NUM`, prefixo único de opção longa, mensagens do getopt), árvore do caso em memória
  (`src/fsview.rs`, com symlink), `grep-searcher` (`search_slice`) varrendo linhas com contexto e `-v`,
  e o casador (`src/grep/matcher.rs`) implementando `grep_matcher::Matcher` sobre o motor do F01
  (`regex-automata` sem backref, `ferroni` com backref, parser GNU nosso), com o `-w` do GNU (casada mais
  curta no mesmo início, depois o próximo início). Dois printers: o nosso no formato do GNU, ou o
  `grep-printer` (Standard e Summary) configurado o mais perto possível.
- `uu_grep`, uutils/sed (`sed`), `sed-rs`, `red-sed` usam `std::fs`, `stdout` e `stdin` do processo:
  cada caso roda num subprocesso (o próprio binário com `--exec`, `argv[0]` igual ao da ferramenta,
  umask 022) com `cwd` numa cópia da fixture em `scratch/f02-grep-sed`, e o diretório é retratado
  depois. O `main` de cada um foi reproduzido a partir do binário deles; o CLI do red é do binário (não
  da lib), então o parser `lexopt` dele foi portado pra receber o argv.
- `bashkit` 0.18.2 (linha de base): o comando roda no bash virtual dele, com a fixture num `InMemoryFs`
  em `/work/case`.

**Esforço de porte** (`src/effort.rs`): no código-fonte de cada candidato, linhas com I/O do host
(`std::fs`, `File`, `stdout`/`stdin`, `std::process`, `std::env`, `libc`, `rustix`, `memmap2`,
`tempfile`) e linhas acopladas ao motor de regex, fora de testes.

## Candidatos

| papel | candidato | versão | cat. (árvore) |
|---|---|---|---|
| grep | ripgrep como biblioteca: `grep-searcher` + `grep-regex` + `grep-printer`, front-end nosso | 0.1.17 / 0.1.14 / 0.3.1 | b |
| grep | `grep-searcher` + motor F01 + `grep-printer` | | b |
| grep | `grep-searcher` + `grep-matcher` + motor F01 + printer nosso | 0.1.17 / 0.1.9 | b |
| grep | `uu_grep` (uutils/grep, onig em C), referência | 0.2.0 | c |
| grep | `bashkit` grep, linha de base | 0.18.2 | b |
| sed | uutils/sed (crate `sed`) | 0.2.0 | b |
| sed | `sed-rs` | 2.0.0 | b |
| sed | `red-sed` (red) | 1.0.2 | b |
| sed | `bashkit` sed, linha de base | 0.18.2 | b |

`grep-searcher` é (b) porque tem `search_path`/`search_file` (memmap); a API usada aqui
(`search_slice`) não toca o host. `grep-matcher` é (a). Examinados e não rodados: `sedx`, `esed`, `sd`,
`ripsed` (não são compatíveis com o sed), `tinysandbox` (puxa wasmtime).

## Resultado

| candidato | estrito | leniente | onde erra |
|---|---|---|---|
| ripgrep como é (grep-regex + grep-printer) | 86,5% | 86,5% | leftmost-first (`-oE 'ab\|abcd'`), backref, printer |
| grep-searcher + F01 + grep-printer | 88,1% | 88,1% | printer: `-o` com contexto imprime as linhas de contexto, `-o -v` imprime linhas, sem `--` entre arquivos, `-T` ignorado, casada vazia do `-o` vira linha vazia |
| **grep-searcher + grep-matcher + F01 + printer nosso** | **100%** (185/185) | **100%** | |
| uu_grep (referência) | 90,8% | 93,5% | `--` com `-o` e entre arquivos, `--group-separator`, `-v -o -C` |
| bashkit grep (linha de base) | 53,5% | 54,1% | caminho absoluto no `-r`, erro no stdout, sem backref, `-NUM`, `--label`, binário |
| uutils/sed | 79,7% | 83,7% | `-i` consome o argumento seguinte como sufixo, sem `\U \L \u`, `F`, `-z`, `\b`; erros |
| sed-rs | 70,3% | 70,3% | sem backref, endereço `I`, `addr,+N`, `0,/re/`, `-`; perde `\r` e a falta de `\n` final; exit codes |
| red | 84,9% | 89,0% | endereço `I`, `i`/`a` com texto começando por `//`, bloco dividido em vários `-e`, `\s \S`, `-z`, `\x00` em `y`, `s///N` |
| **bashkit sed** | **90,1%** | **94,2%** | `\t` dentro de colchete, `\xHH`, `-z`, scripts longos (`uniq`, `mac-mf`) |

Esforço de porte (linhas de código fora de testes; linhas com I/O do host; linhas acopladas à regex):

| candidato | código | I/O do host | regex |
|---|---|---|---|
| uu_grep | 2407 | 14 | 2 (onig) |
| uutils/sed | 4066 | 19 em 6 arquivos, mais `uucore` | 11 (`fancy-regex`/`regex`, em `fast_regex.rs`) |
| sed-rs | 1832 | 17 | 5 (`regex`) |
| red | 11845 | 62 em 11 arquivos | 25 (motor próprio, 4880 linhas) |
| bashkit sed (`src/builtins/sed`) | 1788 | 0 (já é VFS) | 5 (`regex`/`fancy-regex`, em `pattern.rs`) |

## Veredito

**H24: parcial.** `grep-searcher` (varredura de linhas, contexto, `-v`) e `grep-matcher` (a interface do
casador) servem: montados com o nosso front-end e o motor do F01, dão 100% dos 185 casos, incluindo
`foad1` e `yesno` do próprio GNU grep. O resto do ripgrep não serve: `grep-regex` é leftmost-first e sem
backref, e o `grep-printer` tem formato próprio (88% mesmo com o motor certo).

- **Aproveitar:** `grep-searcher`, `grep-matcher`.
- **Fazer à mão:** o front-end de flags (getopt do GNU, `-NUM`, prefixos), o casador sobre o motor do
  F01 com o `-w` do GNU, o printer no formato do GNU (`-m` com contexto, `-o` com contexto e com `-v`,
  separadores, `-b`, `-T`, `-Z`), detecção de binário (NUL ou UTF-8 inválido, aviso no stderr), `-r`/`-R`
  com `--include`/`--exclude`/`--exclude-dir` sobre o VFS e os exit codes. Este experimento já tem isso
  (`src/grep/`, cerca de 1300 linhas).

**H25: parcial.** Nenhum sed em Rust é adotável como está: o melhor é o sed do bashkit, com 94,2%
leniente, e os citados no v1 ficam em 83,7% (uutils/sed) e 70,3% (sed-rs). O sed do bashkit é o único
que já roda sobre VFS em memória (zero linhas de I/O do host) e tem a regex isolada em `pattern.rs`; vira
base de fork extraindo `src/builtins/sed` (1788 linhas), trocando `regex`/`fancy-regex` (leftmost-first)
pelo motor do F01 e corrigindo escapes (`\t` em colchete, `\xHH`), `-z` e scripts longos. uutils/sed e
red exigiriam reescrever o I/O (`std::fs`, `stdout`, `uucore`, `in_place`, `signal-hook`) além do motor.

## Como rodar

```sh
cargo run --release --bin f02-gen-cases                   # regenera os casos importados
cd ../.. && cargo run -q -p oracle -- gen --tool grep && cargo run -q -p oracle -- gen --tool sed
cd experiments/f02-grep-sed && cargo run --release        # results/f02-grep-sed.json, ~40 s
cargo run --release -- red                                # falhas de um candidato
```
