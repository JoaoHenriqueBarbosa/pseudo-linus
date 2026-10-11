//! O QuotaManager de uma navegação que não usa o IndexedDB (nenhum bucket): o banco nasce do molde, com 40960 bytes e o
//! journal de zero byte, e as duas chaves do bootstrap aparecem ou não conforme o sorteio (no oráculo, 10 de 20 execuções
//! de `ss-none`). O sorteio não tem semente no sandbox; o teste confere a forma que vale nos dois resultados e que o
//! resultado de uma execução não se desfaz na seguinte.

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::sys;
use sysabi::*;

const PAGE: &str = include_str!("../../../testbench/corpus/headless-storage/ss-none.html");
const PORT: u16 = 8000;
mod support;
const RUNS: usize = 2;

fn client(ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    let s = sys::current();
    let (listener, _) = s.tcp_listen_at(std::net::Ipv4Addr::LOCALHOST.into(), PORT, 16, false, true).unwrap();
    let server = s.spawn_fn(ProcAttrs::default(), b"server".to_vec(), Box::new(move || support::serve(listener, PAGE))).unwrap();
    for run in 0..RUNS {
        let argv: Vec<Vec<u8>> = ["chromium", "--headless", "--no-sandbox", "--disable-gpu", "--user-data-dir=/tmp/q", "--virtual-time-budget=20000", "--dump-dom", "http://127.0.0.1:8000/ss-none.html"]
            .iter()
            .map(|a| a.as_bytes().to_vec())
            .collect();
        let pid = s.spawn(SpawnSpec { path: b"/usr/bin/chromium".to_vec(), argv, attrs: ProcAttrs::default() }).unwrap();
        let _ = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty());
        let size = |path: &[u8]| sys::read_file(path).map_or(-1, |bytes| bytes.len() as i64);
        let _ = ctx.stdout().write_all(format!("run {run} quota {} journal {}\n", size(b"/tmp/q/Default/WebStorage/QuotaManager"), size(b"/tmp/q/Default/WebStorage/QuotaManager-journal")).as_bytes());
        let query = "select count(*) from buckets; select group_concat(key, ',') from (select key from meta order by key)";
        let argv: Vec<Vec<u8>> = ["sqlite3", "/tmp/q/Default/WebStorage/QuotaManager", query].iter().map(|a| a.as_bytes().to_vec()).collect();
        let pid = s.spawn(SpawnSpec { path: b"/usr/bin/sqlite3".to_vec(), argv, attrs: ProcAttrs::default() }).unwrap();
        let _ = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty());
    }
    let _ = s.kill(KillTarget::Pid(server), Signal::SIGKILL);
    let _ = s.wait4(WaitTarget::Pid(server), WaitOptions::empty());
    0
}

const BASE: &str = "last_compatible_version,mmap_status,version";
const BOOTSTRAPPED: &str = "IsBucketsBootstrapped,IsMediaLicenseDatabaseRemoved,last_compatible_version,mmap_status,version";

#[test]
fn two_navigations_without_a_bucket_keep_the_template_shape() {
    let kernel = Kernel::new(KernelConfig::default());
    let mut programs = ul_chromium::programs();
    programs.extend(ul_sqlite::programs());
    programs.push(Program::bin("quota_client", client));
    let sandbox = kernel.create_sandbox(SandboxConfig { programs, ..SandboxConfig::default() }).unwrap();
    let output = sandbox.run(RunRequest { argv: vec![b"quota_client".to_vec()], timeout: Some(Duration::from_secs(120)), ..RunRequest::default() }).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    // Cada execução: o arquivo do molde (40960 bytes), o journal vazio, nenhum bucket e as chaves de `meta` do molde,
    // com as do bootstrap ou sem elas.
    let mut keys = Vec::new();
    for run in 0..RUNS {
        let at = stdout.find(&format!("run {run} quota 40960 journal 0\n")).unwrap_or_else(|| panic!("{stdout}"));
        let mut lines = stdout[at..].lines().skip(1);
        assert_eq!(lines.next(), Some("0"), "{stdout}");
        let meta = lines.next().unwrap_or_default();
        assert!(meta == BASE || meta == BOOTSTRAPPED, "{meta}\n{stdout}");
        keys.push(meta);
    }
    // O que a primeira execução gravou fica: as chaves do bootstrap não somem na segunda.
    assert!(keys[0] != BOOTSTRAPPED || keys[1] == BOOTSTRAPPED, "{keys:?}");
}
