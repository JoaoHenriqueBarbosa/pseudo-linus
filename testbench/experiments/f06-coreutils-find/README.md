# F06/F07: uutils e findutils portados pro shim `sysio`

Porte real de `cat`, `head`, `wc`, `sort` e `ls` do uutils coreutils 0.12.0 e de `find` e `xargs` do
uutils findutils 0.10.0 pra um shim (`crates/sysio`) com a forma de `std::fs`/`std::io`/`std::env`,
mas apoiado num VFS em memória e num contexto de pseudo-processo. O experimento mede quanto código
mudou, o que travou, e se o porte custa conformidade, rodando o mesmo corpus no porte (em processo,
sobre o VFS) e no original (no container do oráculo).

Resultado em `testbench/results/f06-coreutils-find.json`; tudo é refeito por
`cargo run --release` neste diretório (cerca de 90 s).

## Hipóteses

- **H28** "uutils compila quase sem alteração trocando std::fs por um shim sysio". Critério: porte real
  de cat, sort, ls, head e wc; linhas alteradas, bloqueios (estado global do uucore, libc) e
  conformidade depois do porte.
- **H29** "uutils/findutils serve de ponto de partida pra find e xargs". Critério: esforço pra rodar o
  find sobre FS em memória e conformidade no golden de find.

## Método

### Corpus e golden

306 casos no estilo de agente, todos com golden gerado no oráculo (Debian 13, coreutils 9.7-3,
findutils 4.10.0-3) por `cargo run -p oracle -- gen --tool <tool>`, idêntico em duas gerações
seguidas:

| arquivo | casos | cobre |
|---|---|---|
| `coreutils/cat.toml` | 19 | -n -b -s -A -v -E -T, stdin, `-`, erro, diretório, symlink, heredoc |
| `coreutils/sort.toml` | 26 | -n -r -k -t -u -h -V -f -s -c -o, chave múltipla, UTF-8, erro de chave |
| `coreutils/ls.toml` | 26 | -a -A -1 -R -l -t -S -r -F -p -d -n -h, `--time-style`, `--full-time`, data recente e antiga |
| `coreutils/head.toml` | 18 | -n -c (negativos, sufixo), -q -v -z, forma `-N`, erro |
| `coreutils/wc.toml` | 19 | -l -w -c -m -L, total, `--total`, `--files0-from`, binário, diretório |
| `coreutils/tail.toml`, `text.toml`, `fsops.toml`, `stat_paths.toml`, `misc.toml` | 118 | tail, cut, tr, uniq, cp/mv/rm/mkdir -p/ln -s/touch, stat -c, basename/dirname/realpath/readlink, tee, seq, printf, echo, env, date (faketime), du |
| `find/find.toml` | 52 | -name -iname -path -type -size -empty -mtime -mmin -newer -maxdepth -mindepth -exec `;`/`+` -print0 -delete -prune -o `!` -not parênteses -perm -printf -regex -depth -L -quit, erros |
| `xargs/xargs.toml` | 28 | -0 -n -I -r -d -L -P -t -s -a -E, aspas, comando inexistente, falha (123), pipelines com find |

Os casos com mtime fixo e `faketime` levam `NO_FAKE_STAT=1`: sem ele o libfaketime desloca o mtime que
o `statx` devolve pela diferença entre a data falsa e a real, e o golden dependeria do dia da geração.
O `find` lista na ordem do readdir (no oráculo, a do ext4 sob o overlay), que não é propriedade
semântica: casos com a tag `unordered` são comparados como multiconjunto de registros (linhas, ou
registros terminados em NUL com `print0`/`null`). O resto é byte a byte.

### O shim (`crates/sysio`, sem nenhuma dependência)

- **VFS** (`vfs.rs`): tabela de inodes, diretório como mapa nome -> inode, namei do Linux (symlink no
  meio sempre seguido, no fim quando pedido, ELOOP depois de 40, ENOTDIR, barra final). Convenções do
  ext4 do oráculo: diretório com `st_size` 4096 e 8 blocos, arquivo em blocos de 4 KiB, root ignora
  rwx, `nlink` de diretório = 2 + subdiretórios.
- **Processo** (`proc.rs`): `Ctx` numa thread-local, com VFS, cwd, argv, ambiente, stdin/stdout/stderr
  como buffers, relógio, umask, código de saída (o `EXIT_CODE` do uucore) e armazenamento local do
  processo (`proc_local`, o substituto de `static OnceLock`). Só é correto no modelo A do design
  (uma thread por pseudo-processo); threads do processo herdam o contexto por `sysio::thread::spawn`.
- **API com os caminhos do std**: `sysio::fs` (File, OpenOptions, metadata, read_dir...),
  `sysio::io` (reexporta tudo de `std::io` e sombreia `stdin`/`stdout`/`stderr`/`IsTerminal`),
  `sysio::env`, `sysio::os::unix::fs::{MetadataExt, PermissionsExt, FileTypeExt, ...}`,
  `sysio::os::fd::{AsFd, AsRawFd}` (AsFd vira "dá pra fazer fstat/lseek/fcntl"), macros `print!`,
  `println!`, `eprint!`, `eprintln!`. Na maioria dos arquivos o porte é trocar o caminho do `use`.
- **Execução**: `sysio::process::exit` sobe como unwind (`std::process::exit` mataria o host);
  `spawn`/`spawn_in` despacham pra tabela de programas da bancada (`src/exec.rs`), cada filho numa
  thread nova com o mesmo VFS e o mesmo stdout/stderr.
- **Usuários e entropia**: `/etc/passwd` e `/etc/group` do VFS; bytes aleatórios do `/dev/urandom` do
  VFS ou de um gerador por processo.

### Vendorização e porte

Os crates foram copiados do registry do cargo (`cp -r`, permitido pra código de terceiros) pra
`ported/<crate>-<versão>`, e todas as alterações de porte foram feitas com edição pontual, cada uma
com um comentário `Porte pseudo-linus`, pra que o diff contra o pristino meça exatamente o porte.
Os pacotes foram renomeados (`port-uucore`, `port-uu-cat`...) mantendo o nome da lib, e todos têm
`unsafe_code = "forbid"`. O nome do diretório com a versão é de propósito: o `build.rs` do uucore
embute os `.ftl` dos `uu_<util>-<versão>` vizinhos, e assim funciona sem mexer nele.

Crates portados: `uucore` e `uucore_procs` (compartilhados), `uu_cat`, `uu_head`, `uu_wc`, `uu_sort`,
`uu_ls`, `findutils` (só find e xargs; locate e updatedb ficam de fora) e mais dois forks que o porte
exigiu: `lscolors` (pro ls) e `walkdir` (pro find).

### Como roda

- **Portado**: em processo, cada pseudo-processo numa thread nova, sobre o VFS montado da fixture
  (`src/sandbox.rs`). Casos `script` só entram quando são pipelines de programas da tabela
  (`src/shell.rs`: palavras com aspas e `|`, estágios em sequência). O `echo` que o xargs usa por
  padrão e o find chama no `-exec` é um builtin escrito à mão (`src/builtins.rs`); comando que não está
  na tabela (`rm`, `grep`) vira "unsupported".
- **Original**: o pacote `original/` compila os crates do crates.io sem alteração num multicall, que é
  montado no container do oráculo em `/usr/local/bin` com links `cat`, `sort`, `ls`, `head`, `wc`,
  `find`, `xargs`. Esse diretório vem antes de `/usr/bin` no PATH fixo dos casos, então o mesmo argv
  e o mesmo script caem no uutils sem mexer no ambiente, e usuário, grupo e FS ficam iguais aos do
  golden (no host, `ls -l` mostraria `john`).
- **Medidas** (`src/porting.rs`): diff de linhas (`similar`) de cada crate contra o pristino do
  registry; arquivos que o rustc de fato compilou (dep-info do cargo); nesses arquivos, toque no host
  e unsafe antes e depois com o visitor do depscan, chamadas de métodos de `Path` trocadas e `static`
  de estado global removidos; e o depscan da árvore de dependências do original e do porte.

## Candidatos

| papel | candidato | categoria (depscan) | encaixe |
|---|---|---|---|
| coreutils:cat, head, wc, sort, ls | `uu_<util>` 0.12.0 original | (b) próprio, (c) na árvore | não serve direto |
| coreutils:cat, head, wc, sort, ls | `uu_<util>` 0.12.0 + porte sysio | (a) no head, (b) nos outros por falso positivo (ver Limitações) | serve com trabalho |
| find, xargs | `findutils` 0.10.0 original | (b) próprio, (c) na árvore (`onig_sys`, C de verdade) | não serve direto |
| find, xargs | `findutils` 0.10.0 + porte sysio | (b) por falso positivo; `onig_sys` sai da árvore | serve com trabalho |

## Resultado

### Conformidade (casos estritos / lenientes, contra o golden do GNU)

| papel | casos | portado | original no oráculo | portado igual ao original |
|---|---|---|---|---|
| cat | 18 | 17 / 18 | 17 / 18 | 18 / 18 |
| head | 17 | 16 / 17 | 16 / 17 | 17 / 17 |
| wc | 19 | 19 / 19 | 19 / 19 | 19 / 19 |
| sort | 25 | 24 / 25 | 24 / 25 | 25 / 25 |
| ls | 24 | 24 / 24 | 24 / 24 | 24 / 24 |
| find | 51 | 47 / 50 (1 unsupported) | 48 / 51 | 50 / 51 |
| xargs | 27 | 23 / 26 (1 unsupported) | 24 / 27 | 26 / 27 |

O porte não perdeu nenhum caso: onde o portado difere do original é porque o caso chama um comando que
a bancada não tem (`grep` no `find-xargs-grep`, `rm` no `xargs-rm`). Tudo que falha contra o GNU já
falha no original, e só no stderr (o leniente passa): `cat -Z` com a mensagem do clap, aspas `'x'` no
lugar de `‘x’` (head, sort), `Error: nope: No such file or directory (os error 2)` no find,
`missing argument to -name` sem a crase, e no xargs `Error: Unterminated quote: 39`,
`Error: Command not found` e o `-t` imprimindo o `Debug` do `std::process::Command`
(`env -i HOME="/root" ... "echo" "a" "b"`). Isso é fidelidade do uutils, não custo do porte.

### Custo do porte

Linhas inseridas / removidas nos `.rs` contra o pristino, e auditoria dos arquivos que o rustc compila:

| crate | + / - (`.rs`) | linhas compiladas (pristino) | intactas | toque no host antes -> depois | unsafe antes -> depois | métodos de `Path` trocados | statics globais removidos |
|---|---|---|---|---|---|---|---|
| uu_cat | 12 / 21 | 928 | 907 | 10 -> 1 | 0 -> 0 | 0 | 0 |
| uu_head | 5 / 30 | 1756 | 1726 | 1 -> 0 | 0 -> 0 | 0 | 0 |
| uu_wc | 35 / 56 | 1595 | 1539 | 15 -> 1 | 1 -> 0 | 0 | 1 |
| uu_sort | 132 / 323 | 6119 | 5796 | 45 -> 7 | 8 -> 2 | 2 | 2 |
| uu_ls | 69 / 53 | 5914 | 5861 | 33 -> 1 | 0 -> 0 | 13 | 2 |
| uucore (compartilhado) | 560 / 911 | 21418 | 20507 | 166 -> 18 | 34 -> 20 | 11 | 10 |
| uucore_procs | 5 / 22 | 73 | 51 | 0 -> 0 | 0 -> 0 | 0 | 0 |
| lscolors (fork pro ls) | 10 / 8 | 2070 | 2062 | 8 -> 0 | 0 -> 0 | 2 | 0 |
| findutils (find + xargs) | 473 / 125 | 11787 | 11662 | 59 -> 9 | 1 -> 0 | 8 | 0 |
| walkdir (fork pro find) | 23 / 8 | 1833 | 1825 | 7 -> 0 | 0 -> 0 | 1 | 0 |

No findutils, o find levou 387 / 115 (187 linhas são o tradutor POSIX -> regex novo, `posix_re.rs`, no
lugar do onig) e o xargs 86 / 10: das 1236 linhas não-teste de `xargs/mod.rs`, 1226 ficaram; o que
entrou é um `Command` mínimo (argv, ambiente, `status`, e um `Debug` igual ao do std) que executa
pela tabela de programas.

Os números "depois" que não são zero não são toque no host no código que compila: o
`forbid(unsafe_code)` passa em todos os crates portados, então não há unsafe compilado. O que o
depscan ainda conta é código atrás de `cfg(windows)`, de feature desligada (`libc`, `process`), módulo
de teste `#[cfg(all(test, unix))]` (o depscan só reconhece `test` no topo do `cfg`), ou
`print!`/`eprintln!` que agora resolvem pro `sysio` (o depscan casa macro pelo nome). A lista por
arquivo está em `metrics.audits.<crate>.residual` no JSON. Uma busca por `std::env::`, `std::fs::`,
`std::io::stdout()` e afins nos arquivos compilados também só acha teste, Windows ou feature desligada.

Tempo de agente gasto (estimativa anotada durante o trabalho, não medição): uucore + uucore_procs
90 min, cat 15, head 10, wc 20, sort 45, ls + lscolors 60, findutils + walkdir 75.

### Por que não dá pra só "trocar o std::fs"

1. **O std não é patchável.** Não existe `[patch]` pro std; `std::fs::File` é um tipo concreto sobre
   fd do host. Trocar o std inteiro exigiria `-Zbuild-std` (nightly) com um std modificado, e valeria
   também pro próprio pseudo-kernel. Então o porte é trocar caminho de import em cada arquivo, o que
   cobre a maior parte, mas não tudo:
2. **Métodos inerentes de `Path`** (`exists`, `is_dir`, `metadata`, `symlink_metadata`, `read_link`,
   `canonicalize`) chamam o FS do host e não podem ser sombreados por trait: 36 chamadas reescritas
   à mão (saldo removido menos adicionado nas linhas alteradas). O depscan nem enxerga esse toque no
   host (não é um caminho `std::fs`).
3. **Traits selados e tipos concretos do std em API de terceiros**: `std::io::IsTerminal` é selado;
   `lscolors::Colorable` exige `std::fs::FileType`/`Metadata`, que só nascem de syscall do host;
   `walkdir::DirEntry` e `same_file::Handle` idem; `argmax::Command` é `std::process::Command`. Cada um
   virou fork ou reescrita.
4. **Estado global no uucore e nos utilitários**: `EXIT_CODE` (AtomicI32), `ARGV`/`UTIL_NAME`/
   `EXECUTION_PHRASE` (LazyLock do argv do host), cache de `.ftl` do utilitário (OnceLock: o primeiro
   utilitário do processo host fica com as mensagens de todos), locale de colação, numérico, de
   horário e de ctype, separador decimal, meses, política de SIMD, `COLLATOR`, `POSIXLY_CORRECT` do
   wc. 15 statics viraram estado do pseudo-processo (os que dependem de ambiente) ou tabela internada
   (os que precisam devolver `&'static`).
5. **Efeitos colaterais no processo host inteiro**: `#[uucore::main]` gera um static em `.init_array`
   (`#[unsafe(link_section)]`) e muda a disposição de SIGPIPE, SIGSEGV e SIGBUS; `mute_sigpipe_panic`
   troca o hook de panic global; `get_umask` faz `umask(0)` e volta (corrida com as outras threads);
   `setlocale`; o pool global do rayon; o handler de SIGINT do `ctrlc` com `std::process::exit(2)`;
   `std::process::exit` no tratamento de erro do clap.
6. **Unsafe que o `forbid` não aceita**: o depscan conta 44 ocorrências nos arquivos que o porte
   compila (parte delas atrás de `cfg` de outra plataforma). As que compilam no Linux e tiveram que
   sair: transmute de lifetime no sort, `setlocale`/`nl_langinfo`, `getpwuid`/`getgrgid`, `fcntl`,
   `umask`, `sigaction`, `sysconf`, `from_utf8_unchecked` e o `link_section` do `#[uucore::main]`.
7. **Código C**: o `onig` (Oniguruma) faz o glob do `-name` e o `-regex` do find.
8. **Leitura escondida do ambiente do host**: `.env("TABSIZE")`/`.env("TIME_STYLE")` do clap,
   `LsColors::from_env`, `std::env::args_os()` no `--dired` do ls, `TimeZone::system()` do jiff,
   `chrono::Local`, o `procfs` lendo `/proc/meminfo` do host no `parse_size`.

A demonstração do item 4 está em `metrics.global_state_demo`: o `uu-original --demo-global-state`
chama `wc nope.txt` e depois `cat ok.txt` no mesmo processo, só pelo `uumain`. Resultado: o `wc` imprime
`uu-original: nope.txt: No such file or directory` (nome do executável do host, não `wc`) e o `cat`
de um arquivo que existe sai com 1 (o `EXIT_CODE` do `wc` vazou). No porte, a mesma sequência dá
`wc: nope.txt: ...` com saída 1 e o `cat` com saída 0.

### Bloqueios por utilitário

- **cat**: `splice` via `uucore::pipes` + rustix (saiu, fica read/write); `is_safe_overwrite` por
  `AsFd` + `fstat`/`lseek`/`fcntl` (virou o trait `Fstat`); `RawWriter` escrevendo no fd 1; sinais do
  `#[uucore::main]`.
- **head**: caminho zero-copy (`send_n_bytes`) e `dup(2)` do fd 0 num `File` pra poder dar seek.
- **wc**: `splice` pra `/dev/null`, `fstat` e tamanho de página por rustix, `LazyLock` do
  `POSIXLY_CORRECT`, `from_utf8_unchecked`.
- **sort**: o mais caro. Pool global do rayon e `par_sort` (threads do host sem contexto do
  pseudo-processo; virou sort sequencial); threads de leitura e merge (`sysio::thread::spawn`);
  `tempfile` e `ctrlc` (diretório temporário no VFS, sem limpeza por SIGINT); transmute de lifetime
  (trocado por coleta in-place, segura); `getrlimit`, `/proc/self/fd`, `access`, `sysinfo`;
  `--compress-program` (filho com pipe concorrente, sem equivalente no shim: a compressão fica
  desligada com o mesmo aviso de quando o programa não roda); caches de i18n globais; `rand::rng()`.
- **ls**: `uucore::entries` (getpwuid/getgrgid da libc, virou leitura do `/etc/passwd` do VFS); fork do
  lscolors; `.env()` do clap; `terminal_size`, `hostname`, `xattr`, `statfs`; argv do host no
  `--dired`; fuso do host no `SystemTime -> Zoned` (virou TZ do pseudo-processo com tzdb embutida no
  binário, feature `tzdb-bundle-always` do jiff); 13 chamadas de métodos de `Path`.
- **find**: fork do walkdir; onig trocado por tradutor POSIX -> crate `regex` (sem retrovisor, que a
  crate não tem); `nix` (usuários e grupos), `faccess`, `argmax` + `std::process::Command` no `-exec`;
  `chrono::Local` e `Utc::now`; `std::process::exit` no `Printer`; stdin do host no `-files0-from -`.
- **xargs**: executor (`std::process::Command`) trocado pela tabela de programas; `sysconf(_SC_ARG_MAX)`
  da libc; ambiente do host. O `-P` já era ignorado no original (roda em série).

### O que o porte tirou da árvore de dependências (depscan)

Dependências que tocam o host e saíram: `nix`, `procfs`, `xattr`, `dunce`, `uucore` (o do registry) em
todos; `tempfile`, `ctrlc`, `rayon-core`, `getrandom` no sort; `hostname` e `lscolors` (o do registry)
no ls; `walkdir`, `same-file`, `faccess`, `argmax` e o `onig_sys` (C) no find. O que sobra em todas as
árvores é a pilha do clap (`clap_builder`, `anstream`, `terminal_size`, `is_terminal_polyfill`,
`rustix`, `libc`): ela só toca o host pra decidir cor e largura de terminal no texto de ajuda e de
erro. Isso não sai sem mexer no clap (ou desligar as features `color` e `wrap_help`, o que muda a
saída de `--help`), e fica registrado como pendência pro pseudo-linus.

### Limitações da medição

- O depscan conta código atrás de `cfg` de outra plataforma e resolve a árvore do workspace inteiro,
  com features unificadas com o pacote `original/` (por isso o `chrono` do porte aparece com
  `iana-time-zone`, que o build do porte não liga). A categoria "c" das árvores de uu_* vem de crates
  que só compilam C em outro alvo (`iana-time-zone-haiku`, `wasm-bindgen-shared`) ou que declaram
  `links` sem C (`defmt`, `rayon-core`); o único C de verdade no Linux é o `onig_sys` do findutils.
- O shim simplifica: todo processo é root (não há checagem de rwx), stdout nunca é um arquivo (o
  `is_safe_overwrite` do cat sempre libera, então `cat a >> a` não é detectado), não há sinais, e a
  pipeline da bancada roda estágios em sequência (pipelines infinitas como `yes | head` não cabem).
- O `-P` do xargs e as threads do sort rodam, mas sem o escalonador do pseudo-linus por trás.
- O tempo de porte é estimativa do agente, não medição.

## Veredito

- **H28: Parcial.** O uutils roda sobre o shim sem perder um caso sequer pro original (cat 18/18,
  head 17/17, wc 19/19, sort 25/25, ls 24/24 iguais ao original), e o código de cada utilitário muda
  pouco: 736 linhas inseridas+removidas sobre 16312 compiladas, quase todas trocas de import e remoção
  de caminho rápido de syscall. Mas a frase do v1 ("quase sem alteração trocando std::fs") é falsa no
  mecanismo: o std não é patchável, 36 chamadas de métodos de `Path` e 15 statics de estado global
  tiveram que ser reescritos, o uucore precisou de 1471 linhas de porte (96% das 21418 linhas
  compiladas ficaram intactas), e uucore, uucore_procs e lscolors viraram fork. O sort é a exceção
  de custo (455 linhas): threads, rayon, arquivo temporário, sinal e processo filho.
- **H29: Parcial.** O find portado faz o mesmo que o original em 50 de 51 casos (o que falta chama
  `grep`, que a bancada não tem) e passa 47/51 estrito contra o GNU (50/51 leniente); o custo foi
  387/115 linhas no find, mais 31 no fork do walkdir e a troca do onig (C) por um tradutor POSIX ->
  regex. Serve de ponto de partida. No xargs, 99% de `xargs/mod.rs` sobreviveu, mas só porque o
  executor é pequeno; o que importa (o executor) é nosso, e o formato das mensagens de erro e do `-t`
  diverge do GNU no próprio findutils.

**Recomendação.** Fork por utilitário em cima de um uucore portado uma vez só (o uucore é o grosso do
custo e é compartilhado por todos os utilitários do uutils), com o `sysio` como a cara do `Ctx`
pros programas. Pro find, fork do findutils com o walkdir portado e, no lugar do tradutor, o motor
POSIX à mão do F01 (BRE/ERE com retrovisor). Pro xargs, fork leve: reaproveitar leitores, limites e
`-I`/`-L`/`-n`/`-s`, com o executor do pseudo-kernel e as mensagens de erro do GNU. Em todos, as
divergências de stderr contra o GNU (mensagens do clap, aspas) são trabalho à parte, igual com ou sem
porte. O sort deve ter threads e arquivo temporário pensados junto com o escalonador e o tmpfs do
pseudo-linus, não só trocados por chamada do shim.

## Arquivos

- `src/main.rs`: roda tudo e grava o JSON; `src/exec.rs`: tabela de programas e processos;
  `src/sandbox.rs`: VFS de um caso; `src/shell.rs`: pipeline mínima; `src/scoring.rs`: comparação;
  `src/original.rs`: originais no oráculo e a demonstração de estado global; `src/porting.rs`:
  medidas do porte; `src/builtins.rs`: `echo`.
- `crates/sysio/`: o shim.
- `original/`: multicall com os crates do crates.io sem alteração.
- `ported/`: os crates vendorizados com o porte.
