//! O perfil que o `chromium --remote-debugging-port` com uma página deixa ao sair pelo `Browser.close`, contra o
//! Chromium 154 do oráculo (`testbench/oracle/measure-navigation.sh` com `MODO=close`, `REDE=none` e `ss-none.html`): o
//! cache de disco com a página e o favicon (índice de 96 bytes), o `Preferences` com o engajamento da origem, o
//! QuotaManager (40960 bytes e o journal vazio), o `Network Persistent State` sem rede (116 bytes), o `ukm_db` de 49152
//! bytes sem WAL com a linha de `urls` e o evento da fonte 49, e os `Singleton*` apagados. A árvore sem página está em
//! `disk_profile.rs`.

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::sys;
use sysabi::*;

mod support;

const PAGE: &str = include_str!("../../../testbench/corpus/headless-storage/ss-none.html");

fn close_client(ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    let s = sys::current();
    let server = support::run_devtools(PAGE, "ss-none.html", "close", 3.0);
    let size = |path: &str| sys::read_file(path.as_bytes()).map_or(-1, |bytes| bytes.len() as i64);
    let exists = |path: &str| sys::lstat(path.as_bytes()).is_ok();
    let mut out = String::new();
    let cache = sys::read_dir(b"/tmp/h/Default/Cache/Cache_Data").unwrap_or_default().into_iter().filter(|e| e.name.ends_with(b"_0")).count();
    out += &format!("cache {cache} index {}\n", size("/tmp/h/Default/Cache/Cache_Data/index-dir/the-real-index"));
    out += &format!("quota {} journal {}\n", size("/tmp/h/Default/WebStorage/QuotaManager"), size("/tmp/h/Default/WebStorage/QuotaManager-journal"));
    out += &format!("network {}\n", String::from_utf8_lossy(&sys::read_file(b"/tmp/h/Default/Network Persistent State").unwrap_or_default()));
    out += &format!("transport {}\n", exists("/tmp/h/Default/TransportSecurity"));
    out += &format!("singletons {}\n", ["SingletonCookie", "SingletonLock", "SingletonSocket"].iter().filter(|n| exists(&format!("/tmp/h/{n}"))).count());
    let prefs = String::from_utf8_lossy(&sys::read_file(b"/tmp/h/Default/Preferences").unwrap_or_default()).into_owned();
    out += &format!("prefs omnibox-like {} site {} visits {}\n", prefs.contains("\"pointsAddedToday\":4.5,\"rawScore\":4.5"), prefs.contains("\"pointsAddedToday\":3.0,\"rawScore\":3.0"), prefs.contains("\"visits\":1"));
    out += &format!("ukm {} wal {} shm {}\n", size("/tmp/h/segmentation_platform/ukm_db"), exists("/tmp/h/segmentation_platform/ukm_db-wal"), exists("/tmp/h/segmentation_platform/ukm_db-shm"));
    // O banco em WAL só abre em modo exclusivo no VFS do sandbox.
    let connection = ul_sqlite::cli::open_conn("/tmp/h/segmentation_platform/ukm_db", rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
    connection.query_row("PRAGMA locking_mode=EXCLUSIVE", [], |_| Ok(())).unwrap();
    let urls: i64 = connection.query_row("select count(*) from urls where url = 'http://127.0.0.1:8000/ss-none.html'", [], |row| row.get(0)).unwrap();
    let events: Vec<(i64, i64)> = connection.prepare("select ukm_source_id, url_id from metrics").unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect();
    out += &format!("ukm urls {urls} events {events:?}\n");
    let _ = ctx.stdout().write_all(out.as_bytes());
    let _ = s.kill(KillTarget::Pid(server), Signal::SIGKILL);
    let _ = s.wait4(WaitTarget::Pid(server), WaitOptions::empty());
    0
}

#[test]
fn browser_close_with_a_page_leaves_the_oracles_profile() {
    let kernel = Kernel::new(KernelConfig::default());
    let mut programs = ul_chromium::programs();
    programs.push(Program::bin("close_client", close_client));
    let sandbox = kernel.create_sandbox(SandboxConfig { programs, ..SandboxConfig::default() }).unwrap();
    let output = sandbox.run(RunRequest { argv: vec![b"close_client".to_vec()], timeout: Some(Duration::from_secs(120)), ..RunRequest::default() }).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let expected = "cache 2 index 96\n\
        quota 40960 journal 0\n\
        network {\"net\":{\"http_server_properties\":{\"servers\":[],\"version\":5},\"network_qualities\":{\"CAYSABiAgICA+P////8B\":\"Offline\"}}}\n\
        transport false\n\
        singletons 0\n\
        prefs omnibox-like true site true visits true\n\
        ukm 49152 wal false shm false\n\
        ukm urls 1 events [(49, 2988681432195754744)]\n";
    assert_eq!(stdout, expected);
}
