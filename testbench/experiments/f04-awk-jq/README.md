# F04: awk e jq

Mede se dá pra adotar um awk e um jq escritos em Rust no pseudo-linus, contra o gawk 5.2.1 e o jq
1.7.1 do Debian 13 (o oráculo), e se a avaliação deles pode ser interrompida por um checkpoint sem
fork. Roda tudo com:

```sh
cd testbench/experiments/f04-awk-jq
cargo run --release            # grava ../../results/f04-awk-jq.json (cerca de 2 a 3 minutos)
cargo test --release
```

Na primeira execução o binário instala os candidatos que só existem como CLI (`cargo install` em
`scratch/f04-awk-jq/tools`, versões pinadas, cerca de 10 minutos e 123 MB de executáveis), baixa a
suíte do gawk e as do jq pra `corpus/upstream/` e valida as duas no oráculo (com cache em
`scratch/f04-awk-jq/`). Precisa do oráculo pronto (`cargo run -p oracle -- build`) e do golden
(`cargo run -q -p oracle -- gen --tool awk` e `--tool jq`).

## Hipóteses

| Id | Frase | Critério |
|---|---|---|
| H26 | Nenhum awk em Rust chega perto do gawk; precisa de interpretador próprio | Conformidade estrita de cada candidato nos casos de agente mais o subconjunto da suíte do gawk. Confirmada se nenhum passa de 80%. |
| H27 | jaq tem compatibilidade alta com o jq real | Suítes do jq 1.7.1 e casos de agente, divergências por classe. O critério não fixa número; aqui: confirmada com 90% ou mais nas suítes e 90% ou mais frouxo na CLI, parcial de 70% a 90%, refutada abaixo de 70%. |
| H07 (só evidência) | Laço de CPU sem checkpoint não cede nem morre | As sondagens deste experimento dizem, candidato por candidato, quem cede e quem só para matando o processo. O veredito é do E01. |

## Método

### Corpus

- **awk, casos de agente**: 234 casos em `corpus/cases/awk/` (6 arquivos), escritos no estilo do que
  agentes fazem: campos e separadores (`-F`, `FS`, `OFS`, `NF`, campos além de `NF`, `FPAT`,
  `FIELDWIDTHS`), registros (`RS` vazio, caractere, regex com `RT`, CRLF, sem newline final),
  padrões e faixas, `next`, `nextfile`, `exit`, `BEGINFILE`, printf com todos os formatos usuais,
  números e strnum, `OFMT`/`CONVFMT`, arrays, `in`, `delete`, `SUBSEP`, ordem do for-in,
  `PROCINFO["sorted_in"]`, `asort`, `asorti`, arrays de arrays, funções e recursão, `split`, `substr`,
  `index`, `match` com `RSTART`/`RLENGTH` e com array, `sub`/`gsub` com `&` e `\\&`, `gensub`,
  `tolower`/`toupper`, UTF-8, regex dinâmica, classes POSIX, intervalos, três casos onde
  leftmost-longest difere de leftmost-first, `getline` nas formas comuns, `print > arquivo`, `>>`,
  `| "sort"`, `system`, `/dev/stderr`, `ENVIRON`, `ARGV`, operandos `var=valor`, `-v`, `-f`, `-e`,
  `--`, erros (arquivo ausente, sintaxe, divisão por zero, opção desconhecida), `strftime`, `mktime`,
  `systime`, e 13 pipelines de shell. Golden do gawk 5.2.1 (o `awk` do oráculo).
- **awk, suíte do gawk 5.2.1** (`test/` do tarball, em `corpus/upstream/gawk/`, gitignored porque é
  GPLv3). Seleção automática em `src/gawk_suite.rs`: dos 522 testes de BASIC_TESTS, UNIX_TESTS e
  GAWK_EXT_TESTS, 360 usam a regra padrão do `Gentests` sem flag nem locale (fora de todas as listas
  NEED_* e de RUN_SHELL, sem alvo próprio no `Makefile.am`, sem `.sh`). Cada um vira um caso
  `gawk -f T.awk < T.in` com `AWKPATH=.` e, na fixture, os arquivos do diretório de teste citados no
  programa. O oráculo roda os 360 e 352 reproduzem o `.ok` byte a byte (stdout, stderr e
  `EXIT CODE: n`); os 8 restantes (`back89`, `gsubtst5`, `rebt8b1`, `clos1way6`, `gensub2`, `lint`,
  `regx8bit`, `typeof2`) ficam de fora. O golden de cada um é a saída do oráculo (stdout, stderr,
  exit e árvore de arquivos), igual aos casos de agente. 281 dos 352 não têm diagnóstico no stderr
  ("só saída").
- **jq, casos de CLI**: 194 casos em `corpus/cases/jq/` (3 arquivos): todas as flags pedidas (`-r`,
  `-j`, `-c`, `-n`, `-s`, `-R`, `-e` e os exits 0/1/4, `--arg`, `--argjson`, `--slurpfile`,
  `--rawfile`, `--args`, `--jsonargs`, `--indent`, `--tab`, `-S`, `-a`, `--raw-output0`, `--seq`,
  `--stream`, `-f`), formatos `@csv`, `@tsv`, `@sh`, `@base64`, `@base64d`, `@base32`, `@uri`,
  `@json`, `@text`, `@html`, mensagens de erro e exits 2, 3 e 5, números (`1.0`, `1e1000`, inteiros
  grandes, `-0`, nan), datas, regex (inclusive lookaround, backreference e classes Unicode do
  Oniguruma), caminhos, entradas, ordenação, geradores, `input`/`inputs`, `$__loc__`, `env`/`$ENV`,
  e 6 pipelines de shell. Golden do jq 1.7.1-6+deb13u4 (que se identifica como `jq-1.7`).
- **jq, suítes do jq 1.7.1** (`tests/jq.test`, `man.test`, `onig.test`, `manonig.test`,
  `base64.test`, `optional.test` da tag jq-1.7.1, em `corpus/upstream/jq/`). O parser e a regra de
  aprovação são os do `src/jq_test.c`: programa, entrada, saídas até linha em branco ou comentário;
  `%%FAIL` exige erro de compilação e a mensagem (salvo `%%FAIL IGNORE MSG`); comparação por valor
  (`jv_equal`: números como double, objetos sem ordem); erro depois das saídas esperadas não reprova,
  saída a mais reprova. Cada arquivo roda primeiro no oráculo com `jq --run-tests`; dos 738 testes
  parseados, 15 falham no próprio jq do Debian (ex.: módulos, que pedem `-L`) e ficam de fora: 723
  testes. Nos candidatos, cada teste roda como `jq -c PROGRAMA` com a entrada no stdin.

### Como cada candidato roda

- **Binários** (uutils/awk, awk-rs, awkrs, frawk, zawk, jaq, xq, qj, tq): `cargo install` com versão
  pinada (`src/tools.rs`). Cada caso roda num diretório novo do scratch com a fixture materializada
  (mtime fixo), ambiente do contrato da bancada, umask 022 e um diretório de shims na frente do PATH
  (`awk`, `gawk` ou `jq` apontam pro binário do candidato), então os casos `script` também usam o
  candidato. Timeout de 8 s por caso. É a medida de conformidade da categoria (b) sobre cópia da
  fixture; o acoplamento ao host é medido à parte.
- **Em processo** (rawk-core, bashkit, jaq com a camada nossa): sobre a fixture em memória, num
  subprocesso do próprio experimento (`score-inproc`), pra que um abort do candidato vire resultado em
  vez de derrubar a bancada. Threads com pilha de 256 MiB (1 GiB pro jaq).
- **faketime**: o host não tem libfaketime. Os 2 casos com faketime ficam como não suportados nos
  binários; o jq nosso e o bashkit recebem o relógio injetado (`now` sobrescrito, `fixed_epoch`).
- **Estrito** = stdout, stderr, exit e árvore de arquivos iguais; **frouxo** = tudo menos o stderr.

### A camada nossa sobre o jaq

`src/jq/` (cerca de 2.600 linhas) monta `jaq-core` 3.1.1, `jaq-std` 3.0.3 e `jaq-json` 2.0.3 e põe
por cima o que o jaq não faz igual ao jq 1.7.1:

- CLI: porte do `main.c` (opções curtas combinadas, `--`, `--arg`/`--argjson`/`--slurpfile`/
  `--rawfile`/`--args`/`--jsonargs` com as mesmas mensagens, `--indent` de -1 a 7, `--tab`, `-S`,
  `-a`, `--raw-output0`, `--seq`, `--stream` emulado, `-e` com as regras de `last_result`, códigos
  0/1/2/3/4/5, erro de compilação `jq: N compile error(s)` com a linha do programa);
- leitura: porte do `jv_parse.c` (mensagens e linha/coluna dos erros de parse) e do `jq_util_input`
  (arquivos concatenados, `fgets` de 4096, a posição `(at <stdin>:N)` e o `<unknown>`);
- números: literais preservados na forma canônica do decNumber (`1e2` sai `1E+2`, `1.0` sai `1.0`) e
  números calculados com o `jvp_dtoa_fmt` (17 dígitos, `1e+20`, infinito vira `1.7976931348623157e+308`);
- escritor: indentação, `--tab`, `-S`, `-a`, escapes iguais ao `jv_dump_term`;
- mensagens de erro de topo traduzidas a partir do texto do jaq (`cannot index 5 with "b"` vira
  `Cannot index number with string "b"`, `cannot calculate` vira `object (...) and number (1) cannot
  be added`, com o truncamento de 11 bytes do jq);
- nativas sobrescritas (o compilador do jaq usa a primeira com o nome): `tojson`, `fromjson`, `env`,
  `now`, `input`, `inputs`, `input_filename`, `input_line_number`, `stderr`, `debug`, `halt_error`;
- definições do `builtin.jq` do jq 1.7.1 onde o jaq diverge (`join`, `limit`, `first/1`, `last/1`,
  `nth/2`, `isempty`, `ltrimstr`, `rtrimstr`, `scan`, `splits`, `split/2`, `tostream`,
  `truncate_stream`, `from_entries`, `IN`, `INDEX`, `JOIN`, `format`, `todate`, `gamma`), mais `@csv`,
  `@tsv` (o jaq não tem) e `@base32` (o jq do Debian responde "is not a valid format");
- `$__loc__`, que o parser do jaq não conhece, trocado lexicamente;
- o checkpoint (abaixo).

O mesmo código roda em processo sobre a MemTree e como binário multicall (o executável do experimento
chamado como `jq`), que é o que os casos `script` usam.

### Checkpoint

Cada sondagem (`src/checkpoint.rs`) roda num subprocesso com teto de 3 GB de memória virtual
(`prlimit`) e timeout de 3 s. Uma thread liga a flag de interrupção aos 100 ms; quem respeita a flag
desenrola a pilha com `resume_unwind` (o mecanismo do kill no design) e a sondagem mede a latência e
o maior intervalo entre dois checkpoints. Quem não respeita é morto pelo pai.

No jaq, o gancho é um `DataT` próprio: o avaliador chama `HasLut::lut()` a cada nó avaliado
(`Id::run`, `paths`, `update` e cada nativa), e o nosso `lut()` passa pelo checkpoint. Isso não exige
fork. O custo foi medido contra o jaq puro (`JustLut`) no mesmo programa, 9 rodadas alternadas.

### Acoplamento

`depscan` (`crates/depscan`) em cada candidato, com cache. A categoria registrada é a do depscan
refinada: o depscan marca como (c) toda crate com `links = ...`, e crates como `defmt` e `rayon-core`
usam `links` sem código C; aqui (c) é quem tem ferramenta de build de C (`cc`, `cmake`, `bindgen`,
`pkg-config`) ou é `-sys` com `links` e `build.rs`. A categoria bruta fica no JSON.

### Regra de encaixe

- awk: encaixa se passar de 90% estrito (combinado) e for (a); com trabalho se passar de 80%; senão
  não encaixa.
- jq: encaixa se passar de 95% nas suítes e 95% frouxo na CLI e for (a); com trabalho se passar de
  80% nas suítes e 85% frouxo; senão não encaixa.
- bashkit, o binário do jaq e a variante normalizada do qj são referência, não dependência.

## Candidatos

### awk

| Candidato | Versão | Categoria | Agente estrito / frouxo | Suíte do gawk estrito | Combinado estrito | Checkpoint sem fork | Encaixe |
|---|---|---|---|---|---|---|---|
| awkrs | 0.5.6 | (c): gmp-mpfr-sys, Cranelift, reedline, nix | 210/234 (89,7%) / 93,2% | 187/352 (53,1%) | 67,7% | não (VM + JIT) | não encaixa |
| awk-rs | 0.2.0 | (b): 21 pontos de host | 170/234 (72,6%) / 73,5% | 144/352 (40,9%) | 53,6% | só em E/S | não encaixa |
| frawk `-B interp` | 0.4.8 | (c): llvm-sys opcional, jemalloc | 125/234 (53,4%) | 100/352 (28,4%) | 38,4% | não | não encaixa |
| zawk | 0.5.25 | (c): 619 crates, OpenSSL, MySQL, MQTT, SQLite... | 118/234 (50,4%) | 99/352 (28,1%) | 37,0% | não | não encaixa |
| bashkit awk | 0.18.2 | (c) só por iana-time-zone-haiku (alvo Haiku); (b) no Linux | 122/234 (52,1%) | 92/352 (26,1%) | 36,5% | por ação, via cancelamento | referência |
| rawk-core | 0.6.0 | (a) | 103/234 (44,0%) | 92/352 (26,1%) | 33,3% | não | não encaixa |
| uutils/awk | git e787315 | (c): minrx-sys (C++) | 66/234 (28,2%) | 75/352 (21,3%) | 24,1% | não (VM sans-IO) | não encaixa |
| frawk (Cranelift, padrão) | 0.4.8 | (c) | 37/234 (15,8%) | 53/352 (15,1%) | 15,4% | não | não encaixa |

Notas:

- **awkrs**: o melhor nos casos de agente (erra mensagens de erro, alguns gawk-ismos e 5 casos de
  regex, entre eles os 3 de leftmost-longest), mas cai pra 53% na suíte do gawk. É uma VM com JIT Cranelift, GMP/MPFR, LSP, DAP,
  REPL com reedline e paralelismo de registros: 196 pontos de host no código próprio, 236 crates na
  árvore. Adotar exigiria um fork que remove a maior parte do projeto.
- **awk-rs**: interpretador de árvore pequeno (7,8 mil linhas); a biblioteca recebe `BufRead`/`Write`
  pra entrada e saída principais, mas `print >`, `getline <`, pipes e `system` vão direto pro
  `std::fs`/`std::process` (21 pontos). Usa o crate `regex` (leftmost-first).
- **frawk e zawk**: o backend padrão do frawk (JIT Cranelift 0.93) entra em panic em qualquer programa
  que lê campo (`codegen/clif.rs:938`); com `-B interp` funciona. O zawk é o frawk com uma biblioteca
  padrão enorme. Os dois são "awk-like" de propósito (tipagem estática, sem algumas semânticas POSIX).
- **uutils/awk**: a arquitetura é a que mais combina com o pseudo-linus (a VM suspende em toda E/S e
  o driver atende: `Signal::Suspend(IoRequest)`), mas ainda é WIP: `split`, `gsub`, `match`, `-v` e
  vários opcodes são `todo!()` (266 dos 586 casos terminam em panic). Usa o motor POSIX MinRX em C++.
- **rawk-core**: API recebe as linhas e devolve as linhas de saída: sem `-v`, sem vários arquivos, sem
  saída parcial (`printf` vira linha), código de saída sempre 0. 25 casos não são expressáveis.
- **bashkit awk**: interpretador próprio do bashkit; não implementa `RS`, atribuição a `NF`, arrays de
  arrays, `@função`, e com os limites padrão trunca qualquer laço em 10 mil iterações sem avisar
  (`BEGIN { while (1) x++; print x }` imprime `10000` e sai com 0).

Os números são os do `results/f04-awk-jq.json` desta execução; awk-rs, frawk, zawk e xq variam de 1
a 3 casos entre execuções porque a ordem do for-in (ou de chaves) deles vem de hash com semente
aleatória.

Por tag (estrito nos casos de agente), o que todos têm em comum: **os 3 casos de leftmost-longest
falham em todos os candidatos** (todos usam regex leftmost-first, ou não implementam `match`), a ordem
do for-in do gawk não é reproduzida por ninguém, e as mensagens de erro passam em 1 de 11 no melhor.

### jq

| Candidato | Versão | Categoria | CLI estrito / frouxo | Suítes do jq 1.7.1 | Checkpoint sem fork | Encaixe |
|---|---|---|---|---|---|---|
| qj | 0.2.1 | (c): onig_sys (Oniguruma), libc, simdjson C++ | 147/194 (75,8%) / 95,9% | 713/723 (98,6%) | não (VM sem gancho) | com trabalho |
| qj, nome do programa normalizado | 0.2.1 | | 173/194 (89,2%) / 95,9% | 98,6% | | referência |
| xq | 0.5.0 | (c): onig_sys | 67/194 (34,5%) / 36,6% | 632/723 (87,4%) | não | não encaixa |
| jaq-core + jaq-std + jaq-json com camada nossa | 3.1.1 / 3.0.3 / 2.0.3 | (b) refinada (a bruta é c só por `defmt`); todo ponto de host é nativa sobrescrevível | 168/194 (86,6%) / 89,2% | 604/723 (83,5%) | **sim** (`DataT`) | com trabalho |
| bashkit jq | 0.18.2 | (c) só por iana-time-zone-haiku; (b) no Linux | 102/194 (52,6%) / 67,0% | 580/723 (80,2%) | não (só por valor emitido) | referência |
| jaq (binário) | 3.1.1 | (c): mimalloc | 109/194 (56,2%) / 72,2% | 574/723 (79,4%) | | referência |
| tq (modo JSON) | 0.3.0 | (c): signal-hook com `cc` | 84/194 (43,3%) / 59,8% | 407/723 (56,3%) | | não encaixa |

Notas:

- **qj** (criado em fevereiro de 2026, um autor com 755 commits, 0.2.1, 16 estrelas) é um porte do
  jq 1.8.1 inteiro pra Rust: VM de bytecode
  (`jq::lang::execute::run`), `builtin.jq` do jq, Oniguruma pelo mesmo `onig_sys` que o jq vendoriza,
  e simdjson só no caminho rápido da CLI. Tem uma trait `Host` pra entrada, `debug`, `stderr` e
  `halt`, e um driver em memória (`qj::jq::lang::execute::driver::run`) que roda o núcleo em processo
  sobre bytes (sondagem `qj-core-small`). Quase todas as falhas estritas da CLI são o prefixo `qj:` no
  lugar de `jq:`; com o nome normalizado sobra a diferença 1.8.1 contra 1.7.1: formato de erro de
  compilação com coluna e circunflexo, `ltrimstr` em não string virou erro, `limit` negativo virou
  erro, `trim`/`ltrim`/`rtrim` existem, `-0`, `--indent 0`. As 10 falhas nas suítes são 9 mensagens
  de `%%FAIL` nesse formato novo e uma de captura vazia no `onig.test`.
- **jaq com a camada nossa**: o resto da distância é do núcleo do jaq, não da CLI. Das 119 falhas nas
  suítes: 57 de mensagens (o `jaq_core::Error` é opaco e o texto que chega ao `catch` é o do jaq:
  `cannot use 123 as iterable`, `cannot calculate`; o `?//` e a desestruturação `{$a, $b: [...]}` não
  existem no parser), 22 de semântica de caminhos (o jaq 3 não cria estrutura ao atribuir em `null`
  nem além do fim do array: `null | .a.b = 1`, `setpath`, `del` fora do limite), 16 outras
  (`builtins`, `fromstream`, `pick` em array, inteiros grandes exatos onde o jq usa double), 11 de
  números (`1/0` dá infinito em vez de erro, índice fracionário, nan em `%`), 10 de regex (o
  `regex-bites` não tem lookaround, classes Unicode nem a forma das capturas do Oniguruma) e 3 de
  datas (mensagens do `jiff`). A camada fecha CLI, formatação, leitura e posição (0 falhas de CLI nos
  casos de agente), mas o que sobra só fecha com fork do `jaq-core` (atualização de caminhos,
  parser), do `jaq-json` (mensagens, divisão por zero, índice fracionário) e troca do motor de regex
  do `jaq-std`.
- **Builtins**: o jaq expõe 36 nomes que o jq 1.7.1 não tem (`trim`, `ltrim`, `rtrim`, `toboolean`,
  `null`, `true`, `false`, `tobytes`, `isarray`, `add/1`, `skip/2`, ...) e não tem 21 que o jq tem; a
  camada repõe 14 e faltam 7 (`builtins`, `fromstream`, `get_*`, `lgamma_r`, `modulemeta`). Esconder
  os extras exige varrer a AST do programa antes de compilar (a API de parse do jaq é pública).
- **Pontos de host do jaq** (depscan b): `jaq-core` tem `Loader::with_std_read` (`std::fs`,
  `std::env`, só pra módulos, não usado); `jaq-std` tem `env` (`std::env::vars`), `now`
  (`SystemTime`), `stderr`/`debug` (via `log`) e as nativas de fuso local via `jiff`
  (`TimeZone::system`). A camada sobrescreve `env`, `now`, `stderr`, `debug` e `input*`;
  `localtime`, `strflocaltime` e `mktime` ainda precisam ser sobrescritas pra ler o fuso do `Ctx`.
- **xq**: VM própria com Oniguruma, boa nas suítes (87,4%), mas a CLI não tem `--arg`, `--argjson`,
  `-e`, `--tab`, ... (42 das 61 falhas de CLI) e as mensagens são outras.
- **tq**: processador de TOON "compatível com jq", forçado a JSON com `-i json -o json`; semântica
  longe (56%).

## Resultado

### Checkpoint (alimenta o H07)

| Sondagem | Programa | Resultado | Latência até desenrolar | Maior intervalo entre checkpoints |
|---|---|---|---|---|
| jaq, iterador de saída embrulhado, sem gancho | `last(range(1e18))` | não cede, morto aos 3 s | | |
| jaq + `DataT` com checkpoint | `last(range(1e18))` | interrompido | 0,17 ms | 0,04 ms |
| jaq + `DataT` | `[limit(1e18; repeat(1))] \| length` | interrompido | 0,33 ms | 0,04 ms |
| jaq + `DataT` | `def f: f; f` | interrompido | 0,16 ms | 0,03 ms |
| jaq + `DataT` | `[range(1e18)] \| length` | não cede, morto aos 3 s | | |
| jaq + `DataT` + `range/3` nossa | `[range(1e18)] \| length` | interrompido | 3,6 ms (inclui liberar o array) | 0,15 ms |
| qj, núcleo em processo | `last(range(1e18))` | não cede, morto aos 3 s | | |
| bashkit jq, cancelamento + prazo de 1 s | `last(range(1e18))` | não cede, morto aos 3 s | | |
| bashkit awk, cancelamento | `BEGIN { while (1) x++ }` | interrompido | 0,05 ms | |
| bashkit awk, cancelamento | `BEGIN { while (1) {} }` | não cede, morto aos 3 s | | |
| bashkit awk, limites padrão | `BEGIN { while (1) x++; print x }` | termina sozinho imprimindo `10000` (laço truncado em silêncio) | | |
| rawk-core | `BEGIN { while (1) x++ }` | não cede, morto aos 3 s | | |
| awk-rs (biblioteca) | `BEGIN { while (1) x++ }` | não cede, morto aos 3 s | | |
| awk-rs, `Read` nosso checando a flag | `{ n++ }` com entrada infinita | interrompido no próximo registro | 0,01 ms | |

Leitura:

- **No jaq o checkpoint não precisa de fork.** O `lut()` do `DataT` é chamado a cada nó; o que escapa
  são nativas que consomem geradores nativos sem avaliar nó (`[range(N)]`, e as nativas
  `first`/`last`/`limit` do próprio jaq com `range`). A camada fecha isso com duas coisas que ela já
  faz: as definições do jq pra `first`, `last` e `limit` (avaliam nó por elemento) e uma `range/3`
  nossa que passa pelo checkpoint. O resto das nativas com laço interno trabalha sobre dados finitos
  já em memória (`sort`, `add`, `implode`), limitados pela contabilidade de memória.
- **Custo**: no melhor de 9 rodadas, `reduce range(1000000) ...` leva 263,1 ms no jaq puro e 266,6 ms
  com a camada e o checkpoint ligado (+1,3%, com 5 milhões de checkpoints); a leitura da flag em si
  custa 0,4% (contador sozinho 265,7 ms). A latência e o intervalo variam um pouco entre execuções
  (a máquina é compartilhada); a ordem de grandeza não.
- **Ninguém mais tem gancho no laço da VM**: qj, rawk-core e awk-rs só param matando o processo; o
  awk-rs cede só em E/S (pelo `Read`/`Write` nosso). O bashkit tem cancelamento por ação no awk, mas
  um laço sem ação no corpo gira pra sempre, e no jq ele só olha o prazo a cada valor emitido (o
  próprio código dele diz isso: "jaq evaluation is a synchronous iterator that the async execution
  timeout cannot preempt"). Os binários (frawk, zawk e awkrs com JIT, uutils/awk, xq, tq) estão na
  mesma situação.

## Veredito

### H26: Confirmada

Melhor awk em Rust: awkrs, com 67,7% estrito (74,4% frouxo) nos 586 casos combinados; nenhum passa
de 80%. Nos casos de agente isolados o awkrs chega a 89,7%, mas é (c), tem 196 pontos de host no
código próprio e não tem onde pôr checkpoint sem fork; todos os outros ficam abaixo de 55%. Os casos
de leftmost-longest falham em todos.

**Fazer à mão**: interpretador de awk próprio, categoria (a), com:

- o motor de regex POSIX do F01 (leftmost-longest, BRE/ERE do GNU);
- a ordem do for-in do gawk portada (agentes imprimem arrays sem `sort`);
- a arquitetura do uutils/awk como referência (VM que suspende em toda E/S e devolve o pedido pro
  driver, que aqui é o `Ctx`), com checkpoint a cada N instruções no laço de despacho;
- esta bancada como teste de aceitação: os 234 casos de agente e os 352 da suíte do gawk, com o
  awkrs como segunda referência de semântica gawk.

### H27: Parcial

Nas suítes do jq 1.7.1 (723 testes que o jq do Debian passa), jaq-core com a camada nossa passa 83,5%
e o binário do jaq 3.1.1, 79,4%; nos casos de CLI de agente, a camada nossa passa 86,6% estrito e
89,2% frouxo, contra 56,2% estrito do binário. A camada resolve CLI, números, leitura e mensagens de
topo; o que falta é semântica do núcleo do jaq (mensagens dentro de `try/catch`, atualização de
caminhos em `null` e além do fim, `?//`, divisão por zero, índice fracionário, regex sem lookaround).
O checkpoint encaixa sem fork (`DataT::lut` + `range/3` nossa), com custo de cerca de 1%.

**Recomendação** (há um trade-off real; as duas opções com a consequência):

1. **qj como núcleo do jq (recomendada)**: 98,6% nas suítes e 95,9% frouxo na CLI do jeito que
   está, núcleo com I/O por trait e driver em memória, licença MIT. Custo: categoria (c) (Oniguruma
   em C, que é o mesmo motor do jq e dá a semântica de regex exata; `strptime.c` e libc pra
   tempo/locale), unsafe interno de FFI, projeto novo de um autor só. Trabalho nosso: a CLI de jq 1.7.1
   desta camada (opções, leitura, exits, nome do programa) por cima do núcleo do qj; fixar os
   comportamentos 1.7.1 que o 1.8.1 mudou (lista nas notas); e um checkpoint no laço da VM
   (`jq_next`), que hoje exige patch de poucas linhas (melhor como contribuição upstream do que como
   fork nosso, pra não trazer o unsafe dele pro nosso código).
2. **jaq + camada nossa**: (a) depois de sobrescrever também as nativas de fuso local, sem unsafe
   (`jaq-core` tem `forbid(unsafe_code)`), checkpoint sem fork, projeto maduro. Custo: fica
   em 83,5%; fechar o resto exige fork espalhado em `jaq-core` (parser e atualização), `jaq-json`
   (mensagens e aritmética) e troca do regex do `jaq-std`.

Com o princípio 1 do design (comportamento igual ao Debian byte a byte), a opção 1 chega lá com
trabalho localizado e a 2 não; se o Oniguruma em C dentro do processo host for vetado, a 2 é o
caminho, com a lista de divergências acima como dívida conhecida.

## Arquivos

- `src/main.rs`: orquestração, agregação, vereditos e testes.
- `src/exec.rs`: candidatos binários em subprocesso e placar paralelo.
- `src/tools.rs`: versões pinadas e instalação dos binários.
- `src/gawk_suite.rs`: seleção, fixture e validação da suíte do gawk.
- `src/jqtest.rs`: parser e execução das suítes do jq.
- `src/jq/`: a camada nossa sobre o jaq (`cli`, `json`, `input`, `errors`, `engine`).
- `src/awk_inproc.rs`, `src/bashkit_cand.rs`: candidatos em processo.
- `src/checkpoint.rs`: sondagens e custo do checkpoint.
- `src/scan.rs`: depscan com categoria refinada.
- Detalhe caso a caso de cada candidato: `scratch/f04-awk-jq/details.json` (gitignored).
