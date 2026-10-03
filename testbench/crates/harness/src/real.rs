//! Execução de um caso num sistema real. É o que o `oracle-agent` faz dentro do container Debian.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};

use crate::{Bytes, Case, MemTree, Outcome};

pub const DEFAULT_TIMEOUT_MS: u64 = 20_000;

/// Roda um caso em `workdir` (que é criado do zero) e devolve o resultado.
pub fn run_case(case: &Case, workdir: &Path) -> Result<Outcome> {
    let inv = case.invocation()?;
    if workdir.exists() {
        std::fs::remove_dir_all(workdir)?;
    }
    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(crate::FIXTURE_MTIME);
    inv.files.materialize(workdir, mtime).with_context(|| format!("{}: fixture", case.id))?;

    let mut argv: Vec<String> = Vec::new();
    if let Some(ts) = &inv.faketime {
        // `-f` congela o relógio no instante dado (fração .000). Sem `-f`, o faketime só desloca o
        // relógio, que continua andando a partir de uma fração arbitrária: `%S` e `%N` viram ruído.
        argv.extend(["faketime".to_string(), "-f".to_string(), ts.clone()]);
    }
    match &inv.script {
        Some(script) => argv.extend(["bash".to_string(), "-c".to_string(), script.clone()]),
        None => argv.extend(inv.argv.iter().cloned()),
    }

    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .current_dir(workdir)
        .env_clear()
        .envs(inv.full_env())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            // Mesmo comportamento observável do bash: 127 com "command not found".
            return Ok(Outcome {
                stderr: Bytes::from(format!("{}: {e}\n", argv[0])),
                exit: Some(127),
                files: MemTree::capture(workdir)?,
                ..Outcome::default()
            });
        }
    };

    let mut stdin = child.stdin.take().expect("stdin");
    let input = inv.stdin.clone();
    let writer = std::thread::spawn(move || {
        // EPIPE é normal (o programa pode não ler a entrada toda).
        let _ = stdin.write_all(&input);
    });
    let mut out_pipe = child.stdout.take().expect("stdout");
    let mut err_pipe = child.stderr.take().expect("stderr");
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out_pipe.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err_pipe.read_to_end(&mut buf);
        buf
    });

    let timeout = Duration::from_millis(case.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let _ = writer.join();
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();

    use std::os::unix::process::ExitStatusExt;
    Ok(Outcome {
        stdout: Bytes(stdout),
        stderr: Bytes(stderr),
        exit: status.code(),
        signal: status.signal(),
        timed_out,
        files: MemTree::capture(workdir)?,
        unsupported: None,
    })
}
