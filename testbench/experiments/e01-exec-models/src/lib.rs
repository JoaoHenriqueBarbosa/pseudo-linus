//! E01: o mesmo mini-kernel de pseudo-processos em três modelos de execução, e as medições das
//! hipóteses H01 a H10.
//!
//! - [`model_a`]: uma thread do SO por processo, N tokens de CPU virtual (park/unpark).
//! - [`model_b`]: corrotinas corosensei presas a cada worker, sem migração.
//! - [`model_c`]: processo = future, executor próprio.
//!
//! Pipe, sinais, `wait`, tabela de descritores e timer de fatia são comuns ([`kernel`], [`pipe`]).
//!
//! # H01: migração de corrotina stackful entre threads em Rust seguro
//!
//! Os exemplos abaixo são doctests `compile_fail`: `cargo test --release --doc` falha se algum deles
//! passar a compilar. O rustdoc estável não confere o código de erro anotado (só que a compilação
//! falha), então o binário refaz a prova com `cargo check` num crate de sondas (`probes/h01`), confere
//! os códigos exatos e grava no JSON; o teste `h01_probes_fail_with_expected_codes` faz o mesmo.
//!
//! Controle: a corrotina funciona normalmente na thread em que foi criada.
//!
//! ```
//! #![forbid(unsafe_code)]
//! use corosensei::{Coroutine, CoroutineResult};
//! let mut co: Coroutine<(), i32, i32> = Coroutine::new(|y, ()| {
//!     y.suspend(1);
//!     2
//! });
//! assert!(matches!(co.resume(()), CoroutineResult::Yield(1)));
//! assert!(matches!(co.resume(()), CoroutineResult::Return(2)));
//! ```
//!
//! Mover uma `corosensei::Coroutine` suspensa pra outra thread não compila (`Coroutine` é `!Send`):
//!
//! ```compile_fail,E0277
//! #![forbid(unsafe_code)]
//! use corosensei::Coroutine;
//! let mut co: Coroutine<(), (), ()> = Coroutine::new(|y, ()| y.suspend(()));
//! co.resume(());
//! std::thread::spawn(move || {
//!     co.resume(());
//! })
//! .join()
//! .unwrap();
//! ```
//!
//! Compartilhar atrás de `Arc<Mutex<_>>` também não (`Mutex<T>` só é `Sync` se `T: Send`):
//!
//! ```compile_fail,E0277
//! #![forbid(unsafe_code)]
//! use std::sync::{Arc, Mutex};
//! use corosensei::Coroutine;
//! let co: Coroutine<(), (), ()> = Coroutine::new(|y, ()| y.suspend(()));
//! let shared = Arc::new(Mutex::new(co));
//! let s2 = shared.clone();
//! std::thread::spawn(move || {
//!     s2.lock().unwrap().resume(());
//! })
//! .join()
//! .unwrap();
//! ```
//!
//! A saída "óbvia", um embrulho com `unsafe impl Send`, é barrada pelo `forbid(unsafe_code)`:
//!
//! ```compile_fail
//! #![forbid(unsafe_code)]
//! use corosensei::Coroutine;
//! struct Movable(Coroutine<(), (), ()>);
//! unsafe impl Send for Movable {}
//! let m = Movable(Coroutine::new(|y, ()| y.suspend(())));
//! std::thread::spawn(move || drop(m)).join().unwrap();
//! ```
//!
//! O `may` (M:N pronto) só cria corrotina por `unsafe fn`: chamar sem `unsafe` não compila...
//!
//! ```compile_fail,E0133
//! #![forbid(unsafe_code)]
//! let h = may::coroutine::spawn(|| 1);
//! assert_eq!(h.join().unwrap(), 1);
//! ```
//!
//! ...e com `unsafe` o `forbid(unsafe_code)` barra:
//!
//! ```compile_fail
//! #![forbid(unsafe_code)]
//! let h = unsafe { may::coroutine::spawn(|| 1) };
//! assert_eq!(h.join().unwrap(), 1);
//! ```
//!
//! O `generator` 0.8 compila a migração sem unsafe nosso porque tem `unsafe impl Send` próprio (issue
//! #58), e isso é unsound: o teste `tests/h01_generator.rs` mostra uma referência a thread-local da
//! thread 1 sendo usada depois que a corrotina migrou pra thread 2.

pub mod experiments;
pub mod kernel;
pub mod model_a;
pub mod model_b;
pub mod model_c;
pub mod pipe;
pub mod stats;
pub mod sys;
pub mod vm;
pub mod workloads;

use std::sync::Arc;

use serde::Serialize;

use kernel::{Core, ExitStatus, Fd, File, Pid};
use sys::ProcMain;

/// API do lado do host pros modelos síncronos (A e B).
pub trait SyncKernel {
    fn core(&self) -> &Arc<Core>;
    fn spawn(&self, files: Vec<(Fd, File)>, main: ProcMain) -> Pid;

    fn wait(&self, pid: Pid) -> ExitStatus {
        self.core().wait_host(pid).expect("wait")
    }

    fn kill(&self, pid: Pid, sig: i32) {
        let _ = self.core().kill(pid, sig);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ModelKind {
    A,
    B,
    C,
}

/// Um modelo com seus parâmetros (tamanho de pilha pra A e B; espera ativa antes do park no A).
#[derive(Clone, Copy, Debug)]
pub struct ModelSpec {
    pub kind: ModelKind,
    pub stack: usize,
    pub spin_us: u64,
}

pub const KIB: usize = 1024;

impl ModelSpec {
    pub const A: ModelSpec = ModelSpec { kind: ModelKind::A, stack: 256 * KIB, spin_us: 0 };
    pub const A64: ModelSpec = ModelSpec { kind: ModelKind::A, stack: 64 * KIB, spin_us: 0 };
    /// A com 20 µs de espera ativa antes do `park`.
    pub const ASPIN: ModelSpec = ModelSpec { kind: ModelKind::A, stack: 256 * KIB, spin_us: 20 };
    pub const B64: ModelSpec = ModelSpec { kind: ModelKind::B, stack: 64 * KIB, spin_us: 0 };
    pub const B256: ModelSpec = ModelSpec { kind: ModelKind::B, stack: 256 * KIB, spin_us: 0 };
    pub const C: ModelSpec = ModelSpec { kind: ModelKind::C, stack: 0, spin_us: 0 };

    /// Os modelos comparados na maioria das medições.
    pub const MAIN: [ModelSpec; 4] = [ModelSpec::A, ModelSpec::B64, ModelSpec::B256, ModelSpec::C];
    /// Os modelos comparados nas medições de desempenho de troca (inclui a variante com espera ativa).
    pub const PERF: [ModelSpec; 5] = [ModelSpec::A, ModelSpec::ASPIN, ModelSpec::B64, ModelSpec::B256, ModelSpec::C];

    pub fn label(&self) -> String {
        match self.kind {
            ModelKind::A if self.spin_us > 0 => "A-spin".to_string(),
            ModelKind::A if self.stack == 256 * KIB => "A".to_string(),
            ModelKind::A => format!("A{}", self.stack / KIB),
            ModelKind::B => format!("B{}", self.stack / KIB),
            ModelKind::C => "C".to_string(),
        }
    }

    pub fn parse(s: &str) -> Option<ModelSpec> {
        match s {
            "A" | "A256" => Some(ModelSpec::A),
            "A64" => Some(ModelSpec::A64),
            "A-spin" => Some(ModelSpec::ASPIN),
            "B64" => Some(ModelSpec::B64),
            "B256" => Some(ModelSpec::B256),
            "C" => Some(ModelSpec::C),
            _ => None,
        }
    }

    /// Cria um kernel síncrono (A ou B). `None` pro modelo C.
    pub fn sync_kernel(&self, ncpus: usize, timer: bool) -> Option<Box<dyn SyncKernel>> {
        match self.kind {
            ModelKind::A => Some(Box::new(model_a::KernelA::new(model_a::ConfigA {
                ncpus,
                stack_size: self.stack,
                timer,
                spin: std::time::Duration::from_micros(self.spin_us),
            }))),
            ModelKind::B => Some(Box::new(model_b::KernelB::new(model_b::ConfigB {
                workers: ncpus,
                stack_size: self.stack,
                timer,
                tls_swap: false,
            }))),
            ModelKind::C => None,
        }
    }

    pub fn c_kernel(&self, ncpus: usize, timer: bool) -> model_c::KernelC {
        model_c::KernelC::new(model_c::ConfigC { workers: ncpus, timer })
    }
}
