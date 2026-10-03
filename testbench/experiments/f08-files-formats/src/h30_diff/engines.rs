//! Motores de alinhamento: cada biblioteca recebe as duas sequências de ids e devolve, como o GNU
//! guarda internamente, um vetor `changed` por arquivo (linha apagada no primeiro, inserida no segundo).

use imara_diff::{IndentHeuristic, IndentLevel, NoSliderHeuristic};

pub trait Engine: Send + Sync {
    /// Nome curto do motor (ex.: "similar Myers").
    fn label(&self) -> String;
    /// Crate e versão que o motor representa.
    fn krate(&self) -> (&'static str, &'static str);
    fn changes(&self, a: &[u32], b: &[u32], reps: &[&[u8]]) -> (Vec<bool>, Vec<bool>);
}

pub struct SimilarEngine {
    pub algorithm: similar::Algorithm,
    pub label: &'static str,
}

impl Engine for SimilarEngine {
    fn label(&self) -> String {
        format!("similar {}", self.label)
    }

    fn krate(&self) -> (&'static str, &'static str) {
        ("similar", "3.2.0")
    }

    fn changes(&self, a: &[u32], b: &[u32], _reps: &[&[u8]]) -> (Vec<bool>, Vec<bool>) {
        let mut c0 = vec![false; a.len()];
        let mut c1 = vec![false; b.len()];
        for op in similar::capture_diff_slices(self.algorithm, a, b) {
            for i in op.old_range() {
                if !matches!(op, similar::DiffOp::Equal { .. }) {
                    c0[i] = true;
                }
            }
            for j in op.new_range() {
                if !matches!(op, similar::DiffOp::Equal { .. }) {
                    c1[j] = true;
                }
            }
        }
        (c0, c1)
    }
}

#[derive(Clone, Copy)]
pub enum ImaraPost {
    /// Sem pós-processamento: saída crua do algoritmo.
    None,
    /// Desliza os blocos pra baixo e junta blocos, sem heurística de indentação.
    NoHeuristic,
    /// Pós-processamento do git (heurística de indentação), o padrão recomendado pela crate.
    Indent,
}

pub struct ImaraEngine {
    pub algorithm: imara_diff::Algorithm,
    pub post: ImaraPost,
}

impl Engine for ImaraEngine {
    fn label(&self) -> String {
        let alg = match self.algorithm {
            imara_diff::Algorithm::Histogram => "Histogram",
            imara_diff::Algorithm::Myers => "Myers",
            imara_diff::Algorithm::MyersMinimal => "MyersMinimal",
        };
        let post = match self.post {
            ImaraPost::None => "sem pós-processamento",
            ImaraPost::NoHeuristic => "postprocess_no_heuristic",
            ImaraPost::Indent => "postprocess_lines",
        };
        format!("imara-diff {alg} ({post})")
    }

    fn krate(&self) -> (&'static str, &'static str) {
        ("imara-diff", "0.2.0")
    }

    fn changes(&self, a: &[u32], b: &[u32], reps: &[&[u8]]) -> (Vec<bool>, Vec<bool>) {
        let before: Vec<imara_diff::Token> = a.iter().map(|&t| imara_diff::Token(t)).collect();
        let after: Vec<imara_diff::Token> = b.iter().map(|&t| imara_diff::Token(t)).collect();
        let mut diff = imara_diff::Diff::default();
        diff.compute_with(self.algorithm, &before, &after, reps.len() as u32);
        match self.post {
            ImaraPost::None => {}
            ImaraPost::NoHeuristic => diff.postprocess_with(&before, &after, NoSliderHeuristic),
            ImaraPost::Indent => diff.postprocess_with(
                &before,
                &after,
                IndentHeuristic::new(|token: imara_diff::Token| {
                    IndentLevel::for_ascii_line(reps[token.0 as usize].iter().copied(), 8)
                }),
            ),
        }
        let c0 = (0..a.len() as u32).map(|i| diff.is_removed(i)).collect();
        let c1 = (0..b.len() as u32).map(|j| diff.is_added(j)).collect();
        (c0, c1)
    }
}

/// diffy só diferencia texto: cada id vira uma linha "id\n" e o contexto é grande o bastante pra sair um
/// hunk só, de onde se lê o alinhamento.
pub struct DiffyEngine;

impl Engine for DiffyEngine {
    fn label(&self) -> String {
        "diffy Myers".into()
    }

    fn krate(&self) -> (&'static str, &'static str) {
        ("diffy", "0.5.2")
    }

    fn changes(&self, a: &[u32], b: &[u32], _reps: &[&[u8]]) -> (Vec<bool>, Vec<bool>) {
        let encode = |ids: &[u32]| -> Vec<u8> {
            let mut out = Vec::new();
            for id in ids {
                out.extend_from_slice(format!("{id}\n").as_bytes());
            }
            out
        };
        let ta = encode(a);
        let tb = encode(b);
        let mut opts = diffy::DiffOptions::new();
        opts.set_context_len(a.len() + b.len() + 1);
        let patch = opts.create_patch_bytes(&ta, &tb);
        let mut c0 = vec![false; a.len()];
        let mut c1 = vec![false; b.len()];
        for hunk in patch.hunks() {
            let mut i0 = hunk.old_range().start().saturating_sub(1);
            let mut i1 = hunk.new_range().start().saturating_sub(1);
            if hunk.old_range().is_empty() {
                i0 = hunk.old_range().start();
            }
            if hunk.new_range().is_empty() {
                i1 = hunk.new_range().start();
            }
            for line in hunk.lines() {
                match line {
                    diffy::Line::Context(_) => {
                        i0 += 1;
                        i1 += 1;
                    }
                    diffy::Line::Delete(_) => {
                        c0[i0] = true;
                        i0 += 1;
                    }
                    diffy::Line::Insert(_) => {
                        c1[i1] = true;
                        i1 += 1;
                    }
                }
            }
        }
        (c0, c1)
    }
}

/// O alinhamento que o uutils diffutils 0.5 usa por dentro: crate `diff` (LCS por programação dinâmica).
pub struct LcsDiffCrateEngine;

impl Engine for LcsDiffCrateEngine {
    fn label(&self) -> String {
        "diff (LCS do diffutils)".into()
    }

    fn krate(&self) -> (&'static str, &'static str) {
        ("diff", "0.1.13")
    }

    fn changes(&self, a: &[u32], b: &[u32], _reps: &[&[u8]]) -> (Vec<bool>, Vec<bool>) {
        let mut c0 = vec![false; a.len()];
        let mut c1 = vec![false; b.len()];
        let (mut i0, mut i1) = (0usize, 0usize);
        for r in diff::slice(a, b) {
            match r {
                diff::Result::Left(_) => {
                    c0[i0] = true;
                    i0 += 1;
                }
                diff::Result::Right(_) => {
                    c1[i1] = true;
                    i1 += 1;
                }
                diff::Result::Both(_, _) => {
                    i0 += 1;
                    i1 += 1;
                }
            }
        }
        (c0, c1)
    }
}

/// Confere que o alinhamento é válido: as linhas não marcadas casam uma a uma.
pub fn is_valid_alignment(a: &[u32], b: &[u32], c0: &[bool], c1: &[bool]) -> bool {
    let keep_a: Vec<u32> = a.iter().zip(c0).filter(|(_, c)| !**c).map(|(x, _)| *x).collect();
    let keep_b: Vec<u32> = b.iter().zip(c1).filter(|(_, c)| !**c).map(|(x, _)| *x).collect();
    keep_a == keep_b
}

/// Todos os motores medidos.
pub fn all_engines() -> Vec<Box<dyn Engine>> {
    vec![
        Box::new(SimilarEngine { algorithm: similar::Algorithm::Myers, label: "Myers" }),
        Box::new(SimilarEngine { algorithm: similar::Algorithm::RawMyers, label: "RawMyers" }),
        Box::new(SimilarEngine { algorithm: similar::Algorithm::Patience, label: "Patience" }),
        Box::new(SimilarEngine { algorithm: similar::Algorithm::Histogram, label: "Histogram" }),
        Box::new(ImaraEngine { algorithm: imara_diff::Algorithm::Myers, post: ImaraPost::None }),
        Box::new(ImaraEngine { algorithm: imara_diff::Algorithm::Myers, post: ImaraPost::NoHeuristic }),
        Box::new(ImaraEngine { algorithm: imara_diff::Algorithm::MyersMinimal, post: ImaraPost::NoHeuristic }),
        Box::new(ImaraEngine { algorithm: imara_diff::Algorithm::Histogram, post: ImaraPost::Indent }),
        Box::new(DiffyEngine),
        Box::new(LcsDiffCrateEngine),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_engine_gives_valid_minimal_alignment_on_simple_case() {
        let a = [0u32, 1, 2, 3];
        let b = [0u32, 4, 2, 3, 5];
        let reps: Vec<&[u8]> = vec![b"a\n", b"b\n", b"c\n", b"d\n", b"e\n", b"f\n"];
        for e in all_engines() {
            let (c0, c1) = e.changes(&a, &b, &reps);
            assert!(is_valid_alignment(&a, &b, &c0, &c1), "{}", e.label());
            assert_eq!(c0.iter().filter(|c| **c).count(), 1, "{}", e.label());
            assert_eq!(c1.iter().filter(|c| **c).count(), 2, "{}", e.label());
        }
    }
}
