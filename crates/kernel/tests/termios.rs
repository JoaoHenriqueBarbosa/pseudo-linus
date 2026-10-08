//! Testes de integração de `tcflush(3)` (`TCFLSH`) e `tcflow(3)` (`TCXONC`) sobre pseudoterminais. Os
//! resultados esperados são os do Linux 6.12 (`n_tty_ioctl_helper`, `__tty_perform_flush`, o controle de
//! fluxo do `n_tty` com IXON e `tty_send_xchar`): fila ou ação desconhecida é EINVAL, fd que não é
//! terminal é ENOTTY, STOP e START digitados são consumidos pela disciplina de linha e o `TCOOFF` só se
//! desfaz com `TCOON`.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::sys::{self, write_all};
use sysabi::termios::*;
use sysabi::*;

fn out(text: String) {
    write_all(Fd::STDOUT, format!("{text}\n").as_bytes()).unwrap();
}

/// `ok` ou o nome do errno.
fn verdict<T>(r: SysResult<T>) -> String {
    match r {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("{e:?}"),
    }
}

/// O mestre (sem espera) e o escravo (sem espera) de um pty novo.
fn pair() -> (Fd, Fd) {
    let s = sys::current();
    let master = s.openat(Fd::CWD, b"/dev/ptmx", OFlags::RDWR | OFlags::NOCTTY | OFlags::NONBLOCK, 0).unwrap();
    s.pty_set_lock(master, false).unwrap();
    let path = format!("/dev/pts/{}", s.pty_number(master).unwrap());
    let slave = s.openat(Fd::CWD, path.as_bytes(), OFlags::RDWR | OFlags::NOCTTY | OFlags::NONBLOCK, 0).unwrap();
    (master, slave)
}

/// O que `fd` devolve numa leitura sem espera: os bytes entre aspas ou o errno.
fn peek(fd: Fd) -> String {
    let mut buf = [0u8; 64];
    match sys::current().read(fd, &mut buf) {
        Ok(n) => format!("{:?}", String::from_utf8_lossy(&buf[..n])),
        Err(e) => format!("{e:?}"),
    }
}

/// Escreve `data` sem esperar e diz o resultado (`ok` ou o errno).
fn put(fd: Fd, data: &[u8]) -> String {
    verdict(sys::current().write(fd, data))
}

fn scenario_flush_input() -> String {
    let s = sys::current();
    let (m, sl) = pair();
    // Uma linha completa e uma metade de linha: o eco prova que a disciplina de linha já as viu.
    put(m, b"ab\n");
    let echo = peek(m);
    let flush_slave = verdict(s.tcflush(sl, TCIFLUSH));
    let after = peek(sl);
    put(m, b"xy");
    let echo_half = peek(m);
    s.tcflush(sl, TCIOFLUSH).unwrap();
    put(m, b"z\n");
    let echo_rest = peek(m);
    let line = peek(sl);
    // No mestre, a entrada é o que o escravo escreveu e ninguém leu.
    put(sl, b"hello");
    let flush_master = verdict(s.tcflush(m, TCIFLUSH));
    let master_after = peek(m);
    format!("{echo} {flush_slave} {after} {echo_half} {echo_rest} {line} {flush_master} {master_after}")
}

fn scenario_flush_output() -> String {
    let s = sys::current();
    let (m, sl) = pair();
    // A saída já entregue ao mestre fica: `TCOFLUSH` não a alcança nos dois lados.
    put(sl, b"keep");
    let on_slave = verdict(s.tcflush(sl, TCOFLUSH));
    let on_master = verdict(s.tcflush(m, TCOFLUSH));
    let got = peek(m);
    // A entrada do escravo fica: só `TCIFLUSH` e `TCIOFLUSH` a descartam.
    put(m, b"in\n");
    let echo = peek(m);
    s.tcflush(sl, TCOFLUSH).unwrap();
    format!("{on_slave} {on_master} {got} {echo} {}", peek(sl))
}

fn scenario_flush_errors() -> String {
    let s = sys::current();
    let (m, sl) = pair();
    let (r, w) = s.pipe2(OFlags::empty()).unwrap();
    let r = [
        verdict(s.tcflush(sl, 3)),
        verdict(s.tcflush(m, -1)),
        verdict(s.tcflush(sl, i32::MAX)),
        verdict(s.tcflush(r, TCIFLUSH)),
        verdict(s.tcflush(w, 9)),
        verdict(s.tcflush(Fd(99), TCIFLUSH)),
        verdict(s.tcflush(Fd::STDIN, 7)),
    ];
    r.join(" ")
}

fn scenario_flow_output() -> String {
    let s = sys::current();
    let (m, sl) = pair();
    // O escravo suspenso não escreve; o mestre continua lendo o que já existe.
    let off = verdict(s.tcflow(sl, TCOOFF));
    let blocked = put(sl, b"a");
    let again = verdict(s.tcflow(sl, TCOOFF));
    let still = put(sl, b"a");
    let on = verdict(s.tcflow(sl, TCOON));
    let resumed = put(sl, b"b");
    let got = peek(m);
    // `TCOON` sem `TCOOFF` antes não faz nada.
    let idle = verdict(s.tcflow(sl, TCOON));
    // O mestre também suspende a própria saída (o que ele escreve ao escravo).
    let m_off = verdict(s.tcflow(m, TCOOFF));
    let m_blocked = put(m, b"c");
    s.tcflow(m, TCOON).unwrap();
    let m_resumed = put(m, b"d");
    format!("{off} {blocked} {again} {still} {on} {resumed} {got} {idle} {m_off} {m_blocked} {m_resumed}")
}

fn scenario_flow_send() -> String {
    let s = sys::current();
    let (m, sl) = pair();
    // O escravo manda STOP e START ao que o mestre lê, sem OPOST nem eco.
    let ioff = verdict(s.tcflow(sl, TCIOFF));
    let stop = peek(m);
    let ion = verdict(s.tcflow(sl, TCION));
    let start = peek(m);
    // Sem STOP configurado (`_POSIX_VDISABLE`) não se escreve nada.
    let mut t = s.tcgetattr(sl).unwrap();
    t.c_cc[VSTOP] = 0;
    s.tcsetattr(sl, SetAttrWhen::Now, &t).unwrap();
    let disabled = verdict(s.tcflow(sl, TCIOFF));
    let nothing = peek(m);
    t.c_cc[VSTOP] = 0x13;
    s.tcsetattr(sl, SetAttrWhen::Now, &t).unwrap();
    // O mestre manda o STOP ao escravo: com IXON ele é consumido (nem entrada nem eco) e suspende a saída.
    let m_ioff = verdict(s.tcflow(m, TCIOFF));
    let swallowed = peek(sl);
    let echo = peek(m);
    let stopped = put(sl, b"x");
    let m_ion = verdict(s.tcflow(m, TCION));
    let started = put(sl, b"y");
    format!("{ioff} {stop} {ion} {start} {disabled} {nothing} {m_ioff} {swallowed} {echo} {stopped} {m_ion} {started} {}", peek(m))
}

fn scenario_flow_chars() -> String {
    let s = sys::current();
    let (m, sl) = pair();
    // STOP e START digitados no mestre param e retomam a saída do escravo.
    put(m, &[0x13]);
    let stopped = put(sl, b"a");
    put(m, &[0x11]);
    let started = put(sl, b"a");
    peek(m);
    // O START do teclado não desfaz o `TCOOFF`.
    s.tcflow(sl, TCOOFF).unwrap();
    put(m, &[0x11]);
    let tco = put(sl, b"b");
    s.tcflow(sl, TCOON).unwrap();
    let resumed = put(sl, b"b");
    peek(m);
    // Com IXANY qualquer caractere retoma; sem IXON o STOP é um caractere comum.
    let mut t = s.tcgetattr(sl).unwrap();
    t.c_iflag |= IXANY;
    s.tcsetattr(sl, SetAttrWhen::Now, &t).unwrap();
    put(m, &[0x13]);
    let any_stopped = put(sl, b"c");
    put(m, b"q");
    let any_resumed = put(sl, b"c");
    peek(m);
    s.tcflush(sl, TCIFLUSH).unwrap();
    t.c_iflag &= !(IXON | IXANY);
    s.tcsetattr(sl, SetAttrWhen::Now, &t).unwrap();
    put(m, &[0x13, b'\n']);
    let plain = peek(sl);
    // Desligar o IXON com a saída parada pelo STOP a religa.
    t.c_iflag |= IXON;
    s.tcsetattr(sl, SetAttrWhen::Now, &t).unwrap();
    put(m, &[0x13]);
    let again = put(sl, b"d");
    t.c_iflag &= !IXON;
    s.tcsetattr(sl, SetAttrWhen::Now, &t).unwrap();
    let freed = put(sl, b"d");
    format!("{stopped} {started} {tco} {resumed} {any_stopped} {any_resumed} {plain} {again} {freed}")
}

fn scenario_flow_errors() -> String {
    let s = sys::current();
    let (m, sl) = pair();
    let (r, w) = s.pipe2(OFlags::empty()).unwrap();
    let r = [
        verdict(s.tcflow(sl, 4)),
        verdict(s.tcflow(m, -1)),
        verdict(s.tcflow(sl, i32::MIN)),
        verdict(s.tcflow(r, TCOON)),
        verdict(s.tcflow(w, 9)),
        verdict(s.tcflow(Fd(99), TCOON)),
        verdict(s.tcflow(Fd::STDIN, 7)),
    ];
    r.join(" ")
}

fn p_termios(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let name = String::from_utf8_lossy(args[1].as_bytes()).into_owned();
    let text = match name.as_str() {
        "flush_input" => scenario_flush_input(),
        "flush_output" => scenario_flush_output(),
        "flush_errors" => scenario_flush_errors(),
        "flow_output" => scenario_flow_output(),
        "flow_send" => scenario_flow_send(),
        "flow_chars" => scenario_flow_chars(),
        "flow_errors" => scenario_flow_errors(),
        _ => return 2,
    };
    out(text);
    0
}

fn scenario(name: &str) -> String {
    let k = Kernel::new(KernelConfig::default());
    let sb = k.create_sandbox(SandboxConfig { programs: vec![Program::bin("termios_scenario", p_termios)], ..SandboxConfig::default() }).unwrap();
    let argv = ["termios_scenario", name].iter().map(|s| s.as_bytes().to_vec()).collect();
    let r = sb.run(RunRequest { argv, timeout: Some(Duration::from_secs(60)), ..RunRequest::default() }).unwrap();
    assert_eq!(r.status, WaitStatus::Exited(0), "{name}: {}", String::from_utf8_lossy(&r.stderr));
    String::from_utf8_lossy(&r.stdout).trim_end().to_string()
}

#[test]
fn flush_discards_the_input_of_the_terminal_it_is_called_on() {
    assert_eq!(scenario("flush_input"), r#""ab\r\n" ok EAGAIN "xy" "z\r\n" "z\n" ok EAGAIN"#);
}

#[test]
fn flush_output_keeps_what_was_already_delivered() {
    assert_eq!(scenario("flush_output"), r#"ok ok "keep" "in\r\n" "in\n""#);
}

#[test]
fn flush_rejects_unknown_queues_and_non_terminals() {
    assert_eq!(scenario("flush_errors"), "EINVAL EINVAL EINVAL ENOTTY ENOTTY EBADF ENOTTY");
}

#[test]
fn flow_off_suspends_output_until_on() {
    assert_eq!(scenario("flow_output"), r#"ok EAGAIN ok EAGAIN ok ok "b" ok ok EAGAIN ok"#);
}

#[test]
fn flow_ioff_and_ion_send_the_stop_and_start_characters() {
    assert_eq!(scenario("flow_send"), r#"ok "\u{13}" ok "\u{11}" ok EAGAIN ok EAGAIN EAGAIN EAGAIN ok ok "y""#);
}

#[test]
fn typed_stop_and_start_are_consumed_by_ixon() {
    assert_eq!(scenario("flow_chars"), r#"EAGAIN ok EAGAIN ok EAGAIN ok "\u{13}\n" EAGAIN ok"#);
}

#[test]
fn flow_rejects_unknown_actions_and_non_terminals() {
    assert_eq!(scenario("flow_errors"), "EINVAL EINVAL EINVAL ENOTTY ENOTTY EBADF ENOTTY");
}
