# pseudo-linus: design v2

Um pseudo sistema operacional em Rust, feito pra servir de sandbox leve e robusto pra agentes de IA que
falam bash, rodando na VPS do dono e atendendo vários usuários ao mesmo tempo. O "kernel" é uma
biblioteca, os "programas" são funções Rust registradas numa tabela, e a compatibilidade com Linux é de
comportamento observável: mesmos errnos, mesmos códigos de saída, mesmas mensagens de erro, mesma
semântica de fd, pipe, sinal, caminho e permissão. Tudo que for implementado se comporta igual ao Linux;
o que não for implementado não existe (`bash: python3: command not found`, 127).

## Como ler este documento

Toda afirmação que dependia de medição foi marcada `[Hnn]` e testada na bancada (`testbench/`). O
registro completo, com o veredito e o número que decidiu cada uma, está no fim ("Registro de
hipóteses") e em `docs/bench-report.md`. O v1 virou 42 hipóteses: 19 confirmadas, 5 refutadas e 18
parciais.

O que mudou em relação ao v1:

- O escalonador nasce **EEVDF** (o do Linux 6.6+), fiel à 6.12.101, e já com **grupos e limite de
  banda** porque o alvo é multiusuário.
- **Nenhum `unsafe` no código que escrevermos.** Isso derrubou o M:N com corrotinas migrando entre
  threads; o modelo escolhido é **uma thread do SO por pseudo-processo**, com o nosso EEVDF distribuindo
  CPUs virtuais.
- A árvore rubro-negra e o EEVDF são **feitos à mão**, sem crate, e já existem (`crates/rbtree`,
  `crates/sched`).
- O isolamento tem **três camadas** medidas (lint, depscan, Landlock e seccomp por thread), porque nem o
  `disallowed_methods` nem o `forbid(unsafe_code)` garantem o que o v1 dizia.
- Quase toda ferramenta tem caminho definido, com número: crate como está, fork medido, ou à mão.
- Constantes erradas do v1 foram corrigidas pelo Linux real (pid_max, fatia base).

## Princípios

1. **Compatibilidade de comportamento, não de binário.** Não existe ELF, ld.so nem syscall de verdade. O
   agente só vê texto entrando e saindo, e é ali que a fidelidade é medida, byte a byte, contra um
   Debian 13 real (bash 5.2.37, coreutils 9.7, gawk 5.2.1, grep 3.11, sed 4.9, jq 1.7.1, glibc 2.41).
2. **O que não existe não existe.** Nenhum comando "quase funciona".
3. **Nada de unsafe nosso.** Todo crate do projeto e da bancada tem `unsafe_code = "forbid"`. Dependência
   pode ter unsafe interno se a API que chamamos for segura; nunca `unsafe impl` nosso. Como o lint não
   enxerga unsafe gerado por macro de outro crate [H20], o depscan também audita isso.
4. **Todo I/O passa pelo `Ctx`.** Nenhum builtin toca o host. A garantia tem três camadas (Isolamento).
5. **Sem fork, sem exec real.** O "fork" do shell é clonar o estado do interpretador; o "exec" é despachar
   pra uma função Rust.

## Arquitetura em crates

| Crate | Papel | Estado |
|---|---|---|
| `rbtree` | Árvore rubro-negra aumentada em arena, à mão | pronto, validado [H12] |
| `sched` | EEVDF à mão, fiel ao `fair.c` da 6.12.101, com grupos, banda e balanceamento; relógio injetável, simulador | pronto [H13] [H41] [H42] |
| `kernel` | Processos, pids, fds, pipes, sinais, wait, rlimits, contabilidade | a fazer |
| `vfs` | Inodes, montagens, namei; tmpfs, hostfs, overlay, procfs, devfs | a fazer (desenho medido [H17] [H18]) |
| `sysabi` | `Ctx`, `Errno`, `OFlags`, `Stat`, `strerror` gerado de `linux_facts.json` | a fazer |
| `shell` | Interpretador bash sobre o `brush-parser` | a fazer |
| `userland` | Os builtins | a fazer |
| `host` | Daemon na VPS: supervisor, workers, JSON-RPC, terminal | a fazer |

O contrato central continua o do v1:

```rust
pub type Main = fn(&mut Ctx, &[OsString]) -> i32;

impl Ctx<'_> {
    pub fn open(&mut self, path: &Path, flags: OFlags, mode: u32) -> Result<Fd, Errno>;
    pub fn read(&mut self, fd: Fd, buf: &mut [u8]) -> Result<usize, Errno>;
    pub fn write(&mut self, fd: Fd, buf: &[u8]) -> Result<usize, Errno>;
    pub fn spawn(&mut self, argv: &[OsString], spec: SpawnSpec) -> Result<Pid, Errno>;
    pub fn wait4(&mut self, pid: Pid, opts: WaitOpts) -> Result<(Pid, WaitStatus), Errno>;
    pub fn kill(&mut self, pid: Pid, sig: Signal) -> Result<(), Errno>;
    pub fn checkpoint(&mut self); // ponto de preempção e entrega de sinal em laços de CPU
    // stat, lstat, readdir, mkdir, unlink, rename, symlink, readlink, chdir, getcwd,
    // dup2, pipe2, fcntl, getenv/setenv, clock_gettime, ...
}
```

`Errno`, `strerror` e a tabela de sinais saem de `testbench/golden/linux-facts/linux_facts.json`, extraído
do Linux real pelo E05: 130 de 130 mensagens batem com a glibc e 14 de 14 mensagens reais de ferramentas
terminam exatamente em `": " + strerror(errno)` [H15].

## Modelo de execução: uma thread por pseudo-processo

### Por que o M:N do v1 caiu

`corosensei::Coroutine` é `!Send` de propósito: thread-local capturado na pilha vira data race quando a
corrotina muda de thread. Mover uma corrotina pra outra thread não compila (E0277); `may::spawn` exige
unsafe (E0133); o `generator` 0.8 compila a migração mas ela é unsound (medido: depois de migrar, a
corrotina usa o thread-local da thread antiga). **M:N stackful com migração não existe em Rust seguro**
[H01].

### Os três modelos medidos (E01)

| | A: thread por processo | B: corrosensei preso por worker | C: async próprio |
|---|---|---|---|
| troca de contexto | 2,9 µs (park/unpark) | 28 ns | 34 ns |
| spawn+exit+wait | 20,6 µs | 7,7 µs | 0,19 µs |
| RSS por processo parado | 10 KiB (34 KiB contando pilha de kernel) | 4,5 KiB | 0,5 KiB |
| 30 mil processos simultâneos | sim | sim | sim |
| pior pipeline contra o melhor modelo | 64% (1 CPU virtual), empata com 4 | 100% | 100% |
| stack overflow com `stacker` | sobrevive | **derruba o host** (stacker usa os limites da thread do worker) | sobrevive |
| thread-local por processo | natural | **9800 de 10 mil conferências erradas** sem troca manual | task-local ok |
| laço sem checkpoint | rebaixar a thread pra nice 19 devolve 99% da CPU ao vizinho | sem remédio | sem remédio (thread compartilhada) |
| builtins | código síncrono normal | código síncrono normal | tudo `async fn` |

**Escolha: modelo A** [H02] [H03] [H04] [H10]. Ele perde em troca de contexto, mas mesmo no pior pipeline
fica no nível de processos reais do Linux (1449 a 1730 MB/s nos mesmos pipelines nativos), sustenta 30
mil processos, e é o único que tem remédio pra laço sem checkpoint e que é seguro com `stacker` e
thread-locals de terceiros. B fica descartado por duas falhas de segurança medidas; C exige reescrever
todo builtin e não contém laço sem checkpoint.

Como funciona: o kernel configura N CPUs virtuais (tokens). Só a thread do processo corrente de cada
CPU virtual roda; as outras ficam em `park()`. Na troca, quem sai faz a contabilidade, devolve a CPU
virtual, acorda o próximo escolhido pelo EEVDF e para. Quem leva cada thread pra um núcleo físico é o
host. N fica menor ou igual aos núcleos reservados pro serviço (na VPS, 2); mais que isso devolve o
controle ao escalonador do host.

### Preempção, checkpoints e laço sem checkpoint

A preempção é cooperativa: toda chamada ao `Ctx` e todo `ctx.checkpoint()` (um `AtomicBool` por
processo, ligado por uma thread de timer) são pontos de troca. Checkpoint no salto pra trás de um
interpretador custa 0,07% e o processo cede em p99 de 0,5 µs depois do timer [H05].

Laço de CPU sem nenhum checkpoint não cede nem morre: nos quatro modelos o vizinho ficou parado e o
SIGKILL esperou o laço acabar [H07]. Mitigações, ambas obrigatórias:

1. **Checkpoint em todo interpretador nosso** (awk, jq, sed, shell, find). No jaq dá sem fork: o
   avaliador consulta `HasLut::lut()` a cada nó, e com um `DataT` próprio `last(range(1e18))` é
   interrompido em ms com 1,3% de custo [H27].
2. **Watchdog**: processo que estoura a fatia sem responder tem a thread host rebaixada pra nice 19
   (detectado em 8,7 ms). O rebaixamento é de mão única (voltar pra nice 0 dá EACCES sem privilégio), então
   a thread fica rebaixada até o processo morrer.

### Sinais, kill e panic

Sinais pendentes são entregues nos pontos de checagem. SIGKILL faz unwind com payload próprio
(`resume_unwind`), capturado por `catch_unwind` na entrada do processo; os `Drop` fecham fds. 800 de 800
kills limparam tudo, em 8 a 14 µs [H06]. Panic num builtin vira término anormal pro pai e o kernel
segue atendendo; a única armadilha medida é `lock().unwrap()` num `std::Mutex` envenenado, então o
kernel usa `parking_lot` ou trata `PoisonError` [H09].

### Stack overflow

Derruba o processo host inteiro em qualquer modelo [H08]. Interpretadores recursivos usam
`stacker::maybe_grow` com limite de profundidade (no modelo A isso funciona); e o processo host é
dividido em vários workers (seção Multiusuário) pra que um overflow que escape derrube só uma parte.

## Escalonador: EEVDF

Referência: **6.12.101 do Debian 13**, lida no código (`kernel/sched/fair.c` e `core.c` da árvore
stable). O `crates/sched` reproduz as funções do kernel com os mesmos nomes e a mesma ordem de passos.

- **Peso** do nice pela tabela `sched_prio_to_weight` (nice 0 = 1024) e divisão em ponto fixo com
  `sched_prio_to_wmult` e shift 32, como `__calc_delta`.
- **vruntime** avança `delta * 1024 / w`. **V** (`avg_vruntime`) é a média ponderada relativa a
  `zero_vruntime` (backport da 6.12.64+), guardada como somatório e carga; a divisão usa piso, e o
  `div_s64` trunca o divisor pra 32 bits como no kernel.
- **Elegível** se `Σ(v_j - v0)·w_j >= (v_i - v0)·Σw_j`, sem dividir. **Lag** limitado a
  `calc_delta_fair(max(2·slice, TICK_NSEC))`.
- **Fatia base** = 700000 ns na 6.12.101 (o v1 dizia 0,75 ms) vezes `1 + ilog2(min(ncpus, 8))`: 2,8 ms
  com 16 CPUs, 1,4 ms com as 2 da VPS [H14].
- **Árvore** ordenada só pela deadline, com empates na ordem de chegada (não há desempate por id), e
  augmentação `min_vruntime` e `min_slice` por subárvore.
- **pick_eevdf** desce pela augmentação; RUN_TO_PARITY via `protect_slice()`; PREEMPT_SHORT.
- **place_entity** com PLACE_LAG (lag escalado por `(W + w)/W`), PLACE_DEADLINE_INITIAL,
  PLACE_REL_DEADLINE; **DELAY_DEQUEUE** e **DELAY_ZERO**.
- Na 6.12 o tick só chama `update_curr`, e todo wakeup também encerra a fatia vencida do corrente.

Fidelidade medida contra o kernel do host: a divisão de CPU do simulador fica a no máximo **0,12 ponto
percentual** do kernel em 5 cenários de nice, e a latência de wakeup tem p50/p99 na mesma ordem de
grandeza (a diferença é o custo fixo de 4 a 6 µs do wakeup real, que o simulador não cobra) [H13].

**Travas**: uma por runqueue desde o início. A trava global não escala com operações de escalonamento
coladas (34% da vazão ideal com 2 workers) [H11].

### Grupos e limite de banda (multiusuário)

Divisão por processo deixa um usuário com `xargs -P 16` levar 16 vezes a CPU de outro. Por isso o
`sched` tem a hierarquia do `FAIR_GROUP_SCHED` (entidade de grupo por CPU com runqueue filha, peso
`cpu.weight` mapeado pra shares como no kernel, `calc_group_shares` sobre PELT de carga), na forma
usuário > sandbox > processo, e o controle de banda do CFS (`cpu.max`: quota, período, throttle,
unthrottle e distribuição de runtime). Medido contra cgroups v2 reais do host
(`systemd-run --user --scope -p CPUWeight/CPUQuota`):

- **Grupos** [H41]: grupo com 1 laço contra grupo com 8, pesos 100/100 e 100/300, e hierarquia de três
  níveis. Em 1 CPU a divisão bate a 0,04 ponto percentual do kernel (50,0/50,0 e 25,0/25,0); em 2 CPUs
  (par SMT, como a VPS) a 4,33 pontos, e quem se afasta do peso ideal é o kernel, migrando um laço do grupo
  grande pra CPU do pequeno.
- **Banda** [H42]: quotas de 20% e 50% com período de 100 ms, sozinho e competindo: fração a 0,05 ponto do
  kernel, 30 de 30 períodos estrangulados nos dois lados, execução até estrangular igual a 1% com a fase do
  timer casada (o kernel sorteia a fase em `init_cfs_bandwidth`).
- **Multi-CPU**: uma runqueue por CPU virtual com balanceamento de um domínio (como o domínio MC). A runqueue
  única com trava global chega mais perto do peso ideal, mas não é o modelo do kernel, não aceita
  afinidade, e rende 43% da vazão ideal com 2 workers e operações coladas.
- Aproximações documentadas no crate: PELT só com o sinal de carga, um domínio de balanceamento, e sem
  `select_task_rq_fair` no wakeup (a tarefa acorda na CPU em que dormiu), que é o próximo passo pra
  cargas que dormem e acordam em várias CPUs.

## Árvore rubro-negra aumentada

Feita à mão em `crates/rbtree`: arena `Vec<Node>` com índices `u32`, lista de nós livres, augmentação
genérica recalculada em toda inserção, remoção e rotação, cache do nó mais à esquerda, e API de
navegação pro pick. 100 mil sequências de proptest (15,4 milhões de operações, invariantes checadas a
cada uma) e 20 mil picks contra força bruta sem falha; no máximo 1,53x do `BTreeMap` (remove por chave),
mais rápida que ele no remove por handle, que é o que o escalonador usa; pick aumentado 213x melhor que o
linear com 4096 entidades [H12].

## Árvore de processos

Igual ao v1: pid, ppid, pgid, sid, estado, argv, environ, cwd, umask, fds, sinais, rlimits, contadores
de CPU vindos do escalonador; PID 1 embutido adota órfãos; zumbis até o `wait`; job control com grupos e
sessões. `pid_max` padrão = **4194304**, o valor do Debian real (o kernel com 16 CPUs usaria 32768, mas o
systemd sobe) [H14]. O usuário padrão é root, e root ignora permissões como no Linux (`cat` de arquivo
modo 000 sai com 0): o VFS reproduz CAP_DAC_OVERRIDE [H14].

## Sistema de arquivos

### tmpfs persistente

Medido no E03 [H17], com 100 mil arquivos:

- **Tabela de inodes e diretórios**: `imbl` 7 `OrdMap` (290 bytes por inode, readdir já ordenado).
  Snapshot e sandbox nova custam 6 a 17 ns, iguais com 1 mil e 100 mil arquivos, sem alocar. A primeira
  escrita depois de um snapshot copia 1,7 a 2,6 KiB, sem relação com a profundidade do diretório. Restore
  custa o que mudou, não o tamanho da imagem.
- **Conteúdo**: `imbl::Vector` de blocos `Arc<[u8]>` de 4 KiB. Mudar 1 byte de um arquivo de 100 MiB
  depois de um snapshot leva 2,3 µs e copia 8,5 KiB (com `Arc<Vec<u8>>`, 12,5 ms e o arquivo inteiro).
- **Memória**: 100 sandboxes derivadas custam 43 KiB cada sobre uma imagem de 115 MiB.
- **Trava**: `RwLock` por sandbox por padrão; tabela fatiada em 64 com trava por fatia quando houver
  escritores paralelos (7 M ops/s com 16 threads). `arc-swap` foi o pior em escrita.
- **fd guarda o número do inode**, não um `Arc<Inode>` (senão não vê escrita feita por outro caminho);
  arquivo desvinculado e aberto fica órfão na tabela até o último `close`; restore **não** volta o
  contador de inodes.
- Correção: 400 sequências aleatórias de operações bateram com o tmpfs real do host (`/dev/shm`) sem
  divergência.

**Ordem do readdir** (decidido pelo dono): a do tmpfs do Linux, do mais novo pro mais antigo, com um
índice por ordem de criação em cada diretório, já que o sandbox se apresenta como tmpfs. Desde
2026-10-05 o oráculo também roda os casos num tmpfs (`docker run --tmpfs /work`), então ordem do
readdir, tamanho de diretório e blocos do golden são os do tmpfs; antes, sobre o overlay/ext4 do
contêiner, o readdir saía em ordem de hash e diretórios mediam 4096.

### hostfs

Medido no E04 [H18]: nenhum candidato deixou ler o canário (29 casos e 1 milhão de tentativas de corrida
cada), mas **delegar a resolução ao kernel do host dá a semântica errada**. `cap-std` e
`openat2(RESOLVE_BENEATH)` transformam `..` acima da montagem e qualquer symlink absoluto em erro (sem
errno, ou EXDEV); `RESOLVE_IN_ROOT` prende tudo dentro da montagem. Nos dois casos um symlink pra
`/etc/passwd` dentro de `/work`, que no Linux leria o `/etc/passwd` do sandbox, falha.

Desenho adotado (29 de 29 iguais ao Linux, mesmo custo de um `openat2`):

- o hostfs não segue symlink: faz lookup de um nome por vez (`openat` com `O_PATH | O_NOFOLLOW`, `fstat`,
  `readlinkat` no próprio fd) e devolve symlinks pro namei do nosso VFS, que mantém a pilha de diretórios
  pra `..` e o limite de 40 links;
- caminho rápido: um `openat2` com `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS`
  resolve o resto do caminho numa syscall quando não há symlink; se o kernel recusar (ELOOP, EXDEV), o
  namei componente a componente assume;
- tudo com API segura do `rustix`, sem cap-std.

Hardlink pré-existente pra fora do diretório montado é lido normalmente (não é fuga de resolução): a
defesa é de política, não montar diretório com hardlink pra fora ou montar só leitura.

### procfs, devfs, namei, imagem base

Como no v1, com os formatos exatos tirados do Debian real (`testbench/golden/linux-facts/proc/`).
Constantes conferidas: `PATH_MAX` 4096, `NAME_MAX` 255, 40 symlinks resolvem e 41 dão ELOOP,
`/dev/full` dá ENOSPC [H14].

## File descriptors, pipes e terminal

Pipes com 65536 bytes por padrão e escrita atômica até `PIPE_BUF` = 4096 (medido: 0 de 16 mil blocos de
4096 misturados com 4 escritores; blocos de 256 KiB misturam) [H14]. `yes | head` termina com SIGPIPE
(exit 141 do `yes`), como no bash real.

Edição de linha: `reedline` e `rustyline` falam direto com o tty do host e não servem [H38]. O
`LineEditor` do `termwiz`, sobre uma implementação nossa da trait `Terminal` ligada ao pty do kernel,
passa 20 de 20 passos do roteiro de teclas, com zero acessos a tty ou termios do host. Um bug do termwiz
(Ctrl+letra chega em minúscula e o editor compara com maiúscula) é contornado mapeando as teclas no
nosso lado.

## Shell

O corpus real (E08, 54.707 chamadas Bash de agentes) define a prioridade [H22]: 24 comandos cobrem 90%
das ocorrências; pipeline aparece em 55% das chamadas, `&&` em 49%, `2>&1` em 24%, `$var` em 13%, glob em
11,5%, `$(...)` em 6%, heredoc em 3%; `if`, `while`, `case`, `[[ ]]`, arrays e `<(...)` ficam abaixo de
0,5% cada. O "sofisticado" é composição, não controle de fluxo.

- **Parser**: `brush-parser` 0.4 concorda com `bash -n` em 99,99% das chamadas reais e 95,6% da suíte do
  bash; o segundo nível (palavras cruas, `$(...)`) é re-parseado por nós. Faltam três correções na nossa
  camada: `case` sem parêntese de abertura dentro de `$(...)`, `select`, e extglob em `[[ ]]` [H37].
- **Interpretador**: nosso. O `brush-core` tem 190 acessos ao host e 122 `async fn` sobre tokio.
- **Referência de desenho**: `yash-env` (traits pequenas de sistema mais implementação virtual), só
  leitura, porque é GPLv3.

## Userland

Medido nos experimentos F (conformidade byte a byte contra o Debian, categoria pelo depscan).

| Ferramenta | Decisão | Número |
|---|---|---|
| Motor de regex POSIX | parser BRE/ERE nosso (regcomp do glibc e dfa.c) + `regex-automata` pra padrão sem backref + `ferroni` (Oniguruma em Rust) pra backref, montados em leftmost-longest | 99,27% do corpus de borda, 99,99% do uso real [H23] |
| grep | `grep-searcher` + `grep-matcher` com front-end e printer GNU nossos (~1200 linhas, prontas) | 185/185 [H24] |
| sed | fork do sed do bashkit (já roda sobre VFS) com o motor acima | 94,2% leniente antes do fork [H25] |
| awk | à mão (gawk como alvo), com o motor POSIX e VM que suspende em E/S | melhor candidato 67,7% [H26] |
| jq | `jaq-core`/`jaq-std`/`jaq-json` com a camada do F04 (CLI 1.7.1, leitura, números, erros de topo, checkpoint), e fork localizado do núcleo pro que falta (decisão do dono) | 83,5% nas suítes do jq 1.7.1, 86,6% estrito na CLI de agente [H27] |
| coreutils | fork por utilitário do uutils sobre um `uucore` portado uma vez | portado = original em 103/103; 736 linhas em 5 utils, 1471 no uucore [H28] |
| find, xargs | fork do findutils com o motor POSIX; executor do xargs nosso | find igual ao original em 50/51 [H29] |
| diff | `imara-diff` (Myers, `postprocess_no_heuristic`) + formatador e front-end nossos | 790/800 iguais ao GNU no corpus de empates [H30] |
| patch | localizador nosso sobre o parser do `diffy`, escrito a partir de especificação e testes (não do código GPL) | 500/500 no aleatório [H30] |
| tar, gzip, bzip2, xz, lzip, zstd, zip | `tar`, `flate2`/`zlib-rs`, `bzip2`, `lzma-rust2`, `structured-zstd`, `zip`; CLIs nossos | interop 100% nos dois sentidos, Rust puro [H31] |
| date | `parse_datetime` + `jiff` com camada nossa (strftime GNU completo, horário de verão, `TZ="..."` no `-d`) | 240/264 [H32] |
| csv | `csv` | encaixa [H33] |
| yq | jaq + `saphyr-parser` + resolvedor de escalares nosso; emissor `-y` nosso | 92/105 [H33] |
| file | `pure-magic` + `magic-db` + detecção de texto nossa | 42/48 [H33] |
| bc | fork do bc do posixutils-rs | 92/118 depois do zero à esquerda [H33] |
| curl, wget | CLIs nossos sobre `ureq` 3 (versão fixada) com política única no Resolver e no Connector, `proxy(None)` | 12/12 cenários de allowlist [H34] |
| sqlite3 | `rusqlite` + `sqlite-plugin` (trait `Vfs` segura) sobre o tmpfs, CLI nosso | 82/86, interop nos dois sentidos [H35] |
| git | `gix-*` de baixo nível sobre o VFS, refs, índice e worktree nossos | `git fsck --strict` limpo, 15/15, 1314 linhas [H36] |
| JavaScript | `boa` com `IdleModuleLoader` | 29/29, 0,55 ms de startup [H39] |
| Lua | `mlua` (C) sem `dofile`/`loadfile` | 25/25 [H39] |
| Python | `monty` com shim de `json.load`/`dump`; RustPython descartado (2088 syscalls de host) | 24/28 [H39] |

Detalhes que viraram requisito:

- **Aspas**: em C.UTF-8 o `mkdir` escreve `‘d’` (aspas curvas, `quote()`) e rm, mv, cp, ln e ls escrevem
  `'d'` (`quoteaf()`); o porte mantém a função de cada utilitário.
- **jq nunca passa por `jaq_core::unwrap_valr`**: o `halt` derruba o processo inteiro em três linhas de
  base, e `env` não pode mostrar o ambiente do host [H40].
- `datetime('now')` do sqlite e o "agora" do `date` vêm do relógio do sandbox, não do host.

**jq pelo jaq com a camada nossa** (decidido pelo dono). Rust puro, categoria (a), sem unsafe
(`jaq-core` tem `forbid(unsafe_code)`), projeto maduro. O que essa escolha exige:

- **O que a camada do F04 já fecha** (~2600 linhas): CLI 1.7.1, leitura de JSON, formatação de números e
  mensagens de erro de topo; zero falhas de CLI restantes.
- **Checkpoint pronto**: `DataT` próprio com `HasLut::lut()`, uma `range/3` nossa e as definições do jq
  pra `first`, `last` e `limit` interrompem `last(range(1e18))`, `repeat` e recursão em ms, a ~1,3% de
  custo.
- **O que fica pro fork localizado do núcleo** (a distância de 83,5% pra 100% nas suítes): mensagens
  dentro de `try/catch` (o `jaq_core::Error` é opaco), atribuição em `null` e além do fim do array,
  `?//` e desestruturação alternativa, `1/0` dando infinito, e a regex.
- **Regex do jq**: o `jaq-std` usa `regex-bites` (sem lookaround, semântica diferente do Oniguruma). A
  troca natural é o `ferroni`, o Oniguruma em Rust puro que o F01 já validou (categoria (a)), que dá a
  mesma sintaxe e semântica do jq real sem trazer C.
- **Tempo e fuso**: sobrescrever as nativas de tempo local pra que `TZ` e "agora" venham do `Ctx`.
- **`halt` e `env`**: nunca passar por `jaq_core::unwrap_valr` (o `halt` derrubaria o processo host);
  `halt` vira exit do pseudo-processo e `env`/`$ENV` mostram o ambiente do sandbox.
- **Referência**: o `qj` (porte do jq 1.8.1, 98,6% nas suítes) fica como oráculo de comportamento e fonte
  de consulta pra fechar as divergências, não como dependência.

Projetos existentes (bashkit, rust-bash, kaish, wasmsh) não servem de base: nenhum passa de 44% leniente
no shell; o bashkit chega a 53% em grep/sed/awk e 51% em jq, e é a fonte do fork do sed [H40].

## Limites de recursos e contabilidade

Os limites do v1 continuam (`RLIMIT_CPU` do escalonador, `RLIMIT_NOFILE`, `RLIMIT_NPROC`, `RLIMIT_FSIZE`,
cota do tmpfs, timeout de parede). Memória, medida no E07 [H16]:

- Allocator global: `tracking_allocator::Allocator<MiMalloc>` com uma tabela de grupos nossa (sem unsafe;
  o `#[global_allocator]` compila com `forbid`). Atribuição exata em 11 cenários, inclusive quando um
  processo aloca e outro libera; no sort de 1M linhas o conjunto fica 17% mais rápido que o System puro
  (a contabilidade em si custa 15 a 18% sobre o mimalloc).
- Limite: o allocator liga uma flag quando o processo passa do teto e o checkpoint seguinte mata. Negar a
  alocação dentro do allocator aborta o host inteiro (demonstrado), assim como `Vec::with_capacity`
  gigante: builtins que tiram tamanho da entrada usam `try_reserve`.
- Estruturas do kernel (buffers de pipe, conteúdo de arquivo) ficam fora do rastreamento
  (`AllocationRegistry::untracked`) e são cobradas num contador próprio, em lotes pra cota da sandbox.
- Risco: a crate está parada desde 2022 e não sobrescreve `realloc`; o caminho é PR upstream, porque um
  fork nosso seria `unsafe impl`.

## Isolamento

Medido no E06:

1. **Lint no nosso código** (`unsafe_code = "forbid"`, `disallowed_methods`, `disallowed_macros`): é
   higiene, não garantia. Um crate que lê o host por dependência passa limpo no clippy [H19], e 7 de 9
   formas de unsafe gerado por macro de outro crate compilam sob `forbid` [H20].
2. **depscan**: a única camada estática que enxerga I/O de host e unsafe vindos de dependência e de macro.
   Toda dependência nova de userland passa por ele.
3. **Runtime**: uma thread "spawner" por sandbox aplica Landlock (sem FS do host fora das montagens) e
   seccomp (sem `socket`, `connect`, `execve`, `execveat`, fork, `io_uring_setup`) uma vez, e cria as
   threads dos pseudo-processos, que herdam de graça. Medido: thread restrita leva EACCES/EPERM, vizinha
   e principal seguem livres; Landlock custa +350 ns por `openat`, seccomp ~30 ns por syscall [H21].

Restrições que isso impõe ao kernel: trabalho de pseudo-processo nunca vai pra pool global (rayon, tokio),
porque a thread do pool não é restrita; a rede (curl) roda fora do filtro ou com as regras de rede do
Landlock; montagem nova chega por fd aberto pelo kernel; a lista do seccomp cobre io_uring e x32. Isso
protege contra código seguro fazendo I/O de host, não contra corrupção de memória numa dependência.

## Multiusuário na VPS

Alvo de produção: a VPS do dono (Ubuntu 24.04, kernel 6.8, **2 vCPUs** EPYC, 7,8 GiB com ~3 GiB livres,
cgroups v2, Landlock ABI v4), servindo vários usuários ao mesmo tempo, ao lado dos apps do Dokploy.

- **Vários processos host**: um supervisor e alguns workers, cada um com um conjunto de sandboxes. Um
  stack overflow que escape do `stacker` [H08] ou um abort de allocator derruba só aquele worker, que o
  supervisor reinicia. Sessões do worker que caiu se perdem, a menos que tenham snapshot persistido.
- **CPU**: 2 CPUs virtuais por worker no máximo (ou 1, se precisar deixar folga pros outros apps). O teto
  do serviço inteiro fica com o cgroup do container no Dokploy. Dentro, o EEVDF hierárquico divide entre
  usuários pelo peso, e `cpu.max` por usuário segura o guloso (ambos medidos contra o kernel) [H41] [H42].
- **Memória**: com ~3 GiB livres, cada sandbox tem orçamento (allocator rastreado + contador do kernel) e
  há controle de admissão (máximo de sandboxes, de processos e de memória por usuário). Um processo no
  modelo A custa ~34 KiB reais; mil processos são 34 MiB.
- **Landlock na VPS é ABI v4**: as regras de FS e de rede TCP funcionam; o escopo de sinais e sockets
  abstratos (v6) degrada em modo best effort.

## Interfaces com o mundo

Iguais ao v1: `osh` interativo, `osh -c`, `osh script.sh`, e o servidor JSON-RPC pro agente (`exec` com
streaming, `session.open`, `fs.*` diretos, `snapshot`, `restore`, `ps`, `kill`, `export`/`import`).

## Bancada de testes

`testbench/` (ver `testbench/README.md`): oráculo Debian 13 pinado por digest, corpus (casos de agente,
suítes upstream, corpus real minerado dos transcripts, só local), harness de comparação byte a byte,
depscan, 15 experimentos com README e `results/*.json`, e o runner que gera `docs/bench-report.md`.

## Roteiro

1. **Núcleo**: kernel no modelo A sobre `sched` e `rbtree`, tmpfs com imbl, fds, pipes, `wait4`, shell
   com o que o E08 mostrou (pipeline, listas, redirecionamento de fd, `$var`, glob, `$(...)`), e os 24
   comandos que cobrem 90% do uso. Meta: `ls | sort | head`.
2. **VFS completo** (permissões com bypass do root, symlinks, `/proc`, `/dev`), sinais, motor de regex
   POSIX, grep e sed.
3. **Ferramentas**: coreutils portados, find, xargs, awk, jq (jaq + camada), tar e compressão, diff,
   patch, `ps`/`top`, limites, JSON-RPC.
4. **Produção na VPS**: supervisor e workers, grupos e banda no escalonador, orçamento de memória,
   Landlock e seccomp por sandbox, hostfs, git, sqlite, curl com allowlist, interpretadores.

## Decisões do dono

- **jq**: jaq com a camada nossa e fork localizado do núcleo (2026-10-02).
- **Ordem do readdir**: a do tmpfs do Linux, do mais novo pro mais antigo (2026-10-02).

## Decisões pendentes

- **awk**: gawk assumido como alvo (pergunta sem resposta na sessão).

## Registro de hipóteses

Fonte: `testbench/hypotheses.toml` e `testbench/results/`. Resumo do número que decidiu em
`docs/bench-report.md`.

| Id | Hipótese | Exp. | Veredito |
|---|---|---|---|
| H01 | Corrotinas stackful permitem M:N com migração entre workers em Rust seguro | E01 | Refutada |
| H02 | Troca de contexto de corrotina é muito mais barata que handoff de thread, e isso pesa no throughput de pipelines | E01 | Confirmada (pesa só com 1 CPU virtual) |
| H03 | Pilha virtual por processo só ocupa memória física quando tocada; milhares de processos custam pouco | E01 | Confirmada |
| H04 | Criar pseudo-processo custa microssegundos e cabem dezenas de milhares num processo host | E01 | Confirmada |
| H05 | Checkpoint com `AtomicBool` custa quase nada e dá preempção com granularidade fina | E01 | Confirmada |
| H06 | SIGKILL via unwind libera fds e roda Drops | E01 | Confirmada |
| H07 | Laço sem checkpoint não pode ser preemptado nem morto; rebaixar a thread host mitiga | E01 | Confirmada |
| H08 | Stack overflow derruba o host; `stacker` mitiga | E01 | Parcial (não no modelo B) |
| H09 | Panic num builtin fica isolado e não envenena o kernel | E01 | Confirmada |
| H10 | Thread-local de processo corrente viabiliza o shim e a contabilidade | E01 | Parcial (só no modelo A) |
| H11 | Runqueue com trava global escala até uns 8 workers | E02 | Parcial |
| H12 | Árvore rubro-negra em arena com índices u32 é correta e competitiva com `BTreeMap` | E02 | Confirmada |
| H13 | O nosso EEVDF reproduz a divisão de CPU e a latência do kernel 6.12 do host | E02 | Confirmada |
| H14 | As constantes do Linux do design batem com o sistema real | E05 | Parcial (pid_max e fatia corrigidos) |
| H15 | errno e strerror da glibc reproduzidos byte a byte | E05 | Confirmada |
| H16 | Contabilidade de memória por processo sem unsafe nosso, com overhead aceitável | E07 | Confirmada (sobre mimalloc) |
| H17 | tmpfs persistente dá snapshot, restore e sandbox nova em O(1) | E03 | Confirmada |
| H18 | `openat2` BENEATH ou `cap-std` impedem qualquer fuga do hostfs | E04 | Parcial (sem fuga, semântica errada) |
| H19 | `disallowed_methods` garante isolamento em tempo de compilação | E06 | Refutada |
| H20 | `forbid(unsafe_code)` pega todo unsafe nosso, inclusive gerado por macro | E06 | Refutada |
| H21 | Landlock e seccomp por thread isolam o pseudo-processo sem afetar o resto do host | E06 | Confirmada |
| H22 | Agentes escrevem bash sofisticado, mas concentrado o bastante pra priorizar | E08 | Confirmada |
| H23 | Existe motor de regex Rust puro com semântica POSIX do GNU | F01 | Parcial (combinação + parser nosso) |
| H24 | As crates do ripgrep dão um grep GNU com um front-end de flags | F02 | Parcial (printer nosso) |
| H25 | Existe sed em Rust adotável | F02 | Parcial (fork do bashkit) |
| H26 | Nenhum awk em Rust chega perto do gawk | F04 | Confirmada |
| H27 | jaq tem compatibilidade alta com o jq real | F04 | Parcial |
| H28 | uutils compila quase sem alteração trocando `std::fs` por um shim | F06 | Parcial |
| H29 | uutils/findutils serve de ponto de partida pra find e xargs | F06 | Parcial |
| H30 | `similar` gera unified diff idêntico ao GNU | F08 | Refutada |
| H31 | Crates de compressão e arquivo em Rust puro interoperam com o GNU | F08 | Confirmada |
| H32 | `date` do GNU sai de crates prontas | F08 | Parcial |
| H33 | bc, file, yq e csv têm crate que encaixa | F08 | Parcial |
| H34 | curl/wget sobre `ureq` aplicam a allowlist num ponto só, sem tokio | F12 | Confirmada |
| H35 | `rusqlite` registra VFS próprio e o banco mora no sandbox | F12 | Parcial (via sqlite-plugin) |
| H36 | `gix` aceita customizar o acesso ao repositório | F12 | Parcial (gix-* baixo nível) |
| H37 | brush serve de base pro shell | F15 | Parcial (parser sim, interpretador não) |
| H38 | `reedline` serve pro shell interativo | F15 | Refutada |
| H39 | Python, Lua e JS embutíveis com todo I/O pelo kernel | F15 | Parcial |
| H40 | Projetos existentes já cobrem parte relevante do plano | F15 | Parcial |
| H41 | EEVDF hierárquico divide CPU entre usuários pelo peso do grupo | E02 | Confirmada |
| H42 | `cpu.max` limita um usuário guloso com o padrão de throttling do Linux | E02 | Confirmada |
