//! `Sandbox::spawn_pty`: o processo criado pelo host num terminal, com o mestre do pty no host. O
//! esperado é o que um processo vê num `docker exec -t` do Linux 6.12: stdin, stdout e stderr no
//! `/dev/pts/N`, líder de sessão e em primeiro plano no terminal de controle, eco e modo canônico da
//! N_TTY, SIGWINCH no `TIOCSWINSZ` do mestre, SIGINT no Ctrl-C e SIGHUP quando o mestre fecha.

use std::ffi::OsString;
use std::time::{Duration, Instant};

use kernel::{HostPty, Kernel, KernelConfig, ReadOutcome, Sandbox, SandboxConfig};
use sysabi::sys::{self, write_all};
use sysabi::*;

/// Diz o que vê do terminal e depois ecoa cada linha lida; `size` responde com a janela, um SIGWINCH
/// capturado é avisado, `quit` sai com 7.
fn p_tty_report(_ctx: &mut Ctx, _args: &[OsString]) -> i32 {
    let s = sys::current();
    let say = |text: String| write_all(Fd::STDOUT, format!("{text}\n").as_bytes()).unwrap();
    let size = || s.tcgetwinsize(Fd::STDIN).map(|w| format!("{}x{}", w.rows, w.cols)).unwrap();
    let pid = s.getpid();
    let ttys: Vec<String> =
        (0..3).map(|fd| String::from_utf8_lossy(&s.readlinkat(Fd::CWD, format!("/proc/self/fd/{fd}").as_bytes()).unwrap()).into_owned()).collect();
    say(format!(
        "tty={} leader={} foreground={} size={}",
        ttys.join(","),
        s.getsid(0).unwrap() == pid && s.getpgid(0).unwrap() == pid,
        s.tcgetpgrp(Fd::STDIN).unwrap() == pid,
        size()
    ));
    s.sigaction(Signal::SIGWINCH, SigDisposition::Catch).unwrap();
    let mut line = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        match s.read(Fd::STDIN, &mut byte) {
            // Fim do terminal. O aviso é uma syscall a mais: é nela que um SIGHUP pendente é entregue.
            Ok(0) => {
                let _ = write_all(Fd::STDOUT, b"eof\n");
                return 0;
            }
            Ok(_) if byte[0] != b'\n' => line.push(byte[0]),
            Ok(_) => {
                match line.as_slice() {
                    b"quit" => return 7,
                    b"size" => say(size()),
                    other => say(format!("got:{}", String::from_utf8_lossy(other))),
                }
                line.clear();
            }
            Err(Errno::EINTR) if s.take_caught_signals().contains(&Signal::SIGWINCH) => say(format!("winch:{}", size())),
            Err(e) => {
                say(format!("{e:?}"));
                return 1;
            }
        }
    }
}

fn sandbox() -> Sandbox {
    let k = Kernel::new(KernelConfig::default());
    k.create_sandbox(SandboxConfig { programs: vec![Program::bin("tty_report", p_tty_report)], ..SandboxConfig::default() }).unwrap()
}

fn spawn(sb: &Sandbox) -> (Pid, HostPty) {
    let spec = SpawnSpec { path: b"/usr/bin/tty_report".to_vec(), argv: vec![b"tty_report".to_vec()], attrs: ProcAttrs::default() };
    sb.spawn_pty(spec, Winsize { rows: 37, cols: 111, xpixel: 0, ypixel: 0 }).unwrap()
}

/// Lê do mestre até `want` aparecer; devolve tudo o que chegou.
fn read_until(pty: &HostPty, want: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut got = Vec::new();
    let mut buf = [0u8; 4096];
    while !String::from_utf8_lossy(&got).contains(want) {
        match pty.read(&mut buf, Some(deadline)) {
            ReadOutcome::Data(n) => got.extend_from_slice(&buf[..n]),
            end => panic!("{end:?} esperando {want:?}; chegou {:?}", String::from_utf8_lossy(&got)),
        }
    }
    String::from_utf8_lossy(&got).into_owned()
}

fn wait(sb: &Sandbox, pid: Pid) -> WaitStatus {
    sb.wait(pid, Some(Instant::now() + Duration::from_secs(30))).unwrap().expect("o processo não terminou").0
}

#[test]
fn the_process_gets_the_slave_as_stdio_and_controlling_terminal() {
    let sb = sandbox();
    let (pid, pty) = spawn(&sb);
    assert_eq!(read_until(&pty, "\n"), "tty=/dev/pts/0,/dev/pts/0,/dev/pts/0 leader=true foreground=true size=37x111\r\n");
    // O que o mestre escreve passa pela disciplina de linha: eco, e a linha completa chega ao processo.
    pty.write_all(b"abc\n").unwrap();
    assert_eq!(read_until(&pty, "got:abc\r\n"), "abc\r\ngot:abc\r\n");
    pty.write_all(b"quit\n").unwrap();
    assert_eq!(wait(&sb, pid), WaitStatus::Exited(7));
    // Sem escravo aberto, a leitura do mestre entrega o que sobrou (o eco) e acaba.
    assert_eq!(read_until(&pty, "quit\r\n"), "quit\r\n");
    assert_eq!(pty.read(&mut [0u8; 16], Some(Instant::now() + Duration::from_secs(30))), ReadOutcome::Eof);
}

#[test]
fn resizing_the_master_signals_the_foreground_group() {
    let sb = sandbox();
    let (pid, pty) = spawn(&sb);
    read_until(&pty, "\n");
    pty.set_winsize(Winsize { rows: 50, cols: 132, xpixel: 0, ypixel: 0 });
    assert_eq!(read_until(&pty, "\n"), "winch:50x132\r\n");
    // O mesmo tamanho de novo não é mudança: sem sinal, e a pergunta seguinte responde sozinha.
    pty.set_winsize(Winsize { rows: 50, cols: 132, xpixel: 0, ypixel: 0 });
    pty.write_all(b"size\n").unwrap();
    assert_eq!(read_until(&pty, "50x132\r\n"), "size\r\n50x132\r\n");
    pty.write_all(b"quit\n").unwrap();
    assert_eq!(wait(&sb, pid), WaitStatus::Exited(7));
}

#[test]
fn the_interrupt_character_kills_the_foreground_group() {
    let sb = sandbox();
    let (pid, pty) = spawn(&sb);
    read_until(&pty, "\n");
    pty.write_all(b"\x03").unwrap();
    assert_eq!(wait(&sb, pid), WaitStatus::Signaled { signal: Signal::SIGINT, core_dumped: false });
    assert_eq!(read_until(&pty, "^C"), "^C");
}

#[test]
fn dropping_the_master_hangs_up_the_session_leader() {
    let sb = sandbox();
    let (pid, pty) = spawn(&sb);
    read_until(&pty, "\n");
    drop(pty);
    assert_eq!(wait(&sb, pid), WaitStatus::Signaled { signal: Signal::SIGHUP, core_dumped: false });
    assert!(sb.fs().stat(b"/dev/pts/0").is_err(), "o nó do pty some com o mestre");
}
