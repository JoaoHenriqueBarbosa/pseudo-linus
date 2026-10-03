//! H40: rust-bash 0.3.0 como linha de base.
//!
//! Sem as features `cli` (rustyline), `network` (ureq) e `native-fs`: só o interpretador sobre o
//! `InMemoryFs` dele. A fixture entra pela trait `VirtualFs` (mkdir_p, write_file, symlink, chmod,
//! utimes), o ambiente e o cwd pelo builder, e os limites de contagem (10 mil comandos e 10 mil
//! iterações por padrão) sobem pra não reprovar script legítimo por cota.
//!
//! Duas limitações da API, registradas como resultado:
//!
//! - **stdin**: `RustBash::exec_with_overrides` (src/api.rs, linhas 154 a 183) só aceita `&str` e
//!   implementa stdin colando um here-doc no fim do script (`{input} <<'__EXEC_STDIN__'`), o que só
//!   alimenta o último comando e acrescenta uma quebra de linha. Aqui o stdin vai pra um arquivo do
//!   VFS e o script roda num grupo alimentado por pipe (`cat arquivo | { ...; }`), porque
//!   redirecionamento em comando composto (`{ ...; } < arquivo`, `( ... ) < arquivo`) não chega ao
//!   stdin dos comandos de dentro no rust-bash (medido: `{ read x; }` lê vazio).
//! - **relógio**: o `date` usa `chrono::Local::now()` direto (src/commands/utils.rs, linha 294), sem
//!   relógio injetável; caso com `faketime` vira `unsupported`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use f15_baseline_common::{script_of, serve};
use harness::{Bytes, Entry, Invocation, MemTree, Outcome};
use rust_bash::{ExecutionLimits, NodeType, RustBashBuilder, VirtualFs};

const STDIN_FILE: &str = "/tmp/.f15-stdin";

fn main() {
    serve(run_case);
}

fn run_case(inv: &Invocation) -> Outcome {
    if inv.faketime.is_some() {
        return Outcome::unsupported("rust-bash não tem relógio injetável (date usa chrono::Local::now)");
    }
    let env: HashMap<String, String> = inv.full_env().into_iter().collect();
    let limits = ExecutionLimits {
        max_command_count: usize::MAX,
        max_loop_iterations: usize::MAX,
        ..ExecutionLimits::default()
    };
    let mut shell = match RustBashBuilder::new().env(env).cwd(harness::CASE_DIR).execution_limits(limits).build() {
        Ok(s) => s,
        Err(e) => return Outcome::unsupported(format!("build do RustBash: {e}")),
    };
    let fs: Arc<dyn VirtualFs> = shell.fs().clone();
    if let Err(e) = load_fixture(fs.as_ref(), &inv.files) {
        return Outcome::unsupported(format!("montar fixture: {e}"));
    }
    let mut script = script_of(inv);
    if !inv.stdin.is_empty() {
        if let Err(e) = fs.write_file(Path::new(STDIN_FILE), &inv.stdin) {
            return Outcome::unsupported(format!("gravar stdin no VFS: {e}"));
        }
        // `{ ...; } < arquivo` não alimenta o stdin no rust-bash (redirecionamento em comando
        // composto é ignorado; medido na bancada), então o grupo recebe o stdin por pipe.
        script = format!("cat {STDIN_FILE} | {{ {script}\n}}");
    }
    let (stdout, stderr, exit) = match shell.exec(&script) {
        Ok(r) => {
            let out = r.stdout_bytes.unwrap_or_else(|| r.stdout.into_bytes());
            (out, r.stderr.into_bytes(), r.exit_code)
        }
        Err(e) => (Vec::new(), format!("rust-bash: {e}\n").into_bytes(), 2),
    };
    if !inv.stdin.is_empty() {
        let _ = fs.remove_file(Path::new(STDIN_FILE));
    }
    let files = match snapshot(fs.as_ref()) {
        Ok(t) => t,
        Err(e) => return Outcome::unsupported(format!("retrato do VFS: {e}")),
    };
    Outcome { stdout: Bytes(stdout), stderr: Bytes(stderr), exit: Some(exit), files, ..Outcome::default() }
}

fn fixture_time() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(harness::FIXTURE_MTIME)
}

fn load_fixture(fs: &dyn VirtualFs, tree: &MemTree) -> Result<(), rust_bash::VfsError> {
    let root = PathBuf::from(harness::CASE_DIR);
    fs.mkdir_p(&root)?;
    for (rel, entry) in &tree.entries {
        let path = root.join(rel);
        match entry {
            Entry::Dir { .. } => fs.mkdir_p(&path)?,
            Entry::File { data, .. } => {
                let bytes = data.as_ref().map(|d| d.0.clone()).unwrap_or_default();
                fs.write_file(&path, &bytes)?;
            }
            Entry::Symlink { target } => fs.symlink(Path::new(target), &path)?,
        }
    }
    for (rel, entry) in tree.entries.iter().rev() {
        let path = root.join(rel);
        match entry {
            Entry::File { mode, .. } | Entry::Dir { mode } => {
                fs.chmod(&path, *mode)?;
                fs.utimes(&path, fixture_time())?;
            }
            Entry::Symlink { .. } => {}
        }
    }
    fs.utimes(&root, fixture_time())?;
    Ok(())
}

fn snapshot(fs: &dyn VirtualFs) -> Result<MemTree, rust_bash::VfsError> {
    let root = PathBuf::from(harness::CASE_DIR);
    let mut tree = MemTree::new();
    if !fs.exists(&root) {
        return Ok(tree);
    }
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for item in fs.readdir(&dir)? {
            let path = dir.join(&item.name);
            let rel = path.strip_prefix(&root).expect("dentro da raiz").to_string_lossy().into_owned();
            let meta = fs.lstat(&path)?;
            let mode = meta.mode & 0o7777;
            match meta.node_type {
                NodeType::Symlink => {
                    let target = fs.readlink(&path)?;
                    tree.entries.insert(rel, Entry::symlink(target.to_string_lossy().into_owned()));
                }
                NodeType::Directory => {
                    tree.entries.insert(rel, Entry::dir(mode));
                    stack.push(path);
                }
                NodeType::File => {
                    let data = fs.read_file(&path)?;
                    tree.entries.insert(rel, Entry::file(data, mode));
                }
            }
        }
    }
    Ok(tree)
}
