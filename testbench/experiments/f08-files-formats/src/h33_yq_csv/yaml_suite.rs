//! Métrica separada da H33/yq: taxa de parse correto de cada parser YAML em Rust na YAML test suite
//! (github.com/yaml/yaml-test-suite, branch `data`), baixada pra `corpus/upstream/yaml/` (gitignored).
//!
//! Por teste (diretório com `in.yaml`): se há arquivo `error`, o parser tem que rejeitar; se há
//! `in.json`, os documentos lidos têm que ser iguais aos do JSON (números comparados como f64); sem
//! `in.json` (chave composta, tags exóticas), conta só se o parser aceita.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use super::yaml_layers::{Node, YamlLayer, all_layers};

const REPO: &str = "https://github.com/yaml/yaml-test-suite";

pub fn suite_dir() -> PathBuf {
    harness::paths::corpus_dir().join("upstream").join("yaml").join("yaml-test-suite")
}

/// Garante a suíte no disco (clona a branch `data` se faltar). Devolve o commit.
fn ensure_suite() -> Result<String, String> {
    let dir = suite_dir();
    if !dir.join(".git").exists() {
        std::fs::create_dir_all(dir.parent().expect("pai")).map_err(|e| e.to_string())?;
        let status = Command::new("git")
            .args(["clone", "--quiet", "--depth", "1", "--branch", "data", REPO])
            .arg(&dir)
            .status()
            .map_err(|e| format!("git: {e}"))?;
        if !status.success() {
            return Err("git clone da yaml-test-suite falhou".into());
        }
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

struct TestCase {
    id: String,
    yaml: String,
    error: bool,
    json: Option<Vec<Value>>,
}

fn collect_tests(dir: &Path, prefix: &str, out: &mut Vec<TestCase>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_dir()).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.starts_with('.') || name == "name" || name == "tags" {
            continue;
        }
        let id = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let in_yaml = path.join("in.yaml");
        if in_yaml.exists() {
            let Ok(bytes) = std::fs::read(&in_yaml) else { continue };
            let json = std::fs::read_to_string(path.join("in.json")).ok().map(|text| {
                serde_json::Deserializer::from_str(&text).into_iter::<Value>().filter_map(Result::ok).collect()
            });
            out.push(TestCase {
                id: id.clone(),
                yaml: String::from_utf8_lossy(&bytes).into_owned(),
                error: path.join("error").exists(),
                json,
            });
        }
        // Testes com variantes ficam em subdiretórios (00, 01, ...).
        collect_tests(&path, &id, out);
    }
}

/// [`Node`] pra JSON com a semântica "pura" (sem Python): é o que o `in.json` da suíte descreve.
fn node_to_json(n: &Node) -> Value {
    match n {
        Node::Null => Value::Null,
        Node::Bool(b) => Value::Bool(*b),
        Node::Int(s) => s.parse::<i64>().map(Value::from).unwrap_or_else(|_| json!(s.parse::<f64>().unwrap_or(f64::NAN))),
        Node::Float(f) => serde_json::Number::from_f64(*f).map(Value::Number).unwrap_or(Value::Null),
        Node::Str(s) => Value::String(s.clone()),
        Node::Seq(items) => Value::Array(items.iter().map(node_to_json).collect()),
        Node::Map(pairs) => {
            let mut map = serde_json::Map::new();
            for (k, v) in pairs {
                let key = match k {
                    Node::Str(s) => s.clone(),
                    other => node_to_json(other).to_string(),
                };
                map.insert(key, node_to_json(v));
            }
            Value::Object(map)
        }
    }
}

fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| json_eq(a, b)),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| json_eq(v, w)))
        }
        _ => a == b,
    }
}

fn load_guarded(layer: &dyn YamlLayer, text: &str) -> Result<Vec<Node>, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| layer.load(text)))
        .unwrap_or_else(|_| Err("panic".into()))
}

pub fn run_suite() -> Value {
    let commit = match ensure_suite() {
        Ok(c) => c,
        Err(e) => return json!({"skipped": e}),
    };
    let mut tests = Vec::new();
    collect_tests(&suite_dir(), "", &mut tests);
    // Panics de parser viram falha do teste; o hook padrão só poluiria o stderr.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut per_layer = Vec::new();
    let mut layers = all_layers();
    // A camada nossa resolve escalares como o yq (012 = 10), não como o core schema 1.2: as poucas
    // divergências dela aqui são essa escolha, o resto mede a estrutura do saphyr-parser 0.1.
    layers.push(Box::new(super::yq_resolver::YqResolverLayer));
    for layer in layers {
        let (mut ok_valid, mut total_valid) = (0, 0);
        let (mut ok_error, mut total_error) = (0, 0);
        let (mut ok_nojson, mut total_nojson) = (0, 0);
        let mut panics = 0;
        let mut failing = Vec::new();
        eprintln!("h33 yaml-test-suite: {}", layer.name());
        for t in &tests {
            if std::env::var_os("F08_YAML_TRACE").is_some() {
                eprintln!("  {}", t.id);
            }
            let got = load_guarded(layer.as_ref(), &t.yaml);
            if matches!(&got, Err(e) if e == "panic") {
                panics += 1;
            }
            let pass = if t.error {
                total_error += 1;
                let p = got.is_err();
                ok_error += p as usize;
                p
            } else if let Some(want) = &t.json {
                total_valid += 1;
                let p = match &got {
                    Ok(docs) => {
                        let docs: Vec<Value> = docs.iter().map(node_to_json).collect();
                        docs.len() == want.len() && docs.iter().zip(want).all(|(a, b)| json_eq(a, b))
                    }
                    Err(_) => false,
                };
                ok_valid += p as usize;
                p
            } else {
                total_nojson += 1;
                let p = got.is_ok();
                ok_nojson += p as usize;
                p
            };
            if !pass && failing.len() < 40 {
                failing.push(t.id.clone());
            }
        }
        let total = total_valid + total_error + total_nojson;
        let ok = ok_valid + ok_error + ok_nojson;
        per_layer.push(json!({
            "layer": layer.name(),
            "pass": ok,
            "total": total,
            "rate": if total == 0 { 0.0 } else { (ok as f64 / total as f64 * 1000.0).round() / 1000.0 },
            "valid_with_json": {"pass": ok_valid, "total": total_valid},
            "must_reject": {"pass": ok_error, "total": total_error},
            "valid_without_json": {"pass": ok_nojson, "total": total_nojson},
            "panics": panics,
            "failing_sample": failing,
        }));
    }
    std::panic::set_hook(previous_hook);
    json!({"repo": REPO, "branch": "data", "commit": commit, "tests": tests.len(), "layers": per_layer})
}
