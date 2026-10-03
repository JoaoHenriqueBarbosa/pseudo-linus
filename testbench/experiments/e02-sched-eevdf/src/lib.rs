//! E02: árvore rubro-negra aumentada e EEVDF, com grupos e controle de banda.
//!
//! - [`rbprop`]: teste de propriedade da `rbtree` contra `BTreeMap` e força bruta (H12).
//! - [`pickcheck`]: `pick_eevdf` contra uma escolha por força bruta com aritmética exata (H12).
//! - [`opbench`]: insert/remove/leftmost da `rbtree` contra `BTreeMap` (H12).
//! - [`pickbench`]: `pick_eevdf` aumentado contra varredura linear (H12).
//! - [`host`]: medições no kernel do host: divisão de CPU por nice e latência de wakeup (H13).
//! - [`simcmp`]: os mesmos cenários no simulador do crate `sched` e a comparação (H13).
//! - [`lockscale`]: vazão de pick+put com trava global contra uma runqueue por worker (H11).
//! - [`cgroups`]: cgroups v2 reais pelo systemd do usuário (H41, H42).
//! - [`scenario`]: cenários de grupos no host e no simulador, e a análise do padrão de estrangulamento.
//! - [`groups`]: H41, H42 e a decisão de multi-CPU.

pub mod cgroups;
pub mod cpusel;
pub mod groups;
pub mod host;
pub mod lockscale;
pub mod opbench;
pub mod pickbench;
pub mod pickcheck;
pub mod rbprop;
pub mod rng;
pub mod scenario;
pub mod simcmp;
pub mod stats;
