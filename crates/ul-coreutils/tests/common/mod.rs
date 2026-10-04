//! Conformidade dos utilitários contra o golden da bancada, por utilitário.
//!
//! Usado pelos testes deste crate e (por `#[path]`) pelos workspaces de preparação em `staging/`.
//! Diferenças em relação ao `pl_testing::score_tool`:
//!
//! - os casos são filtrados pelo programa (`argv[0]`, ou a primeira palavra do script), pra dar o
//!   placar de cada utilitário;
//! - casos com a tag `unordered` (saída na ordem do readdir, que não é propriedade semântica) são
//!   comparados como multiconjunto de registros, linhas ou registros NUL (tags `print0`/`null`),
//!   como a bancada F06;
//! - casos `script` só rodam se houver um `bash` entre os programas; sem ele, são contados à parte
//!   como "sem shell".

#![allow(dead_code)]

use std::collections::BTreeMap;

use harness::{Bytes, Candidate, Case, CaseComparison, Entry, Invocation, MemTree, Outcome, compare_outcome};
use sysabi::testkit::{TestKit, TreeEntry};
use sysabi::{Program, WaitStatus};

/// Diretório de cada caso, igual ao oráculo.
const CASE_DIR: &str = harness::CASE_DIR;

/// Candidato no kernel de teste do sysabi. É o mesmo do `pl_testing::TestkitCandidate`, copiado aqui
/// pra que estes testes não dependam do crate `kernel` (que o `pl-testing` puxa) quando ele está em
/// obra.
pub struct KitCandidate {
    name: String,
    programs: Vec<Program>,
}

/// "AAAA-MM-DD HH:MM:SS" (UTC) em segundos desde a época.
fn parse_faketime(ts: &str) -> Option<i64> {
    let ts = ts.trim().trim_start_matches('@');
    let (date, time) = ts.split_once(' ').unwrap_or((ts, "00:00:00"));
    let mut d = date.split('-').map(|x| x.parse::<i64>());
    let (y, mo, da) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.split(':').map(|x| x.parse::<i64>());
    let (h, mi, s) = (t.next()?.ok()?, t.next().unwrap_or(Ok(0)).ok()?, t.next().unwrap_or(Ok(0)).ok()?);
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86_400 + h * 3600 + mi * 60 + s)
}

impl KitCandidate {
    pub fn new(name: impl Into<String>, programs: Vec<Program>) -> KitCandidate {
        KitCandidate { name: name.into(), programs }
    }

    fn kit_for(&self, inv: &Invocation) -> TestKit {
        let mut kit = TestKit::new().programs(self.programs.clone()).dir(CASE_DIR, 0o755).cwd(CASE_DIR);
        for (k, v) in inv.full_env() {
            kit = kit.env(&k, &v);
        }
        if let Some(sec) = inv.faketime.as_deref().and_then(parse_faketime) {
            kit = kit.time(sec);
        }
        for (rel, entry) in &inv.files.entries {
            let path = format!("{CASE_DIR}/{rel}");
            match entry {
                Entry::File { data: Some(d), mode, .. } => kit.put_file(path.as_bytes(), d.as_slice(), *mode),
                Entry::File { data: None, mode, .. } => kit.put_file(path.as_bytes(), b"", *mode),
                Entry::Dir { mode } => kit.put_dir(path.as_bytes(), *mode),
                Entry::Symlink { target } => kit.put_symlink(path.as_bytes(), target.as_bytes()),
            }
        }
        for rel in inv.files.entries.keys() {
            kit.set_mtime(format!("{CASE_DIR}/{rel}").as_bytes(), harness::FIXTURE_MTIME as i64);
        }
        kit
    }
}

impl Candidate for KitCandidate {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        let kit = self.kit_for(inv);
        let argv: Vec<Vec<u8>> = match &inv.script {
            Some(s) => vec![b"bash".to_vec(), b"-c".to_vec(), s.as_bytes().to_vec()],
            None => inv.argv.iter().map(|a| a.as_bytes().to_vec()).collect(),
        };
        let r = kit.run_bytes(&argv, &inv.stdin);
        let (exit, signal) = match r.status {
            WaitStatus::Exited(c) => (Some(c), None),
            WaitStatus::Signaled { signal, .. } => (None, Some(signal.0)),
            WaitStatus::Stopped(s) => (None, Some(s.0)),
            WaitStatus::Continued => (Some(0), None),
        };
        let mut files = MemTree::new();
        for (rel, e) in kit.tree(CASE_DIR) {
            let rel = String::from_utf8_lossy(&rel).into_owned();
            let entry = match e {
                TreeEntry::File { data, mode } => Entry::file(data, mode),
                TreeEntry::Dir { mode } => Entry::dir(mode),
                TreeEntry::Symlink { target } => Entry::symlink(String::from_utf8_lossy(&target).into_owned()),
                TreeEntry::Other { mode } => Entry::file(Vec::new(), mode | 0o170000),
            };
            files.entries.insert(rel, entry);
        }
        Outcome { stdout: r.stdout.into(), stderr: r.stderr.into(), exit, signal, timed_out: false, files, unsupported: None }
    }
}

/// Placar de um utilitário.
#[derive(Debug, Default, Clone)]
pub struct UtilScore {
    pub total: usize,
    pub strict: usize,
    pub lenient: usize,
    pub no_shell: usize,
    pub failures: Vec<CaseComparison>,
}

/// Placar de um conjunto de utilitários.
#[derive(Debug, Default)]
pub struct Board {
    pub tool: String,
    pub utils: BTreeMap<String, UtilScore>,
}

impl Board {
    pub fn total(&self) -> (usize, usize, usize, usize) {
        self.utils.values().fold((0, 0, 0, 0), |a, s| (a.0 + s.total, a.1 + s.strict, a.2 + s.lenient, a.3 + s.no_shell))
    }

    /// Resumo legível: uma linha por utilitário e as falhas (até `max_failures` por utilitário).
    pub fn summary(&self, max_failures: usize) -> String {
        let mut s = String::new();
        let (t, st, le, ns) = self.total();
        s.push_str(&format!("{}: {st}/{t} estrito, {le}/{t} leniente, {ns} sem shell\n", self.tool));
        for (name, u) in &self.utils {
            s.push_str(&format!("  {name}: {}/{} estrito, {}/{} leniente{}\n", u.strict, u.total, u.lenient, u.total, if u.no_shell > 0 { format!(", {} sem shell", u.no_shell) } else { String::new() }));
            for f in u.failures.iter().take(max_failures) {
                s.push_str(&format!("    FALHA {}: {}\n", f.id, f.detail.join(" | ")));
            }
        }
        s
    }

    pub fn print(&self, max_failures: usize) {
        // Teste pode imprimir no stderr do host: roda no host como ferramenta.
        eprint!("{}", self.summary(max_failures));
    }

    pub fn util(&self, name: &str) -> UtilScore {
        self.utils.get(name).cloned().unwrap_or_default()
    }
}

/// Programa que um caso exercita: `argv[0]`, ou a primeira palavra do script.
pub fn case_program(case: &Case) -> String {
    if let Some(p) = case.argv.first() {
        return p.clone();
    }
    let script = case.script.as_deref().unwrap_or("");
    script
        .split(|c: char| c.is_whitespace() || c == '|' || c == ';' || c == '&')
        .find(|w| !w.is_empty() && !w.contains('='))
        .unwrap_or("")
        .to_string()
}

fn sorted_records(data: &[u8], sep: u8) -> Vec<u8> {
    let mut records: Vec<&[u8]> = data.split_inclusive(|b| *b == sep).collect();
    records.sort();
    records.concat()
}

/// Versão normalizada de um resultado (só muda alguma coisa nos casos `unordered`).
pub fn normalize(case: &Case, o: &Outcome) -> Outcome {
    if !case.tags.iter().any(|t| t == "unordered") {
        return o.clone();
    }
    let sep = if case.tags.iter().any(|t| t == "print0" || t == "null") { 0 } else { b'\n' };
    let mut out = o.clone();
    out.stdout = Bytes(sorted_records(o.stdout.as_slice(), sep));
    out.stderr = Bytes(sorted_records(o.stderr.as_slice(), b'\n'));
    out
}

pub fn compare(case: &Case, golden: &Outcome, actual: &Outcome) -> CaseComparison {
    compare_outcome(case, &normalize(case, golden), &normalize(case, actual))
}

fn has_shell(programs: &[Program]) -> bool {
    programs.iter().any(|p| p.name == "bash")
}

/// O candidato: kernel de teste (padrão) ou o kernel real (feature `kernel-tests`, que traz o
/// `pl-testing`, e variável `CONF_KERNEL=1`, lida pelo teste no host).
pub fn candidate(tool: &str, programs: Vec<Program>) -> Box<dyn Candidate> {
    #[cfg(feature = "kernel-tests")]
    {
        #[allow(clippy::disallowed_methods)]
        if std::env::var_os("CONF_KERNEL").is_some() {
            return Box::new(pl_testing::KernelCandidate::new(format!("{tool} (kernel)"), programs));
        }
    }
    Box::new(KitCandidate::new(format!("{tool} (testkit)"), programs))
}

/// Roda os casos de `tool` (diretório de `testbench/corpus/cases`) cujo programa está em `utils`
/// (vazio = todos), no kernel de teste com `programs`.
pub fn score(tool: &str, utils: &[&str], programs: Vec<Program>) -> Board {
    let shell = has_shell(&programs);
    let cand = candidate(tool, programs);
    let (cases, _missing) = harness::paths::load_tool(tool).expect("carregar casos e golden");
    let mut board = Board { tool: tool.to_string(), ..Board::default() };
    for (case, golden) in cases {
        let prog = case_program(&case);
        if !utils.is_empty() && !utils.contains(&prog.as_str()) {
            continue;
        }
        // Variável do host, lida pelo teste (não pelo programa): mostra cada caso antes de rodar.
        #[allow(clippy::disallowed_methods)]
        if std::env::var_os("CONF_TRACE").is_some() {
            eprintln!("caso {} ({prog})", case.id);
        }
        let slot = board.utils.entry(prog).or_default();
        slot.total += 1;
        if case.script.is_some() && !shell {
            slot.no_shell += 1;
            continue;
        }
        let inv = match case.invocation() {
            Ok(inv) => inv,
            Err(e) => {
                let actual = Outcome::unsupported(format!("caso inválido: {e}"));
                slot.failures.push(compare(&case, &golden, &actual));
                continue;
            }
        };
        let actual = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cand.run(&inv))) {
            Ok(o) => o,
            Err(p) => {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "panic sem mensagem".into());
                Outcome::unsupported(format!("panic: {msg}"))
            }
        };
        let cmp = compare(&case, &golden, &actual);
        slot.strict += cmp.strict as usize;
        slot.lenient += cmp.lenient as usize;
        if !cmp.strict {
            slot.failures.push(cmp);
        }
    }
    board
}

/// Roda só os casos com estes ids (pra depurar).
pub fn score_ids(tool: &str, ids: &[&str], programs: Vec<Program>) -> Board {
    let cand = candidate(tool, programs);
    let (cases, _) = harness::paths::load_tool(tool).expect("carregar casos e golden");
    let mut board = Board { tool: tool.to_string(), ..Board::default() };
    for (case, golden) in cases.into_iter().filter(|(c, _)| ids.contains(&c.id.as_str())) {
        let slot = board.utils.entry(case_program(&case)).or_default();
        slot.total += 1;
        let inv = case.invocation().expect("caso válido");
        let actual = cand.run(&inv);
        let cmp = compare(&case, &golden, &actual);
        slot.strict += cmp.strict as usize;
        slot.lenient += cmp.lenient as usize;
        if !cmp.strict {
            slot.failures.push(cmp);
        }
    }
    board
}
