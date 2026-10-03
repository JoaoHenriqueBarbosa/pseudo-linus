//! H40: wasmsh-runtime 0.9.0 como linha de base.
//!
//! O `WorkerRuntime` é dirigido só pelo protocolo dele (`HostCommand` -> `WorkerEvent`), que é a API
//! que o embedder tem: `Init`, `Run`, `WriteFile`, `ReadFile` e `ListDir`. O VFS (`BackendFs`) é campo
//! privado. Por isso:
//!
//! - diretórios, modos e symlinks da fixture entram por comandos do próprio wasmsh (`mkdir -p`,
//!   `chmod`, `ln -s`), num `Run` de preparação antes do caso; o conteúdo dos arquivos por `WriteFile`;
//! - ambiente e cwd entram num `Run` de preparação (`export`, `cd`), já que o estado do shell persiste
//!   entre `Run`s; o script do caso roda intacto;
//! - stdin não existe no protocolo: vai pra um arquivo do VFS e o script roda num grupo alimentado
//!   por pipe (`cat arquivo | { ...; }`), porque redirecionamento em comando composto
//!   (`{ ...; } < arquivo`) não chega ao stdin dos comandos de dentro no wasmsh (medido);
//! - o retrato usa `ListDir` + `ReadFile`. Modos não são observáveis: o protocolo não tem `stat` e o
//!   `stat` do wasmsh devolve 644/755 fixos (wasmsh-utils src/file_ops.rs, linhas 921 e 922), então o
//!   retrato usa 644 pra arquivo e 755 pra diretório;
//! - symlink não existe: `ln` copia o arquivo (wasmsh-utils src/file_ops.rs, linha 814) e `touch`
//!   ignora `-d`/`-t` (linhas 615 a 617), então não há mtime nem relógio injetável (`faketime` vira
//!   `unsupported`).

use f15_baseline_common::{script_of, serve, shell_quote};
use harness::{Bytes, Entry, Invocation, MemTree, Outcome};
use wasmsh_protocol::{HostCommand, WorkerEvent};
use wasmsh_runtime::WorkerRuntime;

const STDIN_FILE: &str = "/tmp/.f15-stdin";

fn main() {
    serve(run_case);
}

/// Saída agregada de uma lista de eventos.
#[derive(Default)]
struct Collected {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit: Option<i32>,
    errors: Vec<String>,
}

fn collect(events: Vec<WorkerEvent>) -> Collected {
    let mut c = Collected::default();
    for ev in events {
        match ev {
            WorkerEvent::Stdout(b) => c.stdout.extend(b),
            WorkerEvent::Stderr(b) => c.stderr.extend(b),
            WorkerEvent::Exit(code) => c.exit = Some(code),
            WorkerEvent::Diagnostic(level, msg) => {
                if format!("{level:?}") == "Error" {
                    c.errors.push(msg);
                }
            }
            _ => {}
        }
    }
    c
}

fn run(rt: &mut WorkerRuntime, input: String) -> Collected {
    collect(rt.handle_command(HostCommand::Run { input }))
}

fn run_case(inv: &Invocation) -> Outcome {
    if inv.faketime.is_some() {
        return Outcome::unsupported("wasmsh não tem relógio injetável nem mtime (touch -d/-t é no-op)");
    }
    let mut rt = WorkerRuntime::new();
    // step_budget 0 = sem teto de passos; o timeout fica com o orquestrador.
    let init = collect(rt.handle_command(HostCommand::Init { step_budget: 0, allowed_hosts: Vec::new() }));
    if !init.errors.is_empty() {
        return Outcome::unsupported(format!("Init: {}", init.errors.join("; ")));
    }
    if let Err(e) = load_fixture(&mut rt, inv) {
        return Outcome::unsupported(e);
    }
    let mut script = script_of(inv);
    if !inv.stdin.is_empty() {
        let w = collect(rt.handle_command(HostCommand::WriteFile { path: STDIN_FILE.into(), data: inv.stdin.clone() }));
        if !w.errors.is_empty() {
            return Outcome::unsupported(format!("gravar stdin no VFS: {}", w.errors.join("; ")));
        }
        // `{ ...; } < arquivo` não alimenta o stdin no wasmsh (medido), então o grupo recebe por pipe.
        script = format!("cat {STDIN_FILE} | {{ {script}\n}}");
    }
    let r = run(&mut rt, script);
    let mut stderr = r.stderr;
    for e in &r.errors {
        stderr.extend(format!("wasmsh: {e}\n").into_bytes());
    }
    let files = snapshot(&mut rt);
    Outcome { stdout: Bytes(r.stdout), stderr: Bytes(stderr), exit: r.exit.or(Some(0)), files, ..Outcome::default() }
}

fn load_fixture(rt: &mut WorkerRuntime, inv: &Invocation) -> Result<(), String> {
    let root = harness::CASE_DIR;
    let mut prep = vec![
        format!("mkdir -p {root} /tmp /root"),
    ];
    for (rel, entry) in &inv.files.entries {
        if let Entry::Dir { .. } = entry {
            prep.push(format!("mkdir -p {}", shell_quote(&format!("{root}/{rel}"))));
        }
    }
    let r = run(rt, prep.join("\n"));
    if r.exit.unwrap_or(0) != 0 || !r.errors.is_empty() {
        return Err(format!("preparar diretórios: {} {}", String::from_utf8_lossy(&r.stderr), r.errors.join("; ")));
    }
    for (rel, entry) in &inv.files.entries {
        if let Entry::File { data, .. } = entry {
            let bytes = data.as_ref().map(|d| d.0.clone()).unwrap_or_default();
            let w = collect(rt.handle_command(HostCommand::WriteFile { path: format!("{root}/{rel}"), data: bytes }));
            if !w.errors.is_empty() {
                return Err(format!("WriteFile {rel}: {}", w.errors.join("; ")));
            }
        }
    }
    let mut post = Vec::new();
    for (rel, entry) in &inv.files.entries {
        let path = shell_quote(&format!("{root}/{rel}"));
        match entry {
            Entry::Symlink { target } => post.push(format!("ln -s {} {path}", shell_quote(target))),
            Entry::File { mode, .. } | Entry::Dir { mode } => post.push(format!("chmod {mode:o} {path}")),
        }
    }
    // Ambiente e cwd: o estado do shell persiste entre Runs.
    for (k, v) in inv.full_env() {
        post.push(format!("export {k}={}", shell_quote(&v)));
    }
    post.push(format!("cd {root}"));
    let r = run(rt, post.join("\n"));
    if !r.errors.is_empty() {
        return Err(format!("preparar modos, symlinks e ambiente: {}", r.errors.join("; ")));
    }
    Ok(())
}

fn list_dir(rt: &mut WorkerRuntime, path: &str) -> Option<Vec<String>> {
    let c = collect(rt.handle_command(HostCommand::ListDir { path: path.into() }));
    if !c.errors.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(&c.stdout).into_owned();
    Some(text.lines().filter(|l| !l.is_empty() && *l != "." && *l != "..").map(str::to_string).collect())
}

fn read_file(rt: &mut WorkerRuntime, path: &str) -> Option<Vec<u8>> {
    let c = collect(rt.handle_command(HostCommand::ReadFile { path: path.into() }));
    if c.errors.is_empty() { Some(c.stdout) } else { None }
}

fn snapshot(rt: &mut WorkerRuntime) -> MemTree {
    let root = harness::CASE_DIR.to_string();
    let mut tree = MemTree::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Some(names) = list_dir(rt, &dir) else { continue };
        for name in names {
            let path = format!("{dir}/{name}");
            let rel = path[root.len() + 1..].to_string();
            match read_file(rt, &path) {
                Some(data) => {
                    tree.entries.insert(rel, Entry::file(data, 0o644));
                }
                None => {
                    if list_dir(rt, &path).is_some() {
                        tree.entries.insert(rel, Entry::dir(0o755));
                        stack.push(path);
                    }
                }
            }
        }
    }
    tree
}
