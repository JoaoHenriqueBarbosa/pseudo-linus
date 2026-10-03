# F15/F16/F17: shell, edição de linha, interpretadores e linhas de base

Um experimento, quatro hipóteses. O binário principal compila sozinho os crates auxiliares (seis
interpretadores, quatro linhas de base, a sonda de tty) numa invocação do cargo por grupo, roda as
quatro partes e grava `results/f15-shell-tty-interp.json`. Do zero (com `cargo clean --release` antes,
compilação inclusa, máquina carregada por outros agentes) levou 8 min 13 s; com tudo compilado, 2 min.

```sh
cd testbench
cargo run -q -p oracle -- gen --tool shell                       # golden do corpus de shell
cargo run --release --manifest-path experiments/f15-shell-tty-interp/Cargo.toml
cargo run --release --manifest-path experiments/f15-shell-tty-interp/Cargo.toml -- --only h37,h38
cargo test --release --manifest-path experiments/f15-shell-tty-interp/Cargo.toml \
  -p f15-shell-tty-interp -p f15-interp-common -p f15-baseline-common
```

`--only` grava em `target/f15-cache/partial-<partes>.json` em vez de `results/`. `--fresh` ignora o cache
do `bash -n` (que guarda só hash do script e códigos de saída; sem cache, o `bash -n` dos 48 mil scripts
leva uns 30 s no oráculo).

## Hipóteses

- **H37** (v1): o brush serve de base pro shell, adaptando a execução ao `Ctx`. Critério: brush-parser
  contra `bash -n` (aceita/rejeita igual) no corpus; depscan e esforço de fork do brush-core.
- **H38** (v1): `reedline` serve pra histórico e edição no shell interativo. Critério: refutada se o
  reedline não aceita I/O próprio; testar termwiz e noline sobre pty em memória com sequência de teclas.
- **H39** (v1): Python, Lua e JS embutíveis com todo I/O passando pelo kernel. Critério: pra Monty,
  RustPython, boa, rquickjs, piccolo e mlua, dá pra negar todo I/O de host e oferecer o nosso? Startup,
  tamanho, corpus de one-liners.
- **H40** (v2): projetos existentes (bashkit e afins) já cobrem parte relevante do plano. Critério: rodar
  os casos `script` do golden e dar a taxa por área (shell, coreutils, grep/sed/awk, jq).

## Método

### H37: brush-parser, AST em níveis, brush-core, yash-env

**Corpus de shell.** 108 casos `script` no estilo de agente em `corpus/cases/shell/` (seis arquivos:
aspas e here-docs, expansões, controle de fluxo, `set -e` e traps, arrays e redirecionamentos, diversos),
cobrindo: aspas e `$'...'`, here-docs com `<<EOF`, `<<'EOF'`, `<<-` (com tab de verdade) e vários na mesma
linha, `$(...)` aninhado e com `case` e here-doc dentro, aritmética (bases, overflow, erros), arrays
indexados e associativos (inclusive a ordem de iteração do hash do bash), `${var...}` em todas as formas
(`:-`, `:=`, `:?`, `:+`, `#`, `##`, `%`, `%%`, `/`, `//`, `/#`, `/%`, substring, `^^`, `,,`, `@Q`, `@A`,
`@a`, `@E`, indireção, `${!prefixo@}`), `[[ ]]` com `=~` e `BASH_REMATCH`, `case` com `;&` e `;;&`, funções,
`local`, `declare -n`, `set -euo pipefail` e as exceções do `-e` (if, `&&`/`||`, `!`, subshell, substituição
de comando com e sem `inherit_errexit`, `local` mascarando o status), `trap` EXIT/ERR/sinal, `read -r` em
`while` com IFS, process substitution, `2>&1`, `&>`, `<<<`, `exec {fd}>`, `{a,b}`, `{1..10}`, globbing
com nullglob/failglob/dotglob/globstar/extglob, `printf %q`, `declare -p`, `getopts`, subshell e grupo,
`$?`, `PIPESTATUS`, xtrace, coproc e jobs. O golden saiu do oráculo (bash 5.2.37 do Debian 13) e é
determinístico: nenhum caso imprime pid, data ou caminho variável (os casos com sinal escondem a mensagem
do bash, que traz o pid).

**Quatro conjuntos contra `bash -n`.** (1) o corpus de shell (108); (2) os scripts dos casos `script` de
todos os outros diretórios da bancada (65 na execução final); (3) a suíte de testes do bash 5.2.37
(`tests/*.tests` e `*.sub`, 474 arquivos baixados do tarball oficial, sha256 conferido, em
`corpus/upstream/bash/`); (4) os comandos minerados dos transcripts (`corpus/agent/commands.jsonl`: 47395
comandos únicos, 54707 chamadas, sem amostragem). Os comandos minerados **nunca são executados**: vão
como arquivos de dados pro container do oráculo (sem rede), onde só `bash -n` (que lê e parseia sem
executar) roda sobre eles, e daqui só saem contagens. Nenhum texto deles vai pro JSON: as classes de erro
do brush são cortadas antes de qualquer trecho citado (tag de here-doc, texto perto do erro).

Três combinações: bash padrão contra brush sem extglob; `bash -O extglob` contra brush com extglob (o
padrão do brush); e a mesma com o parse profundo decidindo (script que o brush aceita no primeiro nível
mas tem erro de sintaxe dentro de `$(...)`, numa palavra ou num here-doc conta como rejeitado, que é o que
o bash faz).

**AST em níveis.** O brush-parser guarda palavras e aritmética como texto cru (`ast::Word { value }`); o
conteúdo de `$(...)` só aparece num segundo parse (`word::parse` devolve
`WordPiece::CommandSubstitution(String)`), e vira programa num terceiro. O experimento implementa esse
re-parse recursivo como o nosso shell faria: toda palavra da AST passa por `word::parse`, cada `$(...)` e
cada crase (depois do desescape de `\\`, `` \` `` e `\$`, que o brush deixa pra quem consome) vira programa
de novo, palavras dentro de `${x:-...}` e `${x/.../...}` são re-parseadas, here-docs que expandem passam
por `word::parse_heredoc`, aritmética vai pro `arithmetic::parse` (e, quando tem `$` ou crase, antes é
tratada como palavra pra achar as substituições de dentro). Mede programas sem erro em nenhum nível,
profundidade de aninhamento, e confere 30 sondas construídas à mão com a profundidade esperada (inclusive
`$(...)` dentro de aritmética, de `${:-}`, de here-doc, de crase, de atribuição de array, de alvo de
redirecionamento, `$(< arquivo)`, e três inválidas que só o segundo nível enxerga).

**brush-core.** depscan da crate e da árvore; pontos de host, linhas, `async fn` e referências a tokio
por arquivo, pra estimar o fork.

**yash-env.** Só leitura de código (GPLv3: nem dependência, nem cópia): licença, traits públicas do
módulo `system` e tamanho da `VirtualSystem`.

### H38: reedline e rustyline (negativos), termwiz e noline (candidatos)

- **Evidência de código** com arquivo e linha, conferida em tempo de execução (se a crate mudar e o
  trecho sumir, o experimento falha em vez de registrar linha velha).
- **Sonda dinâmica** (`probes/tty-probe`): chama `Reedline::read_line` e `rustyline::Editor::readline` do
  jeito documentado, num processo com `setsid` (sem terminal de controle), stdin num pipe com
  `echo hi\r\n`, sob `strace`.
- **Pty em memória**: dois buffers de bytes (entrada e saída) e um mini VT do lado de fora que acompanha
  o cursor e responde `ESC [ 6 n` (CPR), como o terminal do agente responderia. Pro termwiz, uma
  implementação nossa da trait `Terminal` (render pelo `TerminfoRenderer` com o terminfo do
  xterm-256color embutido no binário, entrada pelo `InputParser` do próprio termwiz); pro noline,
  `embedded_io::Read`/`Write` nossos.
- **Roteiro de 20 passos** em bytes crus, com o resultado que o readline do bash 5.2 (modo emacs) daria:
  texto e Enter, setas, Home/End do xterm (`ESC [ H`, `ESC [ F`) e do vt220 (`ESC [ 1 ~`, `ESC [ 4 ~`),
  Ctrl-A/E, Ctrl-W (em palavra e em caminho: o readline apaga até o espaço), Ctrl-U no meio da linha (o
  readline apaga só do cursor pra trás), Ctrl-K, Backspace, Delete, UTF-8 com Backspace, histórico (duas
  vezes pra cima, editar entrada do histórico, cima e baixo restaurando o rascunho), Ctrl-C cancelando e a
  linha seguinte. O termwiz roda duas vezes: com as teclas padrão e com uma tabela de teclas do readline
  no `LineEditorHost` (sem fork).
- **Sem /dev/tty, por teste**: o roteiro inteiro roda num processo filho com `setsid` e
  `strace -e trace=%file,%desc`; entre os marcadores (um `stat` num caminho que não existe, antes e
  depois) não pode haver abertura de `/dev/tty`, `/dev/pts*` ou `/dev/ptmx` nem ioctl de termios. Mais o
  depscan das quatro crates.

### H39: interpretadores

Seis binários (`interp/*`), um por engine, com `unsafe_code = "forbid"` no crate nosso, falando um
protocolo comum (`interp/common`): contexto novo por trecho, `print`/`console.log` escrevendo no nosso
buffer, arquivo lido e escrito num mapa em memória (Python: `open` nosso; JS: `require('fs')` como shim
num prelúdio comum; Lua: `io` nosso no prelúdio comum), ambiente vazio do nosso lado.

- **Corpus** (`data/interp-corpus.toml`): 82 one-liners de agente (Python 28, JS 29, Lua 25) em JSON (ler
  arquivo, filtrar, somar, reserializar, escrever), strings, matemática e listas. O esperado de Python e
  JS foi confirmado rodando o mesmo trecho no python3 (28/28) e no node (29/29) do host, com os arquivos
  materializados num rascunho; o de Lua foi escrito à mão (o host não tem lua) e confere com o Lua 5.4 de
  referência que o mlua compila, o que não é uma conferência independente.
- **Negação de I/O**: 22 sondas (Python 8, JS 7, Lua 7): abrir `/etc/passwd`, listar diretório, ler o
  ambiente, `subprocess`/`child_process`/`os.execute`/`popen`, socket e rede, `import()` dinâmico, módulos
  `std`/`os` do qjs, `dofile`/`loadfile`/`require`/`package`/`debug`. E o binário inteiro roda sob
  `strace` (arquivo, rede e processo), contando toda syscall que toca o host entre os marcadores de início
  e fim dos trechos. Duas configurações ingênuas também são medidas (boa com `Context::default()`, mlua
  com a base inteira), pra mostrar o que acontece sem o cuidado.
- **Custo**: tamanho do binário (release com strip), startup em processo (criar contexto e rodar
  `print(1 + 1)`, mediana de 40), tempo de processo inteiro (spawn até sair, um trecho, 5 repetições),
  depscan do crate do engine.

### H40: linhas de base

Quatro binários (`baseline/*`), um por projeto, falando uma linha JSON por caso: a fixture vai pro FS
virtual do candidato em `/work/case` (modos, symlinks e mtime quando a API deixa), cwd e ambiente fixos
da bancada, stdin do caso, e o retrato de `/work/case` volta como `MemTree` pra comparação byte a byte com
o golden. Cada linha de base roda como processo filho (`prlimit --as=4G`, timeout de 10 s por caso,
processo novo depois de timeout ou crash), 3 filhos em paralelo. Dois conjuntos: os 173 casos `script` de
todos os diretórios do golden (o critério) e os 2808 casos `argv` convertidos em linha de shell, pra ter
taxa por área com mais casos (18 diretórios de golden na execução final). Sondas de isolamento: uma
variável posta só no ambiente do processo host, lida pelo shell e pelo `jq env`, e `cat /etc/machine-id`
do host. Features do bashkit: padrão mais `jq`, `git` e `sqlite` (Turso).

## Candidatos

| Papel | Candidato | Versão | depscan (própria, árvore) | Encaixe |
|---|---|---|---|---|
| shell-parser | brush-parser | 0.4.0 | b (só `println!` atrás de feature desligada e arquivo de teste), c | serve com trabalho |
| shell-core | brush-core | 0.5.0 | b, c | não serve |
| shell-core | yash-env | 0.17.0 | GPLv3, só lido | referência |
| line-edit | reedline | 0.52.0 | b, c | não serve |
| line-edit | rustyline | 18.0.1 | b, b | não serve |
| line-edit | termwiz | 0.23.3 | b, c (host fora do caminho do `LineEditor`) | serve com trabalho |
| line-edit | noline | 0.5.1 | b (na prática a: os pontos contados são `dbg!` de teste), b | serve com trabalho |
| python | monty | 1.0.0 | a, c | serve com trabalho |
| python | rustpython-vm | 0.6.0 | b, c | não serve |
| js | boa_engine | 0.22.0 | b, b | serve |
| js | rquickjs | 0.14.0 | a, c (QuickJS em C) | serve com trabalho |
| lua | piccolo | 0.3.3 | a, b | não serve |
| lua | mlua | 0.12.1 | b, c (Lua 5.4 em C) | serve com trabalho |
| baseline | bashkit | 0.18.2 | b, c | referência |
| baseline | rust-bash | 0.3.0 | b, c | referência |
| baseline | kaish-kernel | 0.17.2 | b, c | referência |
| baseline | wasmsh-runtime | 0.9.0 | a, c | referência |

## Resultado

### H37

Concordância do brush-parser com `bash -n` (aceita/rejeita igual):

| Conjunto | Scripts | bash padrão x brush sem extglob | com extglob | com parse profundo |
|---|---|---|---|---|
| comandos de agente | 47395 (54707 chamadas) | 99,987% (99,989% das chamadas) | 99,987% | 99,987% |
| corpus de shell | 108 | 99,1% | 99,1% | 99,1% |
| outros casos `script` | 65 | 100% | 100% | 100% |
| suíte do bash 5.2.37 | 474 | 95,6% | 96,2% | 96,0% |

No corpus de agente, as 6 divergências são todas o brush rejeitando o que o bash aceita, e todas da mesma
classe: here-doc sem terminador antes do fim do texto (o bash aceita com aviso). Nenhum caso em que o
brush aceita o que o bash rejeita. As causas na suíte do bash (18 divergências com extglob):

- `case` com padrão sem parêntese de abertura dentro de `$(...)` (`$(case x in x) ...; esac)`): 5 na
  suíte e 1 no corpus de shell (`cmdsub-nested`). É a única sonda de aninhamento que falha (29/30).
- here-doc sem terminador: 5 (e as 6 do agente);
- `select` não existe no brush: 2; `done {fd}<arquivo`: 1; `for ((;;))` com quebra antes do `do`: 1;
  `[[ index[7<(4+2)] ... ]]`: 1;
- brush aceita o que o bash rejeita: 3 (uma delas, `$( if x; then echo foo )`, o parse profundo pega);
- só no modo padrão: extglob dentro de `[[ ]]` com extglob desligado (o bash liga sozinho dentro de
  `[[ ]]`, o brush não): 3.

AST em níveis: 100% dos programas do agente (47379), 100% do corpus de shell e 99,3% da suíte parseiam
sem erro de sintaxe em nenhum nível; o agente tem 4336 `$(...)` e 15 crases, com 2807 programas de
profundidade 1 e 24 de profundidade 2. Os erros que sobram na suíte são aritmética que o bash também
rejeita (em tempo de execução: `3425#56`, `7 = 43`), uma crase com escapes aninhados dentro de `${//}`
(`comsub2.sub`) e `$({fd}</dev/stdin)`. Parse mediano de 17 µs por comando de agente (p99 84 µs).

brush-core: 190 pontos de host em 32 de 98 arquivos (11547 de 24431 linhas), 122 `async fn` e 28
referências a tokio em 29 arquivos; árvore com 192 dependências e 7312 pontos de host (nix, mio, socket2,
tokio). Os maiores: `sys/unix/signal.rs` (50), `sys/unix/terminal.rs` (26), `sys/unix/fs.rs` (21),
`openfiles.rs` (13). O executor inteiro é async sobre tokio: portar pro `Ctx` é reescrever o
interpretador, não trocar chamadas.

yash-env: 50 traits públicas e 104 métodos em `src/system`, uma por grupo de syscall (Open, Read, Write,
Dup, Pipe, Fork, Wait, Exec, Sigaction, Select...), e uma `VirtualSystem` de 8522 linhas que implementa
tudo em memória pros testes. Vale copiar o desenho (o interpretador genérico sobre um `S: System` feito
de traits pequenas, com uma implementação virtual completa usada nos testes), não o código: é GPLv3 e só
POSIX sh.

### H38

- **reedline** (evidência: `src/engine.rs`, `read_line` chama `terminal::enable_raw_mode()` e lê por
  `crossterm::event::read()`; `src/painting/painter.rs`, saída em `BufWriter<Stderr>`, e o writer em
  memória `Capture` só existe com `#[cfg(test)]`; crossterm `src/terminal/sys/file_descriptor.rs` abre
  `/dev/tty`). Sonda sem terminal de controle: `openat("/dev/tty") = -1 ENXIO` e
  `ioctl(0, TIOCGWINSZ)`, e o `read_line` falha com "No such device or address".
- **rustyline** (evidência: `src/lib.rs`, `mod tty;` privado e `Terminal::new(&config)` dentro de
  `Editor::with_history`; `src/tty/mod.rs`, o terminal fora de teste é o `PosixTerminal`;
  `src/tty/unix.rs`, fds 0 e 1 do processo ou `/dev/tty` com `PreferTerm`). Sonda: `ioctl(0, TCGETS)`,
  `ioctl(1, TCGETS)` e `read(0, ...)`, e devolve `echo hi` lido direto do stdin do processo host; com
  `PreferTerm`, tenta `/dev/tty` antes.
- **termwiz** `LineEditor` sobre `Terminal` nosso: **11/20** com as teclas padrão, **20/20** com a tabela
  de teclas do readline no `LineEditorHost`. Com as teclas padrão, Ctrl-A/E/K/C não fazem nada porque o
  `InputParser` do termwiz emite Ctrl+letra minúscula (`src/input.rs`,
  `KeyCode::Char((alpha as char).to_ascii_lowercase())`) e o `LineEditor` compara com maiúscula
  (`src/lineedit/mod.rs`, `KeyCode::Char('C')`): é bug do próprio termwiz e vale também com o
  `UnixTerminal` dele. Ctrl-W é por fronteira de palavra (não por espaço) e Ctrl-U não tem ligação. O
  host corrige tudo sem fork. Render no buffer de saída (1981 bytes, 180 ocorrências do prompt, CSI
  presentes). Duas ressalvas de API: `Terminal::waker()` tem que devolver o `UnixTerminalWaker` concreto
  (um `UnixStream` do host com campo privado, `src/terminal/unix.rs`), impossível de construir fora; o
  `LineEditor` nunca chama (o próprio teste do termwiz implementa com `unimplemented!()`). E
  `Capabilities` lê terminfo do FS do host se não receber um banco (`src/caps/mod.rs`): passamos o
  terminfo embutido.
- **noline**: **18/20**. Falham Home/End do xterm (`ESC [ H` vira CUP e toca o sino, `ESC [ F` é
  desconhecido; só `ESC [ 1 ~`/`ESC [ 4 ~` funcionam) e Ctrl-U (apaga a linha inteira, não do cursor pra
  trás). Precisa que o terminal responda CPR (40 respostas no roteiro), e duas entradas dão panic: CPR fora
  de hora (`src/core.rs`) e `ESC [ R` sem argumento (`unwrap` em `src/input.rs`). Sem ganchos de tecla:
  corrigir exige fork.
- **Filho com setsid e strace**: 0 syscalls de arquivo e descritor entre os marcadores (fora a sonda de
  `statx` do próprio std, que vem do marcador), 0 aberturas de tty, 0 ioctls de termios, pros três
  roteiros (termwiz padrão, termwiz com host, noline).

### H39

| Engine | Nega I/O (sondas, syscalls de host no strace) | Binário | Startup em processo (mediana) | Processo inteiro | Corpus | Encaixe |
|---|---|---|---|---|---|---|
| monty 1.0.0 | 8/8, 0 | 9,4 MiB | 3,5 µs | 2,3 ms | 24/28 | com trabalho |
| rustpython 0.6.0 | 8/8, **2088** | 31,0 MiB | 20 ms (16 a 85 ms entre execuções) | 15 ms | 28/28 | não serve |
| boa_engine 0.22.0 | 7/7, 0 | 12,7 MiB | 0,55 ms | 4,5 ms | 29/29 | serve |
| rquickjs 0.14.0 (C) | 7/7, 0 | 1,7 MiB | 0,17 ms | 2,4 ms | 29/29 | com trabalho |
| piccolo 0.3.3 | 7/7, 0 | 1,1 MiB | 0,15 ms | 2,4 ms | 9/25 | não serve |
| mlua 0.12.1 lua54 (C) | 7/7, 0 | 1,1 MiB | 0,09 ms | 2,4 ms | 25/25 | com trabalho |

Os tempos são da execução final, com a máquina carregada por outros agentes: o processo inteiro varia de
1,2 a 4,5 ms entre execuções, a ordem entre os engines se mantém. Os seis compilam num crate nosso com
`forbid(unsafe_code)`. Achados com arquivo e linha (no JSON):

- **RustPython** toca o host sozinho ao criar o interpretador, mesmo sem `host_env`: todo `Interpreter`
  chama `getpath::init_path_config` (`src/vm/interpreter.rs`, `src/getpath.rs`), que lê variáveis do
  ambiente do host e procura `pyvenv.cfg` e `Lib/os.py` subindo os diretórios do host (2016 `statx`, 36
  `access`, 36 `getcwd` entre os marcadores). Não há opção que desligue: isolar exige fork. Além disso,
  31 MiB e dezenas de ms por contexto.
- **Monty**: todo I/O sai do interpretador como `RunProgress::OsCall` e quem responde é o host (nós): é o
  desenho certo. Mas é um subconjunto: o `json` só tem `loads`/`dumps` (sem `load`/`dump`, 3 falhas) e
  `round(2.675, 2)` dá 2.68 (o CPython dá 2.67).
- **boa**: `Context::default()` usa `SimpleModuleLoader::new(".")` (`src/context/mod.rs`), que faz realpath
  do cwd do host e lê arquivo do host no `import()`; a configuração ingênua mediu 8 toques de host. Com
  `IdleModuleLoader`, zero. Closure nativa com captura só via `NativeFunction::from_closure`, que é
  `unsafe`: o estado fica num `thread_local`.
- **mlua**: a seleção de `StdLib` não fecha a biblioteca base, que traz `print` (stdout C), `dofile` e
  `loadfile` (`fopen`); a configuração ingênua abriu `/etc/passwd` e vazou em 2 sondas. Trocados e
  removidos à mão, zero.
- **piccolo**: isolado, mas a stdlib é mínima (sem `string.format`, `find`, `gsub`, `gmatch`, `tonumber`,
  `table.sort`/`insert`/`concat`), e `-7 // 2` dá -3.

### H40

Conjunto `script` (o do critério), estrito/leniente:

| Diretório | n | bashkit | rust-bash | kaish | wasmsh |
|---|---|---|---|---|---|
| shell | 108 | 35%/42% | 34%/44% | 6%/11% | 13%/18% |
| awk | 9 | 89%/89% | 89%/89% | 33%/33% | 89%/89% |
| coreutils | 10 | 70%/70% | 70%/70% | 10%/10% | 60%/60% |
| jq | 5 | 100%/100% | 100%/100% | 20%/20% | 80%/80% |
| git | 15 | 0%/7% | 0%/0% | 0%/0% | 0%/0% |
| archive | 13 | 0%/15% | 8%/8% | 0%/0% | 0%/0% |
| **total** | 173 | **35%/41%** | **34%/40%** | **7%/10%** | **18%/21%** |

Por área, somando `script` e `argv` (2981 casos), estrito/leniente:

| Área | n | bashkit | rust-bash | kaish | wasmsh |
|---|---|---|---|---|---|
| shell | 114 | 36%/43% | 33%/44% | 7%/12% | 13%/18% |
| coreutils | 776 | 29%/35% | 21%/25% | 8%/9% | 20%/22% |
| grep/sed/awk (com regex) | 1448 | 50%/53% | 34%/40% | 8%/8% | 27%/28% |
| jq | 299 | 39%/51% | 28%/36% | 23%/24% | 28%/36% |
| outros (sqlite, git, bc...) | 342 | 8%/8% | 8%/9% | 0%/0% | 16%/16% |

No corpus de shell, o que o bashkit já cobre bem: laços (3/3), here-docs (78%), aspas, `$(...)` e
aritmética (60% a 67%); e o que falha: arrays indexados e associativos, `[[ ]]`, globbing, expansão de
parâmetro (18%), builtins e mensagens de erro. O rust-bash vai melhor em `[[ ]]`, glob e
`${...}`, pior em `read`, `local`, `nounset` e subshell.

Achados que valem fora do H40:

- **`halt`/`halt_error` do jq derruba o processo inteiro** em rust-bash, kaish e wasmsh: o embutidor
  chama `jaq_core::unwrap_valr`, que faz `std::process::exit` (jaq-core 3.1.1 `src/val.rs`), e o wasmsh usa
  o `halt` do jaq-std 2.1.2 que faz o mesmo. O bashkit troca o `halt` por erro. **Vale pro H27/F05**: o
  nosso jq nunca pode passar por `unwrap_valr` nem pelo `halt` do jaq-std.
- **Vazamento do host**: `jq -n env` mostra o ambiente do processo host em rust-bash, kaish e wasmsh
  (jaq-std); o bashkit filtra. Nenhum dos quatro vazou pelo shell nem pelo `/etc/machine-id`.
- **APIs**: o bashkit tem tudo que a bancada precisa na API pública (fixture com modo, symlink e mtime,
  stdin, relógio fixo); o rust-bash passa stdin colando um here-doc no fim do script e não tem relógio
  injetável; o kaish não modela permissão (`MemoryFs` com 0666/0777 fixos) e não é bash; o wasmsh só
  expõe um protocolo de mensagens, sem symlink, mtime nem modo. O rust-bash usa brush-parser 0.3, o que
  conta a favor do H37.
- Crashes (o processo do candidato morre, e o embutidor junto): 4 no rust-bash, 4 no kaish (um SIGABRT
  em `pipeline-sigpipe-producers`, os outros pelo `halt` do jq), 3 no wasmsh; timeouts: 2 no rust-bash
  (awk com arquivo), 6 no wasmsh (trava no `bc` com `define`). O bashkit não teve nenhum.

## Veredito

- **H37: parcial.** O **brush-parser serve de parser** com uma camada nossa por cima: concorda com
  `bash -n` em 99,989% das 54707 chamadas reais de agente (6 divergências, todas here-doc sem terminador),
  99,1% do corpus de shell e 95,6% a 96,2% da suíte do bash, e o re-parse em níveis cobre `$(...)`
  aninhado (29/30 sondas, 100% dos comandos de agente sem erro em nenhum nível). A camada nossa precisa:
  re-parse recursivo de palavras e substituições, desescape de crases, aritmética depois da expansão,
  aceitar here-doc terminado pelo fim do texto, `case` sem parêntese dentro de `$(...)`, `select`, e
  extglob dentro de `[[ ]]`. Isso é patch pequeno num fork do parser, não reescrita. O **brush-core não
  serve**: 190 pontos de host em 32 arquivos e o executor inteiro async sobre tokio; o interpretador é
  nosso, com o desenho do yash-env (traits de sistema pequenas e uma implementação virtual) como
  referência.
- **H38: refutada.** reedline e rustyline estão presos ao tty do host, por código e por teste: sem
  terminal de controle, o reedline abre `/dev/tty` e falha, o rustyline faz `TCGETS` e lê o fd 0 do
  processo host. **Recomendação: `LineEditor` do termwiz** sobre um `Terminal` nosso ligado ao pty do
  kernel, com `LineEditorHost` nosso pras teclas do readline (20/20 no roteiro, nenhum acesso a tty no
  strace), terminfo embutido e `waker()` que nunca é chamado. O noline (18/20, no_std) é a alternativa se o
  termwiz pesar, mas precisa de fork pra Home/End do xterm, Ctrl-U e os dois panics.
- **H39: parcial.** Dá pra negar todo I/O de host e oferecer o nosso em cinco dos seis engines, todos
  compilando com `forbid(unsafe_code)` no nosso crate; o RustPython não, porque toca o FS do host ao criar
  qualquer interpretador. Por linguagem: **JS: boa** (Rust puro, 29/29, 0,55 ms de startup, 12,7 MiB, com
  `IdleModuleLoader`); rquickjs é menor e mais rápido, mas é C. **Lua: mlua** (25/25, 0,09 ms, 1,1 MiB,
  C vendored) com `dofile`/`loadfile` removidos e `print`/`io` nossos; o piccolo é Rust puro mas sem
  stdlib útil. **Python: Monty** (o I/O sai como `OsCall` pro host responder, 3,5 µs de startup, 9,4 MiB),
  aceitando o subconjunto e com um shim pra `json.load`/`json.dump`; RustPython só com fork do
  `getpath.rs`, e ainda custa 31 MiB e dezenas de ms por contexto.
- **H40: parcial.** As linhas de base cobrem parte, mas nenhuma serve de base: o melhor é o bashkit, com
  53% leniente em grep/sed/awk e 51% em jq, e no shell, que é o núcleo do plano, ninguém passa de 44%
  leniente (falham `set -e` e suas exceções, arrays, `[[ ]]`, `${...}`, glob, `declare -p`, getopts). Onde
  cobrem mais do que o plano previa: o jq do bashkit já resolve dois problemas que o nosso teria em cima
  do jaq (desligar o `halt` que mata o processo e filtrar o `env` do host), e vale estudar o desenho de API
  dele (fixture, relógio fixo, stdin) pro nosso modo biblioteca. A estratégia do plano não muda.
