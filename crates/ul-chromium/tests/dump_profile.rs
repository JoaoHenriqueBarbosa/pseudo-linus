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
mod support;
const PORT: u16 = 8000;

fn dump_client(ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    let s = sys::current();
    let (listener, _) = s.tcp_listen_at(std::net::Ipv4Addr::LOCALHOST.into(), PORT, 16, false, true).unwrap();
    let server = s.spawn_fn(ProcAttrs::default(), b"server".to_vec(), Box::new(move || support::serve(listener, PAGE))).unwrap();
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
    // O shared_proto_db: depois de três execuções os tamanhos do oráculo (RUNS=3: 14853, 237 e 774, 255), com os dois LOG
    // 18 bytes menores: o caminho do perfil aqui (/tmp/p) tem 6 caracteres a menos que o do oráculo (/tmp/profile) e aparece três vezes.
    let spdb: Vec<i64> = ["000003.log", "LOG", "LOG.old", "metadata/000003.log", "metadata/LOG", "metadata/LOG.old"].iter().map(|name| size(format!("/tmp/p/Default/shared_proto_db/{name}").as_bytes())).collect();
    let _ = ctx.stdout().write_all(format!("spdb {spdb:?}\n").as_bytes());
    let hex = |path: &[u8]| sys::read_file(path).unwrap_or_default().iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let _ = ctx.stdout().write_all(format!("SPDB-MAIN {}\nSPDB-META {}\n", hex(b"/tmp/p/Default/shared_proto_db/000003.log"), hex(b"/tmp/p/Default/shared_proto_db/metadata/000003.log")).as_bytes());
    // O estado de rede sem rede (como o oráculo em contêiner sem rede) e nenhum TransportSecurity.
    let network = sys::read_file(b"/tmp/p/Default/Network Persistent State").map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default();
    let _ = ctx.stdout().write_all(format!("network {network} transport {}\n", sys::lstat(b"/tmp/p/Default/TransportSecurity").is_ok()).as_bytes());
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
    assert!(stdout.contains("spdb [14853, 219, 219, 774, 237, 237]\n"), "{stdout}");
    assert!(stdout.contains(r#"network {"net":{"http_server_properties":{"servers":[],"version":5},"network_qualities":{"CAYSABiAgICA+P////8B":"Offline"}}} transport false"#), "{stdout}");
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

/// Os registros de um log do leveldb como (sequência, operações), com os instantes em segundos WebKit que fecham os
/// valores de proto (varint de cinco bytes entre 13 e 14 bilhões; mudam a cada execução) trocados por zeros.
fn log_batches(bytes: &[u8]) -> Vec<(u64, Vec<ul_leveldb::batch::Op>)> {
    use ul_leveldb::batch::{Op, WriteBatch};
    let is_time = |chunk: &[u8]| {
        chunk[..4].iter().all(|byte| byte & 0x80 != 0) && chunk[4] & 0x80 == 0 && {
            let value = chunk.iter().enumerate().fold(0u64, |acc, (i, byte)| acc | u64::from(byte & 0x7F) << (7 * i));
            (13_000_000_000..14_000_000_000).contains(&value)
        }
    };
    ul_leveldb::log::read_records(bytes)
        .unwrap()
        .iter()
        .map(|record| {
            let ops = WriteBatch::from_contents(record).unwrap().ops().unwrap();
            let masked = ops
                .into_iter()
                .map(|op| match op {
                    Op::Put { key, mut value } => {
                        let mut at = 0;
                        while at + 5 <= value.len() {
                            if is_time(&value[at..at + 5]) {
                                value[at..at + 5].fill(0);
                                at += 5;
                            } else {
                                at += 1;
                            }
                        }
                        Op::Put { key, value }
                    }
                    other => other,
                })
                .collect();
            (u64::from_le_bytes(record[..8].try_into().unwrap()), masked)
        })
        .collect()
}

/// O `shared_proto_db` de três execuções confere com o do oráculo (`RUNS=3`): o principal registro a registro (sequência,
/// chaves e valores, menos o instante) e o `metadata` com as lojas na ordem que o oráculo sorteia (a ordem muda a cada
/// execução, então cada volta é comparada como conjunto).
#[test]
fn shared_proto_db_after_three_runs_matches_the_oracle() {
    let kernel = Kernel::new(KernelConfig::default());
    let mut programs = ul_chromium::programs();
    programs.extend(ul_sqlite::programs());
    programs.push(Program::bin("dump_client", dump_client));
    let sandbox = kernel.create_sandbox(SandboxConfig { programs, ..SandboxConfig::default() }).unwrap();
    let output = sandbox.run(RunRequest { argv: vec![b"dump_client".to_vec()], timeout: Some(Duration::from_secs(120)), ..RunRequest::default() }).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let from_hex = |label: &str| -> Vec<u8> {
        let line = stdout.lines().find_map(|line| line.strip_prefix(label)).unwrap_or_else(|| panic!("{stdout}"));
        (0..line.len() / 2).map(|i| u8::from_str_radix(&line[2 * i..2 * i + 2], 16).unwrap()).collect()
    };
    let main = log_batches(&from_hex("SPDB-MAIN "));
    let oracle_main = log_batches(include_bytes!("fixtures/shared_proto_db/persist-3-runs-main.log"));
    assert_eq!(main.len(), oracle_main.len());
    for (index, (ours, theirs)) in main.iter().zip(&oracle_main).enumerate() {
        assert_eq!(ours, theirs, "registro {index}");
    }

    let meta = log_batches(&from_hex("SPDB-META "));
    let oracle_meta = log_batches(include_bytes!("fixtures/shared_proto_db/persist-3-runs-metadata.log"));
    assert_eq!(meta.len(), oracle_meta.len());
    // Dois `__global`, duas voltas de onze lojas e um `__global` por execução seguinte; as voltas como conjunto.
    let sorted = |slice: &[(u64, Vec<ul_leveldb::batch::Op>)]| {
        let mut ops: Vec<_> = slice.iter().map(|(_, ops)| ops.clone()).collect();
        ops.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        ops
    };
    assert_eq!(meta[..2], oracle_meta[..2]);
    assert_eq!(sorted(&meta[2..13]), sorted(&oracle_meta[2..13]));
    assert_eq!(sorted(&meta[13..24]), sorted(&oracle_meta[13..24]));
    assert_eq!(meta[24..], oracle_meta[24..]);
    assert_eq!(meta.iter().map(|(sequence, _)| *sequence).collect::<Vec<_>>(), oracle_meta.iter().map(|(sequence, _)| *sequence).collect::<Vec<_>>());
    // O `50_` vem primeiro nas duas voltas, como nas 68 amostras do oráculo.
    for lap in [&meta[2..13], &meta[13..24]] {
        let ul_leveldb::batch::Op::Put { key, .. } = &lap[0].1[0] else { panic!() };
        assert_eq!(key, b"50_");
    }
}
