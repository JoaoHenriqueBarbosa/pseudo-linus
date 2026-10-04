//! Configuração do kernel e dos sandboxes.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use sysabi::{Gid, Program, TimeSpec, Uid};

/// Ambiente padrão dos processos que o host cria (o mesmo do oráculo da bancada).
pub const BASE_ENV: &[(&str, &str)] = &[
    ("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"),
    ("HOME", "/root"),
    ("USER", "root"),
    ("LOGNAME", "root"),
    ("SHELL", "/bin/bash"),
    ("LC_ALL", "C.UTF-8"),
    ("TZ", "UTC"),
];

/// O que a thread spawner de um sandbox passa ao hook do host (Landlock e seccomp, E06).
#[derive(Clone, Debug)]
pub struct SpawnerInfo {
    pub sandbox_id: u64,
    /// Caminhos do host montados no sandbox (hostfs), que o Landlock precisa liberar.
    pub host_mounts: Vec<PathBuf>,
}

/// Hook chamado uma vez na thread spawner de cada sandbox, antes de qualquer thread de processo.
pub type SpawnerHook = Arc<dyn Fn(&SpawnerInfo) -> Result<(), String> + Send + Sync>;

/// Organização das runqueues do escalonador.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuTopology {
    /// Uma runqueue única pras CPUs virtuais (padrão): toda CPU ociosa pega trabalho na hora. Como o
    /// escalonador ainda não escolhe CPU no despertar (`select_task_rq`), é a que não deixa CPU ociosa com
    /// trabalho esperando o balanceamento do tick.
    Shared,
    /// Uma runqueue por CPU com balanceamento no tick, o modelo do kernel.
    PerCpu,
}

/// Configuração do kernel (um por processo host).
#[derive(Clone)]
pub struct KernelConfig {
    /// CPUs virtuais (tokens) que o escalonador distribui.
    pub cpus: usize,
    pub topology: CpuTopology,
    /// Pilha da thread de cada pseudo-processo.
    pub stack_size: usize,
    pub spawner_hook: Option<SpawnerHook>,
}

impl std::fmt::Debug for KernelConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KernelConfig")
            .field("cpus", &self.cpus)
            .field("topology", &self.topology)
            .field("stack_size", &self.stack_size)
            .field("spawner_hook", &self.spawner_hook.is_some())
            .finish()
    }
}

impl Default for KernelConfig {
    fn default() -> Self {
        KernelConfig { cpus: 2, topology: CpuTopology::Shared, stack_size: 2 << 20, spawner_hook: None }
    }
}

/// Relógio de parede de um sandbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockMode {
    /// O relógio do host.
    Host,
    /// Congelado num instante (o `faketime -f` da bancada).
    Fixed(TimeSpec),
    /// Começa no instante dado quando o modo é ligado e anda com o tempo real.
    StartAt(TimeSpec),
}

/// Limites de um sandbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SandboxLimits {
    /// Processos vivos (não contando o init); passar dá EAGAIN no fork/spawn.
    pub max_procs: Option<u32>,
    /// Memória do sandbox (allocator rastreado + estruturas do kernel).
    pub mem_bytes: Option<u64>,
    /// Tamanho do tmpfs da raiz (`size=`); ENOSPC quando acaba.
    pub fs_bytes: Option<u64>,
    /// Inodes do tmpfs da raiz (`nr_inodes=`).
    pub fs_inodes: Option<u64>,
    /// `RLIMIT_NOFILE` inicial (cur e max).
    pub nofile: u64,
    /// `RLIMIT_FSIZE` inicial (cur e max), em bytes.
    pub fsize: u64,
}

impl Default for SandboxLimits {
    fn default() -> Self {
        SandboxLimits {
            max_procs: None,
            mem_bytes: None,
            fs_bytes: None,
            fs_inodes: None,
            // Valor do container Debian da bancada (`ulimit -n`).
            nofile: 1_073_741_816,
            fsize: sysabi::RLIM_INFINITY,
        }
    }
}

/// Configuração de um sandbox.
#[derive(Clone, Debug)]
pub struct SandboxConfig {
    /// Programas embutidos: um inode em `dir/name` pra cada um.
    pub programs: Vec<Program>,
    pub hostname: String,
    /// Ambiente padrão (`NAME=valor`) dos processos criados pelo host.
    pub env: Vec<Vec<u8>>,
    /// cwd padrão dos processos criados pelo host.
    pub cwd: Vec<u8>,
    pub clock: ClockMode,
    pub limits: SandboxLimits,
    /// Credenciais dos processos criados pelo host (padrão: root).
    pub uid: Uid,
    pub gid: Gid,
    /// Grupo de CPU do usuário dono (o sandbox vira subgrupo dele); `None` = direto na raiz.
    pub user_group: Option<crate::kernel::UserGroup>,
    /// `cpu.weight` do sandbox dentro do grupo do usuário (1 a 10000).
    pub cpu_weight: u64,
    /// `cpu.max` do sandbox: `(quota_ns, period_ns)`.
    pub cpu_max: Option<(u64, u64)>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        SandboxConfig {
            programs: Vec::new(),
            hostname: "sandbox".to_string(),
            env: BASE_ENV.iter().map(|(k, v)| format!("{k}={v}").into_bytes()).collect(),
            cwd: b"/root".to_vec(),
            clock: ClockMode::Host,
            limits: SandboxLimits::default(),
            uid: 0,
            gid: 0,
            user_group: None,
            cpu_weight: 100,
            cpu_max: None,
        }
    }
}

/// Prazo padrão de um `run` sem timeout explícito.
pub const DEFAULT_RUN_TIMEOUT: Duration = Duration::from_secs(600);
