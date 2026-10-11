//! O `chromium --dump-dom` de uma página que o servidor redireciona (`/redir` para `/redir/`) contra o Chromium 154 do
//! oráculo (`testbench/oracle/measure-navigation.sh redir`): o History ganha uma visita por endereço da cadeia (a
//! primeira com `0x18000001`, digitada com o início da cadeia e o bit da saída direta; a última com `0xA8000001`, o
//! redirecionamento de servidor e o fim da cadeia), só a última com `context_annotations` e só a primeira com segmento;
//! o `Session_` leva o endereço final e, em `original_request_url`, o pedido.

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::sys;
use sysabi::*;

mod support;

const PAGE: &str = include_str!("../../../testbench/corpus/headless-storage/ss-none.html");

fn redirect_client(ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    let s = sys::current();
    let (listener, _) = s.tcp_listen_at(std::net::Ipv4Addr::LOCALHOST.into(), 8000, 16, false, true).unwrap();
    let server = s.spawn_fn(ProcAttrs::default(), b"server".to_vec(), Box::new(move || support::serve(listener, PAGE))).unwrap();
    let argv: Vec<Vec<u8>> = ["chromium", "--headless", "--no-sandbox", "--disable-gpu", "--user-data-dir=/tmp/p", "--virtual-time-budget=20000", "--dump-dom", "http://127.0.0.1:8000/redir"]
        .iter()
        .map(|a| a.as_bytes().to_vec())
        .collect();
    let pid = s.spawn(SpawnSpec { path: b"/usr/bin/chromium".to_vec(), argv, attrs: ProcAttrs::default() }).unwrap();
    let _ = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty());
    let query = "select * from urls; select * from visits; select * from segments; select * from content_annotations; select * from context_annotations";
    let argv: Vec<Vec<u8>> = ["sqlite3", "/tmp/p/Default/History", query].iter().map(|a| a.as_bytes().to_vec()).collect();
    let pid = s.spawn(SpawnSpec { path: b"/usr/bin/sqlite3".to_vec(), argv, attrs: ProcAttrs::default() }).unwrap();
    let _ = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty());
    // O `Session_`: o tamanho do oráculo (2042) e as ocorrências dos endereços (o pedido é prefixo do final: duas).
    let found = sys::read_dir(b"/tmp/p/Default/Sessions").unwrap_or_default().into_iter().find(|e| e.name.starts_with(b"Session_"));
    let session = found.and_then(|e| sys::read_file(&[b"/tmp/p/Default/Sessions/".as_slice(), &e.name].concat()).ok()).unwrap_or_default();
    let count = |needle: &[u8]| session.windows(needle.len()).filter(|window| *window == needle).count();
    let _ = ctx.stdout().write_all(format!("session {} final {} requested {}\n", session.len(), count(b"http://127.0.0.1:8000/redir/"), count(b"http://127.0.0.1:8000/redir")).as_bytes());
    let _ = s.kill(KillTarget::Pid(server), Signal::SIGKILL);
    let _ = s.wait4(WaitTarget::Pid(server), WaitOptions::empty());
    0
}

/// Os instantes (14 dígitos ou mais), a duração da última visita e o primeiro plano viram `T`: o resto é a forma.
fn mask(out: &str) -> String {
    out.lines()
        .map(|line| {
            let mut fields: Vec<String> = line.split('|').map(|field| if field.len() >= 14 && field.bytes().all(|b| b.is_ascii_digit()) { "T".to_owned() } else { field.to_owned() }).collect();
            if fields.len() == 18 && fields[0] == "2" {
                fields[7] = "D".to_owned();
            }
            if fields.len() == 12 {
                fields[4] = "F".to_owned();
                fields[6] = "T".to_owned();
                fields[7] = "T".to_owned();
            }
            fields.join("|")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_server_redirect_gives_the_oracles_chain_in_the_dump() {
    let kernel = Kernel::new(KernelConfig::default());
    let mut programs = ul_chromium::programs();
    programs.extend(ul_sqlite::programs());
    programs.push(Program::bin("redirect_client", redirect_client));
    let sandbox = kernel.create_sandbox(SandboxConfig { programs, ..SandboxConfig::default() }).unwrap();
    let output = sandbox.run(RunRequest { argv: vec![b"redirect_client".to_vec()], timeout: Some(Duration::from_secs(120)), ..RunRequest::default() }).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let expected = "1|http://127.0.0.1:8000/redir|ss none|1|1|T|0\n\
        2|http://127.0.0.1:8000/redir/|ss none|1|0|T|0\n\
        1|1|T|0||402653185|1|0|1|0||0|0|0|0|1|0|\n\
        2|2|T|1||-1476395007|0|D|0|0||0|0|0|0|1|0|\n\
        1|http://127.0.0.1/redir|1\n\
        2|-1|||-1|0|||||||1|0\n\
        2|0|-1000000|6|F|1|T|T|T|T|-1|200\n\
        session 2042 final 1 requested 2";
    // Depois do DOM que o `--dump-dom` imprime vêm as linhas do History e a do `Session_`.
    let rows = stdout.split_once("</html>\n").map_or(stdout.as_str(), |(_, rest)| rest);
    assert_eq!(mask(rows), expected, "{stdout}");
}
