//! bashkit como linha de base: o comando do caso roda no bash virtual dele, com a fixture num
//! `InMemoryFs` em `/work/case` (o mesmo caminho do oráculo) e o stdin redirecionado de um arquivo
//! fora do diretório do caso. A árvore depois da execução é lida de volta do VFS.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bashkit::{Bash, FileSystem, FileType, InMemoryFs};
use harness::{Bytes, Entry, Invocation, MemTree, Outcome};

pub struct Bashkit {
    pub name: String,
}

const CASE: &str = "/work/case";
const STDIN: &str = "/tmp/f02-stdin";

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

async fn capture(fs: &Arc<InMemoryFs>, root: &Path, rel: &str, out: &mut MemTree) -> Result<(), String> {
    let dir = if rel.is_empty() { root.to_path_buf() } else { root.join(rel) };
    let entries = fs.read_dir(&dir).await.map_err(|e| e.to_string())?;
    for e in entries {
        let child = if rel.is_empty() { e.name.clone() } else { format!("{rel}/{}", e.name) };
        let path = root.join(&child);
        match e.metadata.file_type {
            FileType::Directory => {
                out.entries.insert(child.clone(), Entry::dir(e.metadata.mode & 0o7777));
                Box::pin(capture(fs, root, &child, out)).await?;
            }
            FileType::Symlink => {
                let target = fs.read_link(&path).await.map_err(|e| e.to_string())?;
                out.entries.insert(child, Entry::symlink(target.to_string_lossy().into_owned()));
            }
            _ => {
                let data = fs.read_file(&path).await.map_err(|e| e.to_string())?;
                out.entries.insert(child, Entry::file(data, e.metadata.mode & 0o7777));
            }
        }
    }
    Ok(())
}

impl Bashkit {
    async fn run_async(&self, inv: &Invocation) -> Result<Outcome, String> {
        let fs = Arc::new(InMemoryFs::new());
        let root = PathBuf::from(CASE);
        fs.mkdir(&root, true).await.map_err(|e| e.to_string())?;
        for (path, entry) in &inv.files.entries {
            let p = root.join(path);
            match entry {
                Entry::Dir { .. } => fs.mkdir(&p, true).await.map_err(|e| e.to_string())?,
                Entry::File { mode, .. } => {
                    if let Some(parent) = p.parent() {
                        fs.mkdir(parent, true).await.map_err(|e| e.to_string())?;
                    }
                    fs.add_file(&p, entry.data().unwrap_or_default(), *mode);
                }
                Entry::Symlink { target } => {
                    fs.symlink(Path::new(target), &p).await.map_err(|e| e.to_string())?;
                }
            }
        }
        let mut script = inv.argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
        if !inv.stdin.is_empty() {
            fs.mkdir(Path::new("/tmp"), true).await.map_err(|e| e.to_string())?;
            fs.add_file(STDIN, &inv.stdin, 0o600);
            script.push_str(&format!(" < {STDIN}"));
        }
        let mut builder = Bash::builder().fs(fs.clone()).cwd(CASE);
        for (k, v) in inv.full_env() {
            builder = builder.env(k, v);
        }
        let mut bash = builder.build();
        let r = bash.exec(&script).await.map_err(|e| e.to_string())?;
        let mut files = MemTree::new();
        capture(&fs, &root, "", &mut files).await?;
        Ok(Outcome {
            stdout: Bytes(r.stdout.into_bytes()),
            stderr: Bytes(r.stderr.into_bytes()),
            exit: Some(r.exit_code),
            files,
            ..Outcome::default()
        })
    }
}

impl harness::Candidate for Bashkit {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        if inv.script.is_some() {
            return Outcome::unsupported("caso com script de shell");
        }
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => return Outcome::unsupported(format!("tokio: {e}")),
        };
        match rt.block_on(self.run_async(inv)) {
            Ok(o) => o,
            Err(e) => Outcome::unsupported(format!("bashkit: {e}")),
        }
    }
}
