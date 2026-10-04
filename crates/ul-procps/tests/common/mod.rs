//! Infraestrutura dos testes com `/proc` falso.
//!
//! Cada `tests/fixtures/<nome>.fixture` descreve uma árvore `/proc` (e, opcionalmente, arquivos fora
//! dela, como `/var/run/utmp`). Os `<lista>.cmds` têm um comando por linha e os
//! `<lista>.expected.json` guardam a saída do procps real rodado sobre a mesma árvore montada em
//! `/proc` num container descartável do oráculo (`pseudo-linus-oracle:719900900623`), com pid 101 (o
//! pid que o programa ganha no testkit) e relógio virtual igual ao do testkit (CLOCK_REALTIME
//! parado em 2026-01-15T12:00:00Z mais o deslocamento do `@clock`, CLOCK_BOOTTIME em 1000 s mais o
//! mesmo deslocamento, andando só quando o programa dorme). A captura é feita pela ferramenta
//! `capture.py` descrita no STATUS.md do crate.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use sysabi::testkit::TestKit;

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

/// Uma entrada da árvore falsa.
#[derive(Clone, Debug)]
pub enum Entry {
    File { path: String, data: Vec<u8> },
    Link { path: String, target: String },
    Dir { path: String },
}

/// Árvore falsa já interpretada, com o relógio (`@clock <realtime> <uptime>`).
#[derive(Clone, Debug)]
pub struct Fixture {
    pub entries: Vec<Entry>,
    pub realtime: i64,
}

fn unescape(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            match b[i + 1] {
                b'0' => out.push(0),
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'\\' => out.push(b'\\'),
                b'x' => {
                    let h = std::str::from_utf8(&b[i + 2..i + 4]).expect("escape \\x");
                    out.push(u8::from_str_radix(h, 16).expect("escape \\x"));
                    i += 2;
                }
                other => {
                    out.push(b'\\');
                    out.push(other);
                }
            }
            i += 2;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

pub fn load_fixture(name: &str) -> Fixture {
    let path = fixtures_dir().join(format!("{name}.fixture"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut entries = Vec::new();
    let mut cur: Option<(String, bool, Vec<String>)> = None;
    let mut realtime = sysabi::testkit::DEFAULT_TIME;
    let flush = |cur: &mut Option<(String, bool, Vec<String>)>, entries: &mut Vec<Entry>| {
        if let Some((path, nonl, mut lines)) = cur.take() {
            while lines.last().is_some_and(String::is_empty) {
                lines.pop();
            }
            let mut data = Vec::new();
            for l in &lines {
                data.extend(unescape(l));
                if !nonl {
                    data.push(b'\n');
                }
            }
            entries.push(Entry::File { path, data });
        }
    };
    for line in text.split('\n') {
        if line.starts_with('@') {
            flush(&mut cur, &mut entries);
            let parts: Vec<&str> = line.split_whitespace().collect();
            match parts[0] {
                "@file" => cur = Some((parts[1].to_string(), parts[2..].contains(&"nonl"), Vec::new())),
                "@link" => entries.push(Entry::Link { path: parts[1].to_string(), target: parts[2].to_string() }),
                "@dir" => entries.push(Entry::Dir { path: parts[1].to_string() }),
                "@clock" => realtime = parts[1].parse().expect("@clock realtime"),
                "@end" => {}
                other => panic!("diretiva desconhecida {other}"),
            }
        } else if line == "#" || line.starts_with("# ") {
            continue;
        } else if let Some((_, _, lines)) = cur.as_mut() {
            lines.push(line.to_string());
        }
    }
    flush(&mut cur, &mut entries);
    Fixture { entries, realtime }
}

/// Monta o kit com os programas do crate, a árvore falsa e o ambiente.
pub fn kit(fx: &Fixture, env: &[(String, String)]) -> TestKit {
    let mut k = TestKit::new().programs(ul_procps::programs()).time(fx.realtime);
    for (key, v) in env {
        k = k.env(key, v);
    }
    for e in &fx.entries {
        match e {
            Entry::File { path, data } => k.put_file(path.as_bytes(), data, 0o444),
            Entry::Link { path, target } => k.put_symlink(path.as_bytes(), target.as_bytes()),
            Entry::Dir { path } => k.put_dir(path.as_bytes(), 0o555),
        }
    }
    k
}

/// Divide uma linha de comando: atribuições `NOME=valor` no começo viram ambiente; aspas simples
/// agrupam e barra invertida fora de aspas protege o próximo caractere.
pub fn split_cmd(line: &str) -> (Vec<(String, String)>, Vec<String>) {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                for d in chars.by_ref() {
                    if d == '\'' {
                        break;
                    }
                    cur.push(d);
                }
            }
            '"' => {
                in_word = true;
                while let Some(d) = chars.next() {
                    if d == '"' {
                        break;
                    }
                    if d == '\\' {
                        if let Some(&n) = chars.peek() {
                            if n == '"' || n == '\\' || n == '$' {
                                cur.push(n);
                                chars.next();
                                continue;
                            }
                        }
                    }
                    cur.push(d);
                }
            }
            '\\' => {
                in_word = true;
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ' ' | '\t' => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            other => {
                in_word = true;
                cur.push(other);
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    let mut env = Vec::new();
    let mut i = 0;
    while i < words.len() {
        match words[i].split_once('=') {
            Some((k, v)) if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                env.push((k.to_string(), v.to_string()));
                i += 1;
            }
            _ => break,
        }
    }
    (env, words[i..].to_vec())
}

/// Um caso capturado do programa real.
#[derive(Clone, Debug)]
pub struct Expected {
    pub cmd: String,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit: i32,
}

fn field_bytes(v: &serde_json::Value, key: &str) -> Vec<u8> {
    if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
        return s.as_bytes().to_vec();
    }
    if let Some(b) = v.get(format!("{key}_b64")).and_then(|x| x.as_str()) {
        return b64(b);
    }
    Vec::new()
}

fn b64(s: &str) -> Vec<u8> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let Some(v) = T.iter().position(|t| *t == c) else { continue };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

pub fn load_expected(name: &str) -> Vec<Expected> {
    let path = fixtures_dir().join(format!("{name}.expected.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let v: serde_json::Value = serde_json::from_str(&text).expect("json");
    v["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|c| Expected {
            cmd: c["cmd"].as_str().expect("cmd").to_string(),
            stdout: field_bytes(c, "stdout"),
            stderr: field_bytes(c, "stderr"),
            exit: c["exit"].as_i64().expect("exit") as i32,
        })
        .collect()
}

/// Resultado de comparar uma lista: casos que falharam com o detalhe.
pub struct Outcome {
    pub total: usize,
    pub failures: Vec<String>,
}

fn show(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Roda cada comando de `<lista>.expected.json` sobre a fixture e compara stdout, stderr e status.
/// `filter` escolhe os comandos (pelo texto); `known` lista divergências documentadas, que são
/// puladas.
pub fn check(fixture: &str, list: &str, filter: impl Fn(&str) -> bool, known: &[(&str, &str)]) -> Outcome {
    let fx = load_fixture(fixture);
    let mut failures = Vec::new();
    let mut total = 0;
    for exp in load_expected(list) {
        if !filter(&exp.cmd) || known.iter().any(|(c, _)| *c == exp.cmd) {
            continue;
        }
        total += 1;
        let (env, argv) = split_cmd(&exp.cmd);
        let k = kit(&fx, &env);
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        let r = k.run(&argv, b"");
        let mut problems = Vec::new();
        if r.stdout != exp.stdout {
            problems.push(format!("stdout\n--- esperado\n{}--- obtido\n{}", show(&exp.stdout), show(&r.stdout)));
        }
        if r.stderr != exp.stderr {
            problems.push(format!("stderr\n--- esperado\n{}--- obtido\n{}", show(&exp.stderr), show(&r.stderr)));
        }
        if r.code() != exp.exit {
            problems.push(format!("status esperado {} obtido {}", exp.exit, r.code()));
        }
        if !problems.is_empty() {
            failures.push(format!("==== {}\n{}", exp.cmd, problems.join("\n")));
        }
    }
    Outcome { total, failures }
}

impl Outcome {
    /// Imprime as falhas (rode com `--nocapture`) e falha o teste se houver alguma.
    pub fn assert_ok(&self, what: &str) {
        for f in &self.failures {
            eprintln!("{f}");
        }
        eprintln!("{what}: {}/{} casos iguais ao procps real", self.total - self.failures.len(), self.total);
        assert!(self.failures.is_empty(), "{what}: {} de {} casos divergem", self.failures.len(), self.total);
    }
}
