//! Conformidade contra o golden da bancada.
//!
//! Uso típico, num teste de um crate de userland:
//!
//! ```ignore
//! let cand = pl_testing::TestkitCandidate::new("grep (testkit)", ul_textproc::programs());
//! let report = pl_testing::score_tool("grep", &cand);
//! report.print();
//! assert!(report.lenient_rate() >= 0.95);
//! ```
//!
//! Casos `script` precisam de um `bash` entre os programas (o crate `shell` exporta).

use std::collections::BTreeMap;

use harness::{Candidate, Conformance, Entry, Invocation, MemTree, Outcome};
use sysabi::testkit::{TestKit, TreeEntry};
use sysabi::{Program, WaitStatus};

/// Onde a fixture de cada caso é montada, igual ao oráculo.
pub const CASE_DIR: &str = harness::CASE_DIR;

/// Candidato que roda cada caso no kernel de teste em memória.
pub struct TestkitCandidate {
    name: String,
    programs: Vec<Program>,
}

impl TestkitCandidate {
    pub fn new(name: impl Into<String>, programs: Vec<Program>) -> TestkitCandidate {
        TestkitCandidate { name: name.into(), programs }
    }

    /// Monta o testkit com a fixture do caso.
    pub fn kit_for(&self, inv: &Invocation) -> TestKit {
        let mut kit = TestKit::new().programs(self.programs.clone()).dir(CASE_DIR, 0o755).cwd(CASE_DIR);
        for (k, v) in inv.full_env() {
            kit = kit.env(&k, &v);
        }
        if let Some(ts) = &inv.faketime {
            if let Some(sec) = parse_faketime(ts) {
                kit = kit.time(sec);
            }
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

impl Candidate for TestkitCandidate {
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
        Outcome {
            stdout: r.stdout.into(),
            stderr: r.stderr.into(),
            exit,
            signal,
            timed_out: false,
            files: tree_to_memtree(kit.tree(CASE_DIR)),
            unsupported: None,
        }
    }
}

/// Candidato que roda cada caso no kernel real (`crates/kernel`): threads de verdade, pipes
/// concorrentes, sinais, escalonador. Cada caso ganha um sandbox novo, derivado em O(1) de um retrato da
/// imagem base com os programas. Exige a feature `kernel`.
#[cfg(feature = "kernel")]
pub struct KernelCandidate {
    name: String,
    programs: Vec<Program>,
    kernel: kernel::Kernel,
    base: kernel::Snapshot,
    timeout: std::time::Duration,
}

#[cfg(feature = "kernel")]
impl KernelCandidate {
    pub fn new(name: impl Into<String>, programs: Vec<Program>) -> KernelCandidate {
        let kernel = kernel::Kernel::new(kernel::KernelConfig::default());
        let template = kernel
            .create_sandbox(kernel::SandboxConfig { programs: programs.clone(), ..kernel::SandboxConfig::default() })
            .expect("sandbox da imagem base");
        let base = template.snapshot();
        drop(template);
        KernelCandidate { name: name.into(), programs, kernel, base, timeout: std::time::Duration::from_secs(20) }
    }

    /// Prazo de parede por caso (padrão 20 s, igual ao oráculo).
    pub fn with_timeout(mut self, d: std::time::Duration) -> KernelCandidate {
        self.timeout = d;
        self
    }

    /// Sandbox com a fixture do caso montada em `/work/case`.
    pub fn sandbox_for(&self, inv: &Invocation) -> Result<kernel::Sandbox, String> {
        let clock = match inv.faketime.as_deref().and_then(parse_faketime) {
            Some(sec) => kernel::ClockMode::Fixed(sysabi::TimeSpec { sec, nsec: 0 }),
            None => kernel::ClockMode::Host,
        };
        let env: Vec<Vec<u8>> = inv.full_env().into_iter().map(|(k, v)| format!("{k}={v}").into_bytes()).collect();
        let cfg = kernel::SandboxConfig {
            programs: self.programs.clone(),
            clock,
            env,
            cwd: CASE_DIR.as_bytes().to_vec(),
            ..kernel::SandboxConfig::default()
        };
        let sb = self.kernel.create_sandbox_from(&self.base, cfg).map_err(|e| format!("create_sandbox: {e:?}"))?;
        let fs = sb.fs();
        fs.mkdir_all(CASE_DIR.as_bytes(), 0o755).map_err(|e| format!("mkdir {CASE_DIR}: {e:?}"))?;
        for (rel, entry) in &inv.files.entries {
            let path = format!("{CASE_DIR}/{rel}");
            let p = path.as_bytes();
            let r = match entry {
                Entry::File { data: Some(d), mode, .. } => fs.write_file(p, d.as_slice(), *mode),
                Entry::File { data: None, mode, .. } => fs.write_file(p, b"", *mode),
                Entry::Dir { mode } => fs.mkdir_all(p, *mode),
                Entry::Symlink { target } => fs.symlink(target.as_bytes(), p),
            };
            r.map_err(|e| format!("fixture {rel}: {e:?}"))?;
        }
        let t = sysabi::TimeSpec { sec: harness::FIXTURE_MTIME as i64, nsec: 0 };
        for rel in inv.files.entries.keys() {
            let _ = fs.set_mtime(format!("{CASE_DIR}/{rel}").as_bytes(), t);
        }
        Ok(sb)
    }
}

#[cfg(feature = "kernel")]
impl Candidate for KernelCandidate {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        let sb = match self.sandbox_for(inv) {
            Ok(sb) => sb,
            Err(e) => return Outcome::unsupported(e),
        };
        let argv: Vec<Vec<u8>> = match &inv.script {
            Some(s) => vec![b"bash".to_vec(), b"-c".to_vec(), s.as_bytes().to_vec()],
            None => inv.argv.iter().map(|a| a.as_bytes().to_vec()).collect(),
        };
        let req = kernel::RunRequest {
            argv,
            env: None,
            cwd: Some(CASE_DIR.as_bytes().to_vec()),
            stdin: inv.stdin.clone(),
            timeout: Some(self.timeout),
        };
        let out = match sb.run(req) {
            Ok(o) => o,
            Err(e) => return Outcome::unsupported(format!("run: {e:?}")),
        };
        let (exit, signal) = match out.status {
            WaitStatus::Exited(c) => (Some(c), None),
            WaitStatus::Signaled { signal, .. } => (None, Some(signal.0)),
            WaitStatus::Stopped(s) => (None, Some(s.0)),
            WaitStatus::Continued => (Some(0), None),
        };
        let files = match sb.fs().tree(CASE_DIR.as_bytes()) {
            Ok(t) => kernel_tree_to_memtree(t),
            Err(e) => return Outcome::unsupported(format!("tree: {e:?}")),
        };
        Outcome {
            stdout: out.stdout.into(),
            stderr: out.stderr.into(),
            exit,
            signal,
            timed_out: out.timed_out,
            files,
            unsupported: None,
        }
    }
}

#[cfg(feature = "kernel")]
pub fn kernel_tree_to_memtree(tree: Vec<(Vec<u8>, kernel::TreeEntry)>) -> MemTree {
    tree_to_memtree(
        tree.into_iter()
            .map(|(p, e)| {
                let e = match e {
                    kernel::TreeEntry::File { data, mode } => TreeEntry::File { data, mode },
                    kernel::TreeEntry::Dir { mode } => TreeEntry::Dir { mode },
                    kernel::TreeEntry::Symlink { target } => TreeEntry::Symlink { target },
                    kernel::TreeEntry::Other { mode } => TreeEntry::Other { mode },
                };
                (p, e)
            })
            .collect(),
    )
}

pub fn tree_to_memtree(tree: Vec<(Vec<u8>, TreeEntry)>) -> MemTree {
    let mut m = MemTree::new();
    for (rel, e) in tree {
        let rel = String::from_utf8_lossy(&rel).into_owned();
        let entry = match e {
            TreeEntry::File { data, mode } => Entry::file(data, mode),
            TreeEntry::Dir { mode } => Entry::dir(mode),
            TreeEntry::Symlink { target } => Entry::symlink(String::from_utf8_lossy(&target).into_owned()),
            TreeEntry::Other { mode } => Entry::file(Vec::new(), mode | 0o170000),
        };
        m.entries.insert(rel, entry);
    }
    m
}

/// "AAAA-MM-DD HH:MM:SS" (UTC) em segundos desde a época.
pub fn parse_faketime(ts: &str) -> Option<i64> {
    let ts = ts.trim().trim_start_matches('@');
    let (date, time) = ts.split_once(' ').unwrap_or((ts, "00:00:00"));
    let mut d = date.split('-').map(|x| x.parse::<i64>());
    let (y, mo, da) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.split(':').map(|x| x.parse::<i64>());
    let (h, mi, s) = (t.next()?.ok()?, t.next().unwrap_or(Ok(0)).ok()?, t.next().unwrap_or(Ok(0)).ok()?);
    // Dias desde 1970-01-01 (algoritmo de Howard Hinnant).
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + s)
}

/// Placar de uma ferramenta do golden.
pub struct Report {
    pub tool: String,
    pub conformance: Conformance,
    pub missing_golden: usize,
    pub comparisons: Vec<harness::CaseComparison>,
}

impl Report {
    pub fn strict_rate(&self) -> f64 {
        self.conformance.strict_rate()
    }

    pub fn lenient_rate(&self) -> f64 {
        self.conformance.lenient_rate()
    }

    /// Resumo legível (vai pro stderr do teste; rode com `--nocapture` pra ver).
    pub fn summary(&self) -> String {
        let c = &self.conformance;
        let mut s = format!(
            "{}: {}/{} estrito ({:.1}%), {}/{} leniente ({:.1}%), {} sem suporte, {} casos sem golden\n",
            self.tool,
            c.strict_pass,
            c.total,
            100.0 * c.strict_rate(),
            c.lenient_pass,
            c.total,
            100.0 * c.lenient_rate(),
            c.unsupported,
            self.missing_golden
        );
        for f in self.comparisons.iter().filter(|c| !c.strict).take(40) {
            s.push_str(&format!("  FALHA {}: {}\n", f.id, f.detail.join(" | ")));
        }
        s
    }

    pub fn print(&self) {
        // Teste pode imprimir: este crate só roda no host, como ferramenta de teste.
        eprint!("{}", self.summary());
    }

    /// Ids que falham, pra quem quiser manter uma lista de falhas conhecidas.
    pub fn failing_ids(&self) -> Vec<String> {
        self.comparisons.iter().filter(|c| !c.strict).map(|c| c.id.clone()).collect()
    }
}

/// Tag de caso cuja saída depende da ordem do readdir (o oráculo roda em overlayfs sobre ext4, o
/// pseudo-linus segue o tmpfs): stdout é comparado com as linhas ordenadas.
pub const ORDER_INSENSITIVE: &str = "order-insensitive";

fn sorted_lines(b: &harness::Bytes) -> harness::Bytes {
    let mut lines: Vec<&[u8]> = b.as_slice().split_inclusive(|c| *c == b'\n').collect();
    lines.sort();
    harness::Bytes(lines.concat())
}

/// Como `harness::score`, mais a normalização de casos `order-insensitive`.
fn score_cases(cand: &dyn Candidate, cases: &[(harness::Case, Outcome)]) -> (Conformance, Vec<harness::CaseComparison>) {
    let mut conf = Conformance { candidate: cand.name(), ..Conformance::default() };
    let mut all = Vec::with_capacity(cases.len());
    for (case, golden) in cases {
        let actual = match case.invocation() {
            Ok(inv) => match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cand.run(&inv))) {
                Ok(o) => o,
                Err(_) => Outcome::unsupported("panic no candidato"),
            },
            Err(e) => Outcome::unsupported(format!("caso inválido: {e}")),
        };
        let cmp = if case.tags.iter().any(|t| t == ORDER_INSENSITIVE) {
            let mut g = golden.clone();
            let mut a = actual;
            g.stdout = sorted_lines(&g.stdout);
            a.stdout = sorted_lines(&a.stdout);
            harness::compare_outcome(case, &g, &a)
        } else {
            harness::compare_outcome(case, golden, &actual)
        };
        all.push(cmp);
    }
    for cmp in &all {
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
    (conf, all)
}

/// Roda todos os casos de `testbench/corpus/cases/<tool>` que têm golden.
pub fn score_tool(tool: &str, cand: &dyn Candidate) -> Report {
    let (cases, missing_golden) = harness::paths::load_tool(tool).expect("carregar casos e golden");
    let (conformance, comparisons) = score_cases(cand, &cases);
    Report { tool: tool.to_string(), conformance, missing_golden, comparisons }
}

/// Roda só os casos cujo id está em `ids` (útil pra depurar).
pub fn score_ids(tool: &str, cand: &dyn Candidate, ids: &[&str]) -> Report {
    let (cases, _) = harness::paths::load_tool(tool).expect("carregar casos e golden");
    let picked: Vec<_> = cases.into_iter().filter(|(c, _)| ids.contains(&c.id.as_str())).collect();
    let (conformance, comparisons) = score_cases(cand, &picked);
    Report { tool: tool.to_string(), conformance, missing_golden: 0, comparisons }
}

/// Mapa tool -> placar, pra relatórios agregados.
pub fn score_tools(tools: &[&str], cand: &dyn Candidate) -> BTreeMap<String, Report> {
    tools.iter().map(|t| (t.to_string(), score_tool(t, cand))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faketime_parse() {
        assert_eq!(parse_faketime("2026-01-15 12:00:00"), Some(1_768_478_400));
        assert_eq!(parse_faketime("1970-01-01 00:00:00"), Some(0));
    }
}
