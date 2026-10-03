//! H40: kaish-kernel 0.17.2 como linha de base.
//!
//! kaish não é bash: é uma linguagem própria parecida com Bourne pra agentes (variáveis tipadas, JSON
//! nativo, sem várias construções do bash). Roda do mesmo jeito que as outras linhas de base, e a
//! diferença de linguagem aparece no placar.
//!
//! Configuração: `KernelConfig::isolated()` (VFS só em memória, `VfsMountMode::NoLocal`, sem comando
//! externo), sem as features `localfs`, `overlay` e `subprocess`. A fixture entra pelo `VfsRouter`
//! (`Kernel::vfs()`, trait `Filesystem`); o ambiente, o cwd e o stdin pelo `ExecuteOptions`.
//!
//! Limitações da API registradas como resultado:
//!
//! - o `MemoryFs` não tem modelo de permissão nem `chmod`: reporta sempre 0666 pra arquivo e 0777 pra
//!   diretório (kaish-vfs src/memory.rs, linhas 200 a 216, "There is no `chmod` builtin");
//! - symlink só com alvo relativo (`refuse_absolute_target`, kaish-vfs src/memory.rs, linha 700);
//! - sem relógio injetável: caso com `faketime` vira `unsupported`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use f15_baseline_common::{script_of, serve};
use harness::{Bytes, Entry, Invocation, MemTree, Outcome};
use kaish_kernel::ast::Value;
use kaish_kernel::vfs::{DirEntryKind, Filesystem};
use kaish_kernel::{ExecuteOptions, Kernel, KernelConfig};

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime tokio");
    serve(|inv| rt.block_on(run_case(inv)));
}

async fn run_case(inv: &Invocation) -> Outcome {
    if inv.faketime.is_some() {
        return Outcome::unsupported("kaish não tem relógio injetável");
    }
    let kernel = match Kernel::new(KernelConfig::isolated()) {
        Ok(k) => k,
        Err(e) => return Outcome::unsupported(format!("Kernel::new: {e}")),
    };
    let vfs = kernel.vfs();
    let mut notes = Vec::new();
    if let Err(e) = load_fixture(vfs.as_ref(), &inv.files, &mut notes).await {
        return Outcome::unsupported(format!("montar fixture no VFS: {e}"));
    }
    let vars: HashMap<String, Value> = inv.full_env().into_iter().map(|(k, v)| (k, Value::String(v))).collect();
    let mut opts = ExecuteOptions { vars, cwd: Some(PathBuf::from(harness::CASE_DIR)), ..ExecuteOptions::default() };
    opts.stdin = Some(inv.stdin.clone());
    let script = script_of(inv);
    let (stdout, stderr, exit) = match kernel.execute_with_options(&script, opts).await {
        Ok(r) => {
            let out = match r.out_bytes() {
                Some(b) => b.to_vec(),
                None => r.text_out().into_owned().into_bytes(),
            };
            (out, r.err.clone().into_bytes(), r.code as i32)
        }
        Err(e) => (Vec::new(), format!("kaish: {e}\n").into_bytes(), 2),
    };
    let files = match snapshot(vfs.as_ref()).await {
        Ok(t) => t,
        Err(e) => return Outcome::unsupported(format!("retrato do VFS: {e}")),
    };
    let _ = kernel.shutdown().await;
    Outcome { stdout: Bytes(stdout), stderr: Bytes(stderr), exit: Some(exit), files, ..Outcome::default() }
}

fn fixture_time() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(harness::FIXTURE_MTIME)
}

async fn mkdir_p(fs: &dyn Filesystem, path: &Path) -> std::io::Result<()> {
    let mut cur = PathBuf::from("/");
    for comp in path.components().skip(1) {
        cur.push(comp);
        if !fs.exists(&cur).await {
            fs.mkdir(&cur).await?;
        }
    }
    Ok(())
}

async fn load_fixture(fs: &dyn Filesystem, tree: &MemTree, notes: &mut Vec<String>) -> std::io::Result<()> {
    let root = PathBuf::from(harness::CASE_DIR);
    mkdir_p(fs, &root).await?;
    for (rel, entry) in &tree.entries {
        let path = root.join(rel);
        match entry {
            Entry::Dir { .. } => mkdir_p(fs, &path).await?,
            Entry::File { data, .. } => {
                let bytes = data.as_ref().map(|d| d.0.clone()).unwrap_or_default();
                fs.write(&path, &bytes).await?;
            }
            Entry::Symlink { target } => {
                if let Err(e) = fs.symlink(Path::new(target), &path).await {
                    notes.push(format!("symlink {rel} -> {target}: {e}"));
                }
            }
        }
    }
    // Sem chmod no MemoryFs: os modos da fixture se perdem (limitação registrada). Só o mtime entra.
    for (rel, entry) in tree.entries.iter().rev() {
        if !matches!(entry, Entry::Symlink { .. }) {
            let _ = fs.set_mtime(&root.join(rel), fixture_time()).await;
        }
    }
    let _ = fs.set_mtime(&root, fixture_time()).await;
    Ok(())
}

async fn snapshot(fs: &dyn Filesystem) -> std::io::Result<MemTree> {
    let root = PathBuf::from(harness::CASE_DIR);
    let mut tree = MemTree::new();
    if !fs.exists(&root).await {
        return Ok(tree);
    }
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for item in fs.list(&dir).await? {
            let path = dir.join(&item.name);
            let rel = path.strip_prefix(&root).expect("dentro da raiz").to_string_lossy().into_owned();
            let mode = item.permissions.unwrap_or(0) & 0o7777;
            match item.kind {
                DirEntryKind::Symlink => {
                    let target = match item.symlink_target.clone() {
                        Some(t) => t,
                        None => fs.read_link(&path).await?,
                    };
                    tree.entries.insert(rel, Entry::symlink(target.to_string_lossy().into_owned()));
                }
                DirEntryKind::Directory => {
                    tree.entries.insert(rel, Entry::dir(mode));
                    stack.push(path);
                }
                DirEntryKind::File => {
                    let data = fs.read(&path).await?;
                    tree.entries.insert(rel, Entry::file(data, mode));
                }
                // Tipo novo que esta versão não conhece (o enum é non_exhaustive).
                _ => {
                    tree.entries.insert(rel, Entry::file(Vec::new(), mode | 0o170000));
                }
            }
        }
    }
    Ok(tree)
}
