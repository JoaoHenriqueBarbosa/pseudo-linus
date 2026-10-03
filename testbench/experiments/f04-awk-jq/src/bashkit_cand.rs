//! bashkit 0.18 em processo: bash virtual sobre VFS em memória, com `awk` próprio (interpretador
//! escrito pra ele) e `jq` sobre o jaq com uma camada de compatibilidade dele. É a linha de base mais
//! próxima do pseudo-linus; aqui medimos só o awk e o jq dele.
//!
//! Os limites de execução padrão do bashkit (10 mil iterações por laço, 100 níveis de função, 1 MiB
//! de stdout) são política de sandbox, não semântica; pra medir a semântica eles são alargados. O
//! efeito dos limites padrão é medido à parte na sondagem de checkpoint.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bashkit::{Bash, ExecOptions, ExecutionLimits, FileSystem, FileType};
use harness::{Bytes, Candidate, Entry, Invocation, MemTree, Outcome};

use crate::exec::shell_quote;

pub struct Bashkit {
    pub tool: &'static str,
    pub limits: ExecutionLimits,
}

pub fn wide_limits() -> ExecutionLimits {
    ExecutionLimits {
        max_work_units: 10_000_000_000,
        max_commands: 10_000_000,
        max_loop_iterations: 1_000_000_000,
        max_total_loop_iterations: 10_000_000_000,
        max_function_depth: 10_000,
        timeout: Duration::from_secs(10),
        max_stdout_bytes: 16 << 20,
        max_stderr_bytes: 16 << 20,
        ..ExecutionLimits::default()
    }
}

const CASE_DIR: &str = "/work/case";

impl Bashkit {
    pub fn new(tool: &'static str) -> Bashkit {
        Bashkit { tool, limits: wide_limits() }
    }

    fn command_line(&self, inv: &Invocation) -> Option<String> {
        if let Some(script) = &inv.script {
            return Some(script.clone());
        }
        let prog = inv.program()?;
        // O bashkit só tem `awk`; `gawk` é o mesmo comando.
        let prog = if prog == "gawk" { "awk" } else { prog };
        let mut parts = vec![prog.to_string()];
        parts.extend(inv.args().iter().map(|a| shell_quote(a)));
        Some(parts.join(" "))
    }
}

impl Candidate for Bashkit {
    fn name(&self) -> String {
        format!("bashkit 0.18.2 ({})", self.tool)
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        if let Some(p) = inv.program()
            && p != self.tool
            && !(self.tool == "awk" && p == "gawk")
        {
            return Outcome::unsupported(format!("caso de outra ferramenta: {p}"));
        }
        let Some(cmd) = self.command_line(inv) else { return Outcome::unsupported("caso vazio") };
        let rt = match tokio::runtime::Builder::new_current_thread().enable_time().build() {
            Ok(rt) => rt,
            Err(e) => return Outcome::unsupported(format!("tokio: {e}")),
        };
        let limits = self.limits.clone();
        rt.block_on(async move { run_async(inv, &cmd, limits).await })
    }
}

async fn run_async(inv: &Invocation, cmd: &str, limits: ExecutionLimits) -> Outcome {
    let mut builder = Bash::builder().cwd(CASE_DIR).limits(limits);
    for (k, v) in inv.full_env() {
        builder = builder.env(k, v);
    }
    if let Some(ft) = &inv.faketime
        && let Some(epoch) = crate::jq::parse_faketime(ft)
    {
        builder = builder.fixed_epoch(epoch as i64);
    }
    let mut bash = builder.build();
    let fs = bash.fs();
    if let Err(e) = populate(fs.as_ref(), &inv.files).await {
        return Outcome::unsupported(format!("fixture no VFS do bashkit: {e}"));
    }
    let stdin = String::from_utf8_lossy(&inv.stdin).into_owned();
    let opts = ExecOptions::new().stdin(stdin);
    let result = match bash.exec_with_options(cmd, opts).await {
        Ok(r) => r,
        Err(e) => {
            return Outcome {
                stderr: Bytes::from(format!("bashkit: {e}\n")),
                exit: Some(2),
                files: snapshot(fs.as_ref()).await.unwrap_or_default(),
                ..Outcome::default()
            };
        }
    };
    Outcome {
        stdout: Bytes(result.stdout.as_bytes().to_vec()),
        stderr: Bytes(result.stderr.as_bytes().to_vec()),
        exit: Some(result.exit_code),
        files: snapshot(fs.as_ref()).await.unwrap_or_default(),
        ..Outcome::default()
    }
}

async fn populate(fs: &dyn FileSystem, files: &MemTree) -> anyhow::Result<()> {
    let root = Path::new(CASE_DIR);
    fs.mkdir(root, true).await?;
    for (rel, entry) in &files.entries {
        let path = root.join(rel);
        match entry {
            Entry::Dir { mode } => {
                fs.mkdir(&path, true).await?;
                fs.chmod(&path, *mode).await?;
            }
            Entry::File { data, mode, .. } => {
                if let Some(parent) = path.parent() {
                    fs.mkdir(parent, true).await?;
                }
                let bytes = data.as_ref().map(|d| d.0.clone()).unwrap_or_default();
                fs.write_file(&path, &bytes).await?;
                fs.chmod(&path, *mode).await?;
            }
            Entry::Symlink { target } => {
                if let Some(parent) = path.parent() {
                    fs.mkdir(parent, true).await?;
                }
                fs.symlink(Path::new(target), &path).await?;
            }
        }
    }
    Ok(())
}

async fn snapshot(fs: &dyn FileSystem) -> anyhow::Result<MemTree> {
    let root = PathBuf::from(CASE_DIR);
    let mut tree = MemTree::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for item in fs.read_dir(&dir).await? {
            let path = dir.join(&item.name);
            let rel = path.strip_prefix(&root)?.to_string_lossy().into_owned();
            let mode = item.metadata.mode & 0o7777;
            match item.metadata.file_type {
                FileType::Directory => {
                    tree.entries.insert(rel, Entry::dir(mode));
                    stack.push(path);
                }
                FileType::Symlink => {
                    let target = fs.read_link(&path).await?;
                    tree.entries.insert(rel, Entry::symlink(target.to_string_lossy().into_owned()));
                }
                FileType::File => {
                    let data = fs.read_file(&path).await?;
                    tree.entries.insert(rel, Entry::file(data, mode));
                }
                _ => {
                    tree.entries.insert(rel, Entry::file(Vec::new(), mode | 0o170000));
                }
            }
        }
    }
    Ok(tree)
}
