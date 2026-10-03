//! E05: verdade de campo do Linux real.
//!
//! Mede no host (kernel 6.12.101, glibc 2.41) e no oráculo (Debian 13, mesmo kernel) as constantes,
//! errnos, mensagens e formatos que o pseudo-linus tem que reproduzir, e confere as afirmações do design.
//! Grava, além do resultado, `golden/linux-facts/linux_facts.json` (tabelas que o kernel vai consumir) e
//! amostras de `/proc` em `golden/linux-facts/proc/`.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::Command;

use anyhow::{Context, Result};
use harness::{ExperimentResult, MemTree, Oracle, Verdict, paths};
use serde_json::{Value, json};

const PROBE: &str = include_str!("probe.sh");

/// Mensagem do strerror da glibc, via std (que chama strerror_r), sem o sufixo " (os error N)".
fn strerror(n: i32) -> String {
    let full = std::io::Error::from_raw_os_error(n).to_string();
    match full.rfind(" (os error ") {
        Some(i) => full[..i].to_string(),
        None => full,
    }
}

/// Nomes de errno e de sinal lidos da glibc do host pelo python3 (que expõe as tabelas dos headers).
fn glibc_tables() -> Option<Value> {
    let script = r#"
import errno, os, signal, json
errnos = {str(n): {"name": name, "strerror": os.strerror(n)} for n, name in sorted(errno.errorcode.items())}
sigs = {}
for s in signal.Signals:
    n = int(s)
    sigs.setdefault(str(n), {"names": [], "description": signal.strsignal(n)})
    sigs[str(n)]["names"].append(s.name)
print(json.dumps({"errno": errnos, "signals": sigs}))
"#;
    let out = Command::new("python3").args(["-c", script]).output().ok()?;
    serde_json::from_slice(&out.stdout).ok()
}

fn sections(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut buf = String::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("@@") {
            if let Some(c) = current.take() {
                map.insert(c, buf.trim_end_matches('\n').to_string());
            }
            current = Some(name.to_string());
            buf.clear();
        } else if current.is_some() {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    if let Some(c) = current {
        map.insert(c, buf.trim_end_matches('\n').to_string());
    }
    map
}

fn kv(section: &str) -> BTreeMap<String, String> {
    section
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

/// Escrita de exatamente PIPE_BUF bytes é atômica? Quatro escritores, blocos uniformes, leitor confere.
fn pipe_atomicity(block: usize, writers: usize, blocks_per_writer: usize) -> Result<(usize, usize)> {
    let (reader, writer) = rustix::pipe::pipe()?;
    let writer = std::sync::Arc::new(std::fs::File::from(writer));
    let mut handles = Vec::new();
    for id in 0..writers {
        let w = writer.clone();
        handles.push(std::thread::spawn(move || {
            let data = vec![b'A' + id as u8; block];
            for _ in 0..blocks_per_writer {
                (&*w).write_all(&data).expect("write");
            }
        }));
    }
    let total = block * writers * blocks_per_writer;
    let reader_thread = std::thread::spawn(move || {
        let mut r = std::fs::File::from(reader);
        let mut data = Vec::with_capacity(total);
        let mut buf = vec![0u8; 1 << 16];
        while data.len() < total {
            let n = r.read(&mut buf).expect("read");
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
        }
        data
    });
    for h in handles {
        h.join().expect("writer");
    }
    drop(writer);
    let data = reader_thread.join().expect("reader");
    let chunks = data.chunks(block).count();
    let mixed = data.chunks(block).filter(|c| c.iter().any(|b| *b != c[0])).count();
    Ok((mixed, chunks))
}

fn symlink_chain_limit() -> Result<(Option<i32>, Option<i32>)> {
    let dir = paths::scratch_dir("e05").join("chain");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("target"), b"ok")?;
    let make = |prefix: &str, n: usize| -> Result<()> {
        for i in 0..n {
            let target = if i + 1 == n { "target".to_string() } else { format!("{prefix}{}", i + 1) };
            std::os::unix::fs::symlink(target, dir.join(format!("{prefix}{i}")))?;
        }
        Ok(())
    };
    make("a", 40)?;
    make("b", 41)?;
    let err = |p: &str| std::fs::read(dir.join(p)).err().and_then(|e| e.raw_os_error());
    Ok((err("a0"), err("b0")))
}

fn name_limits() -> Result<(Option<i32>, Option<i32>)> {
    let dir = paths::scratch_dir("e05").join("names");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    let ok = std::fs::write(dir.join("n".repeat(255)), b"").err().and_then(|e| e.raw_os_error());
    let too_long = std::fs::write(dir.join("n".repeat(256)), b"").err().and_then(|e| e.raw_os_error());
    Ok((ok, too_long))
}

fn main() -> Result<()> {
    let mut result = ExperimentResult::new("e05-linux-facts", "Verdade de campo do Linux real");
    let mut checks: Vec<(String, String, String, bool)> = Vec::new(); // (constante, design, medido, bate)
    let mut check = |name: &str, design: &str, measured: String| {
        let ok = design == measured;
        checks.push((name.to_string(), design.to_string(), measured, ok));
    };

    // --- Host ---
    let (pr, pw) = rustix::pipe::pipe()?;
    let pipe_size = rustix::pipe::fcntl_getpipe_size(&pw)?;
    drop((pr, pw));
    check("capacidade padrão do pipe", "65536", pipe_size.to_string());

    let (mixed_4096, chunks_4096) = pipe_atomicity(4096, 4, 4000)?;
    let (mixed_big, chunks_big) = pipe_atomicity(256 * 1024, 4, 40)?;
    check("escrita de PIPE_BUF (4096) é atômica", "0 blocos misturados", format!("{mixed_4096} blocos misturados"));

    let (chain40, chain41) = symlink_chain_limit()?;
    check("cadeia de 40 symlinks resolve", "None", format!("{chain40:?}"));
    check("cadeia de 41 symlinks dá ELOOP (40)", "Some(40)", format!("{chain41:?}"));
    let (name255, name256) = name_limits()?;
    check("nome de 255 bytes aceito", "None", format!("{name255:?}"));
    check("nome de 256 bytes dá ENAMETOOLONG (36)", "Some(36)", format!("{name256:?}"));

    let dev_full = std::fs::write("/dev/full", b"x").err().and_then(|e| e.raw_os_error());
    check("escrita em /dev/full dá ENOSPC (28)", "Some(28)", format!("{dev_full:?}"));

    let host_pid_max = std::fs::read_to_string("/proc/sys/kernel/pid_max")?.trim().to_string();
    check("pid_max", "32768", host_pid_max.clone());

    let ncpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let factor = 1 + (ncpus.min(8) as f64).log2().floor() as u64;
    let fair_c = fetch_fair_c();
    let base_slice_src = fair_c
        .as_deref()
        .and_then(|src| src.lines().find(|l| l.trim_start().starts_with("unsigned int sysctl_sched_base_slice")))
        .and_then(|l| l.split('=').nth(1))
        .map(|v| v.trim().trim_end_matches(';').trim_end_matches("ULL").trim().to_string());
    match &base_slice_src {
        Some(ns) => check("sysctl_sched_base_slice na 6.12.101 (ns)", "750000", ns.clone()),
        None => result.notes.push("fair.c da 6.12.101 não pôde ser baixado; fatia base não conferida aqui (ver E02).".into()),
    }

    // --- Tabelas da glibc ---
    let tables = glibc_tables();
    let mut errno_table = BTreeMap::new();
    let mut strerror_agree = 0;
    let mut strerror_total = 0;
    for n in 1..=133 {
        let ours = strerror(n);
        let py = tables
            .as_ref()
            .and_then(|t| t.pointer(&format!("/errno/{n}")))
            .cloned();
        if let Some(py) = &py {
            strerror_total += 1;
            if py.get("strerror").and_then(Value::as_str) == Some(ours.as_str()) {
                strerror_agree += 1;
            }
        }
        errno_table.insert(
            n.to_string(),
            json!({
                "name": py.as_ref().and_then(|p| p.get("name")).cloned().unwrap_or(Value::Null),
                "strerror": ours,
            }),
        );
    }
    let signal_table = tables.as_ref().and_then(|t| t.get("signals")).cloned().unwrap_or(Value::Null);

    // --- Oráculo ---
    let oracle = Oracle::locate().context("oráculo")?;
    let probe = oracle.run_script("e05-probe", PROBE, MemTree::new())?;
    let text = String::from_utf8_lossy(probe.stdout.as_slice()).into_owned();
    let sec = sections(&text);
    let getconf = kv(sec.get("getconf").map(String::as_str).unwrap_or(""));
    check("PATH_MAX", "4096", getconf.get("PATH_MAX").cloned().unwrap_or_default());
    check("NAME_MAX", "255", getconf.get("NAME_MAX").cloned().unwrap_or_default());
    check("PIPE_BUF", "4096", getconf.get("PIPE_BUF").cloned().unwrap_or_default());
    let container_pid_max = sec.get("pid_max").cloned().unwrap_or_default();
    let exit_codes = kv(sec.get("exit_codes").map(String::as_str).unwrap_or(""));
    check("bash: morto por SIGTERM sai com 143", "143", exit_codes.get("TERM").cloned().unwrap_or_default());
    check("bash: comando inexistente sai com 127", "127", exit_codes.get("NOTFOUND").cloned().unwrap_or_default());
    check("bash: sem permissão de execução sai com 126", "126", exit_codes.get("NOEXEC").cloned().unwrap_or_default());
    check("yes | head: yes morre por SIGPIPE (141)", "141 0", exit_codes.get("PIPESTATUS").cloned().unwrap_or_default());
    let root_bypass = kv(sec.get("root_bypass").map(String::as_str).unwrap_or(""));

    // H15: a mensagem das ferramentas termina com o strerror do errno esperado.
    let expected: &[(&str, i32)] = &[
        ("cat: missing:", 2),
        ("mkdir: cannot create directory", 17),
        ("cd: f:", 20),
        ("rmdir: failed to remove 'e':", 39),
        ("cat: d:", 21),
        ("cat: loop1:", 40),
        ("cat: f/x:", 20),
        ("write error:", 28),
        ("ls: cannot access", 36),
        ("rm: cannot remove 'd':", 21),
        ("mv: cannot stat 'missing':", 2),
        ("cp: cannot stat 'missing':", 2),
        ("rmdir: failed to remove 'missing':", 2),
        ("ln: failed to create symbolic link 'f':", 17),
    ];
    let error_lines: Vec<&str> = sec.get("errors").map(|s| s.lines().collect()).unwrap_or_default();
    let mut message_checks = Vec::new();
    for (needle, errno) in expected {
        let line = error_lines.iter().find(|l| l.contains(needle)).copied().unwrap_or("");
        let want = strerror(*errno);
        let ok = line.ends_with(&format!(": {want}"));
        message_checks.push(json!({"tool_prefix": needle, "errno": errno, "line": line, "matches_strerror": ok}));
    }
    let messages_ok = message_checks.iter().filter(|m| m["matches_strerror"] == true).count();

    // Amostras de /proc e tabela consumível pelo kernel.
    let golden_dir = paths::root().join("golden/linux-facts");
    std::fs::create_dir_all(golden_dir.join("proc"))?;
    for (name, content) in &sec {
        if let Some(file) = name.strip_prefix("proc_") {
            std::fs::write(golden_dir.join("proc").join(format!("{file}.txt")), format!("{content}\n"))?;
        }
    }
    let facts = json!({
        "source": {"kernel": result.host.kernel, "glibc": "2.41 (Debian 13)", "oracle_image": Oracle::image_tag()?},
        "errno": errno_table,
        "signals": signal_table,
        "limits": {
            "PATH_MAX": 4096, "NAME_MAX": 255, "PIPE_BUF": 4096, "MAXSYMLINKS": 40,
            "pipe_default_capacity": pipe_size,
            "ARG_MAX": getconf.get("ARG_MAX"), "CLK_TCK": getconf.get("CLK_TCK"), "PAGESIZE": getconf.get("PAGESIZE"),
            "pid_max": host_pid_max,
        },
        "bash_exit_codes": exit_codes,
        "umask": sec.get("umask"),
        "ulimit_a": sec.get("ulimit"),
        "uname": sec.get("uname"),
        "kill_l": sec.get("kill_l"),
        "root_bypass": root_bypass,
        "ls_time_format": sec.get("ls_time"),
        "error_messages": error_lines,
    });
    std::fs::write(golden_dir.join("linux_facts.json"), serde_json::to_string_pretty(&facts)? + "\n")?;

    let mismatches: Vec<&(String, String, String, bool)> = checks.iter().filter(|c| !c.3).collect();
    result.metrics = json!({
        "checks": checks.iter().map(|(n, d, m, ok)| json!({"constant": n, "design": d, "measured": m, "matches": ok})).collect::<Vec<_>>(),
        "pipe_atomicity": {
            "block_4096": {"mixed_blocks": mixed_4096, "blocks": chunks_4096},
            "block_256KiB": {"mixed_blocks": mixed_big, "blocks": chunks_big},
        },
        "pid_max": {"host": host_pid_max, "container": container_pid_max, "kernel_default_for_16_cpus": 32768, "note": "o Debian 13 sobe via /usr/lib/sysctl.d (systemd); pid_max não é por namespace"},
        "base_slice": {"source_ns": base_slice_src, "ncpus": ncpus, "tunable_scaling_factor": factor},
        "strerror": {"agree_with_glibc_tables": strerror_agree, "compared": strerror_total},
        "tool_messages": {"match": messages_ok, "total": message_checks.len(), "detail": message_checks},
        "root_bypass": root_bypass,
    });

    let h14 = if mismatches.is_empty() { Verdict::Confirmed } else { Verdict::Partial };
    result.hypothesis(
        "H14",
        h14,
        format!(
            "{} de {} constantes batem. Divergem: {}. Pipe tem {} bytes por padrão; escrita de 4096 bytes nunca misturou ({} blocos misturados em {}), escrita de 256 KiB misturou {} de {} blocos.",
            checks.len() - mismatches.len(),
            checks.len(),
            if mismatches.is_empty() {
                "nenhuma".to_string()
            } else {
                mismatches.iter().map(|(n, d, m, _)| format!("{n} (v1 dizia {d}, real {m})")).collect::<Vec<_>>().join("; ")
            },
            pipe_size,
            mixed_4096,
            chunks_4096,
            mixed_big,
            chunks_big,
        ),
        json!({"mismatches": mismatches.iter().map(|(n, d, m, _)| json!({"constant": n, "design": d, "measured": m})).collect::<Vec<_>>()}),
    );
    let h15 = if strerror_agree == strerror_total && messages_ok == message_checks.len() && strerror_total > 0 {
        Verdict::Confirmed
    } else {
        Verdict::Partial
    };
    result.hypothesis(
        "H15",
        h15,
        format!(
            "strerror da glibc (via std) bate com a tabela da glibc em {strerror_agree}/{strerror_total} errnos, e {messages_ok}/{} mensagens reais de cat, mkdir, cd, rmdir, ls, rm, mv, cp, ln e escrita em /dev/full terminam exatamente com o strerror do errno esperado. A tabela errno, nome e mensagem foi gravada em golden/linux-facts/linux_facts.json.",
            message_checks.len()
        ),
        json!({"strerror_agree": strerror_agree, "tool_messages_match": messages_ok}),
    );
    if let Some(mkdir_line) = error_lines.iter().find(|l| l.starts_with("mkdir:")) {
        result.notes.push(format!(
            "Aspas dependem da função de quoting do coreutils, não só do locale: em C.UTF-8 o mkdir escreve {mkdir_line:?} (aspas curvas, quote()), enquanto rm, mv, cp, ln e ls usam aspas ASCII (quoteaf()). O porte de cada utilitário tem que manter a função certa."
        ));
    }
    result.notes.push(format!(
        "Root ignora permissões no Debian real: cat de arquivo modo 000 sai com {} e escrever em diretório modo 000 sai com {}. O VFS precisa reproduzir CAP_DAC_OVERRIDE pro usuário root.",
        root_bypass.get("cat_mode_000_as_root").cloned().unwrap_or_default(),
        root_bypass.get("write_into_mode_000_dir_as_root").cloned().unwrap_or_default(),
    ));
    let path = result.write()?;
    eprintln!("gravado {}", path.display());
    println!("{}", serde_json::to_string_pretty(&result.hypotheses)?);
    Ok(())
}

/// fair.c da 6.12.101 (árvore stable). Sem rede, devolve None e a nota registra.
fn fetch_fair_c() -> Option<String> {
    let cache = paths::scratch_dir("e05").join("fair-6.12.101.c");
    if let Ok(text) = std::fs::read_to_string(&cache) {
        return Some(text);
    }
    let url = "https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git/plain/kernel/sched/fair.c?h=v6.12.101";
    let out = Command::new("curl").args(["-sf", "--max-time", "30", url]).output().ok()?;
    if !out.status.success() || out.stdout.is_empty() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let _ = std::fs::write(&cache, &text);
    Some(text)
}
