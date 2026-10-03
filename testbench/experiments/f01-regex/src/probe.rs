//! Sondas: comandos GNU reais que o oráculo roda, e a emulação de cada um sobre um motor candidato.
//!
//! | sonda | comando no oráculo | o que mede |
//! |---|---|---|
//! | `grep-n` | `grep -n -G/-E [-i] -e RE s.txt` | casa ou não, linha a linha |
//! | `grep-o` | `grep -ob -G/-E [-i] -e RE s.txt` | spans de todas as casadas não vazias (leftmost-longest) |
//! | `sed-n` | `sed -n [-E] '\cREcp' s.txt` | casa ou não, no dialeto do sed |
//! | `sed-g` | `sed -n [-E] 's/RE/\x02&\x03/gp' s.txt` | spans pela iteração do `s///g` |
//! | `sed-sub` | `sed -n [-E] 's/RE/\x02&\x03\1\x03...\x04/p' s.txt` | span e conteúdo dos grupos da 1a casada |
//! | `gawk` | `gawk -f m.awk s.txt` com `match($0, re, a)` | posição e participação dos grupos (ERE) |
//!
//! O delimitador dos comandos do sed é `\x01`, que não aparece em padrão nenhum.

use std::collections::BTreeMap;

use harness::{Case, FileSpec, Invocation, MemTree, Outcome};
use serde::{Deserialize, Serialize};

use crate::ast::{self, Regex};
use crate::engines::{CompileError, Engine, Matcher, next_boundary};
use crate::parse::{Dialect, parse, parse_dfa_view};

pub const SUBJECT_FILE: &str = "s.txt";
pub const REGEX_FILE: &str = "re.txt";
pub const AWK_FILE: &str = "m.awk";
const DELIM: char = '\u{1}';
const OPEN: char = '\u{2}';
const SEP: char = '\u{3}';
const CLOSE: char = '\u{4}';

pub const AWK_PROGRAM: &str = r#"BEGIN { getline re < "re.txt" }
{
  if (match($0, re, a)) {
    out = NR " " RSTART " " RLENGTH
    for (k = 1; k <= N; k++) {
      if ((k, "start") in a) out = out " " a[k, "start"] ":" a[k, "length"]
      else out = out " -"
    }
    print out
  } else print NR " 0"
}
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeKind {
    GrepN,
    GrepO,
    SedN,
    SedG,
    SedSub,
    Gawk,
}

impl ProbeKind {
    pub fn label(self) -> &'static str {
        match self {
            ProbeKind::GrepN => "grep-n",
            ProbeKind::GrepO => "grep-o",
            ProbeKind::SedN => "sed-n",
            ProbeKind::SedG => "sed-g",
            ProbeKind::SedSub => "sed-sub",
            ProbeKind::Gawk => "gawk",
        }
    }

    /// Qual aspecto da concordância a sonda mede.
    pub fn aspect(self) -> &'static str {
        match self {
            ProbeKind::GrepN | ProbeKind::SedN => "match",
            ProbeKind::GrepO | ProbeKind::SedG => "span",
            ProbeKind::SedSub | ProbeKind::Gawk => "submatch",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    pub kind: ProbeKind,
    /// Dialeto em que o padrão é interpretado (grep-* nas sondas do grep e do gawk, sed-* nas do sed).
    pub dialect: Dialect,
    pub icase: bool,
    pub pattern: String,
    /// Grupos referenciados (sed-sub e gawk).
    pub groups: usize,
    pub subjects: Vec<String>,
}

impl Probe {
    pub fn argv(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        match self.kind {
            ProbeKind::GrepN | ProbeKind::GrepO => {
                v.push("grep".into());
                v.push(if self.kind == ProbeKind::GrepN { "-n" } else { "-ob" }.into());
                v.push(if self.dialect.ere() { "-E" } else { "-G" }.into());
                if self.icase {
                    v.push("-i".into());
                }
                v.extend(["-e".to_string(), self.pattern.clone(), SUBJECT_FILE.to_string()]);
            }
            ProbeKind::SedN | ProbeKind::SedG | ProbeKind::SedSub => {
                v.extend(["sed".to_string(), "-n".to_string()]);
                if self.dialect.ere() {
                    v.push("-E".into());
                }
                let flag_i = if self.icase { "I" } else { "" };
                let script = match self.kind {
                    ProbeKind::SedN => format!("\\{DELIM}{}{DELIM}{flag_i}p", self.pattern),
                    ProbeKind::SedG => format!("s{DELIM}{}{DELIM}{OPEN}&{SEP}{DELIM}g{flag_i}p", self.pattern),
                    _ => {
                        let mut repl = format!("{OPEN}&");
                        for k in 1..=self.groups.min(9) {
                            repl.push(SEP);
                            repl.push_str(&format!("\\{k}"));
                        }
                        repl.push(CLOSE);
                        format!("s{DELIM}{}{DELIM}{repl}{DELIM}{flag_i}p", self.pattern)
                    }
                };
                v.push(script);
                v.push(SUBJECT_FILE.into());
            }
            ProbeKind::Gawk => {
                v.extend(["gawk".to_string(), "-v".to_string(), format!("N={}", self.groups)]);
                if self.icase {
                    v.extend(["-v".to_string(), "IGNORECASE=1".to_string()]);
                }
                v.extend(["-f".to_string(), AWK_FILE.to_string(), SUBJECT_FILE.to_string()]);
            }
        }
        v
    }

    pub fn subject_text(&self) -> String {
        let mut s = self.subjects.join("\n");
        s.push('\n');
        s
    }

    pub fn files(&self) -> BTreeMap<String, FileSpec> {
        let mut files = BTreeMap::new();
        files.insert(SUBJECT_FILE.to_string(), FileSpec::Text(self.subject_text()));
        if self.kind == ProbeKind::Gawk {
            files.insert(REGEX_FILE.to_string(), FileSpec::Text(format!("{}\n", self.pattern)));
            files.insert(AWK_FILE.to_string(), FileSpec::Text(AWK_PROGRAM.to_string()));
        }
        files
    }

    pub fn to_case(&self, id: String, tags: Vec<String>) -> Case {
        Case {
            id,
            argv: self.argv(),
            script: None,
            stdin: None,
            stdin_b64: None,
            files: self.files(),
            env: BTreeMap::new(),
            tags,
            faketime: None,
            timeout_ms: Some(10_000),
        }
    }

    /// Reconstrói a sonda a partir do argv e dos arquivos de um caso.
    pub fn from_invocation(inv: &Invocation) -> Result<Probe, String> {
        let text = |name: &str| -> Result<String, String> {
            let data = inv.files.read(name).ok_or_else(|| format!("sem {name}"))?;
            String::from_utf8(data.to_vec()).map_err(|_| format!("{name} não é UTF-8"))
        };
        let subjects = |raw: String| -> Vec<String> {
            let mut lines: Vec<String> = raw.split('\n').map(str::to_string).collect();
            if lines.last().is_some_and(String::is_empty) {
                lines.pop();
            }
            lines
        };
        let a = &inv.argv;
        match a.first().map(String::as_str) {
            Some("grep") => {
                let kind = match a.get(1).map(String::as_str) {
                    Some("-n") => ProbeKind::GrepN,
                    Some("-ob") => ProbeKind::GrepO,
                    other => return Err(format!("grep com {other:?}")),
                };
                let dialect = if a.get(2).map(String::as_str) == Some("-E") { Dialect::GrepEre } else { Dialect::GrepBre };
                let icase = a.get(3).map(String::as_str) == Some("-i");
                let pi = if icase { 5 } else { 4 };
                let pattern = a.get(pi).cloned().ok_or("sem padrão")?;
                Ok(Probe { kind, dialect, icase, pattern, groups: 0, subjects: subjects(text(SUBJECT_FILE)?) })
            }
            Some("sed") => {
                let ere = a.get(2).map(String::as_str) == Some("-E");
                let dialect = if ere { Dialect::SedEre } else { Dialect::SedBre };
                let script = a.get(if ere { 3 } else { 2 }).ok_or("sem script")?;
                let parts: Vec<&str> = script.split(DELIM).collect();
                let (kind, pattern, flags, groups) = if script.starts_with('\\') {
                    (ProbeKind::SedN, parts[1].to_string(), parts[2], 0)
                } else if parts[2] == format!("{OPEN}&{SEP}") {
                    (ProbeKind::SedG, parts[1].to_string(), parts[3], 0)
                } else {
                    let groups = parts[2].matches(SEP).count();
                    (ProbeKind::SedSub, parts[1].to_string(), parts[3], groups)
                };
                let icase = flags.contains('I');
                Ok(Probe { kind, dialect, icase, pattern, groups, subjects: subjects(text(SUBJECT_FILE)?) })
            }
            Some("gawk") => {
                let groups: usize = a
                    .get(2)
                    .and_then(|s| s.strip_prefix("N="))
                    .and_then(|n| n.parse().ok())
                    .ok_or("sem N")?;
                let icase = a.iter().any(|s| s == "IGNORECASE=1");
                let mut pattern = text(REGEX_FILE)?;
                if pattern.ends_with('\n') {
                    pattern.pop();
                }
                Ok(Probe {
                    kind: ProbeKind::Gawk,
                    dialect: Dialect::GrepEre,
                    icase,
                    pattern,
                    groups,
                    subjects: subjects(text(SUBJECT_FILE)?),
                })
            }
            other => Err(format!("sonda desconhecida: {other:?}")),
        }
    }
}

/// O `gawk` interpreta a regex no dialeto dele (`RE_SYNTAX_GNU_AWK`); a sonda só é gerada quando
/// o padrão significa a mesma coisa lá e no `grep -E`.
pub fn gawk_safe(pattern: &str, re: &Regex, dialect: Dialect) -> bool {
    if !dialect.ere() || ast::uses_backref(re) || re.groups == 0 {
        return false;
    }
    if pattern.contains('[') && pattern.contains('\\') {
        return false;
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            match chars.get(i + 1) {
                Some(n) if ".[]()*+?{}|^$\\/wWsS<>B`'".contains(*n) => i += 2,
                _ => return false,
            }
            continue;
        }
        if "*+?{".contains(c) {
            let prev = if i == 0 { None } else { Some(chars[i - 1]) };
            if matches!(prev, None | Some('(') | Some('|') | Some('^')) {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// Resultado de rodar uma sonda num motor.
pub fn emulate(probe: &Probe, engine: &dyn Engine, files: MemTree) -> Outcome {
    let re = match parse(&probe.pattern, probe.dialect) {
        Ok(re) => re,
        Err(e) => return error_outcome(probe, &e.to_string(), files),
    };
    let spans_only = !matches!(probe.kind, ProbeKind::SedSub | ProbeKind::Gawk);
    let compiled = if spans_only {
        engine.compile_spans(&re, &probe.pattern, probe.dialect, probe.icase)
    } else {
        engine.compile(&re, &probe.pattern, probe.dialect, probe.icase)
    };
    let matcher = match compiled {
        Ok(m) => m,
        Err(CompileError::Unsupported(why)) => return Outcome::unsupported(why),
        Err(CompileError::Engine(msg)) => return error_outcome(probe, &format!("motor recusou: {msg}"), files),
    };
    // No grep, quem escolhe as linhas é o dfa.c (quando não há backref); ver `parse_dfa_view`.
    let mut selector: Option<Box<dyn Matcher>> = None;
    if matches!(probe.kind, ProbeKind::GrepN | ProbeKind::GrepO)
        && !ast::uses_backref(&re)
        && let Ok(view) = parse_dfa_view(&probe.pattern, probe.dialect)
        && view != re
    {
        selector = match engine.compile_spans(&view, &probe.pattern, probe.dialect, probe.icase) {
            Ok(m) => Some(m),
            Err(CompileError::Unsupported(why)) => return Outcome::unsupported(why),
            Err(CompileError::Engine(msg)) => {
                return error_outcome(probe, &format!("motor recusou: {msg}"), files);
            }
        };
    }
    let sel: &dyn Matcher = selector.as_deref().unwrap_or(matcher.as_ref());
    match run_probe(probe, matcher.as_ref(), sel) {
        Ok((stdout, exit)) => Outcome::exited(stdout, Vec::new(), exit, files),
        Err(e) => Outcome::unsupported(format!("erro em tempo de busca: {e}")),
    }
}

fn error_outcome(probe: &Probe, msg: &str, files: MemTree) -> Outcome {
    match probe.kind {
        ProbeKind::GrepN | ProbeKind::GrepO => Outcome::exited("", format!("grep: {msg}\n"), 2, files),
        ProbeKind::Gawk => Outcome::exited("", format!("gawk: fatal: {msg}\n"), 2, files),
        _ => Outcome::exited("", format!("sed: -e expression #1, char 0: {msg}\n"), 1, files),
    }
}

/// Primeira casada da linha (com grupos).
pub fn first_match(m: &dyn Matcher, line: &[u8]) -> Result<Option<crate::engines::Caps>, String> {
    m.captures_at(line, 0)
}

/// Casadas não vazias na ordem do `grep -o` (busca recomeça no fim da casada; casada vazia avança
/// um caractere; nada é aceito começando no fim da linha).
pub fn grep_o_spans(m: &dyn Matcher, line: &[u8]) -> Result<Vec<(usize, usize)>, String> {
    if !m.supports_start() {
        return Ok(m.native_all(line)?.into_iter().filter(|(s, e)| e > s).collect());
    }
    let mut out = Vec::new();
    let mut cur = 0;
    while cur < line.len() {
        let Some(c) = m.captures_at(line, cur)? else { break };
        let Some((s, e)) = c[0] else { break };
        if s >= line.len() {
            break;
        }
        if e == s {
            cur = next_boundary(line, s);
        } else {
            out.push((s, e));
            cur = e;
        }
    }
    Ok(out)
}

/// Casadas na ordem do `s///g` do GNU sed: casada vazia colada no fim da anterior é pulada.
pub fn sed_g_spans(m: &dyn Matcher, line: &[u8]) -> Result<Vec<(usize, usize)>, String> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    if !m.supports_start() {
        for (s, e) in m.native_all(line)? {
            if s == e && out.last().is_some_and(|&(_, pe)| pe == s && pe != 0) {
                continue;
            }
            if out.last().is_some_and(|&(_, pe)| s < pe) {
                continue;
            }
            out.push((s, e));
        }
        return Ok(out);
    }
    let mut start = 0;
    let mut prev_end: Option<usize> = None;
    while let Some(c) = m.captures_at(line, start)? {
        let Some((s, e)) = c[0] else { break };
        if s == e && prev_end == Some(s) {
            if s >= line.len() {
                break;
            }
            start = next_boundary(line, s);
            continue;
        }
        out.push((s, e));
        prev_end = Some(e);
        if s == e {
            if s >= line.len() {
                break;
            }
            start = next_boundary(line, s);
        } else {
            start = e;
        }
        if start > line.len() {
            break;
        }
    }
    Ok(out)
}

fn run_probe(probe: &Probe, m: &dyn Matcher, sel: &dyn Matcher) -> Result<(Vec<u8>, i32), String> {
    let mut out = Vec::new();
    let mut any = false;
    let mut offset = 0usize;
    for (i, line) in probe.subjects.iter().enumerate() {
        let b = line.as_bytes();
        match probe.kind {
            ProbeKind::GrepN | ProbeKind::SedN => {
                if first_match(sel, b)?.is_some() {
                    any = true;
                    if probe.kind == ProbeKind::GrepN {
                        out.extend_from_slice(format!("{}:", i + 1).as_bytes());
                    }
                    out.extend_from_slice(b);
                    out.push(b'\n');
                }
            }
            ProbeKind::GrepO => {
                if first_match(sel, b)?.is_some() {
                    any = true;
                    for (s, e) in grep_o_spans(m, b)? {
                        out.extend_from_slice(format!("{}:", offset + s).as_bytes());
                        out.extend_from_slice(&b[s..e]);
                        out.push(b'\n');
                    }
                }
            }
            ProbeKind::SedG => {
                let spans = sed_g_spans(m, b)?;
                if !spans.is_empty() {
                    let mut last = 0;
                    for (s, e) in spans {
                        out.extend_from_slice(&b[last..s]);
                        out.extend_from_slice(OPEN.to_string().as_bytes());
                        out.extend_from_slice(&b[s..e]);
                        out.extend_from_slice(SEP.to_string().as_bytes());
                        last = e;
                    }
                    out.extend_from_slice(&b[last..]);
                    out.push(b'\n');
                }
            }
            ProbeKind::SedSub => {
                if let Some(c) = first_match(m, b)? {
                    let (s, e) = c[0].ok_or("casada sem span")?;
                    out.extend_from_slice(&b[..s]);
                    out.extend_from_slice(OPEN.to_string().as_bytes());
                    out.extend_from_slice(&b[s..e]);
                    for k in 1..=probe.groups.min(9) {
                        out.extend_from_slice(SEP.to_string().as_bytes());
                        if let Some(Some((gs, ge))) = c.get(k) {
                            out.extend_from_slice(&b[*gs..*ge]);
                        }
                    }
                    out.extend_from_slice(CLOSE.to_string().as_bytes());
                    out.extend_from_slice(&b[e..]);
                    out.push(b'\n');
                }
            }
            ProbeKind::Gawk => {
                let chars = |upto: usize| line[..upto].chars().count();
                match first_match(m, b)? {
                    Some(c) => {
                        let (s, e) = c[0].ok_or("casada sem span")?;
                        let mut row = format!("{} {} {}", i + 1, chars(s) + 1, line[s..e].chars().count());
                        for k in 1..=probe.groups {
                            match c.get(k).copied().flatten() {
                                Some((gs, ge)) => row.push_str(&format!(" {}:{}", chars(gs) + 1, line[gs..ge].chars().count())),
                                None => row.push_str(" -"),
                            }
                        }
                        row.push('\n');
                        out.extend_from_slice(row.as_bytes());
                    }
                    None => out.extend_from_slice(format!("{} 0\n", i + 1).as_bytes()),
                }
            }
        }
        offset += b.len() + 1;
    }
    let exit = match probe.kind {
        ProbeKind::GrepN | ProbeKind::GrepO => {
            if any {
                0
            } else {
                1
            }
        }
        _ => 0,
    };
    Ok((out, exit))
}

/// Um motor visto como candidato da bancada: lê a sonda do caso e emula.
pub struct EngineCandidate<'a> {
    pub engine: &'a dyn Engine,
}

impl harness::Candidate for EngineCandidate<'_> {
    fn name(&self) -> String {
        format!("{} {}", self.engine.name(), self.engine.version())
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        match Probe::from_invocation(inv) {
            Ok(p) => emulate(&p, self.engine, inv.files.clone()),
            Err(e) => Outcome::unsupported(format!("caso não é sonda: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::all_engines;

    fn probe(kind: ProbeKind, dialect: Dialect, pattern: &str, groups: usize, subjects: &[&str]) -> Probe {
        Probe {
            kind,
            dialect,
            icase: false,
            pattern: pattern.into(),
            groups,
            subjects: subjects.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn run(p: &Probe, engine: &str) -> String {
        let e = crate::engines::engine_by_name(engine).unwrap();
        let out = emulate(p, e.as_ref(), MemTree::new());
        assert!(out.unsupported.is_none(), "{engine}: {:?}", out.unsupported);
        String::from_utf8(out.stdout.0).unwrap()
    }

    #[test]
    fn argv_roundtrip() {
        for kind in [ProbeKind::GrepN, ProbeKind::GrepO, ProbeKind::SedN, ProbeKind::SedG, ProbeKind::SedSub, ProbeKind::Gawk] {
            let dialect = match kind {
                ProbeKind::GrepN | ProbeKind::GrepO | ProbeKind::Gawk => Dialect::GrepEre,
                _ => Dialect::SedEre,
            };
            let groups = if matches!(kind, ProbeKind::SedSub | ProbeKind::Gawk) { 2 } else { 0 };
            let mut p = probe(kind, dialect, "(a)(b)", groups, &["ab", "x"]);
            p.icase = true;
            let case = p.to_case("t".into(), vec![]);
            let back = Probe::from_invocation(&case.invocation().unwrap()).unwrap();
            assert_eq!(back, p, "{kind:?}");
        }
    }

    #[test]
    fn leftmost_longest_vs_leftmost_first() {
        let p = probe(ProbeKind::GrepO, Dialect::GrepEre, "a|ab", 0, &["xab"]);
        assert_eq!(run(&p, "regex"), "1:a\n");
        assert_eq!(run(&p, "revera"), "1:ab\n");
        assert_eq!(run(&p, "regex-automata-longest"), "1:ab\n");
        assert_eq!(run(&p, "ferroni-longest"), "1:ab\n");
        assert_eq!(run(&p, "rusty_expressions-longest"), "1:ab\n");
    }

    #[test]
    fn sed_g_skips_empty_after_match() {
        let p = probe(ProbeKind::SedG, Dialect::SedBre, "a*", 0, &["baaac"]);
        let expected = "\u{2}\u{3}b\u{2}aaa\u{3}c\u{2}\u{3}\n";
        assert_eq!(run(&p, "regex"), expected);
        assert_eq!(run(&p, "revera"), expected);
    }

    #[test]
    fn every_engine_handles_a_literal() {
        let p = probe(ProbeKind::GrepO, Dialect::GrepBre, "foo", 0, &["a foo b foo", "bar"]);
        for e in all_engines() {
            assert_eq!(run(&p, e.name()), "2:foo\n8:foo\n", "{}", e.name());
        }
    }

    #[test]
    fn gawk_safety() {
        let ok = |p: &str| gawk_safe(p, &parse(p, Dialect::GrepEre).unwrap(), Dialect::GrepEre);
        assert!(ok("(a+)(b)"));
        assert!(!ok("(a)\\1"));
        assert!(!ok("(*a)"));
        assert!(!ok("([\\]])"));
        assert!(!ok("(a)\\b"));
    }
}
