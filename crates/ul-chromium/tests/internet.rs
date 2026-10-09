//! Placar do conjunto da internet: cada página de `testbench/corpus/internet/pages.txt` roda sobre a fita gravada (as
//! mesmas respostas que o oráculo recebeu no replay) e o DOM despejado é comparado com o golden do chromium do Debian
//! (`testbench/oracle/gen-internet.sh`). O placar sai por grupo (páginas estáticas e SPAs) e o teste falha abaixo da
//! meta. Páginas que o próprio oráculo não repete (`unstable.txt`) ficam fora.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ul_chromium::fetch::Fetcher;
use ul_chromium::page::Page;
use ul_net::net::client::{FetchError, Response};
use ul_net::net::url;

/// Fração mínima de páginas idênticas ao oráculo, em cada grupo.
const GOAL: f64 = 0.95;
/// O mesmo `--virtual-time-budget` que o oráculo usou.
const BUDGET_MS: f64 = 15000.0;

fn bench() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testbench")
}

struct Entry {
    status: u16,
    headers: Vec<(Vec<u8>, Vec<u8>)>,
    body: Vec<u8>,
}

/// A chave do replay: método, host com porta e caminho com consulta, sem esquema (o servidor do oráculo responde nas
/// duas portas com a mesma tabela).
fn key(method: &str, address: &str) -> Option<String> {
    let parsed = url::parse(address, false).ok()?;
    let host = match parsed.port {
        Some(port) if Some(port) != url::default_port(&parsed.scheme) => format!("{}:{port}", parsed.host_for_url()),
        _ => parsed.host_for_url(),
    };
    Some(format!("{method} {}{}", host.to_ascii_lowercase(), parsed.target()))
}

/// O fetcher que responde da fita, seguindo os redirecionamentos dela como o cliente HTTP seguiria.
fn tape_fetcher(dir: &Path) -> Fetcher {
    let tape: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("tape.json")).unwrap()).unwrap();
    let mut table: HashMap<String, Rc<Entry>> = HashMap::new();
    for item in tape["entries"].as_array().unwrap() {
        let address = item["url"].as_str().unwrap();
        let headers: Vec<(Vec<u8>, Vec<u8>)> = item["headers"].as_array().unwrap().iter().map(|pair| (pair[0].as_str().unwrap().as_bytes().to_vec(), pair[1].as_str().unwrap().as_bytes().to_vec())).collect();
        let location = headers.iter().find(|(n, _)| n == b"location").map(|(_, v)| String::from_utf8_lossy(v).into_owned());
        if let (Some(location), Ok(from)) = (&location, url::parse(address, false)) {
            if let Ok(to) = url::resolve(&from, location) {
                if to.host == from.host && to.target() == from.target() {
                    continue;
                }
            }
        }
        let body = match item["body"].as_str() {
            Some(digest) => std::fs::read(dir.join("bodies").join(digest)).unwrap_or_default(),
            None => Vec::new(),
        };
        let Some(k) = key(item["method"].as_str().unwrap(), address) else { continue };
        table.entry(k).or_insert_with(|| Rc::new(Entry { status: item["status"].as_u64().unwrap() as u16, headers, body }));
    }
    Rc::new(move |target: &str, spec: &ul_net::net::client::RequestSpec<'_>| {
        let mut current = url::parse(target, false).map_err(FetchError::InvalidUrl)?;
        let mut method = spec.method.to_owned();
        for _ in 0..=spec.max_redirects {
            let address = current.to_string_with(false);
            let found = key(&method, &address).and_then(|k| table.get(&k).cloned()).or_else(|| if method == "HEAD" { key("GET", &address).and_then(|k| table.get(&k).cloned()) } else { None });
            let entry = match found {
                Some(entry) => entry,
                None => Rc::new(Entry { status: 404, headers: vec![(b"content-type".to_vec(), b"text/plain".to_vec())], body: Vec::new() }),
            };
            let location = matches!(entry.status, 301 | 302 | 303 | 307 | 308).then(|| entry.headers.iter().find(|(n, _)| n == b"location").map(|(_, v)| String::from_utf8_lossy(v).into_owned())).flatten();
            if let Some(location) = location {
                current = url::resolve(&current, &location).map_err(FetchError::InvalidUrl)?;
                if matches!(entry.status, 301..=303) && method != "GET" && method != "HEAD" {
                    method = "GET".to_owned();
                }
                continue;
            }
            return Ok(Response { status: entry.status, reason: String::new(), headers: entry.headers.clone(), body: entry.body.clone(), url: current });
        }
        Err(FetchError::TooManyRedirects)
    })
}

/// `CONSOLE=1`: o `console.*` da página vai para o stderr do teste (diagnóstico de divergência).
#[derive(Debug)]
struct StderrConsole;

impl zjsc::runtime::console_host::ConsoleHost for StderrConsole {
    fn write_stdout(&self, bytes: &[u8]) {
        eprint!("{}", String::from_utf8_lossy(bytes));
    }
    fn write_stderr(&self, bytes: &[u8]) {
        eprint!("{}", String::from_utf8_lossy(bytes));
    }
    fn read_stdin_line(&self) -> Option<String> {
        None
    }
    fn read_stdin_rest(&self) -> Vec<u8> {
        Vec::new()
    }
}

#[test]
fn internet_scoreboard() {
    let corpus = bench().join("corpus/internet");
    let golden = bench().join("golden/chromium-internet");
    let unstable: Vec<String> = std::fs::read_to_string(corpus.join("unstable.txt")).unwrap_or_default().lines().map(str::to_owned).collect();
    let only: Option<String> = std::env::var("PAGE").ok();
    let mut groups: HashMap<String, (usize, usize, Vec<String>)> = HashMap::new();
    for line in std::fs::read_to_string(corpus.join("pages.txt")).unwrap().lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let mut parts = line.split('|');
        let (id, group, address) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
        if unstable.iter().any(|u| u == id) || only.as_deref().is_some_and(|o| o != id) {
            continue;
        }
        let Ok(expected) = std::fs::read_to_string(golden.join(format!("{id}.dom"))) else { continue };
        eprintln!("página: {id}");
        let fetcher = tape_fetcher(&corpus.join(id));
        let console = std::env::var("CONSOLE").is_ok();
        // Um pânico do motor numa página conta como divergência dela e não derruba a medição das outras.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match Page::load(address, fetcher) {
            Ok(mut page) => {
                page.budget_ms = BUDGET_MS;
                if console {
                    page.console = Rc::new(StderrConsole);
                }
                page.run();
                page.dump_dom()
            }
            Err(message) => format!("falha ao carregar: {message}"),
        }));
        let actual = outcome.unwrap_or_else(|_| "pânico".to_owned());
        let slot = groups.entry(group.to_owned()).or_default();
        slot.1 += 1;
        if actual == expected {
            slot.0 += 1;
        } else {
            slot.2.push(id.to_owned());
            if let Ok(dir) = std::env::var("DIFF_DIR") {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(Path::new(&dir).join(format!("{id}.actual")), &actual);
                let _ = std::fs::write(Path::new(&dir).join(format!("{id}.expected")), &expected);
            }
        }
    }
    let mut report = String::new();
    let mut below = false;
    let mut names: Vec<_> = groups.keys().cloned().collect();
    names.sort();
    for name in names {
        let (ok, total, failing) = &groups[&name];
        let score = *ok as f64 / (*total).max(1) as f64;
        below |= score < GOAL;
        report.push_str(&format!("{name}: {ok}/{total} ({:.1}%) divergem: {}\n", score * 100.0, failing.join(", ")));
    }
    println!("{report}");
    assert!(!below || only.is_some(), "placar abaixo da meta de {:.0}%:\n{report}", GOAL * 100.0);
}
