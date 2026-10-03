//! Candidatos categoria (b), que leem e escrevem pelo `std::fs`, `stdout` e `stdin` do processo:
//! cada caso roda num subprocesso (o próprio binário com `--exec`), com `cwd` numa cópia da fixture em
//! `scratch/f02-grep-sed/` e o ambiente fixo do oráculo; o retrato do diretório depois da execução
//! entra na comparação (`sed -i`, `w`). O acoplamento ao host dessas crates é medido à parte (depscan).

use std::io::{Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use harness::{Bytes, Invocation, MemTree, Outcome};

static SEQ: AtomicUsize = AtomicUsize::new(0);

/// Um candidato rodado em subprocesso.
pub struct Subprocess {
    pub name: String,
    /// Implementação dentro do modo `--exec` (ver `main.rs`).
    pub implementation: &'static str,
    /// `argv[0]` do processo (o nome que a ferramenta usa nas mensagens).
    pub argv0: &'static str,
    pub scratch: PathBuf,
}

impl harness::Candidate for Subprocess {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        if inv.script.is_some() {
            return Outcome::unsupported("caso com script de shell");
        }
        match run_in_dir(self, inv) {
            Ok(o) => o,
            Err(e) => Outcome::unsupported(format!("falha da bancada: {e:#}")),
        }
    }
}

fn run_in_dir(c: &Subprocess, inv: &Invocation) -> anyhow::Result<Outcome> {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = c.scratch.join(format!("{}-{}-{n}", c.implementation, std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(harness::FIXTURE_MTIME);
    inv.files.materialize(&dir, mtime)?;
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg0(c.argv0)
        .arg("--exec")
        .arg(c.implementation)
        .args(inv.args())
        .current_dir(&dir)
        .env_clear()
        .envs(inv.full_env())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().expect("stdin");
    let input = inv.stdin.clone();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let mut out_pipe = child.stdout.take().expect("stdout");
    let mut err_pipe = child.stderr.take().expect("stderr");
    let out_reader = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out_pipe.read_to_end(&mut b);
        b
    });
    let err_reader = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err_pipe.read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if start.elapsed() > Duration::from_secs(10) {
            timed_out = true;
            let _ = child.kill();
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let _ = writer.join();
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    let files = MemTree::capture(&dir)?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(Outcome {
        stdout: Bytes(stdout),
        stderr: Bytes(stderr),
        exit: status.code(),
        signal: status.signal(),
        timed_out,
        files,
        unsupported: None,
    })
}

/// Diretório de rascunho dos subprocessos.
pub fn scratch() -> PathBuf {
    harness::paths::scratch_dir("f02-grep-sed")
}

/// Garante que o diretório existe (o scratch pode ser limpo entre execuções).
pub fn ensure(p: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(p)
}
