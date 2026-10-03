# E07: contabilidade de memória por pseudo-processo

No modelo de execução A (uma thread do SO por pseudo-processo), o kernel precisa saber quantos bytes cada
processo tem vivos pra aplicar limite de memória. Escrever um allocator global é `unsafe impl GlobalAlloc`,
proibido no nosso código; então a pergunta é se existe allocator pronto que atribua alocações a um grupo por
thread, com API segura, custo aceitável e atribuição certa quando a memória troca de thread.

## Hipóteses

| Id | Frase | Critério |
|---|---|---|
| H16 | Contabilidade de memória por processo é possível sem unsafe nosso, com overhead aceitável | Allocator pronto com grupos por thread: overhead no sort de 1M linhas < 15%, atribuição correta quando A aloca e B libera, latência de detecção de limite estourado. |

O experimento também mede o que o design pressupõe em volta da H16: que o limite duro dentro do allocator
aborta o host (e por isso o limite vira contagem + kill no próximo checkpoint) e quanto custa contar à parte
as estruturas do kernel (buffers de pipe, conteúdo de arquivo).

## Método

**Um binário por candidato.** O allocator global é um por binário, então cada candidato é um `src/bin/cand-*.rs`
que declara o `#[global_allocator]` com o tipo da crate (o atributo não é `unsafe`; tudo compila com
`unsafe_code = "forbid"`), implementa o trait `Accounting` (`src/accounting.rs`) e chama `cli::run`. O
orquestrador (`src/main.rs`) roda cada binário como subprocesso, lê uma linha JSON por comando e grava
`results/e07-mem-accounting.json`. A lib nunca referencia crate que declare o próprio allocator (a
`allocation-counter` declara), senão todo binário herdaria esse allocator.

**Bytes vivos** são os bytes pedidos (`Layout::size`) que o processo alocou desde que começou e ainda não
liberou, segundo a conta do candidato. Cada adaptador devolve esse número relativo ao início do processo.

**Tabela por grupo (nossa, `src/group_table.rs`).** O `tracking-allocator` só entrega eventos com o id do
grupo; a conta fica com a gente. Um vetor estático de 65536 slots de 64 bytes (um por linha de cache, no
`.bss`), indexado por `id & (SLOTS - 1)`. O cabeçalho que a crate grava antes de cada alocação traz o grupo de
origem, então o `dealloc` debita quem alocou. O contador do dono é escrito com load e store relaxados (só a
thread do grupo escreve nele) e as liberações de outras threads vão num segundo contador com `fetch_add`; o
grupo raiz, compartilhado por todas as threads fora de processo, usa sempre `fetch_add`. O slot também guarda
o teto e uma flag que o próprio tracker liga quando o processo passa dele.

**Cargas** (`src/workloads.rs`), sempre dentro de um pseudo-processo:

- `sort`: 1M linhas aleatórias de 1 a 64 caracteres (33,5 MB), uma `Vec<u8>` por linha, `sort()` estável,
  saída num `Vec` que cresce por realocação, tudo liberado no fim. É a carga do critério.
- `sort-borrowed`: o mesmo sort com linhas emprestadas da entrada (como o uutils faz), poucas alocações.
- `small`: 10M operações num anel de 4096 buffers de 8 a 512 bytes (padrão de interpretador).
- `small-mt`: a mesma coisa em 16 pseudo-processos simultâneos, 2M operações cada.

O tempo medido é o de CPU das threads da carga (`clock_gettime(CLOCK_THREAD_CPUTIME_ID)` via `rustix`), que
não conta a espera na fila do escalonador; o de parede fica registrado ao lado. O `schedstat` de `/proc` foi
descartado depois que o teste mostrou que ele só atualiza no tick do escalonador (4 ms).

**Medida do critério: processos vivos pareados.** A máquina é compartilhada com outros 11 agentes e a carga
oscilou de load 3 a load 750 durante o desenvolvimento; rodadas sequenciais deram, pro mesmo binário,
medianas de 8% a 21%. A medida que decide usa processos vivos (`serve-sort`): cada participante gera a
entrada uma vez e roda um sort a cada linha `go` do stdin. Dois modos com os mesmos processos, razão por
iteração, 30 iterações no conjunto principal e 20 no do mimalloc:

- **simultâneo** (passo travado): `go` pra todos de uma vez, a mesma iteração de cada um sofre a mesma carga
  dos vizinhos; viés conhecido: eles disputam banda de memória entre si, o que infla a base e dilui custo fixo;
- **alternado**: um por vez, em ordem sorteada a cada iteração, sem disputa mútua, pares a ~1 s de distância.

Vale a **pior das duas medianas**. Os participantes são System, `tracking-allocator` e o mesmo binário do
`tracking-allocator` com `AllocationRegistry::disable_tracking()` (sobra só o cabeçalho de 8 bytes e o realloc
sem override), o que decompõe o custo; e, num segundo conjunto, System, mimalloc, `tracking-allocator` sobre
mimalloc e a versão desligada dele. A tabela geral (todos os candidatos, rodadas sequenciais de ordem
sorteada) é contexto; ali as notas usam a razão dos mínimos.

**Corretude** (`src/scenarios.rs`), com sincronização por `Barrier` e `Mutex` (futex, sem alocação dentro da
janela medida):

- *A aloca e B libera*: A aloca 1000 caixas de 1000 bytes (1.016.000 bytes com o vetor), passa pra B, B
  libera. Certo se A volta ao que era e B não muda. Em seguida A aloca 1 MiB, B recebe e cresce pra 4 MiB:
  certo se a memória passa a ser de B (A em zero, B em 4 MiB).
- *Bytes vivos controlados*: 11 casos com valor esperado exato (Vec de 1 MiB, 10k caixas de 100 bytes,
  crescimento por `push`, `shrink_to_fit`, tudo liberado, alocação do kernel dentro do escopo de exclusão,
  dois processos simultâneos com 3 e 5 MiB lidos de dentro e de fora).
- *Resíduo na saída*: 1 MiB em `thread_local`, liberado pelos destrutores de TLS depois que o processo
  termina; lido de fora depois do `join`.
- *Custos fixos*: criar e entrar no grupo, ler o contador de dentro e de fora, entrar no escopo do kernel.

**Estouro de limite**: o processo aloca pedaços de 64 B ou 64 KiB (preenchidos) com teto de 64 MiB. Três
formas de perceber: o checkpoint lê os bytes vivos a cada N alocações (`self_poll`); o checkpoint lê a flag
que o allocator ligou (`flag`); um vigia lê de fora a cada 100 µs ou 1 ms e liga a flag de kill, que o
checkpoint lê a cada 64 alocações (`watcher`). Mede quanto passou do teto e quanto tempo levou do cruzamento
até o processo parar, e confere que o "kill" (soltar tudo) devolve os bytes vivos a zero. O teto é múltiplo do
pedaço e o cruzamento cai logo depois de um checkpoint: é o pior caso de fase (estouro de N x pedaço).

**Demonstrações em subprocesso** (`src/demos.rs`): três threads vizinhas batem o coração enquanto um processo
guloso faz o pedido que o allocator recusa; o orquestrador olha o sinal de término e o stderr.

**Contabilidade explícita do kernel** (`src/kernel_acct.rs`): pipe à moda do Linux (16 páginas de 4 KiB, a
página é alocada no `write`, cobrada do dono e devolvida no `read`) e arquivo de tmpfs (appends de 4 KiB, cobra
o tamanho lógico, devolve no truncate), sem contador, com contador do processo, com contador do processo e da
sandbox e, no binário do `tracking-allocator`, com as alocações do kernel dentro de
`AllocationRegistry::untracked`. Mais o microbenchmark do contador atômico isolado, em uma thread e em 16
(contador próprio, contador compartilhado e compartilhado em lotes de 64 KiB).

### Como rodar

```sh
cd testbench/experiments/e07-mem-accounting
cargo run --release            # compila os binários de candidato e grava results/e07-mem-accounting.json (~5 a 7 min)
E07_QUICK=1 cargo run --release   # só confere o encanamento; não grava (E07_WRITE=1 grava)
cargo test --release           # 16 testes de unidade + 10 de integração (as demonstrações rodam em subprocesso)
```

## Candidatos

Pesquisa no crates.io por allocators com contagem; os que têm algum tipo de divisão por thread ou de limite
viraram binário, os outros foram eliminados pela leitura do código.

| Crate | Versão | Mecanismo | Grupo por processo | Lê de fora | A aloca e B libera | Encaixe |
|---|---|---|---|---|---|---|
| `tracking-allocator` (+ nossa tabela) | 0.4.0 | cabeçalho com id do grupo; tracker recebe origem e grupo corrente | sim (token por thread) | sim | **certo** | fits_with_work |
| `tracking-allocator` sobre `mimalloc` | 0.4.0 + 0.1.52 | o mesmo, allocator interno mimalloc (C) | sim | sim | **certo** | fits_with_work |
| `alloc-track` | 0.4.0 | DashMap ponteiro para thread, 1024 x 1024 contadores | sim (índice de thread) | sim, ~88 ms por leitura | certo | does_not_fit |
| `jqf-resource` | 0.1.1 | conta `!Send` instalada na thread, teto com slab de emergência de 1 MiB | sim (por thread) | não | errado (A fica com a carga) | does_not_fit |
| `alloc_count` | 0.4.0 | `Cell` em `thread_local` + 6 atômicos globais | sim (por thread) | não | errado (A +, B -) | does_not_fit |
| `allocation-counter` | 0.8.1 | pilha `thread_local`, número só no fim do `measure()` | sim (por thread) | não, nem de dentro | errado | does_not_fit |
| `tikv-jemallocator` + `tikv-jemalloc-ctl` | 0.7.0 | `thread.allocatedp`/`deallocatedp` do jemalloc | sim (por thread) | não (`!Send`) | errado | does_not_fit |
| `cap` | 0.1.2 | contador e teto globais | não | só global | não se aplica | does_not_fit |
| `stats_alloc` | 0.1.10 | 6 contadores globais | não | só global | não se aplica | does_not_fit |
| `accounting-allocator` | 0.2.0 | contadores por thread, API só com o agregado | não | só global | não se aplica | does_not_fit |
| `staging-tracking-allocator` | 2.0.0 | spinlock global; `start_tracking` é `unsafe fn`; GPL-3.0 | não | só global | | does_not_fit (leitura) |
| `mod-alloc`, `rallo`, `re_memory`, `peakmem-alloc` | | profilers de contagem global ou por ponto de chamada | não | | | does_not_fit (leitura) |

`System` (malloc da glibc) e `mimalloc` puro entram como linha de base.

## Resultado

Números da rodada gravada em `results/e07-mem-accounting.json` (host Debian 13, kernel 6.12.101, Ryzen 7 5700
com 16 threads, `vm.overcommit_memory = 0`).

### Custo no sort de 1M linhas (CPU, processos pareados)

| Arranjo | contra o System, alternado | simultâneo |
|---|---|---|
| `tracking-allocator` sobre System | **+16,7%** (quartis 14,8% a 20,6%) | **+15,0%** (quartis 13,4% a 17,1%) |
| só cabeçalho e realloc (rastreamento desligado) | +8,9% | +7,8% |
| rastreamento em si (ligado contra desligado) | +7,8% | +6,5% |
| `mimalloc` puro | -30,5% | -28,0% |
| `tracking-allocator` sobre mimalloc | **-16,9%** | **-17,5%** |
| `tracking-allocator` sobre mimalloc contra o mimalloc puro | +17,8% | +14,6% |

Metade do custo é o cabeçalho de 8 bytes por alocação (o pico de RSS do sort sobe de 146,5 pra 159,6 MiB, +9%)
e o `realloc` que a crate não sobrescreve (sempre aloca, copia e libera, perdendo o `mremap` da glibc nos
buffers grandes); a outra metade é o rastreamento (TLS com `RefCell`, chamada dinâmica pro tracker, a nossa
tabela). A troca do contador do dono por load e store sem `lock` ganhou cerca de 1 ponto, dentro do ruído.

### Tabela geral (contexto; razão dos mínimos de CPU contra o System)

| Candidato | sort 1M | sort emprestado | pequenas, 1 thread | pequenas, 16 threads |
|---|---|---|---|---|
| tracking-allocator | +19,2% | +3,3% | +77,6% | +57,7% |
| tracking-allocator+mimalloc | -12,0% | +2,3% | +34,9% | +63,2% |
| mimalloc | -28,0% | +1,2% | -39,1% | -6,1% |
| alloc-track | +66,3% | +1,3% | +443,9% | +710,9% |
| jqf-resource | +3,1% | +0,4% | +41,6% | +32,6% |
| alloc_count | +4,3% | +2,8% | +54,9% | +1558,8% |
| allocation-counter | -1,5% | +1,4% | +27,6% | +14,6% |
| jemalloc (contadores por thread) | -23,7% | +1,2% | -50,3% | -52,7% |
| cap | -0,8% | -0,2% | +17,1% | +749,5% |
| stats_alloc | -0,7% | +1,5% | +5,4% | +1307,8% |
| accounting-allocator | +4,1% | +2,9% | +41,6% | +28,3% |

Os contadores globais atômicos (`stats_alloc`, `cap`, os seis globais do `alloc_count`) custam pouco numa
thread e explodem em 16, por disputa da linha de cache. O sort emprestado (poucas alocações) quase não sente
nenhum allocator. As somas de verificação das cargas batem entre todos os allocators.

### Corretude

| Candidato | A aloca, B libera (A / B depois) | B realoca o buffer de A | Casos controlados exatos | Resíduo depois da saída |
|---|---|---|---|---|
| tracking-allocator (as duas variantes) | 0 / 0 | B fica com 4 MiB, A em 0 | 11/11 | 0 |
| alloc-track | 0 / 0 | certo | 10/11 (sem escopo do kernel) | 0 |
| jqf-resource | +1.016.000 / 0 (piso em zero) | errado | 8/9 | sem leitura de fora |
| alloc_count | +1.016.000 / -1.016.000 | errado | 9/9 | sem leitura de fora |
| allocation-counter | +2.064.576 / -2.064.576 no fim | errado | sem leitura no meio | sem leitura de fora |
| jemalloc (contadores por thread) | +1.040.384 / -1.040.384 | errado | 8/9 (classe de tamanho: 10k x 100 B contam 112 B cada) | sem leitura de fora |
| cap, stats_alloc, accounting-allocator | só global (delta 0) | não se aplica | 6/9 (sem grupo nem escopo) | |

Custos fixos do tracking-allocator: criar e entrar no grupo de um processo ~15 ns; ler o contador ~2 ns de
dentro e de fora; entrar no escopo do kernel (`untracked`) ~2 ns. No `alloc-track`, abrir um processo custa
~186 ms (a crate não expõe o índice da thread e o adaptador o descobre com um marcador, entre dois relatórios)
e cada leitura ~88 ms (`thread_report()` varre 1024 x 1024 contadores e aloca 1M Strings).

### Estouro de limite (teto de 64 MiB, pior caso de fase)

| Candidato e forma | pedaço | a cada | estouro máximo | latência mediana |
|---|---|---|---|---|
| tracking, flag do allocator | 64 B | 1 | 64 B | 40 ns |
| tracking, flag do allocator | 64 B | 64 | 4 KiB | 8,7 µs |
| tracking, flag do allocator | 64 KiB | 64 | 4 MiB | 1,38 ms |
| tracking, flag do allocator | 64 KiB | 1024 | 64 MiB | 21,9 ms |
| tracking, lendo o contador | 64 KiB | 64 | 4 MiB | 1,56 ms |
| tracking, vigia a cada 100 µs | 64 B | 64 | 36 KiB | 177 µs (vigia: 37 µs) |
| tracking, vigia a cada 1 ms | 64 B | 64 | 700 KiB | 613 µs (vigia: 613 µs) |
| alloc-track, vigia | 64 B e 64 KiB | 64 | não detectou em 5/5 (a leitura de 88 ms perde a corrida) | |
| jqf-resource, flag | 64 B | 1 a 1024 | 64 B a 64 KiB | 30 ns a 10,8 µs |
| jqf-resource, flag com 64 KiB a cada 64, ou lendo o contador | | | **o host aborta (SIGABRT)**: o slab de 1 MiB acaba antes do checkpoint | |
| jemalloc, lendo o contador | | | detecta antes de cruzar (a conta usa a classe de tamanho) | |

A latência é o tempo até o próximo checkpoint: com a flag ligada pelo próprio allocator, o kernel não precisa
de vigia e o estouro máximo é N x tamanho do pedido. Em todas as formas que detectam, soltar tudo no "kill"
devolve os bytes vivos do processo a zero.

### O limite duro dentro do allocator aborta o host

| Demonstração (subprocesso, três vizinhos vivos) | Resultado |
|---|---|
| `cap` com teto a 32 MiB de folga, pedido de 64 MiB (`vec!`), dentro de `catch_unwind` | **SIGABRT** do host inteiro: `memory allocation of 67108864 bytes failed` |
| o mesmo com `try_reserve_exact` | sobrevive, devolve erro |
| System sem limite nenhum, `Vec::with_capacity(32 TiB)` | **SIGABRT**: o `mmap` é recusado (overcommit heurístico) e `handle_alloc_error` aborta |
| o mesmo com `try_reserve_exact` | sobrevive, devolve erro |
| `jqf-resource` com teto de 32 MiB, pedido de 4 KiB depois do teto | sobrevive: o slab serve e o checkpoint recusa com erro tipado |
| `jqf-resource`, pedido de 4 MiB depois do teto | **SIGABRT**: maior que o slab, o allocator devolve nulo |
| `alloc-track`, 1200 threads criadas em sequência | **SIGABRT** na thread 1025 (índice fora do vetor de 1024 dentro do allocator) |

`catch_unwind` não segura porque não há unwind: `handle_alloc_error` chama `abort`. O `set_alloc_error_hook`
e o `-Z oom=panic` seguem instáveis no Rust 1.98.

### Contabilidade explícita do kernel

| Medida | System | tracking-allocator |
|---|---|---|
| par cobrar + devolver, contador próprio, 1 thread | 3,2 ns | 3,2 ns |
| o mesmo em 16 threads, cada uma no seu contador | 4,8 ns | 4,0 ns |
| 16 threads no mesmo contador (cota da sandbox) | **420 ns** | 419 ns |
| 16 threads, contador da sandbox em lotes de 64 KiB | 29,5 ns | 27,2 ns |
| operação de pipe de 4 KiB (CPU, mínimo) | 351 ns | 401 ns |
| custo esperado no pipe: processo / processo + sandbox | +0,9% / +1,8% | +0,8% / +1,6% |
| medido no pipe numa thread (mínimo) | +1,7% / +1,6% | -0,6% / +1,4% (com `untracked`: -0,5%) |
| append de 4 KiB em arquivo (CPU, mínimo); esperado | 460 ns; +0,7% | 920 ns; +0,4% |

O contador próprio do processo custa ~1 a 2% do caminho de um pipe, abaixo do ruído da medida de ponta a ponta
(os testes conferem que tudo que é cobrado é devolvido). O que não escala é um contador compartilhado por todas
as threads da sandbox: 420 ns por par com 16 escritores. Em lotes de 64 KiB cai pra 30 ns, ao preço de a cota
da sandbox atrasar até 16 x 64 KiB.

## Veredito

**H16: Confirmada**, com a ressalva do arranjo. A contabilidade por processo sem `unsafe` nosso funciona: o
`tracking-allocator` 0.4 com a nossa tabela por grupo atribui certo quando A aloca e B libera e quando B
realoca o que recebeu de A, acerta os 11 cenários de bytes vivos, não deixa resíduo na saída da thread, e a
flag de limite ligada pelo próprio allocator é vista no checkpoint seguinte (40 ns com checkpoint a cada
alocação; 1,4 ms e 4 MiB de estouro no pior caso de fase com checkpoint a cada 64 pedidos de 64 KiB). O custo
contra o System, que é o allocator de hoje, depende do allocator interno:

- **sobre o System**: +15,0% a +16,7% no sort de 1M linhas, na fronteira do critério e acima dele pela medida
  conservadora (a pior das duas). Metade é o cabeçalho e o realloc que copia, metade o rastreamento;
- **sobre o mimalloc**: -17% contra o System nos dois modos. A contabilidade em si custa 15% a 18% sobre o
  mimalloc puro, mas o mimalloc é 28% a 30% mais rápido que a glibc nessa carga e paga a conta com sobra.

O critério pede "allocator pronto com grupos por thread" contra o System; o arranjo
`tracking_allocator::Allocator<MiMalloc>` é pronto e passa com folga. Se a régua for o custo da contabilidade
contra o mesmo allocator sem ela, o número é 15% a 18%, logo acima dos 15%.

Os demais candidatos não servem, e cada um por motivo medido: os de contador por thread (`jqf-resource`,
`alloc_count`, `allocation-counter`, contadores do jemalloc) erram a atribuição quando outra thread libera e
não deixam o kernel ler de fora; `alloc-track` acerta a atribuição mas aborta o host na thread 1025 e lê em
88 ms; os globais (`cap`, `stats_alloc`, `accounting-allocator`) não têm grupo, e os de atômico compartilhado
(`cap`, `stats_alloc`, e o `alloc_count` com os seus seis globais) ficam 8x a 16x mais lentos com 16 threads.

Também ficou demonstrado o pressuposto do design: recusar a alocação dentro do allocator aborta o host inteiro
(vizinhos inclusive), e um pedido gigante aborta o host mesmo sem limite nenhum. O limite tem de ser contagem
mais kill no checkpoint.

## Recomendação para o design

1. **Allocator global: `tracking_allocator::Allocator<mimalloc::MiMalloc>`** com o nosso tracker de tabela por
   grupo (`src/group_table.rs` vira código do kernel). Sem mimalloc (se C no allocator for vetado), o mesmo
   arranjo sobre o System custa ~15% a 17% nas cargas que alocam por linha e ~3% nas que emprestam do buffer.
2. **Limite = flag no tracker + kill no checkpoint.** O tracker liga a flag do slot quando o processo passa do
   teto; o checkpoint que já existe pra preempção lê essa flag junto. Nada de vigia por timer: ele só piora a
   latência (o tempo dele soma ao do checkpoint).
3. **Nunca limite duro no allocator**, e builtins que tiram tamanho da entrada do usuário (`head -c`, `dd bs=`,
   `seq`, leitura de cabeçalho de arquivo) reservam com `try_reserve`: um `Vec::with_capacity` gigante aborta o
   host mesmo sem teto nenhum.
4. **Estruturas do kernel**: alocar dentro de `AllocationRegistry::untracked` e cobrar num contador próprio do
   processo (~1% do caminho de um pipe). A cota da sandbox não pode ser um atômico compartilhado por todas as
   threads: usar lotes por thread (64 KiB) ou contador por CPU. Sem o `untracked`, a página de pipe fica na conta
   de quem escreveu até alguém ler, e o tracking-allocator já a debita certo quando o leitor libera, então a
   escolha é só não contar duas vezes.
5. **Riscos a cobrir**: a crate está parada desde 2022 (MPL-2.0, `static mut` interno); o `realloc` não é
   sobrescrito (vale um PR upstream, não um fork nosso, porque o fork seria `unsafe impl` nosso); o slot por
   máscara exige reciclar o slot na morte do processo; uma thread auxiliar de um builtin só entra no grupo se o
   token for movido pra ela (o token é um só e o guard é `!Send`).
