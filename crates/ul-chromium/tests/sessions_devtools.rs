//! Os arquivos de sessão do `chromium --remote-debugging-port` com uma URL na linha de comando, contra o Chromium 154 do
//! oráculo (`testbench/oracle/measure-navigation.sh` com `MODO=term` e `MODO=close`, `ss-none.html`): o `Session_` já
//! leva a página (entrada única, 1170 bytes) e o `Browser.close` grava o `Tabs_` (912 bytes, com o título); o SIGTERM só
//! deixa o `Session_`, aos 4, 8 e 12 s. A forma byte a byte dos dois arquivos está em `tests/sessions.rs`; aqui se confere
//! o que o navegador de verdade deixa em disco.

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::sys;
use sysabi::*;

mod support;

const PAGE: &str = include_str!("../../../testbench/corpus/headless-storage/ss-none.html");

/// `sessions_client COMO SEGUNDOS`: o nome e o tamanho dos arquivos de `Default/Sessions` e se o `Session_` leva o
/// endereço da página, depois de encerrar o navegador.
fn sessions_client(ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let how = argv[1].to_string_lossy().into_owned();
    let seconds: f64 = argv[2].to_string_lossy().parse().unwrap();
    let s = sys::current();
    let server = support::run_devtools(PAGE, "ss-none.html", &how, seconds);
    let mut files: Vec<(String, Vec<u8>)> = sys::read_dir(b"/tmp/h/Default/Sessions")
        .unwrap_or_default()
        .into_iter()
        .map(|entry| {
            let content = sys::read_file(&[b"/tmp/h/Default/Sessions/".as_slice(), &entry.name].concat()).unwrap_or_default();
            (String::from_utf8_lossy(&entry.name).into_owned(), content)
        })
        .collect();
    files.sort();
    let url = b"http://127.0.0.1:8000/ss-none.html";
    for (name, content) in files {
        let prefix = name.split('_').next().unwrap_or_default();
        let has_url = content.windows(url.len()).any(|window| window == url);
        let _ = ctx.stdout().write_all(format!("{prefix} {} url {has_url}\n", content.len()).as_bytes());
    }
    let _ = s.kill(KillTarget::Pid(server), Signal::SIGKILL);
    let _ = s.wait4(WaitTarget::Pid(server), WaitOptions::empty());
    0
}

fn run(how: &str, seconds: &str) -> String {
    let kernel = Kernel::new(KernelConfig::default());
    let mut programs = ul_chromium::programs();
    programs.push(Program::bin("sessions_client", sessions_client));
    let sandbox = kernel.create_sandbox(SandboxConfig { programs, ..SandboxConfig::default() }).unwrap();
    let output = sandbox.run(RunRequest { argv: vec![b"sessions_client".to_vec(), how.as_bytes().to_vec(), seconds.as_bytes().to_vec()], timeout: Some(Duration::from_secs(120)), ..RunRequest::default() }).unwrap();
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn sigterm_leaves_only_the_session_file_with_the_page() {
    assert_eq!(run("term", "3"), "Session 1170 url true\n");
}

#[test]
fn browser_close_also_writes_the_tabs_file() {
    assert_eq!(run("close", "3"), "Session 1170 url true\nTabs 912 url true\n");
}
