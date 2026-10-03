//! E07: contabilidade de memória por pseudo-processo (hipótese H16).
//!
//! No modelo de execução A (uma thread do SO por pseudo-processo), contar os bytes vivos de cada
//! processo exige um allocator global que saiba quem está alocando. Escrever um é
//! `unsafe impl GlobalAlloc`, proibido no nosso código; então a bancada mede allocators prontos.
//!
//! Como o allocator global é um por binário, cada candidato é um binário (`src/bin/cand-*.rs`) que
//! implementa [`accounting::Accounting`] e chama [`cli::run`]. O orquestrador (`src/main.rs`) roda cada
//! binário como subprocesso, recolhe uma linha JSON por comando e grava `results/e07-mem-accounting.json`.
//!
//! Regra de link: esta lib nunca referencia crate que declare `#[global_allocator]` por conta própria
//! (a `allocation-counter` declara o dela), senão todo binário herdaria esse allocator.
//!
//! Módulos:
//! - [`accounting`]: o contrato que cada candidato implementa.
//! - [`group_table`]: a tabela de contadores por grupo que alimentamos a partir do `tracking-allocator`.
//! - [`workloads`]: sort de 1M linhas, alocações pequenas em uma e em 16 threads.
//! - [`scenarios`]: atribuição entre threads, bytes vivos em cenários controlados, estouro de limite.
//! - [`kernel_acct`]: custo de contar explicitamente buffers de pipe e conteúdo de arquivo.
//! - [`demos`]: demonstrações que abortam o processo (limite duro, alocação gigante).
//! - [`cli`]: o `main` comum dos binários de candidato.

pub mod accounting;
pub mod cli;
pub mod demos;
pub mod group_table;
pub mod kernel_acct;
pub mod rng;
pub mod scenarios;
pub mod sys;
pub mod workloads;

pub use accounting::{Accounting, Capabilities, Pid};
