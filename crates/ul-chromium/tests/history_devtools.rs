//! O History do `chromium --remote-debugging-port` com uma URL na linha de comando, contra o Chromium 154 do oráculo
//! (`testbench/oracle/measure-navigation.sh` com `MODO=term` e `MODO=close`, `ss-none.html`, 4 a 18 segundos):
//!
//! - o commit do navegador de pé sai 10 segundos depois da navegação: o SIGTERM aos 10 s deixa o banco sem a visita (e o
//!   journal quente do molde, 66176 bytes) e aos 11 s ou mais encontra a visita aberta (transição `0x30000001`, sem o bit
//!   a mais do `--dump-dom`, duração 0, motivo de fim 0 e primeiro plano `-1000000`), com o journal de zero byte;
//! - o `Browser.close` grava a visita fechada na hora (duração e primeiro plano medidos, motivo 6), mesmo aos 4 s, e fecha
//!   a que o commit de 10 segundos deixou aberta.

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::sys;
use sysabi::*;

mod support;

const PAGE: &str = include_str!("../../../testbench/corpus/headless-storage/ss-none.html");
/// `history_client COMO SEGUNDOS`: sobe o navegador pela página, espera `SEGUNDOS` depois da porta abrir e o encerra por
/// `term` (SIGTERM) ou `close` (`Browser.close`); imprime o que o History e o journal têm.
fn history_client(ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let how = argv[1].to_string_lossy().into_owned();
    let seconds: f64 = argv[2].to_string_lossy().parse().unwrap();
    let s = sys::current();
    let server = support::run_devtools(PAGE, "ss-none.html", &how, seconds);
    let size = |path: &[u8]| sys::read_file(path).map_or(-1, |bytes| bytes.len() as i64);
    let _ = ctx.stdout().write_all(format!("journal {}\n", size(b"/tmp/h/Default/History-journal")).as_bytes());
    let query = "select * from urls; select * from visits; select * from segments; select * from segment_usage; select * from content_annotations; select * from context_annotations; select count(*) from visited_links";
    let args: Vec<Vec<u8>> = ["sqlite3", "/tmp/h/Default/History", query].iter().map(|a| a.as_bytes().to_vec()).collect();
    let pid = s.spawn(SpawnSpec { path: b"/usr/bin/sqlite3".to_vec(), argv: args, attrs: ProcAttrs::default() }).unwrap();
    let _ = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty());
    let _ = s.kill(KillTarget::Pid(server), Signal::SIGKILL);
    let _ = s.wait4(WaitTarget::Pid(server), WaitOptions::empty());
    0
}

fn run(how: &str, seconds: &str) -> String {
    let kernel = Kernel::new(KernelConfig::default());
    let mut programs = ul_chromium::programs();
    programs.extend(ul_sqlite::programs());
    programs.push(Program::bin("history_client", history_client));
    let sandbox = kernel.create_sandbox(SandboxConfig { programs, ..SandboxConfig::default() }).unwrap();
    let output = sandbox.run(RunRequest { argv: vec![b"history_client".to_vec(), how.as_bytes().to_vec(), seconds.as_bytes().to_vec()], timeout: Some(Duration::from_secs(120)), ..RunRequest::default() }).unwrap();
    String::from_utf8(output.stdout).unwrap()
}

/// Os instantes (textos só de dígitos com 14 ou mais) e os ids sorteados da janela e da aba (campos 7 e 8 da linha de
/// `context_annotations`, a única com doze campos) viram `T`: o que resta é a forma da linha.
fn mask(out: &str) -> String {
    out.lines()
        .map(|line| {
            let mut fields: Vec<String> = line.split('|').map(|field| if field.len() >= 14 && field.bytes().all(|b| b.is_ascii_digit()) { "T".to_owned() } else { field.to_owned() }).collect();
            if fields.len() == 12 {
                fields[6] = "T".to_owned();
                fields[7] = "T".to_owned();
            }
            fields.join("|")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn sigterm_before_the_ten_second_commit_leaves_the_hot_journal_and_no_visit() {
    let out = run("term", "3");
    assert!(out.starts_with("journal 66176\n"), "{out}");
    assert!(!out.contains("ss-none"), "{out}");
}

/// A visita aberta que o commit de 10 segundos grava, linha a linha como a do oráculo (`ss-none.html`, SIGTERM aos 11 s
/// ou mais): o que muda a cada execução (instantes, ids da janela e da aba, o dia do `segment_usage`) vira `T`.
#[test]
fn sigterm_after_the_ten_second_commit_finds_the_open_visit() {
    let out = run("term", "12");
    let expected = "journal 0\n\
        1|http://127.0.0.1:8000/ss-none.html|ss none|1|1|T|0\n\
        1|1|T|0||805306369|1|0|1|0||0|0|0|0|1|0|\n\
        1|http://127.0.0.1/ss-none.html|1\n\
        1|1|T|1\n\
        1|-1|||-1|0|||||||1|0\n\
        1|0|-1000000|0|-1000000|1|T|T|T|T|-1|200\n\
        0";
    assert_eq!(mask(&out), expected, "{out}");
}

/// A visita fechada do `Browser.close`: a mesma linha do commit de 10 segundos, com a duração (`visit_duration`), o
/// motivo 6 e o primeiro plano medidos. Devolve (duração, primeiro plano) depois de conferir o resto da forma.
fn closed_visit(out: &str) -> (i64, i64) {
    let masked = mask(out);
    let lines: Vec<&str> = masked.lines().collect();
    assert_eq!(lines[0], "journal 0", "{out}");
    assert_eq!(lines[1], "1|http://127.0.0.1:8000/ss-none.html|ss none|1|1|T|0", "{out}");
    let visit: Vec<&str> = lines[2].split('|').collect();
    assert_eq!((visit[0], visit[1], visit[2], visit[3], visit[4], visit[5], visit[6]), ("1", "1", "T", "0", "", "805306369", "1"), "{out}");
    assert_eq!(&visit[8..], ["1", "0", "", "0", "0", "0", "0", "1", "0", ""], "{out}");
    assert_eq!(&lines[3..6], ["1|http://127.0.0.1/ss-none.html|1", "1|1|T|1", "1|-1|||-1|0|||||||1|0"], "{out}");
    let context: Vec<&str> = lines[6].split('|').collect();
    assert_eq!((context[0], context[1], context[2], context[3]), ("1", "0", "-1000000", "6"), "{out}");
    assert_eq!((context[5], context[6], context[7], context[8], context[9], context[10], context[11]), ("1", "T", "T", "T", "T", "-1", "200"), "{out}");
    assert_eq!(lines[7], "0", "{out}");
    (visit[7].parse().unwrap(), context[4].parse().unwrap())
}

#[test]
fn browser_close_writes_the_closed_visit_at_once() {
    let (duration, foreground) = closed_visit(&run("close", "3"));
    // O primeiro plano conta desde o início do navegador, um pouco antes da navegação (no oráculo de 0,17 a 0,2 s).
    assert!(duration > 0 && foreground > duration && foreground - duration < 2_000_000, "{duration} {foreground}");
}

#[test]
fn browser_close_after_the_commit_closes_the_open_visit() {
    let (duration, foreground) = closed_visit(&run("close", "12"));
    assert!(duration > 11_000_000 && foreground > duration && foreground - duration < 2_000_000, "{duration} {foreground}");
}
