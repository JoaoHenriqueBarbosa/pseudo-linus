//! Cgroups v2 reais pelo systemd do usuário, pros diferenciais H41 e H42.
//!
//! Cada grupo folha do cenário é um *scope* transitório (`systemd-run --user --scope`) com
//! `CPUWeight` e, se houver, `CPUQuota` (o `cpu.max`, com o período padrão de 100 ms); o processo do
//! scope é este mesmo binário em modo `hog`, com N threads em laço fixadas nas CPUs do cenário. Grupos
//! intermediários (usuário, na hierarquia usuário > sandbox > processo) são *slices* aninhadas pelo nome
//! (`<prefixo>-u1.slice` é filha de `<prefixo>.slice`), com o peso posto por
//! `systemctl --user set-property --runtime`. O controlador `cpu` precisa estar delegado ao usuário;
//! o `cpuset` não está, por isso a fixação de CPU é feita dentro do processo (`sched_setaffinity`).
//!
//! Limpeza: [`Session`] guarda tudo o que criou e, no fim (ou no `Drop`, inclusive em pânico), para a
//! slice raiz da sessão (o que derruba todos os scopes dentro), remove os drop-ins de runtime das
//! slices (`systemctl --user revert`) e confere que nenhuma unidade da sessão sobrou.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;

/// `cpu.stat` de um cgroup.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct CpuStat {
    pub usage_usec: u64,
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub throttled_usec: u64,
}

/// Lê `cpu.stat`.
pub fn read_cpu_stat(cgroup: &Path) -> Result<CpuStat> {
    let path = cgroup.join("cpu.stat");
    let text = std::fs::read_to_string(&path).with_context(|| format!("ler {}", path.display()))?;
    let mut s = CpuStat::default();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(k), Some(v)) = (it.next(), it.next()) else { continue };
        let v: u64 = v.parse().unwrap_or(0);
        match k {
            "usage_usec" => s.usage_usec = v,
            "nr_periods" => s.nr_periods = v,
            "nr_throttled" => s.nr_throttled = v,
            "throttled_usec" => s.throttled_usec = v,
            _ => {}
        }
    }
    Ok(s)
}

/// Só o `usage_usec`, lido sem alocar o mapa inteiro (usado no laço de amostragem de 1 ms).
pub fn read_usage_usec(cgroup_stat: &Path) -> Result<u64> {
    let text = std::fs::read_to_string(cgroup_stat)?;
    let line = text.lines().next().ok_or_else(|| anyhow!("cpu.stat vazio"))?;
    let v = line.strip_prefix("usage_usec ").ok_or_else(|| anyhow!("cpu.stat sem usage_usec na primeira linha"))?;
    Ok(v.trim().parse()?)
}

/// O que subir num scope.
#[derive(Clone, Copy, Debug)]
pub struct HogSpec<'a> {
    pub name: &'a str,
    /// Slice onde o scope entra.
    pub slice: &'a str,
    /// `CPUWeight`.
    pub weight: u64,
    /// `CPUQuota` em porcentagem de uma CPU.
    pub quota_pct: Option<u32>,
    pub threads: usize,
    pub cpus: &'a [usize],
    pub seconds: f64,
}

/// Um processo `hog` rodando num scope.
#[derive(Debug)]
pub struct Hog {
    pub name: String,
    pub unit: String,
    pub cgroup: PathBuf,
    child: Child,
}

impl Hog {
    /// `cpu.stat` do scope.
    pub fn cpu_stat(&self) -> Result<CpuStat> {
        read_cpu_stat(&self.cgroup)
    }

    /// Caminho do `cpu.stat`.
    pub fn stat_path(&self) -> PathBuf {
        self.cgroup.join("cpu.stat")
    }

    /// CPU em que cada thread de laço está agora (campo 39, `processor`, de `/proc/<pid>/task/<tid>/stat`:
    /// a CPU da fila da thread, rodando ou esperando).
    pub fn thread_cpus(&self) -> Result<Vec<usize>> {
        let pid = self.child.id();
        let mut out = Vec::new();
        for entry in std::fs::read_dir(format!("/proc/{pid}/task")).context("listar threads do hog")? {
            let entry = entry?;
            if entry.file_name().to_str() == Some(pid.to_string().as_str()) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path().join("stat")) else { continue };
            // Depois do ")" do nome vêm os campos a partir do 3º; o 39º é o índice 36.
            let rest = text.rsplit_once(')').map(|(_, r)| r).unwrap_or("");
            if let Some(cpu) = rest.split_whitespace().nth(36).and_then(|c| c.parse().ok()) {
                out.push(cpu);
            }
        }
        Ok(out)
    }

    /// Migrações das threads de laço até agora: soma do `se.nr_migrations` de
    /// `/proc/<pid>/task/<tid>/sched`, sem a thread principal (que só espera as outras). O
    /// `systemd-run --scope` executa o comando no próprio processo, então o pid do filho é o do hog.
    pub fn migrations(&self) -> Result<u64> {
        let pid = self.child.id();
        let mut total = 0;
        for entry in std::fs::read_dir(format!("/proc/{pid}/task")).context("listar threads do hog")? {
            let entry = entry?;
            if entry.file_name().to_str() == Some(pid.to_string().as_str()) {
                continue;
            }
            let text = std::fs::read_to_string(entry.path().join("sched")).unwrap_or_default();
            if let Some(v) = text.lines().find_map(|l| l.strip_prefix("se.nr_migrations")) {
                total += v.trim_start_matches([' ', ':']).trim().parse::<u64>().unwrap_or(0);
            }
        }
        Ok(total)
    }

    /// Espera o processo terminar.
    pub fn wait(mut self) -> Result<()> {
        let st = self.child.wait().context("esperar hog")?;
        if !st.success() {
            bail!("hog {} terminou com {st}", self.name);
        }
        Ok(())
    }
}

/// Uma sessão de experimento: prefixo único, tudo o que foi criado e a limpeza.
#[derive(Debug)]
pub struct Session {
    prefix: String,
    exe: PathBuf,
    slices: BTreeSet<String>,
    counter: u32,
    cleaned: bool,
}

/// Raiz dos cgroups do gerenciador do usuário.
fn user_manager_cgroup() -> Result<PathBuf> {
    let uid = rustix::process::getuid().as_raw();
    let p = PathBuf::from(format!("/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service"));
    if !p.exists() {
        bail!("{} não existe", p.display());
    }
    Ok(p)
}

fn run(cmd: &mut Command) -> Result<String> {
    let out = cmd.output().with_context(|| format!("executar {cmd:?}"))?;
    if !out.status.success() {
        bail!("{cmd:?} falhou: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

impl Session {
    /// Confere que dá pra criar scopes com o controlador `cpu` e abre uma sessão; os scopes rodam este
    /// mesmo binário em modo `hog`.
    pub fn new() -> Result<Session> {
        Session::with_exe(std::env::current_exe().context("caminho do binário")?)
    }

    /// Como [`Session::new`], com o binário do experimento dado (nos testes, o binário corrente é o de
    /// teste, não o que tem o modo `hog`).
    pub fn with_exe(exe: PathBuf) -> Result<Session> {
        let mgr = user_manager_cgroup()?;
        let ctl = std::fs::read_to_string(mgr.join("cgroup.subtree_control")).context("ler cgroup.subtree_control")?;
        if !ctl.split_whitespace().any(|c| c == "cpu") {
            bail!("o controlador cpu não está delegado ao usuário ({})", ctl.trim());
        }
        run(Command::new("systemd-run").args(["--user", "--version"]))?;
        let prefix = format!("e02r{}", std::process::id());
        Ok(Session { prefix, exe, slices: BTreeSet::new(), counter: 0, cleaned: false })
    }

    /// Slice raiz da sessão.
    pub fn root_slice(&self) -> String {
        format!("{}.slice", self.prefix)
    }

    /// Nome de uma slice filha (`<pai sem .slice>-<nome>.slice`).
    pub fn child_slice(&mut self, parent: &str, name: &str) -> String {
        let base = parent.strip_suffix(".slice").unwrap_or(parent);
        let s = format!("{base}-{name}.slice");
        self.slices.insert(s.clone());
        s
    }

    /// `CPUWeight` de uma slice (vale mesmo antes de ela existir: fica num drop-in de runtime).
    pub fn set_slice_weight(&mut self, slice: &str, weight: u64) -> Result<()> {
        self.slices.insert(slice.to_string());
        run(Command::new("systemctl").args(["--user", "set-property", "--runtime", slice, &format!("CPUWeight={weight}")]))?;
        Ok(())
    }

    /// Sobe um scope com `threads` laços fixados em `cpus`, por `seconds`, e espera ele dizer em que
    /// cgroup está.
    pub fn spawn_hog(&mut self, spec: &HogSpec<'_>) -> Result<Hog> {
        self.counter += 1;
        let name = spec.name;
        let unit = format!("{}-{}-{}", self.prefix, self.counter, name);
        let cpu_list: Vec<String> = spec.cpus.iter().map(|c| c.to_string()).collect();
        let mut cmd = Command::new("systemd-run");
        cmd.args(["--user", "--scope", "--quiet", "--collect"])
            .arg(format!("--unit={unit}"))
            .arg(format!("--slice={}", spec.slice))
            .arg("-p")
            .arg(format!("CPUWeight={}", spec.weight));
        if let Some(q) = spec.quota_pct {
            cmd.arg("-p").arg(format!("CPUQuota={q}%"));
        }
        cmd.arg("--")
            .arg(&self.exe)
            .arg("hog")
            .arg(spec.threads.to_string())
            .arg(cpu_list.join(","))
            .arg(format!("{}", spec.seconds))
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = cmd.spawn().with_context(|| format!("subir scope {unit}"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("sem stdout do hog"))?;
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).context("ler a linha de pronto do hog")?;
        let path = line.trim().strip_prefix("ready ").ok_or_else(|| anyhow!("hog {unit} não ficou pronto: {line:?}"))?;
        let cgroup = PathBuf::from(format!("/sys/fs/cgroup{path}"));
        if !cgroup.join("cpu.stat").exists() {
            bail!("cgroup {} sem cpu.stat", cgroup.display());
        }
        Ok(Hog { name: name.to_string(), unit, cgroup, child })
    }

    /// Para tudo o que a sessão criou e confere que não sobrou unidade.
    pub fn cleanup(&mut self) -> Result<()> {
        if self.cleaned {
            return Ok(());
        }
        self.cleaned = true;
        let root = self.root_slice();
        let _ = Command::new("systemctl").args(["--user", "stop", &root]).output();
        let mut all: Vec<String> = self.slices.iter().cloned().collect();
        all.push(root.clone());
        let _ = Command::new("systemctl").args(["--user", "revert"]).args(&all).output();
        let _ = Command::new("systemctl").args(["--user", "stop", &root]).output();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let out = run(Command::new("systemctl").args([
                "--user",
                "list-units",
                "--all",
                "--no-legend",
                "--plain",
                &format!("{}*", self.prefix),
            ]))?;
            let left: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
            if left.is_empty() {
                break;
            }
            if Instant::now() > deadline {
                bail!("unidades sobraram depois da limpeza: {left:?}");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let mgr = user_manager_cgroup()?;
        if mgr.join(&root).exists() {
            bail!("cgroup {} sobrou", mgr.join(&root).display());
        }
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Err(e) = self.cleanup() {
            eprintln!("limpeza dos cgroups: {e:#}");
        }
    }
}

/// Modo `hog`: imprime o cgroup, fixa `threads` threads nas CPUs dadas e gira por `seconds`.
pub fn hog_main(threads: usize, cpus: &[usize], seconds: f64) -> Result<()> {
    use std::io::Write;
    let cg = std::fs::read_to_string("/proc/self/cgroup").context("ler /proc/self/cgroup")?;
    let path = cg.lines().find_map(|l| l.strip_prefix("0::")).ok_or_else(|| anyhow!("sem cgroup v2"))?;
    let mut set = rustix::thread::CpuSet::new();
    for &c in cpus {
        set.set(c);
    }
    rustix::thread::sched_setaffinity(None, &set).context("fixar a thread principal")?;
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    let mut handles = Vec::new();
    for _ in 0..threads {
        handles.push(std::thread::spawn(move || {
            let _ = rustix::thread::sched_setaffinity(None, &set);
            let mut x = 0x9e37_79b9u64;
            while Instant::now() < deadline {
                for _ in 0..4096 {
                    x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                }
                std::hint::black_box(x);
            }
        }));
    }
    println!("ready {path}");
    std::io::stdout().flush()?;
    for h in handles {
        let _ = h.join();
    }
    Ok(())
}
