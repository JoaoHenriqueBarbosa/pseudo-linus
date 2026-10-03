//! H40: bashkit 0.18.2 como linha de base.
//!
//! A fixture vai pro `InMemoryFs` que o próprio `BashBuilder` monta (acessado por `Bash::fs()` depois
//! do build), com modo (`chmod`), symlink e mtime (`set_modified_time`). O caso roda com
//! `ExecutionLimits::cli()` (os limites que o CLI do bashkit usa quando o usuário escolheu rodar o
//! script: sem teto de comandos e laços, saída até 10 MB), cwd `/work/case`, o ambiente fixo da
//! bancada, stdin pelo `ExecOptions::stdin` e relógio fixo pelo `BashBuilder::fixed_epoch` quando o
//! caso tem `faketime`. Features ligadas: jq, git e sqlite (Turso), pra cobrir os diretórios de
//! golden que usam essas ferramentas.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bashkit::{Bash, ExecOptions, ExecutionLimits, FileSystem, FileType, GitConfig};
use f15_baseline_common::{faketime_epoch, script_of, serve};
use harness::{Bytes, Entry, Invocation, MemTree, Outcome};

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime tokio");
    serve(|inv| rt.block_on(run_case(inv)));
}

async fn run_case(inv: &Invocation) -> Outcome {
    let mut builder = Bash::builder()
        .cwd(harness::CASE_DIR)
        .limits(ExecutionLimits::cli())
        .username("root")
        .hostname("oracle")
        .git(GitConfig::new())
        .sqlite();
    for (k, v) in inv.full_env() {
        builder = builder.env(k, v);
    }
    if let Some(ft) = &inv.faketime {
        match faketime_epoch(ft) {
            Some(epoch) => builder = builder.fixed_epoch(epoch),
            None => return Outcome::unsupported(format!("faketime não reconhecido: {ft}")),
        }
    }
    let mut bash = builder.build();
    let fs = bash.fs();
    if let Err(e) = load_fixture(fs.as_ref(), &inv.files).await {
        return Outcome::unsupported(format!("montar fixture no InMemoryFs: {e}"));
    }
    let script = script_of(inv);
    let opts = ExecOptions::new().stdin(inv.stdin.clone());
    let (stdout, stderr, exit) = match bash.exec_with_options(&script, opts).await {
        Ok(r) => (r.stdout.as_bytes().to_vec(), r.stderr.as_bytes().to_vec(), r.exit_code),
        // Erro da API (parse, limite, cancelamento) é a saída observável do candidato.
        Err(e) => {
            let code = if matches!(e, bashkit::Error::Parse { .. }) { 2 } else { 1 };
            (Vec::new(), format!("bashkit: {e}\n").into_bytes(), code)
        }
    };
    let files = match snapshot(fs.as_ref()).await {
        Ok(t) => t,
        Err(e) => return Outcome::unsupported(format!("retrato do InMemoryFs: {e}")),
    };
    Outcome { stdout: Bytes(stdout), stderr: Bytes(stderr), exit: Some(exit), files, ..Outcome::default() }
}

fn fixture_time() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(harness::FIXTURE_MTIME)
}

async fn load_fixture(fs: &dyn FileSystem, tree: &MemTree) -> bashkit::Result<()> {
    let root = PathBuf::from(harness::CASE_DIR);
    fs.mkdir(&root, true).await?;
    // BTreeMap: pai antes de filho.
    for (rel, entry) in &tree.entries {
        let path = root.join(rel);
        match entry {
            Entry::Dir { .. } => fs.mkdir(&path, true).await?,
            Entry::File { data, .. } => {
                let bytes = data.as_ref().map(|d| d.0.clone()).unwrap_or_default();
                fs.write_file(&path, &bytes).await?;
            }
            Entry::Symlink { target } => fs.symlink(Path::new(target), &path).await?,
        }
    }
    // Modos e mtimes do mais fundo pro mais raso, como o `MemTree::materialize` do oráculo.
    for (rel, entry) in tree.entries.iter().rev() {
        let path = root.join(rel);
        match entry {
            Entry::File { mode, .. } | Entry::Dir { mode } => {
                fs.chmod(&path, *mode).await?;
                fs.set_modified_time(&path, fixture_time()).await?;
            }
            Entry::Symlink { .. } => {}
        }
    }
    fs.set_modified_time(&root, fixture_time()).await?;
    Ok(())
}

async fn snapshot(fs: &dyn FileSystem) -> bashkit::Result<MemTree> {
    let root = PathBuf::from(harness::CASE_DIR);
    let mut tree = MemTree::new();
    if !fs.exists(&root).await? {
        return Ok(tree);
    }
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for item in fs.read_dir(&dir).await? {
            let path = dir.join(&item.name);
            let rel = path.strip_prefix(&root).expect("dentro da raiz").to_string_lossy().into_owned();
            let mode = item.metadata.mode & 0o7777;
            match item.metadata.file_type {
                FileType::Symlink => {
                    let target = fs.read_link(&path).await?;
                    tree.entries.insert(rel, Entry::symlink(target.to_string_lossy().into_owned()));
                }
                FileType::Directory => {
                    tree.entries.insert(rel, Entry::dir(mode));
                    stack.push(path);
                }
                FileType::File => {
                    let data = fs.read_file(&path).await?;
                    tree.entries.insert(rel, Entry::file(data, mode));
                }
                FileType::Fifo => {
                    tree.entries.insert(rel, Entry::file(Vec::new(), mode | 0o010000));
                }
            }
        }
    }
    Ok(tree)
}

#[allow(dead_code)]
fn assert_send(_: Arc<dyn FileSystem>) {}
