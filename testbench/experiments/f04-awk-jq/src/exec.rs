//! Execução de candidatos que são binários (categoria b ou c): cada caso roda num diretório de rascunho
//! com a fixture materializada, e o nome da ferramenta (`awk`, `gawk`, `jq`) resolve, via PATH, para um
//! shim que aponta pro binário do candidato. Assim os casos `script` (pipelines em bash) também usam o
//! candidato. Também fica aqui o placar paralelo, equivalente ao `harness::score`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use harness::{Bytes, Candidate, Case, CaseComparison, Conformance, Invocation, MemTree, Outcome};

/// Tempo máximo de um caso num candidato (o oráculo usa 20 s; candidato travado não pode segurar a bancada).
pub const CASE_TIMEOUT: Duration = Duration::from_secs(8);

static RUN_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Um binário de terceiro exposto sob um ou mais nomes (ex.: `awk` e `gawk`).
pub struct SubprocessCandidate {
    pub label: String,
    /// Diretório com os shims (`awk`, `gawk` ou `jq`) apontando pro binário do candidato.
    pub shim_dir: PathBuf,
    /// Nomes de programa que este candidato atende.
    pub aliases: Vec<String>,
    /// Raiz dos diretórios de trabalho deste candidato.
    pub work_root: PathBuf,
    /// Variante de medida: troca o nome do programa no início das linhas de stderr (ex.: `qj:` por
    /// `jq:`), pra separar a divergência de nome da divergência de comportamento.
    pub rename_prog: Option<(String, String)>,
}

impl SubprocessCandidate {
    /// Cria os shims. `prefix_args` vazio vira symlink; senão vira um script `exec bin args "$@"`.
    pub fn new(label: &str, binary: &Path, prefix_args: &[&str], aliases: &[&str], scratch: &Path) -> Result<Self> {
        let slug: String = label
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let shim_dir = scratch.join("shims").join(&slug);
        if shim_dir.exists() {
            std::fs::remove_dir_all(&shim_dir)?;
        }
        std::fs::create_dir_all(&shim_dir)?;
        for alias in aliases {
            let shim = shim_dir.join(alias);
            if prefix_args.is_empty() {
                std::os::unix::fs::symlink(binary, &shim)?;
            } else {
                let quoted: Vec<String> = prefix_args.iter().map(|a| shell_quote(a)).collect();
                let body = format!("#!/bin/sh\nexec {} {} \"$@\"\n", shell_quote(&binary.to_string_lossy()), quoted.join(" "));
                std::fs::write(&shim, body)?;
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))?;
            }
        }
        let work_root = scratch.join("work").join(&slug);
        std::fs::create_dir_all(&work_root)?;
        Ok(SubprocessCandidate {
            label: label.to_string(),
            shim_dir,
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            work_root,
            rename_prog: None,
        })
    }
}

pub fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./=:,+@%".contains(&b)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

impl Candidate for SubprocessCandidate {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        if inv.faketime.is_some() {
            return Outcome::unsupported("faketime: o host não tem libfaketime e o candidato não aceita relógio injetado");
        }
        if let Some(prog) = inv.program()
            && !self.aliases.iter().any(|a| a == prog)
        {
            return Outcome::unsupported(format!("caso de outra ferramenta: {prog}"));
        }
        match run_in_dir(&self.work_root, &self.shim_dir, inv, CASE_TIMEOUT) {
            Ok(mut out) => {
                if let Some((from, to)) = &self.rename_prog {
                    let text = String::from_utf8_lossy(out.stderr.as_slice()).into_owned();
                    let renamed: String = text
                        .split_inclusive('\n')
                        .map(|l| {
                            let l = match l.strip_prefix(&format!("{from}:")) {
                                Some(rest) => format!("{to}:{rest}"),
                                None => l.to_string(),
                            };
                            l.replace(&format!("Use {from} --help"), &format!("Use {to} --help"))
                        })
                        .collect();
                    out.stderr = Bytes::from(renamed);
                }
                out
            }
            Err(e) => Outcome::unsupported(format!("falha da bancada ao executar: {e:#}")),
        }
    }
}

/// Roda um caso num diretório novo sob `work_root`, com `shim_dir` na frente do PATH.
pub fn run_in_dir(work_root: &Path, shim_dir: &Path, inv: &Invocation, timeout: Duration) -> Result<Outcome> {
    let n = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    let slug: String = inv
        .case_id
        .chars()
        .take(60)
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    let workdir = work_root.join(format!("{slug}-{n}"));
    if workdir.exists() {
        std::fs::remove_dir_all(&workdir)?;
    }
    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(harness::FIXTURE_MTIME);
    inv.files.materialize(&workdir, mtime).context("fixture")?;

    let mut env = inv.full_env();
    let base_path = env.get("PATH").cloned().unwrap_or_default();
    env.insert("PATH".into(), format!("{}:{base_path}", shim_dir.display()));

    // O contrato da bancada fixa umask 022 (o host costuma ter 002); o `Command` não expõe umask, então
    // um `sh` intermediário ajusta e faz `exec` (o argv[0] do programa continua sendo o nome pedido).
    let mut cmd = Command::new("/bin/sh");
    match &inv.script {
        Some(script) => {
            cmd.arg("-c").arg("umask 022; exec bash -c \"$0\"").arg(script);
        }
        None => {
            cmd.arg("-c").arg("umask 022; exec \"$0\" \"$@\"").args(&inv.argv);
        }
    }
    cmd.current_dir(&workdir)
        .env_clear()
        .envs(&env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let outcome = spawn_and_wait(cmd, &inv.stdin, timeout, &workdir);
    let files = MemTree::capture(&workdir).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&workdir);
    let mut outcome = outcome?;
    outcome.files = files;
    Ok(outcome)
}

/// Spawna, alimenta stdin, coleta stdout/stderr e mata no timeout.
pub fn spawn_and_wait(mut cmd: Command, stdin: &[u8], timeout: Duration, _cwd: &Path) -> Result<Outcome> {
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Ok(Outcome {
                stderr: Bytes::from(format!("spawn: {e}\n")),
                exit: Some(127),
                ..Outcome::default()
            });
        }
    };
    let mut child_stdin = child.stdin.take().expect("stdin");
    let input = stdin.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = child_stdin.write_all(&input);
    });
    let mut out_pipe = child.stdout.take().expect("stdout");
    let mut err_pipe = child.stderr.take().expect("stderr");
    // Limite de captura: candidato em laço infinito imprimindo não pode encher a memória da bancada.
    const CAP: u64 = 16 << 20;
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut out_pipe).take(CAP).read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut err_pipe).take(CAP).read_to_end(&mut buf);
        buf
    });
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
        std::thread::sleep(Duration::from_millis(1));
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
        files: MemTree::new(),
        unsupported: None,
    })
}

/// Um panic dentro do candidato vira falha do caso, não da bancada.
pub fn run_guarded(candidate: &dyn Candidate, inv: &Invocation) -> Outcome {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| candidate.run(inv))) {
        Ok(out) => out,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic sem mensagem".into());
            Outcome::unsupported(format!("panic: {msg}"))
        }
    }
}

/// Roda `f` em cada item com até `threads` threads e devolve os resultados na ordem dos itens.
pub fn par_map<T: Sync, R: Send>(items: &[T], threads: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            // Pilha grande: candidatos em processo (bashkit, jaq) recursam fundo em alguns casos.
            std::thread::Builder::new()
                .stack_size(256 << 20)
                .spawn_scoped(s, || {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= items.len() {
                            break;
                        }
                        let r = f(&items[i]);
                        *slots[i].lock().expect("slot") = Some(r);
                    }
                })
                .expect("thread do par_map");
        }
    });
    slots
        .into_iter()
        .map(|m| m.into_inner().expect("slot").expect("todo item processado"))
        .collect()
}

pub fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
}

/// Equivalente paralelo do `harness::score`.
pub fn score_parallel(
    candidate: &(dyn Candidate + Sync),
    cases: &[(Case, Outcome)],
    threads: usize,
) -> (Conformance, Vec<CaseComparison>) {
    let all: Vec<CaseComparison> = par_map(cases, threads, |(case, golden)| {
        let actual = match case.invocation() {
            Ok(inv) => run_guarded(candidate, &inv),
            Err(e) => Outcome::unsupported(format!("caso inválido: {e}")),
        };
        harness::compare_outcome(case, golden, &actual)
    });
    (conformance_of(&candidate.name(), &all), all)
}

/// Agrega comparações num placar (mesma regra do `harness::score`).
pub fn conformance_of(name: &str, all: &[CaseComparison]) -> Conformance {
    let mut conf = Conformance { candidate: name.to_string(), ..Conformance::default() };
    for cmp in all {
        conf.total += 1;
        conf.strict_pass += cmp.strict as usize;
        conf.lenient_pass += cmp.lenient as usize;
        conf.unsupported += cmp.unsupported.is_some() as usize;
        let tags: Vec<String> = if cmp.tags.is_empty() { vec!["untagged".into()] } else { cmp.tags.clone() };
        for tag in tags {
            let slot = conf.by_tag.entry(tag).or_default();
            slot.0 += cmp.strict as usize;
            slot.1 += cmp.lenient as usize;
            slot.2 += 1;
        }
    }
    conf.sample_failures = all.iter().filter(|c| !c.strict).take(15).cloned().collect();
    conf
}
