//! Isolamento em runtime das threads de pseudo-processo (E06, H21).
//!
//! O kernel cria uma spawner thread por sandbox, chama [`IsolationProfile::apply_current_thread`]
//! nela uma vez, e cria dela as threads dos pseudo-processos, que herdam as duas camadas sem custo.
//! Nada é aplicado na thread principal nem nas threads do worker (protocolo, timer, rede do kernel).
//!
//! - **Landlock**: nega todo acesso a arquivo do host fora dos diretórios montados (hostfs), nega
//!   bind e connect TCP, e (ABI v6) isola sinais e sockets abstratos. Pedido na ABI v6 em modo best
//!   effort: num kernel com ABI menor (a VPS tem v4) aplica o que existe e o relatório diz o que ficou.
//! - **seccomp**: lista de negação com EPERM pras syscalls que um código seguro poderia usar pra sair
//!   da sandbox (rede, exec, fork, io_uring, sinais pra fora, ptrace, montagem, módulos...), mais as
//!   variantes x32; `clone3` volta ENOSYS pra glibc cair no `clone`, onde o filtro olha o
//!   `CLONE_THREAD` (thread passa, processo não).
//!
//! Limite (do próprio E06): protege contra código seguro fazendo I/O de host, não contra corrupção de
//! memória numa dependência com unsafe.

use std::collections::BTreeMap;
use std::path::PathBuf;

use landlock::{
    ABI, Access, AccessFs, AccessNet, CompatLevel, Compatible, LandlockStatus, Ruleset, RulesetAttr, RulesetCreatedAttr,
    RulesetStatus, Scope, path_beneath_rules,
};
use seccompiler::{BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter, SeccompRule, TargetArch};
use serde::{Deserialize, Serialize};

use crate::config::{Enforcement, IsolationConfig};

/// ABI do Landlock pedida (a do host de referência, Linux 6.12).
pub const TARGET_ABI: ABI = ABI::V6;

#[cfg(target_arch = "x86_64")]
const X32_SYSCALL_BIT: i64 = 0x4000_0000;

/// O que foi aplicado numa thread.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsolationReport {
    /// `fully_enforced`, `partially_enforced`, `not_enforced` ou `off`.
    pub landlock: String,
    /// ABI efetiva do Landlock no kernel do host.
    pub landlock_abi: Option<i32>,
    pub seccomp: bool,
}

impl IsolationReport {
    pub fn summary(&self) -> String {
        format!(
            "landlock={} (ABI {}), seccomp={}",
            self.landlock,
            self.landlock_abi.map_or("-".to_string(), |a| a.to_string()),
            if self.seccomp { "on" } else { "off" }
        )
    }
}

fn abi_number(abi: ABI) -> i32 {
    match abi {
        ABI::Unsupported => 0,
        ABI::V1 => 1,
        ABI::V2 => 2,
        ABI::V3 => 3,
        ABI::V4 => 4,
        ABI::V5 => 5,
        ABI::V6 => 6,
        _ => -1,
    }
}

/// Os filtros compilados uma vez e a política de cada camada.
#[derive(Clone, Debug)]
pub struct IsolationProfile {
    cfg: IsolationConfig,
    deny: Option<BpfProgram>,
    clone3_enosys: Option<BpfProgram>,
}

/// Syscalls negadas com EPERM, pelo número nativo. `clone` entra à parte (só sem `CLONE_THREAD`).
fn denied() -> Vec<i64> {
    let mut v = vec![
        libc::SYS_socket,
        libc::SYS_connect,
        libc::SYS_execve,
        libc::SYS_execveat,
        // io_uring executa SOCKET/CONNECT/OPENAT sem passar pelo seccomp.
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
        // Sinal pra processo do host (o `raise` do próprio processo usa tgkill, que fica liberado).
        libc::SYS_kill,
        libc::SYS_pidfd_open,
        libc::SYS_pidfd_send_signal,
        libc::SYS_pidfd_getfd,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_chroot,
        libc::SYS_unshare,
        libc::SYS_setns,
        libc::SYS_bpf,
        libc::SYS_perf_event_open,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_reboot,
        libc::SYS_kexec_load,
        libc::SYS_init_module,
        libc::SYS_finit_module,
        libc::SYS_delete_module,
        libc::SYS_swapon,
        libc::SYS_swapoff,
        libc::SYS_settimeofday,
        libc::SYS_clock_settime,
        libc::SYS_sethostname,
        libc::SYS_setdomainname,
        libc::SYS_acct,
        libc::SYS_userfaultfd,
        libc::SYS_open_by_handle_at,
        libc::SYS_name_to_handle_at,
    ];
    #[cfg(target_arch = "x86_64")]
    {
        v.push(libc::SYS_fork);
        v.push(libc::SYS_vfork);
    }
    v
}

/// Casa quando `flags & CLONE_THREAD == 0` (o clone cria processo, não thread).
fn clone_without_thread() -> Result<Vec<SeccompRule>, String> {
    let cond = SeccompCondition::new(0, SeccompCmpArgLen::Qword, SeccompCmpOp::MaskedEq(libc::CLONE_THREAD as u64), 0)
        .map_err(|e| e.to_string())?;
    Ok(vec![SeccompRule::new(vec![cond]).map_err(|e| e.to_string())?])
}

fn compile_seccomp() -> Result<(BpfProgram, BpfProgram), String> {
    let arch = TargetArch::try_from(std::env::consts::ARCH).map_err(|e| format!("arquitetura sem seccomp: {e}"))?;
    let mut deny: BTreeMap<i64, Vec<SeccompRule>> = denied().into_iter().map(|n| (n, Vec::new())).collect();
    deny.insert(libc::SYS_clone, clone_without_thread()?);
    let mut clone3: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
    clone3.insert(libc::SYS_clone3, Vec::new());
    #[cfg(target_arch = "x86_64")]
    {
        // x32: o número é o do x86_64 com o bit 30 ligado, menos as syscalls com ABI própria no x32
        // (512 em diante), que entram pelo número delas. O filtro compara o número exato.
        for nr in denied() {
            deny.insert(X32_SYSCALL_BIT | nr, Vec::new());
        }
        // execve 520, ptrace 521, process_vm_readv 539, process_vm_writev 540, execveat 545.
        for nr in [520, 521, 539, 540, 545] {
            deny.insert(X32_SYSCALL_BIT | nr, Vec::new());
        }
        deny.insert(X32_SYSCALL_BIT | libc::SYS_clone, clone_without_thread()?);
        clone3.insert(X32_SYSCALL_BIT | libc::SYS_clone3, Vec::new());
    }
    let deny: BpfProgram = SeccompFilter::new(deny, SeccompAction::Allow, SeccompAction::Errno(libc::EPERM as u32), arch)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|e: seccompiler::BackendError| e.to_string())?;
    let clone3: BpfProgram = SeccompFilter::new(clone3, SeccompAction::Allow, SeccompAction::Errno(libc::ENOSYS as u32), arch)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|e: seccompiler::BackendError| e.to_string())?;
    Ok((deny, clone3))
}

impl IsolationProfile {
    /// Compila os filtros. Falha só se o seccomp for exigido e não compilar.
    pub fn new(cfg: &IsolationConfig) -> Result<IsolationProfile, String> {
        let (deny, clone3_enosys) = match cfg.seccomp {
            Enforcement::Off => (None, None),
            _ => match compile_seccomp() {
                Ok((d, c)) => (Some(d), Some(c)),
                Err(e) if cfg.seccomp == Enforcement::BestEffort => {
                    tracing::warn!("seccomp indisponível: {e}");
                    (None, None)
                }
                Err(e) => return Err(format!("seccomp: {e}")),
            },
        };
        Ok(IsolationProfile { cfg: cfg.clone(), deny, clone3_enosys })
    }

    /// Restringe a thread que chama (e as que ela criar depois). `mounts`: diretórios do host que a
    /// sandbox enxerga (hostfs); o resto do FS do host fica inacessível. Irreversível.
    pub fn apply_current_thread(&self, mounts: &[PathBuf]) -> Result<IsolationReport, String> {
        let mut report = IsolationReport { landlock: "off".into(), landlock_abi: None, seccomp: false };
        if self.cfg.landlock != Enforcement::Off {
            let level = CompatLevel::BestEffort;
            let status = Ruleset::default()
                .set_compatibility(level)
                .handle_access(AccessFs::from_all(TARGET_ABI))
                .and_then(|r| r.handle_access(AccessNet::from_all(TARGET_ABI)))
                .and_then(|r| r.scope(Scope::from_all(TARGET_ABI)))
                .and_then(|r| r.create())
                .and_then(|r| r.add_rules(path_beneath_rules(mounts, AccessFs::from_all(TARGET_ABI))))
                .and_then(|r| r.restrict_self())
                .map_err(|e| format!("landlock: {e}"))?;
            report.landlock = match status.ruleset {
                RulesetStatus::FullyEnforced => "fully_enforced",
                RulesetStatus::PartiallyEnforced => "partially_enforced",
                RulesetStatus::NotEnforced => "not_enforced",
            }
            .into();
            if let LandlockStatus::Available { effective_abi, .. } = status.landlock {
                report.landlock_abi = Some(abi_number(effective_abi));
            }
            if self.cfg.landlock == Enforcement::Required && status.ruleset == RulesetStatus::NotEnforced {
                return Err("landlock exigido, mas o kernel do host não aplicou nada (Landlock desligado?)".into());
            }
        }
        if let (Some(deny), Some(clone3)) = (&self.deny, &self.clone3_enosys) {
            match seccompiler::apply_filter(deny).and_then(|()| seccompiler::apply_filter(clone3)) {
                Ok(()) => report.seccomp = true,
                Err(e) if self.cfg.seccomp == Enforcement::Required => return Err(format!("seccomp: {e}")),
                Err(e) => tracing::warn!("seccomp não aplicado: {e}"),
            }
        }
        Ok(report)
    }
}

/// Resultado do autoteste: o que uma thread restrita conseguiu fazer no host onde o daemon roda.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelfTest {
    pub applied: IsolationReport,
    /// Errno de cada tentativa (`ok` = conseguiu).
    pub read_outside: String,
    pub read_mounted: String,
    pub spawn_process: String,
    pub tcp_connect: String,
    pub child_thread_read_outside: String,
    /// Se o resultado bate com a política configurada.
    pub ok: bool,
    pub problems: Vec<String>,
}

fn errno_of<T>(r: std::io::Result<T>) -> String {
    match r {
        Ok(_) => "ok".into(),
        Err(e) => match e.raw_os_error() {
            Some(n) => sysabi::Errno(n).name().unwrap_or("?").to_string(),
            None => e.to_string(),
        },
    }
}

/// Aplica o perfil numa thread descartável e tenta sair da sandbox de quatro jeitos.
pub fn selftest(cfg: &IsolationConfig, mounted: &std::path::Path) -> Result<SelfTest, String> {
    let profile = IsolationProfile::new(cfg)?;
    let probe = mounted.join(".pl-selftest");
    std::fs::write(&probe, b"ok").map_err(|e| format!("{}: {e}", probe.display()))?;
    let outside = std::env::current_exe().map_err(|e| e.to_string())?;
    let mounts = vec![mounted.to_path_buf()];
    let probe2 = probe.clone();
    let r = std::thread::Builder::new()
        .name("pl-selftest".into())
        .spawn(move || -> Result<SelfTest, String> {
            let applied = profile.apply_current_thread(&mounts)?;
            let read_outside = errno_of(std::fs::read(&outside));
            let read_mounted = errno_of(std::fs::read(&probe2));
            let spawn_process = errno_of(std::process::Command::new(&outside).arg("--version").output());
            let tcp_connect = errno_of(std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], 9)),
                std::time::Duration::from_millis(200),
            ));
            let out2 = outside.clone();
            let child_thread_read_outside = std::thread::spawn(move || errno_of(std::fs::read(&out2)))
                .join()
                .map_err(|_| "a thread filha entrou em pânico".to_string())?;
            Ok(SelfTest {
                applied,
                read_outside,
                read_mounted,
                spawn_process,
                tcp_connect,
                child_thread_read_outside,
                ok: true,
                problems: Vec::new(),
            })
        })
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "a thread do autoteste entrou em pânico".to_string())?;
    let _ = std::fs::remove_file(&probe);
    let mut t = r?;
    let landlock_on = matches!(t.applied.landlock.as_str(), "fully_enforced" | "partially_enforced");
    if cfg.landlock != Enforcement::Off && landlock_on {
        if t.read_outside != "EACCES" {
            t.problems.push(format!("leitura fora da montagem deu {}, esperado EACCES", t.read_outside));
        }
        if t.child_thread_read_outside != "EACCES" {
            t.problems.push(format!("thread filha leu fora da montagem: {}", t.child_thread_read_outside));
        }
    }
    if cfg.landlock == Enforcement::Required && !landlock_on {
        t.problems.push("landlock exigido e não aplicado".into());
    }
    if t.read_mounted != "ok" {
        t.problems.push(format!("leitura dentro da montagem deu {}", t.read_mounted));
    }
    if cfg.seccomp != Enforcement::Off {
        if !t.applied.seccomp && cfg.seccomp == Enforcement::Required {
            t.problems.push("seccomp exigido e não aplicado".into());
        }
        if t.applied.seccomp {
            // EPERM vem do seccomp (clone/fork); EACCES, do Landlock negando executar o arquivo antes.
            if !matches!(t.spawn_process.as_str(), "EPERM" | "EACCES") {
                t.problems.push(format!("criar processo deu {}, esperado EPERM ou EACCES", t.spawn_process));
            }
            if t.tcp_connect != "EPERM" {
                t.problems.push(format!("connect TCP deu {}, esperado EPERM", t.tcp_connect));
            }
        }
    }
    t.ok = t.problems.is_empty();
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restricted_thread_cannot_touch_host_but_neighbors_can() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("dentro.txt");
        std::fs::write(&inside, b"ok").unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(outside.path(), b"segredo").unwrap();
        let prof = IsolationProfile::new(&IsolationConfig { landlock: Enforcement::BestEffort, seccomp: Enforcement::Required })
            .unwrap();
        let mounts = vec![dir.path().to_path_buf()];
        let (outside_path, inside_path) = (outside.path().to_path_buf(), inside.clone());
        let r = std::thread::spawn(move || {
            let rep = prof.apply_current_thread(&mounts).unwrap();
            let read_inside = std::fs::read(&inside_path).map(|_| ()).map_err(|e| e.kind());
            let read_outside = std::fs::read(&outside_path).map(|_| ()).map_err(|e| e.raw_os_error());
            let spawn = std::process::Command::new("/bin/true").status().map(|_| ()).map_err(|e| e.raw_os_error());
            let net = std::net::TcpStream::connect("127.0.0.1:9").map(|_| ()).map_err(|e| e.raw_os_error());
            // Uma thread filha da restrita herda as duas camadas.
            let child = std::thread::spawn(move || std::fs::read("/etc/hostname").map(|_| ()).map_err(|e| e.raw_os_error()))
                .join()
                .unwrap();
            (rep, read_inside, read_outside, spawn, net, child)
        })
        .join()
        .unwrap();
        let (rep, read_inside, read_outside, spawn, net, child) = r;
        assert!(rep.seccomp);
        assert_eq!(read_inside, Ok(()));
        if rep.landlock != "not_enforced" {
            assert_eq!(read_outside, Err(Some(libc::EACCES)), "{rep:?}");
            assert_eq!(child, Err(Some(libc::EACCES)));
        }
        assert_eq!(spawn, Err(Some(libc::EPERM)));
        assert_eq!(net, Err(Some(libc::EPERM)));
        // A thread de teste (vizinha) continua livre.
        assert_eq!(std::fs::read(outside.path()).unwrap(), b"segredo");
        assert!(std::process::Command::new("/bin/true").status().is_ok());
    }

    #[test]
    fn selftest_passes_on_this_host() {
        let dir = tempfile::tempdir().unwrap();
        let t = selftest(&IsolationConfig::default(), dir.path()).unwrap();
        assert!(t.ok, "{t:?}");
        assert_ne!(t.spawn_process, "ok");
        // Só o seccomp: o exec chega até o filtro e leva EPERM.
        let cfg = IsolationConfig { landlock: Enforcement::Off, seccomp: Enforcement::Required };
        let t = selftest(&cfg, dir.path()).unwrap();
        assert!(t.ok, "{t:?}");
        assert_eq!(t.spawn_process, "EPERM");
        assert_eq!(t.read_outside, "ok");
    }

    #[test]
    fn off_applies_nothing() {
        let prof = IsolationProfile::new(&IsolationConfig { landlock: Enforcement::Off, seccomp: Enforcement::Off }).unwrap();
        let rep = std::thread::spawn(move || prof.apply_current_thread(&[]).unwrap()).join().unwrap();
        assert_eq!(rep.landlock, "off");
        assert!(!rep.seccomp);
    }
}
