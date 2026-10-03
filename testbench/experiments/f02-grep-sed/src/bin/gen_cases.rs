//! Importa testes upstream pra casos do harness (derivados mecânicos; a fonte é a suíte upstream):
//!
//! - GNU grep 3.11 `tests/foad1` (`grep_test ENTRADA SAÍDA ARGS...`) e `tests/yesno` (combinações de
//!   `-m`, `-v`, `-o`, `-C` sobre um arquivo fixo) em `corpus/cases/grep/upstream-*.toml`;
//! - GNU sed 4.9 `testsuite/misc.pl` (a lista `@Tests`, lida com `perl` + `JSON::PP`) e os testes de
//!   script `.sed/.inp` em `corpus/cases/sed/upstream-*.toml`.
//!
//! Os resultados esperados das suítes não são usados: o golden sai do oráculo.
//!
//! `cargo run --release --bin f02-gen-cases` (precisa de `corpus/upstream/{grep,sed}`).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use harness::{Case, CaseFile, FileSpec};
use serde_json::Value;

const HEADER: &str = "# Gerado por testbench/experiments/f02-grep-sed (binário f02-gen-cases) a partir das suítes\n\
# upstream em corpus/upstream. Não edite à mão.\n\n";

fn case(id: String, argv: Vec<String>, stdin: Option<Vec<u8>>, files: BTreeMap<String, FileSpec>, tags: Vec<String>) -> Case {
    let (stdin, stdin_b64) = match stdin {
        None => (None, None),
        Some(b) => match String::from_utf8(b) {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(base64::engine::general_purpose::STANDARD.encode(e.into_bytes()))),
        },
    };
    Case {
        id,
        argv,
        script: None,
        stdin,
        stdin_b64,
        files,
        env: BTreeMap::new(),
        tags,
        faketime: None,
        timeout_ms: Some(10_000),
    }
}

fn file_spec(bytes: &[u8]) -> FileSpec {
    match std::str::from_utf8(bytes) {
        Ok(s) => FileSpec::Text(s.to_string()),
        Err(_) => FileSpec::Table(harness::case::FileTable {
            content_b64: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            ..Default::default()
        }),
    }
}

fn write(tool: &str, name: &str, source: &str, cases: Vec<Case>) -> Result<()> {
    let n = cases.len();
    let file = CaseFile { tool: Some(tool.into()), source: Some(source.into()), cases };
    let dir = harness::paths::cases_dir(tool);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.toml"));
    std::fs::write(&path, format!("{HEADER}{}", toml::to_string(&file)?))?;
    CaseFile::load(&path)?;
    println!("{}: {n} casos", path.display());
    Ok(())
}

/// `tests/foad1`: só as linhas com argumentos literais (sem variável de shell nem `--color=always`);
/// o laço `for mode in F G E` é expandido.
fn grep_foad1(dir: &Path) -> Result<Vec<Case>> {
    let text = std::fs::read_to_string(dir.join("foad1"))?;
    let mut joined: Vec<String> = Vec::new();
    let mut acc = String::new();
    for line in text.lines() {
        if let Some(stripped) = line.strip_suffix('\\') {
            acc.push_str(stripped);
            continue;
        }
        acc.push_str(line);
        joined.push(std::mem::take(&mut acc));
    }
    let mut out = Vec::new();
    for (n, line) in joined.iter().enumerate() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("grep_test ") || line.contains("--color=always") {
            continue;
        }
        let in_loop = line.starts_with("  ");
        let modes: Vec<&str> = if in_loop { vec!["F", "G", "E"] } else { vec![""] };
        for mode in modes {
            let line = trimmed.replace("$mode", mode).replace(" 2>/dev/null", "");
            // Variáveis de shell fora de aspas simples: fica de fora.
            let outside_single: String = line.split('\'').step_by(2).collect();
            if outside_single.contains('$') {
                continue;
            }
            let words = f01_regex::shell::simple_commands(&line);
            let Some(words) = words.first() else { continue };
            if words.len() < 4 {
                continue;
            }
            let input = words[1].replace('/', "\n");
            let mut argv = vec!["grep".to_string()];
            argv.extend(words[3..].iter().cloned());
            let id = if mode.is_empty() { format!("foad1-{:03}", n + 1) } else { format!("foad1-{:03}-{}", n + 1, mode.to_lowercase()) };
            out.push(case(id, argv, Some(input.into_bytes()), BTreeMap::new(), vec!["src:upstream-foad1".into()]));
        }
    }
    Ok(out)
}

/// `tests/yesno`: `grep -F -n -b OPÇÕES yes` sobre o arquivo do teste.
fn grep_yesno(dir: &Path) -> Result<Vec<Case>> {
    let text = std::fs::read_to_string(dir.join("yesno"))?;
    let start = text.find("<< 'EOF'").or_else(|| text.find("<<EOF")).or_else(|| text.find("<< EOF")).context("heredoc do yesno")?;
    let body_start = text[start..].find('\n').context("fim da linha")? + start + 1;
    let body_end = text[body_start..].find("\nEOF").context("fim do heredoc")? + body_start + 1;
    let input = &text[body_start..body_end];
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let t = line.trim_start_matches('#').trim_start();
        let commented = line.trim_start().starts_with('#');
        if !t.starts_with('\'') || !t.contains('"') {
            continue;
        }
        let Some(opts) = t[1..].split('\'').next() else { continue };
        let mut argv: Vec<String> = vec!["grep".into(), "-F".into(), "-n".into(), "-b".into()];
        argv.extend(opts.split(',').filter(|s| !s.is_empty()).map(str::to_string));
        argv.push("yes".into());
        let mut tags = vec!["src:upstream-yesno".to_string()];
        if commented {
            tags.push("upstream-todo".into());
        }
        out.push(case(format!("yesno-{:03}", n + 1), argv, Some(input.as_bytes().to_vec()), BTreeMap::new(), tags));
    }
    if out.is_empty() {
        bail!("nenhum caso no yesno");
    }
    Ok(out)
}

/// `testsuite/misc.pl`: a lista `@Tests` vira JSON pelo próprio Perl.
fn sed_misc(dir: &Path) -> Result<Vec<Case>> {
    let dump = r#"
use strict; use JSON::PP;
local $/; open(my $f, '<', $ARGV[0]) or die; my $src = <$f>;
my ($block) = $src =~ /(my \@Tests\s*=\s*\(.*?\n\s*\);)/s or die "sem \@Tests";
my $prog = 'sed';
my @Tests; eval "\@Tests = " . substr($block, index($block, '(')); die $@ if $@;
print JSON::PP->new->canonical->encode(\@Tests);
"#;
    let out = Command::new("perl").arg("-e").arg(dump).arg(dir.join("misc.pl")).output().context("perl")?;
    if !out.status.success() {
        bail!("perl: {}", String::from_utf8_lossy(&out.stderr));
    }
    let tests: Vec<Vec<Value>> = serde_json::from_slice(&out.stdout)?;
    let mut cases = Vec::new();
    'tests: for t in tests {
        let Some(name) = t.first().and_then(Value::as_str) else { continue };
        let mut shell_args: Vec<String> = Vec::new();
        let mut files: BTreeMap<String, FileSpec> = BTreeMap::new();
        let mut stdin: Option<Vec<u8>> = None;
        let mut env: BTreeMap<String, String> = BTreeMap::new();
        let mut file_args: Vec<String> = Vec::new();
        let mut in_count = 0;
        for item in &t[1..] {
            match item {
                Value::String(s) => shell_args.push(s.clone()),
                Value::Object(map) => {
                    for (k, v) in map {
                        match (k.as_str(), v) {
                            ("IN", Value::String(s)) => {
                                in_count += 1;
                                let fname = format!("in{in_count}");
                                files.insert(fname.clone(), file_spec(s.as_bytes()));
                                file_args.push(fname);
                            }
                            ("IN", Value::Object(m)) => {
                                for (fname, content) in m {
                                    let c = content.as_str().unwrap_or_default();
                                    files.insert(fname.clone(), file_spec(c.as_bytes()));
                                    file_args.push(fname.clone());
                                }
                            }
                            ("AUX", Value::Object(m)) => {
                                for (fname, content) in m {
                                    files.insert(fname.clone(), file_spec(content.as_str().unwrap_or_default().as_bytes()));
                                }
                            }
                            ("IN_PIPE", Value::String(s)) => stdin = Some(s.as_bytes().to_vec()),
                            ("ENV", Value::String(s)) => {
                                for kv in s.split_whitespace() {
                                    if let Some((k, v)) = kv.split_once('=') {
                                        env.insert(k.into(), v.into());
                                    }
                                }
                            }
                            // Saídas esperadas (inclusive arquivos, CMP) vêm do oráculo, não da suíte.
                            ("OUT" | "ERR" | "EXIT" | "OUT_SUBST" | "ERR_SUBST" | "CMP", _) => {}
                            _ => continue 'tests,
                        }
                    }
                }
                _ => continue 'tests,
            }
        }
        let joined = shell_args.join(" ");
        if joined.contains('`') || joined.contains("$(") {
            continue;
        }
        let words = f01_regex::shell::simple_commands(&joined);
        let mut argv = vec!["sed".to_string()];
        match words.len() {
            0 => {}
            1 => argv.extend(words[0].iter().cloned()),
            _ => continue,
        }
        argv.extend(file_args);
        let mut c = case(format!("misc-{name}"), argv, stdin, files, vec!["src:upstream-misc".into()]);
        c.env = env;
        cases.push(c);
    }
    Ok(cases)
}

/// Testes de script com entrada e saída em arquivo (`madding`, `uniq`, `mac-mf`).
fn sed_scripts(dir: &Path) -> Result<Vec<Case>> {
    let mut out = Vec::new();
    for (name, stdin_input) in [("madding", false), ("uniq", true), ("mac-mf", false)] {
        let script = std::fs::read(dir.join(format!("{name}.sed")))?;
        let input = std::fs::read(dir.join(format!("{name}.inp")))?;
        let mut files = BTreeMap::new();
        files.insert(format!("{name}.sed"), file_spec(&script));
        let mut argv = vec!["sed".to_string(), "-f".to_string(), format!("{name}.sed")];
        let stdin = if stdin_input {
            Some(input)
        } else {
            files.insert(format!("{name}.inp"), file_spec(&input));
            argv.push(format!("{name}.inp"));
            None
        };
        out.push(case(format!("script-{name}"), argv, stdin, files, vec!["src:upstream-scripts".into()]));
    }
    Ok(out)
}

fn main() -> Result<()> {
    let up = harness::paths::corpus_dir().join("upstream");
    let grep_dir = up.join("grep");
    let sed_dir = up.join("sed/testsuite");
    write("grep", "upstream-foad1", "upstream:grep-3.11/tests/foad1", grep_foad1(&grep_dir)?)?;
    write("grep", "upstream-yesno", "upstream:grep-3.11/tests/yesno", grep_yesno(&grep_dir)?)?;
    write("sed", "upstream-misc", "upstream:sed-4.9/testsuite/misc.pl", sed_misc(&sed_dir)?)?;
    write("sed", "upstream-scripts", "upstream:sed-4.9/testsuite/{madding,uniq,mac-mf}", sed_scripts(&sed_dir)?)?;
    Ok(())
}
