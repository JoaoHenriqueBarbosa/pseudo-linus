//! H35: sqlite com o banco morando no FS do sandbox, por três caminhos.

pub mod cli;
pub mod experiments;
pub mod memfs;
pub mod rusq;
pub mod turso_engine;

use std::cell::RefCell;

use harness::{Candidate, Entry, Invocation, MemTree, Outcome};

use crate::shell::{Ctx, Programs, run_script};
use cli::{Backend, UNSUPPORTED_MODES, run_sqlite3};

/// Um motor embrulhado como candidato do harness: roda casos `argv` (`sqlite3 ...`) e `script`.
pub struct SqliteCandidate {
    pub label: String,
    pub backend: RefCell<Box<dyn Backend>>,
}

impl SqliteCandidate {
    pub fn new(label: &str, backend: Box<dyn Backend>) -> SqliteCandidate {
        SqliteCandidate { label: label.to_string(), backend: RefCell::new(backend) }
    }
}

struct SqlitePrograms<'a> {
    backend: &'a mut dyn Backend,
}

impl Programs for SqlitePrograms<'_> {
    fn run(&mut self, argv: &[String], ctx: &mut Ctx<'_>) -> Option<i32> {
        (argv[0] == "sqlite3").then(|| run_sqlite3(self.backend, argv, ctx))
    }
}

impl Candidate for SqliteCandidate {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        let mut backend = self.backend.borrow_mut();
        let env = inv.full_env();
        let mut fs = inv.files.clone();
        if let Some(script) = &inv.script {
            let mut programs = SqlitePrograms { backend: backend.as_mut() };
            return match run_script(script, &mut fs, &env, &inv.stdin, &mut programs) {
                Ok(o) => Outcome::exited(o.stdout, o.stderr, o.status, fs),
                Err(e) => Outcome::unsupported(format!("mini-shell: {e}")),
            };
        }
        if let Some(mode) = inv.argv.iter().find(|a| UNSUPPORTED_MODES.contains(&a.as_str())) {
            return Outcome::unsupported(format!("modo de saída {mode} fora da camada mínima (só list)"));
        }
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = {
            let mut ctx = Ctx { fs: &mut fs, env: &env, stdin: &inv.stdin, stdout: &mut out, stderr: &mut err };
            run_sqlite3(backend.as_mut(), &inv.argv, &mut ctx)
        };
        Outcome::exited(out, err, code, fs)
    }
}

/// Cabeçalho de todo arquivo SQLite.
pub const MAGIC: &[u8] = b"SQLite format 3\0";

/// Retrato lógico de um banco: esquema, linhas (ordenadas) e pragmas persistentes. Dois arquivos com o
/// mesmo retrato têm o mesmo conteúdo pra quem lê pelo SQL, mesmo com bytes diferentes (versão do
/// SQLite no cabeçalho, ordem física das páginas, motor diferente).
pub fn logical_dump(bytes: &[u8]) -> String {
    dump(bytes, true)
}

/// Como [`logical_dump`], mas sem o texto SQL do esquema (só nomes, linhas e pragmas).
pub fn data_dump(bytes: &[u8]) -> String {
    dump(bytes, false)
}

fn dump(bytes: &[u8], with_sql: bool) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let mut data = bytes.to_vec();
    // Cabeçalho em modo WAL (18/19 = 2): o banco em memória só abre em modo rollback.
    if data.len() > 19 && data[18] == 2 {
        data[18] = 1;
        data[19] = 1;
    }
    let conn = match rusq::SerializeSession::from_bytes(&data, true) {
        Ok(c) => c,
        Err(e) => return format!("unreadable: {e}"),
    };
    let mut out = String::new();
    let mut schema: Vec<(String, String, String)> = Vec::new();
    let r = conn.prepare("SELECT type, name, coalesce(sql, '') FROM sqlite_schema").and_then(|mut s| {
        let rows = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        for row in rows {
            schema.push(row?);
        }
        Ok(())
    });
    if let Err(e) = r {
        return format!("unreadable: {e}");
    }
    schema.sort();
    for (kind, name, sql) in &schema {
        if with_sql {
            out.push_str(&format!("{kind} {name}: {sql}\n"));
        } else {
            out.push_str(&format!("{kind} {name}\n"));
        }
        if kind == "table" {
            let cols: Vec<String> = conn
                .prepare(&format!("SELECT name FROM pragma_table_info('{}')", name.replace('\'', "''")))
                .and_then(|mut s| s.query_map([], |r| r.get::<_, String>(0))?.collect())
                .unwrap_or_default();
            if cols.is_empty() {
                continue;
            }
            let expr = cols.iter().map(|c| format!("quote(\"{}\")", c.replace('"', "\"\""))).collect::<Vec<_>>().join("||','||");
            let mut rows: Vec<String> = conn
                .prepare(&format!("SELECT {expr} FROM \"{}\"", name.replace('"', "\"\"")))
                .and_then(|mut s| s.query_map([], |r| r.get::<_, String>(0))?.collect())
                .unwrap_or_else(|e| vec![format!("error: {e}")]);
            rows.sort();
            for r in rows {
                out.push_str(&format!("  {r}\n"));
            }
        }
    }
    for pragma in ["user_version", "application_id"] {
        let v: i64 = conn.query_row(&format!("PRAGMA {pragma}"), [], |r| r.get(0)).unwrap_or(-1);
        out.push_str(&format!("pragma {pragma} = {v}\n"));
    }
    out
}

/// Troca o conteúdo de todo arquivo SQLite da árvore pelo retrato lógico dele.
pub fn normalize_tree(tree: &MemTree) -> MemTree {
    let mut out = tree.clone();
    for (path, entry) in tree.entries.iter() {
        if let Entry::File { mode, data: Some(d), .. } = entry
            && d.as_slice().starts_with(MAGIC)
        {
            let dump = format!("logical-dump\n{}", logical_dump(d.as_slice()));
            out.entries.insert(path.clone(), Entry::file(dump.into_bytes(), *mode));
        }
    }
    out
}

/// Normaliza um outcome (golden ou do candidato).
pub fn normalize_outcome(o: &Outcome) -> Outcome {
    Outcome { files: normalize_tree(&o.files), ..o.clone() }
}

/// Candidato que normaliza a árvore de saída antes da comparação.
pub struct Normalized<'a>(pub &'a dyn Candidate);

impl Candidate for Normalized<'_> {
    fn name(&self) -> String {
        self.0.name()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        normalize_outcome(&self.0.run(inv))
    }
}

/// Bytes iguais ignorando os campos de versão do cabeçalho (offsets 92..100).
pub fn bytes_equal_modulo_version(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).enumerate().all(|(i, (x, y))| (92..100).contains(&i) || x == y)
}
