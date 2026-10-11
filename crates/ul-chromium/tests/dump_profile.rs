//! O `chromium --dump-dom` com `--user-data-dir` contra o Chromium 154 do oráculo: o `localStorage` e o IndexedDB que
//! uma execução grava a seguinte lê (`testbench/corpus/headless-storage/persist.html`, servida por HTTP em
//! `127.0.0.1:8000`, três execuções no mesmo perfil; o oráculo deu `ls=null idb=0`, `ls=1 idb=1` e `ls=2 idb=2`), e a
//! saída deixa o perfil do `--dump-dom` (sem os `Singleton*`).

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::sys;
use sysabi::*;

const PAGE: &str = include_str!("../../../testbench/corpus/headless-storage/persist.html");
const PORT: u16 = 8000;

/// Responde toda requisição com a página, em HTTP/1.0 como o `http.server` do Python.
fn serve(listener: Fd) -> i32 {
    let s = sys::current();
    loop {
        let Ok((fd, _)) = s.tcp_accept(listener, false, true) else { continue };
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            match sys::read(fd, &mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => request.extend_from_slice(&buffer[..n]),
            }
        }
        let response = format!("HTTP/1.0 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{PAGE}", PAGE.len());
        let _ = sys::write_all(fd, response.as_bytes());
        let _ = sys::close(fd);
    }
}

fn dump_client(ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    let s = sys::current();
    let (listener, _) = s.tcp_listen_at(std::net::Ipv4Addr::LOCALHOST.into(), PORT, 16, false, true).unwrap();
    let server = s.spawn_fn(ProcAttrs::default(), b"server".to_vec(), Box::new(move || serve(listener))).unwrap();
    for _ in 0..3 {
        let argv: Vec<Vec<u8>> =
            ["chromium", "--headless", "--no-sandbox", "--disable-gpu", "--user-data-dir=/tmp/p", "--virtual-time-budget=20000", "--dump-dom", "http://127.0.0.1:8000/persist.html"]
                .iter()
                .map(|a| a.as_bytes().to_vec())
                .collect();
        let pid = s.spawn(SpawnSpec { path: b"/usr/bin/chromium".to_vec(), argv, attrs: ProcAttrs::default() }).unwrap();
        let status = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap().unwrap().1;
        let _ = ctx.stdout().write_all(format!("\nstatus {status:?}\n").as_bytes());
    }
    let query = "select url, title, visit_count, typed_count from urls; select count(*), count(distinct segment_id), min(transition), max(visit_duration) > 0 from visits; select count(*) from segments; select visit_id, page_end_reason from context_annotations order by visit_id desc limit 1";
    let argv: Vec<Vec<u8>> = ["sqlite3", "/tmp/p/Default/History", query].iter().map(|a| a.as_bytes().to_vec()).collect();
    let pid = s.spawn(SpawnSpec { path: b"/usr/bin/sqlite3".to_vec(), argv, attrs: ProcAttrs::default() }).unwrap();
    let _ = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty());
    // O QuotaManager: o bucket da origem, com duas aberturas por execução (três execuções, seis usos).
    let query = "select id, storage_key, host, name, use_count, expiration, quota, persistent, durability from buckets; select key, value from meta order by key";
    let argv: Vec<Vec<u8>> = ["sqlite3", "/tmp/p/Default/WebStorage/QuotaManager", query].iter().map(|a| a.as_bytes().to_vec()).collect();
    let pid = s.spawn(SpawnSpec { path: b"/usr/bin/sqlite3".to_vec(), argv, attrs: ProcAttrs::default() }).unwrap();
    let _ = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty());
    let size = |path: &[u8]| sys::read_file(path).map_or(-1, |bytes| bytes.len() as i64);
    let _ = ctx.stdout().write_all(format!("quota {} journal {}\n", size(b"/tmp/p/Default/WebStorage/QuotaManager"), size(b"/tmp/p/Default/WebStorage/QuotaManager-journal")).as_bytes());
    // Os arquivos de sessão da saída levam o endereço navegado (SNSS: o `Session_*` e o `Tabs_*`).
    for prefix in ["Session_", "Tabs_"] {
        let found = sys::read_dir(b"/tmp/p/Default/Sessions").unwrap_or_default().into_iter().find(|e| e.name.starts_with(prefix.as_bytes()));
        let has_url = found.and_then(|e| sys::read_file(&[b"/tmp/p/Default/Sessions/".as_slice(), &e.name].concat()).ok()).is_some_and(|bytes| bytes.windows(33).any(|w| w == b"http://127.0.0.1:8000/persist.html"[..33].as_ref()));
        let _ = ctx.stdout().write_all(format!("{prefix} has url {has_url}\n").as_bytes());
    }
    // O cache de disco: a página e o favicon (a mesma resposta nas três execuções), o índice com as duas.
    let entries = sys::read_dir(b"/tmp/p/Default/Cache/Cache_Data").unwrap_or_default().into_iter().filter(|e| e.name.ends_with(b"_0")).count();
    let index = sys::read_file(b"/tmp/p/Default/Cache/Cache_Data/index-dir/the-real-index").map_or(0, |bytes| bytes.len());
    let _ = ctx.stdout().write_all(format!("cache entries {entries} index {index}\n").as_bytes());
    // O Preferences: o engajamento do site e o de mídia da origem navegada, e o instante do último engajamento.
    let prefs = sys::read_file(b"/tmp/p/Default/Preferences").map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default();
    // Três execuções: o engajamento acumula como no oráculo (omnibox 10.5, site 6.0, três visitas de mídia).
    let _ = ctx.stdout().write_all(format!("prefs omnibox {} site {} media {}\n", prefs.contains("\"pointsAddedToday\":10.5,\"rawScore\":10.5"), prefs.contains("\"pointsAddedToday\":6.0,\"rawScore\":6.0"), prefs.contains("\"visits\":3}")).as_bytes());
    let _ = ctx.stdout().write_all(format!("PREFS-BEGIN{prefs}PREFS-END\n").as_bytes());
    let singletons = ["SingletonCookie", "SingletonLock", "SingletonSocket"].iter().filter(|n| sys::lstat(format!("/tmp/p/{n}").as_bytes()).is_ok()).count();
    let _ = ctx.stdout().write_all(format!("singletons {singletons}\n").as_bytes());
    let _ = s.kill(KillTarget::Pid(server), Signal::SIGKILL);
    let _ = s.wait4(WaitTarget::Pid(server), WaitOptions::empty());
    0
}

#[test]
fn each_dump_reads_what_the_previous_one_wrote() {
    let kernel = Kernel::new(KernelConfig::default());
    let mut programs = ul_chromium::programs();
    programs.extend(ul_sqlite::programs());
    programs.push(Program::bin("dump_client", dump_client));
    let sandbox = kernel.create_sandbox(SandboxConfig { programs, ..SandboxConfig::default() }).unwrap();
    let output = sandbox.run(RunRequest { argv: vec![b"dump_client".to_vec()], timeout: Some(Duration::from_secs(120)), ..RunRequest::default() }).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let bodies: Vec<&str> = stdout.lines().filter_map(|line| line.strip_prefix("</head><body>")).collect();
    assert_eq!(bodies, ["ls=null idb=0</body></html>", "ls=1 idb=1</body></html>", "ls=2 idb=2</body></html>"], "{stdout}");
    assert_eq!(stdout.matches("status Exited(0)").count(), 3, "{stdout}");
    // O History: a mesma página digitada três vezes é um endereço com três visitas, de um segmento só, e a última
    // visita de cada execução termina com o fechamento do processo (motivo 6).
    assert!(stdout.contains("http://127.0.0.1:8000/persist.html|persist|3|3\n3|1|939524097|1\n1\n3|6\n"), "{stdout}");
    assert!(stdout.contains("1|http://127.0.0.1:8000/|127.0.0.1|_default|6|0|0|0|0\nIsBucketsBootstrapped|1\nIsMediaLicenseDatabaseRemoved|1\nlast_compatible_version|11\nmmap_status|-1\nversion|11\n"), "{stdout}");
    assert!(stdout.contains("quota 40960 journal 0\n"), "{stdout}");
    assert!(stdout.contains("Session_ has url true\nTabs_ has url true\n"), "{stdout}");
    assert!(stdout.contains("cache entries 2 index 96\n"), "{stdout}");
    assert!(stdout.contains("prefs omnibox true site true media true\n"), "{stdout}");
    assert!(stdout.ends_with("singletons 0\n"), "{stdout}");
}

/// Os instantes (textos de 14 dígitos ou mais e doubles acima de 1e15) viram `"T"`: o que resta é a forma e as contas.
fn mask_times(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::String(text) if text.len() >= 14 && text.bytes().all(|b| b.is_ascii_digit()) => Value::String("T".into()),
        Value::Number(number) if number.as_f64().is_some_and(|n| n.fract() == 0.0 && n >= 1e15 && !number.is_i64()) => Value::String("T".into()),
        Value::Object(map) => Value::Object(map.into_iter().map(|(key, inner)| (key, mask_times(inner))).collect()),
        other => other,
    }
}

/// As três peças do `Preferences` que uma navegação mexe: `site_engagement`, `media_engagement` e `last_engagement_time`.
fn engagement(preferences: &serde_json::Value) -> serde_json::Value {
    let exceptions = &preferences["profile"]["content_settings"]["exceptions"];
    serde_json::json!({
        "site_engagement": exceptions["site_engagement"],
        "media_engagement": exceptions["media_engagement"],
        "last_engagement_time": preferences["profile"]["last_engagement_time"],
    })
}

/// O `Preferences` de três execuções seguidas confere com o do oráculo (`measure-navigation.sh` com `RUNS=3`) campo a
/// campo, menos os instantes, que mudam a cada execução.
#[test]
fn preferences_after_three_runs_match_the_oracle() {
    let kernel = Kernel::new(KernelConfig::default());
    let mut programs = ul_chromium::programs();
    programs.extend(ul_sqlite::programs());
    programs.push(Program::bin("dump_client", dump_client));
    let sandbox = kernel.create_sandbox(SandboxConfig { programs, ..SandboxConfig::default() }).unwrap();
    let output = sandbox.run(RunRequest { argv: vec![b"dump_client".to_vec()], timeout: Some(Duration::from_secs(120)), ..RunRequest::default() }).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let text = stdout.split("PREFS-BEGIN").nth(1).and_then(|rest| rest.split("PREFS-END").next()).expect(&stdout);
    let ours = engagement(&serde_json::from_str(text).unwrap());
    let oracle: serde_json::Value = serde_json::from_str(include_str!("fixtures/preferences/persist-3-runs.json")).unwrap();
    assert_eq!(mask_times(ours.clone()), mask_times(oracle), "{ours:#}");
    // A omnibox é engajada antes da página, nas quatro execuções medidas.
    let time = |key: &str| ours["site_engagement"][key]["setting"]["lastEngagementTime"].as_f64().unwrap();
    assert!(time("chrome://omnibox-popup.top-chrome/,*") < time("http://127.0.0.1:8000,*"));
    assert_eq!(ours["last_engagement_time"].as_str().unwrap().parse::<f64>().unwrap(), time("http://127.0.0.1:8000,*"));
}
