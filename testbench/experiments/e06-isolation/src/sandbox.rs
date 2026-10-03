//! Perfis de isolamento em runtime de uma thread de pseudo-processo (modelo A do design).
//!
//! Tudo aqui age só na thread que chama (e nas threads que ela criar depois). Só o subprocesso do
//! experimento ([`crate::child`]) chama as funções que aplicam restrição.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use landlock::{
    ABI, Access, AccessFs, CompatLevel, Compatible, LandlockStatus, RestrictSelfAttr, RestrictionStatus, Ruleset,
    RulesetAttr, RulesetCreated, RulesetCreatedAttr, RulesetError, RulesetStatus, path_beneath_rules,
};
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter, SeccompRule,
    TargetArch,
};
use serde::{Deserialize, Serialize};

/// ABI do Landlock que o pseudo-linus pede: a do host de referência (Linux 6.12). O crate fixa a ABI
/// em tempo de compilação de propósito; num kernel mais velho ele rebaixa em best-effort e o
/// `RestrictionStatus` diz o que ficou de fora.
pub const TARGET_ABI: ABI = ABI::V6;

/// Bit das syscalls x32 no x86_64. O filtro do `seccompiler` só compara o número exato, então as
/// variantes x32 das syscalls negadas entram explicitamente (no host de referência o x32 está
/// desligado no boot, mas o filtro não deve depender disso).
#[cfg(target_arch = "x86_64")]
const X32_SYSCALL_BIT: i64 = 0x4000_0000;

/// Ruleset que nega todo acesso a FS, exceto embaixo dos diretórios "montados".
pub fn landlock_ruleset<P: AsRef<Path>>(mounted_dirs: &[P]) -> Result<RulesetCreated, RulesetError> {
    Ruleset::default()
        .handle_access(AccessFs::from_all(TARGET_ABI))?
        .create()?
        .add_rules(path_beneath_rules(mounted_dirs, AccessFs::from_all(TARGET_ABI)))
}

/// O que o `restrict_self` devolveu, em forma serializável.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandlockApplied {
    /// `fully_enforced`, `partially_enforced` ou `not_enforced`.
    pub ruleset: String,
    pub no_new_privs: bool,
    /// `available`, `not_enabled` ou `not_implemented`.
    pub landlock: String,
    /// ABI que o crate usou (interseção do kernel com o que o crate conhece).
    pub effective_abi: Option<i32>,
    /// ABI do kernel quando ela é mais nova que a última conhecida pelo crate.
    pub kernel_abi: Option<i32>,
    /// Se a restrição valeu pra todas as threads (TSYNC, só na ABI v8).
    pub all_threads: bool,
}

impl From<&RestrictionStatus> for LandlockApplied {
    fn from(s: &RestrictionStatus) -> LandlockApplied {
        let ruleset = match s.ruleset {
            RulesetStatus::FullyEnforced => "fully_enforced",
            RulesetStatus::PartiallyEnforced => "partially_enforced",
            RulesetStatus::NotEnforced => "not_enforced",
        };
        let (landlock, effective_abi, kernel_abi) = match s.landlock {
            LandlockStatus::Available { effective_abi, kernel_abi } => ("available", Some(abi_number(effective_abi)), kernel_abi),
            LandlockStatus::NotEnabled => ("not_enabled", None, None),
            LandlockStatus::NotImplemented => ("not_implemented", None, None),
        };
        LandlockApplied {
            ruleset: ruleset.to_string(),
            no_new_privs: s.no_new_privs,
            landlock: landlock.to_string(),
            effective_abi,
            kernel_abi,
            all_threads: s.all_threads,
        }
    }
}

/// Número da ABI. `ABI` é `non_exhaustive`, então não dá pra fazer `as i32` fora do crate.
pub fn abi_number(abi: ABI) -> i32 {
    match abi {
        ABI::Unsupported => 0,
        ABI::V1 => 1,
        ABI::V2 => 2,
        ABI::V3 => 3,
        ABI::V4 => 4,
        ABI::V5 => 5,
        ABI::V6 => 6,
        ABI::V7 => 7,
        ABI::V8 => 8,
        ABI::V9 => 9,
        _ => -1,
    }
}

/// Aplica o ruleset na thread atual (e nas que ela criar depois).
pub fn restrict_current_thread(ruleset: RulesetCreated) -> Result<LandlockApplied, RulesetError> {
    let status = ruleset.restrict_self()?;
    Ok(LandlockApplied::from(&status))
}

/// Pede TSYNC (`all_threads`) como requisito duro, só no builder: nada é aplicado. Na ABI < 8 o
/// crate recusa aqui mesmo; se aceitar, o ruleset é descartado sem `restrict_self`.
pub fn tsync_hard_requirement() -> std::result::Result<(), String> {
    let created = Ruleset::default()
        .handle_access(AccessFs::from_all(TARGET_ABI))
        .and_then(Ruleset::create)
        .map_err(|e| e.to_string())?;
    created.set_compatibility(CompatLevel::HardRequirement).all_threads(true).map(drop).map_err(|e| e.to_string())
}

/// Os dois filtros BPF do pseudo-processo, compilados uma vez e aplicados por thread.
#[derive(Clone, Debug)]
pub struct SeccompProfile {
    /// Lista de negação com EPERM.
    pub deny: BpfProgram,
    /// `clone3` com ENOSYS: os argumentos do clone3 ficam numa struct na memória, que o BPF não lê,
    /// então não dá pra separar thread de fork ali. ENOSYS faz a glibc cair no `clone`, onde o filtro
    /// `deny` olha o `CLONE_THREAD`.
    pub clone3_enosys: BpfProgram,
}

/// Syscalls negadas, com o motivo, pro relatório.
pub fn denied_syscalls() -> Vec<(&'static str, &'static str)> {
    let mut v = vec![
        ("socket", "EPERM"),
        ("connect", "EPERM"),
        ("execve", "EPERM"),
        ("execveat", "EPERM"),
        ("clone sem CLONE_THREAD", "EPERM"),
        ("io_uring_setup", "EPERM"),
        ("io_uring_enter", "EPERM"),
        ("io_uring_register", "EPERM"),
        ("clone3", "ENOSYS"),
    ];
    if cfg!(target_arch = "x86_64") {
        v.push(("fork", "EPERM"));
        v.push(("vfork", "EPERM"));
        v.push(("variantes x32 das anteriores", "igual"));
    }
    v
}

fn arch() -> Result<TargetArch> {
    TargetArch::try_from(std::env::consts::ARCH).map_err(|e| anyhow::anyhow!("arquitetura sem seccompiler: {e}"))
}

/// Casa quando `flags & CLONE_THREAD == 0`, ou seja, quando o clone cria processo e não thread.
fn clone_without_thread() -> Result<Vec<SeccompRule>> {
    let cond = SeccompCondition::new(0, SeccompCmpArgLen::Qword, SeccompCmpOp::MaskedEq(libc::CLONE_THREAD as u64), 0)?;
    Ok(vec![SeccompRule::new(vec![cond])?])
}

pub fn seccomp_profile() -> Result<SeccompProfile> {
    let mut deny: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
    for nr in [
        libc::SYS_socket,
        libc::SYS_connect,
        libc::SYS_execve,
        libc::SYS_execveat,
        // io_uring executa IORING_OP_SOCKET/CONNECT/OPENAT sem passar pelo seccomp: tem que fechar a porta.
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
    ] {
        deny.insert(nr, Vec::new());
    }
    deny.insert(libc::SYS_clone, clone_without_thread()?);
    let mut clone3: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
    clone3.insert(libc::SYS_clone3, Vec::new());
    #[cfg(target_arch = "x86_64")]
    {
        deny.insert(libc::SYS_fork, Vec::new());
        deny.insert(libc::SYS_vfork, Vec::new());
        // Números x32: socket 41, connect 42, fork 57, vfork 58, execve 520, execveat 545,
        // io_uring 425 a 427, clone 56, clone3 435.
        for nr in [41, 42, 57, 58, 520, 545, 425, 426, 427] {
            deny.insert(X32_SYSCALL_BIT | nr, Vec::new());
        }
        deny.insert(X32_SYSCALL_BIT | 56, clone_without_thread()?);
        clone3.insert(X32_SYSCALL_BIT | 435, Vec::new());
    }
    let arch = arch()?;
    let deny: BpfProgram =
        SeccompFilter::new(deny, SeccompAction::Allow, SeccompAction::Errno(libc::EPERM as u32), arch)?.try_into()?;
    let clone3_enosys: BpfProgram =
        SeccompFilter::new(clone3, SeccompAction::Allow, SeccompAction::Errno(libc::ENOSYS as u32), arch)?
            .try_into()?;
    Ok(SeccompProfile { deny, clone3_enosys })
}

/// Aplica os dois filtros na thread atual (sem TSYNC: as outras threads não são tocadas).
pub fn apply_seccomp(profile: &SeccompProfile) -> Result<()> {
    seccompiler::apply_filter(&profile.deny).context("seccomp deny")?;
    seccompiler::apply_filter(&profile.clone3_enosys).context("seccomp clone3")?;
    Ok(())
}

/// Filtro só pra medição: regra com argumento nas syscalls do laço quente, com uma condição que nunca
/// casa. A ação deixa de ser constante pra essas syscalls e o kernel não pode mais pular o BPF pelo
/// cache de ação (Linux 5.11+). Mede o custo do BPF quando ele roda de verdade.
pub fn arg_checked_hot_syscalls() -> Result<BpfProgram> {
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
    for nr in [libc::SYS_getppid, libc::SYS_pread64, libc::SYS_openat, libc::SYS_close] {
        let never = SeccompCondition::new(0, SeccompCmpArgLen::Qword, SeccompCmpOp::Eq, 0xdead_beef_dead_beef)?;
        rules.insert(nr, vec![SeccompRule::new(vec![never])?]);
    }
    Ok(SeccompFilter::new(rules, SeccompAction::Allow, SeccompAction::Errno(libc::EPERM as u32), arch()?)?
        .try_into()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_compile_to_bpf() {
        let p = seccomp_profile().expect("perfil");
        assert!(p.deny.len() > 10, "deny com {} instruções", p.deny.len());
        assert!(!p.clone3_enosys.is_empty());
        assert!(!arg_checked_hot_syscalls().expect("filtro de medição").is_empty());
    }

    #[test]
    fn abi_numbers() {
        assert_eq!(abi_number(ABI::V6), 6);
        assert_eq!(abi_number(ABI::Unsupported), 0);
    }
}
