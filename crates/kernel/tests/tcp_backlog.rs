//! Testes de integração da fila de aceite do TCP de loopback. Os resultados esperados são os do Linux 6.12
//! (`tcp_conn_request` e `sk_acceptq_is_full` em `net/ipv4/tcp_input.c` e `include/net/sock.h`,
//! `tcp_v4_connect` e `tcp_retransmit_timer` em `net/ipv4/tcp_ipv4.c` e `tcp_timer.c`):
//!
//! - a fila aceita `backlog + 1` conexões completas (`sk_ack_backlog > sk_max_ack_backlog` é cheia), então
//!   `listen(fd, 0)` aceita uma;
//! - com ela cheia o SYN é descartado, sem RST: o cliente fica em SYN_SENT e retransmite em 1, 3, 7, 15, 31 e
//!   63 s (RTO inicial de 1 s que dobra), e o `connect` bloqueante espera, não recebe ECONNREFUSED;
//! - o `connect` não bloqueante dá EINPROGRESS e o socket só ganha `POLLOUT` quando uma retransmissão acha vaga
//!   (liberar vaga com `accept` não adianta o SYN: ele só sai no próximo instante do RTO);
//! - porta sem ouvinte recusa na hora.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::time::{Duration, Instant};

use kernel::{Kernel, KernelConfig, RunRequest, Sandbox, SandboxConfig};
use sysabi::sys::{self, write_all};
use sysabi::*;

fn out(text: String) {
    write_all(Fd::STDOUT, format!("{text}\n").as_bytes()).unwrap();
}

/// `ok` ou o nome do errno, pra comparar uma sequência de resultados numa linha só.
fn verdict<T>(r: SysResult<T>) -> String {
    match r {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("{e:?}"),
    }
}

fn lo() -> std::net::IpAddr {
    std::net::Ipv4Addr::LOCALHOST.into()
}

/// Um cliente não bloqueante que já chamou `connect`: o fd e o que o `connect` devolveu.
fn nb_connect(port: u16) -> (Fd, String) {
    let s = sys::current();
    let fd = s.tcp_socket(false, true, true).unwrap();
    let r = verdict(s.tcp_connect_fd(fd, lo(), port));
    (fd, r)
}

/// Os eventos de um `poll` em `POLLOUT`, esperando até `wait`.
fn polled(fd: Fd, wait: Duration) -> PollEvents {
    let mut p = [PollFd { fd, events: PollEvents::OUT, revents: PollEvents::empty() }];
    sys::current().poll(&mut p, Some(wait)).unwrap();
    p[0].revents
}

/// O cliente já está conectado (`POLLOUT` sem esperar).
fn established(fd: Fd) -> u8 {
    u8::from(polled(fd, Duration::ZERO).contains(PollEvents::OUT))
}

fn in_ms(started: Instant, lo: u64, hi: u64) -> bool {
    (lo..hi).contains(&(started.elapsed().as_millis() as u64))
}

/// As linhas do `/proc/net/tcp` sem o cabeçalho, cada uma em campos.
fn tcp_table() -> Vec<Vec<String>> {
    let s = sys::current();
    let fd = sys::open(b"/proc/net/tcp", OFlags::empty(), 0).unwrap();
    let mut text = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = s.read(fd, &mut buf).unwrap();
        if n == 0 {
            break;
        }
        text.extend_from_slice(&buf[..n]);
    }
    s.close(fd).unwrap();
    String::from_utf8(text).unwrap().lines().skip(1).map(|l| l.split_whitespace().map(str::to_string).collect()).collect()
}

/// `st tx:rx retrnsmt tr rto cwnd ssthresh` da linha de estado `state` que envolve a porta `port` (a local do
/// ouvinte, a remota do cliente em SYN_SENT), e o `tm->when` em 1/100 s.
fn row_of(state: &str, port: u16) -> (String, u64) {
    let rows = tcp_table();
    let suffix = format!(":{port:04X}");
    let r = rows
        .iter()
        .find(|r| r[3] == state && (r[1].ends_with(&suffix) || r[2].ends_with(&suffix)))
        .unwrap_or_else(|| panic!("sem linha {state}: {rows:?}"));
    let (timer, when) = r[5].split_once(':').unwrap();
    (format!("{} {} {} {timer} {} {} {}", r[3], r[4], r[6], r[12], r[15], r[16]), u64::from_str_radix(when, 16).unwrap())
}

fn p_scenario(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let s = sys::current();
    let name = String::from_utf8_lossy(args[1].as_bytes()).into_owned();
    match name.as_str() {
        // Com `backlog` N a fila completa aceita N + 1 conexões; as seguintes ficam em SYN_SENT (sem `POLLOUT`).
        "fill" => {
            let backlog: u32 = String::from_utf8_lossy(args[2].as_bytes()).parse().unwrap();
            let (_l, port) = s.tcp_listen(0, backlog, false, true).unwrap();
            let row: Vec<String> = (0..backlog + 3)
                .map(|_| {
                    let (fd, r) = nb_connect(port);
                    format!("{r}:{}", established(fd))
                })
                .collect();
            out(row.join(" "));
            0
        }
        // O `accept` não adianta o SYN: o cliente só acha vaga na retransmissão de 1 s (`POLLOUT`), e até lá o
        // segundo `connect` dá EALREADY. Depois o `connect` conclui com 0 e o seguinte dá EISCONN.
        "retransmit" => {
            let (l, port) = s.tcp_listen(0, 0, false, true).unwrap();
            let (_c1, _) = nb_connect(port);
            let (c2, first) = nb_connect(port);
            let again = verdict(s.tcp_connect_fd(c2, lo(), port));
            s.tcp_accept(l, false, true).unwrap();
            let early = established(c2);
            let started = Instant::now();
            let late = polled(c2, Duration::from_secs(3)).contains(PollEvents::OUT);
            let on_time = in_ms(started, 800, 1500);
            let done = verdict(s.tcp_connect_fd(c2, lo(), port));
            let err = s.sock_error(c2).unwrap();
            let isconn = verdict(s.tcp_connect_fd(c2, lo(), port));
            s.tcp_accept(l, false, true).unwrap();
            out(format!("{first} {again} {early} {late} {on_time} {done} {err} {isconn}"));
            0
        }
        // `connect` bloqueante com a fila cheia espera; o ouvinte aceita aos 300 ms e a retransmissão de 1 s entra.
        "blocking" => {
            let (l, port) = s.tcp_listen(0, 0, false, true).unwrap();
            let (_c1, _) = nb_connect(port);
            let acceptor = s
                .spawn_thread(Box::new(move || {
                    let s = sys::current();
                    s.nanosleep(Duration::from_millis(300)).unwrap();
                    s.tcp_accept(l, false, true).unwrap();
                }))
                .unwrap();
            let c = s.tcp_socket(false, false, true).unwrap();
            let started = Instant::now();
            let r = verdict(s.tcp_connect_fd(c, lo(), port));
            let on_time = in_ms(started, 900, 1500);
            s.join_thread(acceptor).unwrap();
            out(format!("{r} {on_time} {}", verdict(s.tcp_accept(l, false, true))));
            0
        }
        // O ouvinte só aceita aos 1,5 s: a retransmissão de 1 s não acha vaga, o RTO dobra e a de 3 s entra.
        "backoff" => {
            let (l, port) = s.tcp_listen(0, 0, false, true).unwrap();
            let (_c1, _) = nb_connect(port);
            let acceptor = s
                .spawn_thread(Box::new(move || {
                    let s = sys::current();
                    s.nanosleep(Duration::from_millis(1500)).unwrap();
                    s.tcp_accept(l, false, true).unwrap();
                }))
                .unwrap();
            let c = s.tcp_socket(false, false, true).unwrap();
            let started = Instant::now();
            let r = verdict(s.tcp_connect_fd(c, lo(), port));
            let on_time = in_ms(started, 2900, 3700);
            s.join_thread(acceptor).unwrap();
            out(format!("{r} {on_time}"));
            0
        }
        // Porta sem ouvinte: ECONNREFUSED na hora, no bloqueante.
        "refused" => {
            let (l, port) = s.tcp_listen(0, 0, false, true).unwrap();
            s.close(l).unwrap();
            let c = s.tcp_socket(false, false, true).unwrap();
            let started = Instant::now();
            let r = verdict(s.tcp_connect_fd(c, lo(), port));
            out(format!("{r} {}", in_ms(started, 0, 300)));
            0
        }
        // `listen` de novo num socket que já escuta só ajusta o backlog: com 2 a fila passa a aceitar 3.
        "relisten" => {
            let (l, port) = s.tcp_listen(0, 0, false, true).unwrap();
            let (c1, _) = nb_connect(port);
            let (c2, _) = nb_connect(port);
            s.tcp_listen_bound(l, 2).unwrap();
            let rest: Vec<Fd> = (0..3).map(|_| nb_connect(port).0).collect();
            let row: Vec<String> = [c1, c2].iter().chain(&rest).map(|&fd| established(fd).to_string()).collect();
            out(row.join(" "));
            0
        }
        // O `/proc/net/tcp` mostra o ouvinte com a fila cheia (`rx_queue` 1) e o cliente em SYN_SENT: `tx_queue`
        // 1 (o SYN), timer de retransmissão (1) com o tempo que falta, `retrnsmt` que sobe a cada SYN e o RTO que
        // dobra (em 1/100 s).
        "proc" => {
            let (_l, port) = s.tcp_listen(0, 0, false, true).unwrap();
            let (_c1, _) = nb_connect(port);
            let (_c2, _) = nb_connect(port);
            let (listen, _) = row_of("0A", port);
            let (syn, when) = row_of("02", port);
            s.nanosleep(Duration::from_millis(1100)).unwrap();
            let (syn2, when2) = row_of("02", port);
            out(format!("{listen} | {syn} {} | {syn2} {}", (90..=100).contains(&when), (170..=190).contains(&when2)));
            0
        }
        // O ouvinte fecha enquanto o cliente retransmite: o SYN seguinte recebe RST e o `connect` falha com
        // ECONNREFUSED (poll com `ERR`, `SO_ERROR` lido uma vez).
        "listener-closes" => {
            let (l, port) = s.tcp_listen(0, 0, false, true).unwrap();
            let (_c1, _) = nb_connect(port);
            let (c2, _) = nb_connect(port);
            s.close(l).unwrap();
            let ev = polled(c2, Duration::from_secs(3));
            let first = s.sock_error(c2).unwrap() == Errno::ECONNREFUSED.0;
            let second = s.sock_error(c2).unwrap();
            out(format!("{} {first} {second}", ev.contains(PollEvents::ERR | PollEvents::OUT)));
            0
        }
        _ => 2,
    }
}

fn sandbox() -> Sandbox {
    let k = Kernel::new(KernelConfig::default());
    k.create_sandbox(SandboxConfig { programs: vec![Program::bin("backlog_scenario", p_scenario)], ..SandboxConfig::default() }).unwrap()
}

fn scenario(args: &[&str]) -> String {
    let sb = sandbox();
    let mut full = vec!["backlog_scenario"];
    full.extend_from_slice(args);
    let argv = full.iter().map(|s| s.as_bytes().to_vec()).collect();
    let r = sb.run(RunRequest { argv, timeout: Some(Duration::from_secs(30)), ..RunRequest::default() }).unwrap();
    assert_eq!(r.status, WaitStatus::Exited(0), "{args:?}: {}", String::from_utf8_lossy(&r.stderr));
    String::from_utf8_lossy(&r.stdout).trim_end().to_string()
}

#[test]
fn backlog_zero_accepts_one_connection() {
    assert_eq!(scenario(&["fill", "0"]), "EINPROGRESS:1 EINPROGRESS:0 EINPROGRESS:0");
}

#[test]
fn backlog_one_accepts_two_connections() {
    assert_eq!(scenario(&["fill", "1"]), "EINPROGRESS:1 EINPROGRESS:1 EINPROGRESS:0 EINPROGRESS:0");
}

#[test]
fn backlog_two_accepts_three_connections() {
    assert_eq!(scenario(&["fill", "2"]), "EINPROGRESS:1 EINPROGRESS:1 EINPROGRESS:1 EINPROGRESS:0 EINPROGRESS:0");
}

#[test]
fn nonblocking_connect_is_einprogress_then_pollout_on_the_retransmission() {
    assert_eq!(scenario(&["retransmit"]), "EINPROGRESS EALREADY 0 true true ok 0 EISCONN");
}

#[test]
fn blocking_connect_waits_for_the_first_retransmission_and_is_accepted() {
    assert_eq!(scenario(&["blocking"]), "ok true ok");
}

#[test]
fn retransmission_interval_doubles() {
    assert_eq!(scenario(&["backoff"]), "ok true");
}

#[test]
fn connect_to_a_port_without_listener_is_refused_at_once() {
    assert_eq!(scenario(&["refused"]), "ECONNREFUSED true");
}

#[test]
fn listening_again_adjusts_the_backlog() {
    assert_eq!(scenario(&["relisten"]), "1 0 1 1 0");
}

#[test]
fn proc_net_tcp_shows_the_pending_syn_and_the_full_queue() {
    assert_eq!(
        scenario(&["proc"]),
        "0A 00000000:00000001 00000000 00 100 10 0 | 02 00000001:00000000 00000000 01 100 10 -1 true | 02 00000001:00000000 00000001 01 200 10 -1 true"
    );
}

#[test]
fn listener_closing_refuses_the_retransmitted_syn() {
    assert_eq!(scenario(&["listener-closes"]), "true true 0");
}
