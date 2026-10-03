//! Tabela única de programas do pseudo-linus: junta o `programs()` de cada crate de userland.
//!
//! Cada crate entra por uma feature (todas ligadas por padrão), pra que um crate quebrado de passagem
//! possa ser deixado de fora com `--no-default-features --features ...` sem travar o resto.
//! Em nome repetido, vale o primeiro da ordem abaixo (shell, depois coreutils, depois o resto).

use sysabi::Program;

pub fn all_programs() -> Vec<Program> {
    let mut all: Vec<Program> = Vec::new();
    #[cfg(feature = "shell")]
    all.extend(shell::programs());
    #[cfg(feature = "coreutils")]
    all.extend(ul_coreutils::programs());
    #[cfg(feature = "textproc")]
    all.extend(ul_textproc::programs());
    #[cfg(feature = "awk")]
    all.extend(ul_awk::programs());
    #[cfg(feature = "jq")]
    all.extend(ul_jq::programs());
    #[cfg(feature = "diff")]
    all.extend(ul_diff::programs());
    #[cfg(feature = "archive")]
    all.extend(ul_archive::programs());
    #[cfg(feature = "misc")]
    all.extend(ul_misc::programs());
    #[cfg(feature = "procps")]
    all.extend(ul_procps::programs());
    #[cfg(feature = "sqlite")]
    all.extend(ul_sqlite::programs());
    #[cfg(feature = "git")]
    all.extend(ul_git::programs());
    dedup(all)
}

/// Remove nomes repetidos no mesmo diretório, mantendo o primeiro.
fn dedup(programs: Vec<Program>) -> Vec<Program> {
    let mut seen = std::collections::BTreeSet::new();
    programs.into_iter().filter(|p| seen.insert(p.path())).collect()
}

/// Programas que mais de um crate exporta (útil pra achar conflito de dono).
pub fn duplicates(programs: &[Program]) -> Vec<String> {
    let mut count: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for p in programs {
        *count.entry(p.path()).or_default() += 1;
    }
    count.into_iter().filter(|(_, n)| *n > 1).map(|(k, _)| k).collect()
}
