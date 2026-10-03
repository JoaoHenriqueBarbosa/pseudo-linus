//! Corpus aleatório de pares de arquivos (semente fixa), com o GNU diff rodando no oráculo em tempo de
//! execução. Mede a concordância de alinhamento de cada motor em muito mais empates do que o corpus manual
//! cobre, e classifica a divergência pelo custo do script de edição.

use anyhow::Result;
use harness::{Case, FileSpec, Oracle};
use serde::Serialize;

use super::cli::{GnuRenderer, Opts, Renderer, Style};
use super::engines::Engine;
use super::text::{Normalize, intern, split_lines};

/// Gerador congruencial linear (determinístico, sem dependência).
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

const CODE_LINES: &[&str] = &[
    "}", "", "{", "    return 0;", "    }", "    if (x) {", "        x++;", "    } else {", "int f(void)", "    break;",
    "    return x;", "#include <stdio.h>", "        return 1;", "    for (;;) {", "// comment", "end",
];

const TEXT_LINES: &[&str] = &[
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu", "nu",
    "xi", "omicron", "pi", "rho", "sigma", "tau", "upsilon", "phi", "chi", "psi", "omega",
];

#[derive(Clone, Copy, Debug, Serialize)]
pub enum Family {
    /// Alfabeto de 2 a 4 linhas: quase tudo é empate.
    Tiny,
    /// Linhas típicas de código, com muita repetição ("}", vazias).
    Code,
    /// Linhas quase únicas, edições pequenas.
    Text,
    /// Arquivos médios (200 a 600 linhas) com repetição alta.
    Medium,
}

pub struct Pair {
    pub family: Family,
    pub a: Vec<u8>,
    pub b: Vec<u8>,
}

fn render(lines: &[String], newline_at_end: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        out.extend_from_slice(l.as_bytes());
        if i + 1 < lines.len() || newline_at_end {
            out.push(b'\n');
        }
    }
    out
}

/// Aplica de 1 a `max_edits` edições aleatórias (inserir, apagar, trocar, mover bloco).
fn mutate(rng: &mut Lcg, a: &[String], pick: &mut dyn FnMut(&mut Lcg) -> String, max_edits: u64) -> Vec<String> {
    let mut b = a.to_vec();
    let edits = 1 + rng.below(max_edits);
    for _ in 0..edits {
        let len = b.len() as u64;
        match rng.below(4) {
            0 => {
                let at = rng.below(len + 1) as usize;
                let n = 1 + rng.below(3);
                for _ in 0..n {
                    let l = pick(rng);
                    b.insert(at, l);
                }
            }
            1 if len > 0 => {
                let at = rng.below(len) as usize;
                let n = (1 + rng.below(3)).min(len - at as u64) as usize;
                b.drain(at..at + n);
            }
            2 if len > 0 => {
                let at = rng.below(len) as usize;
                b[at] = pick(rng);
            }
            3 if len > 2 => {
                let at = rng.below(len - 1) as usize;
                let n = (1 + rng.below(3)).min(len - at as u64) as usize;
                let block: Vec<String> = b.drain(at..at + n).collect();
                let to = rng.below(b.len() as u64 + 1) as usize;
                for (k, l) in block.into_iter().enumerate() {
                    b.insert(to + k, l);
                }
            }
            _ => {}
        }
    }
    b
}

pub fn generate(seed: u64, count: usize) -> Vec<Pair> {
    let mut rng = Lcg::new(seed);
    let mut pairs = Vec::with_capacity(count);
    for i in 0..count {
        let family = match i % 8 {
            0..=2 => Family::Tiny,
            3..=4 => Family::Code,
            5..=6 => Family::Text,
            _ => Family::Medium,
        };
        let (len, max_edits): (u64, u64) = match family {
            Family::Tiny => (rng.below(16), 4),
            Family::Code => (5 + rng.below(30), 5),
            Family::Text => (5 + rng.below(40), 4),
            Family::Medium => (200 + rng.below(400), 25),
        };
        let alphabet = 2 + rng.below(3);
        let mut pick = |r: &mut Lcg| -> String {
            match family {
                Family::Tiny => ((b'a' + r.below(alphabet) as u8) as char).to_string(),
                Family::Code => CODE_LINES[r.below(CODE_LINES.len() as u64) as usize].to_string(),
                Family::Text => {
                    let w = TEXT_LINES[r.below(TEXT_LINES.len() as u64) as usize];
                    format!("{w} {}", r.below(50))
                }
                Family::Medium => format!("line {}", r.below(12)),
            }
        };
        let a: Vec<String> = (0..len).map(|_| pick(&mut rng)).collect();
        let b = mutate(&mut rng, &a, &mut pick, max_edits);
        // ~1 em 10 pares termina sem newline em um dos lados.
        let nl_a = rng.below(10) != 0;
        let nl_b = rng.below(10) != 0;
        pairs.push(Pair { family, a: render(&a, nl_a || a.is_empty()), b: render(&b, nl_b || b.is_empty()) });
    }
    pairs
}

fn case_for(id: String, pair: &Pair, flags: &[&str]) -> Case {
    use base64::Engine as _;
    let enc = |d: &[u8]| {
        FileSpec::Table(harness::case::FileTable {
            content_b64: Some(base64::engine::general_purpose::STANDARD.encode(d)),
            ..Default::default()
        })
    };
    let mut argv: Vec<String> = vec!["diff".into()];
    argv.extend(flags.iter().map(|s| s.to_string()));
    argv.extend(["a".to_string(), "b".to_string()]);
    let mut case = Case {
        id,
        argv,
        script: None,
        stdin: None,
        stdin_b64: None,
        files: Default::default(),
        env: Default::default(),
        tags: Vec::new(),
        faketime: None,
        timeout_ms: None,
    };
    case.files.insert("a".into(), enc(&pair.a));
    case.files.insert("b".into(), enc(&pair.b));
    case
}

/// Saída do GNU pra cada par (com `flags`, ex.: `[]` ou `["-u"]`), rodando todos num container só.
pub fn gnu_outputs(oracle: &Oracle, pairs: &[Pair], flags: &[&str]) -> Result<Vec<Vec<u8>>> {
    let cases: Vec<Case> =
        pairs.iter().enumerate().map(|(i, p)| case_for(format!("fuzz-{i:04}"), p, flags)).collect();
    let outs = oracle.run(&cases)?;
    Ok(outs.into_iter().map(|o| o.stdout.0).collect())
}

/// Linhas apagadas + inseridas numa saída no formato normal.
pub fn cost_of_normal(out: &[u8]) -> usize {
    out.split(|&c| c == b'\n')
        .filter(|l| {
            l.starts_with(b"< ")
                || l.starts_with(b"> ")
                || (l.starts_with(b"-") && !l.starts_with(b"---"))
                || (l.starts_with(b"+") && !l.starts_with(b"+++"))
        })
        .count()
}

#[derive(Debug, Default, Serialize)]
pub struct Agreement {
    pub engine: String,
    pub total: usize,
    pub identical: usize,
    /// Mesmo número de linhas mudadas que o GNU, alinhamento diferente.
    pub same_cost_other_alignment: usize,
    /// Script mais longo que o do GNU.
    pub longer: usize,
    /// Script mais curto que o do GNU (o GNU não foi mínimo).
    pub shorter: usize,
    pub invalid: usize,
    pub by_family: std::collections::BTreeMap<String, (usize, usize)>,
    /// Até 3 exemplos de par divergente (índices).
    pub examples: Vec<usize>,
}

/// Concordância de um motor no formato normal (que expõe o alinhamento sem contexto).
pub fn agreement(engine: Box<dyn Engine>, pairs: &[Pair], gnu: &[Vec<u8>]) -> Agreement {
    let renderer = GnuRenderer { engine };
    agreement_with(&renderer, Style::Normal, pairs, gnu)
}

/// Concordância de um renderizador qualquer num estilo (normal ou -u com cabeçalho de data fixa).
pub fn agreement_with(renderer: &dyn Renderer, style: Style, pairs: &[Pair], gnu: &[Vec<u8>]) -> Agreement {
    let opts = Opts { style, ..Opts::default() };
    let stamp = super::cli::format_mtime(harness::FIXTURE_MTIME as i64, "UTC");
    let (ha, hb) = (format!("a\t{stamp}"), format!("b\t{stamp}"));
    let mut ag = Agreement { engine: renderer.name(), ..Agreement::default() };
    for (i, (pair, expected)) in pairs.iter().zip(gnu).enumerate() {
        ag.total += 1;
        let family = format!("{:?}", pair.family);
        let slot = ag.by_family.entry(family).or_default();
        slot.1 += 1;
        let ours = if pair.a == pair.b {
            Vec::new()
        } else {
            let la = split_lines(&pair.a);
            let lb = split_lines(&pair.b);
            let it = intern(&la, &lb, &Normalize::default());
            if it.a == it.b {
                Vec::new()
            } else {
                renderer.render(&opts, &pair.a, &pair.b, (&ha, &hb)).map(|(o, _)| o).unwrap_or_default()
            }
        };
        if ours.starts_with(b"<alinhamento invalido>") {
            ag.invalid += 1;
            continue;
        }
        if &ours == expected {
            ag.identical += 1;
            slot.0 += 1;
            continue;
        }
        if ag.examples.len() < 3 {
            ag.examples.push(i);
        }
        let (c_ours, c_gnu) = (cost_of_normal(&ours), cost_of_normal(expected));
        match c_ours.cmp(&c_gnu) {
            std::cmp::Ordering::Equal => ag.same_cost_other_alignment += 1,
            std::cmp::Ordering::Greater => ag.longer += 1,
            std::cmp::Ordering::Less => ag.shorter += 1,
        }
    }
    ag
}

/// Como o alvo do patch foi derivado do original.
#[derive(Clone, Copy, Debug, Serialize)]
pub enum Drift {
    /// Alvo igual ao original: aplicação limpa.
    Clean,
    /// Linhas inseridas antes: deslocamento.
    Offset,
    /// Uma linha qualquer alterada: fuzz, rejeição ou nada.
    Edited,
    /// Deslocamento e linha alterada.
    Both,
    /// Alvo já é o arquivo novo: patch já aplicado.
    Applied,
}

pub struct PatchTrial {
    pub drift: Drift,
    pub inv: harness::Invocation,
    pub case: Case,
    pub gnu: harness::Outcome,
}

/// Gera pares (original, novo) e um alvo derivado; o GNU faz `diff -u` e aplica com `patch` no oráculo.
pub fn patch_trials(oracle: &Oracle, seed: u64, count: usize) -> Result<Vec<PatchTrial>> {
    use base64::Engine as _;
    let enc = |d: &[u8]| {
        FileSpec::Table(harness::case::FileTable {
            content_b64: Some(base64::engine::general_purpose::STANDARD.encode(d)),
            ..Default::default()
        })
    };
    let mut rng = Lcg::new(seed);
    let mut cases = Vec::new();
    let mut drifts = Vec::new();
    let mut targets = Vec::new();
    for i in 0..count {
        let code = i % 2 == 0;
        let len = 8 + rng.below(40);
        let mut pick = |r: &mut Lcg| -> String {
            if code {
                CODE_LINES[r.below(CODE_LINES.len() as u64) as usize].to_string()
            } else {
                format!("{} {}", TEXT_LINES[r.below(TEXT_LINES.len() as u64) as usize], r.below(30))
            }
        };
        let a: Vec<String> = (0..len).map(|_| pick(&mut rng)).collect();
        let b = mutate(&mut rng, &a, &mut pick, 3);
        let drift = match i % 5 {
            0 => Drift::Clean,
            1 => Drift::Offset,
            2 => Drift::Edited,
            3 => Drift::Both,
            _ => Drift::Applied,
        };
        let mut t = match drift {
            Drift::Applied => b.clone(),
            _ => a.clone(),
        };
        if matches!(drift, Drift::Offset | Drift::Both) {
            let at = rng.below(t.len() as u64 / 2 + 1) as usize;
            for _ in 0..1 + rng.below(4) {
                let l = pick(&mut rng);
                t.insert(at, l);
            }
        }
        if matches!(drift, Drift::Edited | Drift::Both) && !t.is_empty() {
            let at = rng.below(t.len() as u64) as usize;
            t[at] = format!("{} (local)", t[at]);
        }
        let (ra, rb, rt) = (render(&a, true), render(&b, true), render(&t, true));
        let mut case = Case {
            id: format!("patch-fuzz-{i:04}"),
            argv: Vec::new(),
            script: Some(
                "diff -u a b > fix.patch; rm -f a b; patch t fix.patch > .out 2> .err < /dev/null; echo -n $? > .status"
                    .into(),
            ),
            stdin: None,
            stdin_b64: None,
            files: Default::default(),
            env: Default::default(),
            tags: vec![format!("{drift:?}").to_lowercase()],
            faketime: None,
            timeout_ms: None,
        };
        case.files.insert("a".into(), enc(&ra));
        case.files.insert("b".into(), enc(&rb));
        case.files.insert("t".into(), enc(&rt));
        cases.push(case);
        drifts.push(drift);
        targets.push(rt);
    }
    let outs = oracle.run(&cases)?;
    let mut trials = Vec::new();
    for (((case, out), drift), target) in cases.into_iter().zip(outs).zip(drifts).zip(targets) {
        let mut files = out.files.clone();
        let take = |files: &mut harness::MemTree, name: &str| -> Vec<u8> {
            files.entries.remove(name).and_then(|e| e.data().map(|d| d.to_vec())).unwrap_or_default()
        };
        let stdout = take(&mut files, ".out");
        let stderr = take(&mut files, ".err");
        let status: i32 = String::from_utf8_lossy(&take(&mut files, ".status")).trim().parse().unwrap_or(-1);
        let patch_text = files.read("fix.patch").map(|d| d.to_vec()).unwrap_or_default();
        let mut input = harness::MemTree::new();
        input.insert("t", harness::Entry::file(target, 0o644));
        input.insert("fix.patch", harness::Entry::file(patch_text, 0o644));
        let inv = harness::Invocation {
            case_id: case.id.clone(),
            argv: vec!["patch".into(), "t".into(), "fix.patch".into()],
            script: None,
            stdin: Vec::new(),
            files: input,
            env: Default::default(),
            faketime: None,
        };
        let gnu = harness::Outcome::exited(stdout, stderr, status, files);
        trials.push(PatchTrial { drift, inv, case, gnu });
    }
    Ok(trials)
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PatchAgreement {
    pub engine: String,
    pub total: usize,
    /// stdout, exit e arquivos iguais (stderr pode divergir).
    pub lenient: usize,
    /// Só o conteúdo final do alvo e o exit code.
    pub content: usize,
    pub by_drift: std::collections::BTreeMap<String, (usize, usize, usize)>,
}

pub fn patch_agreement(candidate: &dyn harness::Candidate, trials: &[PatchTrial]) -> PatchAgreement {
    let mut ag = PatchAgreement { engine: candidate.name(), ..PatchAgreement::default() };
    for t in trials {
        let actual = candidate.run(&t.inv);
        let cmp = harness::compare_outcome(&t.case, &t.gnu, &actual);
        let content_ok = actual.unsupported.is_none()
            && actual.exit == t.gnu.exit
            && actual.files.read("t") == t.gnu.files.read("t")
            && actual.files.get("t").is_some() == t.gnu.files.get("t").is_some();
        if !cmp.lenient && std::env::var("F08_DEBUG_PATCH_FUZZ").is_ok_and(|v| candidate.name().contains(&v)) {
            println!("--- {} {:?}\n{}", t.case.id, t.drift, cmp.detail.join("\n"));
            println!("patch:\n{}", String::from_utf8_lossy(t.inv.files.read("fix.patch").unwrap_or_default()));
            println!("alvo:\n{}", String::from_utf8_lossy(t.inv.files.read("t").unwrap_or_default()));
            println!("GNU stdout:\n{}\nnosso:\n{}", String::from_utf8_lossy(t.gnu.stdout.as_slice()), String::from_utf8_lossy(actual.stdout.as_slice()));
            println!(
                "GNU rej:\n{}\nnosso rej:\n{}",
                String::from_utf8_lossy(t.gnu.files.read("t.rej").unwrap_or_default()),
                String::from_utf8_lossy(actual.files.read("t.rej").unwrap_or_default())
            );
        }
        ag.total += 1;
        ag.lenient += cmp.lenient as usize;
        ag.content += content_ok as usize;
        let slot = ag.by_drift.entry(format!("{:?}", t.drift)).or_default();
        slot.0 += cmp.lenient as usize;
        slot.1 += content_ok as usize;
        slot.2 += 1;
    }
    ag
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic() {
        let a = generate(7, 20);
        let b = generate(7, 20);
        assert!(a.iter().zip(&b).all(|(x, y)| x.a == y.a && x.b == y.b));
        assert_eq!(cost_of_normal(b"1c1\n< a\n---\n> b\n"), 2);
    }
}
