//! Comparação contra o golden. Casos com a tag `unordered` (saída na ordem do readdir, que não é
//! propriedade semântica) são comparados como multiconjunto de registros: linhas, ou registros
//! terminados em NUL quando o caso tem a tag `print0` ou `null`. O resto é byte a byte, igual ao
//! `harness::compare_outcome`.

use harness::{Bytes, Case, CaseComparison, Conformance, Outcome, compare_outcome};

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

/// Placar no mesmo formato do `harness::score`.
pub fn summarize(candidate: &str, comparisons: &[CaseComparison]) -> Conformance {
    let mut conf = Conformance { candidate: candidate.to_string(), ..Conformance::default() };
    for cmp in comparisons {
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
    conf.sample_failures = comparisons.iter().filter(|c| !c.strict).take(15).cloned().collect();
    conf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(tags: &[&str]) -> Case {
        Case {
            id: "x".into(),
            argv: vec!["find".into()],
            script: None,
            stdin: None,
            stdin_b64: None,
            files: Default::default(),
            env: Default::default(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            faketime: None,
            timeout_ms: None,
        }
    }

    #[test]
    fn unordered_compares_as_multiset() {
        let a = Outcome::exited("./b\n./a\n", "", 0, Default::default());
        let b = Outcome::exited("./a\n./b\n", "", 0, Default::default());
        assert!(compare(&case(&["unordered"]), &a, &b).strict);
        assert!(!compare(&case(&[]), &a, &b).strict);
        let a0 = Outcome::exited("./b\0./a\0", "", 0, Default::default());
        let b0 = Outcome::exited("./a\0./b\0", "", 0, Default::default());
        assert!(compare(&case(&["unordered", "print0"]), &a0, &b0).strict);
    }
}
