//! Infraestrutura comum da bancada.
//!
//! - [`case`]: formato dos casos do corpus (TOML) e carregamento.
//! - [`memtree`]: árvore de arquivos em memória, usada como fixture de entrada e como retrato da saída.
//! - [`outcome`]: o que um caso produz (stdout, stderr, exit, arquivos) e a trait [`Candidate`].
//! - [`compare`]: comparação byte a byte contra o golden e placar de conformidade.
//! - [`real`]: execução de um caso num sistema real (usado pelo `oracle-agent` dentro do container).
//! - [`oracle`]: cliente que roda casos no container do oráculo.
//! - [`result`]: formato de `results/<experimento>.json`.
//! - [`paths`]: caminhos do workspace (corpus, golden, results).

pub mod bytes;
pub mod case;
pub mod compare;
pub mod memtree;
pub mod oracle;
pub mod outcome;
pub mod paths;
pub mod real;
pub mod result;

pub use bytes::Bytes;
pub use case::{Case, CaseFile, FileSpec};
pub use compare::{CaseComparison, Conformance, compare_outcome, score};
pub use memtree::{Entry, MemTree};
pub use oracle::Oracle;
pub use outcome::{Candidate, Invocation, Outcome};
pub use result::{CandidateResult, ExperimentResult, Fit, HostInfo, HypothesisVerdict, Verdict};

/// mtime fixo de todo arquivo de fixture: 2026-01-15T12:00:00Z.
pub const FIXTURE_MTIME: u64 = 1_768_478_400;

/// Diretório de trabalho de todo caso, no oráculo e no candidato (recriado do zero a cada caso).
pub const CASE_DIR: &str = "/work/case";

/// Ambiente fixo usado no oráculo e que os candidatos devem simular.
pub const BASE_ENV: &[(&str, &str)] = &[
    ("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"),
    ("HOME", "/root"),
    ("USER", "root"),
    ("LOGNAME", "root"),
    ("SHELL", "/bin/bash"),
    ("LC_ALL", "C.UTF-8"),
    ("TZ", "UTC"),
];
