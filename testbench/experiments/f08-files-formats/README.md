# F08 files-formats: diff/patch, compressão e arquivos, date, bc/file/yq/csv

Experimento da onda de userland que cobre F08 (diff/patch), F09 (compressão e arquivos), F10 (datas) e
F11 (diversos) do plano. Um binário só refaz tudo e grava `results/f08-files-formats.json`.

```sh
cd testbench
cargo run -q -p oracle -- gen --tool diff      # idem patch, archive, date, bc, file, yq, csv
cargo run --release --manifest-path experiments/f08-files-formats/Cargo.toml
cargo test --release --manifest-path experiments/f08-files-formats/Cargo.toml
```

Pra desenvolver uma parte só: `cargo run --release --bin part-h30` (ou `part-h31`, `part-h32`,
`part-h33-bc-file`, `part-h33-yq-csv`), que grava o resultado parcial em `scratch/f08-files-formats/`.

## Hipóteses

| Id | Frase | Critério |
|---|---|---|
| H30 | similar gera unified diff idêntico ao GNU | Byte a byte contra `diff -u` num corpus com empates de alinhamento, arquivos sem newline final e binários. |
| H31 | As crates de compressão e arquivo servem em Rust puro e interoperam com as ferramentas GNU | Matriz de interoperabilidade nos dois sentidos com gzip, bzip2, xz, zstd, tar (ustar, gnu, pax) e zip do oráculo; depscan sem C. |
| H32 | date com formatos e parsing do GNU sai de crates prontas | Conformidade de `+FORMAT` e de `date -d` com relógio e TZ fixos. |
| H33 | bc, file, yq e csv têm crate que encaixa | Conformidade de cada candidato no golden correspondente. |

## Método

Regra comum: o front-end de cada ferramenta (opções, mensagens, exit codes, leitura da fixture em
memória) é nosso e igual pra todos os candidatos; o que muda é só a biblioteca. Assim a divergência
medida é da biblioteca, e o tamanho do front-end vira a medida do "à mão". Tudo é comparado byte a byte
contra o golden do oráculo (Debian 13: diffutils 3.10, patch 2.8, coreutils 9.7, tar 1.35, gzip 1.13,
bzip2 1.0.8, xz 5.8.1, zstd 1.5.7, zip 3.0/unzip 6.0, lzip 1.25, file 5.46, bc 1.07.1, yq 3.4.3 com jq
1.7.1, tzdata 2026c). `strict` exige stdout, stderr, exit e árvore final iguais; `lenient` aceita stderr
diferente.

- **H30 (diff, patch).** Corpus manual: 107 casos de diff (formatos normal, `-u`, `-U n`, `-c`, `-e`,
  `-y`, `-q`, `-s`, rótulos, vazios, sem newline final, binários, CRLF, bytes não UTF-8, stdin,
  diretórios com `-r`/`-N`, `-i`/`-w`/`-b`/`-B`, 21 casos de empate de alinhamento) e 41 de patch
  (limpo, `-p`, stdin, criação, remoção, vários arquivos, formato git, newline final, `--dry-run`, `-o`,
  `-b`, `-R`, deslocamento, fuzz, empate entre posições, rejeição parcial, patch já aplicado com `-N`,
  `-t` e sem flag, CRLF, formatos de contexto e normal, erros). Corpus aleatório gerado com semente fixa
  e rodado no oráculo em tempo de execução: 800 pares de arquivos (alfabetos de 2 a 4 linhas, linhas de
  código repetitivas, texto, arquivos de 200 a 600 linhas) comparados no formato normal (que expõe o
  alinhamento) e no `-u`; e 500 aplicações de patch em que o GNU gera o `diff -u` e aplica num alvo
  limpo, deslocado, com uma linha alterada, com as duas coisas, ou já aplicado. Cada motor de
  alinhamento entra com o formatador GNU nosso; cada crate que tem formatador próprio entra também do
  jeito que vem. Velocidade num par de 20 mil linhas e num repetitivo de 3 mil, contra o GNU no container.
- **H31 (compressão e arquivos).** Corpus determinístico (vazio, 1 byte, log de 1 MB, registros binários,
  aleatório). O oráculo comprime em 33 variantes (níveis, checksums, multi-membro, multi-bloco, BCJ/delta,
  stream sem tamanho) e monta tar em 5 formatos e zip em 3 métodos a partir de uma árvore com setuid,
  sticky, symlink, hardlink, Unicode e nomes longos; cada crate lê. No sentido contrário, cada crate
  produz e o GNU testa, descomprime, lista e extrai. Taxa e velocidade no nível padrão, depscan de cada
  crate, e um shim de CLI nosso (`tar tv`, `gzip -l`, `unzip -l`, mensagens) contra o golden de
  `corpus/cases/archive`.
- **H32 (date).** 264 casos com relógio congelado (`FAKETIME` absoluto + `LD_PRELOAD` da libfaketime),
  fusos UTC, America/Sao_Paulo, America/New_York (perto da troca de horário), Europe/Berlin,
  Asia/Kolkata, Australia/Lord_Howe e TZ POSIX; todas as diretivas do `+FORMAT` com flags e largura,
  `-d` com relativos, `@epoch`, ISO 8601, RFC 2822, horário inexistente, datas inválidas, `-u`, `-R`,
  `-I`, `--rfc-3339`, `-r`, `-f`.
- **H33 (bc, file, yq, csv).** bc: 118 casos (estilo agente e suíte do posixutils-rs) contra o GNU bc
  1.07.1. file: fixtures binárias determinísticas nos modos `file`, `-b`, `--mime-type`, `-i`, `-L`.
  yq: 105 casos contra o yq 3.4.3 do kislyuk (PyYAML + jq) e a YAML test suite (402 testes) como métrica
  separada dos parsers. csv: `cut -d,` e `column -t -s,` em CSV sem aspas e o módulo `csv` do Python 3.13
  do oráculo (dependência do yq) como referência de RFC 4180; não há ferramenta GNU de CSV.

## Candidatos

### H30: diff e patch

- Motores de alinhamento, todos com o front-end e o formatador GNU nossos: `similar` 3.2 (Myers,
  RawMyers, Patience, Histogram), `imara-diff` 0.2 (Myers cru, Myers e MyersMinimal com
  `postprocess_no_heuristic`, Histogram com `postprocess_lines`), `diffy` 0.5 (Myers) e a crate `diff`
  0.1.13, que é o LCS que o uutils `diffutils` 0.5 usa por dentro.
- Formatadores próprios, com o nosso front-end em volta: `similar` `unified_diff`, `diffy`
  `create_patch`, `diffutils` 0.5 (`diffutilslib`: normal, unificado, contexto, ed, lado a lado) e o
  `BasicLineDiffPrinter` do `imara-diff`.
- patch: `diffy` 0.5 (`Patch::from_bytes` + `apply_bytes`), `flickzeug` 0.6 (fork do diffy com
  `apply_bytes_partial` e fuzz, similaridade 1.0) e um localizador nosso sobre o parser do diffy. O
  front-end converte contexto e normal pra unificado, então os três motores recebem os mesmos hunks.

### H31: compressão e arquivos

- gzip: `flate2` 1.1.10 com `miniz_oxide` e `zlib-rs` 0.6.8. As features do flate2 se unificam no grafo,
  então não dá pra ter os dois backends no mesmo binário: o zlib-rs foi testado pela API segura
  `Deflate`/`Inflate` em modo raw, com o enquadramento gzip feito por nós, que é o mesmo que o flate2 faz
  com a feature `zlib-rs`.
- bzip2: `bzip2` 0.6 com `libbz2-rs-sys` (Rust puro).
- xz, lzma e lzip: `lzma-rust2` 0.21, `lzma-rs` 0.3 e `xz4rust` 0.2 (só decodifica).
- zstd: `ruzstd` 0.9 e `structured-zstd` 0.0.58 (fork mantido do ruzstd com todos os níveis).
- Arquivos: `tar` 0.4.46 sem default features e `zip` 8.6 sem default features (deflate-flate2, bzip2,
  xz). A feature zstd do zip ficou de fora porque puxa C.

### H32: date

O front-end do `date` é nosso e igual pra todos: getopt_long com permutação, `-d`, `-f`, `-r`, `-u`, `-R`,
`-I`, `--rfc-3339`, mensagens no formato do GNU, e o "agora" e o TZ vindos do caso (nunca do host). Só
mudam o parser do `-d` e o strftime com o banco de fusos.

1. `parse_datetime` 0.16 + `jiff` 0.2, com o tzdb embutido (`jiff-tzdb`, 2026c, igual ao do oráculo).
2. `interim` 0.2 + `chrono` 0.4 + `chrono-tz` 0.10 (tzdb 2025b).
3. `parse_datetime` 0.11, a última versão sobre chrono, + `chrono`/`chrono-tz`: alternativa do lado
   chrono, porque o interim não tem a gramática do GNU.
4. `parse_datetime` 0.16 + strftime do chrono: só referência, pra atribuir cada divergência ao parser ou
   ao formatador.

### H33: bc, file, yq e csv

- bc: nenhuma implementação existe como biblioteca no crates.io. Candidatos reais: o bc do posixutils-rs
  (git 4073af04, pacote `posixutils-calc`, motor `calc/bc_util`) e o bc_clone_rs (git d0e3f445, motor
  `bc_core` no_std), clonados numa revisão fixa em `corpus/upstream/bc/`, compilados como binário e
  rodados pelo mesmo caminho do oráculo. Descartados por evidência do fonte: a crate `bc` 0.1.17 (monta um
  `Command` do executável `bc` do sistema) e o builtin do bashkit 0.18.2 (f64, sem `define`, `ibase` nem
  laços); clones pequenos do GitHub triados e descartados.
- file: `pure-magic` 0.4.1 + `magic-db` 0.6.0 (`first_magic` e `best_magic`), `libmagic-rs` 0.12.6
  (regras embutidas e magdir do magic-db em texto), `infer` 0.22 e `file-format` 0.29 só no MIME. O
  front-end do `file` (opções, symlink, diretório, vazio, alinhamento dos nomes, charset do `-i`) é nosso e
  igual pra todos; a camada ascmagic nossa (~150 linhas: codificação, terminadores de linha, linhas
  longas) é medida à parte. O `file` do posixutils-rs foi descartado sem rodar (saída POSIX, lê o magic do
  host).

- yq, filtro: `jaq-core`/`jaq-std`/`jaq-json` 3.x. Front-end nosso e igual pra todos: argumentos do yq,
  conversão YAML pra JSON com a semântica do `json.dumps` do Python (inclusive chave 1 == True) e impressão
  do jq 1.7.1 (forma canônica do decNumber pra literais, dtoa pra números calculados).
- yq, camada YAML: `jaq-fmts` 0.1, `saphyr` 0.1, `serde-saphyr` 1.3 (configurada pra ficar perto do yq),
  `yaml-rust2` 0.13, `noyalib` 0.0.51 e uma camada nossa sobre o `saphyr-parser` 0.1 (resolvedor de
  escalares do yq, 252 linhas). Motor alternativo: `yqr` 0.8 (gramática própria sobre o noyalib).
  `serde_yml` (RUSTSEC-2025-0068) e libyaml ficaram de fora.
- csv: crate `csv` 1.4, com front-ends nossos imitando `cut -d,`, `column -t -s,` e a saída JSON do leitor
  do Python.

## Resultado

### H30: diff e patch

Motores de alinhamento com o formatador GNU nosso (corpus manual de 107 casos; 800 pares aleatórios no
formato normal e no `-u`; tempo do alinhamento num par de 20 mil linhas com 1% de edições e num de 3 mil
linhas repetitivas, contra 3,8 ms e 1,4 ms do GNU diff medidos no container):

| Motor | Corpus | Aleatório normal | Aleatório `-u` | Mesmo custo, outro alinhamento | Script mais longo | ms 20k / 3k |
|---|---|---|---|---|---|---|
| imara-diff Myers + `postprocess_no_heuristic` | 106/107 | 756/800 | 790/800 | 43 | 1 | 0,49 / 0,13 |
| imara-diff MyersMinimal + `postprocess_no_heuristic` | 106/107 | 756/800 | 790/800 | 43 | 1 | 0,62 / 0,15 |
| imara-diff Histogram + `postprocess_lines` | 105/107 | 667/800 | 689/800 | 58 | 75 | 1,10 / 0,22 |
| imara-diff Myers cru | 106/107 | 656/800 | 630/800 | 143 | 1 | 0,39 / 0,13 |
| diff 0.1 (LCS do diffutils) | 105/107 | 656/800 | 631/800 | 144 | 0 | tabela de 1,5 GiB / 23,7 |
| similar Myers | 101/107 | 642/800 | 640/800 | 158 | 0 | 1,83 / 0,20 |
| similar RawMyers | 101/107 | 642/800 | 640/800 | 158 | 0 | 0,40 / 0,22 |
| diffy Myers | 101/107 | 635/800 | 658/800 | 165 | 0 | 9,9 / 1,7 |
| similar Patience | 101/107 | 623/800 | 630/800 | 152 | 25 | 15,1 / 0,70 |
| similar Histogram | 99/107 | 614/800 | 612/800 | 166 | 20 | 82,1 / 46,6 |

Formatadores próprios das crates (front-end nosso em volta):

| Formatador | Corpus | Casos que suporta | Aleatório `-u` |
|---|---|---|---|
| similar `unified_diff` | 63/107 | 63/64 | 640/800 |
| diffy `create_patch` (cabeçalho trocado) | 60/107 | 60/64 | 534/800 |
| diffutils 0.5 (normal, -u, -c, -e, -y) | 83/107 | 83/101 | 420/800 |
| imara-diff `BasicLineDiffPrinter` | 38/107 | 38/64 | 438/800 |

- O formatador do similar acerta tudo que é formatação (faixas `-l` e `-l,n`, `-l,0`, `\ No newline at end
  of file`, agrupamento de hunks com distância de 2n linhas); a única falha no corpus manual e as 160 no
  aleatório são de alinhamento: o Myers do similar prefere inserir antes de apagar nos empates.
- diffy põe o nome do arquivo entre aspas e escapa o tab quando o rótulo tem data, e não tem normal, `-c`
  nem `-e`. diffutils 0.5 lê o mtime do disco do host pro cabeçalho (`std::fs::metadata` +
  `chrono::Local`), escreve faixa redundante no formato normal (`3a4,4`) e alinha por LCS com tabela
  O(N·M) de `u32`. O printer do imara-diff só aceita UTF-8, escreve a faixa sempre com contagem e não
  emite `\ No newline at end of file`; a própria documentação manda escrever um printer sobre `hunks()`.
- Todas as divergências de alinhamento são scripts válidos e mínimos (zero inválidos, quase zero mais
  longos pros Myers): o GNU desempata com o `compareseq` do gnulib (`diffseq.h`) e depois desliza os
  blocos (`shift_boundaries`), os dois GPLv3. O pós-processamento do imara-diff (portado do git, Apache)
  reproduz o deslizamento; o que sobra (44 de 800) é desempate do miolo do Myers.
- Front-end e formatador nossos que todos os candidatos usaram: 507 linhas (`cli.rs`: opções, fixture,
  binário, diretórios, cabeçalhos, erros), 358 (`gnu_format.rs`: normal, `-u`, `-c`, `-e`, `-y`) e 79
  (`text.rs`: `-i`, `-w`, `-b`, `--strip-trailing-cr`). Falhas do front-end no corpus: nenhuma.

patch:

| Motor | Corpus estrito | Corpus leniente | Aleatório leniente | Aleatório, só conteúdo e exit |
|---|---|---|---|---|
| localizador nosso sobre o parser do diffy | 40/41 | 41/41 | 500/500 | 500/500 |
| flickzeug 0.6 `apply_bytes_partial` | 30/41 | 31/41 | 332/500 | 480/500 |
| diffy 0.5 `apply_bytes` | 29/41 | 30/41 | 343/500 | 421/500 |

- diffy aplica tudo ou nada (um hunk ruim derruba o arquivo), não tem fuzz, desempata pra trás (o GNU
  tenta pra frente primeiro) e não informa posição nem deslocamento, então "Hunk #n succeeded at L
  (offset k lines)" e o `.orig` não saem.
- flickzeug aplica parcial e tem fuzz, mas também não informa posição, deslocamento nem fuzz usado,
  desempata pra trás e aplica patch LF em arquivo CRLF (o GNU recusa com "different line endings").
- O localizador nosso reproduz o comportamento observável do GNU patch 2.8: deslocamento pra frente antes
  de pra trás, fuzz cortando contexto das pontas, âncora no começo ou no fim quando o contexto do hunk é
  assimétrico, detecção de patch invertido por nível de fuzz, posição informada em coordenadas do arquivo
  novo, `.orig` em qualquer desencontro, `.rej` com o cabeçalho original e as faixas deslocadas pelo
  saldo já aplicado, saída do `-o` com modo 0600. A única diferença no corpus é o texto do erro de patch
  malformado. Front-end, conversão de contexto/normal, localizador e os três adaptadores: 980 linhas
  (`patch.rs`).
- O parser do diffy (unificado e git) serviu sem ajuste; o `PatchSet` dele também reconhece criação,
  remoção, renomeação e modo do git.
- depscan: similar e diffy (a), imara-diff e flickzeug (b) só por testes e pelo módulo `fs` do flickzeug,
  que não usamos; diffutils (c) por dependências de outros alvos do chrono (`iana-time-zone-haiku`,
  `wasm-bindgen-shared`) e (b) pelo cabeçalho que lê o disco. Nenhuma linka C no Linux.

### H31: compressão e arquivos

| Crate | GNU produz, crate lê | Crate produz, GNU lê | Saída / GNU (texto, nível padrão) | Encaixe |
|---|---|---|---|---|
| flate2 (miniz_oxide) | 15/15 | 15/15 | 0,98 | encaixa |
| zlib-rs | 15/15 | 15/15 | 1,02 | encaixa |
| bzip2 0.6 (libbz2-rs-sys) | 9/9 | 10/10 | 1,00, byte a byte igual ao bzip2 1.0.8 no nível 9 | encaixa |
| lzma-rust2 (xz, lzma, lzip) | 23/23, 9/9, 13/13 | 45/45 | 1,00 | encaixa |
| structured-zstd | 15/15 | 15/15 | 1,00 | encaixa |
| ruzstd | 15/15 | 5/5 | 1,39 (só tem o nível Fastest) | não encaixa |
| lzma-rs | 15/23 no xz | 10/10 | 5,45 no xz (não comprime), 3,1 no lzma | não encaixa |
| xz4rust | 21/23 | só decodifica | não se aplica | não encaixa |

- lzma-rs falha em SHA-256, BCJ e delta; xz4rust devolve só o primeiro de dois .xz concatenados, sem erro.
- tar 0.4 encaixa com trabalho: lê sem divergência ustar, gnu, oldgnu, pax e v7 do GNU (atributos, links e
  conteúdo), e o GNU lê e extrai os nossos ustar, gnu e pax sem mensagem de erro (22/22 entradas, inode do
  hardlink conferido). Com cabeçalho ustar e nome longo o Builder grava `././@LongLink` (extensão GNU)
  sem avisar; o pax só sai montando os registros com `append_pax_extensions`; a extração sobre o VFS é
  nossa, porque o `unpack` usa `std::fs`.
- zip 8 lê os zips do Info-ZIP (13/13); o unzip lê os nossos em stored, deflate e bzip2. O método xz o
  unzip 6.0 recusa (exit 81), por limitação do próprio unzip.
- Velocidade no nível padrão, no log de 1 MB (MB/s, compressão / descompressão; crate medida no processo,
  GNU num laço dentro do container descontando 3,8 ms de criar processo; a máquina tinha outros
  experimentos rodando, então vale a ordem de grandeza, não a casa decimal):

  | Formato | Crate | GNU |
  |---|---|---|
  | gzip | flate2/miniz_oxide 30,7 / 589; zlib-rs 68,8 / 1131 | 26,5 / 185 |
  | bzip2 | bzip2 (libbz2-rs-sys) 20,6 / 61,8 | 11,3 / 11,2 |
  | xz | lzma-rust2 2,9 / 46,5 | 1,8 / 88,7 |
  | zstd | structured-zstd 110 / 632; ruzstd 44 / 145 | 55,1 / 157 |
- depscan: nenhuma das 12 crates tem C. A categoria (b) vem de código fora do caminho usado (unpack em
  `std::fs`, src/bin, testes, a ABI C do libbz2-rs-sys).
- O shim de CLI nosso (`tar tv/t/x`, `gzip -l/-lv/-d/-t`, `zcat`, `xz/bzip2/lzip/zstd -dc/-t`,
  `unzip -l/-Z1/-p/-q`) fica 100% estrito no golden de `archive`, inclusive nas mensagens de erro.

### H32: date

| Candidato | Total | Diretivas | `+FORMAT` com flags | `-d` | Fusos | Encaixe |
|---|---|---|---|---|---|---|
| parse_datetime 0.16 + jiff | 240/264 | 59/59 | 74/83 | 119/134 | 47/58 | encaixa com trabalho |
| parse_datetime 0.11 + chrono | 206/264 | 58/59 | 61/83 | 103/134 | 34/58 | não encaixa |
| interim + chrono | 173/264 | 58/59 | 61/83 | 70/134 | 32/58 | não encaixa |

Divergências do candidato 1, atribuídas por troca de componente:

- Formatador (10 casos): não aceita flags combinadas (`%-^b`), o flag `+` nem `%E`/`%O`; ignora largura em
  `%A`, `%z` e `%s`; `#` e `^` em `%P` diferem; `%s` trunca pra zero em instante negativo fracionário.
- Parser (11 casos): aceita horário inexistente na troca de horário de verão (o GNU rejeita); aceita
  "2 days agoo" lendo o "o" como fuso militar; não lê "+3" depois da hora como fuso; em hora ambígua não
  herda o horário de verão do "agora"; relativos atravessando a troca dão outro resultado; `-d ""`
  devolve o agora (o GNU devolve meia-noite).
- Fuso (3 casos): o jiff põe "Foo" em maiúsculas; e o Debian não tem os nomes legados (`US/Eastern`,
  `Universal`, pacote tzdata-legacy), que o tzdb embutido tem.
- Nenhuma falha vem do front-end.

Acoplamento ao host: o `parse_datetime` liga `tz-system` e `tzdb-zoneinfo` do jiff, e o `TZ="..."` dentro
do `-d` é resolvido pelo banco global, que lê `/usr/share/zoneinfo`; o front-end separa esse prefixo e
resolve pelo `TimeZoneDatabase::bundled()`. O depscan acusa C só em dependências de outros alvos
(`cargo tree --target x86_64-unknown-linux-gnu` não traz nenhuma); a categoria (b) vem do `tz-system` do
jiff e do `iana-time-zone` do chrono. No lado chrono faltam ao formatador `%N`, os flags `^` e `#` e
largura; o chrono-tz não entende TZ POSIX.

Nota de método: o campo `faketime` do harness não congela a fração de segundo (o wrapper herda a fração
real, medido 12:00:00.79, 12:00:00.92), então `%S` às vezes vira o segundo seguinte; os casos de date usam
`FAKETIME` absoluto + `LD_PRELOAD` da libfaketime.

### H33: bc, file, yq e csv

bc (118 casos: 68 estilo agente e 50 da suíte do posixutils-rs, contra o GNU bc 1.07.1):

| | posixutils-rs | bc_clone_rs |
|---|---|---|
| Estrito | 73/118 | 91/118 |
| Com o zero à esquerda corrigido (estimado, sem rodar o fork) | 92/118 | 91/118 |
| Núcleo numérico | 49/69 (67/69 com a correção) | 65/69 |
| Extensões GNU | 0/11 | 0/11 |
| Casos que terminam em panic | 0 | 13 |
| Motor | 5504 linhas, sem host, sem unsafe | `bc_core` no_std, categoria (a) |

- posixutils-rs: 19 falhas são só "0.5" no lugar de ".5", uma linha no formatador. O pacote cai na
  categoria (c) porque depende do `plib`, que compila C, mas o motor `calc/bc_util` não toca o host.
- bc_clone_rs: o motor transforma erro em `panic!`.
- A base de fork é escolhida pelo código (mais casos estritos com o zero corrigido, desempate por menos
  panic); hoje dá posixutils-rs, por um caso. Extensões GNU, mensagens de erro e CLI ficam com a gente.

file (89 casos contra o file 5.46):

| | Descrição | `--mime-type` |
|---|---|---|
| pure-magic + magic-db | 28/48 | 28/29 |
| pure-magic + magic-db + ascmagic nosso | 42/48 | 28/29 |
| libmagic-rs com magdir + ascmagic nosso | 38/48 | 17/29 |
| libmagic-rs com regras embutidas | 15/48 | 15/29 |
| infer, file-format (só MIME) | sem descrição | 24/29 |

- Com a camada ascmagic sobram: data em ISO no gzip e no zip (bug do pure-magic), `-z` não implementado,
  CSV e PDF. As regras do magic-db vêm do file de 2026, mais novas que as do 5.46, então parte do que
  sobra pode ser regra nova, não motor.
- O libmagic-rs não é determinístico no MIME: o HTML alterna entre `text/html` e `text/plain` de uma
  execução pra outra (busca num HashMap estático, `src/mime.rs:158`); o placar dele varia em um caso entre
  execuções.
- O depscan marca (c) por causa do blake3 (só no proc-macro `magic-embed`, em tempo de build) e de crates
  de outras plataformas; nada disso vai pro binário Linux (`c_deps_linked_on_linux` vazio no JSON).

yq (105 casos; núcleo = sem formas YAML 1.1, sem `-y` e sem casos de erro):

| Candidato yq | Estrito | Leniente | Núcleo | `-y` |
|---|---|---|---|---|
| jaq + saphyr-parser + resolvedor nosso | 92/105 | 98 | 74/74 | 6/12 |
| jaq + noyalib | 84 | 90 | 71/74 | 5/12 |
| jaq + yaml-rust2 | 83 | 89 | 67/74 | 6/12 |
| jaq + serde-saphyr | 83 | 89 | 67/74 | 7/12 |
| jaq + jaq-fmts | 82 | 88 | 69/74 | 4/12 |
| jaq + saphyr | 81 | 87 | 65/74 | 6/12 |
| yqr | 60 | 66 | 50/74 | 3/12 |

- O carregador do yq 3.4.3 resolve escalares pelo core schema do YAML 1.2 mas monta inteiros como o PyYAML
  1.1 (`012` vira 10, `08` dá ValueError) e expande merge keys na ordem do `flatten_mapping` do PyYAML;
  nenhuma crate reproduz isso, daí o resolvedor nosso.
- Divergências das crates no núcleo: saphyr devolve `""` pra valor e documento vazios; yaml-rust2 trata
  `Null`/`NULL` como string e recusa chave repetida; noyalib e saphyr param inteiro em 64 bits; só
  serde-saphyr e noyalib fazem merge key, e na ordem errada; jaq-fmts aceita `-0x10` e `0b101`; todas,
  menos noyalib, deixam o BOM na chave. yqr não tem `,`, `map`, `select`, `keys`, `--arg`.
- `-y`: nenhum emissor reproduz o PyYAML (aspas simples, `...` depois de escalar no topo, quebra em 80
  colunas, estilo preservado no `-Y`); o emissor fica com a gente. Sobram também mensagens de erro (6
  casos, só stderr) e a divisão por zero do jaq, que é assunto do F05.
- YAML test suite (402 testes, commit 6ad3d2c): noyalib 401, saphyr-parser + resolvedor 401, yaml-rust2
  399, jaq-fmts 398, serde-saphyr 388, saphyr 376; nenhum panic.
- depscan: nenhuma árvore tem C (o `defmt` na do jaq-fmts é falso positivo: `links` de namespace e a crate
  nem entra no build). O jaq-core toca o host em `unwrap_valr` (`process::exit`) e no loader de módulos,
  que não usamos.

csv: a crate `csv` 1.4 faz 30/36 estrito (30/33 fora dos casos em que `cut`/`column` divergem de CSV por
definição): `cut` 8/10, `column` 4/5, leitura contra o Python 14/16, escrita 4/5. As 3 divergências
restantes são política da crate: tira o BOM, pula linha vazia e escreve `""` pra registro vazio. Não há
oráculo GNU de CSV: a referência de RFC 4180 é o módulo `csv` do Python, que não é norma, e o que nem
`cut` nem Python cobrem (dialetos, `--delimiter` com aspas) fica sem oráculo.

## Veredito

| Id | Veredito | Número decisivo |
|---|---|---|
| H30 | Refutada | similar `unified_diff`: 63/64 casos `-u` do corpus manual, mas 640/800 pares aleatórios iguais ao `diff -u`; melhor motor (imara-diff Myers + `postprocess_no_heuristic`) 790/800 no `-u` e 756/800 no normal, sempre com script de mesmo custo |
| H31 | Confirmada | 6/6 papéis com crate Rust pura que interopera 100% nos dois sentidos, saída de 0,98 a 1,02 vezes a do GNU, sem C no binário |
| H32 | Parcial | parse_datetime 0.16 + jiff 0.2: 240/264 (90,9%), diretivas 59/59, `-d` 119/134; lado chrono 206/264 no máximo |
| H33 | Parcial | csv encaixa como está (30/33 nos casos com oráculo); yq, file e bc só com camada nossa ou fork |

O que encaixa, por papel:

- diff: `imara-diff` 0.2 (Myers + `postprocess_no_heuristic`) pro alinhamento; o resto é nosso. Ficar
  byte a byte igual em 100% dos empates exigiria reproduzir o desempate do `diffseq.h` do GNU, que é GPLv3:
  a decisão é aceitar ~1% de diferença no `-u` (scripts válidos e mínimos) ou escrever esse desempate a
  partir de especificação de comportamento.
- patch: parser do `diffy` 0.5; localização, aplicação, mensagens, `.orig` e `.rej` à mão (500/500 no
  aleatório e 41/41 leniente no corpus). Nem diffy nem flickzeug informam posição, deslocamento ou fuzz.
- gzip `flate2` (ou `zlib-rs`, duas vezes mais rápido), bzip2 `bzip2` 0.6, xz/lzma/lzip `lzma-rust2`,
  zstd `structured-zstd`, zip `zip` 8 sem default features: encaixam como estão. tar `tar` 0.4: encaixa,
  com extração pro VFS e pax por `append_pax_extensions` nossos.
- date: `parse_datetime` + `jiff` com tzdb embutido, mais uma camada nossa: strftime GNU completo por
  cima do jiff (flags combinadas, `+`, `%E`/`%O`, largura em tudo), separação do `TZ="..."` do `-d`,
  rejeitar horário inexistente e herdar o horário de verão do "agora".
- bc: fork do motor do posixutils-rs (ou do bc_clone_rs), com extensões GNU, erros e CLI nossos.
- file: `pure-magic` + `magic-db` com ascmagic nosso (42/48 descrições, 28/29 MIME).
- yq: `jaq` 3 + `saphyr-parser` com resolvedor de escalares do yq nosso (252 linhas) e emissor `-y` nosso.
- csv: crate `csv` 1.4 como está.

Fazer à mão (com evidência acima): o formatador e o front-end do `diff` (~950 linhas medidas aqui), a
localização do `patch` (~980 linhas com front-end e adaptadores), os CLIs de compressão e arquivo, o
strftime GNU do date, as extensões GNU do bc, a detecção de texto do `file`, o emissor YAML do yq.

Notas pra quem for usar o resultado:

- O campo `faketime` do harness não congela a fração de segundo; casos que dependem de `%S` com relógio
  correndo precisam de `FAKETIME` absoluto + `LD_PRELOAD` (ou `faketime -f`), como os de date fazem.
- O localizador de patch reproduz o comportamento observado no GNU patch (GPLv3); pra produção, manter
  a implementação a partir dessa especificação e dos testes, sem derivar do código GPL. O deslizamento de
  blocos do diff vem do próprio imara-diff (Apache-2.0).
- O placar do libmagic-rs varia em um caso entre execuções (MIME não determinístico da própria crate);
  o resto do JSON é determinístico, menos as velocidades.
