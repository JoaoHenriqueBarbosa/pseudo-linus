//! Os sinais do `chromium --remote-debugging-port` contra o Chromium 154 do oráculo: SIGTERM, SIGINT e SIGHUP encerram
//! como o `Browser.close` (saída 0 e o perfil da saída limpa, com `Default/Preferences` e `Default/Secure Preferences`,
//! que só a saída grava); SIGQUIT e SIGUSR1 matam o processo pela ação padrão (o shell vê 131 e 138); SIGPIPE é
//! ignorado. Medido com `kill -<sinal>` num `chromium --headless --user-data-dir=... --remote-debugging-port=9222`
//! depois de a porta abrir.

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::sys;
use sysabi::*;

const PORT: u16 = 9222;

fn pause(s: &dyn Syscalls, wait: Duration) {
    let mut none: [PollFd; 0] = [];
    let _ = s.poll(&mut none, Some(wait));
}

fn exists(path: &str) -> bool {
    sys::stat(path.as_bytes()).is_ok()
}

/// `signal_client SINAL`: sobe o navegador, espera a porta, manda o sinal e imprime o estado do processo e se o perfil
/// tem os arquivos da saída limpa. Com SIGPIPE, que não mata, manda depois um SIGTERM.
fn signal_client(ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let signal = Signal::parse(&argv[1].to_string_lossy()).unwrap();
    let s = sys::current();
    let argv: Vec<Vec<u8>> = ["chromium", "--headless", "--no-sandbox", "--disable-gpu", &format!("--remote-debugging-port={PORT}"), "--user-data-dir=/tmp/profile", "about:blank"]
        .iter()
        .map(|a| a.as_bytes().to_vec())
        .collect();
    let pid = s.spawn(SpawnSpec { path: b"/usr/bin/chromium".to_vec(), argv, attrs: ProcAttrs::default() }).unwrap();
    let lo: std::net::IpAddr = std::net::Ipv4Addr::LOCALHOST.into();
    let fd = (0..500)
        .find_map(|_| match s.tcp_connect_at(lo, PORT, false, true) {
            Ok((fd, _)) => Some(fd),
            Err(_) => {
                pause(&*s, Duration::from_millis(20));
                None
            }
        })
        .expect("o DevTools não abriu a porta");
    let _ = sys::close(fd);
    s.kill(KillTarget::Pid(pid), signal).unwrap();
    pause(&*s, Duration::from_millis(500));
    let mut alive = false;
    let status = match s.wait4(WaitTarget::Pid(pid), WaitOptions::NOHANG).unwrap() {
        Some((_, status)) => status,
        None => {
            alive = true;
            s.kill(KillTarget::Pid(pid), Signal::SIGTERM).unwrap();
            s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap().unwrap().1
        }
    };
    let line = format!("{} {} {:?} {} {}\n", signal.name().unwrap_or_default(), alive, status, exists("/tmp/profile/Default/Preferences"), exists("/tmp/profile/Default/Secure Preferences"));
    let _ = ctx.stdout().write_all(line.as_bytes());
    0
}

fn run(signal: &str) -> String {
    let kernel = Kernel::new(KernelConfig::default());
    let mut programs = ul_chromium::programs();
    programs.push(Program::bin("signal_client", signal_client));
    let sandbox = kernel.create_sandbox(SandboxConfig { programs, ..SandboxConfig::default() }).unwrap();
    let output = sandbox.run(RunRequest { argv: vec![b"signal_client".to_vec(), signal.as_bytes().to_vec()], timeout: Some(Duration::from_secs(60)), ..RunRequest::default() }).unwrap();
    assert_eq!(output.status, WaitStatus::Exited(0), "{signal}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn term_int_and_hup_close_the_browser_cleanly() {
    for signal in ["TERM", "INT", "HUP"] {
        assert_eq!(run(signal), format!("{signal} false Exited(0) true true\n"));
    }
}

#[test]
fn quit_and_usr1_kill_by_default_and_pipe_is_ignored() {
    assert_eq!(run("QUIT"), "QUIT false Signaled { signal: SIGQUIT, core_dumped: false } false false\n");
    assert_eq!(run("USR1"), "USR1 false Signaled { signal: SIGUSR1, core_dumped: false } false false\n");
    assert_eq!(run("PIPE"), "PIPE true Exited(0) true true\n");
}
