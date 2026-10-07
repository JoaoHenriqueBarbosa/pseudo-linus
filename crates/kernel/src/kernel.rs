//! O kernel: estado global de um processo host (CPUs virtuais e escalonador, contadores de dispositivo e de
//! inode do pipefs, a configuração), grupos de CPU por usuário e a fábrica de sandboxes.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use sched::{GroupId, Topology};
use sysabi::Errno;
use vfs::makedev;

use crate::config::{CpuTopology, KernelConfig, SandboxConfig};
use crate::cpu::{Cpus, GroupLimits};
use crate::sandbox::{CreateError, Sandbox, Snapshot};

pub(crate) struct KernelInner {
    pub config: KernelConfig,
    pub cpus: Arc<Cpus>,
    next_sandbox: AtomicU64,
    /// Menor livre dos dispositivos anônimos (0:N) dos sistemas de arquivos.
    next_anon_dev: AtomicU32,
    /// Inodes do pipefs (`pipe:[N]`), globais como no Linux.
    next_pipe_ino: AtomicU64,
}

impl KernelInner {
    pub(crate) fn anon_dev(&self) -> u64 {
        makedev(0, self.next_anon_dev.fetch_add(1, Ordering::Relaxed))
    }

    pub(crate) fn pipe_ino(&self) -> u64 {
        self.next_pipe_ino.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn sandbox_id(&self) -> u64 {
        self.next_sandbox.fetch_add(1, Ordering::Relaxed)
    }
}

/// Peso e limite de banda de um grupo de CPU (`cpu.weight` e `cpu.max` do cgroup v2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserGroupSpec {
    /// 1 a 10000; 100 é o padrão (um processo de nice 0).
    pub cpu_weight: u64,
    /// `(quota_ns, period_ns)`: no máximo `quota` de CPU a cada `period` (somando as CPUs). Quota mínima
    /// de 1 ms; período entre 1 ms e 1 s. `None` = sem limite.
    pub cpu_max: Option<(u64, u64)>,
}

impl Default for UserGroupSpec {
    fn default() -> Self {
        UserGroupSpec { cpu_weight: 100, cpu_max: None }
    }
}

struct UserGroupInner {
    id: GroupId,
    cpus: Arc<Cpus>,
}

impl Drop for UserGroupInner {
    fn drop(&mut self) {
        self.cpus.release_group(self.id);
    }
}

/// Grupo de CPU de um usuário: os sandboxes dele são subgrupos (usuário > sandbox > processo). O grupo é
/// devolvido quando a última alça (inclusive as guardadas pelos sandboxes) some.
#[derive(Clone)]
pub struct UserGroup {
    inner: Arc<UserGroupInner>,
}

impl UserGroup {
    pub(crate) fn id(&self) -> GroupId {
        self.inner.id
    }
}

impl std::fmt::Debug for UserGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UserGroup({})", self.inner.id.index())
    }
}

/// O kernel do pseudo-linus. Clonar é barato (um `Arc`).
#[derive(Clone)]
pub struct Kernel {
    pub(crate) inner: Arc<KernelInner>,
}

impl std::fmt::Debug for Kernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Kernel({:?})", self.inner.config)?;
        // `{:#?}` acrescenta o estado das CPUs e das tarefas, para diagnóstico de travamento.
        if f.alternate() {
            write!(f, "\n{}", self.inner.cpus.debug_state())?;
        }
        Ok(())
    }
}

impl Kernel {
    pub fn new(config: KernelConfig) -> Kernel {
        assert!(config.cpus >= 1 && config.cpus <= 64, "de 1 a 64 CPUs virtuais");
        let topo = match config.topology {
            CpuTopology::Shared => Topology::Shared,
            CpuTopology::PerCpu => Topology::PerCpu,
        };
        let cpus = Cpus::new(config.cpus, topo);
        Kernel {
            inner: Arc::new(KernelInner {
                config,
                cpus,
                next_sandbox: AtomicU64::new(1),
                // 0:14 é o pipefs; os tmpfs e procfs começam acima dos que um Debian costuma ter.
                next_anon_dev: AtomicU32::new(0x20),
                next_pipe_ino: AtomicU64::new(10_000),
            }),
        }
    }

    pub fn config(&self) -> &KernelConfig {
        &self.inner.config
    }

    /// Sandbox novo com a imagem Debian e os programas dados.
    pub fn create_sandbox(&self, cfg: SandboxConfig) -> Result<Sandbox, CreateError> {
        Sandbox::create(self.inner.clone(), cfg, None)
    }

    /// Sandbox novo a partir de um snapshot (O(1) no sistema de arquivos).
    pub fn create_sandbox_from(&self, snap: &Snapshot, cfg: SandboxConfig) -> Result<Sandbox, CreateError> {
        Sandbox::create(self.inner.clone(), cfg, Some(snap))
    }

    /// Grupo de CPU de um usuário. EINVAL com peso fora de 1..=10000 ou `cpu_max` inválido.
    pub fn create_user_group(&self, spec: UserGroupSpec) -> Result<UserGroup, Errno> {
        let cpus = self.inner.cpus.clone();
        let id = cpus.create_group(None, GroupLimits { weight: spec.cpu_weight, max: spec.cpu_max })?;
        Ok(UserGroup { inner: Arc::new(UserGroupInner { id, cpus }) })
    }

    /// Muda peso e limite de um grupo de usuário (vale na hora pros sandboxes dele).
    pub fn set_user_group_limits(&self, g: &UserGroup, cpu_weight: u64, cpu_max: Option<(u64, u64)>) -> Result<(), Errno> {
        self.inner.cpus.set_group(g.id(), GroupLimits { weight: cpu_weight, max: cpu_max })
    }

    /// CPU consumida por um grupo de usuário (todos os sandboxes dele), em ns.
    pub fn user_group_cpu_ns(&self, g: &UserGroup) -> u64 {
        self.inner.cpus.group_runtime(g.id())
    }
}
