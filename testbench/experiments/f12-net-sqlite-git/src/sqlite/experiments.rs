//! Medições do H35: conformidade contra o golden, interoperabilidade de arquivo com o sqlite3 do
//! oráculo, dois "processos" no mesmo banco, relógio do sandbox e a superfície de API de cada caminho.

use std::collections::BTreeMap;
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use harness::{Candidate, Case, CaseComparison, Conformance, Entry, MemTree, Oracle, Outcome, paths, score};
use serde_json::{Value as Json, json};

use super::cli::{Backend, Session};
use super::memfs::{SharedFs, next_id};
use super::rusq::{PluginBackend, SerializeBackend, SerializeSession, open_vfs};
use super::turso_engine::{TursoBackend, open_db};
use super::{Normalized, SqliteCandidate, bytes_equal_modulo_version, normalize_outcome};

/// Relógio do sandbox usado nos testes: 2026-01-15T12:00:00Z.
pub fn sandbox_wall() -> turso_core::io::clock::WallClockInstant {
    turso_core::io::clock::WallClockInstant { secs: harness::FIXTURE_MTIME as i64, micros: 0 }
}

pub fn candidates() -> Vec<SqliteCandidate> {
    vec![
        SqliteCandidate::new("rusqlite-serialize", Box::new(SerializeBackend)),
        SqliteCandidate::new("rusqlite-sqlite-plugin", Box::new(PluginBackend)),
        SqliteCandidate::new("turso-core", Box::new(TursoBackend { wall: Some(sandbox_wall()) })),
    ]
}

/// Resultado de conformidade de um candidato, com as comparações caso a caso.
pub struct ConformanceRun {
    pub conformance: Conformance,
    pub comparisons: Vec<CaseComparison>,
    pub stdouts: BTreeMap<String, Vec<u8>>,
    /// Casos cujo banco final ficou igual byte a byte ao do oráculo, ignorando os campos de versão.
    pub db_bytes_equal: usize,
    /// Casos cujo banco final tem as mesmas tabelas, linhas e pragmas (ignorando o texto do esquema).
    pub db_data_equal: usize,
    pub db_files_compared: usize,
    /// Amostra de deslocamentos onde os bytes divergem: (caso, tamanhos, primeiros deslocamentos).
    pub db_diff_samples: Vec<(String, usize, usize, Vec<usize>)>,
}

pub fn load_cases() -> Result<Vec<(Case, Outcome)>> {
    let (cases, missing) = paths::load_tool("sqlite")?;
    anyhow::ensure!(missing == 0, "{missing} casos de sqlite sem golden; rode `cargo run -p oracle -- gen --tool sqlite`");
    Ok(cases)
}

pub fn run_conformance(cand: &SqliteCandidate, cases: &[(Case, Outcome)]) -> ConformanceRun {
    let normalized_cases: Vec<(Case, Outcome)> =
        cases.iter().map(|(c, g)| (c.clone(), normalize_outcome(g))).collect();
    let (conformance, comparisons) = score(&Normalized(cand), &normalized_cases);
    let mut stdouts = BTreeMap::new();
    let mut db_bytes_equal = 0;
    let mut db_data_equal = 0;
    let mut db_files_compared = 0;
    let mut db_diff_samples = Vec::new();
    for (case, golden) in cases {
        let Ok(inv) = case.invocation() else { continue };
        let out = cand.run(&inv);
        stdouts.insert(case.id.clone(), out.stdout.0.clone());
        for (path, entry) in &golden.files.entries {
            if let (Some(g), Some(a)) = (entry.data(), out.files.read(path))
                && g.starts_with(super::MAGIC)
            {
                db_files_compared += 1;
                let eq = bytes_equal_modulo_version(g, a);
                db_bytes_equal += eq as usize;
                db_data_equal += (super::data_dump(g) == super::data_dump(a)) as usize;
                if !eq && db_diff_samples.len() < 4 {
                    let offs: Vec<usize> =
                        (0..g.len().min(a.len())).filter(|&i| g[i] != a[i] && !(92..100).contains(&i)).take(12).collect();
                    db_diff_samples.push((case.id.clone(), g.len(), a.len(), offs));
                }
            }
        }
    }
    ConformanceRun { conformance, comparisons, stdouts, db_bytes_equal, db_data_equal, db_files_compared, db_diff_samples }
}

/// Classifica a divergência de cada caso: o que é do motor e o que é do CLI.
pub fn classify(runs: &BTreeMap<String, ConformanceRun>) -> Json {
    let reference = &runs["rusqlite-serialize"];
    let mut out = serde_json::Map::new();
    for (name, run) in runs {
        let mut stdout_diff_vs_sqlite_engine = Vec::new();
        for (id, stdout) in &run.stdouts {
            if reference.stdouts.get(id) != Some(stdout) {
                stdout_diff_vs_sqlite_engine.push(id.clone());
            }
        }
        out.insert(
            name.clone(),
            json!({
                "cases_with_stdout_different_from_real_sqlite_engine_same_cli": stdout_diff_vs_sqlite_engine.len(),
                "ids": stdout_diff_vs_sqlite_engine,
            }),
        );
    }
    Json::Object(out)
}

pub fn failures(run: &ConformanceRun) -> Vec<Json> {
    run.comparisons
        .iter()
        .filter(|c| !c.strict)
        .map(|c| json!({"id": c.id, "lenient": c.lenient, "detail": c.detail}))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Uso direto dos motores (sem CLI) pros testes de interop e concorrência
// ---------------------------------------------------------------------------------------------

/// Roda SQL num `Session` e devolve as linhas como texto (`a|b`).
pub fn query_lines(session: &mut dyn Session, sql: &str) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    for stmt in split_statements(sql) {
        session
            .execute(&stmt, &mut |_, cells| {
                let row: Vec<String> = cells
                    .iter()
                    .map(|c| match c {
                        super::cli::Cell::Null => String::new(),
                        super::cli::Cell::Integer(i) => i.to_string(),
                        super::cli::Cell::Real(f) => super::cli::fmt_real(*f),
                        super::cli::Cell::Text(t) | super::cli::Cell::Blob(t) => String::from_utf8_lossy(t).into_owned(),
                    })
                    .collect();
                lines.push(row.join("|"));
            })
            .map_err(|e| e.message)?;
    }
    Ok(lines)
}

pub fn split_statements(sql: &str) -> Vec<String> {
    let scan = super::cli::scan(sql);
    let mut out = Vec::new();
    let mut start = 0;
    let mut ends = scan.boundaries;
    if ends.last().copied() != Some(sql.len()) {
        ends.push(sql.len());
    }
    for end in ends {
        let seg = &sql[start..end];
        start = end;
        if !super::cli::is_blank_sql(seg) {
            out.push(seg.trim().to_string());
        }
    }
    out
}

/// Script de criação usado nos testes de interoperabilidade.
pub const INTEROP_CREATE: &str = r#"
PRAGMA user_version = 42;
CREATE TABLE kinds(id INTEGER PRIMARY KEY, i INTEGER, r REAL, t TEXT, b BLOB, n);
WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 300)
INSERT INTO kinds(i, r, t, b, n) SELECT x * 7, x / 3.0, 'linha ' || x || ' ação', CASE x % 3 WHEN 0 THEN x'00ff' WHEN 1 THEN zeroblob(4) ELSE x'deadbeef01' END, CASE WHEN x % 4 = 0 THEN NULL ELSE x END FROM c;
CREATE INDEX kinds_t ON kinds(t);
CREATE TABLE kv(k TEXT PRIMARY KEY, v);
INSERT INTO kv VALUES ('b', 2), ('a', 1), ('日本', 3);
CREATE VIEW big AS SELECT id, i FROM kinds WHERE i > 2000;
CREATE TABLE log(msg);
CREATE TRIGGER kv_ai AFTER INSERT ON kv BEGIN INSERT INTO log VALUES (NEW.k); END;
INSERT INTO kv VALUES ('c', 4);
DELETE FROM kinds WHERE id % 10 = 0;
UPDATE kinds SET t = upper(t) WHERE id % 7 = 0;
"#;

/// Consulta de verificação: agrega tudo que foi criado.
pub const INTEROP_QUERY: &str = r#"
SELECT count(*), sum(i), printf('%.6f', sum(r)), sum(length(t)), sum(length(b)), count(n) FROM kinds;
SELECT group_concat(k || '=' || v, ',') FROM (SELECT * FROM kv ORDER BY k);
SELECT count(*) FROM big;
SELECT group_concat(msg) FROM log;
SELECT t FROM kinds WHERE t LIKE 'LINHA 7%' ORDER BY id;
PRAGMA user_version;
"#;

fn run_on_backend(backend: &mut dyn Backend, fs: &mut MemTree, path: &str, sql: &str) -> Result<Vec<String>, String> {
    if fs.get(path).is_none() {
        fs.insert(path, Entry::file(Vec::new(), 0o644));
    }
    let mut session = backend.open(fs, Some(path), false)?;
    let r = query_lines(session.as_mut(), sql);
    session.close(fs, false)?;
    r
}

/// Interoperabilidade de arquivo nos dois sentidos.
pub fn interop(oracle: &Oracle) -> Result<Json> {
    let mut backends: Vec<(&str, Box<dyn Backend>)> = vec![
        ("rusqlite-serialize", Box::new(SerializeBackend)),
        ("rusqlite-sqlite-plugin", Box::new(PluginBackend)),
        ("turso-core", Box::new(TursoBackend { wall: Some(sandbox_wall()) })),
    ];
    // 1) Bancos criados aqui.
    let mut files = MemTree::new();
    files.insert("create.sql", Entry::file(INTEROP_CREATE.as_bytes().to_vec(), 0o644));
    files.insert("query.sql", Entry::file(INTEROP_QUERY.as_bytes().to_vec(), 0o644));
    let mut ours_query = BTreeMap::new();
    for (name, backend) in backends.iter_mut() {
        let mut fs = MemTree::new();
        let created = run_on_backend(backend.as_mut(), &mut fs, "db.sqlite", INTEROP_CREATE);
        let queried = run_on_backend(backend.as_mut(), &mut fs, "db.sqlite", INTEROP_QUERY);
        ours_query.insert(name.to_string(), json!({"create": created.err(), "query": queried.clone().unwrap_or_default()}));
        for (path, entry) in &fs.entries {
            files.insert(&format!("{name}/{path}"), entry.clone());
        }
    }
    let script = r#"
set -u
sqlite3 ref.sqlite < create.sql
sqlite3 ref.sqlite < query.sql > ref.out
cp ref.sqlite wal.sqlite
sqlite3 wal.sqlite 'PRAGMA journal_mode=WAL; INSERT INTO log VALUES (1); DELETE FROM log WHERE msg = 1;' > /dev/null
sqlite3 bad.sqlite "CREATE TABLE t(x); INSERT INTO t VALUES (CAST(x'41ff42' AS TEXT)); INSERT INTO t VALUES ('ok');"
sqlite3 bad.sqlite 'SELECT length(x), hex(x) FROM t' > bad.out
for d in rusqlite-serialize rusqlite-sqlite-plugin turso-core; do
  echo "== $d"
  ls "$d"
  sqlite3 "$d/db.sqlite" 'PRAGMA integrity_check' 2>&1
  sqlite3 "$d/db.sqlite" < query.sql > "$d/out" 2>&1
  if cmp -s ref.out "$d/out"; then echo "query: SAME"; else echo "query: DIFF"; diff ref.out "$d/out" | head -20; fi
done
"#;
    let outcome = oracle.run_script("sqlite-interop", script, files)?;
    let oracle_stdout = String::from_utf8_lossy(&outcome.stdout.0).into_owned();
    let ref_out = outcome.files.read("ref.out").map(|d| String::from_utf8_lossy(d).into_owned()).unwrap_or_default();
    let ref_lines: Vec<String> = ref_out.lines().map(str::to_string).collect();
    let mut ours_to_oracle = serde_json::Map::new();
    for section in oracle_stdout.split("== ").skip(1) {
        let (name, body) = section.split_once('\n').unwrap_or((section, ""));
        let name = name.trim().to_string();
        let integrity_ok = body.lines().any(|l| l == "ok");
        let same = body.contains("query: SAME");
        ours_to_oracle.insert(name, json!({"integrity_check_ok": integrity_ok, "query_same_as_native": same, "oracle_output": body}));
    }
    // 2) Bancos criados pelo sqlite3 do oráculo (rollback e WAL), lidos aqui.
    let mut oracle_to_ours = serde_json::Map::new();
    for (name, backend) in backends.iter_mut() {
        let mut per_file = serde_json::Map::new();
        for db in ["ref.sqlite", "wal.sqlite"] {
            let Some(bytes) = outcome.files.read(db) else { continue };
            let mut fs = MemTree::new();
            fs.insert("db.sqlite", Entry::file(bytes.to_vec(), 0o644));
            let lines = run_on_backend(backend.as_mut(), &mut fs, "db.sqlite", INTEROP_QUERY);
            let integrity = run_on_backend(backend.as_mut(), &mut fs, "db.sqlite", "PRAGMA integrity_check;");
            let same = lines.as_ref().map(|l| *l == ref_lines).unwrap_or(false);
            per_file.insert(
                db.to_string(),
                json!({
                    "query_same_as_oracle": same,
                    "integrity_check": integrity.unwrap_or_else(|e| vec![format!("erro: {e}")]),
                    "error": lines.err(),
                }),
            );
        }
        // TEXT com UTF-8 inválido gravado pelo sqlite3 (o SQLite aceita; o que cada motor faz ao ler).
        if let Some(bytes) = outcome.files.read("bad.sqlite") {
            let mut fs = MemTree::new();
            fs.insert("db.sqlite", Entry::file(bytes.to_vec(), 0o644));
            let got = run_on_backend(backend.as_mut(), &mut fs, "db.sqlite", "SELECT length(x), hex(x) FROM t;");
            let want: Vec<String> = outcome
                .files
                .read("bad.out")
                .map(|d| String::from_utf8_lossy(d).lines().map(str::to_string).collect())
                .unwrap_or_default();
            per_file.insert(
                "invalid_utf8_text.sqlite".to_string(),
                json!({"same_as_oracle": got.as_ref().map(|g| *g == want).unwrap_or(false), "ours": got, "oracle": want}),
            );
        }
        oracle_to_ours.insert(name.to_string(), Json::Object(per_file));
    }
    // Evidência crua: o que acontece com o arquivo WAL sem os contornos dos backends.
    let raw_wal = outcome.files.read("wal.sqlite").map(|bytes| {
        let serialize_raw = SerializeSession::from_bytes(bytes, false)
            .and_then(|c| c.query_row("SELECT count(*) FROM kinds", [], |r| r.get::<_, i64>(0)).map_err(|e| e.to_string()));
        let path = format!("/rawwal{}/db.sqlite", next_id());
        let node = SharedFs::global().get_or_create(&path);
        *node.data.write().expect("dados") = bytes.to_vec();
        let plugin_raw = open_vfs(&path, false)
            .and_then(|c| c.query_row("SELECT count(*) FROM kinds", [], |r| r.get::<_, i64>(0)).map_err(|e| e.to_string()));
        SharedFs::global().clear(path.rsplit_once('/').map(|x| x.0).unwrap_or(""));
        json!({
            "serialize_without_header_patch": serialize_raw.map(|n| n.to_string()).unwrap_or_else(|e| format!("erro: {e}")),
            "plugin_without_exclusive_locking": plugin_raw.map(|n| n.to_string()).unwrap_or_else(|e| format!("erro: {e}")),
        })
    });
    Ok(json!({
        "wal_file_without_workarounds": raw_wal,
        "oracle_reference_output": ref_lines,
        "ours_query_output": ours_query,
        "created_here_opened_by_oracle": ours_to_oracle,
        "created_by_oracle_opened_here": oracle_to_ours,
        "oracle_raw": oracle_stdout,
    }))
}

// ---------------------------------------------------------------------------------------------
// Dois "processos" no mesmo banco
// ---------------------------------------------------------------------------------------------

/// Arquivo do caminho 1: bytes num `Mutex`, cada comando lê tudo e grava tudo.
struct SerializedFile {
    bytes: Mutex<Vec<u8>>,
    /// Trava de arquivo inteiro que o kernel daria a cada comando `sqlite3` (opcional no teste).
    whole_file_lock: Mutex<()>,
}

fn serialize_command(file: &SerializedFile, sql: &str, lock: bool) -> Result<Vec<String>, String> {
    let _guard = if lock { Some(file.whole_file_lock.lock().expect("trava")) } else { None };
    let bytes = file.bytes.lock().expect("bytes").clone();
    let conn = SerializeSession::from_bytes(&bytes, false)?;
    let mut lines = Vec::new();
    for stmt in split_statements(sql) {
        super::rusq::execute(&conn, &stmt, &mut |_, cells| lines.push(format!("{cells:?}"))).map_err(|e| e.message)?;
    }
    let out = SerializeSession::to_bytes(&conn)?;
    *file.bytes.lock().expect("bytes") = out;
    Ok(lines)
}

fn count_from(lines: &[String]) -> i64 {
    lines
        .first()
        .and_then(|l| l.trim_start_matches("[Integer(").trim_end_matches(")]").parse().ok())
        .unwrap_or(-1)
}

/// Contador incrementado por 2 threads, cada incremento como um comando separado.
pub fn concurrency() -> Json {
    const PER_THREAD: usize = 100;
    let mut out = serde_json::Map::new();

    // Caminho 1, sem e com trava de arquivo inteiro.
    for lock in [false, true] {
        let file = Arc::new(SerializedFile { bytes: Mutex::new(Vec::new()), whole_file_lock: Mutex::new(()) });
        serialize_command(&file, "CREATE TABLE c(n INTEGER); INSERT INTO c VALUES (0);", true).expect("setup");
        let barrier = Arc::new(Barrier::new(2));
        let start = Instant::now();
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let file = file.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    for _ in 0..PER_THREAD {
                        serialize_command(&file, "UPDATE c SET n = n + 1;", lock).expect("update");
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("thread");
        }
        let n = count_from(&serialize_command(&file, "SELECT n FROM c;", true).expect("select"));
        out.insert(
            format!("rusqlite-serialize/counter/{}", if lock { "whole-file-lock" } else { "no-lock" }),
            json!({"expected": 2 * PER_THREAD, "final": n, "lost_updates": 2 * PER_THREAD as i64 - n, "ms": start.elapsed().as_millis()}),
        );
    }
    out.insert("rusqlite-serialize/visibility".into(), serialize_visibility());

    // Caminho 2: conexões independentes pelo VFS, travas do próprio SQLite.
    out.insert("rusqlite-sqlite-plugin/counter".into(), plugin_counter(PER_THREAD));
    out.insert("rusqlite-sqlite-plugin/visibility".into(), plugin_visibility());

    // Caminho 3: turso, cada comando uma conexão nova.
    out.insert("turso-core/counter".into(), turso_counter(PER_THREAD));
    out.insert("turso-core/visibility".into(), turso_visibility());
    Json::Object(out)
}

fn serialize_visibility() -> Json {
    // A abre a sessão, grava e confirma, mas só devolve os bytes ao "FS" quando o comando termina.
    let file = SerializedFile { bytes: Mutex::new(Vec::new()), whole_file_lock: Mutex::new(()) };
    serialize_command(&file, "CREATE TABLE t(x);", false).expect("setup");
    let bytes = file.bytes.lock().expect("bytes").clone();
    let a = SerializeSession::from_bytes(&bytes, false).expect("A");
    let run = |c: &rusqlite::Connection, sql: &str| -> Result<Vec<String>, String> {
        let mut lines = Vec::new();
        for s in split_statements(sql) {
            super::rusq::execute(c, &s, &mut |_, cells| lines.push(format!("{cells:?}"))).map_err(|e| e.message)?;
        }
        Ok(lines)
    };
    run(&a, "BEGIN; INSERT INTO t VALUES (1);").expect("A begin");
    let b_during_tx = count_from(&serialize_command(&file, "SELECT count(*) FROM t;", false).expect("B"));
    run(&a, "COMMIT;").expect("A commit");
    let b_after_commit_before_exit = count_from(&serialize_command(&file, "SELECT count(*) FROM t;", false).expect("B"));
    // B grava enquanto A ainda está aberto; depois A termina e escreve os bytes dele por cima.
    serialize_command(&file, "INSERT INTO t VALUES (2);", false).expect("B insert");
    *file.bytes.lock().expect("bytes") = SerializeSession::to_bytes(&a).expect("A exit");
    let final_count = count_from(&serialize_command(&file, "SELECT count(*) FROM t;", false).expect("final"));
    json!({
        "reader_during_uncommitted_tx_sees": b_during_tx,
        "reader_after_commit_before_writer_exit_sees": b_after_commit_before_exit,
        "rows_after_both_writers_exit_expected": 2,
        "rows_after_both_writers_exit": final_count,
        "note": "commit só fica visível quando o processo termina; escritas concorrentes se sobrescrevem (última a sair ganha)",
    })
}

fn plugin_counter(per_thread: usize) -> Json {
    let path = format!("/conc{}/db.sqlite", next_id());
    {
        let c = open_vfs(&path, false).expect("abrir");
        c.execute_batch("CREATE TABLE c(n INTEGER); INSERT INTO c VALUES (0);").expect("setup");
    }
    let barrier = Arc::new(Barrier::new(2));
    let start = Instant::now();
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            let errors = errors.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..per_thread {
                    let c = open_vfs(&path, false).expect("abrir");
                    c.busy_timeout(Duration::from_secs(10)).expect("busy");
                    if let Err(e) = c.execute("UPDATE c SET n = n + 1", []) {
                        errors.lock().expect("erros").push(e.to_string());
                    }
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread");
    }
    let c = open_vfs(&path, false).expect("abrir");
    let n: i64 = c.query_row("SELECT n FROM c", [], |r| r.get(0)).unwrap_or(-1);
    let errs = errors.lock().expect("erros").clone();
    SharedFs::global().clear(path.rsplit_once('/').map(|x| x.0).unwrap_or(""));
    let succeeded = (2 * per_thread - errs.len()) as i64;
    json!({"expected": 2 * per_thread, "succeeded": succeeded, "final": n, "lost_updates": succeeded - n, "errors": errs.len(), "sample_errors": errs.iter().take(3).collect::<Vec<_>>(), "ms": start.elapsed().as_millis()})
}

fn plugin_visibility() -> Json {
    let path = format!("/conc{}/db.sqlite", next_id());
    let a = open_vfs(&path, false).expect("A");
    a.execute_batch("CREATE TABLE t(x);").expect("setup");
    let b = open_vfs(&path, false).expect("B");
    let count = |c: &rusqlite::Connection| c.query_row("SELECT count(*) FROM t", [], |r| r.get::<_, i64>(0));
    a.execute_batch("BEGIN; INSERT INTO t VALUES (1);").expect("A begin");
    let b_during = count(&b).map_err(|e| e.to_string());
    // Escrita concorrente sem espera: tem que dar SQLITE_BUSY.
    b.busy_timeout(Duration::ZERO).expect("busy");
    let b_write_during = b.execute("INSERT INTO t VALUES (2)", []).map(|_| "ok".to_string()).unwrap_or_else(|e| e.to_string());
    a.execute_batch("COMMIT;").expect("A commit");
    let b_after = count(&b).map_err(|e| e.to_string());
    b.busy_timeout(Duration::from_secs(5)).expect("busy");
    let b_write_after = b.execute("INSERT INTO t VALUES (2)", []).map(|_| "ok".to_string()).unwrap_or_else(|e| e.to_string());
    let final_count = count(&a).map_err(|e| e.to_string());
    drop(a);
    drop(b);
    SharedFs::global().clear(path.rsplit_once('/').map(|x| x.0).unwrap_or(""));
    json!({
        "reader_during_uncommitted_tx_sees": b_during,
        "concurrent_writer_without_wait": b_write_during,
        "reader_after_commit_sees": b_after,
        "writer_after_commit": b_write_after,
        "rows_final": final_count,
    })
}

fn turso_run(conn: &Arc<turso_core::Connection>, sql: &str) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    for s in split_statements(sql) {
        super::turso_engine::execute(conn, &s, &mut |_, cells| lines.push(format!("{cells:?}"))).map_err(|e| e.message)?;
    }
    Ok(lines)
}

fn turso_counter(per_thread: usize) -> Json {
    let path = format!("/conc{}/db.sqlite", next_id());
    {
        let db = open_db(SharedFs::global(), &path, None).expect("abrir");
        let c = db.connect().expect("conectar");
        turso_run(&c, "CREATE TABLE c(n INTEGER); INSERT INTO c VALUES (0);").expect("setup");
        c.close().expect("fechar");
    }
    let barrier = Arc::new(Barrier::new(2));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let retries = Arc::new(Mutex::new(0usize));
    let start = Instant::now();
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            let errors = errors.clone();
            let retries = retries.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..per_thread {
                    let deadline = Instant::now() + Duration::from_secs(10);
                    loop {
                        let r = open_db(SharedFs::global(), &path, None).and_then(|db| {
                            let c = db.connect().map_err(|e| e.to_string())?;
                            let r = turso_run(&c, "UPDATE c SET n = n + 1;");
                            let _ = c.close();
                            r
                        });
                        match r {
                            Ok(_) => break,
                            Err(e) if (e.contains("busy") || e.contains("locked") || e.contains("Busy")) && Instant::now() < deadline => {
                                *retries.lock().expect("retries") += 1;
                                std::thread::sleep(Duration::from_micros(200));
                            }
                            Err(e) => {
                                errors.lock().expect("erros").push(e);
                                break;
                            }
                        }
                    }
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread");
    }
    let n = open_db(SharedFs::global(), &path, None)
        .and_then(|db| {
            let c = db.connect().map_err(|e| e.to_string())?;
            turso_run(&c, "SELECT n FROM c;")
        })
        .map(|l| count_from(&l))
        .unwrap_or(-1);
    let errs = errors.lock().expect("erros").clone();
    let retries = *retries.lock().expect("retries");
    SharedFs::global().clear(path.rsplit_once('/').map(|x| x.0).unwrap_or(""));
    let succeeded = (2 * per_thread - errs.len()) as i64;
    json!({"expected": 2 * per_thread, "succeeded": succeeded, "final": n, "lost_updates": succeeded - n, "busy_retries": retries, "errors": errs.len(), "sample_errors": errs.iter().take(3).collect::<Vec<_>>(), "ms": start.elapsed().as_millis()})
}

fn turso_visibility() -> Json {
    let path = format!("/conc{}/db.sqlite", next_id());
    let db = open_db(SharedFs::global(), &path, None).expect("abrir");
    let a = db.connect().expect("A");
    turso_run(&a, "CREATE TABLE t(x);").expect("setup");
    let db2 = open_db(SharedFs::global(), &path, None).expect("abrir B");
    let same_instance = Arc::ptr_eq(&db, &db2);
    let b = db2.connect().expect("B");
    turso_run(&a, "BEGIN; INSERT INTO t VALUES (1);").expect("A begin");
    let b_during = turso_run(&b, "SELECT count(*) FROM t;").map(|l| count_from(&l));
    let b_write_during = turso_run(&b, "INSERT INTO t VALUES (2);").map(|_| "ok".to_string()).unwrap_or_else(|e| e);
    turso_run(&a, "COMMIT;").expect("A commit");
    let b_after = turso_run(&b, "SELECT count(*) FROM t;").map(|l| count_from(&l));
    let b_write_after = turso_run(&b, "INSERT INTO t VALUES (2);").map(|_| "ok".to_string()).unwrap_or_else(|e| e);
    let final_count = turso_run(&a, "SELECT count(*) FROM t;").map(|l| count_from(&l));
    let _ = a.close();
    let _ = b.close();
    drop(db);
    drop(db2);
    SharedFs::global().clear(path.rsplit_once('/').map(|x| x.0).unwrap_or(""));
    json!({
        "second_open_reuses_database_instance": same_instance,
        "reader_during_uncommitted_tx_sees": b_during,
        "concurrent_writer_without_wait": b_write_during,
        "reader_after_commit_sees": b_after,
        "writer_after_commit": b_write_after,
        "rows_final": final_count,
    })
}

/// `datetime('now')` com o relógio do sandbox fixo em 2026-01-15 12:00:00.
pub fn clock() -> Json {
    let want = "2026-01-15 12:00:00";
    let mut out = serde_json::Map::new();
    let mut backends: Vec<(&str, Box<dyn Backend>)> = vec![
        ("rusqlite-serialize", Box::new(SerializeBackend)),
        ("rusqlite-sqlite-plugin", Box::new(PluginBackend)),
        ("turso-core", Box::new(TursoBackend { wall: Some(sandbox_wall()) })),
    ];
    for (name, backend) in backends.iter_mut() {
        let mut fs = MemTree::new();
        let got = run_on_backend(backend.as_mut(), &mut fs, "db.sqlite", "SELECT datetime('now');")
            .map(|l| l.join(""))
            .unwrap_or_else(|e| format!("erro: {e}"));
        out.insert(name.to_string(), json!({"datetime_now": got, "follows_sandbox_clock": got == want}));
    }
    Json::Object(out)
}
