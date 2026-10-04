# Procedência do código: crates `sched` e `rbtree`

Os dois crates são portes declarados de código do Linux (os cabeçalhos dos módulos dizem isso). Ficam com a
licença do código de origem até a decisão do dono (relicenciar ou reescrever em sala limpa).

## `sched` (`GPL-2.0-only`)

| Arquivo | Classe | Origem no Linux 6.12.101 |
|---|---|---|
| `src/fair.rs` | porte | `kernel/sched/fair.c` (enqueue/dequeue, `place_entity`, `update_curr`, pick, `calc_group_shares`, `update_tg_load_avg`, `DELAY_DEQUEUE`), mesmos nomes e ordem de passos |
| `src/timeline.rs` | porte | `kernel/sched/fair.c` (`__enqueue_entity`, `avg_vruntime`, `vruntime_eligible`, `pick_eevdf`, `cfs_rq_min_slice`) |
| `src/bandwidth.rs` | porte | `kernel/sched/fair.c` (`CONFIG_CFS_BANDWIDTH`: `assign_cfs_rq_runtime`, throttle, `distribute_cfs_runtime`, timers de período e de folga, `tg_set_cfs_bandwidth`) |
| `src/balance.rs` | porte reduzido | `kernel/sched/fair.c` (`sched_balance_rq`, `calculate_imbalance`, `detach_tasks`, NOHZ) |
| `src/pelt.rs` | porte | `kernel/sched/pelt.c`, `kernel/sched/sched-pelt.h` (tabela `runnable_avg_yN_inv`) |
| `src/weight.rs` | porte | `kernel/sched/core.c` (tabelas `sched_prio_to_weight`/`sched_prio_to_wmult`), `fair.c` (`__calc_delta`), `include/linux/math64.h` |
| `src/features.rs` | porte | `kernel/sched/features.h`, sysctls do `fair.c` |
| `src/sched.rs` | porte | estruturas de `kernel/sched/sched.h` e operações do `core.c`/`syscalls.c` sobre a classe CFS |
| `src/rq.rs` | porte (fachada) | `core.c` |
| `src/clock.rs`, `src/invariants.rs`, `src/sim.rs`, `src/lib.rs` | original (simulador e verificação nossos) | |
| `src/group_tests.rs`, `src/rq/tests.rs`, `tests/*` | original (valores esperados calculados das fórmulas do `fair.c`) | |

`kernel/sched/fair.c`, `pelt.c` e `core.c` são `GPL-2.0` (SPDX do arquivo). Por isso o `license` do
`Cargo.toml` passou a `GPL-2.0-only`.

## `rbtree` (`GPL-2.0-or-later`)

| Arquivo | Classe | Origem |
|---|---|---|
| `src/lib.rs` | tradução | `lib/rbtree.c` e `include/linux/rbtree_augmented.h` (inserção, remoção, rebalanceamento e callbacks de augmentação caso a caso), com arena e índices no lugar de ponteiros |
| `src/tests.rs` | original | |

`lib/rbtree.c` e `rbtree_augmented.h` são `GPL-2.0-or-later` (SPDX do arquivo, a conferir na árvore
stable). Por isso o `license` do `crates/rbtree/Cargo.toml` passou a `GPL-2.0-or-later`.

## Compatibilidade

- `sched` é `GPL-2.0-only`: não pode ir no mesmo binário que código `GPL-3.0` (ex.: o porte do grep).
- `rbtree` é `GPL-2.0-or-later`: combina com GPL-3.0, mas o `sched` que depende dele não.
- O kernel usa o `sched` a partir do marco 2: o binário do host passa a conter código `GPL-2.0-only`.
