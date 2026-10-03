//! Modos do subprocesso. O processo principal roda `<exe> child <modo>` e lê um JSON do stdout.
//! Só aqui dentro alguma thread recebe Landlock ou seccomp; o processo principal e o shell nunca.
//!
//! Modos:
//! - `landlock`: thread restrita, thread filha dela, vizinhas e a thread principal.
//! - `seccomp`: mesma ideia com a lista de negação de syscalls.
//! - `seccomp-spawn`: sequência curta pra rodar sob `strace` e mostrar o caminho clone3 -> clone.
//! - `bench [full|quick]`: custo por syscall sem filtro, com Landlock, com seccomp e com os dois.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs::{self, File};
use std::hint::black_box;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use rustix::fs::{Mode, OFlags};
use rustix::net::{AddressFamily, SocketType};
use serde::{Deserialize, Serialize};

use crate::layout;
use crate::probe::{CallResult, Expect, Probe, Role};
use crate::sandbox::{self, LandlockApplied, SeccompProfile};

/// Arquivo do host fora do scratch, só lido: mostra que o FS real do host some pra thread restrita.
pub const HOST_FILE: &str = "/etc/os-release";

pub fn main(args: &[String]) -> Result<()> {
    let mode = args.first().map(String::as_str).unwrap_or("");
    let json = match mode {
        "landlock" => serde_json::to_string(&landlock_mode()?)?,
        "seccomp" => serde_json::to_string(&seccomp_mode()?)?,
        "seccomp-spawn" => serde_json::to_string(&seccomp_spawn_mode()?)?,
        "bench" => {
            let scale = BenchScale::parse(args.get(1).map(String::as_str).unwrap_or("full"))?;
            serde_json::to_string(&bench_mode(scale)?)?
        }
        other => bail!("modo de subprocesso desconhecido: {other:?}"),
    };
    let mut out = io::stdout().lock();
    out.write_all(json.as_bytes())?;
    out.write_all(b"\n")?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Utilitários

fn reset_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir).with_context(|| format!("limpar {}", dir.display()))?;
    }
    fs::create_dir_all(dir).with_context(|| format!("criar {}", dir.display()))
}

fn shown(path: &Path) -> String {
    path.display().to_string()
}

fn read_file(path: &Path) -> CallResult {
    CallResult::from_io(&fs::read(path), |b| format!("{} bytes", b.len()))
}

fn create_file(path: &Path) -> CallResult {
    CallResult::from_io(&fs::write(path, b"e06\n"), |_| String::new())
}

fn list_dir(path: &Path) -> CallResult {
    CallResult::from_io(&fs::read_dir(path).map(Iterator::count), |n| format!("{n} entradas"))
}

fn spawn_process() -> CallResult {
    let status = Command::new("/bin/true").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    match status {
        Ok(s) if s.success() => CallResult::ok("exit 0"),
        Ok(s) => CallResult::failed(format!("{s}")),
        Err(e) => CallResult::from_io::<()>(&Err(e), |_| String::new()),
    }
}

/// Uma thread nova que só termina; devolve o resultado do spawn.
fn spawn_thread() -> CallResult {
    match thread::Builder::new().spawn(|| ()) {
        Ok(h) => match h.join() {
            Ok(()) => CallResult::ok(""),
            Err(_) => CallResult::failed("pânico"),
        },
        Err(e) => CallResult::from_io::<()>(&Err(e), |_| String::new()),
    }
}

/// Thread criada antes da restrição que executa tarefas de outras threads: o modelo de qualquer pool
/// global (rayon, tokio) iniciado fora do pseudo-processo.
struct PoolWorker {
    tx: Option<mpsc::Sender<Box<dyn FnOnce() + Send>>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl PoolWorker {
    fn spawn() -> PoolWorker {
        let (tx, rx) = mpsc::channel::<Box<dyn FnOnce() + Send>>();
        let handle = thread::spawn(move || {
            for job in rx {
                job();
            }
        });
        PoolWorker { tx: Some(tx), handle: Some(handle) }
    }

    fn run<R: Send + 'static>(&self, f: impl FnOnce() -> R + Send + 'static) -> Option<R> {
        let (rtx, rrx) = mpsc::channel();
        self.tx
            .as_ref()?
            .send(Box::new(move || {
                let _ = rtx.send(f());
            }))
            .ok()?;
        rrx.recv().ok()
    }
}

impl Drop for PoolWorker {
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Landlock

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TsyncProbe {
    /// O crate aceitou `all_threads(true)` com `HardRequirement` (ABI >= 8).
    pub accepted: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LandlockReport {
    pub mounted_dir: String,
    pub applied: Option<LandlockApplied>,
    pub restrict_error: Option<String>,
    pub tsync_hard_requirement: TsyncProbe,
    pub probes: Vec<Probe>,
}

#[derive(Clone)]
struct LandlockFixture {
    mounted: PathBuf,
    inside_file: PathBuf,
    outside: PathBuf,
    outside_file: PathBuf,
    host_file: PathBuf,
}

pub fn landlock_mode() -> Result<LandlockReport> {
    let base = layout::scratch().join("landlock");
    reset_dir(&base)?;
    let fx = LandlockFixture {
        mounted: base.join("mounted"),
        inside_file: base.join("mounted").join("inside.txt"),
        outside: base.join("outside"),
        outside_file: base.join("outside").join("outside.txt"),
        host_file: PathBuf::from(HOST_FILE),
    };
    fs::create_dir_all(&fx.mounted)?;
    fs::create_dir_all(&fx.outside)?;
    fs::write(&fx.inside_file, b"inside\n")?;
    fs::write(&fx.outside_file, b"outside\n")?;
    // Aberto pelo processo antes de qualquer restrição: o Landlock só decide no open.
    let preopened = File::open(&fx.outside_file)?;
    let pool = PoolWorker::spawn();

    let (applied, mut probes) = thread::scope(|s| s.spawn(|| restricted_landlock(&fx, &preopened, &pool)).join())
        .map_err(|_| anyhow!("pânico na thread restrita por Landlock"))?;

    // Vizinha criada depois da restrição, pela thread principal (que nunca foi restrita).
    let neighbor = {
        let fx = fx.clone();
        thread::spawn(move || {
            let n = "neighbor_spawned_after";
            vec![
                Probe::new(n, "read_file", shown(&fx.outside_file), Role::Criterion, Expect::Allowed, read_file(&fx.outside_file)),
                Probe::new(n, "create_file", shown(&fx.outside.join("by-neighbor.txt")), Role::Criterion, Expect::Allowed, create_file(&fx.outside.join("by-neighbor.txt"))),
                Probe::new(n, "read_file", shown(&fx.host_file), Role::Criterion, Expect::Allowed, read_file(&fx.host_file)),
                Probe::new(n, "list_dir", "/", Role::Criterion, Expect::Allowed, list_dir(Path::new("/"))),
                Probe::new(n, "read_file", "/proc/self/status", Role::Criterion, Expect::Allowed, read_file(Path::new("/proc/self/status"))),
            ]
        })
        .join()
        .map_err(|_| anyhow!("pânico na vizinha"))?
    };
    probes.extend(neighbor);
    probes.push(Probe::new("main", "read_file", shown(&fx.outside_file), Role::Criterion, Expect::Allowed, read_file(&fx.outside_file)));
    probes.push(Probe::new("main", "read_file", shown(&fx.host_file), Role::Criterion, Expect::Allowed, read_file(&fx.host_file)));

    let tsync = match sandbox::tsync_hard_requirement() {
        Ok(()) => TsyncProbe { accepted: true, error: None },
        Err(e) => TsyncProbe { accepted: false, error: Some(e) },
    };
    drop(pool);
    let (applied, restrict_error) = match applied {
        Ok(a) => (Some(a), None),
        Err(e) => (None, Some(e)),
    };
    Ok(LandlockReport { mounted_dir: shown(&fx.mounted), applied, restrict_error, tsync_hard_requirement: tsync, probes })
}

fn restricted_landlock(
    fx: &LandlockFixture,
    preopened: &File,
    pool: &PoolWorker,
) -> (std::result::Result<LandlockApplied, String>, Vec<Probe>) {
    let applied = sandbox::landlock_ruleset(&[&fx.mounted])
        .and_then(sandbox::restrict_current_thread)
        .map_err(|e| e.to_string());
    let mut probes = Vec::new();
    if applied.is_err() {
        return (applied, probes);
    }
    let eacces = Expect::Errno(libc::EACCES);
    let t = "restricted";
    let created_inside = fx.mounted.join("by-restricted.txt");
    let created_outside = fx.outside.join("by-restricted.txt");
    probes.push(Probe::new(t, "read_file", shown(&fx.inside_file), Role::Criterion, Expect::Allowed, read_file(&fx.inside_file)));
    probes.push(Probe::new(t, "create_file", shown(&created_inside), Role::Criterion, Expect::Allowed, create_file(&created_inside)));
    probes.push(Probe::new(t, "list_dir", shown(&fx.mounted), Role::Criterion, Expect::Allowed, list_dir(&fx.mounted)));
    probes.push(Probe::new(t, "read_file", shown(&fx.outside_file), Role::Criterion, eacces, read_file(&fx.outside_file)));
    probes.push(Probe::new(t, "create_file", shown(&created_outside), Role::Criterion, eacces, create_file(&created_outside)));
    probes.push(Probe::new(t, "list_dir", shown(&fx.outside), Role::Criterion, eacces, list_dir(&fx.outside)));
    probes.push(Probe::new(t, "read_file", shown(&fx.host_file), Role::Criterion, eacces, read_file(&fx.host_file)));
    probes.push(Probe::new(t, "list_dir", "/", Role::Criterion, eacces, list_dir(Path::new("/"))));
    probes.push(Probe::new(t, "read_file", "/proc/self/status", Role::Finding, eacces, read_file(Path::new("/proc/self/status"))));

    // O fd aberto antes continua utilizável: o Landlock não revoga fd existente.
    let mut buf = Vec::new();
    let mut reader: &File = preopened;
    let read_pre = reader.read_to_end(&mut buf);
    probes.push(Probe::new(t, "read_preopened_fd", shown(&fx.outside_file), Role::Finding, Expect::Allowed, CallResult::from_io(&read_pre, |n| format!("{n} bytes"))));
    // Reabrir o mesmo arquivo pelo link mágico do /proc passa pelo Landlock de novo.
    let proc_fd = PathBuf::from(format!("/proc/self/fd/{}", preopened.as_raw_fd()));
    probes.push(Probe::new(t, "reopen_via_proc_fd", shown(&proc_fd), Role::Finding, eacces, read_file(&proc_fd)));

    // Thread filha: herda o domínio.
    let child_fx = fx.clone();
    let child = thread::Builder::new().spawn(move || {
        let c = "child_of_restricted";
        let created = child_fx.mounted.join("by-child.txt");
        vec![
            Probe::new(c, "read_file", shown(&child_fx.inside_file), Role::Criterion, Expect::Allowed, read_file(&child_fx.inside_file)),
            Probe::new(c, "create_file", shown(&created), Role::Criterion, Expect::Allowed, create_file(&created)),
            Probe::new(c, "read_file", shown(&child_fx.outside_file), Role::Criterion, Expect::Errno(libc::EACCES), read_file(&child_fx.outside_file)),
            Probe::new(c, "read_file", shown(&child_fx.host_file), Role::Criterion, Expect::Errno(libc::EACCES), read_file(&child_fx.host_file)),
        ]
    });
    match child.map(|h| h.join()) {
        Ok(Ok(v)) => probes.extend(v),
        Ok(Err(_)) => probes.push(Probe::new("child_of_restricted", "spawn_thread", "", Role::Criterion, Expect::Allowed, CallResult::failed("pânico"))),
        Err(e) => probes.push(Probe::new("child_of_restricted", "spawn_thread", "", Role::Criterion, Expect::Allowed, CallResult::from_io::<()>(&Err(e), |_| String::new()))),
    }

    // Vizinha que já existia (pool criado antes): não é afetada, e por isso faz I/O de host a pedido
    // da thread restrita.
    let outside_file = fx.outside_file.clone();
    let via_pool = pool.run(move || read_file(&outside_file)).unwrap_or_else(|| CallResult::failed("pool fechado"));
    probes.push(Probe::new("pool_worker_created_before", "read_file_for_restricted_thread", shown(&fx.outside_file), Role::Criterion, Expect::Allowed, via_pool));
    (applied, probes)
}

// ---------------------------------------------------------------------------------------------
// seccomp

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SeccompReport {
    pub denied: Vec<(String, String)>,
    pub deny_instructions: usize,
    pub clone3_instructions: usize,
    pub apply_error: Option<String>,
    pub probes: Vec<Probe>,
}

struct SeccompFixture {
    addr: SocketAddr,
    dir: File,
    data_file: PathBuf,
}

const MISSING_EXEC: &str = "/nonexistent-e06/probe";
const MISSING_EXEC_REL: &str = "nonexistent-e06-probe";

/// Bateria de syscalls. `filtered` diz se a thread está sob o filtro: aí a expectativa é EPERM; fora
/// dele, `execve` de um caminho inexistente chega ao kernel e volta ENOENT (prova que não foi o filtro).
fn syscall_battery(thread_name: &str, filtered: bool, fx: &SeccompFixture, presock: Option<&OwnedFd>) -> Vec<Probe> {
    let expect = |unfiltered: Expect| if filtered { Expect::Errno(libc::EPERM) } else { unfiltered };
    let t = thread_name;
    let addr = fx.addr;
    let mut v = Vec::new();
    v.push(Probe::new(t, "socket", "AF_INET SOCK_STREAM", Role::Criterion, expect(Expect::Allowed), CallResult::from_rustix(&rustix::net::socket(AddressFamily::INET, SocketType::STREAM, None))));
    v.push(Probe::new(t, "tcp_connect", addr.to_string(), Role::Criterion, expect(Expect::Allowed), CallResult::from_io(&TcpStream::connect(addr), |_| String::new())));
    if let Some(sock) = presock {
        v.push(Probe::new(t, "connect_existing_socket", addr.to_string(), Role::Criterion, expect(Expect::Allowed), CallResult::from_rustix(&rustix::net::connect(sock, &addr))));
    }
    let path = CString::new(MISSING_EXEC).expect("sem NUL");
    let argv = [path.as_c_str()];
    let envp: [&std::ffi::CStr; 0] = [];
    let execve = match nix::unistd::execve(&path, &argv, &envp) {
        Ok(never) => match never {},
        Err(e) => CallResult::errno(e as i32),
    };
    v.push(Probe::new(t, "execve", MISSING_EXEC, Role::Criterion, expect(Expect::Errno(libc::ENOENT)), execve));
    let rel = CString::new(MISSING_EXEC_REL).expect("sem NUL");
    let execveat = match nix::unistd::execveat(&fx.dir, &rel, &argv, &envp, nix::fcntl::AtFlags::empty()) {
        Ok(never) => match never {},
        Err(e) => CallResult::errno(e as i32),
    };
    v.push(Probe::new(t, "execveat", MISSING_EXEC_REL, Role::Criterion, expect(Expect::Errno(libc::ENOENT)), execveat));
    v.push(Probe::new(t, "spawn_process", "/bin/true", Role::Criterion, expect(Expect::Allowed), spawn_process()));
    v.push(Probe::new(t, "io_uring_setup", "IoUring::new(4)", Role::Criterion, expect(Expect::Allowed), CallResult::from_io(&io_uring::IoUring::new(4), |_| String::new())));
    v.push(Probe::new(t, "read_file", shown(&fx.data_file), Role::Control, Expect::Allowed, read_file(&fx.data_file)));
    v
}

pub fn seccomp_mode() -> Result<SeccompReport> {
    let base = layout::scratch().join("seccomp");
    reset_dir(&base)?;
    let data_file = base.join("data.txt");
    fs::write(&data_file, b"data\n")?;
    let profile = sandbox::seccomp_profile()?;
    // Um connect num socket em escuta completa pelo backlog, sem accept.
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let fx = SeccompFixture { addr: listener.local_addr()?, dir: File::open(&base)?, data_file };
    // Sockets criados antes da restrição: separam o teste do connect do teste do socket.
    let presock_restricted = rustix::net::socket(AddressFamily::INET, SocketType::STREAM, None)?;
    let presock_neighbor = rustix::net::socket(AddressFamily::INET, SocketType::STREAM, None)?;

    let (apply_error, mut probes) =
        thread::scope(|s| s.spawn(|| restricted_seccomp(&profile, &fx, &presock_restricted)).join())
            .map_err(|_| anyhow!("pânico na thread com seccomp"))?;
    let neighbor = thread::scope(|s| s.spawn(|| syscall_battery("neighbor_spawned_after", false, &fx, Some(&presock_neighbor))).join())
        .map_err(|_| anyhow!("pânico na vizinha"))?;
    probes.extend(neighbor);
    probes.push(Probe::new("main", "socket", "AF_INET SOCK_STREAM", Role::Criterion, Expect::Allowed, CallResult::from_rustix(&rustix::net::socket(AddressFamily::INET, SocketType::STREAM, None))));
    probes.push(Probe::new("main", "spawn_process", "/bin/true", Role::Criterion, Expect::Allowed, spawn_process()));
    drop(listener);
    Ok(SeccompReport {
        denied: sandbox::denied_syscalls().into_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        deny_instructions: profile.deny.len(),
        clone3_instructions: profile.clone3_enosys.len(),
        apply_error,
        probes,
    })
}

fn restricted_seccomp(profile: &SeccompProfile, fx: &SeccompFixture, presock: &OwnedFd) -> (Option<String>, Vec<Probe>) {
    if let Err(e) = sandbox::apply_seccomp(profile) {
        return (Some(format!("{e:#}")), Vec::new());
    }
    let mut probes = syscall_battery("restricted", true, fx, Some(presock));
    // Criar thread continua permitido (clone com CLONE_THREAD), e a filha herda o filtro.
    let child = thread::scope(|s| match thread::Builder::new().spawn_scoped(s, || syscall_battery("child_of_restricted", true, fx, None)) {
        Ok(h) => h.join().map_err(|_| CallResult::failed("pânico")),
        Err(e) => Err(CallResult::from_io::<()>(&Err(e), |_| String::new())),
    });
    match child {
        Ok(v) => {
            probes.push(Probe::new("restricted", "spawn_thread", "", Role::Criterion, Expect::Allowed, CallResult::ok("")));
            probes.extend(v);
        }
        Err(r) => probes.push(Probe::new("restricted", "spawn_thread", "", Role::Criterion, Expect::Allowed, r)),
    }
    (None, probes)
}

// ---------------------------------------------------------------------------------------------
// seccomp sob strace

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpawnReport {
    pub probes: Vec<Probe>,
}

/// Marca de fase visível no strace (um `statx` num caminho que não existe).
fn marker(name: &str) {
    let _ = fs::metadata(format!("/nonexistent-e06-marker/{name}"));
}

pub fn seccomp_spawn_mode() -> Result<SpawnReport> {
    let profile = sandbox::seccomp_profile()?;
    let mut probes = Vec::new();
    marker("main_before");
    probes.push(Probe::new("main_before", "spawn_thread", "", Role::Control, Expect::Allowed, spawn_thread()));
    probes.push(Probe::new("main_before", "spawn_process", "/bin/true", Role::Control, Expect::Allowed, spawn_process()));
    let restricted = thread::scope(|s| {
        s.spawn(|| {
            if let Err(e) = sandbox::apply_seccomp(&profile) {
                return vec![Probe::new("restricted", "apply_seccomp", "", Role::Control, Expect::Allowed, CallResult::failed(format!("{e:#}")))];
            }
            marker("restricted");
            let v = vec![
                Probe::new("restricted", "spawn_process", "/bin/true", Role::Criterion, Expect::Errno(libc::EPERM), spawn_process()),
                Probe::new("restricted", "spawn_thread", "", Role::Criterion, Expect::Allowed, spawn_thread()),
            ];
            marker("restricted_end");
            v
        })
        .join()
    })
    .map_err(|_| anyhow!("pânico na thread com seccomp"))?;
    probes.extend(restricted);
    marker("main_after");
    probes.push(Probe::new("main_after", "spawn_thread", "", Role::Control, Expect::Allowed, spawn_thread()));
    probes.push(Probe::new("main_after", "spawn_process", "/bin/true", Role::Control, Expect::Allowed, spawn_process()));
    marker("end");
    Ok(SpawnReport { probes })
}

// ---------------------------------------------------------------------------------------------
// Medição

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BenchScale {
    pub rounds: usize,
    pub null_iters: u64,
    pub read_iters: u64,
    pub open_iters: u64,
    pub spawn_iters: usize,
}

impl BenchScale {
    pub const FULL: BenchScale =
        BenchScale { rounds: 15, null_iters: 1_000_000, read_iters: 500_000, open_iters: 200_000, spawn_iters: 400 };
    pub const QUICK: BenchScale =
        BenchScale { rounds: 2, null_iters: 20_000, read_iters: 20_000, open_iters: 5_000, spawn_iters: 20 };

    pub fn parse(s: &str) -> Result<BenchScale> {
        match s {
            "full" => Ok(BenchScale::FULL),
            "quick" => Ok(BenchScale::QUICK),
            other => bail!("escala de medição desconhecida: {other:?}"),
        }
    }
}

/// Configurações medidas. `SeccompArgChecked` é a lista de negação mais um filtro que obriga o BPF a
/// rodar nas syscalls do laço (sem o cache de ação constante do kernel).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchConfig {
    None,
    Landlock,
    Seccomp,
    LandlockSeccomp,
    SeccompArgChecked,
}

pub const BENCH_CONFIGS: [BenchConfig; 5] =
    [BenchConfig::None, BenchConfig::Landlock, BenchConfig::Seccomp, BenchConfig::LandlockSeccomp, BenchConfig::SeccompArgChecked];

impl BenchConfig {
    fn landlock(self) -> bool {
        matches!(self, BenchConfig::Landlock | BenchConfig::LandlockSeccomp)
    }

    fn seccomp(self) -> bool {
        matches!(self, BenchConfig::Seccomp | BenchConfig::LandlockSeccomp | BenchConfig::SeccompArgChecked)
    }

    pub fn name(self) -> &'static str {
        match self {
            BenchConfig::None => "none",
            BenchConfig::Landlock => "landlock",
            BenchConfig::Seccomp => "seccomp",
            BenchConfig::LandlockSeccomp => "landlock_seccomp",
            BenchConfig::SeccompArgChecked => "seccomp_arg_checked",
        }
    }
}

/// Mediana, mínimo e máximo de uma série.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stat {
    pub median: f64,
    pub min: f64,
    pub max: f64,
    pub n: usize,
}

impl Stat {
    pub fn of(mut v: Vec<f64>) -> Stat {
        v.sort_by(f64::total_cmp);
        let n = v.len();
        if n == 0 {
            return Stat { median: f64::NAN, min: f64::NAN, max: f64::NAN, n };
        }
        let median = if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 };
        Stat { median, min: v[0], max: v[n - 1], n }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigStats {
    pub config: BenchConfig,
    /// `getppid`: syscall quase vazia, mede o custo fixo de entrar no kernel (e do BPF).
    pub getppid_ns: Stat,
    /// `pread` de 1 byte num arquivo dentro do diretório montado (offset 0, sempre 1 syscall).
    pub pread_1b_ns: Stat,
    /// `openat` + `close` do mesmo arquivo, caminho absoluto.
    pub open_close_ns: Stat,
    /// Criar thread, aplicar a configuração dentro dela e dar join.
    pub spawn_restrict_join_us: Stat,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BenchReport {
    pub scale: BenchScale,
    pub open_path: String,
    pub open_path_components: usize,
    pub configs: Vec<ConfigStats>,
    /// Montar um ruleset com um diretório (sem aplicar): custo por sandbox com montagens próprias.
    pub ruleset_build_us: Stat,
    /// Criar e dar join numa thread a partir de uma thread que já tem Landlock e seccomp: a filha
    /// herda os dois domínios sem reaplicar nada (desenho de uma thread "spawner" por sandbox).
    pub inherited_spawn_us: Stat,
    /// A filha da spawner estava mesmo restrita (EACCES fora do diretório, EPERM no socket).
    pub inherited_sanity: Vec<Probe>,
    /// Conferência de que cada configuração estava mesmo ativa durante a medição.
    pub sanity: Vec<Probe>,
}

struct BenchEnv {
    ruleset: landlock::RulesetCreated,
    seccomp: SeccompProfile,
    arg_checked: seccompiler::BpfProgram,
    file: CString,
    outside_file: PathBuf,
}

fn apply_config(config: BenchConfig, env: &BenchEnv) -> Result<()> {
    if config.landlock() {
        let rs = env.ruleset.try_clone().context("dup do ruleset")?;
        let applied = sandbox::restrict_current_thread(rs)?;
        if applied.ruleset != "fully_enforced" {
            bail!("Landlock não ficou completo: {}", applied.ruleset);
        }
    }
    if config.seccomp() {
        sandbox::apply_seccomp(&env.seccomp)?;
    }
    if config == BenchConfig::SeccompArgChecked {
        seccompiler::apply_filter(&env.arg_checked).context("filtro de medição")?;
    }
    Ok(())
}

/// Amostras de uma configuração, uma por rodada.
#[derive(Default)]
struct Series {
    getppid_ns: Vec<f64>,
    pread_1b_ns: Vec<f64>,
    open_close_ns: Vec<f64>,
}

struct RoundSample {
    getppid_ns: f64,
    pread_1b_ns: f64,
    open_close_ns: f64,
    sanity: Vec<Probe>,
}

fn measure(config: BenchConfig, env: &BenchEnv, scale: BenchScale, check: bool) -> Result<RoundSample> {
    apply_config(config, env)?;
    let mut sanity = Vec::new();
    if check {
        let t = config.name();
        let ll = if config.landlock() { Expect::Errno(libc::EACCES) } else { Expect::Allowed };
        let sc = if config.seccomp() { Expect::Errno(libc::EPERM) } else { Expect::Allowed };
        sanity.push(Probe::new(t, "read_file", shown(&env.outside_file), Role::Control, ll, read_file(&env.outside_file)));
        sanity.push(Probe::new(t, "socket", "AF_INET SOCK_STREAM", Role::Control, sc, CallResult::from_rustix(&rustix::net::socket(AddressFamily::INET, SocketType::STREAM, None))));
    }

    let start = Instant::now();
    for _ in 0..scale.null_iters {
        black_box(rustix::process::getppid());
    }
    let getppid_ns = start.elapsed().as_nanos() as f64 / scale.null_iters as f64;

    let fd = rustix::fs::open(env.file.as_c_str(), OFlags::RDONLY | OFlags::CLOEXEC, Mode::empty())?;
    let mut byte = [0u8; 1];
    let start = Instant::now();
    for _ in 0..scale.read_iters {
        let n = rustix::io::pread(&fd, &mut byte[..], 0)?;
        black_box(n);
    }
    let pread_1b_ns = start.elapsed().as_nanos() as f64 / scale.read_iters as f64;
    drop(fd);

    let start = Instant::now();
    for _ in 0..scale.open_iters {
        let fd = rustix::fs::open(env.file.as_c_str(), OFlags::RDONLY | OFlags::CLOEXEC, Mode::empty())?;
        drop(black_box(fd));
    }
    let open_close_ns = start.elapsed().as_nanos() as f64 / scale.open_iters as f64;
    Ok(RoundSample { getppid_ns, pread_1b_ns, open_close_ns, sanity })
}

pub fn bench_mode(scale: BenchScale) -> Result<BenchReport> {
    let base = layout::scratch().join("bench");
    reset_dir(&base)?;
    let mounted = base.join("mounted");
    let outside = base.join("outside");
    fs::create_dir_all(&mounted)?;
    fs::create_dir_all(&outside)?;
    let file = mounted.join("file.bin");
    fs::write(&file, vec![0x5a_u8; 4096])?;
    let outside_file = outside.join("outside.bin");
    fs::write(&outside_file, b"x")?;

    // Ruleset montado uma vez; cada thread aplica um dup dele (como o kernel do pseudo-linus faria).
    let env = BenchEnv {
        ruleset: sandbox::landlock_ruleset(&[&mounted])?,
        seccomp: sandbox::seccomp_profile()?,
        arg_checked: sandbox::arg_checked_hot_syscalls()?,
        file: CString::new(file.as_os_str().as_bytes())?,
        outside_file,
    };

    let mut series: BTreeMap<BenchConfig, Series> = BTreeMap::new();
    let mut sanity = Vec::new();
    for round in 0..scale.rounds {
        // Ordem girada a cada rodada, pra ruído de máquina compartilhada não cair sempre na mesma config.
        for i in 0..BENCH_CONFIGS.len() {
            let config = BENCH_CONFIGS[(i + round) % BENCH_CONFIGS.len()];
            let sample = thread::scope(|s| s.spawn(|| measure(config, &env, scale, round == 0)).join())
                .map_err(|_| anyhow!("pânico medindo {}", config.name()))??;
            sanity.extend(sample.sanity);
            let e = series.entry(config).or_default();
            e.getppid_ns.push(sample.getppid_ns);
            e.pread_1b_ns.push(sample.pread_1b_ns);
            e.open_close_ns.push(sample.open_close_ns);
        }
    }

    let mut configs = Vec::new();
    for config in BENCH_CONFIGS {
        let mut spawn = Vec::with_capacity(scale.spawn_iters);
        for _ in 0..scale.spawn_iters {
            let start = Instant::now();
            thread::scope(|s| s.spawn(|| apply_config(config, &env)).join())
                .map_err(|_| anyhow!("pânico no spawn de {}", config.name()))??;
            spawn.push(start.elapsed().as_nanos() as f64 / 1000.0);
        }
        let s = series.remove(&config).unwrap_or_default();
        configs.push(ConfigStats {
            config,
            getppid_ns: Stat::of(s.getppid_ns),
            pread_1b_ns: Stat::of(s.pread_1b_ns),
            open_close_ns: Stat::of(s.open_close_ns),
            spawn_restrict_join_us: Stat::of(spawn),
        });
    }

    let mut build = Vec::with_capacity(scale.spawn_iters);
    for _ in 0..scale.spawn_iters {
        let start = Instant::now();
        drop(black_box(sandbox::landlock_ruleset(&[&mounted])?));
        build.push(start.elapsed().as_nanos() as f64 / 1000.0);
    }

    let (inherited, inherited_sanity) = thread::scope(|s| s.spawn(|| inherited_spawns(&env, scale)).join())
        .map_err(|_| anyhow!("pânico na thread spawner"))??;

    Ok(BenchReport {
        scale,
        open_path: shown(&file),
        open_path_components: file.components().count(),
        configs,
        ruleset_build_us: Stat::of(build),
        inherited_spawn_us: Stat::of(inherited),
        inherited_sanity,
        sanity,
    })
}

/// Roda numa thread que aplica Landlock e seccomp uma vez e depois só cria filhas.
fn inherited_spawns(env: &BenchEnv, scale: BenchScale) -> Result<(Vec<f64>, Vec<Probe>)> {
    apply_config(BenchConfig::LandlockSeccomp, env)?;
    let outside = env.outside_file.clone();
    let sanity = thread::spawn(move || {
        let t = "child_of_spawner";
        vec![
            Probe::new(t, "read_file", shown(&outside), Role::Control, Expect::Errno(libc::EACCES), read_file(&outside)),
            Probe::new(t, "socket", "AF_INET SOCK_STREAM", Role::Control, Expect::Errno(libc::EPERM), CallResult::from_rustix(&rustix::net::socket(AddressFamily::INET, SocketType::STREAM, None))),
        ]
    })
    .join()
    .map_err(|_| anyhow!("pânico na filha da spawner"))?;
    let mut samples = Vec::with_capacity(scale.spawn_iters);
    for _ in 0..scale.spawn_iters {
        let start = Instant::now();
        thread::spawn(|| black_box(())).join().map_err(|_| anyhow!("pânico na filha da spawner"))?;
        samples.push(start.elapsed().as_nanos() as f64 / 1000.0);
    }
    Ok((samples, sanity))
}
