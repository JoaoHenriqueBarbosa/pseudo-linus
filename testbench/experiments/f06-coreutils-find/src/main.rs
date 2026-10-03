//! F06/F07: porte real do uutils (cat, head, wc, sort, ls) e do findutils (find, xargs) pro shim
//! `sysio`, medindo linhas alteradas, bloqueios e conformidade antes e depois do porte.
//!
//! O binário refaz tudo: roda o corpus de coreutils, find e xargs nos utilitários portados (em
//! processo, sobre o VFS do shim), roda os originais no container do oráculo, mede o diff de cada
//! crate vendorizado contra o pristino do registry e grava `results/f06-coreutils-find.json`.

mod builtins;
mod exec;
mod original;
mod porting;
mod sandbox;
mod scoring;
mod shell;

use std::collections::BTreeMap;
use std::time::Instant;

use anyhow::Result;
use harness::{CandidateResult, Case, CaseComparison, ExperimentResult, Fit, Outcome, Verdict, paths};
use serde_json::json;

const EXPERIMENT: &str = "f06-coreutils-find";

/// Um papel disputado: utilitário, suíte do corpus e crate original.
struct Role {
    role: &'static str,
    util: &'static str,
    suite: &'static str,
    original: &'static str,
    version: &'static str,
    ported: &'static str,
}

const ROLES: &[Role] = &[
    Role { role: "coreutils:cat", util: "cat", suite: "coreutils", original: "uu_cat", version: "0.12.0", ported: "port-uu-cat" },
    Role { role: "coreutils:head", util: "head", suite: "coreutils", original: "uu_head", version: "0.12.0", ported: "port-uu-head" },
    Role { role: "coreutils:wc", util: "wc", suite: "coreutils", original: "uu_wc", version: "0.12.0", ported: "port-uu-wc" },
    Role { role: "coreutils:sort", util: "sort", suite: "coreutils", original: "uu_sort", version: "0.12.0", ported: "port-uu-sort" },
    Role { role: "coreutils:ls", util: "ls", suite: "coreutils", original: "uu_ls", version: "0.12.0", ported: "port-uu-ls" },
    Role { role: "find", util: "find", suite: "find", original: "findutils", version: "0.10.0", ported: "port-findutils" },
    Role { role: "xargs", util: "xargs", suite: "xargs", original: "findutils", version: "0.10.0", ported: "port-findutils" },
];

/// O que travou o porte de cada utilitário (levantado lendo e portando o código; cada item tem o
/// comentário "Porte pseudo-linus" correspondente no crate vendorizado).
fn blockers(util: &str) -> Vec<&'static str> {
    match util {
        "cat" => vec![
            "splice(2) via uucore::pipes + rustix: fd do host dos dois lados; saiu (fica o read/write)",
            "is_safe_overwrite: fstat/lseek/fcntl por AsFd + rustix; virou o trait Fstat do shim",
            "RawWriter: write(2) direto no fd 1 (rustix); virou embrulho sobre Write",
            "#[uucore::main] gerava static em .init_array (unsafe link_section) e mexia em SIGPIPE/SIGSEGV/SIGBUS do processo host",
        ],
        "head" => vec![
            "caminho zero-copy (uucore::pipes::send_n_bytes, splice) removido",
            "stdin: dup(2) do fd 0 num std::fs::File pra poder dar seek; ficou o caminho genérico",
        ],
        "wc" => vec![
            "count_bytes_using_splice: splice pra /dev/null (rustix) removido",
            "fstat + page_size (rustix, libc::S_IFREG) trocados por Fstat do shim e página de 4 KiB",
            "static LazyLock POSIXLY_CORRECT: lido uma vez do ambiente do primeiro wc do host",
            "unsafe str::from_utf8_unchecked trocado pela versão checada",
            "SimdPolicy::detect: OnceLock global com GLIBC_TUNABLES do host",
        ],
        "sort" => vec![
            "rayon: par_sort no pool global do host (threads sem contexto do pseudo-processo); build_global do --parallel valia pro host inteiro",
            "threads de leitura/merge (std::thread::spawn) nasciam sem processo corrente; viraram sysio::thread::spawn",
            "tempfile (diretório temporário no FS do host) e ctrlc (handler de SIGINT do processo host + std::process::exit) trocados por diretório no VFS, sem limpeza por sinal",
            "unsafe: transmute de lifetime em chunks.rs (coleta in-place segura no lugar), setlocale e fcntl da libc",
            "getrlimit(NOFILE), /proc/self/fd, access(2), sysinfo(totalram): todos do host",
            "--compress-program: filho com pipe concorrente (fork/exec); sem equivalente no shim, a compressão fica desligada",
            "i18n: COLLATOR, separador decimal, meses (nl_langinfo, unsafe) e locale eram OnceLock globais com o ambiente do primeiro processo",
            "rand::rng() (sort -R) semeava do getrandom(2) do host; virou sysio::random",
        ],
        "ls" => vec![
            "uucore::entries: getpwuid/getgrgid da libc (unsafe, NSS do host); virou leitura do /etc/passwd e /etc/group do VFS",
            "lscolors: a API pública (Colorable) é tipada com std::fs::{FileType, Metadata}, que só nascem de syscall do host; exigiu fork",
            "clap .env(\"TABSIZE\"/\"TIME_STYLE\"): o clap lê o ambiente do processo host",
            "terminal_size (ioctl no fd do host), hostname (gethostname do host), xattr (ACL), statfs (libc) removidos ou trocados",
            "dired: std::env::args_os() lia o argv do processo host",
            "fuso: SystemTime -> Zoned usava TimeZone::system() (TZ e /etc/localtime do host); virou TZ do pseudo-processo com tzdb embutida",
            "métodos inerentes de Path (exists, metadata, symlink_metadata, read_link) tocam o host e não dá pra sombrear por import",
        ],
        "find" => vec![
            "walkdir: DirEntry/Metadata do std e same_file::Handle (fd do host); exigiu fork",
            "onig: Oniguruma em C (FFI) pro glob do -name e pro -regex; trocado por tradutor POSIX -> crate regex (sem retrovisor)",
            "nix (usuários e grupos), faccess (faccessat), argmax + std::process::Command (-exec)",
            "chrono::Local e Utc::now: fuso e relógio do host",
            "std::process::exit no Printer",
        ],
        "xargs" => vec![
            "executor: std::process::Command (fork/exec do host) trocado pela tabela de programas do pseudo-kernel; o resto (leitores, limites, -I, -L, -n, -s) ficou",
            "sysconf(_SC_ARG_MAX) unsafe da libc",
            "std::env::vars_os do processo host",
            "-P já era ignorado no original (roda em série)",
        ],
        _ => Vec::new(),
    }
}

/// Estimativa (não medição) do tempo de agente gasto em cada porte, em minutos, anotada durante o
/// trabalho. O uucore e o uucore_procs são compartilhados por todos os utilitários do uutils.
fn agent_minutes(krate: &str) -> u32 {
    match krate {
        "uucore" | "uucore_procs" => 90,
        "uu_cat" => 15,
        "uu_head" => 10,
        "uu_wc" => 20,
        "uu_sort" => 45,
        "uu_ls" | "lscolors" => 60,
        "findutils" | "walkdir" => 75,
        _ => 0,
    }
}

fn util_tag(case: &Case) -> Option<&'static str> {
    ["cat", "head", "wc", "sort", "ls"].into_iter().find(|u| case.tags.iter().any(|t| t == u))
}

/// O caso entra no papel? argv do próprio utilitário, ou pipeline só com programas da tabela.
fn selects(role: &Role, suite: &str, case: &Case) -> bool {
    if suite != role.suite {
        return false;
    }
    if let Some(prog) = case.argv.first() {
        return prog == role.util;
    }
    let Some(script) = &case.script else { return false };
    if shell::programs(script).is_none() {
        return false;
    }
    match suite {
        "coreutils" => util_tag(case) == Some(role.util),
        _ => true,
    }
}

fn run_ported(case: &Case) -> Outcome {
    let inv = match case.invocation() {
        Ok(inv) => inv,
        Err(e) => return Outcome::unsupported(format!("caso inválido: {e}")),
    };
    match &case.script {
        Some(script) => shell::run(&inv, script),
        None => exec::run_argv(&inv, &case.argv),
    }
}

fn conformance_json(c: &harness::Conformance) -> serde_json::Value {
    json!({
        "total": c.total,
        "strict_pass": c.strict_pass,
        "lenient_pass": c.lenient_pass,
        "unsupported": c.unsupported,
        "strict_rate": (c.strict_rate() * 1000.0).round() / 1000.0,
        "lenient_rate": (c.lenient_rate() * 1000.0).round() / 1000.0,
    })
}

fn main() -> Result<()> {
    // Panic dentro de um pseudo-processo vira resultado do caso, não lixo no stderr da bancada.
    std::panic::set_hook(Box::new(|_| {}));
    let started = Instant::now();
    let mut result = ExperimentResult::new(EXPERIMENT, "uutils (cat, head, wc, sort, ls) e findutils (find, xargs) portados pro shim sysio");

    // 1. Corpus e golden.
    let mut suites: BTreeMap<&str, Vec<(Case, Outcome)>> = BTreeMap::new();
    let mut missing_golden = 0;
    for suite in ["coreutils", "find", "xargs"] {
        let (cases, missing) = paths::load_tool(suite)?;
        missing_golden += missing;
        suites.insert(suite, cases);
    }
    let corpus_size: usize = suites.values().map(Vec::len).sum();

    // 2. Seleção por papel.
    let mut selected: Vec<(usize, &str, &Case, &Outcome)> = Vec::new();
    for (ri, role) in ROLES.iter().enumerate() {
        for (suite, cases) in &suites {
            for (case, golden) in cases {
                if selects(role, suite, case) {
                    selected.push((ri, suite, case, golden));
                }
            }
        }
    }

    // 3. Portados, em processo.
    let t_port = Instant::now();
    let ported: Vec<Outcome> = selected.iter().map(|(_, _, case, _)| run_ported(case)).collect();
    let port_ms = t_port.elapsed().as_millis();

    // 4. Originais, no container (um container pra todos os casos).
    let t_orig = Instant::now();
    let orig_cases: Vec<Case> = selected.iter().map(|(_, _, case, _)| (*case).clone()).collect();
    let originals: Option<Vec<Outcome>> = match original::run(&orig_cases) {
        Ok(o) => Some(o),
        Err(e) => {
            result.notes.push(format!("originais não rodaram no oráculo: {e:#}"));
            None
        }
    };
    let orig_ms = t_orig.elapsed().as_millis();

    // 5. Medidas do porte.
    let pristine = porting::pristine_dirs()?;
    let mut diffs: BTreeMap<String, porting::CrateDiff> = BTreeMap::new();
    let mut audits: BTreeMap<String, porting::Audit> = BTreeMap::new();
    for p in porting::PORTS {
        let Some(pdir) = pristine.get(p.original) else {
            result.notes.push(format!("pristino de {} não encontrado no registry", p.original));
            continue;
        };
        let diff = porting::diff_crate(&format!("{} -> {}", p.original, p.ported), pdir, &p.ported_dir())?;
        let compiled = porting::compiled_sources(p.lib, &p.ported_dir())?;
        let audit = porting::audit(pdir, &p.ported_dir(), &compiled)?;
        diffs.insert(p.original.to_string(), diff);
        audits.insert(p.original.to_string(), audit);
    }
    let shared_uutils: Vec<&str> = vec!["uucore", "uucore_procs"];
    let shared_lines: usize =
        shared_uutils.iter().filter_map(|k| diffs.get(*k)).map(|d| d.rs_inserted + d.rs_deleted).sum();

    // 6. depscan dos originais e dos portes.
    let exp = porting::experiment_dir();
    let mut categories: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for (krate, manifest) in [
        ("uu_cat", "original/Cargo.toml"),
        ("uu_head", "original/Cargo.toml"),
        ("uu_wc", "original/Cargo.toml"),
        ("uu_sort", "original/Cargo.toml"),
        ("uu_ls", "original/Cargo.toml"),
        ("uucore", "original/Cargo.toml"),
        ("findutils", "original/Cargo.toml"),
        ("port-uu-cat", "Cargo.toml"),
        ("port-uu-head", "Cargo.toml"),
        ("port-uu-wc", "Cargo.toml"),
        ("port-uu-sort", "Cargo.toml"),
        ("port-uu-ls", "Cargo.toml"),
        ("port-uucore", "Cargo.toml"),
        ("port-findutils", "Cargo.toml"),
        ("sysio", "Cargo.toml"),
    ] {
        match depscan::scan(&exp.join(manifest), krate) {
            Ok(scan) => {
                categories.insert(
                    krate.to_string(),
                    json!({
                        "own": scan.root.category.letter(),
                        "tree": scan.tree_category.letter(),
                        "own_host_touch": scan.root.counts.host_touch(),
                        "own_unsafe": scan.root.counts.unsafe_total(),
                        "tree_deps": scan.deps.len(),
                        "c_deps": scan.c_deps,
                        "host_touching_deps": scan.host_touching_deps,
                    }),
                );
            }
            Err(e) => {
                categories.insert(krate.to_string(), json!({ "error": format!("{e:#}") }));
            }
        }
    }

    // 6b. Dependências que tocam o host: o que o porte tirou da árvore e o que sobrou.
    let host_deps = |k: &str| -> std::collections::BTreeSet<String> {
        categories
            .get(k)
            .and_then(|v| v.get("host_touching_deps"))
            .and_then(|v| v.as_object())
            .map(|m| m.keys().map(|d| d.rsplit_once(' ').map(|(n, _)| n.to_string()).unwrap_or_else(|| d.clone())).collect())
            .unwrap_or_default()
    };
    let mut dep_changes = serde_json::Map::new();
    for role in ROLES {
        let before = host_deps(role.original);
        let after = host_deps(role.ported);
        dep_changes.insert(
            role.role.to_string(),
            json!({
                "removed": before.difference(&after).collect::<Vec<_>>(),
                "remaining": before.intersection(&after).collect::<Vec<_>>(),
                "added": after.difference(&before).collect::<Vec<_>>(),
            }),
        );
    }

    // 6c. Estado global do uucore: dois utilitários seguidos no mesmo processo host (original) e
    // os mesmos dois como pseudo-processos do shim (porte).
    let demo_original = original::global_state_demo().unwrap_or_else(|e| json!({ "error": format!("{e:#}") }));
    let demo_port = {
        let mut files = harness::MemTree::new();
        files.insert("ok.txt", harness::Entry::file("conteúdo\n", 0o644));
        let mk = |argv: &[&str]| harness::Invocation {
            case_id: "demo".into(),
            argv: argv.iter().map(|s| s.to_string()).collect(),
            script: None,
            stdin: Vec::new(),
            files: files.clone(),
            env: Default::default(),
            faketime: None,
        };
        let wc = exec::run_argv(&mk(&["wc", "nope.txt"]), &["wc".into(), "nope.txt".into()]);
        let cat = exec::run_argv(&mk(&["cat", "ok.txt"]), &["cat".into(), "ok.txt".into()]);
        json!({
            "sequence": ["wc nope.txt", "cat ok.txt"],
            "exit_codes": {"wc_missing_exit": wc.exit, "cat_ok_exit": cat.exit},
            "stderr": String::from_utf8_lossy(wc.stderr.as_slice()),
        })
    };

    // 7. Placar por papel.
    let mut role_metrics = serde_json::Map::new();
    let mut port_rates: BTreeMap<&str, (f64, f64)> = BTreeMap::new();
    for (ri, role) in ROLES.iter().enumerate() {
        let idx: Vec<usize> = selected.iter().enumerate().filter(|(_, s)| s.0 == ri).map(|(i, _)| i).collect();
        let port_cmp: Vec<CaseComparison> =
            idx.iter().map(|&i| scoring::compare(selected[i].2, selected[i].3, &ported[i])).collect();
        let port_conf = scoring::summarize(&format!("{} {} + porte sysio", role.original, role.version), &port_cmp);
        let orig_cmp: Option<Vec<CaseComparison>> = originals
            .as_ref()
            .map(|o| idx.iter().map(|&i| scoring::compare(selected[i].2, selected[i].3, &o[i])).collect());
        let orig_conf = orig_cmp.as_ref().map(|c| scoring::summarize(&format!("{} {} original", role.original, role.version), c));

        // Porte x original, caso a caso: o porte fez exatamente o que o original fez?
        let same_as_original = originals.as_ref().map(|o| {
            idx.iter()
                .filter(|&&i| {
                    let case = selected[i].2;
                    let a = scoring::normalize(case, &ported[i]);
                    let b = scoring::normalize(case, &o[i]);
                    a.unsupported.is_none() && a.stdout == b.stdout && a.stderr == b.stderr && a.exit == b.exit && a.files.diff(&b.files).is_empty()
                })
                .count()
        });
        let divergent_from_original: Vec<String> = originals
            .as_ref()
            .map(|o| {
                idx.iter()
                    .filter(|&&i| {
                        let case = selected[i].2;
                        let a = scoring::normalize(case, &ported[i]);
                        let b = scoring::normalize(case, &o[i]);
                        !(a.stdout == b.stdout && a.stderr == b.stderr && a.exit == b.exit && a.files.diff(&b.files).is_empty())
                    })
                    .map(|&i| {
                        let why = ported[i].unsupported.clone().unwrap_or_else(|| "saída diferente".into());
                        format!("{}: {why}", selected[i].2.id)
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut util_diff = diffs.get(role.original).cloned().unwrap_or_default();
        let util_audit = audits.get(role.original).cloned().unwrap_or_default();
        // find e xargs moram no mesmo crate: cada papel conta só os arquivos dele (o lib.rs fica no find).
        if role.original == "findutils" {
            let keep = |path: &str| match role.util {
                "xargs" => path.starts_with("src/xargs/"),
                _ => !path.starts_with("src/xargs/"),
            };
            util_diff.rs_inserted = util_diff.files.iter().filter(|f| keep(&f.path)).map(|f| f.inserted).sum();
            util_diff.rs_deleted = util_diff.files.iter().filter(|f| keep(&f.path)).map(|f| f.deleted).sum();
        }
        let util_lines = util_diff.rs_inserted + util_diff.rs_deleted;
        let shared_note = if role.suite == "coreutils" {
            format!("; uucore+uucore_procs (compartilhados): {shared_lines} linhas")
        } else {
            let walk = diffs.get("walkdir").map(|d| d.rs_inserted + d.rs_deleted).unwrap_or(0);
            let core = diffs.get("uucore").map(|d| d.rs_inserted + d.rs_deleted).unwrap_or(0);
            format!("; walkdir: {walk} linhas; usa o uucore 0.12 portado ({core} linhas, compartilhadas)")
        };
        let extra = if role.util == "ls" {
            let lc = diffs.get("lscolors").map(|d| d.rs_inserted + d.rs_deleted).unwrap_or(0);
            format!("; fork do lscolors: {lc} linhas")
        } else {
            String::new()
        };
        let pr = port_conf.strict_rate();
        let or = orig_conf.as_ref().map(|c| c.strict_rate());
        port_rates.insert(role.role, (pr, or.unwrap_or(f64::NAN)));

        role_metrics.insert(
            role.role.to_string(),
            json!({
                "cases": idx.len(),
                "ported": conformance_json(&port_conf),
                "original": orig_conf.as_ref().map(conformance_json),
                "ported_same_as_original": same_as_original,
                "divergent_from_original": divergent_from_original,
                "lines_changed_rs": util_lines,
                "rs_inserted": util_diff.rs_inserted,
                "rs_deleted": util_diff.rs_deleted,
                "manifest_lines_changed": util_diff.manifest_inserted + util_diff.manifest_deleted,
                "compiled_files": util_audit.files,
                "compiled_pristine_lines": util_audit.compiled_pristine_lines,
                "host_touch_before": util_audit.host_touch_before,
                "host_touch_after": util_audit.host_touch_after,
                "unsafe_before": util_audit.unsafe_before,
                "unsafe_after": util_audit.unsafe_after,
                "path_method_calls_replaced": util_audit.path_method_calls_replaced,
                "global_statics_removed": util_audit.global_statics_removed,
                "approx_agent_minutes": agent_minutes(role.original),
                "blockers": blockers(role.util),
            }),
        );

        let orig_category = categories.get(role.original).and_then(|v| v.get("tree")).and_then(|v| v.as_str()).map(String::from);
        let port_category = categories.get(role.ported).and_then(|v| v.get("own")).and_then(|v| v.as_str()).map(String::from);
        let orig_text = orig_conf
            .as_ref()
            .map(|c| format!("{}/{} estrito, {}/{} leniente", c.strict_pass, c.total, c.lenient_pass, c.total))
            .unwrap_or_else(|| "não rodou".into());
        result.candidates.push(CandidateResult {
            name: format!("{} {} (original, sem porte)", role.original, role.version),
            version: role.version.to_string(),
            role: role.role.to_string(),
            category: orig_category,
            conformance: orig_conf.clone(),
            fit: Fit::DoesNotFit,
            notes: format!(
                "Rodado no oráculo com o mesmo argv/script ({orig_text}). Não serve como dependência direta: faz I/O de host (std::fs, stdout, libc/rustix/nix), tem estado global no uucore e unsafe; os bloqueios estão no candidato portado."
            ),
            metrics: json!({ "category_detail": categories.get(role.original) }),
        });
        result.candidates.push(CandidateResult {
            name: format!("{} {} + porte sysio", role.original, role.version),
            version: role.version.to_string(),
            role: role.role.to_string(),
            category: port_category,
            conformance: Some(port_conf.clone()),
            fit: Fit::FitsWithWork,
            notes: format!(
                "{util_lines} linhas alteradas no crate do utilitário ({} +, {} -){shared_note}{extra}. Toque no host nos arquivos compilados: {} -> {}; unsafe: {} -> {}. Igual ao original em {}/{} casos. Bloqueios: {}.",
                util_diff.rs_inserted,
                util_diff.rs_deleted,
                util_audit.host_touch_before,
                util_audit.host_touch_after,
                util_audit.unsafe_before,
                util_audit.unsafe_after,
                same_as_original.map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
                idx.len(),
                blockers(role.util).join("; ")
            ),
            metrics: role_metrics.get(role.role).cloned().unwrap_or_default(),
        });
    }

    // 8. Vereditos.
    let uutils_roles = ["coreutils:cat", "coreutils:head", "coreutils:wc", "coreutils:sort", "coreutils:ls"];
    let util_lines_total: usize = ["uu_cat", "uu_head", "uu_wc", "uu_sort", "uu_ls"]
        .iter()
        .filter_map(|k| diffs.get(*k))
        .map(|d| d.rs_inserted + d.rs_deleted)
        .sum();
    let util_compiled_total: usize =
        ["uu_cat", "uu_head", "uu_wc", "uu_sort", "uu_ls"].iter().filter_map(|k| audits.get(*k)).map(|a| a.compiled_pristine_lines).sum();
    let core_audit = audits.get("uucore").cloned().unwrap_or_default();
    let core_diff = diffs.get("uucore").cloned().unwrap_or_default();
    let survived = if core_audit.compiled_pristine_lines > 0 {
        core_audit.compiled_unchanged_lines as f64 / core_audit.compiled_pristine_lines as f64
    } else {
        0.0
    };
    let port_equal_or_better = uutils_roles.iter().all(|r| {
        let (p, o) = port_rates.get(r).copied().unwrap_or((0.0, f64::NAN));
        o.is_nan() || p + 1e-9 >= o
    });
    let unsafe_after_total: usize = audits.values().map(|a| a.unsafe_after).sum();
    let host_after_total: usize = audits.values().map(|a| a.host_touch_after).sum();
    let statics_total: isize = audits.values().map(|a| a.global_statics_removed).sum();
    let path_total: isize = audits.values().map(|a| a.path_method_calls_replaced).sum();
    let same = |r: &str| -> String {
        let m = role_metrics.get(r);
        let s = m.and_then(|m| m.get("ported_same_as_original")).and_then(|v| v.as_u64());
        let n = m.and_then(|m| m.get("cases")).and_then(|v| v.as_u64()).unwrap_or(0);
        let div: Vec<String> = m
            .and_then(|m| m.get("divergent_from_original"))
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        match s {
            Some(s) if div.is_empty() => format!("{s}/{n}"),
            Some(s) => format!("{s}/{n} (fora: {})", div.join("; ")),
            None => "original não rodou".into(),
        }
    };
    let same_uutils: Vec<String> =
        uutils_roles.iter().map(|r| format!("{} {}", r.trim_start_matches("coreutils:"), same(r))).collect();
    result.hypothesis(
        "H28",
        Verdict::Partial,
        format!(
            "O porte funciona e não custa conformidade: o portado faz exatamente o que o original faz no oráculo ({}; portado >= original: {port_equal_or_better}), e o código de cada utilitário muda pouco ({util_lines_total} linhas inseridas+removidas sobre {util_compiled_total} compiladas). Mas não é 'só trocar std::fs': o uucore exigiu {} linhas ({:.0}% das {} linhas compiladas dele ficaram intactas), {statics_total} statics de estado global viraram estado do pseudo-processo, {path_total} chamadas de métodos de Path (que não dá pra sombrear por import) foram reescritas, e uucore, uucore_procs e lscolors viraram fork; std não é patchável.",
            same_uutils.join(", "),
            core_diff.rs_inserted + core_diff.rs_deleted,
            survived * 100.0,
            core_audit.compiled_pristine_lines,
        ),
        json!({
            "util_lines_changed": util_lines_total,
            "util_compiled_lines": util_compiled_total,
            "uucore_lines_changed": core_diff.rs_inserted + core_diff.rs_deleted,
            "uucore_compiled_lines": core_audit.compiled_pristine_lines,
            "uucore_survival_rate": (survived * 1000.0).round() / 1000.0,
            "global_statics_removed": statics_total,
            "path_method_calls_replaced": path_total,
            "unsafe_after_in_compiled_ported_files": unsafe_after_total,
            "host_touch_after_in_compiled_ported_files": host_after_total,
            "conformance": role_metrics,
        }),
    );
    let find_rates = port_rates.get("find").copied().unwrap_or((0.0, f64::NAN));
    let xargs_rates = port_rates.get("xargs").copied().unwrap_or((0.0, f64::NAN));
    let fu_lines = diffs.get("findutils").map(|d| d.rs_inserted + d.rs_deleted).unwrap_or(0);
    let fu_compiled = audits.get("findutils").map(|a| a.compiled_pristine_lines).unwrap_or(0);
    let wd_lines = diffs.get("walkdir").map(|d| d.rs_inserted + d.rs_deleted).unwrap_or(0);
    result.hypothesis(
        "H29",
        Verdict::Partial,
        format!(
            "find portado: {:.0}% estrito contra o GNU (original no oráculo: {:.0}%), igual ao original em {}; xargs portado: {:.0}% (original: {:.0}%), igual ao original em {}. O porte custou {fu_lines} linhas no findutils ({fu_compiled} compiladas) e {wd_lines} no fork do walkdir, com um tradutor POSIX -> regex no lugar do onig (C). Serve de ponto de partida pro find; no xargs o executor é nosso e sobra a leitura de argumentos e os limites. O que falha contra o GNU é do próprio findutils (formato das mensagens de erro e do -t), não do porte.",
            find_rates.0 * 100.0,
            find_rates.1 * 100.0,
            same("find"),
            xargs_rates.0 * 100.0,
            xargs_rates.1 * 100.0,
            same("xargs"),
        ),
        json!({
            "findutils_lines_changed": fu_lines,
            "findutils_compiled_lines": fu_compiled,
            "walkdir_lines_changed": wd_lines,
            "find": role_metrics.get("find"),
            "xargs": role_metrics.get("xargs"),
        }),
    );

    // 9. Métricas gerais.
    result.metrics = json!({
        "corpus": {
            "cases_total": corpus_size,
            "cases_without_golden": missing_golden,
            "cases_selected": selected.len(),
        },
        "timing_ms": {
            "ported_in_process": port_ms,
            "original_in_container": orig_ms,
            "total": started.elapsed().as_millis(),
        },
        "diffs": diffs,
        "audits": audits,
        "depscan": categories,
        "host_dependency_changes": dep_changes,
        "global_state_demo": { "original_same_host_process": demo_original, "ported_pseudo_processes": demo_port },
        "agent_minutes_note": "estimativa do tempo de agente gasto em cada porte, anotada durante o trabalho; não é medição",
    });
    result.notes.push(
        "Casos 'unordered' (find e xargs -P) comparados como multiconjunto de registros: a ordem do readdir do oráculo (ext4 sob overlay) não é semântica e o VFS do shim lista em ordem de bytes.".into(),
    );
    result.notes.push(
        "Casos script só entram quando são pipelines de programas da tabela (cat, head, wc, sort, ls, find, xargs, echo); o resto do corpus de coreutils (cp, mv, stat, date...) fica pros experimentos que vão implementar esses comandos.".into(),
    );
    result.notes.push(
        "O echo usado como comando padrão do xargs e no find -exec é um builtin escrito à mão na bancada (src/builtins.rs), no oráculo é o echo do Debian.".into(),
    );
    let path = result.write()?;
    println!("gravado {}", path.display());
    for (role, m) in &role_metrics {
        println!(
            "{role:16} casos={:3} portado={} original={} igual_ao_original={} linhas={}",
            m["cases"],
            m["ported"]["strict_pass"],
            m["original"]["strict_pass"],
            m["ported_same_as_original"],
            m["lines_changed_rs"],
        );
    }
    println!("tempo total: {:.1}s", started.elapsed().as_secs_f64());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness::{Entry, MemTree};

    fn inv(argv: &[&str], stdin: &str, files: MemTree) -> harness::Invocation {
        harness::Invocation {
            case_id: "t".into(),
            argv: argv.iter().map(|s| s.to_string()).collect(),
            script: None,
            stdin: stdin.as_bytes().to_vec(),
            files,
            env: Default::default(),
            faketime: Some("2026-01-15 12:00:00".into()),
        }
    }

    #[test]
    fn ported_utils_run_on_the_vfs() {
        let mut files = MemTree::new();
        files.insert("a.txt", Entry::file("b\na\n", 0o644));
        let out = exec::run_argv(&inv(&["sort", "a.txt"], "", files.clone()), &["sort".into(), "a.txt".into()]);
        assert_eq!(out.stdout.as_slice(), b"a\nb\n", "{out:?}");
        let out = exec::run_argv(&inv(&["cat", "nope"], "", files.clone()), &["cat".into(), "nope".into()]);
        assert_eq!(out.exit, Some(1));
        assert_eq!(out.stderr.as_slice(), b"cat: nope: No such file or directory\n");
        let out = exec::run_argv(&inv(&["wc", "-l"], "x\ny\n", files), &["wc".into(), "-l".into()]);
        assert_eq!(out.stdout.as_slice(), b"2\n");
    }

    #[test]
    fn exit_code_and_util_name_are_per_process() {
        // Dois processos seguidos no mesmo host: o EXIT_CODE e o util_name() do uucore não vazam.
        let files = MemTree::new();
        let bad = exec::run_argv(&inv(&["head", "nope"], "", files.clone()), &["head".into(), "nope".into()]);
        assert_eq!(bad.exit, Some(1));
        let ok = exec::run_argv(&inv(&["wc", "-c"], "abc", files), &["wc".into(), "-c".into()]);
        assert_eq!(ok.exit, Some(0));
        assert_eq!(ok.stdout.as_slice(), b"3\n");
    }

    #[test]
    fn find_globs_and_regex_without_onig() {
        // O glob do -name e o -regex passam pelo tradutor POSIX -> crate regex (posix_re.rs).
        let mut files = MemTree::new();
        for f in ["a.rs", "B.RS", "z.rs", "d/c.rs", "]x", "n1", "n22"] {
            files.insert(f, Entry::file("", 0o644));
        }
        let run = |args: &[&str]| {
            let mut argv = vec!["find"];
            argv.extend_from_slice(args);
            let out = exec::run_argv(&inv(&argv, "", files.clone()), &argv.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            let mut lines: Vec<String> = String::from_utf8_lossy(out.stdout.as_slice()).lines().map(String::from).collect();
            lines.sort();
            lines
        };
        assert_eq!(run(&[".", "-regex", r".*/[a-c]\.rs"]), vec!["./a.rs", "./d/c.rs"]);
        assert_eq!(run(&[".", "-iname", "*.rs"]), vec!["./B.RS", "./a.rs", "./d/c.rs", "./z.rs"]);
        assert_eq!(run(&[".", "-name", "[]]x"]), vec!["./]x"]);
        assert_eq!(run(&[".", "-regextype", "posix-extended", "-regex", r"\./n[[:digit:]]{2}"]), vec!["./n22"]);
        assert_eq!(run(&[".", "-regex", r".*\(1\|22\)"]), vec!["./n1", "./n22"]);
    }

    #[test]
    fn find_exec_and_xargs_use_the_program_table() {
        let mut files = MemTree::new();
        files.insert("d/a.txt", Entry::file("1\n2\n", 0o644));
        let out = shell::run(&inv(&[], "", files), "find d -name '*.txt' -print0 | xargs -0 wc -l");
        assert_eq!(out.stdout.as_slice(), b"2 d/a.txt\n", "{out:?}");
    }
}
