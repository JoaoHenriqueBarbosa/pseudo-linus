//! Compara o zicu com o ICU4C: os goldens saem de `tools/brkref.c` sobre as mesmas tabelas, com o
//! pacote de dados do ICU do Flutter (o mesmo `icudtl.dat` de onde as tabelas foram tiradas).

use zicu::{DONE, LanguageBreakEngine, RbbiData, RuleBasedBreakIterator};

/// Decodifica uma linha do corpus como o `brkref`: `\uXXXX`, `\n` e `\\`.
fn decode(line: &str) -> Vec<u16> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('u') => {
                    chars.next();
                    let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                    out.push(u16::from_str_radix(&hex, 16).unwrap());
                    continue;
                }
                Some('n') => {
                    chars.next();
                    out.push(u16::from(b'\n'));
                    continue;
                }
                Some('\\') => {
                    chars.next();
                    out.push(u16::from(b'\\'));
                    continue;
                }
                _ => {}
            }
        }
        let mut buf = [0u16; 2];
        out.extend_from_slice(c.encode_utf16(&mut buf));
    }
    out
}

/// As quatro linhas que o `brkref` imprime para um texto.
pub fn run(bi: &mut RuleBasedBreakIterator<'_>, text: &[u16]) -> Vec<String> {
    bi.set_text(text);
    let n = text.len() as i32;
    let mut forward = String::from("N");
    let mut p = bi.first();
    while p != DONE {
        forward.push_str(&format!(" {p}:{}", bi.rule_status()));
        p = bi.next();
    }
    let all = |f: &mut dyn FnMut(i32) -> String| -> String {
        (-1..=n + 1).map(|i| format!(" {}", f(i))).collect()
    };
    let following = format!("F{}", all(&mut |i| bi.following(i).to_string()));
    let preceding = format!("P{}", all(&mut |i| bi.preceding(i).to_string()));
    let boundary = format!(
        "I{}",
        all(&mut |i| i32::from(bi.is_boundary(i)).to_string())
    );
    vec![forward, following, preceding, boundary]
}

pub fn check(brk: &str, corpus: &str, golden: &str, engines: Vec<&dyn LanguageBreakEngine>) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/");
    let data = RbbiData::from_bytes(&std::fs::read(format!("{dir}{brk}")).unwrap()).unwrap();
    let texts = std::fs::read_to_string(format!("{dir}{corpus}")).unwrap();
    let golden = std::fs::read_to_string(format!("{dir}{golden}")).unwrap();
    let mut expected = golden.lines();
    let mut bi = RuleBasedBreakIterator::new(&data, engines);
    let mut failures = Vec::new();
    for (number, line) in texts.lines().enumerate() {
        for got in run(&mut bi, &decode(line)) {
            let want = expected.next().unwrap();
            if got != want {
                failures.push(format!(
                    "{golden}, texto {}: {line}\n  icu:   {want}\n  zicu:  {got}",
                    number + 1
                ));
                break;
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} textos diferem:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn caractere() {
    check("char.brk", "texts.txt", "char.golden", Vec::new());
}

#[test]
fn palavra() {
    check("word.brk", "texts.txt", "word.golden", Vec::new());
}

#[test]
fn palavra_japones() {
    check("word_ja.brk", "texts.txt", "word_ja.golden", Vec::new());
}

#[test]
fn linha() {
    check(
        "line_normal.brk",
        "texts.txt",
        "line_normal.golden",
        Vec::new(),
    );
}

#[test]
fn linha_cj() {
    check(
        "line_normal_cj.brk",
        "texts.txt",
        "line_normal_cj.golden",
        Vec::new(),
    );
}

fn engines() -> Vec<zicu::DictionaryBreakEngine> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/");
    let dict = |name: &str| {
        zicu::DictionaryMatcher::from_bytes(&std::fs::read(format!("{dir}{name}.dict")).unwrap())
            .unwrap()
    };
    vec![
        zicu::DictionaryBreakEngine::thai(dict("thaidict")),
        zicu::DictionaryBreakEngine::lao(dict("laodict")),
        zicu::DictionaryBreakEngine::burmese(dict("burmesedict")),
        zicu::DictionaryBreakEngine::khmer(dict("khmerdict")),
    ]
}

fn check_sa(brk: &str, golden: &str) {
    let owned = engines();
    let refs: Vec<&dyn LanguageBreakEngine> = owned
        .iter()
        .map(|e| e as &dyn LanguageBreakEngine)
        .collect();
    check(brk, "texts_sa.txt", golden, refs);
}

#[test]
fn dicionario_caractere() {
    check_sa("char.brk", "char_sa.golden");
}

#[test]
fn dicionario_palavra() {
    check_sa("word.brk", "word_sa.golden");
}

#[test]
fn dicionario_linha() {
    check_sa("line_normal.brk", "line_normal_sa.golden");
}

#[test]
fn dicionario_linha_cj() {
    check_sa("line_normal_cj.brk", "line_normal_cj_sa.golden");
}
