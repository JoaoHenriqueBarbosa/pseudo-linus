# E02: árvore rubro-negra aumentada, EEVDF, grupos e banda

Valida as peças do escalonador que o design v2 manda fazer à mão, sem crate: a árvore rubro-negra
aumentada em arena (`crates/rbtree`) e o escalonador EEVDF fiel ao `kernel/sched/fair.c` da 6.12.101
(`crates/sched`), agora com hierarquia de grupos (`CONFIG_FAIR_GROUP_SCHED`), controle de banda
(`CONFIG_CFS_BANDWIDTH`, o `cpu.max`) e várias CPUs com balanceamento. O experimento não tem crate
candidata de terceiros: ele mede a nossa implementação contra referências (o `BTreeMap` da std, uma
escolha por força bruta e o kernel do host, com e sem cgroups v2 reais).

O motivo dos grupos é a VPS onde o pseudo-linus vai rodar (Ubuntu 24.04, kernel 6.8, 2 vCPUs, 3 GiB)
atendendo vários usuários: com divisão plana por processo, quem roda `xargs -P 16` leva 16 vezes a CPU
de quem roda um processo só.

```sh
cd testbench/experiments/e02-sched-eevdf
cargo test --release            # proptest de 100 mil sequências, oráculo do pick, cenários e fumaça com cgroups reais
cargo run --release             # refaz todas as medições (cerca de 6 minutos) e grava results/e02-sched-eevdf.json
cargo run --release -- --quick  # versão curta, pra desenvolvimento
```

Os cenários de grupos criam unidades transitórias no systemd do usuário (`systemd-run --user`), todas
com o prefixo `e02r<pid>`. O binário para a slice raiz da sessão, remove os drop-ins de runtime
(`systemctl --user revert`) e confere que nenhuma unidade nem cgroup sobrou, inclusive em pânico (no
`Drop`). Precisa do controlador `cpu` delegado ao usuário (padrão no Debian 13 e no Ubuntu 24.04); sem
ele, H41 e H42 saem como inconclusivas e o resto roda.

## Hipóteses

| Id | Hipótese | Critério |
|---|---|---|
| H11 | Runqueue com trava global escala até uns 8 workers | Operações de escalonamento por segundo (pick + put) com 1, 2, 4, 8 e 16 workers, trava global contra runqueue por worker. Confirmada se a trava global mantém >= 70% da vazão ideal até 8 workers. |
| H12 | Árvore rubro-negra em arena com índices u32 é correta e competitiva com `BTreeMap` | proptest com 100 mil sequências sem falha (ordem, invariantes, augmentação); insert/remove/leftmost no máximo 2x mais lento que `BTreeMap`; `pick_eevdf` aumentado melhor que linear a partir de n=64. |
| H13 | O nosso EEVDF reproduz a divisão de CPU e a latência de wakeup do kernel 6.12 do host | Mesmos cenários no host (threads fixadas numa CPU, nices diferentes, sleeper periódico) e no simulador. Confirmada se as divisões batem em até 5 pontos percentuais e p50/p99 de latência de wakeup ficam na mesma ordem de grandeza. |
| H41 | EEVDF hierárquico divide CPU entre usuários e sandboxes pelo peso do grupo, independente de quantos processos cada um roda | Mesmos cenários no simulador e no kernel do host com cgroups v2 reais (`CPUWeight`): grupo com 1 laço contra grupo com 8 laços, pesos 100/100 e 100/300, hierarquia usuário > sandbox > processo, em 1 e 2 CPUs. Confirmada se as divisões batem em até 5 pontos percentuais. |
| H42 | Controle de banda (`cpu.max`) limita um usuário guloso a uma fração fixa de CPU, com o mesmo padrão de throttling do Linux | Grupo com quota de 20% e 50% (período de 100 ms), sozinho e competindo com outro grupo, no simulador e no kernel do host (`CPUQuota`). Confirmada se a fração de CPU bate em até 2 pontos percentuais e o padrão roda/estrangula por período é o mesmo. |

## O que foi construído

**`crates/rbtree`** é uma tradução do `lib/rbtree.c` e do `include/linux/rbtree_augmented.h`: os nós
moram num `Vec<Node>` e se ligam por índice `u32`, com `u32::MAX` como sentinela de nulo e uma lista de
nós livres reaproveitados. Inserção (`__rb_insert`), remoção (`__rb_erase_augmented`) e rebalanceamento
da remoção (`____rb_erase_color`) seguem o kernel caso a caso, inclusive a ordem dos callbacks de
augmentação (`propagate`, `copy`, `rotate`). A augmentação é genérica (trait `Augment`: o resumo de um
nó é função dele e dos resumos dos filhos), o mais à esquerda fica em cache como no `rb_root_cached`, e a
API de navegação (`root`, `left`, `right`, `summary`, `key`, `value`) é a que o `pick_eevdf` usa pra
descer a árvore. Chaves repetidas são aceitas e ficam na ordem de inserção. `check_invariants` confere
ordem, raiz preta, ausência de vermelho com filho vermelho, altura preta igual, ligações pai/filho,
resumo de cada nó, cache do mais à esquerda e lista de livres; um teste corrompe a árvore por dentro e
confere que cada violação é acusada.

**`crates/sched`** reproduz o fair.c da 6.12.101 (baixado da árvore stable) com relógio injetável:

- `weight`: `sched_prio_to_weight`/`sched_prio_to_wmult`, `scale_load`/`scale_load_down`,
  `__calc_delta` com inverso de 32 bits e os deslocamentos do kernel, `div_s64` com divisor `s32`, e o
  mapeamento do `cpu.weight` do cgroup v2 (`sched_weight_from_cgroup`: `CPUWeight=100` vira shares de
  1024, `300` vira 3072).
- `timeline`: a árvore ordenada pela deadline virtual com a comparação circular do `entity_before`,
  augmentação `min_vruntime`/`min_slice`, `avg_vruntime` relativo a `zero_vruntime` com piso,
  `vruntime_eligible` sem divisão, `cfs_rq_min_slice` e `pick_eevdf`.
- `pelt`: o PELT da 6.12.101 (`decay_load` pela tabela `runnable_avg_yN_inv`, unidades de 1024 ns,
  `get_pelt_divider`), só com o sinal de carga.
- `Sched` (módulos `sched`, `fair`, `bandwidth`, `balance`): as estruturas `rq`, `cfs_rq`,
  `sched_entity` e `task_group`, com a hierarquia do kernel: cada grupo tem por CPU uma fila filha
  (`my_q`) e uma entidade de grupo na fila do pai, com peso de `calc_group_shares` (shares do grupo vezes
  a fração da carga PELT do grupo que está naquela CPU, `update_tg_load_avg` com limite de 1 ms e de
  1/64). `enqueue_task_fair` e `dequeue_entities` sobem a hierarquia nos dois laços do kernel
  (`h_nr_queued`, `h_nr_runnable`, `h_nr_delayed`), o pick desce da raiz até uma tarefa
  (`pick_task_fair`), `pick_next_task_fair` troca só os níveis diferentes (`find_matching_se`), o tick
  cobra e testa cada nível (`task_tick_fair`), e `place_entity`, lag, RUN_TO_PARITY, PREEMPT_SHORT e
  DELAY_DEQUEUE valem em cada nível. Qualquer profundidade (usuário > sandbox > processo).
- `bandwidth`: `cfs_bandwidth` com pool reabastecido a cada período, fatias de 5 ms
  (`__assign_cfs_rq_runtime`), cobrança no `update_curr` (`__account_cfs_rq_runtime`), estrangulamento
  no `put_prev_entity`, no pick e ao enfileirar (`throttle_cfs_rq`: a entidade do grupo sai da fila do
  pai, as tarefas ficam na fila estrangulada e o relógio do PELT dela para), `distribute_cfs_runtime` na
  ordem de estrangulamento, timer de período com `hrtimer_forward`, timer de folga (slack), dívida
  carregada pro período seguinte, limites do `tg_set_cfs_bandwidth` (quota mínima de 1 ms, período entre
  1 ms e 1 s). Os timers são dirigidos de fora (`next_timer_ns`, `run_timers`).
- `balance`: o `sched_balance_rq` reduzido a um domínio (o caso de uma VPS de poucas vCPUs, ou de um par
  SMT): classificação das CPUs, escolha da origem, `calculate_imbalance`, `detach_tasks` com
  `task_h_load >> nr_balance_failed`, `task_hot`, balanceamento ativo, newidle, decaimento da carga
  bloqueada antes de balancear (`sched_balance_update_blocked_averages`) e o chute do NOHZ pras CPUs
  ociosas (carga bloqueada a cada 32 ms e puxada de trabalho). Domínios prontos: MC (o padrão pra várias
  CPUs, que é o de uma VPS) e SMT.
- `Topology::Shared`: a alternativa medida pra decisão de multi-CPU, uma runqueue única compartilhada
  pelas CPUs, com cada fila podendo ter várias entidades rodando ao mesmo tempo. Não existe no kernel.
- `rq`: `RunQueue`, o atalho de uma CPU só no grupo raiz, que os experimentos H11 a H13 usam.
- `sim`: simulador de eventos discretos com várias CPUs, tick em grade (só nas CPUs ocupadas, como
  NO_HZ), timers de sono com folga, timers de banda, grupos com peso e quota, afinidade e amostragem do
  uso por grupo, rodando o mesmo `Sched`.

Fatos do design v2 conferidos no código da 6.12.101:

- Confirmado: fatia base `0,70 ms * (1 + ilog2(min(ncpus, 8)))`, 2,8 ms com 16 CPUs; `TICK_NSEC` de
  4 ms com HZ=250; `zero_vruntime` iniciado em `-(1 << 20)` e movido pra V a cada `avg_vruntime`
  (chamado em `place_entity`, `update_entity_lag`, `update_deadline` e `reweight_entity`); lag inflado
  por `(W + w) / W` no PLACE_LAG; meia fatia pra tarefa nova; `protect_slice` como `vlag == deadline`.
- Correção: **a árvore compara só a deadline**, com empates à direita (ordem de chegada). Não existe
  desempate por id. A implementação segue o kernel.
- Detalhes que mudam números: `div_s64` trunca o divisor pra 32 bits; `avg_load` é `u64`; a tabela
  `sched_prio_to_wmult` não segue uma regra só de arredondamento (pesos 56, 45, 29 e 23 estão truncados);
  o `__update_inv_weight` daria 4194303 pra nice 0 e a tabela tem 4194304.
- Comportamentos que importam pra latência: na 6.12 o `entity_tick` só chama `update_curr` (não há mais
  `check_preempt_tick`), então tarefa CPU-bound só perde a CPU no tick que nota a deadline vencida; e
  todo wakeup chama `update_curr` dentro do `enqueue_entity`, então um wakeup também encerra a fatia
  vencida do corrente.
- Banda: o uso de um laço de CPU só é cobrado no tick, então o grupo passa da quota em até um tick e
  paga a dívida no período seguinte; o `init_cfs_bandwidth` **sorteia a fase do timer de período**
  (`get_random_u32_below(period)`), e essa fase decide em que tick a quota acaba (abaixo, no H42).
- Grupos: a carga de uma fila que esvaziou só decai no `update_blocked_averages`, que o balanceamento
  chama. Sem ele (a primeira versão deste crate não tinha), o `tg->load_avg` guarda a carga velha da
  outra CPU e o grupo perde peso onde está rodando: no cenário de hierarquia em 2 CPUs, o sandbox s1 caía
  de 12,5% pra 11,5%.

### Aproximações

Todas documentadas no código (`fair`, `balance`):

- PELT só com o sinal de carga (sem `runnable` e `util`); sem propagação da carga do filho pra entidade
  de grupo (`propagate_entity_load_avg`): a carga da entidade de grupo é a média do tempo em que ela
  esteve na fila, com o peso dela. A carga das filas, que é a que entra no `calc_group_shares`, segue o
  kernel. A remoção de carga na migração é feita na hora (o kernel adia pra próxima atualização da fila).
  Sem escala por capacidade nem frequência.
- Balanceamento de um domínio só (a VPS de 2 vCPUs tem um), sem capacidade reduzida por IRQ e RT; no
  newidle, sem o corte por `avg_idle` e sem o sorteio do NI_RANDOM; o `rd->overloaded` é "alguma CPU tem
  2 tarefas ou mais agora"; o trabalho do NOHZ é feito na hora pela CPU ocupada, sem o IPI.
- Wakeup sem `select_task_rq_fair`: a tarefa acorda na CPU em que dormiu, e só o balanceamento a move.
  Os cenários deste experimento são laços de CPU e não passam por aí; é o próximo passo pra cargas que
  dormem e acordam em várias CPUs.
- Fica de fora: `util_est`, SCHED_IDLE/SCHED_BATCH, classes RT e deadline (inclusive o `fair_server`),
  `cpu.max.burst` (fica em 0), uclamp, NUMA.

## Método

### H12: correção e desempenho da árvore e do pick

- **proptest** (`src/rbprop.rs`): 100 mil sequências de até 511 operações (inserir, remover o n-ésimo,
  remover o primeiro, trocar valor, navegar), com chaves em faixas de 3, 40 e 65535 valores. Depois de
  cada operação: `check_invariants`, ordem simétrica igual à de um `BTreeMap<(chave, seq), _>` (que
  modela repetidas na ordem de inserção), resumo de cada nó igual ao agregado calculado por força bruta a
  partir dos valores crus, `first`, `find`, `lower_bound`, `next`, `prev` e `last` iguais ao modelo.
  Semente fixa (ChaCha determinístico), sem arquivo de regressão.
- **oráculo do pick** (`src/pickcheck.rs`): 20 mil linhas do tempo com até 300 entidades de nice -20 a
  19, deadlines em três formas (logo depois do vruntime, independente dele, e no formato do EEVDF),
  remoções, corrente protegido ou não, base perto da volta do u64. O `pick_eevdf` é comparado com uma
  escolha por força bruta em `i128` que não usa a árvore nem as somas incrementais.
- **desempenho da árvore** (`src/opbench.rs`): insert, remove (por chave e por handle), leftmost e
  churn (tira o menor e reinsere mais à frente) contra `BTreeMap`, com n de 8 a 32768, sem e com a
  augmentação do EEVDF. Mínimo de 7 repetições intercaladas.
- **pick aumentado contra linear** (`src/pickbench.rs`): n = 8, 64, 512 e 4096 em três famílias de
  estado: simulados (n tarefas CPU-bound com nice de -5 a 5 no simulador), nices misturados no formato do
  EEVDF, e independentes (deadline sem relação com o vruntime, o pior caso, que força a descida). A linha
  de base é uma runqueue ingênua com vetor contíguo e as mesmas somas de V. As escolhas são conferidas
  uma contra a outra em todo estado.

### H13: diferencial contra o kernel do host

- **Host** (`src/host.rs`): a CPU mais ociosa por `/proc/stat` recebe todas as threads do cenário
  (`sched_setaffinity`); a thread que coordena fica em outra CPU, fora do par SMT. O nice de cada thread
  vem de `setpriority(PRIO_PROCESS, tid)` (só nices >= 0, sem root). O tempo de CPU vem do primeiro campo
  de `/proc/self/task/<tid>/schedstat`.
  - Divisão de CPU: laços de CPU com nices `[0, 0]`, `[0, 5]`, `[0, 3, 6, 9]`, `[0, 19]` e
    `[0, 0, 0, 0]`; 300 ms de aquecimento e janela de 2 s; 3 repetições.
  - Latência de wakeup: uma thread dorme 1 ms em laço (`nanosleep`) com 0, 1 ou 3 laços de CPU na mesma
    CPU; latência = tempo dormido menos 1 ms, com a folga de timer padrão (50 µs) e com folga de 1 ns
    (`PR_SET_TIMERSLACK`); 200 voltas de aquecimento e 2000 medidas; 3 repetições.
- **Simulador** (`src/simcmp.rs`): os mesmos cenários com as CPUs online do host (fatia 2,8 ms), HZ lido
  do `/boot/config` (250, tick de 4 ms), 5 sementes de fase de tick. A tarefa que dorme gasta por volta o
  tempo de CPU medido no host, e o timer dela dispara no vencimento duro ou no tick que cair antes.
- **Comparação**: maior diferença entre a divisão média do host e a do simulador; p50 e p99 de latência
  juntando as repetições, mesma ordem de grandeza = razão menor que 10 entre host e simulador nos cenários
  com laços de CPU e folga padrão.

### H11: trava global contra trava por runqueue

`src/lockscale.rs`: cada worker é uma CPU virtual com a sua `RunQueue` de 8 tarefas. Uma operação é um
tick que preempta (relógio da runqueue anda 3 ms, `tick`, `schedule`: put do anterior, `pick_eevdf`,
`set_next_entity`). Trava global = todas as runqueues atrás de um `Mutex`; por runqueue = um `Mutex` por
runqueue, sem disputa. Entre operações, cada worker gasta 0, 1, 10 ou 100 µs de trabalho fora da trava.
A vazão ideal com W workers é a da trava por runqueue com os mesmos W (mesmo hardware, sem disputa de
trava). Cada ponto é a maior vazão de 5 repetições de 250 ms, com as duas organizações intercaladas.

### H41: divisão entre grupos com cgroups v2 reais

- **Host** (`src/cgroups.rs`, `src/scenario.rs`, `src/groups.rs`): cada grupo folha do cenário é um
  scope transitório (`systemd-run --user --scope -p CPUWeight=W`) cujo processo é o próprio binário em
  modo `hog` (N threads em laço). Grupos intermediários (os usuários, na hierarquia) são slices
  aninhadas, com o peso posto por `systemctl --user set-property --runtime`. O `cpuset` não é delegado ao
  usuário, então as threads se fixam nas CPUs do cenário com `sched_setaffinity`. 1 s de aquecimento,
  janela de 3 s, 3 repetições; o tempo de cada grupo é o `usage_usec` do `cpu.stat` do scope, e a divisão
  é a fração do tempo de todas as folhas (intermediário = soma dos filhos). Também saem as migrações por
  segundo das threads (`se.nr_migrations` de `/proc/<pid>/task/<tid>/sched`) e, nos cenários de 1 contra
  8 em 2 CPUs, a cada 10 ms, quantas threads dos outros grupos estão na fila da CPU do laço do primeiro.
- **Cenários**: A (1 laço) contra B (8 laços) com pesos 100/100 e 100/300; e usuário > sandbox >
  processo: u1 (100) > s (100) com 1 laço; u2 (100) > s1 (100) com 4 laços e s2 (300) com 2 laços (ideal:
  u1 50%, u2 50%, s1 12,5%, s2 37,5%).
- **CPUs**: 1 CPU (a mais ociosa); 2 CPUs = a mesma e a irmã SMT dela, que formam um domínio de
  balanceamento de 2 CPUs como numa VPS de 2 vCPUs; a coordenação fica numa terceira CPU. Os cenários de
  1 contra 8 também rodam em dois núcleos diferentes, só no host: aí as duas CPUs caem no domínio MC de
  16 CPUs, com as irmãs SMT ociosas, e o balanceamento muda (abaixo).
- **Simulador**: a mesma árvore de grupos, 4 sementes (fase do tick, início das tarefas), modelo do
  kernel (runqueue por CPU com o domínio SMT nos cenários de 2 CPUs) e, em 2 CPUs, também a runqueue
  única.
- **Critério**: maior diferença entre a divisão média do host e a do simulador (runqueue por CPU) em
  todos os grupos, em 1 CPU e no par SMT.

### H42: controle de banda

- **Host**: grupo Q com `CPUQuota` de 20% ou 50% (`cpu.max` de 20000 ou 50000 por 100000 µs), 1 laço,
  sozinho ou disputando a CPU com O (1 laço, peso 100, sem limite); 1 CPU; 1 s de aquecimento, 3 s de
  janela, 3 repetições. A thread que coordena amostra o `usage_usec` de cada scope a cada 1 ms (folga de
  timer de 1 ns, prazos absolutos), e o `cpu.stat` dá `nr_periods`, `nr_throttled` e `throttled_usec`.
- **Padrão** (`analyze_pattern`, o mesmo algoritmo pros dois lados): trechos de pelo menos 12 ms sem
  progresso são estrangulamento; o fim deles marca o começo do período (fase pela média circular); cada
  período é medido em uso, se estrangulou e quanto tempo rodou antes. No simulador o uso amostrado é o já
  cobrado pelo `update_curr`, que anda nos mesmos pontos que o `usage_usec` (no tick, num laço de CPU).
- **Fase**: o kernel sorteia a fase do timer de período, e a distância dele até a grade de ticks decide
  se a quota de 20 ms acaba no 5º ou no 6º tick (17 ou 21 ms de execução). Sozinho, o laço roda desde o
  timer, então o primeiro degrau de uso depois de cada estrangulamento mede essa distância; cada rodada
  do host vira uma rodada do simulador com a mesma distância (`SimOpts::tick_lead_ns`). Disputando, o
  laço só volta num tick do outro grupo (o primeiro degrau é sempre 4 ms) e a fase não é observável: o
  simulador sorteia a fase, como o kernel.
- **Critério**: fração de CPU a até 2 pontos; mesmo padrão = fração de períodos estrangulados a até
  0,15 e tempo médio de execução até o estrangulamento a até 2 ms por rodada com a fase casada (5 ms na
  média, com a fase sorteada). A comparação é pela média e não pela mediana: com dívida, a execução
  alterna entre dois ticks vizinhos (48 e 52 ms com quota de 50 ms), e a mediana de duas modas quase
  iguais pula de uma pra outra por um período a mais ou a menos (numa rodada de desenvolvimento, isso
  deu 52,2 contra 49,0 ms com as médias iguais).

### Multi-CPU: runqueue por vCPU ou runqueue única

Nos três cenários de 2 CPUs, o simulador roda as duas organizações e cada uma é comparada com o kernel e
com a divisão ideal; o custo de trava vem do H11 com 2 workers. Regra (`groups::decide`): runqueue por
vCPU se ela fica a até 5 pp do kernel e não mais que 1 pp pior que a runqueue única; senão, a única, se
ela ficar a até 5 pp.

## Candidatos

| Candidato | Papel | Encaixe |
|---|---|---|
| `rbtree` (nosso, `crates/rbtree`) | árvore da runqueue | serve |
| `std::collections::BTreeMap` | linha de base de desempenho | referência: não tem augmentação nem handle estável, então não dá o pick em O(log n) |
| `sched` (nosso, `crates/sched`) | escalonador | serve |

## Resultado

Rodada oficial de 2026-10-02 (`results/e02-sched-eevdf.json`), 355 s, com outros agentes usando a
máquina (load average de 2,9 no começo e 5,3 no fim, em 16 CPUs). Medidas de correção e de divisão de
CPU não dependem da carga; os tempos de H11 e H12 são da máquina como estava.

### H12

- proptest: 100 mil sequências, 15,4 milhões de operações, árvores de até 368 nós, **nenhuma falha**.
  Oráculo do pick: 20 mil casos, **nenhuma divergência**; nos benchmarks, as duas escolhas bateram em
  todos os estados.
- Razão de tempo `rbtree / BTreeMap` (abaixo de 1, a árvore é mais rápida):

| n | insert | remove por chave | remove por handle | leftmost | churn | churn com augmentação |
|---|---|---|---|---|---|---|
| 8 | 0,74 | 0,91 | 0,62 | 1,00 | 0,37 | 1,07 |
| 64 | 0,73 | 0,96 | 0,41 | 0,75 | 0,81 | 2,79 |
| 512 | 0,93 | 1,10 | 0,31 | 0,60 | 1,04 | 3,56 |
| 4096 | 0,97 | 1,14 | 0,43 | 0,50 | 1,36 | 4,10 |
| 32768 | 1,23 | 1,50 | 0,52 | 0,43 | 1,79 | 4,37 |

  A árvore binária perde terreno com n grande (mais níveis e mais faltas de cache que a B-tree), mas
  fica dentro de 2x em tudo que o critério pede. Manter o `min_vruntime` custa caro no churn: tirar o mais
  à esquerda muda o mínimo de todo o caminho até a raiz, e o kernel paga o mesmo preço.

- `pick_eevdf` aumentado contra varredura linear (ns por pick):

| n | simulados | nices misturados | independentes (pior caso) |
|---|---|---|---|
| 8 | 11,4 contra 5,6 | 4,4 contra 5,0 | 6,2 contra 5,2 |
| 64 | 7,8 contra 28,1 | 4,4 contra 27,7 | 9,2 contra 27,9 |
| 512 | 7,3 contra 228 | 4,5 contra 219 | 11,1 contra 214 |
| 4096 | 7,4 contra 1774 | 4,4 contra 1845 | 16,6 contra 2492 |

  Nos estados simulados e nos de nices misturados o mais à esquerda já é elegível em 96% a 100% dos casos
  a partir de n=64, e o pick sai pelo atalho O(1). No pior caso (mais à esquerda elegível em só 46% a
  58%), a descida cresce como log n e o ganho é de 3,0x em n=64 e 150x em n=4096. Com n=8 a varredura de
  8 entradas contíguas ganha da descida (o critério começa em n=64).

### H13

Divisão de CPU (porcentagem de cada thread; host é a média de 3 repetições, desvio padrão de no máximo
0,14 pp):

| nices | host | simulador | ideal (peso / soma) | maior diferença |
|---|---|---|---|---|
| 0, 0 | 50,13 / 49,87 | 50,00 / 50,00 | 50 / 50 | 0,13 pp |
| 0, 5 | 75,40 / 24,60 | 75,40 / 24,60 | 75,35 / 24,65 | 0,00 pp |
| 0, 3, 6, 9 | 52,33 / 26,78 / 13,88 / 7,00 | 52,32 / 26,87 / 13,81 / 7,00 | 52,27 / 26,85 / 13,88 / 6,99 | 0,08 pp |
| 0, 19 | 98,60 / 1,40 | 98,60 / 1,40 | 98,56 / 1,44 | 0,00 pp |
| 0, 0, 0, 0 | 25,00 / 24,97 / 25,05 / 24,98 | 25 cada | 25 cada | 0,05 pp |

Latência de wakeup de quem dorme 1 ms (µs, 6000 amostras no host, cerca de 10 mil no simulador):

| laços de CPU | folga do timer | host p50 / p99 / máx. | simulador p50 / p99 / máx. |
|---|---|---|---|
| 0 | 50 µs | 55,2 / 67,2 / 4222 | 50,0 / 50,0 / 50 |
| 1 | 50 µs | 52,8 / 57,7 / 75 | 50,0 / 50,0 / 50 |
| 3 | 50 µs | 52,8 / 58,4 / 7196 | 50,0 / 50,0 / 8891 |
| 1 | 1 ns | 2,8 / 5,1 / 78 | 0,0 / 0,0 / 0 |
| 3 | 1 ns | 2,9 / 8,9 / 8360 | 0,0 / 0,0 / 4992 |

Nos dois lados, quase todo despertar preempta o laço na hora: a tarefa que dorme guarda lag positivo
(ela consome pouco), é elegível e tem a deadline mais cedo. Por isso p50 e p99 ficam na folga do timer.
O máximo com 3 laços, de 5 a 9 ms nos dois lados, vem da proteção de fatia (RUN_TO_PARITY): se o laço
que está rodando ainda é elegível, ele não é preemptado até gastar a fatia, mesmo que quem acorda tenha
lag positivo e deadline mais cedo, e a espera vai até o tick que nota a fatia vencida. Numa rodada de
13 s do simulador instrumentado (durante o desenvolvimento), todas as esperas acima de 100 µs tinham o
laço corrente protegido e elegível, e nenhum outro laço com deadline antes da do sleeper; com 3 laços
isso acontece também com lag positivo, porque o laço escolhido costuma ser o mais atrasado dos três e
continua elegível.

### H11

Eficiência da trava global (vazão sobre a da trava por runqueue com os mesmos workers):

| trabalho entre operações | 2 workers | 4 workers | 8 workers | 16 workers |
|---|---|---|---|---|
| 0 (pick + put de 168 ns colados) | 0,43 | 0,18 | 0,08 | 0,06 |
| 1 µs | 0,97 | 0,95 | 0,34 | 0,15 |
| 10 µs | 1,00 | 0,99 | 0,99 | 1,00 |
| 100 µs | 1,00 | 1,00 | 1,00 | 0,99 |

Com operações coladas, a trava global perde já com 2 workers e chega a 8% com 8; com 1 µs entre
operações aguenta 4 workers e cai pra 34% com 8; de 10 µs pra cima fica acima de 99% até 16 workers.

### H41

Divisão entre grupos (%; host é a média de 3 repetições, desvio padrão de no máximo 0,09 pp em 1 CPU e
1,4 pp em 2 CPUs):

| cenário | CPUs | host | simulador (runqueue por CPU) | runqueue única | maior diferença |
|---|---|---|---|---|---|
| A 1 laço contra B 8 laços, 100/100 | 1 | 50,0 / 50,0 | 50,0 / 50,0 |  | 0,00 pp |
| A 1 laço contra B 8 laços, 100/300 | 1 | 25,0 / 75,0 | 25,0 / 75,0 |  | 0,04 pp |
| u1 > s; u2 > s1, s2 | 1 | 50,0; 50,0 > 12,5 / 37,5 | 50,0; 50,0 > 12,5 / 37,5 |  | 0,03 pp |
| A 1 laço contra B 8 laços, 100/100 | par SMT | 45,7 / 54,3 | 50,0 / 50,0 | 50,0 / 50,0 | 4,33 pp |
| A 1 laço contra B 8 laços, 100/300 | par SMT | 25,6 / 74,4 | 25,6 / 74,4 | 25,0 / 75,0 | 0,07 pp |
| u1 > s; u2 > s1, s2 | par SMT | 47,3; 52,7 > 13,4 / 39,3 | 50,0; 50,0 > 12,5 / 37,5 | igual ao por CPU | 2,67 pp |
| A 1 laço contra B 8 laços, 100/100 | 2 núcleos | 35,3 / 64,7 | | | só host |
| A 1 laço contra B 8 laços, 100/300 | 2 núcleos | 23,5 / 76,5 | | | só host |

Em 1 CPU os dois lados dão a divisão do peso a menos de 0,15 pp: o grupo com 8 laços leva a mesma CPU
que o grupo com 1, e a hierarquia divide em cada nível. Com pesos 100/300 em 2 CPUs, A divide a CPU no
host com 2 laços de B em 43% do tempo e com 3 em 57%; o simulador também alterna (13,9 migrações por
segundo, contra 14,1 no host), e os dois chegam a 25,6%.

Onde o host se afasta é no ponto de equilíbrio com pesos iguais em 2 CPUs. A divisão do peso exige A
sozinho numa CPU e os 8 laços de B na outra; aí as cargas das duas raízes empatam (1024 contra 1024 no
simulador, que fica parado nesse ponto, sem nenhuma migração na janela). No host, os laços de B migram
17,7 vezes por segundo, e a amostragem mostra 0 laços de B na CPU de A em 36% do tempo, 1 em 62% e 2 ou
mais em 2%; com 1 laço de B na fila, A fica com 1024 / (1024 + 128) da CPU dele, e a média esperada,
46,3%, bate com os 45,7% medidos. A hierarquia em 2 CPUs mostra o mesmo efeito (os 4 laços de s1 migram
16,9 vezes por segundo, e u1, sozinho numa CPU, cai pra 47,3%). Numa medição de diagnóstico, com
amostras a cada 15 ms, o host alterna em ciclos regulares de uns 45 ms em cada estado com 8 laços em B,
fica parado em 0 com 4 laços e alterna só de vez em quando com 2. O mecanismo exato no kernel não foi
isolado (sem root não há tracing, debugfs nem schedstats); como o limite pra puxar uma tarefa é
`task_h_load >> nr_balance_failed` contra o desequilíbrio, e o `task_h_load` de cada laço de B é 1/8 da
carga do grupo, um empate quebrado por diferenças pequenas que o simulador não tem (capacidade de cada
CPU descontada do tempo de IRQ, PELT dos cinco níveis de cgroup acima do scope) basta pra mover um laço
com 8 em B e não com 4. A diferença ficou entre 2,8 e 4,9 pp nas seis rodadas completas do dia.

Em dois núcleos diferentes, a divisão do host (35,3%) é a de igualar o número de tarefas entre os
núcleos: A divide a CPU com 3 laços de B em 42% do tempo e com 4 em 49%, ou seja, 4 ou 5 tarefas de cada
lado, o que dá 36,4% e 33,3% pra A. Isso é consistente com o domínio MC quando os grupos têm CPU
sobrando (as irmãs SMT ociosas): o `calculate_imbalance` de um grupo com folga iguala o número de
tarefas, e a irmã ociosa, que não pode receber os laços, vira destino redirecionado pra CPU permitida
do mesmo núcleo (`LBF_DST_PINNED`). Uma VPS de 2 vCPUs não tem irmãs ociosas (é um domínio de 2 CPUs),
por isso o diferencial usa o par SMT, e o simulador usa o domínio MC de 2 CPUs como padrão.

### H42

| cenário | fração de CPU de Q (host / simulador) | períodos estrangulados no `cpu.stat`, por rodada | tempo estrangulado em 3 s (host / simulador) | execução média até estrangular, por rodada (host / simulador) |
|---|---|---|---|---|
| 20%, sozinho | 19,98% / 20,03% | 30 de 30 nos dois | 2388 a 2395 ms / 2398 a 2399 ms | 19,37, 17,28, 19,21 / 19,34, 17,14, 19,21 ms (fase casada) |
| 50%, sozinho | 50,00% / 50,00% | 30 de 30 nos dois | 1490 a 1501 ms / 1497 a 1502 ms | 49,45, 47,44, 49,01 / 49,41, 47,21, 48,86 ms (fase casada) |
| 20%, disputando com O | 20,00% / 20,00% (O: 79,98% / 80,00%) | 30 de 30 nos dois | 1799 a 1802 ms / 1796 a 1802 ms | 33,1 / 33,0 ms (fase sorteada) |
| 50%, disputando com O | 49,55% / 49,61% (O: 50,43% / 50,39%) | 11 a 14 / 10 a 14 | 8 a 42 ms / 12 a 37 ms | sem estrangulamento longo (1 período em 87 no host, nenhum no simulador) |

Sozinho, Q roda do começo do período até a quota acabar num tick e a CPU fica parada até o período
seguinte; o primeiro degrau de uso (1,62, 3,79 e 1,76 ms nas três rodadas de 20%) é a distância do timer
até o tick, e com a mesma distância o simulador estrangula nos mesmos ticks (5º ou 6º, com a dívida
alternando). Disputando, Q e O alternam a cada tick (a fatia de 2,8 ms é menor que o tick de 4 ms), então
os 20 ms de quota levam uns 33 ms de relógio e O fica com o resto. Com 50% disputando, o peso já dá 50% a
Q, a quota é a mesma, e Q é estrangulado só no fim de alguns períodos, por poucos milissegundos, nos dois
lados.

### Decisão de multi-CPU

| organização | maior diferença contra o kernel (par SMT) | maior distância até a divisão ideal |
|---|---|---|
| runqueue por vCPU com balanceamento (simulador) | 4,33 pp | 0,56 pp |
| runqueue única (simulador) | 4,33 pp | 0,04 pp |
| kernel do host, par SMT | | 4,33 pp |
| kernel do host, 2 núcleos | | 14,74 pp |

A fidelidade não separa as duas (a maior diferença é o equilíbrio de pesos iguais, onde as duas dão a
divisão ideal e o kernel não); com pesos 100/300 só a runqueue por vCPU reproduz o detalhe do kernel
(25,6% contra 25,0% da única). A trava global da runqueue única custa caro mesmo com 2 workers: 43% da
vazão ideal com operações coladas e 97% com 1 µs entre elas (H11). **Escolha: runqueue por vCPU com
balanceamento**, que é o modelo do Linux (o que o sandbox promete imitar, com o mesmo `calc_group_shares`,
PELT e balanceamento), aceita afinidade e não divide trava entre os workers. Pra VPS, o domínio é o MC de
2 CPUs (`BalanceConfig::mc_domain(2)`, o padrão do `SchedConfig::new` pra várias CPUs). A runqueue única
fica mais perto do peso ideal (0,04 pp), e seria a escolha se o objetivo fosse justiça ideal em vez de
fidelidade ao kernel.

## Veredito

- **H12: Confirmada.** Nenhuma falha em 100 mil sequências nem em 20 mil picks contra força bruta;
  insert, remove e leftmost dentro de 2x do `BTreeMap` em todos os tamanhos (pior: 1,50x no remove por
  chave com n=32768; por handle, que é o que o escalonador usa, a árvore é mais rápida); pick aumentado
  3,0x melhor que o linear já em n=64 no pior caso.
- **H13: Confirmada.** A divisão de CPU do simulador fica a no máximo 0,13 pp do kernel do host (critério:
  5 pp), e as latências de wakeup com laços de CPU têm p50 e p99 na mesma ordem de grandeza (razões entre
  1,06 e 1,17).
- **H11: Parcial.** Com operações de escalonamento coladas a trava global não escala (43% da vazão
  ideal com 2 workers, 8% com 8). Ela só sustenta 70% até 8 workers quando cada worker faz uns 10 µs de
  trabalho entre operações. Pipelines que bloqueiam e acordam a cada poucos microssegundos ficam no
  regime ruim, então o design deve nascer com uma trava por runqueue.
- **H41: Confirmada.** Maior diferença host contra simulador de 0,04 pp em 1 CPU e 4,33 pp em 2 CPUs
  (critério: 5 pp). O grupo de 8 laços leva o mesmo que o grupo de 1 (50/50 em 1 CPU; 46/54 no kernel e
  50/50 no simulador em 2 CPUs), e a hierarquia usuário > sandbox > processo divide em cada nível. A folga
  em 2 CPUs é pequena e vem do kernel, que oscila em torno do equilíbrio de pesos iguais (acima).
- **H42: Confirmada.** Fração de CPU do grupo com `cpu.max` a no máximo 0,07 pp do kernel (critério:
  2 pp) nos quatro cenários; os mesmos períodos estrangulados no `cpu.stat`, e com a fase casada a
  execução até estrangular bate a menos de 0,3 ms por rodada.
- **Multi-CPU:** runqueue por vCPU com balanceamento.

## Divergências entre o simulador e o kernel

1. **Custo fixo do wakeup.** Com folga de 1 ns, o host tem p50 de 3 µs e o simulador 0: o simulador
   cobra zero pela interrupção do hrtimer, pelo `try_to_wake_up`, pela troca de contexto e pela volta
   do `nanosleep`. Com folga de 50 µs, o mesmo custo aparece como os 3 a 5 µs acima dos 50 µs.
2. **Cauda com a CPU ociosa.** Sem laços de CPU, o host tem p99 de 67 µs e máximo de 4,2 ms; o simulador
   fica nos 50 µs. A saída de estados de economia de energia custa, e o balanceador traz pra CPU ociosa
   tarefas de outros processos; o simulador só tem as tarefas do cenário.
3. **Máximos de latência.** Além da espera pela fatia protegida, que os dois reproduzem, o host tem as
   nossas threads num cgroup disputando com outros grupos na runqueue raiz, kworkers e softirqs.
4. **Equilíbrio de pesos iguais em 2 CPUs.** O kernel oscila um laço do grupo grande pra CPU do grupo
   pequeno e de volta (até 4,9 pp de diferença); o simulador fica no empate exato. Ver H41.
5. **Fase do timer de banda.** O degrau medido no host inclui a latência de acordar a CPU (dezenas de
   µs), que o simulador não tem; com a fase casada, a execução até estrangular ainda bate a menos de
   0,3 ms.
6. **Divisão de CPU numa CPU** não diverge além do arredondamento: o EEVDF converge pro peso em segundos e
   os dois lados ficam a menos de 0,2 pp do ideal, com ou sem grupos.
