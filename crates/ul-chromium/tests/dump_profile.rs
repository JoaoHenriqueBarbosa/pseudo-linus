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
    // Os arquivos de sessão da saída levam o endereço navegado (SNSS: o `Session_*` e o `Tabs_*`).
    for prefix in ["Session_", "Tabs_"] {
        let found = sys::read_dir(b"/tmp/p/Default/Sessions").unwrap_or_default().into_iter().find(|e| e.name.starts_with(prefix.as_bytes()));
        let has_url = found.and_then(|e| sys::read_file(&[b"/tmp/p/Default/Sessions/".as_slice(), &e.name].concat()).ok()).is_some_and(|bytes| bytes.windows(33).any(|w| w == b"http://127.0.0.1:8000/persist.html"[..33].as_ref()));
        let _ = ctx.stdout().write_all(format!("{prefix} has url {has_url}\n").as_bytes());
    }
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
    assert!(stdout.contains("Session_ has url true\nTabs_ has url true\n"), "{stdout}");
    assert!(stdout.ends_with("singletons 0\n"), "{stdout}");
}
