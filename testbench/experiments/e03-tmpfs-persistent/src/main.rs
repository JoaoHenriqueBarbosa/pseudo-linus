//! E03: roda todas as medições do tmpfs persistente e grava `results/e03-tmpfs-persistent.json`.
//!
//! Ordem: memória (subprocesso `e03-mem`, com allocator contador), conformidade diferencial
//! (modelo x tmpfs do host, nosso x modelo), tempos de estrutura e de conteúdo, vazão concorrente,
//! depscan das crates candidatas, e por fim os vereditos.

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use e03_tmpfs_persistent::check::{Model, Ours, RealFs, Target, compare, random_ops};
use e03_tmpfs_persistent::flavors::Meta;
use e03_tmpfs_persistent::maps::Flavor;
use e03_tmpfs_persistent::measure::{concurrency_timing, content_image_timing, huge_file_timing, structure_timing};
use e03_tmpfs_persistent::workload::Rng;
use e03_tmpfs_persistent::{for_each_content, for_each_final, for_each_structure};
use harness::{CandidateResult, ExperimentResult, Fit, Verdict};
use serde_json::{Map, Value, json};

const SEQUENCES: usize = 400;
const SEQ_LEN: usize = 60;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Compila (se preciso) e roda o binário de memória; devolve o JSON dele.
fn run_memory() -> Result<Value> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| env!("CARGO").to_string());
    let manifest = manifest_dir().join("Cargo.toml");
    let status = Command::new(&cargo)
        .args(["build", "--release", "--bin", "e03-mem", "--manifest-path"])
        .arg(&manifest)
        .status()
        .context("cargo build do e03-mem")?;
    if !status.success() {
        bail!("cargo build do e03-mem falhou");
    }
    let exe = std::env::current_exe()?.with_file_name("e03-mem");
    let out = Command::new(&exe).stderr(std::process::Stdio::inherit()).output().with_context(|| format!("rodar {}", exe.display()))?;
    if !out.status.success() {
        bail!("e03-mem saiu com {}", out.status);
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}

fn model_vs_linux() -> Value {
    let mut rng = Rng::new(0x11);
    let mut divergences = 0;
    let mut errors_seen = std::collections::BTreeMap::<String, usize>::new();
    let mut first = Value::Null;
    for _ in 0..SEQUENCES {
        let ops = random_ops(&mut rng, SEQ_LEN, false);
        let mut model = Model::new();
        let mut real = RealFs::new().expect("diretório em /dev/shm");
        if let Some(d) = compare(&ops, &mut model, &mut real) {
            divergences += 1;
            if first.is_null() {
                first = json!(format!("{d:?}"));
            }
        }
        // Errnos efetivamente exercitados (no modelo, que já foi comparado com o real).
        let mut m = Model::new();
        for op in &ops {
            if let Err(e) = m.apply(op) {
                *errors_seen.entry(format!("{e:?}")).or_insert(0) += 1;
            }
        }
    }
    json!({"sequences": SEQUENCES, "ops_per_sequence": SEQ_LEN, "divergences": divergences,
           "first_divergence": first, "errnos_exercised": errors_seen})
}

fn ours_vs_model<F: Flavor>() -> Value {
    let mut rng = Rng::new(0x22);
    let mut divergences = 0;
    let mut fsck_failures = 0;
    let mut first = Value::Null;
    for _ in 0..SEQUENCES {
        let ops = random_ops(&mut rng, SEQ_LEN, true);
        let mut model = Model::new();
        let mut ours = Ours::<F>::new();
        if let Some(d) = compare(&ops, &mut model, &mut ours) {
            divergences += 1;
            if first.is_null() {
                first = json!(format!("{d:?}"));
            }
        }
        let problems = ours.vfs.fsck();
        if !problems.is_empty() {
            fsck_failures += 1;
            if first.is_null() {
                first = json!(problems.join("; "));
            }
        }
    }
    json!({"sequences": SEQUENCES, "ops_per_sequence": SEQ_LEN, "with_snapshots": true,
           "divergences": divergences, "fsck_failures": fsck_failures, "first_problem": first})
}

/// Versão travada no Cargo.lock.
fn locked_version(name: &str) -> String {
    let lock = std::fs::read_to_string(manifest_dir().join("Cargo.lock")).unwrap_or_default();
    let needle = format!("name = \"{name}\"\nversion = \"");
    lock.find(&needle)
        .map(|i| {
            let rest = &lock[i + needle.len()..];
            rest[..rest.find('"').unwrap_or(0)].to_string()
        })
        .unwrap_or_else(|| "?".to_string())
}

fn depscan_of(name: &str) -> Value {
    match depscan::scan(&manifest_dir().join("Cargo.toml"), name) {
        Ok(t) => json!({
            "category": t.tree_category.letter(),
            "unsafe_in_crate": t.root.counts.unsafe_total(),
            "unsafe_in_tree": t.totals.unsafe_total(),
            "host_touch_in_tree": t.totals.host_touch(),
            "deps": t.deps.iter().map(|d| format!("{} {}", d.name, d.version)).collect::<Vec<_>>(),
        }),
        Err(e) => json!({"error": e.to_string()}),
    }
}

fn num(v: &Value, ptr: &str) -> f64 {
    v.pointer(ptr).and_then(Value::as_f64).unwrap_or(f64::NAN)
}

struct Record {
    meta: &'static Meta,
    kind: &'static str,
    timing: Value,
    memory: Value,
    conformance: Value,
    concurrency: Value,
}

fn record(meta: &'static Meta, kind: &'static str, timing: Value, memory: &Value, conformance: Value) -> Record {
    let memory = memory.get(meta.key).cloned().unwrap_or(Value::Null);
    Record { meta, kind, timing, memory, conformance, concurrency: Value::Null }
}

/// Números-chave de estrutura extraídos das medições.
struct StructureFacts {
    snap_1k: f64,
    snap_100k: f64,
    restore_1k: f64,
    restore_100k: f64,
    restore_hot_1k: f64,
    restore_hot_100k: f64,
    restore_one_1k: f64,
    restore_one_100k: f64,
    snap_bytes: f64,
    write_1k: f64,
    write_100k: f64,
    write_hot_1k: f64,
    write_hot_100k: f64,
    write_bytes_1k: f64,
    write_bytes_100k: f64,
    big_dir_create: f64,
    big_dir_bytes: f64,
    base_bytes: f64,
    per_sandbox: f64,
    depth_1: f64,
    depth_32: f64,
    depth_bytes_1: f64,
    depth_bytes_32: f64,
    overhead_per_inode: f64,
    bad: f64,
}

fn structure_facts(r: &Record) -> StructureFacts {
    let t = &r.timing;
    let m = r.memory.get("structure").cloned().unwrap_or(Value::Null);
    let depth_t = |d: usize| -> f64 {
        t["depth"].as_array().and_then(|a| a.iter().find(|x| x["depth"] == d)).map(|x| num(x, "/first_write/median_ns")).unwrap_or(f64::NAN)
    };
    let depth_m = |d: usize| -> f64 {
        m["depth"]
            .as_array()
            .and_then(|a| a.iter().find(|x| x["depth"] == d))
            .map(|x| num(x, "/first_write/mean_live_bytes"))
            .unwrap_or(f64::NAN)
    };
    StructureFacts {
        snap_1k: num(t, "/sizes/0/snapshot_and_drop_ns/median_ns"),
        snap_100k: num(t, "/sizes/2/snapshot_and_drop_ns/median_ns"),
        restore_1k: num(t, "/sizes/0/restore_after_10_writes_ns/median_ns"),
        restore_100k: num(t, "/sizes/2/restore_after_10_writes_ns/median_ns"),
        restore_hot_1k: num(t, "/sizes/0/restore_after_1_write_hot_ns/median_ns"),
        restore_hot_100k: num(t, "/sizes/2/restore_after_1_write_hot_ns/median_ns"),
        restore_one_1k: num(&m, "/sizes/0/restore_after_1_write/mean_freed_bytes"),
        restore_one_100k: num(&m, "/sizes/2/restore_after_1_write/mean_freed_bytes"),
        snap_bytes: (0..3).map(|i| num(&m, &format!("/sizes/{i}/snapshot/allocated_bytes"))).sum::<f64>()
            + (0..3).map(|i| num(&m, &format!("/sizes/{i}/sandbox_new/allocated_bytes"))).sum::<f64>(),
        write_1k: num(t, "/sizes/0/first_write_after_snapshot_ns/median_ns"),
        write_100k: num(t, "/sizes/2/first_write_after_snapshot_ns/median_ns"),
        write_hot_1k: num(t, "/sizes/0/first_write_after_snapshot_hot_ns/median_ns"),
        write_hot_100k: num(t, "/sizes/2/first_write_after_snapshot_hot_ns/median_ns"),
        write_bytes_1k: num(&m, "/sizes/0/first_write_after_snapshot/mean_live_bytes"),
        write_bytes_100k: num(&m, "/sizes/2/first_write_after_snapshot/mean_live_bytes"),
        big_dir_create: num(t, "/big_dir/create_after_snapshot_ns/median_ns"),
        big_dir_bytes: num(&m, "/big_dir/create_after_snapshot/live_bytes"),
        base_bytes: num(&m, "/sandboxes_100k/base_live_bytes"),
        per_sandbox: num(&m, "/sandboxes_100k/per_sandbox_live_bytes"),
        depth_1: depth_t(1),
        depth_32: depth_t(32),
        depth_bytes_1: depth_m(1),
        depth_bytes_32: depth_m(32),
        overhead_per_inode: num(&m, "/sizes/2/overhead_bytes_per_inode"),
        bad: num(&r.conformance, "/divergences") + num(&r.conformance, "/fsck_failures"),
    }
}

/// Critérios do H17 por candidato de estrutura.
struct StructureChecks {
    snapshot_constant: bool,
    restore_constant: bool,
    write_not_proportional: bool,
    write_independent_of_depth: bool,
    shared_memory: bool,
    big_dir_ok: bool,
    conformant: bool,
}

/// Razão máxima do trabalho determinístico (bytes copiados ou liberados) entre 100k e 1k arquivos
/// pra contar como "não cresce com o tamanho": uma árvore de grau 32 vai de 2 pra 4 níveis nesse
/// intervalo (2x); linear seria 100x.
const MAX_RATIO: f64 = 2.5;
/// Razão máxima de tempo (caminho quente) entre 100k e 1k: só checagem de sanidade, com folga pro
/// ruído de uma máquina compartilhada; continua 20x abaixo do linear.
const MAX_TIME_RATIO: f64 = 5.0;

fn structure_checks(f: &StructureFacts) -> StructureChecks {
    StructureChecks {
        // Snapshot e sandbox nova não alocam nada e levam o mesmo tempo com 1k e 100k arquivos.
        snapshot_constant: f.snap_bytes == 0.0 && f.snap_100k / f.snap_1k <= MAX_TIME_RATIO && f.snap_100k < 1_000.0,
        // O trabalho do restore é liberar o que foi copiado desde o snapshot.
        restore_constant: f.restore_one_100k / f.restore_one_1k <= MAX_RATIO
            && f.restore_hot_100k / f.restore_hot_1k <= MAX_TIME_RATIO,
        write_not_proportional: f.write_bytes_100k / f.write_bytes_1k <= MAX_RATIO
            && f.write_hot_100k / f.write_hot_1k <= MAX_TIME_RATIO
            && f.write_100k < 100_000.0,
        write_independent_of_depth: f.depth_bytes_32 / f.depth_bytes_1 <= 1.5,
        shared_memory: f.per_sandbox <= 0.05 * f.base_bytes,
        big_dir_ok: f.big_dir_create < 20_000.0,
        conformant: f.bad == 0.0,
    }
}

fn kib(b: f64) -> String {
    format!("{:.1} KiB", b / 1024.0)
}

fn us(ns: f64) -> String {
    if ns < 1_000.0 { format!("{ns:.0} ns") } else { format!("{:.1} µs", ns / 1_000.0) }
}

fn structure_candidate(r: &Record, scans: &Value) -> (CandidateResult, StructureFacts, StructureChecks) {
    let f = structure_facts(r);
    let c = structure_checks(&f);
    let maintained = r.meta.key != "im-hamt";
    let core = c.snapshot_constant && c.restore_constant && c.write_not_proportional && c.shared_memory && c.conformant;
    let fit = if !core || !maintained {
        Fit::DoesNotFit
    } else if !c.big_dir_ok || r.meta.hand {
        // Parte à mão é "fits with work" por definição: é código nosso pra manter.
        Fit::FitsWithWork
    } else {
        Fit::Fits
    };
    let mut notes = format!(
        "Snapshot {} (1k) e {} (100k), sem alocar nada; restore após 1 escrita {} e {} (caminho quente), após 10 escritas \
         em arquivos sorteados {} e {} (frio); restore após 1 escrita libera {} e {}; 1a escrita pós-snapshot {} e {} quente \
         ({} e {} frio), copiando {} e {}; \
         profundidade 1 vs 32: {} vs {} ({} vs {} copiados); criar em diretório de 10k entradas pós-snapshot {} ({} copiados); \
         imagem de 100k arquivos: {:.0} bytes de estrutura por inode; 100 sandboxes derivadas dela custam {} cada (imagem: {}).",
        us(f.snap_1k),
        us(f.snap_100k),
        us(f.restore_hot_1k),
        us(f.restore_hot_100k),
        us(f.restore_1k),
        us(f.restore_100k),
        kib(f.restore_one_1k),
        kib(f.restore_one_100k),
        us(f.write_hot_1k),
        us(f.write_hot_100k),
        us(f.write_1k),
        us(f.write_100k),
        kib(f.write_bytes_1k),
        kib(f.write_bytes_100k),
        us(f.depth_1),
        us(f.depth_32),
        kib(f.depth_bytes_1),
        kib(f.depth_bytes_32),
        us(f.big_dir_create),
        kib(f.big_dir_bytes),
        f.overhead_per_inode,
        kib(f.per_sandbox),
        kib(f.base_bytes),
    );
    if !maintained {
        notes.push_str(" Descartado: sem manutenção e com advisories RustSec (RUSTSEC-2020-0096, RUSTSEC-2023-0126, RUSTSEC-2026-0248).");
    }
    if !c.write_not_proportional {
        notes.push_str(" Falha o critério central: a primeira escrita depois do snapshot cresce com o número de arquivos.");
    }
    if !c.big_dir_ok {
        notes.push_str(" Diretório grande: a primeira inserção depois do snapshot copia o diretório inteiro.");
    }
    if !c.conformant {
        notes.push_str(" Divergiu do modelo de referência.");
    }
    let version = if r.meta.crates.is_empty() {
        "à mão".to_string()
    } else {
        r.meta.crates.iter().map(|c| format!("{c} {}", locked_version(c))).collect::<Vec<_>>().join(", ")
    };
    let scan: Value = r.meta.crates.iter().map(|c| (c.to_string(), scans.get(*c).cloned().unwrap_or(Value::Null))).collect::<Map<_, _>>().into();
    let category = if r.meta.crates.is_empty() { Some("a".to_string()) } else { scans.get(r.meta.crates[0]).and_then(|s| s["category"].as_str()).map(str::to_string) };
    let cand = CandidateResult {
        name: r.meta.name.to_string(),
        version,
        role: "tmpfs".to_string(),
        category,
        conformance: None,
        fit,
        notes,
        metrics: json!({
            "key": r.meta.key,
            "kind": r.kind,
            "checks": {
                "snapshot_constant": c.snapshot_constant,
                "restore_constant": c.restore_constant,
                "write_not_proportional_to_size": c.write_not_proportional,
                "write_independent_of_dir_depth": c.write_independent_of_depth,
                "shared_memory": c.shared_memory,
                "big_dir_ok": c.big_dir_ok,
                "conformant": c.conformant,
            },
            "timing": r.timing,
            "memory": r.memory,
            "conformance": r.conformance,
            "concurrency": r.concurrency,
            "depscan": scan,
        }),
    };
    (cand, f, c)
}

struct ContentFacts {
    write_ns: f64,
    write_bytes: f64,
    in_place_ns: f64,
    overhead_per_inode: f64,
    huge_overhead: f64,
    read_4k: f64,
    bad: f64,
}

fn content_facts(r: &Record) -> ContentFacts {
    let t = &r.timing;
    let m = r.memory.get("content").cloned().unwrap_or(Value::Null);
    ContentFacts {
        write_ns: num(t, "/huge_file/write_1_byte_after_snapshot_ns/median_ns"),
        write_bytes: num(&m, "/huge_file/write_1_byte_after_snapshot_live_bytes"),
        in_place_ns: num(t, "/huge_file/write_1_byte_in_place_ns/median_ns"),
        overhead_per_inode: num(&m, "/image_100k/overhead_bytes_per_inode"),
        huge_overhead: num(&m, "/huge_file/overhead_bytes"),
        read_4k: num(t, "/huge_file/read_4k_ns/median_ns"),
        bad: num(&r.conformance, "/divergences") + num(&r.conformance, "/fsck_failures"),
    }
}

fn content_candidate(r: &Record, scans: &Value) -> (CandidateResult, ContentFacts) {
    let f = content_facts(r);
    let fit = if f.bad != 0.0 || f.write_bytes > 1024.0 * 1024.0 {
        Fit::DoesNotFit
    } else if f.write_bytes > 64.0 * 1024.0 || r.meta.hand {
        Fit::FitsWithWork
    } else {
        Fit::Fits
    };
    let mut notes = format!(
        "Arquivo de 100 MiB: escrever 1 byte depois de snapshot custa {} e retém {}; no lugar {}; ler 4 KiB {}. \
         Overhead do arquivo grande {}; imagem de 100k arquivos pequenos: {:.0} bytes de overhead por inode (tabela, diretório e conteúdo).",
        us(f.write_ns),
        kib(f.write_bytes),
        us(f.in_place_ns),
        us(f.read_4k),
        kib(f.huge_overhead),
        f.overhead_per_inode,
    );
    if f.write_bytes > 1024.0 * 1024.0 {
        notes.push_str(" Falha: a primeira escrita depois do snapshot copia o arquivo inteiro.");
    } else if f.write_bytes > 64.0 * 1024.0 {
        notes.push_str(" A cópia é do vetor de ponteiros inteiro: proporcional ao tamanho do arquivo (16 bytes por bloco de 4 KiB).");
    }
    let version = if r.meta.crates.is_empty() {
        "à mão (std)".to_string()
    } else {
        r.meta.crates.iter().map(|c| format!("{c} {}", locked_version(c))).collect::<Vec<_>>().join(", ")
    };
    let category = if r.meta.crates.is_empty() { Some("a".to_string()) } else { scans.get(r.meta.crates[0]).and_then(|s| s["category"].as_str()).map(str::to_string) };
    let cand = CandidateResult {
        name: r.meta.name.to_string(),
        version,
        role: "tmpfs".to_string(),
        category,
        conformance: None,
        fit,
        notes,
        metrics: json!({
            "key": r.meta.key,
            "kind": r.kind,
            "timing": r.timing,
            "memory": r.memory,
            "conformance": r.conformance,
        }),
    };
    (cand, f)
}

fn loadavg() -> String {
    std::fs::read_to_string("/proc/loadavg").map(|s| s.trim().to_string()).unwrap_or_default()
}

fn main() -> Result<()> {
    let start = Instant::now();
    let load_start = loadavg();
    let mut res = ExperimentResult::new("e03-tmpfs-persistent", "tmpfs persistente: snapshot, restore e sandbox nova em O(1)");

    eprintln!("[e03] memória (subprocesso com allocator contador)");
    let memory = run_memory()?;

    eprintln!("[e03] conformidade: modelo x tmpfs do host");
    let linux = model_vs_linux();
    eprintln!("[e03] divergências modelo x Linux: {}", linux["divergences"]);

    let mut records: Vec<Record> = Vec::new();
    for_each_structure!(|meta, F| {
        eprintln!("[e03] estrutura: {}", meta.key);
        let conf = ours_vs_model::<F>();
        let timing = structure_timing::<F>();
        records.push(record(meta, "structure", timing, &memory, conf));
    });
    for_each_final!(|meta, F| {
        eprintln!("[e03] combinação final: {}", meta.key);
        let conf = ours_vs_model::<F>();
        let mut timing = structure_timing::<F>();
        timing["huge_file"] = huge_file_timing::<F>();
        timing["image_100k"] = content_image_timing::<F>();
        timing["concurrency"] = concurrency_timing::<F>();
        let mut r = record(meta, "final", timing, &memory, conf);
        r.concurrency = r.timing["concurrency"].clone();
        records.push(r);
    });
    for_each_content!(|meta, F| {
        eprintln!("[e03] conteúdo: {}", meta.key);
        let conf = ours_vs_model::<F>();
        let timing = json!({"huge_file": huge_file_timing::<F>(), "image_100k": content_image_timing::<F>()});
        records.push(record(meta, "content", timing, &memory, conf));
    });
    // Vazão concorrente nas estruturas principais (as finais já mediram acima).
    {
        use e03_tmpfs_persistent::flavors::{HandRadix, ImblHamt, RpdsHamt};
        for (key, value) in [
            ("imbl-hamt", concurrency_timing::<ImblHamt>()),
            ("rpds-hamt", concurrency_timing::<RpdsHamt>()),
            ("hand-radix", concurrency_timing::<HandRadix>()),
        ] {
            eprintln!("[e03] vazão: {key}");
            if let Some(r) = records.iter_mut().find(|r| r.meta.key == key && r.kind == "structure") {
                r.concurrency = value;
            }
        }
    }

    eprintln!("[e03] depscan");
    let mut scans = Map::new();
    for name in ["imbl", "im", "rpds", "immutable-chunkmap", "arc-swap", "archery", "triomphe"] {
        scans.insert(name.to_string(), depscan_of(name));
    }
    let scans = Value::Object(scans);

    let mut best: Vec<(String, StructureFacts)> = Vec::new();
    let mut flat: Option<StructureFacts> = None;
    let mut content_ok: Vec<String> = Vec::new();
    let mut arc_vec: Option<ContentFacts> = None;
    for r in &records {
        match r.kind {
            "structure" | "final" => {
                let (cand, f, c) = structure_candidate(r, &scans);
                let core = c.snapshot_constant && c.restore_constant && c.write_not_proportional && c.shared_memory && c.conformant;
                if r.meta.key == "hand-flat" {
                    flat = Some(f);
                } else if core && cand.fit != Fit::DoesNotFit {
                    best.push((r.meta.key.to_string(), f));
                }
                if r.kind == "final" {
                    let cf = content_facts(r);
                    if cf.write_bytes <= 64.0 * 1024.0 && cf.bad == 0.0 {
                        content_ok.push(r.meta.key.to_string());
                    }
                }
                res.candidates.push(cand);
            }
            _ => {
                let (cand, f) = content_candidate(r, &scans);
                if r.meta.key == "content-arc-vec" {
                    arc_vec = Some(f);
                } else if cand.fit != Fit::DoesNotFit && f.write_bytes <= 64.0 * 1024.0 {
                    content_ok.push(r.meta.key.to_string());
                }
                res.candidates.push(cand);
            }
        }
    }

    let linux_ok = linux["divergences"].as_u64() == Some(0);
    let verdict = if !best.is_empty() && !content_ok.is_empty() && linux_ok {
        Verdict::Confirmed
    } else if !best.is_empty() {
        Verdict::Partial
    } else {
        Verdict::Refuted
    };
    let pick = ["final-imbl-ord", "final-hand-ord", "imbl-btree", "final-hand", "imbl-hamt"]
        .iter()
        .find_map(|want| best.iter().find(|(k, _)| k == want))
        .or(best.first());
    let summary = match (pick, &flat, &arc_vec) {
        (Some((key, f)), Some(fl), Some(av)) => format!(
            "{} candidatos de estrutura cumprem o critério. Em {key}: snapshot {} com 1k e {} com 100k arquivos, sem alocar; \
             restore após 1 escrita {} e {} (caminho quente), após 10 escritas sorteadas {} e {} (frio), liberando {} e {} por escrita; \
             primeira escrita pós-snapshot {} e {} quente ({} e {} frio), copiando {} e {}, \
             sem relação com a profundidade de diretório ({} na profundidade 1 e {} na 32); \
             100 sandboxes derivadas custam {} cada sobre uma imagem de {}. O controle negativo (tabela num Arc<BTreeMap>) também tem \
             snapshot O(1), mas a primeira escrita copia {} com 100k arquivos; e Arc<Vec<u8>> copia {} pra mudar 1 byte de um arquivo de 100 MiB.",
            best.len(),
            us(f.snap_1k),
            us(f.snap_100k),
            us(f.restore_hot_1k),
            us(f.restore_hot_100k),
            us(f.restore_1k),
            us(f.restore_100k),
            kib(f.restore_one_1k),
            kib(f.restore_one_100k),
            us(f.write_hot_1k),
            us(f.write_hot_100k),
            us(f.write_1k),
            us(f.write_100k),
            kib(f.write_bytes_1k),
            kib(f.write_bytes_100k),
            kib(f.depth_bytes_1),
            kib(f.depth_bytes_32),
            kib(f.per_sandbox),
            kib(f.base_bytes),
            kib(fl.write_bytes_100k),
            kib(av.write_bytes),
        ),
        _ => "Nenhum candidato cumpriu todos os critérios; ver métricas por candidato.".to_string(),
    };
    res.hypothesis(
        "H17",
        verdict,
        summary,
        json!({
            "passing_structures": best.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>(),
            "passing_contents": content_ok,
            "model_vs_linux": linux,
        }),
    );
    res.metrics = json!({
        "model_vs_linux": res.hypotheses[0].evidence["model_vs_linux"].clone(),
        "elapsed_s": start.elapsed().as_secs_f64(),
        "loadavg_start": load_start,
        "loadavg_end": loadavg(),
        "sizes": e03_tmpfs_persistent::measure::SIZES,
        "big_dir_entries": e03_tmpfs_persistent::measure::BIG_DIR,
        "huge_file_bytes": e03_tmpfs_persistent::measure::HUGE_FILE,
        "max_ratio_1k_to_100k": MAX_RATIO,
    });
    res.notes.push(
        "Tempos em release no host (allocator do sistema); memória medida em processo separado com stats_alloc (bytes pedidos vivos, \
         sem o overhead do malloc). A máquina é compartilhada com outros agentes (ver loadavg nas métricas): os critérios usam \
         medidas determinísticas (bytes copiados e liberados) e tempos com caminho quente; os tempos frios ficam registrados como \
         o custo realista com falta de cache."
            .to_string(),
    );
    res.notes.push(
        "depscan classifica imbl e rpds como (b) por artefatos: println em código #[cfg(test)] do imbl; getrandom, que aparece \
         porque o cargo metadata unifica features com as dev-dependencies (o proptest liga os_rng no rand_core), mas não está na \
         árvore normal do imbl (cargo tree -p imbl -e normal); e std::process::abort do triomphe em estouro de contador. \
         Nenhum caminho usado pelo tmpfs toca o host."
            .to_string(),
    );
    res.notes.push(
        "fd guarda o ino, não Arc<Inode>: com estrutura persistente o Arc<Inode> é um valor congelado e não veria escritas feitas por \
         outro caminho. Arquivo desvinculado e aberto fica na tabela como órfão até o último close."
            .to_string(),
    );
    res.notes.push(
        "Restore não volta o contador de inos: senão um fd aberto antes do restore passaria a apontar pra outro arquivo com o mesmo ino."
            .to_string(),
    );
    for r in records.iter().filter(|r| !r.concurrency.is_null()) {
        let rows = r.concurrency["rows"].as_array().cloned().unwrap_or_default();
        let line: Vec<String> = rows
            .iter()
            .map(|row| {
                let ops = &row["ops_per_s"];
                let parts: Vec<String> = ops
                    .as_object()
                    .map(|o| o.iter().map(|(k, v)| format!("{k} {:.2} M", v.as_f64().unwrap_or(0.0) / 1e6)).collect())
                    .unwrap_or_default();
                format!("{} threads: {}", row["threads"], parts.join(", "))
            })
            .collect();
        res.notes.push(format!("Vazão (ops/s, mediana de {} rodadas) em {}: {}.", r.concurrency["repetitions"], r.meta.key, line.join("; ")));
    }
    res.notes.push(
        "Recomendação: imbl 7 com OrdMap na tabela de inodes e nos diretórios (readdir já ordenado, cópia de caminho barata em \
         diretório grande) e imbl::Vector de blocos Arc<[u8]> de 4 KiB com o último bloco do tamanho exato; trava RwLock por \
         sandbox, trocando pra fatias da tabela quando houver escritores paralelos na mesma sandbox; nunca RCU pra escrita. \
         A trie de raiz 64 à mão na tabela é uma otimização medida (lookup mais rápido), não uma necessidade."
            .to_string(),
    );
    res.notes.push(
        "readdir: o tmpfs do host (6.12) devolve as entradas do mais novo pro mais antigo; aqui o readdir sai em ordem de nome. \
         Reproduzir a ordem do tmpfs exige um índice por ordem de criação no diretório (o tmpfs usa offsets estáveis num maple \
         tree), fora do escopo do H17."
            .to_string(),
    );
    let path = res.write()?;
    eprintln!("[e03] gravado {} em {:.1} s", path.display(), start.elapsed().as_secs_f64());
    Ok(())
}
