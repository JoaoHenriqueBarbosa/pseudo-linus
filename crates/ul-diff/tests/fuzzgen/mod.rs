//! Geradores determinísticos dos corpora aleatórios (o do F08 pro diff e um de triplas pro diff3), cache
//! das saídas do oráculo e placar. Compartilhado pelos testes `random_*`.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use harness::{Candidate, Case, FileSpec, Oracle, Outcome};

/// Gerador congruencial linear (o mesmo do F08).
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

pub const CODE_LINES: &[&str] = &[
    "}", "", "{", "    return 0;", "    }", "    if (x) {", "        x++;", "    } else {", "int f(void)", "    break;",
    "    return x;", "#include <stdio.h>", "        return 1;", "    for (;;) {", "// comment", "end",
];

pub const TEXT_LINES: &[&str] = &[
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu", "nu",
    "xi", "omicron", "pi", "rho", "sigma", "tau", "upsilon", "phi", "chi", "psi", "omega",
];

#[derive(Clone, Copy, Debug)]
pub enum Family {
    Tiny,
    Code,
    Text,
    Medium,
}

pub struct Pair {
    pub family: Family,
    pub a: String,
    pub b: String,
}

pub fn render(lines: &[String], newline_at_end: bool) -> String {
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        out.push_str(l);
        if i + 1 < lines.len() || newline_at_end {
            out.push('\n');
        }
    }
    out
}

/// De 1 a `max_edits` edições aleatórias (inserir, apagar, trocar, mover bloco).
pub fn mutate(rng: &mut Lcg, a: &[String], pick: &mut dyn FnMut(&mut Lcg) -> String, max_edits: u64) -> Vec<String> {
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

/// O corpus de pares do F08.
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
        let nl_a = rng.below(10) != 0;
        let nl_b = rng.below(10) != 0;
        pairs.push(Pair { family, a: render(&a, nl_a || a.is_empty()), b: render(&b, nl_b || b.is_empty()) });
    }
    pairs
}

/// Triplas (meu, velho, seu): o velho aleatório, meu e seu mutados dele de forma independente.
pub fn generate_triples(seed: u64, count: usize) -> Vec<[String; 3]> {
    let mut rng = Lcg::new(seed);
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let family = match i % 4 {
            0 | 1 => Family::Tiny,
            2 => Family::Code,
            _ => Family::Text,
        };
        let len = match family {
            Family::Tiny => rng.below(14),
            Family::Code => 4 + rng.below(25),
            _ => 4 + rng.below(30),
        };
        let alphabet = 2 + rng.below(3);
        let mut pick = |r: &mut Lcg| -> String {
            match family {
                Family::Tiny => ((b'a' + r.below(alphabet) as u8) as char).to_string(),
                Family::Code => CODE_LINES[r.below(CODE_LINES.len() as u64) as usize].to_string(),
                _ => format!("{} {}", TEXT_LINES[r.below(TEXT_LINES.len() as u64) as usize], r.below(20)),
            }
        };
        let old: Vec<String> = (0..len).map(|_| pick(&mut rng)).collect();
        let mine = mutate(&mut rng, &old, &mut pick, 3);
        let yours = if rng.below(6) == 0 { mine.clone() } else { mutate(&mut rng, &old, &mut pick, 3) };
        let nl = |r: &mut Lcg| r.below(12) != 0;
        let (n0, n1, n2) = (nl(&mut rng), nl(&mut rng), nl(&mut rng));
        out.push([
            render(&mine, n0 || mine.is_empty()),
            render(&old, n1 || old.is_empty()),
            render(&yours, n2 || yours.is_empty()),
        ]);
    }
    out
}

pub fn case_with_files(id: String, argv: Vec<String>, files: &[(&str, &str)]) -> Case {
    let mut map = BTreeMap::new();
    for (k, v) in files {
        map.insert(k.to_string(), FileSpec::Text(v.to_string()));
    }
    Case {
        id,
        argv,
        script: None,
        stdin: None,
        stdin_b64: None,
        files: map,
        env: Default::default(),
        tags: Vec::new(),
        faketime: None,
        timeout_ms: None,
    }
}

pub fn case_for(id: String, pair: &Pair, flags: &[&str]) -> Case {
    let mut argv: Vec<String> = vec!["diff".into()];
    argv.extend(flags.iter().map(|s| s.to_string()));
    argv.extend(["a".to_string(), "b".to_string()]);
    case_with_files(id, argv, &[("a", &pair.a), ("b", &pair.b)])
}

fn cache_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("ul-diff-oracle-{name}.json"))
}

/// Saídas do GNU pros casos, do cache ou do oráculo (um container por lote de 400).
pub fn gnu_outcomes(name: &str, cases: &[Case]) -> Vec<Outcome> {
    let path = cache_path(name);
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(v) = serde_json::from_str::<Vec<Outcome>>(&text)
        && v.len() == cases.len()
    {
        return v;
    }
    let oracle = Oracle::locate().expect("oráculo (docker) disponível");
    let mut all = Vec::new();
    for chunk in cases.chunks(400) {
        all.extend(oracle.run(chunk).expect("rodar no oráculo"));
    }
    let _ = std::fs::write(&path, serde_json::to_string(&all).expect("json"));
    all
}

pub struct Score {
    pub total: usize,
    pub same: usize,
    pub failing: Vec<String>,
}

/// Compara stdout, stderr, exit e a árvore final.
pub fn score(cases: &[Case], gnu: &[Outcome], cand: &dyn Candidate) -> Score {
    let mut s = Score { total: 0, same: 0, failing: Vec::new() };
    for (case, g) in cases.iter().zip(gnu) {
        let inv = case.invocation().expect("caso");
        let ours = cand.run(&inv);
        s.total += 1;
        if ours.stdout == g.stdout && ours.exit == g.exit && ours.stderr == g.stderr && ours.files.diff(&g.files).is_empty()
        {
            s.same += 1;
        } else {
            s.failing.push(case.id.clone());
        }
    }
    s
}
