//! Configuração do daemon.
//!
//! Arquivo TOML (todas as chaves são opcionais; os padrões cabem na VPS de 2 vCPUs e ~3 GiB livres), com
//! sobrescrita por variáveis de ambiente pras chaves que mudam por implantação: `PL_LISTEN`,
//! `PL_DATA_DIR`, `PL_WORKERS`, `PL_CPUS_PER_WORKER`.

use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Tamanho em bytes; no TOML aceita número ou texto com unidade (`512MiB`, `1GiB`, `64KiB`, `10M`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteSize(pub u64);

impl ByteSize {
    pub const fn kib(n: u64) -> ByteSize {
        ByteSize(n << 10)
    }
    pub const fn mib(n: u64) -> ByteSize {
        ByteSize(n << 20)
    }
    pub const fn gib(n: u64) -> ByteSize {
        ByteSize(n << 30)
    }

    pub fn parse(s: &str) -> Result<ByteSize, String> {
        let s = s.trim();
        let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        let (num, unit) = s.split_at(split);
        let n: u64 = num.parse().map_err(|_| format!("tamanho inválido: {s:?}"))?;
        let mult: u64 = match unit.trim() {
            "" | "B" => 1,
            "K" | "KiB" | "k" => 1 << 10,
            "M" | "MiB" => 1 << 20,
            "G" | "GiB" => 1 << 30,
            "T" | "TiB" => 1 << 40,
            "KB" | "kB" => 1000,
            "MB" => 1_000_000,
            "GB" => 1_000_000_000,
            u => return Err(format!("unidade de tamanho desconhecida {u:?} em {s:?}")),
        };
        n.checked_mul(mult).map(ByteSize).ok_or_else(|| format!("tamanho grande demais: {s:?}"))
    }
}

impl fmt::Display for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.0;
        for (shift, unit) in [(40, "TiB"), (30, "GiB"), (20, "MiB"), (10, "KiB")] {
            if n >= 1 << shift && n.is_multiple_of(1 << shift) {
                return write!(f, "{}{unit}", n >> shift);
            }
        }
        write!(f, "{n}")
    }
}

impl Serialize for ByteSize {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(self.0)
    }
}

impl<'de> Deserialize<'de> for ByteSize {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<ByteSize, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            N(u64),
            S(String),
        }
        match Raw::deserialize(d)? {
            Raw::N(n) => Ok(ByteSize(n)),
            Raw::S(s) => ByteSize::parse(&s).map_err(serde::de::Error::custom),
        }
    }
}

/// `cpu.max` do cgroup v2: `quota_us` de CPU a cada `period_us`. `quota_us = period_us` é uma CPU
/// virtual inteira.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CpuMax {
    pub quota_us: u64,
    pub period_us: u64,
}

impl CpuMax {
    /// Os mesmos limites do kernel (`tg_set_cfs_bandwidth`): período de 1 ms a 1 s, quota de pelo
    /// menos 1 ms.
    pub fn validate(&self) -> Result<(), String> {
        if !(1_000..=1_000_000).contains(&self.period_us) {
            return Err(format!("cpu_max.period_us fora de 1000..=1000000: {}", self.period_us));
        }
        if self.quota_us < 1_000 {
            return Err(format!("cpu_max.quota_us menor que 1000: {}", self.quota_us));
        }
        Ok(())
    }
}

/// Quota de um usuário: teto da soma dos recursos de todas as sandboxes dele e dos pedidos em voo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Quota {
    /// Sandboxes vivas ao mesmo tempo.
    pub max_sandboxes: u32,
    /// Soma dos tetos de memória das sandboxes.
    pub mem_bytes: ByteSize,
    /// Soma dos tetos de processos das sandboxes.
    pub max_procs: u32,
    /// Peso do grupo do usuário no escalonador (`cpu.weight`, 1 a 10000; 100 é o padrão do cgroup).
    pub cpu_weight: u32,
    /// Teto de banda do grupo do usuário (`cpu.max`); `None` é `max`.
    pub cpu_max: Option<CpuMax>,
    /// `exec` e `session.exec` em andamento ao mesmo tempo.
    pub max_concurrent_execs: u32,
    /// Sessões abertas ao mesmo tempo.
    pub max_sessions: u32,
    /// Maior timeout de parede que o usuário pode pedir.
    pub max_timeout_ms: u64,
    /// Maior limite de saída (por fluxo) que o usuário pode pedir.
    pub max_output_bytes: ByteSize,
    /// Snapshots persistidos em disco (sobrevivem à queda do worker), somando todas as sandboxes.
    pub max_persisted_snapshots: u32,
}

impl Default for Quota {
    fn default() -> Quota {
        Quota {
            max_sandboxes: 4,
            mem_bytes: ByteSize::mib(1024),
            max_procs: 512,
            cpu_weight: 100,
            cpu_max: Some(CpuMax { quota_us: 100_000, period_us: 100_000 }),
            max_concurrent_execs: 8,
            max_sessions: 8,
            max_timeout_ms: 30 * 60 * 1000,
            max_output_bytes: ByteSize::mib(16),
            max_persisted_snapshots: 8,
        }
    }
}

impl Quota {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=10_000).contains(&self.cpu_weight) {
            return Err(format!("cpu_weight fora de 1..=10000: {}", self.cpu_weight));
        }
        if let Some(c) = &self.cpu_max {
            c.validate()?;
        }
        if self.max_timeout_ms == 0 {
            return Err("max_timeout_ms precisa ser maior que zero".into());
        }
        Ok(())
    }
}

/// Sobrescrita parcial da quota padrão, guardada por usuário no arquivo de autenticação. O que fica
/// `None` segue o padrão da configuração (e acompanha mudanças nele).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct QuotaOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_sandboxes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_bytes: Option<ByteSize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_procs: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_weight: Option<u32>,
    /// `Some(None)` desliga o teto de banda (`max`); `None` segue o padrão.
    #[serde(skip_serializing_if = "Option::is_none", with = "double_option")]
    pub cpu_max: Option<Option<CpuMax>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrent_execs: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_sessions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_bytes: Option<ByteSize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_persisted_snapshots: Option<u32>,
}

mod double_option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<T: Serialize, S: Serializer>(v: &Option<Option<T>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(inner) => inner.serialize(s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
        Option::<T>::deserialize(d).map(Some)
    }
}

impl QuotaOverride {
    pub fn apply(&self, base: &Quota) -> Quota {
        Quota {
            max_sandboxes: self.max_sandboxes.unwrap_or(base.max_sandboxes),
            mem_bytes: self.mem_bytes.unwrap_or(base.mem_bytes),
            max_procs: self.max_procs.unwrap_or(base.max_procs),
            cpu_weight: self.cpu_weight.unwrap_or(base.cpu_weight),
            cpu_max: self.cpu_max.unwrap_or(base.cpu_max),
            max_concurrent_execs: self.max_concurrent_execs.unwrap_or(base.max_concurrent_execs),
            max_sessions: self.max_sessions.unwrap_or(base.max_sessions),
            max_timeout_ms: self.max_timeout_ms.unwrap_or(base.max_timeout_ms),
            max_output_bytes: self.max_output_bytes.unwrap_or(base.max_output_bytes),
            max_persisted_snapshots: self.max_persisted_snapshots.unwrap_or(base.max_persisted_snapshots),
        }
    }

    /// Junta `other` por cima (o que `other` define vence).
    pub fn merge(&mut self, other: &QuotaOverride) {
        macro_rules! take {
            ($($f:ident),*) => { $( if other.$f.is_some() { self.$f = other.$f.clone(); } )* };
        }
        take!(
            max_sandboxes,
            mem_bytes,
            max_procs,
            cpu_weight,
            cpu_max,
            max_concurrent_execs,
            max_sessions,
            max_timeout_ms,
            max_output_bytes,
            max_persisted_snapshots
        );
    }
}

/// Padrões e tetos de uma sandbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SandboxDefaults {
    /// Teto de memória padrão (allocator rastreado mais o contador do kernel).
    pub mem_bytes: ByteSize,
    /// Teto padrão de processos simultâneos (`RLIMIT_NPROC` da sandbox).
    pub max_procs: u32,
    /// Cota do tmpfs.
    pub fs_bytes: ByteSize,
    /// `RLIMIT_NOFILE` padrão dos processos.
    pub nofile: u64,
    /// Timeout de parede padrão do `exec`.
    pub exec_timeout_ms: u64,
    /// Limite padrão de saída guardada por fluxo (stdout e stderr, cada um).
    pub output_limit_bytes: ByteSize,
    /// Depois do limite, a saída continua sendo lida e descartada até este total; aí o host fecha o
    /// pipe e o escritor leva SIGPIPE, como num `| head -c`.
    pub max_discard_bytes: ByteSize,
    /// Quanto o `exec` espera a saída de processos em segundo plano depois que o comando termina.
    pub drain_grace_ms: u64,
    /// Sandbox sem uso por esse tempo é destruída (0 desliga).
    pub idle_ttl_secs: u64,
    /// Snapshots guardados por sandbox (em memória; cada um segura os blocos que mudaram depois dele).
    pub max_snapshots: u32,
    /// Intervalo mínimo entre dois autosaves de uma sandbox alterada: o host persiste sozinho um
    /// snapshot (`autosave`, um por sandbox, sempre o mais novo) pra que a queda do worker nunca
    /// perca o estado do usuário, sem ninguém ter pedido snapshot. 0 desliga.
    pub autosave_secs: u64,
}

impl Default for SandboxDefaults {
    fn default() -> SandboxDefaults {
        SandboxDefaults {
            mem_bytes: ByteSize::mib(256),
            max_procs: 128,
            fs_bytes: ByteSize::mib(256),
            nofile: 1024,
            exec_timeout_ms: 120_000,
            output_limit_bytes: ByteSize::mib(1),
            max_discard_bytes: ByteSize::mib(256),
            drain_grace_ms: 100,
            idle_ttl_secs: 24 * 3600,
            max_snapshots: 16,
            autosave_secs: 60,
        }
    }
}

/// Tetos do serviço inteiro (controle de admissão).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ServiceLimits {
    /// Soma dos tetos de memória de todas as sandboxes de todos os usuários.
    pub memory_budget: ByteSize,
    /// Sandboxes vivas no serviço.
    pub max_sandboxes: u32,
    /// `exec` em andamento no serviço.
    pub max_concurrent_execs: u32,
    /// Maior corpo de requisição HTTP (inclui `fs.write` e `import` em base64).
    pub max_request_bytes: ByteSize,
    /// Conexões HTTP abertas ao mesmo tempo.
    pub max_connections: u32,
}

impl Default for ServiceLimits {
    fn default() -> ServiceLimits {
        ServiceLimits {
            memory_budget: ByteSize::mib(2048),
            max_sandboxes: 64,
            max_concurrent_execs: 64,
            max_request_bytes: ByteSize::mib(64),
            max_connections: 512,
        }
    }
}

/// Como os workers são geridos.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WorkerTuning {
    /// Espera inicial antes de reiniciar um worker que caiu; dobra a cada queda seguida.
    pub restart_backoff_min_ms: u64,
    pub restart_backoff_max_ms: u64,
    /// Um worker que ficou de pé esse tempo zera o backoff.
    pub stable_after_ms: u64,
    /// Intervalo do ping de saúde e quanto esperar a resposta antes de matar o worker.
    pub ping_interval_ms: u64,
    pub ping_timeout_ms: u64,
    /// Quanto esperar o worker ficar pronto ao subir.
    pub startup_timeout_ms: u64,
}

impl Default for WorkerTuning {
    fn default() -> WorkerTuning {
        WorkerTuning {
            restart_backoff_min_ms: 200,
            restart_backoff_max_ms: 30_000,
            stable_after_ms: 60_000,
            ping_interval_ms: 5_000,
            ping_timeout_ms: 20_000,
            startup_timeout_ms: 30_000,
        }
    }
}

/// Exigência de uma camada de isolamento em runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Enforcement {
    /// A sandbox não sobe se a camada não puder ser aplicada por completo.
    Required,
    /// Aplica o que o kernel do host suportar e registra o que ficou de fora.
    BestEffort,
    /// Desligada (só pra diagnóstico; o daemon avisa no log).
    Off,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct IsolationConfig {
    pub landlock: Enforcement,
    pub seccomp: Enforcement,
}

impl Default for IsolationConfig {
    fn default() -> IsolationConfig {
        IsolationConfig { landlock: Enforcement::BestEffort, seccomp: Enforcement::Required }
    }
}

/// Configuração completa.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// Endereço HTTP.
    pub listen: String,
    /// Diretório de dados (chaves, snapshots persistidos). Precisa sobreviver a reinícios.
    pub data_dir: PathBuf,
    /// Quantos workers (processos do host com um kernel cada).
    pub workers: usize,
    /// CPUs virtuais de cada worker.
    pub cpus_per_worker: usize,
    /// `kernel` (o pseudo-linus de verdade) ou `fake` (dublê de teste; exige a feature `fake-backend` e
    /// `PL_ALLOW_FAKE_BACKEND=1`).
    pub backend: String,
    pub service: ServiceLimits,
    /// Quota padrão de usuário (cada usuário pode ter sobrescritas no arquivo de autenticação).
    pub quota: Quota,
    pub sandbox: SandboxDefaults,
    pub worker: WorkerTuning,
    pub isolation: IsolationConfig,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            listen: "127.0.0.1:8080".into(),
            data_dir: PathBuf::from("/var/lib/pseudo-linus"),
            workers: 2,
            cpus_per_worker: 2,
            backend: "kernel".into(),
            service: ServiceLimits::default(),
            quota: Quota::default(),
            sandbox: SandboxDefaults::default(),
            worker: WorkerTuning::default(),
            isolation: IsolationConfig::default(),
        }
    }
}

impl Config {
    /// Lê o arquivo (se houver), aplica as variáveis de ambiente e valida.
    pub fn load(path: Option<&Path>) -> Result<Config, String> {
        let mut cfg = match path {
            Some(p) => {
                let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {}", p.display(), io_msg(&e)))?;
                toml::from_str::<Config>(&text).map_err(|e| format!("{}: {e}", p.display()))?
            }
            None => Config::default(),
        };
        cfg.apply_env(|k| std::env::var(k).ok())?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn apply_env(&mut self, get: impl Fn(&str) -> Option<String>) -> Result<(), String> {
        if let Some(v) = get("PL_LISTEN") {
            self.listen = v;
        }
        if let Some(v) = get("PL_DATA_DIR") {
            self.data_dir = PathBuf::from(v);
        }
        if let Some(v) = get("PL_WORKERS") {
            self.workers = v.parse().map_err(|_| format!("PL_WORKERS inválido: {v:?}"))?;
        }
        if let Some(v) = get("PL_CPUS_PER_WORKER") {
            self.cpus_per_worker = v.parse().map_err(|_| format!("PL_CPUS_PER_WORKER inválido: {v:?}"))?;
        }
        if let Some(v) = get("PL_BACKEND") {
            self.backend = v;
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        self.listen.parse::<SocketAddr>().map_err(|_| format!("listen inválido: {:?}", self.listen))?;
        if !(1..=64).contains(&self.workers) {
            return Err(format!("workers fora de 1..=64: {}", self.workers));
        }
        if !(1..=16).contains(&self.cpus_per_worker) {
            return Err(format!("cpus_per_worker fora de 1..=16: {}", self.cpus_per_worker));
        }
        if !matches!(self.backend.as_str(), "kernel" | "fake") {
            return Err(format!("backend desconhecido: {:?} (use kernel)", self.backend));
        }
        self.quota.validate().map_err(|e| format!("quota: {e}"))?;
        let s = &self.sandbox;
        if s.exec_timeout_ms == 0 || s.exec_timeout_ms > self.quota.max_timeout_ms {
            return Err("sandbox.exec_timeout_ms precisa ficar entre 1 e quota.max_timeout_ms".into());
        }
        if s.output_limit_bytes > self.quota.max_output_bytes {
            return Err("sandbox.output_limit_bytes maior que quota.max_output_bytes".into());
        }
        if s.max_procs == 0 || s.mem_bytes.0 == 0 {
            return Err("sandbox.max_procs e sandbox.mem_bytes precisam ser maiores que zero".into());
        }
        if self.service.max_request_bytes.0 < 1024 {
            return Err("service.max_request_bytes pequeno demais".into());
        }
        Ok(())
    }

    pub fn listen_addr(&self) -> SocketAddr {
        self.listen.parse().expect("validado em Config::validate")
    }

    pub fn auth_path(&self) -> PathBuf {
        self.data_dir.join("auth.json")
    }

    pub fn snapshots_dir(&self) -> PathBuf {
        self.data_dir.join("snapshots")
    }
}

/// Mensagem de erro de I/O do host sem o sufixo " (os error N)" do `Display` do std.
pub fn io_msg(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(n) => sysabi::Errno(n).message().to_string(),
        None => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_sizes() {
        assert_eq!(ByteSize::parse("512MiB").unwrap(), ByteSize::mib(512));
        assert_eq!(ByteSize::parse("1G").unwrap(), ByteSize::gib(1));
        assert_eq!(ByteSize::parse("10MB").unwrap(), ByteSize(10_000_000));
        assert_eq!(ByteSize::parse("4096").unwrap(), ByteSize(4096));
        assert!(ByteSize::parse("1PiB").is_err());
        assert_eq!(ByteSize::mib(256).to_string(), "256MiB");
        assert_eq!(ByteSize(1000).to_string(), "1000");
    }

    #[test]
    fn toml_with_sizes_and_env() {
        let mut cfg: Config = toml::from_str(
            r#"
            listen = "0.0.0.0:9000"
            workers = 3
            [quota]
            mem_bytes = "2GiB"
            cpu_max = { quota_us = 50000, period_us = 100000 }
            [sandbox]
            mem_bytes = 134217728
            "#,
        )
        .unwrap();
        assert_eq!(cfg.quota.mem_bytes, ByteSize::gib(2));
        assert_eq!(cfg.sandbox.mem_bytes, ByteSize::mib(128));
        assert_eq!(cfg.quota.cpu_max, Some(CpuMax { quota_us: 50_000, period_us: 100_000 }));
        cfg.apply_env(|k| (k == "PL_WORKERS").then(|| "1".to_string())).unwrap();
        assert_eq!(cfg.workers, 1);
        cfg.validate().unwrap();
        assert!(toml::from_str::<Config>("nope = 1").is_err(), "chave desconhecida é erro");
    }

    #[test]
    fn quota_override_semantics() {
        let base = Quota::default();
        let mut o = QuotaOverride { max_sandboxes: Some(10), ..Default::default() };
        assert_eq!(o.apply(&base).max_sandboxes, 10);
        assert_eq!(o.apply(&base).cpu_max, base.cpu_max);
        o.merge(&QuotaOverride { cpu_max: Some(None), ..Default::default() });
        assert_eq!(o.apply(&base).cpu_max, None);
        assert_eq!(o.max_sandboxes, Some(10));
        let json = serde_json::to_string(&o).unwrap();
        let back: QuotaOverride = serde_json::from_str(&json).unwrap();
        assert_eq!(back.cpu_max, Some(None), "{json}");
        let empty: QuotaOverride = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.cpu_max, None);
    }

    #[test]
    fn cpu_max_bounds() {
        assert!(CpuMax { quota_us: 500, period_us: 100_000 }.validate().is_err());
        assert!(CpuMax { quota_us: 1000, period_us: 2_000_000 }.validate().is_err());
        assert!(CpuMax { quota_us: 200_000, period_us: 100_000 }.validate().is_ok());
    }
}
