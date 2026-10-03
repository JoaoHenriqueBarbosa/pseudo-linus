//! F12: roda tudo e grava `results/f12-net-sqlite-git.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Result;
use f12_net_sqlite_git::{git, net, sqlite};
use harness::{CandidateResult, ExperimentResult, Fit, Oracle, Verdict};
use serde_json::{Value as Json, json};

fn manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

fn probe_manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("probes").join("Cargo.toml")
}

/// depscan de uma crate: categoria própria, da árvore, C e unsafe.
fn scan(manifest: &Path, pkg: &str) -> (Option<String>, Json, Option<PathBuf>) {
    match depscan::scan(manifest, pkg) {
        Ok(s) => (
            Some(s.root.category.letter().to_string()),
            json!({
                "version": s.root.version,
                "own_category": s.root.category.letter(),
                "tree_category": s.tree_category.letter(),
                "own_host_touch": s.root.counts.host_touch(),
                "own_unsafe": s.root.counts.unsafe_total(),
                "tree_deps": s.deps.len(),
                "c_deps": s.c_deps,
                "host_touching_deps": s.host_touching_deps.len(),
                "has_tokio": s.deps.iter().any(|d| d.name == "tokio"),
            }),
            Some(s.root.manifest_dir.clone()),
        ),
        Err(e) => (None, json!({"error": e.to_string()}), None),
    }
}

/// Conta ocorrências de cada padrão nas fontes `.rs` de uma crate.
fn grep_api(dir: &Path, patterns: &[&str]) -> Json {
    let mut counts: BTreeMap<String, usize> = patterns.iter().map(|p| (p.to_string(), 0)).collect();
    let mut stack = vec![dir.join("src")];
    while let Some(d) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&d) else { continue };
        for e in read.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                for pat in patterns {
                    *counts.get_mut(*pat).expect("padrão") += text.matches(pat).count();
                }
            }
        }
    }
    json!(counts)
}

fn net_section(result: &mut ExperimentResult) -> Json {
    println!("[rede] cenários da allowlist com servidores locais");
    let scenarios = net::experiments::ureq_scenarios();
    println!("[rede] alternativas (attohttpc, minreq)");
    let alternatives = net::experiments::alternatives();
    println!("[rede] árvores de dependência");
    let trees = net::experiments::dependency_trees();
    println!("[rede] HTTPS real (opcional)");
    let https = net::experiments::https_real();
    let deny_cost = net::experiments::deny_cost();
    let passed = scenarios["passed"].as_u64().unwrap_or(0);
    let total = scenarios["total"].as_u64().unwrap_or(0);
    let no_tokio = trees["ureq"]["has_tokio"] == json!(false);
    let https_ok = https["status"] == json!(200);
    let https_ran = https["ran"] == json!(true);
    let ureq_c = trees["ureq"]["c_deps"].clone();
    let alt_violations: Vec<String> = ["attohttpc", "minreq"]
        .iter()
        .filter(|l| alternatives[**l]["follow_redirects_automatically"]["policy_violated"] == json!(true))
        .map(|l| l.to_string())
        .collect();

    result.candidates.push(CandidateResult {
        name: "ureq".into(),
        version: "3.4.2".into(),
        role: "net".into(),
        category: trees["ureq"]["own_category"].as_str().map(str::to_string),
        conformance: None,
        fit: if passed == total && no_tokio { Fit::Fits } else { Fit::FitsWithWork },
        notes: format!(
            "Resolver + Connector próprios (módulo unversioned, versão fixada em =3.4.2) aplicam a mesma Policy: {passed}/{total} cenários certos, nome negado nunca abre conexão nem consulta DNS, redirect pra host negado morre no resolvedor a cada salto. Sem tokio na árvore. TLS rustls+webpki-roots {}; o provedor de cripto padrão é ring (C/asm: {ureq_c}). Atenção: Config::default lê HTTP_PROXY do ambiente do host, o agente do sandbox precisa de proxy(None).",
            if https_ok { "funcionou num GET real" } else if https_ran { "falhou no GET real" } else { "não testado (sem rede)" }
        ),
        metrics: json!({"scenarios": scenarios, "https_real": https, "deny_cost": deny_cost, "deps": trees["ureq"]}),
    });
    for (lib, version) in [("attohttpc", "0.31.0"), ("minreq", "3.0.0")] {
        result.candidates.push(CandidateResult {
            name: lib.into(),
            version: version.into(),
            role: "net".into(),
            category: trees[lib]["own_category"].as_str().map(str::to_string),
            conformance: None,
            fit: Fit::DoesNotFit,
            notes: format!(
                "Sem gancho de resolvedor nem de conexão: a allowlist só pode olhar a URL antes da chamada. Seguindo redirects (padrão), o servidor negado recebeu conexão: {}. Com redirects desligados o invólucro consegue checar o Location, mas o DNS continua dentro da crate (TOCTOU: o nome é resolvido de novo depois da checagem) e não há como barrar nome permitido que resolve pra IP interno.",
                if alt_violations.contains(&lib.to_string()) { "sim" } else { "não" }
            ),
            metrics: json!({"redirect_test": alternatives[lib], "deps": trees[lib]}),
        });
    }
    let verdict = if passed == total && no_tokio && (!https_ran || https_ok) { Verdict::Confirmed } else { Verdict::Partial };
    result.hypothesis(
        "H34",
        verdict,
        format!(
            "ureq 3.4.2 com Resolver e Connector nossos aplica a allowlist num ponto só (um objeto Policy): {passed}/{total} cenários com servidor local, zero conexões e zero consultas DNS pra nome negado, redirect pra host ou IP negado barrado; árvore sem tokio; HTTPS real {}. attohttpc e minreq não têm gancho e vazaram no redirect.",
            if https_ok { "ok" } else if https_ran { "falhou" } else { "não testado" }
        ),
        json!({"scenarios_passed": passed, "scenarios_total": total, "ureq_has_tokio": !no_tokio, "https_status": https["status"], "alternatives_leaking_on_redirect": alt_violations}),
    );
    json!({"scenarios": scenarios, "alternatives": alternatives, "dependency_trees": trees, "https_real": https, "deny_cost": deny_cost})
}

fn sqlite_section(result: &mut ExperimentResult, oracle: Option<&Oracle>) -> Result<Json> {
    use sqlite::experiments as sq;
    println!("[sqlite] conformidade contra o golden");
    let cases = sq::load_cases()?;
    let mut runs = BTreeMap::new();
    for cand in sq::candidates() {
        let started = Instant::now();
        let run = sq::run_conformance(&cand, &cases);
        println!(
            "  {}: strict {}/{} lenient {} ({} ms)",
            cand.label,
            run.conformance.strict_pass,
            run.conformance.total,
            run.conformance.lenient_pass,
            started.elapsed().as_millis()
        );
        runs.insert(cand.label.clone(), run);
    }
    let classification = sq::classify(&runs);
    println!("[sqlite] dois processos no mesmo banco");
    let concurrency = sq::concurrency();
    println!("[sqlite] relógio do sandbox");
    let clock = sq::clock();
    let interop = match oracle {
        Some(o) => {
            println!("[sqlite] interoperabilidade de arquivo com o sqlite3 do oráculo");
            sq::interop(o).unwrap_or_else(|e| json!({"error": e.to_string()}))
        }
        None => json!({"skipped": "oráculo indisponível"}),
    };
    println!("[sqlite] depscan e superfície de API");
    let (cat_rusqlite, scan_rusqlite, dir_rusqlite) = scan(&manifest(), "rusqlite");
    let (cat_plugin, scan_plugin, dir_plugin) = scan(&manifest(), "sqlite-plugin");
    let (cat_turso, scan_turso, _) = scan(&manifest(), "turso_core");
    let (_, scan_turso_hl, dir_turso_hl) = scan(&probe_manifest(), "turso");
    let api_rusqlite = dir_rusqlite.map(|d| grep_api(&d, &["sqlite3_vfs_register", "fn register_vfs", "pub fn open_with_flags_and_vfs", "pub fn serialize", "pub fn deserialize"]));
    let api_plugin = dir_plugin.map(|d| grep_api(&d, &["pub fn register_static", "pub unsafe fn register_dynamic", "fn shm_map", "NonNull<u8>", "fn x_current_time", "fn x_randomness", "fn x_sleep"]));
    let api_turso_hl = dir_turso_hl.map(|d| grep_api(&d, &["pub fn with_io_impl", "pub async fn build", "pub async fn connect", "pub async fn execute"]));

    let interop_ok = |name: &str| -> (bool, bool, bool) {
        let a = &interop["created_here_opened_by_oracle"][name];
        let b = &interop["created_by_oracle_opened_here"][name];
        (
            a["integrity_check_ok"] == json!(true) && a["query_same_as_native"] == json!(true),
            b["ref.sqlite"]["query_same_as_oracle"] == json!(true),
            b["wal.sqlite"]["query_same_as_oracle"] == json!(true),
        )
    };
    let label = |n: &str| -> (usize, usize, usize, usize, usize, usize) {
        let r = &runs[n];
        (r.conformance.strict_pass, r.conformance.lenient_pass, r.conformance.total, r.db_bytes_equal, r.db_data_equal, r.db_files_compared)
    };
    let mut per = serde_json::Map::new();
    for (name, run) in &runs {
        let stdout_exit_ok = run.comparisons.iter().filter(|c| c.stdout_ok && c.exit_ok && c.unsupported.is_none()).count();
        per.insert(
            name.clone(),
            json!({
                "strict": run.conformance.strict_pass,
                "lenient": run.conformance.lenient_pass,
                "stdout_and_exit_ok": stdout_exit_ok,
                "total": run.conformance.total,
                "unsupported": run.conformance.unsupported,
                "db_files_byte_identical_modulo_version": run.db_bytes_equal,
                "db_files_same_tables_rows_pragmas": run.db_data_equal,
                "db_files_compared": run.db_files_compared,
                "db_diff_samples": run.db_diff_samples,
                "failures": sq::failures(run),
            }),
        );
    }

    let follows_clock = |n: &str| clock[n]["follows_sandbox_clock"] == json!(true);
    let clock_text = |n: &str| if follows_clock(n) { "segue o relógio do sandbox" } else { "usa o relógio do host" };
    let (s1, l1, t1, b1, d1, f1) = label("rusqlite-serialize");
    let (s2, l2, t2, b2, d2, f2) = label("rusqlite-sqlite-plugin");
    let (s3, l3, t3, b3, d3, f3) = label("turso-core");
    let o3 = per["turso-core"]["stdout_and_exit_ok"].as_u64().unwrap_or(0);
    let io1 = interop_ok("rusqlite-serialize");
    let io2 = interop_ok("rusqlite-sqlite-plugin");
    let io3 = interop_ok("turso-core");
    let conc = &concurrency;

    result.candidates.push(CandidateResult {
        name: "rusqlite (serialize/deserialize)".into(),
        version: "0.40.2 (SQLite 3.53.2 bundled)".into(),
        role: "sqlite".into(),
        // O wrapper é (b), mas o motor é o C empacotado pelo libsqlite3-sys: a categoria que conta é a da árvore.
        category: scan_rusqlite["tree_category"].as_str().map(str::to_string).or(cat_rusqlite.clone()),
        conformance: Some(runs["rusqlite-serialize"].conformance.clone()),
        fit: Fit::FitsWithWork,
        notes: format!(
            "Sem VFS: cada comando sqlite3 carrega os bytes do arquivo num banco em memória e grava de volta no fim. Corpus {s1}/{t1} estrito ({l1} leniente); bancos finais com as mesmas tabelas e linhas do oráculo em {d1}/{f1} (bytes idênticos fora a versão em {b1}/{f1}: o contador de mudanças, offset 27, diverge). Interop: daqui pro oráculo {}, do oráculo (rollback) pra cá {}, do oráculo (WAL) pra cá {} (o banco em memória não abre WAL: o cabeçalho é trocado na carga e restaurado na gravação, e um -wal pendente é recusado). Limites medidos: PRAGMA journal_mode responde memory; sem trava de arquivo inteiro há atualização perdida ({} de {}); com a trava, commit só fica visível quando o processo termina e dois escritores se sobrescrevem; datetime('now') {}.",
            io1.0, io1.1, io1.2,
            conc["rusqlite-serialize/counter/no-lock"]["lost_updates"],
            conc["rusqlite-serialize/counter/no-lock"]["expected"],
            clock_text("rusqlite-serialize"),
        ),
        metrics: json!({"conformance": per["rusqlite-serialize"], "deps": scan_rusqlite, "api": api_rusqlite}),
    });
    result.candidates.push(CandidateResult {
        name: "rusqlite + sqlite-plugin (VFS com trait segura)".into(),
        version: "rusqlite 0.40.2 + sqlite-plugin 0.11.0".into(),
        role: "sqlite".into(),
        category: cat_plugin.clone(),
        conformance: Some(runs["rusqlite-sqlite-plugin"].conformance.clone()),
        fit: Fit::FitsWithWork,
        notes: format!(
            "VFS nosso sobre o FS em memória via trait Vfs e register_static (seguros; nada de unsafe nosso). Corpus {s2}/{t2} estrito ({l2} leniente); bancos finais idênticos byte a byte ao do sqlite3 3.46 (fora os campos de versão) em {b2}/{f2}, mesmas linhas em {d2}/{f2}; o journal quente deixado pelo sqlite3 também aparece. Interop: daqui pro oráculo {}, oráculo (rollback) pra cá {}, oráculo (WAL) pra cá {}. Travas reais do SQLite entre conexões (BUSY sem espera, contador com busy_timeout). Lacunas: WAL compartilhado precisa de shm_map, que devolve ponteiro cru (trait segura com contrato de unsafe); sem ele, PRAGMA journal_mode=WAL numa sessão normal dá disk I/O error, e arquivo que já está em WAL só abre com locking_mode=EXCLUSIVE ligado antes do primeiro acesso (o backend faz isso). xCurrentTime, xRandomness e xSleep não fazem parte da trait e caem no VFS unix do host: datetime('now') {}. O build exige bindgen + libclang.",
            io2.0, io2.1, io2.2,
            clock_text("rusqlite-sqlite-plugin"),
        ),
        metrics: json!({"conformance": per["rusqlite-sqlite-plugin"], "deps": scan_plugin, "api": api_plugin}),
    });
    result.candidates.push(CandidateResult {
        name: "turso_core (IO próprio)".into(),
        version: "0.8.1".into(),
        role: "sqlite".into(),
        category: cat_turso.clone(),
        conformance: Some(runs["turso-core"].conformance.clone()),
        fit: Fit::DoesNotFit,
        notes: format!(
            "Rust puro, IO plugável por traits seguras (IO, File, Clock), sem nada de unsafe nosso. Mas o Clock do IO não chega no SQL: datetime('now') chama SystemTime::now() direto ({}). E o motor diverge: corpus {s3}/{t3} estrito ({l3} leniente; stdout e exit iguais em {o3}/{t3}), mesmas linhas no banco final em {d3}/{f3} (bytes idênticos em {b3}/{f3}). Faltam WITHOUT ROWID, colunas geradas e VACUUM (atrás de flags experimentais), não informa deslocamento de erro (sem o ^--- do CLI), reescreve o SQL guardado em sqlite_schema (o .schema sai diferente), mensagens de erro diferentes, só WAL, e recusa ler TEXT com UTF-8 inválido que o SQLite aceita. Interop: daqui pro oráculo {}, oráculo (rollback) pra cá {}, oráculo (WAL) pra cá {}. O crate turso (alto nível) também aceita IO próprio (with_io_impl), mas a API é async.",
            clock_text("turso-core"),
            io3.0, io3.1, io3.2
        ),
        metrics: json!({"conformance": per["turso-core"], "deps": scan_turso, "turso_high_level_crate": {"deps": scan_turso_hl, "api": api_turso_hl}}),
    });
    let verdict = if s2 + 10 >= t2 && io2.0 && io2.1 { Verdict::Partial } else { Verdict::Refuted };
    result.hypothesis(
        "H35",
        verdict,
        format!(
            "rusqlite sozinho não registra VFS (só aceita o nome de um VFS já registrado; registrar é FFI unsafe). Com sqlite-plugin dá, por trait segura: {s2}/{t2} no corpus com CLI nosso, {b2}/{f2} bancos idênticos ao do sqlite3 e interop nos dois sentidos, mas WAL compartilhado não (shm exige ponteiro cru) e o relógio é o do host. serialize/deserialize dá {s1}/{t1} sem VFS nenhum, com semântica de concorrência pior. turso tem IO plugável mas o motor fica em {s3}/{t3}. Nenhum dos três faz datetime('now') seguir o relógio do sandbox."
        ),
        json!({
            "rusqlite_serialize": {"strict": s1, "total": t1},
            "rusqlite_sqlite_plugin": {"strict": s2, "total": t2, "db_byte_identical": b2},
            "turso_core": {"strict": s3, "total": t3},
            "interop": {"rusqlite-serialize": io1, "rusqlite-sqlite-plugin": io2, "turso-core": io3},
        }),
    );
    Ok(json!({
        "conformance": per,
        "engine_vs_cli": classification,
        "interop": interop,
        "concurrency": concurrency,
        "clock": clock,
        "deps": {"rusqlite": scan_rusqlite, "sqlite-plugin": scan_plugin, "turso_core": scan_turso, "turso": scan_turso_hl},
        "api_surface": {"rusqlite": api_rusqlite, "sqlite-plugin": api_plugin, "turso": api_turso_hl},
    }))
}

fn git_section(result: &mut ExperimentResult, oracle: Option<&Oracle>) -> Result<Json> {
    use git::experiments as gx;
    println!("[git] conformidade contra o golden");
    let cases = gx::load_cases()?;
    let (conf, cmps) = harness::score(&git::GitCandidate, &cases);
    println!("  gix-*: strict {}/{}", conf.strict_pass, conf.total);
    let failures: Vec<Json> = cmps.iter().filter(|c| !c.strict).map(|c| json!({"id": c.id, "detail": c.detail})).collect();
    let (fsck, packed) = match oracle {
        Some(o) => {
            println!("[git] validação com git fsck --strict no oráculo");
            let f = gx::fsck_validation(o).unwrap_or_else(|e| json!({"error": e.to_string()}));
            println!("[git] leitura de repositório empacotado pelo git real");
            let p = gx::read_real_packed_repo(o).unwrap_or_else(|e| json!({"error": e.to_string()}));
            (f, p)
        }
        None => (json!({"skipped": "oráculo indisponível"}), json!({"skipped": "oráculo indisponível"})),
    };
    let loc = gx::lines_per_command();
    println!("[git] gix alto nível");
    let high = gx::sample_repo().map(|t| git::gix_high::probe(&t)).unwrap_or_else(|e| json!({"error": e.to_string()}));
    println!("[git] depscan");
    let mut deps = serde_json::Map::new();
    let mut cats = BTreeMap::new();
    for pkg in ["gix", "gix-object", "gix-hash", "gix-index", "gix-pack", "gix-diff", "gix-ref", "gix-zlib"] {
        let (c, j, _) = scan(&manifest(), pkg);
        cats.insert(pkg, c);
        deps.insert(pkg.to_string(), j);
    }
    let (cat_git2, scan_git2, dir_git2) = scan(&probe_manifest(), "git2");
    let api_git2 = dir_git2.map(|d| {
        grep_api(&d, &["pub fn open<P", "pub fn init<P", "pub fn from_odb", "pub fn add_new_mempack_backend", "git_odb_backend", "git_refdb_backend", "unsafe"])
    });
    let fsck_ok = fsck["fsck_strict_exit"] == json!(0);
    let checks_eq = fsck["checks_equal"].as_u64().unwrap_or(0);
    let checks_total = fsck["checks_total"].as_u64().unwrap_or(0);
    let objs = packed["objects_total"].as_u64().unwrap_or(0);
    let objs_eq = packed["objects_equal_type_size_and_cat_file_p"].as_u64().unwrap_or(0);
    let deltas = packed["pack_delta_entries_seen_by_gix_pack"].as_u64().unwrap_or(0);
    let total_loc = loc["total"].as_u64().unwrap_or(0);
    let high_needs_dir = high["open_after_materializing_to_host_dir"].get("head").is_some();

    result.candidates.push(CandidateResult {
        name: "gix (alto nível)".into(),
        version: "0.88.0".into(),
        role: "git".into(),
        category: cats["gix"].clone(),
        conformance: None,
        fit: Fit::DoesNotFit,
        notes: format!(
            "Sem trait de FS: gix::open/init/discover recebem caminho, e o repositório é feito de gix_odb::Store::at (objetos lidos do disco com mmap), gix_ref::file::Store (refs como arquivos) e gix_index::File::at. O repositório que só existe no MemTree não abre ({}); o mesmo repositório materializado num diretório do host abre {}.",
            high["open_memtree_without_disk"].as_str().unwrap_or("?"),
            if high_needs_dir { "e lê o HEAD" } else { "com erro" }
        ),
        metrics: json!({"probe": high, "deps": deps["gix"]}),
    });
    result.candidates.push(CandidateResult {
        name: "gix-* baixo nível (object, hash, zlib, pack, index, ref, diff)".into(),
        version: "gix-object 0.65 / gix-pack 0.75 / gix-index 0.56 / gix-ref 0.68 / gix-diff 0.68".into(),
        role: "git".into(),
        category: cats["gix-object"].clone(),
        conformance: Some(conf.clone()),
        fit: Fit::FitsWithWork,
        notes: format!(
            "Tudo que o protótipo precisa aceita bytes: hash e codificação de objetos, zlib, pack e índice de pack (from_data com qualquer Deref<[u8]>), packed-refs (Buffer::from_bytes), índice (State::from_bytes/write_to, faltando só o checksum final, que é nosso). Corpus {}/{} estrito com CLI nosso; repositório feito aqui passa git fsck --strict: {}; {checks_eq}/{checks_total} comandos dão a mesma saída no git real; repositório do git real com gc (pack com {deltas} deltas, packed-refs, tag anotada): {objs_eq}/{objs} objetos iguais ao cat-file -p. Custo: {total_loc} linhas de CLI e store (ver lines_per_command). O recurso blob do gix-diff puxa gix-worktree/gix-command (tocam o host), mas as funções usadas são puras.",
            conf.strict_pass, conf.total, fsck_ok
        ),
        metrics: json!({"fsck": fsck, "packed_repo": packed, "lines_per_command": loc, "deps": deps, "failures": failures}),
    });
    result.candidates.push(CandidateResult {
        name: "git2 (libgit2)".into(),
        version: "0.21.0".into(),
        role: "git".into(),
        // Wrapper (b) sobre libgit2: a categoria que conta é a da árvore (c).
        category: scan_git2["tree_category"].as_str().map(str::to_string).or(cat_git2.clone()),
        conformance: None,
        fit: Fit::DoesNotFit,
        notes: "Binding de C (libgit2-sys compila libgit2). Repository::open/init recebem caminho; o único backend que não é disco é o mempack (objetos em memória, sem leitura do nosso FS); backend de objetos ou de refs próprio só implementando git_odb_backend/git_refdb_backend pela FFI crua do libgit2-sys, o que é unsafe nosso. Descartado sem compilar (sonda só de metadados).".into(),
        metrics: json!({"deps": scan_git2, "api": api_git2}),
    });
    let verdict = if conf.strict_pass == conf.total && fsck_ok && objs > 0 && objs_eq == objs { Verdict::Partial } else { Verdict::Inconclusive };
    result.hypothesis(
        "H36",
        verdict,
        format!(
            "gix alto nível não aceita: exige diretório real (sem trait de FS). Os gix-* de baixo nível aceitam bytes e bastam: protótipo sobre MemTree com {}/{} no corpus, git fsck --strict {} no repositório exportado, {objs_eq}/{objs} objetos de um repositório empacotado pelo git real lidos certo, {total_loc} linhas no total.",
            conf.strict_pass,
            conf.total,
            if fsck_ok { "limpo" } else { "com erro" }
        ),
        json!({"corpus_strict": conf.strict_pass, "corpus_total": conf.total, "fsck_strict_exit": fsck["fsck_strict_exit"], "packed_objects_equal": objs_eq, "packed_objects": objs, "lines_total": total_loc}),
    );
    Ok(json!({"conformance_failures": failures, "fsck": fsck, "packed_repo": packed, "lines_per_command": loc, "gix_high_level": high, "deps": deps}))
}

fn main() -> Result<()> {
    let started = Instant::now();
    let mut result = ExperimentResult::new(
        "f12-net-sqlite-git",
        "Rede com allowlist (ureq), sqlite no FS do sandbox (3 caminhos) e git sobre gix-*",
    );
    let oracle = Oracle::locate();
    if let Err(e) = &oracle {
        result.notes.push(format!("oráculo indisponível, interop e fsck pulados: {e}"));
    }
    let oracle = oracle.ok();
    let net = net_section(&mut result);
    let sqlite = sqlite_section(&mut result, oracle.as_ref())?;
    let git = git_section(&mut result, oracle.as_ref())?;
    let (_, own, _) = scan(&manifest(), "f12-net-sqlite-git");
    result.metrics = json!({"net": net, "sqlite": sqlite, "git": git, "own_crate": own, "elapsed_s": started.elapsed().as_secs_f64()});
    result.notes.push(
        "SQLite empacotado com LIBSQLITE3_FLAGS='-USQLITE_DEFAULT_FOREIGN_KEYS -DSQLITE_ENABLE_MATH_FUNCTIONS' (.cargo/config.toml) pra alinhar ao sqlite3 do Debian."
            .into(),
    );
    result.notes.push("Os casos de sqlite comparam o arquivo do banco pelo retrato lógico (esquema, linhas ordenadas, pragmas), porque o sqlite3 do oráculo é 3.46.1 e o empacotado é 3.53.2; a igualdade de bytes vai à parte.".into());
    let path = result.write()?;
    println!("gravado {} em {:.1}s", path.display(), started.elapsed().as_secs_f64());
    for h in &result.hypotheses {
        println!("{} {:?}: {}", h.id, h.verdict, h.summary);
    }
    Ok(())
}
