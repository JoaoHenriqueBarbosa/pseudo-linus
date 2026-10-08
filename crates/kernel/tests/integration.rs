//! Testes de integração do kernel com programas de teste mínimos (só aqui): processos, pipes, sinais,
//! wait, exec, fds, /proc e /dev, snapshot.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::time::{Duration, Instant};

use kernel::{Kernel, KernelConfig, RunRequest, Sandbox, SandboxConfig, TreeEntry};
use sysabi::sys::{self, write_all};
use sysabi::*;

fn arg(a: &OsString) -> &[u8] {
    a.as_bytes()
}

fn p_echo(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let words: Vec<&[u8]> = args[1..].iter().map(arg).collect();
    let mut line = words.join(&b' ');
    line.push(b'\n');
    match write_all(Fd::STDOUT, &line) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

fn copy(from: Fd) -> Result<(), Errno> {
    let mut buf = vec![0u8; 8192];
    loop {
        let n = sys::read(from, &mut buf)?;
        if n == 0 {
            return Ok(());
        }
        write_all(Fd::STDOUT, &buf[..n])?;
    }
}

fn p_cat(ctx: &mut Ctx, args: &[OsString]) -> i32 {
    if args.len() == 1 {
        return if copy(Fd::STDIN).is_ok() { 0 } else { 1 };
    }
    let mut rc = 0;
    for a in &args[1..] {
        match sys::open(arg(a), OFlags::RDONLY, 0) {
            Ok(fd) => {
                if let Err(e) = copy(fd) {
                    ctx.error(format!("{}: {}", a.to_string_lossy(), e.message()));
                    rc = 1;
                }
                let _ = sys::close(fd);
            }
            Err(e) => {
                ctx.error(format!("{}: {}", a.to_string_lossy(), e.message()));
                rc = 1;
            }
        }
    }
    rc
}

fn p_yes(ctx: &mut Ctx, _args: &[OsString]) -> i32 {
    let buf = b"y\n".repeat(4096);
    loop {
        if let Err(e) = write_all(Fd::STDOUT, &buf) {
            ctx.error(format!("standard output: {}", e.message()));
            return 1;
        }
    }
}

/// `head -c N`.
fn p_head(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let n: usize = std::str::from_utf8(arg(&args[2])).unwrap().parse().unwrap();
    let mut left = n;
    let mut buf = vec![0u8; 4096];
    while left > 0 {
        let want = left.min(buf.len());
        let got = sys::read(Fd::STDIN, &mut buf[..want]).unwrap();
        if got == 0 {
            break;
        }
        write_all(Fd::STDOUT, &buf[..got]).unwrap();
        left -= got;
    }
    0
}

fn p_true(_ctx: &mut Ctx, _args: &[OsString]) -> i32 {
    0
}

fn p_false(_ctx: &mut Ctx, _args: &[OsString]) -> i32 {
    1
}

/// `sleep MS`
fn p_sleep(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let ms: u64 = std::str::from_utf8(arg(&args[1])).unwrap().parse().unwrap();
    match sys::current().nanosleep(Duration::from_millis(ms)) {
        Ok(()) => 0,
        Err(_) => 2,
    }
}

fn status_text(st: WaitStatus) -> String {
    match st {
        WaitStatus::Exited(c) => format!("exit {c}"),
        WaitStatus::Signaled { signal, .. } => format!("signal {}", signal.name().unwrap_or_default()),
        WaitStatus::Stopped(s) => format!("stopped {}", s.name().unwrap_or_default()),
        WaitStatus::Continued => "continued".into(),
    }
}

/// `pipeline cmd args | cmd args | ...`: um shell mínimo de pipeline. Escreve no stderr o status de cada
/// estágio e sai com o do último, como o bash.
fn p_pipeline(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let s = sys::current();
    let mut stages: Vec<Vec<Vec<u8>>> = vec![Vec::new()];
    for a in &args[1..] {
        if arg(a) == b"|" {
            stages.push(Vec::new());
        } else {
            stages.last_mut().unwrap().push(arg(a).to_vec());
        }
    }
    let n = stages.len();
    let mut pids = Vec::new();
    let mut prev_read: Option<Fd> = None;
    for (i, argv) in stages.iter().enumerate() {
        let next = if i + 1 < n { Some(s.pipe2(OFlags::CLOEXEC).unwrap()) } else { None };
        let mut acts = Vec::new();
        if let Some(r) = prev_read {
            acts.push(FdAction::Dup2 { from: r, to: Fd::STDIN });
        }
        if let Some((_, w)) = next {
            acts.push(FdAction::Dup2 { from: w, to: Fd::STDOUT });
        }
        let path = [b"/usr/bin/".as_slice(), &argv[0]].concat();
        let pid = s
            .spawn(SpawnSpec { path, argv: argv.clone(), attrs: ProcAttrs { fd_actions: acts, ..ProcAttrs::default() } })
            .unwrap();
        pids.push(pid);
        if let Some(r) = prev_read.take() {
            s.close(r).unwrap();
        }
        if let Some((r, w)) = next {
            s.close(w).unwrap();
            prev_read = Some(r);
        }
    }
    let mut last = 0;
    for (i, pid) in pids.iter().enumerate() {
        let (p, st) = s.wait4(WaitTarget::Pid(*pid), WaitOptions::empty()).unwrap().unwrap();
        assert_eq!(p, *pid);
        let line = format!("{}: {}\n", String::from_utf8_lossy(&stages[i][0]), status_text(st));
        write_all(Fd::STDERR, line.as_bytes()).unwrap();
        last = st.shell_status();
    }
    last
}

/// `SOCK_SEQPACKET` do Linux.
const SOCK_SEQPACKET: u8 = 5;

/// Um `socketpair(AF_UNIX, SOCK_SEQPACKET)`, bloqueante ou não.
fn seq_pair(nonblock: bool) -> (Fd, Fd) {
    sys::unix_socketpair(SOCK_SEQPACKET, nonblock, true).unwrap()
}

/// Os eventos de `poll` de um fd, sem esperar.
fn poll_now(fd: Fd, events: PollEvents) -> PollEvents {
    let mut p = [PollFd { fd, events, revents: PollEvents::empty() }];
    sys::current().poll(&mut p, Some(Duration::ZERO)).unwrap();
    p[0].revents
}

/// Um par de sockets TCP ligados pelo loopback: `(cliente, aceito)`.
fn tcp_pair() -> (Fd, Fd) {
    let s = sys::current();
    let (listener, port) = s.tcp_listen(0, 4, false, true).unwrap();
    let (client, _) = s.tcp_connect(port, false, true).unwrap();
    let (server, _) = s.tcp_accept(listener, false, true).unwrap();
    s.close(listener).unwrap();
    (client, server)
}

/// Um socket TCP em escuta no loopback, criado por `socket` + `bind` + `listen`.
fn tcp_listening() -> Fd {
    let s = sys::current();
    let (fd, _) = s.tcp_bind(std::net::Ipv4Addr::LOCALHOST.into(), 0, false, false, true).unwrap();
    s.tcp_listen_bound(fd, 4).unwrap();
    fd
}

/// Quantos SIGPIPE o processo recebeu desde a última chamada (o cenário os captura em vez de morrer).
fn sigpipes() -> usize {
    sys::current().take_caught_signals().iter().filter(|sig| **sig == Signal::SIGPIPE).count()
}

/// Testes de processo que precisam rodar dentro do sandbox; o nome escolhe o cenário.
fn p_scenario(ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let s = sys::current();
    let name = String::from_utf8_lossy(arg(&args[1])).into_owned();
    let out = |t: String| write_all(Fd::STDOUT, t.as_bytes()).unwrap();
    match name.as_str() {
        "zombie" => {
            let pid = s.spawn_fn(ProcAttrs::default(), b"child".to_vec(), Box::new(|| 3)).unwrap();
            // Espera virar zumbi sem colher.
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let st = s.list_processes().into_iter().find(|p| p.pid == pid).map(|p| p.state);
                if st == Some('Z') {
                    break;
                }
                assert!(Instant::now() < deadline, "filho não virou zumbi");
                s.nanosleep(Duration::from_millis(2)).unwrap();
            }
            let stat_ok = s.fstatat(Fd::CWD, format!("/proc/{pid}").as_bytes(), AtFlags::empty()).is_ok();
            let r = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap().unwrap();
            let again = s.wait4(WaitTarget::Any, WaitOptions::empty());
            let gone = s.list_processes().into_iter().all(|p| p.pid != pid);
            out(format!("{} {} {:?} {}\n", stat_ok, status_text(r.1), again, gone));
            0
        }
        "orphan" => {
            // O filho cria um neto e sai; o neto confere que foi adotado pelo init.
            let child = s
                .spawn_fn(
                    ProcAttrs::default(),
                    b"child".to_vec(),
                    Box::new(|| {
                        let s = sys::current();
                        let parent = s.getpid();
                        s.spawn_fn(
                            ProcAttrs::default(),
                            b"grandchild".to_vec(),
                            Box::new(move || {
                                let s = sys::current();
                                let first = s.getppid();
                                let deadline = Instant::now() + Duration::from_secs(5);
                                while s.getppid() != 1 && Instant::now() < deadline {
                                    s.nanosleep(Duration::from_millis(1)).unwrap();
                                }
                                // O pai pode já ter saído quando o neto olha pela primeira vez.
                                let msg = format!("{} {}\n", first == parent || first == 1, s.getppid());
                                let fd = s.openat(Fd::CWD, b"/tmp/orphan", OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, 0o644).unwrap();
                                write_all(fd, msg.as_bytes()).unwrap();
                                s.close(fd).unwrap();
                                0
                            }),
                        )
                        .unwrap();
                        0
                    }),
                )
                .unwrap();
            let (_, st) = s.wait4(WaitTarget::Pid(child), WaitOptions::empty()).unwrap().unwrap();
            out(format!("{}\n", status_text(st)));
            // O neto não é filho deste processo.
            let r = s.wait4(WaitTarget::Any, WaitOptions::NOHANG);
            out(format!("{r:?}\n"));
            0
        }
        "sigpipe-ignored" => {
            s.sigaction(Signal::SIGPIPE, SigDisposition::Ignore).unwrap();
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            s.close(r).unwrap();
            let e = s.write(w, b"x");
            out(format!("{e:?}\n"));
            0
        }
        "eintr" => {
            s.sigaction(Signal::SIGUSR1, SigDisposition::Catch).unwrap();
            let me = s.getpid();
            let _ = s
                .spawn_fn(
                    ProcAttrs::default(),
                    b"poker".to_vec(),
                    Box::new(move || {
                        let s = sys::current();
                        s.nanosleep(Duration::from_millis(30)).unwrap();
                        s.kill(KillTarget::Pid(me), Signal::SIGUSR1).unwrap();
                        s.nanosleep(Duration::from_millis(30)).unwrap();
                        0
                    }),
                )
                .unwrap();
            let (r, _w) = s.pipe2(OFlags::empty()).unwrap();
            let mut b = [0u8; 4];
            let e = s.read(r, &mut b);
            let caught = s.take_caught_signals();
            let w = s.wait4(WaitTarget::Any, WaitOptions::empty()).unwrap().unwrap();
            out(format!("{e:?} {caught:?} {}\n", status_text(w.1)));
            0
        }
        "exit-drop" => {
            struct Guard;
            impl Drop for Guard {
                fn drop(&mut self) {
                    let _ = write_all(Fd::STDOUT, b"drop ran\n");
                }
            }
            fn deep(n: u32) -> i32 {
                let _g = Guard;
                if n == 0 {
                    sys::exit(42);
                }
                deep(n - 1)
            }
            deep(2)
        }
        "tcp" => {
            let (lfd, port) = s.tcp_listen(0, 4, false, true).unwrap();
            let busy = s.tcp_listen(port, 4, false, true).map(|_| ());
            let refused = s.tcp_connect(1, false, true).map(|_| ());
            let server = s
                .spawn_thread(Box::new(move || {
                    let s = sys::current();
                    let (c, _) = s.tcp_accept(lfd, false, true).unwrap();
                    let mut data = Vec::new();
                    let mut buf = [0u8; 3];
                    loop {
                        let n = s.read(c, &mut buf).unwrap();
                        if n == 0 {
                            break;
                        }
                        data.extend_from_slice(&buf[..n]);
                    }
                    write_all(c, &data.to_ascii_uppercase()).unwrap();
                    s.close(c).unwrap();
                }))
                .unwrap();
            let (c, local) = s.tcp_connect(port, false, true).unwrap();
            let ports = s.tcp_ports(c).unwrap();
            write_all(c, b"hello tcp").unwrap();
            s.tcp_shutdown(c, false, true).unwrap();
            let got = sys::read_to_end(c).unwrap();
            s.join_thread(server).unwrap();
            let st = s.fstat(c).unwrap();
            out(format!(
                "{} {:?} {:?} {} {} {}\n",
                String::from_utf8_lossy(&got),
                busy,
                refused,
                ports == (local, Some(port)),
                (32768..=60999).contains(&port),
                st.mode & sysabi::mode::S_IFMT == sysabi::mode::S_IFSOCK
            ));
            0
        }
        "host-service" => {
            let ip: std::net::IpAddr = "127.0.0.80".parse().unwrap();
            let exchange = |fd: Fd, msg: &[u8]| {
                write_all(fd, msg).unwrap();
                s.tcp_shutdown(fd, false, true).unwrap();
                String::from_utf8_lossy(&sys::read_to_end(fd).unwrap()).into_owned()
            };
            let (c, _) = s.tcp_connect_at(ip, 4443, false, true).unwrap();
            let direct = exchange(c, b"ping");
            // Pelo nome do /etc/hosts, como o curl e o wget chegam.
            let named = exchange(s.net_connect(b"pypi.sandbox", 4443, None).unwrap().fd, b"named");
            let other_ip = s.tcp_connect_at("127.0.0.1".parse().unwrap(), 4443, false, true).map(|_| ());
            let busy = s.tcp_listen_at(ip, 4443, 4, false, true).map(|_| ());
            let busy_any = s.tcp_listen(4443, 4, false, true).map(|_| ());
            let free_other = s.tcp_listen_at("127.0.0.1".parse().unwrap(), 4443, 4, false, true).is_ok();
            let tcp = String::from_utf8_lossy(&sys::read_file(b"/proc/net/tcp").unwrap()).into_owned();
            let listed = tcp.lines().any(|l| l.contains("5000007F:115B 00000000:0000 0A"));
            out(format!("{direct} {named} {other_ip:?} {busy:?} {busy_any:?} {free_other} {listed}\n"));
            0
        }
        "net-names" => {
            let show = |host: &[u8]| s.net_connect(host, 9, None).map(|_| ());
            // Nenhum escuta na porta 9: o nome resolveu e o loopback respondeu recusando.
            out(format!(
                "{:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?}\n",
                show(b"localhost"),
                show(b"LOCALHOST"),
                show(b"ip6-loopback"),
                show(b"::1"),
                show(b"127.0.0.7"),
                show(b"10.0.0.1"),
                show(b"example.org"),
                show(b"alias.test")
            ));
            0
        }
        "threads" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let mut tids = Vec::new();
            for i in 0..4u8 {
                let t = s
                    .spawn_thread(Box::new(move || {
                        let s = sys::current();
                        assert_ne!(s.gettid(), s.getpid());
                        s.write(w, &[b'a' + i]).unwrap();
                    }))
                    .unwrap();
                tids.push(t);
            }
            for t in &tids {
                s.join_thread(*t).unwrap();
            }
            let again = s.join_thread(tids[0]);
            let me = s.join_thread(s.gettid());
            let mut buf = [0u8; 8];
            let n = s.read(r, &mut buf).unwrap();
            let mut got = buf[..n].to_vec();
            got.sort();
            out(format!(
                "{} {} {} {}\n",
                String::from_utf8_lossy(&got),
                again == Err(Errno::EINVAL),
                me == Err(Errno::EDEADLK),
                s.gettid() == s.getpid()
            ));
            0
        }
        "thread-exit" => {
            let _t = s
                .spawn_thread(Box::new(|| {
                    sys::exit(7);
                }))
                .unwrap();
            // A thread principal fica numa espera que o exit_group interrompe.
            let (r, _w) = s.pipe2(OFlags::empty()).unwrap();
            let mut b = [0u8; 1];
            let _ = s.read(r, &mut b);
            99
        }
        "dev" => {
            let mut b = [9u8; 4];
            let null = s.openat(Fd::CWD, b"/dev/null", OFlags::RDWR, 0).unwrap();
            let r0 = s.read(null, &mut b).unwrap();
            let zero = s.openat(Fd::CWD, b"/dev/zero", OFlags::RDONLY, 0).unwrap();
            s.read(zero, &mut b).unwrap();
            let full = s.openat(Fd::CWD, b"/dev/full", OFlags::WRONLY, 0).unwrap();
            let e = s.write(full, b"x");
            let tty = s.openat(Fd::CWD, b"/dev/tty", OFlags::RDONLY, 0);
            let link = s.readlinkat(Fd::CWD, b"/proc/self/fd/0").unwrap();
            out(format!("{r0} {b:?} {e:?} {tty:?} {}\n", String::from_utf8_lossy(&link)));
            0
        }
        "exec" => {
            let e = s.execve(b"/usr/bin/missing", &[b"x".to_vec()], None);
            out(format!("{e:?}\n"));
            let e = s.execve(b"/etc/passwd", &[b"x".to_vec()], None);
            out(format!("{e:?}\n"));
            // execve bem-sucedido: troca o programa (o eco vem do echo).
            let e = s.execve(b"/usr/bin/echo", &[b"echo".to_vec(), b"after".to_vec(), b"exec".to_vec()], None);
            out(format!("não devia voltar: {e:?}\n"));
            1
        }
        "seq-boundary" => {
            // Duas escritas, dois reads: a fronteira de cada mensagem sobrevive.
            let (a, b) = seq_pair(false);
            s.unix_sendto(a, b"abc", None).unwrap();
            s.unix_sendto(a, b"defgh", None).unwrap();
            let first = s.unix_recvfrom(b, 64, false).unwrap().0;
            let second = s.unix_recvfrom(b, 64, false).unwrap().0;
            out(format!("{} {}\n", String::from_utf8_lossy(&first), String::from_utf8_lossy(&second)));
            0
        }
        "seq-truncate" => {
            // Buffer menor que a mensagem: o resto é descartado, a próxima leitura traz a mensagem seguinte.
            let (a, b) = seq_pair(false);
            s.unix_sendto(a, b"0123456789", None).unwrap();
            s.unix_sendto(a, b"next", None).unwrap();
            let cut = s.unix_recvfrom(b, 4, false).unwrap().0;
            let after = s.unix_recvfrom(b, 64, false).unwrap().0;
            // O mesmo pelo lado cru (read do fd).
            s.write(a, b"abcdefgh").unwrap();
            s.write(a, b"zz").unwrap();
            let mut small = [0u8; 3];
            let n = s.read(b, &mut small).unwrap();
            let mut rest = [0u8; 16];
            let m = s.read(b, &mut rest).unwrap();
            out(format!(
                "{} {} {} {}\n",
                String::from_utf8_lossy(&cut),
                String::from_utf8_lossy(&after),
                String::from_utf8_lossy(&small[..n]),
                String::from_utf8_lossy(&rest[..m])
            ));
            0
        }
        "seq-peek" => {
            let (a, b) = seq_pair(true);
            s.unix_sendto(a, b"keep", None).unwrap();
            let p1 = s.unix_recvfrom(b, 64, true).unwrap().0;
            let p2 = s.unix_recvfrom(b, 2, true).unwrap().0;
            let got = s.unix_recvfrom(b, 64, false).unwrap().0;
            let empty = s.unix_recvfrom(b, 64, true).map(|_| ());
            out(format!(
                "{} {} {} {:?}\n",
                String::from_utf8_lossy(&p1),
                String::from_utf8_lossy(&p2),
                String::from_utf8_lossy(&got),
                empty
            ));
            0
        }
        "seq-empty-message" => {
            // Mensagem vazia chega como 0 bytes mas não é o fim: o fim só vem com o RDHUP do par.
            let (a, b) = seq_pair(true);
            let sent = s.unix_sendto(a, b"", None).unwrap();
            let ev = poll_now(b, PollEvents::IN | PollEvents::RDHUP);
            let got = s.unix_recvfrom(b, 64, false).unwrap().0.len();
            let idle = s.unix_recvfrom(b, 64, false).map(|_| ());
            let open = poll_now(b, PollEvents::IN | PollEvents::RDHUP);
            s.close(a).unwrap();
            let closed = poll_now(b, PollEvents::IN | PollEvents::RDHUP);
            let eof = s.unix_recvfrom(b, 64, false).unwrap().0.len();
            out(format!(
                "{sent} {} {} {got} {idle:?} {} {} {eof}\n",
                ev.contains(PollEvents::IN),
                ev.contains(PollEvents::RDHUP),
                open.contains(PollEvents::RDHUP),
                closed.contains(PollEvents::RDHUP)
            ));
            0
        }
        "seq-msgsize" => {
            // Limite de `sk_sndbuf - 32`: um byte acima é EMSGSIZE, o limite exato passa.
            let (a, _b) = seq_pair(false);
            let over = s.unix_sendto(a, &vec![0u8; 212_961], None);
            let exact = s.unix_sendto(a, &vec![0u8; 212_960], None);
            out(format!("{over:?} {exact:?}\n"));
            0
        }
        "seq-full-queue" => {
            let (a, b) = seq_pair(true);
            let mut sent = 0usize;
            let full = loop {
                match s.unix_sendto(a, b"x", None) {
                    Ok(_) => sent += 1,
                    Err(e) => break e,
                }
                assert!(sent < 100_000, "a fila nunca encheu");
            };
            let writable = poll_now(a, PollEvents::OUT).contains(PollEvents::OUT);
            // Ler uma mensagem devolve espaço ao remetente.
            s.unix_recvfrom(b, 64, false).unwrap();
            let again = s.unix_sendto(a, b"x", None);
            out(format!("{} {full:?} {writable} {again:?}\n", sent > 0));
            0
        }
        "seq-reset-on-unread-close" => {
            // O lado que fecha com mensagens do par sem ler faz o par ver ECONNRESET uma vez, depois o fim.
            let (a, b) = seq_pair(true);
            s.unix_sendto(a, b"unread", None).unwrap();
            s.close(b).unwrap();
            let err = poll_now(a, PollEvents::IN).contains(PollEvents::ERR);
            let first = s.unix_recvfrom(a, 64, false).map(|_| ());
            let second = s.unix_recvfrom(a, 64, false).map(|(d, _)| d.len());
            // Fechar sem nada pendente não gera reset.
            let (c, d) = seq_pair(true);
            s.close(d).unwrap();
            let clean = s.unix_recvfrom(c, 64, false).map(|(d, _)| d.len());
            out(format!("{err} {first:?} {second:?} {clean:?}\n"));
            0
        }
        "seq-shutdown-epipe" => {
            s.sigaction(Signal::SIGPIPE, SigDisposition::Ignore).unwrap();
            let (a, b) = seq_pair(false);
            s.tcp_shutdown(a, false, true).unwrap();
            let send = s.unix_sendto(a, b"late", None);
            let raw = s.write(a, b"late");
            let eof = s.unix_recvfrom(b, 64, false).map(|(d, _)| d.len());
            out(format!("{send:?} {raw:?} {eof:?}\n"));
            0
        }
        "seq-data-after-sender-close" => {
            let (a, b) = seq_pair(false);
            s.unix_sendto(a, b"one", None).unwrap();
            s.unix_sendto(a, b"two", None).unwrap();
            s.close(a).unwrap();
            let one = s.unix_recvfrom(b, 64, false).unwrap().0;
            let two = s.unix_recvfrom(b, 64, false).unwrap().0;
            let eof = s.unix_recvfrom(b, 64, false).unwrap().0.len();
            out(format!("{} {} {eof}\n", String::from_utf8_lossy(&one), String::from_utf8_lossy(&two)));
            0
        }
        "seq-raw-bytes" => {
            // O read e o write do fd veem exatamente os bytes da mensagem, sem cabeçalho de enquadramento,
            // mesmo quando o conteúdo parece um (quatro bytes de tamanho na frente).
            let (a, b) = seq_pair(false);
            let framed_looking: &[u8] = &[0, 0, 0, 3, b'a', b'b', b'c'];
            s.write(a, framed_looking).unwrap();
            s.write(a, b"raw bytes").unwrap();
            let mut buf = [0u8; 64];
            let n1 = s.read(b, &mut buf).unwrap();
            let first = buf[..n1].to_vec();
            let n2 = s.read(b, &mut buf).unwrap();
            let second = buf[..n2].to_vec();
            // E no sentido contrário, do sendto ao read.
            s.unix_sendto(b, b"x", None).unwrap();
            let n3 = s.read(a, &mut buf).unwrap();
            out(format!("{} {} {n3}\n", first == framed_looking, second == b"raw bytes"));
            0
        }
        "tcp-send-unconnected" => {
            // `tcp_sendmsg` em CLOSE ou LISTEN: `sk_stream_wait_connect` dá EPIPE (não ENOTCONN) e o
            // `sk_stream_error` manda SIGPIPE, salvo `MSG_NOSIGNAL`. `write(2)` é `send` sem flags.
            s.sigaction(Signal::SIGPIPE, SigDisposition::Catch).unwrap();
            let fresh = s.tcp_socket(false, false, true).unwrap();
            let send = s.sock_send(fresh, b"x", MsgFlags::empty());
            let sig_send = sigpipes();
            let nosignal = s.sock_send(fresh, b"x", MsgFlags::NOSIGNAL);
            let sig_nosignal = sigpipes();
            let write = s.write(fresh, b"x");
            let sig_write = sigpipes();
            let listening = s.sock_send(tcp_listening(), b"x", MsgFlags::empty());
            let sig_listening = sigpipes();
            out(format!("{send:?} {sig_send} {nosignal:?} {sig_nosignal} {write:?} {sig_write} {listening:?} {sig_listening}\n"));
            0
        }
        "tcp-recv-unconnected" => {
            // `tcp_recvmsg`: em CLOSE (nunca conectado) e em LISTEN, ENOTCONN. Depois de `shutdown(SHUT_RD)` o que
            // já chegou segue legível, a fila vazia dá 0 sem esperar, e o par continua podendo escrever.
            let fresh = s.tcp_socket(false, false, true).unwrap();
            let recv = s.sock_recv(fresh, 16, MsgFlags::empty()).map(|v| v.len());
            let mut buf = [0u8; 16];
            let read = s.read(fresh, &mut buf);
            let listening = s.sock_recv(tcp_listening(), 16, MsgFlags::DONTWAIT).map(|v| v.len());
            let (client, server) = tcp_pair();
            write_all(server, b"abc").unwrap();
            s.tcp_shutdown(client, true, false).unwrap();
            let queued = s.sock_recv(client, 16, MsgFlags::DONTWAIT).unwrap();
            let after = s.sock_recv(client, 16, MsgFlags::empty()).map(|v| v.len());
            let peer_write = s.write(server, b"def");
            out(format!("{recv:?} {read:?} {listening:?} {} {after:?} {peer_write:?}\n", String::from_utf8_lossy(&queued)));
            0
        }
        "tcp-refused" => {
            // `connect` não bloqueante recusado: o RST chega depois (EINPROGRESS) e o socket fica em CLOSE com
            // `SHUTDOWN_MASK`. `tcp_poll` dá `IN|OUT|HUP|RDHUP`, mais `ERR` enquanto o `sk_err` não foi lido.
            // `recv` entrega o ECONNREFUSED uma vez e depois dá 0; `send` dá EPIPE. O `connect` bloqueante recusado
            // faz `tcp_disconnect`: o socket volta ao estado inicial, e `recv` dá ENOTCONN.
            let lo: std::net::IpAddr = std::net::Ipv4Addr::LOCALHOST.into();
            let all = PollEvents::IN | PollEvents::OUT | PollEvents::HUP | PollEvents::RDHUP;
            let fd = s.tcp_socket(false, true, true).unwrap();
            let connect = s.tcp_connect_fd(fd, lo, 1);
            let with_err = poll_now(fd, PollEvents::IN | PollEvents::OUT | PollEvents::RDHUP).contains(all | PollEvents::ERR);
            let first = s.sock_recv(fd, 16, MsgFlags::empty()).map(|v| v.len());
            let second = s.sock_recv(fd, 16, MsgFlags::empty()).map(|v| v.len());
            let so_error = s.sock_error(fd);
            let send = s.sock_send(fd, b"x", MsgFlags::NOSIGNAL);
            let drained = poll_now(fd, PollEvents::IN | PollEvents::OUT | PollEvents::RDHUP) == all;
            let other = s.tcp_socket(false, true, true).unwrap();
            let _ = s.tcp_connect_fd(other, lo, 1);
            let consumed = s.sock_error(other).map(|e| e == Errno::ECONNREFUSED.0);
            let after_so_error = s.sock_recv(other, 16, MsgFlags::empty()).map(|v| v.len());
            let blocking = s.tcp_socket(false, false, true).unwrap();
            let blocking_connect = s.tcp_connect_fd(blocking, lo, 1);
            let blocking_recv = s.sock_recv(blocking, 16, MsgFlags::DONTWAIT).map(|v| v.len());
            out(format!(
                "{connect:?} {with_err} {first:?} {second:?} {so_error:?} {send:?} {drained} {consumed:?} {after_so_error:?} {blocking_connect:?} {blocking_recv:?}\n"
            ));
            0
        }
        "tcp-rst" => {
            // O par que fecha com dados não lidos manda RST: o `sk_err` da outra ponta é ECONNRESET, `tcp_poll` dá
            // `ERR|HUP|IN|RDHUP|OUT`. O erro sai uma vez, por `recv` (só com a fila vazia), por `SO_ERROR` ou por
            // `send` (sem SIGPIPE: o `sk_stream_error` só o manda quando sobra EPIPE); depois `recv` dá 0 e `send`,
            // EPIPE com SIGPIPE.
            s.sigaction(Signal::SIGPIPE, SigDisposition::Catch).unwrap();
            let reset_pair = || {
                let (client, server) = tcp_pair();
                write_all(client, b"x").unwrap();
                s.close(server).unwrap();
                client
            };
            let c = reset_pair();
            let flags = PollEvents::IN | PollEvents::OUT | PollEvents::RDHUP;
            let polled = poll_now(c, flags).contains(flags | PollEvents::ERR | PollEvents::HUP);
            let recv_first = s.sock_recv(c, 16, MsgFlags::empty()).map(|v| v.len());
            let recv_then = s.sock_recv(c, 16, MsgFlags::empty()).map(|v| v.len());
            let send_then = s.sock_send(c, b"x", MsgFlags::NOSIGNAL);
            let c = reset_pair();
            let so_error = s.sock_error(c).map(|e| e == Errno::ECONNRESET.0);
            let recv_after = s.sock_recv(c, 16, MsgFlags::empty()).map(|v| v.len());
            let c = reset_pair();
            let _ = sigpipes();
            let send_first = s.sock_send(c, b"y", MsgFlags::empty());
            let sig_first = sigpipes();
            let send_second = s.sock_send(c, b"y", MsgFlags::empty());
            let sig_second = sigpipes();
            out(format!(
                "{polled} {recv_first:?} {recv_then:?} {send_then:?} {so_error:?} {recv_after:?} {send_first:?} {sig_first} {send_second:?} {sig_second}\n"
            ));
            0
        }
        "tcp-peer-closed" => {
            // O par fechou sem dados pendentes (FIN): `tcp_poll` dá `IN|RDHUP|OUT` (sem HUP nem ERR). A primeira
            // escrita é aceita e o RST que volta, com a ponta em CLOSE_WAIT, deixa `sk_err = EPIPE`: `tcp_poll`
            // passa a dar `ERR|HUP`, `recv` segue dando 0 (o `SOCK_DONE` do FIN vale antes do `sk_err`), `SO_ERROR`
            // entrega EPIPE, e a escrita seguinte dá EPIPE com SIGPIPE.
            s.sigaction(Signal::SIGPIPE, SigDisposition::Catch).unwrap();
            let closed_pair = || {
                let (client, server) = tcp_pair();
                s.close(server).unwrap();
                client
            };
            let flags = PollEvents::IN | PollEvents::OUT | PollEvents::RDHUP;
            let c = closed_pair();
            let fin_only = poll_now(c, flags) == flags;
            let first = s.sock_send(c, b"x", MsgFlags::NOSIGNAL);
            let recv = s.sock_recv(c, 16, MsgFlags::empty()).map(|v| v.len());
            let aborted = poll_now(c, flags).contains(PollEvents::ERR | PollEvents::HUP);
            let so_error = s.sock_error(c).map(|e| e == Errno::EPIPE.0);
            let c = closed_pair();
            let _ = sigpipes();
            let ok = s.sock_send(c, b"x", MsgFlags::NOSIGNAL).map(|n| n == 1);
            let epipe = s.sock_send(c, b"x", MsgFlags::empty());
            out(format!("{fin_only} {first:?} {recv:?} {aborted} {so_error:?} {ok:?} {epipe:?} {}\n", sigpipes()));
            0
        }
        "unix-stream-epipe" => {
            // `unix_stream_sendmsg` com o par fechado: EPIPE na primeira escrita (não há RST a esperar), com SIGPIPE
            // salvo `MSG_NOSIGNAL`.
            s.sigaction(Signal::SIGPIPE, SigDisposition::Catch).unwrap();
            let (a, b) = s.unix_socketpair(1, false, true).unwrap();
            s.close(b).unwrap();
            let send = s.sock_send(a, b"x", MsgFlags::empty());
            let sig_send = sigpipes();
            let nosignal = s.sock_send(a, b"x", MsgFlags::NOSIGNAL);
            let sig_nosignal = sigpipes();
            let write = s.write(a, b"x");
            let sig_write = sigpipes();
            out(format!("{send:?} {sig_send} {nosignal:?} {sig_nosignal} {write:?} {sig_write}\n"));
            0
        }
        "tcp-recv-flags" => {
            // `MSG_DONTWAIT` não espera num fd bloqueante; `MSG_PEEK` não consome a fila; `MSG_WAITALL` espera o
            // tanto pedido, para no fim do fluxo, e com fd sem bloqueio (ou `MSG_DONTWAIT`) leva o que tem
            // (`!timeo` com `copied > 0`) ou dá EAGAIN se a fila está vazia.
            let (c, srv) = tcp_pair();
            let dontwait = s.sock_recv(c, 16, MsgFlags::DONTWAIT);
            write_all(srv, b"hello").unwrap();
            let peek_part = s.sock_recv(c, 3, MsgFlags::PEEK).unwrap();
            let peek_all = s.sock_recv(c, 16, MsgFlags::PEEK).unwrap();
            let got = s.sock_recv(c, 16, MsgFlags::empty()).unwrap();
            let peek_empty = s.sock_recv(c, 16, MsgFlags::PEEK | MsgFlags::DONTWAIT);
            write_all(srv, b"he").unwrap();
            let writer = s
                .spawn_thread(Box::new(move || {
                    let s = sys::current();
                    s.nanosleep(Duration::from_millis(40)).unwrap();
                    write_all(srv, b"llo").unwrap();
                }))
                .unwrap();
            let waitall = s.sock_recv(c, 5, MsgFlags::WAITALL).unwrap();
            s.join_thread(writer).unwrap();
            write_all(srv, b"ab").unwrap();
            s.close(srv).unwrap();
            let until_eof = s.sock_recv(c, 5, MsgFlags::WAITALL).unwrap();
            let eof = s.sock_recv(c, 5, MsgFlags::WAITALL).map(|v| v.len());
            let (c2, srv2) = tcp_pair();
            write_all(srv2, b"xy").unwrap();
            let partial = s.sock_recv(c2, 5, MsgFlags::WAITALL | MsgFlags::DONTWAIT).unwrap();
            let nothing = s.sock_recv(c2, 5, MsgFlags::WAITALL | MsgFlags::DONTWAIT);
            let (c3, srv3) = tcp_pair();
            write_all(srv3, b"ab").unwrap();
            let peeker = s
                .spawn_thread(Box::new(move || {
                    let s = sys::current();
                    s.nanosleep(Duration::from_millis(40)).unwrap();
                    write_all(srv3, b"cde").unwrap();
                }))
                .unwrap();
            let peek_waitall = s.sock_recv(c3, 5, MsgFlags::PEEK | MsgFlags::WAITALL).unwrap();
            s.join_thread(peeker).unwrap();
            let kept = s.sock_recv(c3, 16, MsgFlags::empty()).unwrap();
            let text = |v: &[u8]| String::from_utf8_lossy(v).into_owned();
            out(format!(
                "{dontwait:?} {} {} {} {peek_empty:?} {} {} {eof:?} {} {nothing:?} {} {}\n",
                text(&peek_part),
                text(&peek_all),
                text(&got),
                text(&waitall),
                text(&until_eof),
                text(&partial),
                text(&peek_waitall),
                text(&kept)
            ));
            0
        }
        "sock-info-opts" => {
            // `SO_DOMAIN`, `SO_TYPE`, `SO_PROTOCOL` e `SO_ACCEPTCONN` vêm do socket; as opções que o Python
            // normaliza (`SO_RCVBUF`/`SO_SNDBUF` dobrados, com os mínimos e o padrão de `tcp_rmem`/`tcp_wmem`
            // 131072/16384) ficam guardadas como bytes na descrição e são vistas por todos os fds de `dup`.
            let tcp4 = s.tcp_socket(false, false, true).unwrap();
            let tcp6 = s.tcp_socket(true, false, true).unwrap();
            let udp = s.udp_socket(false, false, true).unwrap();
            let unix_dgram = s.unix_socket(2, false, true).unwrap();
            let unix_stream = s.unix_socket(1, false, true).unwrap();
            let infos: Vec<_> = [tcp4, tcp6, tcp_listening(), udp, unix_dgram, unix_stream].iter().map(|fd| s.sock_info(*fd).unwrap()).collect();
            s.sock_setopt(tcp4, 1, 2, &1i32.to_le_bytes()).unwrap();
            s.sock_setopt(tcp4, 6, 1, &1i32.to_le_bytes()).unwrap();
            let duplicate = s.dup_min(tcp4, Fd(0), true).unwrap();
            let reuse = s.sock_getopt(duplicate, 1, 2).unwrap();
            let nodelay = s.sock_getopt(tcp4, 6, 1).unwrap();
            let unset = s.sock_getopt(tcp4, 1, 8).unwrap();
            let not_a_socket = s.sock_getopt(s.pipe2(OFlags::empty()).unwrap().0, 1, 2);
            out(format!("{infos:?} {reuse:?} {nodelay:?} {unset:?} {not_a_socket:?}\n"));
            0
        }
        "tcp-sndbuf-expand" => {
            // `tcp_init_buffer_space` chama o `tcp_sndbuf_expand` no estabelecimento, dos dois lados: no loopback
            // `sk_sndbuf` vira 2 * 10 * (roundup_pow_of_two(65495 + 256 + 320) + 256) = 2626560 (abaixo do teto
            // `tcp_wmem[2]` de 4194304). Sem conexão fica o padrão (nada guardado aqui; o Python mostra 16384).
            // Um `SO_SNDBUF` definido antes trava o buffer (`SOCK_SNDBUF_LOCK`) e o valor dobrado se mantém; o
            // socket aceito herda o do que escuta.
            let sndbuf = |fd: Fd| s.sock_getopt(fd, 1, 7).unwrap().map(|v| i64::from_le_bytes(v.try_into().unwrap()));
            let lo: std::net::IpAddr = std::net::Ipv4Addr::LOCALHOST.into();
            let unconnected = sndbuf(s.tcp_socket(false, false, true).unwrap());
            let (client, server) = tcp_pair();
            let (client_buf, server_buf) = (sndbuf(client), sndbuf(server));
            let (listener, port) = s.tcp_listen(0, 4, false, true).unwrap();
            let locked = s.tcp_socket(false, false, true).unwrap();
            s.sock_setopt(locked, 1, 7, &131072i64.to_le_bytes()).unwrap();
            s.tcp_connect_fd(locked, lo, port).unwrap();
            let (accepted, _) = s.tcp_accept(listener, false, true).unwrap();
            let (listener2, port2) = s.tcp_listen(0, 4, false, true).unwrap();
            s.sock_setopt(listener2, 1, 7, &16384i64.to_le_bytes()).unwrap();
            s.tcp_connect(port2, false, true).unwrap();
            let (inherited, _) = s.tcp_accept(listener2, false, true).unwrap();
            out(format!("{unconnected:?} {client_buf:?} {server_buf:?} {:?} {:?} {:?}\n", sndbuf(locked), sndbuf(accepted), sndbuf(inherited)));
            0
        }
        "tcp-shutdown-wakes" => {
            // Quem já espera acorda: `shutdown(SHUT_RD)` de outra thread tira o `recv` bloqueado da espera com 0
            // (`sk_state_change` e o `RCV_SHUTDOWN` conferido no laço do `tcp_recvmsg`); `shutdown(SHUT_RD)` num
            // socket em LISTEN faz o `tcp_disconnect`: o `accept` bloqueado dá EINVAL, o `connect` seguinte
            // ECONNREFUSED, `SO_ACCEPTCONN` volta a 0 e o poll dá `OUT|HUP`.
            let (c, _srv) = tcp_pair();
            let waker = s
                .spawn_thread(Box::new(move || {
                    let s = sys::current();
                    s.nanosleep(Duration::from_millis(40)).unwrap();
                    s.tcp_shutdown(c, true, false).unwrap();
                }))
                .unwrap();
            let recv = s.sock_recv(c, 16, MsgFlags::empty()).map(|v| v.len());
            s.join_thread(waker).unwrap();
            let (l, port) = s.tcp_listen(0, 4, false, true).unwrap();
            let waker = s
                .spawn_thread(Box::new(move || {
                    let s = sys::current();
                    s.nanosleep(Duration::from_millis(40)).unwrap();
                    s.tcp_shutdown(l, true, false).unwrap();
                }))
                .unwrap();
            let accept = s.tcp_accept(l, false, true).map(|_| ());
            s.join_thread(waker).unwrap();
            let connect = s.tcp_connect(port, false, true).map(|_| ());
            let listening = s.sock_info(l).unwrap().3;
            let polled = poll_now(l, PollEvents::IN | PollEvents::OUT) == PollEvents::OUT | PollEvents::HUP;
            out(format!("{recv:?} {accept:?} {connect:?} {listening} {polled}\n"));
            0
        }
        "unix-shutdown-rd" => {
            // `unix_shutdown(SHUT_RD)` só marca `RCV_SHUTDOWN` no socket (e `SEND_SHUTDOWN` no par): o que está na fila
            // sai antes do 0, o par recebe EPIPE na escrita, `unix_poll` dá `IN|OUT|RDHUP` e, com `SHUT_WR` também
            // (`SHUTDOWN_MASK`), HUP. Um `recv` bloqueado acorda com 0.
            let (a, b) = s.unix_socketpair(1, false, true).unwrap();
            write_all(b, b"abc").unwrap();
            s.tcp_shutdown(a, true, false).unwrap();
            let queued = s.sock_recv(a, 16, MsgFlags::DONTWAIT).unwrap();
            let after = s.sock_recv(a, 16, MsgFlags::empty()).map(|v| v.len());
            let peer_write = s.sock_send(b, b"x", MsgFlags::NOSIGNAL);
            let flags = PollEvents::IN | PollEvents::OUT | PollEvents::RDHUP;
            let polled = poll_now(a, flags) == flags;
            s.tcp_shutdown(a, false, true).unwrap();
            let hup = poll_now(a, flags).contains(PollEvents::HUP);
            let (c, _d) = s.unix_socketpair(1, false, true).unwrap();
            let waker = s
                .spawn_thread(Box::new(move || {
                    let s = sys::current();
                    s.nanosleep(Duration::from_millis(40)).unwrap();
                    s.tcp_shutdown(c, true, false).unwrap();
                }))
                .unwrap();
            let blocked = s.sock_recv(c, 16, MsgFlags::empty()).map(|v| v.len());
            s.join_thread(waker).unwrap();
            out(format!("{} {after:?} {peer_write:?} {polled} {hup} {blocked:?}\n", String::from_utf8_lossy(&queued)));
            0
        }
        "unix-connect-wakes" => {
            // `connect` esperando vaga na fila (`unix_recvq_full`: um além do backlog 0) acorda quando o socket que
            // escuta fecha: o socket morto não é mais achado e o `connect` dá ECONNREFUSED.
            let l = s.unix_socket(1, false, true).unwrap();
            s.unix_bind(l, b"\0wake-connect").unwrap();
            s.unix_listen(l, 0).unwrap();
            let first = s.unix_socket(1, false, true).unwrap();
            let queued = s.unix_connect(first, b"\0wake-connect");
            let second = s.unix_socket(1, false, true).unwrap();
            let closer = s
                .spawn_thread(Box::new(move || {
                    let s = sys::current();
                    s.nanosleep(Duration::from_millis(40)).unwrap();
                    s.close(l).unwrap();
                }))
                .unwrap();
            let blocked = s.unix_connect(second, b"\0wake-connect");
            s.join_thread(closer).unwrap();
            out(format!("{queued:?} {blocked:?}\n"));
            0
        }
        "unix-shutdown-unconnected" => {
            // `unix_shutdown` não confere o estado: `shutdown(SHUT_RD)` num AF_UNIX de fluxo que nunca conectou
            // devolve 0 (medido no oráculo), não ENOTCONN.
            let a = s.unix_socket(1, false, true).unwrap();
            let rd = s.tcp_shutdown(a, true, false);
            let wr = s.tcp_shutdown(a, false, true);
            out(format!("{rd:?} {wr:?}\n"));
            0
        }
        "unix-listener-close-resets" => {
            // `unix_release_sock` do listener descarta o embrião com `embrion` verdadeiro: o cliente que ficou na
            // fila de `accept` recebe ECONNRESET uma vez (`sk_err`) e depois o fim (0).
            let l = s.unix_socket(1, false, true).unwrap();
            s.unix_bind(l, b"\0close-pending").unwrap();
            s.unix_listen(l, 4).unwrap();
            let client = s.unix_socket(1, false, true).unwrap();
            let connected = s.unix_connect(client, b"\0close-pending");
            s.close(l).unwrap();
            let first = s.sock_recv(client, 16, MsgFlags::DONTWAIT).map(|v| v.len());
            let second = s.sock_recv(client, 16, MsgFlags::DONTWAIT).map(|v| v.len());
            out(format!("{connected:?} {first:?} {second:?}\n"));
            0
        }
        "tcp-listener-close-resets" => {
            // `inet_csk_listen_stop` manda RST às conexões que ninguém aceitou: o `recv` do cliente dá ECONNRESET e
            // depois 0 (medido no oráculo).
            let (l, port) = s.tcp_listen(0, 4, false, true).unwrap();
            let (client, _) = s.tcp_connect(port, false, true).unwrap();
            s.close(l).unwrap();
            let first = s.sock_recv(client, 16, MsgFlags::DONTWAIT).map(|v| v.len());
            let second = s.sock_recv(client, 16, MsgFlags::DONTWAIT).map(|v| v.len());
            out(format!("{first:?} {second:?}\n"));
            0
        }
        "socket-rcvbuf-fixed" => {
            // O `SO_RCVBUF` não cresce no Linux (131072 antes e depois do connect, medido): o kernel não guarda
            // valor algum, o padrão 131072 é do `_socket.py`, e a conexão não muda nada (diferente do `SO_SNDBUF`).
            let rcvbuf = |fd: Fd| s.sock_getopt(fd, 1, 8).unwrap().map(|v| i64::from_le_bytes(v.try_into().unwrap()));
            let (client, server) = tcp_pair();
            let before = rcvbuf(s.tcp_socket(false, false, true).unwrap());
            out(format!("{before:?} {:?} {:?}\n", rcvbuf(client), rcvbuf(server)));
            0
        }
        "scm-rights-stream" => {
            // `SCM_RIGHTS` num fluxo: o fd de leitura de um pipe vai pelo socket, o remetente o fecha, e quem
            // recebe lê o que se escreve na outra ponta pelo fd novo.
            let (a, b) = s.unix_socketpair(1, false, true).unwrap();
            let (r, w) = s.pipe2(OFlags::CLOEXEC).unwrap();
            let data = r.0.to_le_bytes();
            let control = cmsg::build(&[cmsg::Item { level: cmsg::SOL_SOCKET, kind: cmsg::SCM_RIGHTS, data: &data }]);
            let sent = s.unix_sendmsg(a, b"x", None, &control, MsgFlags::empty());
            s.close(r).unwrap();
            let got = s.unix_recvmsg(b, 16, cmsg::space(4), MsgFlags::empty()).unwrap();
            let items = cmsg::parse(&got.control).unwrap();
            let fd = Fd(i32::from_le_bytes(items[0].data[..4].try_into().unwrap()));
            write_all(w, b"hi").unwrap();
            let mut buf = [0u8; 8];
            let n = s.read(fd, &mut buf).unwrap();
            out(format!(
                "{sent:?} {} {} {} {} {}\n",
                String::from_utf8_lossy(&got.data),
                items.len(),
                items[0].kind,
                got.flags.bits(),
                String::from_utf8_lossy(&buf[..n])
            ));
            0
        }
        "scm-rights-stream-boundaries" => {
            // Cada `sendmsg` é um `sk_buff`: a leitura cola os dados de vários, mas encerra no que leva descritores
            // (`unix_stream_read_generic`), então "ab" e "cd" (com o fd) saem juntos e "ef" fica para a próxima.
            let (a, b) = s.unix_socketpair(1, false, true).unwrap();
            let (r, _w) = s.pipe2(OFlags::CLOEXEC).unwrap();
            let data = r.0.to_le_bytes();
            let control = cmsg::build(&[cmsg::Item { level: cmsg::SOL_SOCKET, kind: cmsg::SCM_RIGHTS, data: &data }]);
            s.unix_sendmsg(a, b"ab", None, &[], MsgFlags::empty()).unwrap();
            s.unix_sendmsg(a, b"cd", None, &control, MsgFlags::empty()).unwrap();
            s.unix_sendmsg(a, b"ef", None, &[], MsgFlags::empty()).unwrap();
            let first = s.unix_recvmsg(b, 16, cmsg::space(4), MsgFlags::empty()).unwrap();
            let second = s.unix_recvmsg(b, 16, cmsg::space(4), MsgFlags::empty()).unwrap();
            out(format!(
                "{} {} {} {}\n",
                String::from_utf8_lossy(&first.data),
                cmsg::parse(&first.control).unwrap().len(),
                String::from_utf8_lossy(&second.data),
                second.control.len()
            ));
            0
        }
        "scm-rights-stream-partial-read" => {
            // O fd sai com a primeira leitura que toca o `sk_buff`, mesmo parcial; o resto do `sk_buff` é dado comum.
            let (a, b) = s.unix_socketpair(1, false, true).unwrap();
            let (r, _w) = s.pipe2(OFlags::CLOEXEC).unwrap();
            let data = r.0.to_le_bytes();
            let control = cmsg::build(&[cmsg::Item { level: cmsg::SOL_SOCKET, kind: cmsg::SCM_RIGHTS, data: &data }]);
            s.unix_sendmsg(a, b"wxyz", None, &control, MsgFlags::empty()).unwrap();
            let first = s.unix_recvmsg(b, 2, cmsg::space(4), MsgFlags::empty()).unwrap();
            let second = s.unix_recvmsg(b, 8, cmsg::space(4), MsgFlags::empty()).unwrap();
            out(format!(
                "{} {} {} {}\n",
                String::from_utf8_lossy(&first.data),
                cmsg::parse(&first.control).unwrap().len(),
                String::from_utf8_lossy(&second.data),
                second.control.len()
            ));
            0
        }
        "scm-rights-truncated" => {
            // Sem `msg_control`, ou com menos que um item de um fd (`CMSG_LEN(4)` = 20), o descritor se perde e a
            // chamada marca `MSG_CTRUNC` (8).
            let (a, b) = s.unix_socketpair(5, false, true).unwrap();
            let (r, _w) = s.pipe2(OFlags::CLOEXEC).unwrap();
            let data = r.0.to_le_bytes();
            let control = cmsg::build(&[cmsg::Item { level: cmsg::SOL_SOCKET, kind: cmsg::SCM_RIGHTS, data: &data }]);
            let mut seen = Vec::new();
            for room in [0, 16, 19] {
                s.unix_sendmsg(a, b"m", None, &control, MsgFlags::empty()).unwrap();
                let got = s.unix_recvmsg(b, 8, room, MsgFlags::empty()).unwrap();
                seen.push(format!("{}/{}", got.flags.bits(), got.control.len()));
            }
            s.unix_sendmsg(a, b"m", None, &control, MsgFlags::empty()).unwrap();
            let fits = s.unix_recvmsg(b, 8, 20, MsgFlags::empty()).unwrap();
            seen.push(format!("{}/{}", fits.flags.bits(), fits.control.len()));
            out(format!("{}\n", seen.join(" ")));
            0
        }
        "scm-rights-dgram-peek" => {
            // `MSG_PEEK` num datagrama com descritores instala cópias (`unix_peek_fds`) e a mensagem continua na fila
            // com os dela: cada leitura dá um fd novo da mesma descrição de arquivo.
            let (a, b) = s.unix_socketpair(2, false, true).unwrap();
            let (r, _w) = s.pipe2(OFlags::CLOEXEC).unwrap();
            let data = r.0.to_le_bytes();
            let control = cmsg::build(&[cmsg::Item { level: cmsg::SOL_SOCKET, kind: cmsg::SCM_RIGHTS, data: &data }]);
            s.unix_sendmsg(a, b"m", None, &control, MsgFlags::empty()).unwrap();
            let fd_of = |control: &[u8]| Fd(i32::from_le_bytes(cmsg::parse(control).unwrap()[0].data[..4].try_into().unwrap()));
            let peeked = s.unix_recvmsg(b, 8, cmsg::space(4), MsgFlags::PEEK).unwrap();
            let taken = s.unix_recvmsg(b, 8, cmsg::space(4), MsgFlags::empty()).unwrap();
            let (f1, f2) = (fd_of(&peeked.control), fd_of(&taken.control));
            let empty = s.unix_recvmsg(b, 8, cmsg::space(4), MsgFlags::DONTWAIT);
            out(format!("{} {} {} {:?}\n", f1 != f2, s.fstat(f1).unwrap().ino == s.fstat(f2).unwrap().ino, s.fstat(f1).unwrap().ino == s.fstat(r).unwrap().ino, empty.map(|_| ())));
            0
        }
        "scm-rights-errors" => {
            // `__scm_send`: descritor inexistente é EBADF; `cmsg_len` fora do buffer é EINVAL; nível diferente de
            // `SOL_SOCKET` é ignorado; tipo desconhecido em `SOL_SOCKET` é EINVAL; mais de 253 fds é EINVAL.
            let (a, _b) = s.unix_socketpair(1, false, true).unwrap();
            let bad = cmsg::build(&[cmsg::Item { level: cmsg::SOL_SOCKET, kind: cmsg::SCM_RIGHTS, data: &99i32.to_le_bytes() }]);
            let mut cut = bad.clone();
            cut[0] = 200;
            let other_level = cmsg::build(&[cmsg::Item { level: 6, kind: 1, data: &[1, 2, 3, 4] }]);
            let unknown = cmsg::build(&[cmsg::Item { level: cmsg::SOL_SOCKET, kind: 77, data: &[0; 4] }]);
            let many = vec![0u8; 254 * 4];
            let too_many = cmsg::build(&[cmsg::Item { level: cmsg::SOL_SOCKET, kind: cmsg::SCM_RIGHTS, data: &many }]);
            let send = |control: &[u8]| s.unix_sendmsg(a, b"z", None, control, MsgFlags::empty());
            out(format!("{:?} {:?} {:?} {:?} {:?}\n", send(&bad), send(&cut), send(&other_level), send(&unknown), send(&too_many)));
            0
        }
        "scm-credentials" => {
            // `SO_PASSCRED` no receptor: o `recvmsg` traz `SCM_CREDENTIALS` com pid, uid e gid reais de quem enviou,
            // antes de qualquer `SCM_RIGHTS`; sem a opção não vem item nenhum. Vale em fluxo, datagrama e seqpacket.
            let mut seen = Vec::new();
            for ty in [1u8, 2, 5] {
                let (a, b) = s.unix_socketpair(ty, false, true).unwrap();
                s.unix_sendmsg(a, b"c", None, &[], MsgFlags::empty()).unwrap();
                let plain = s.unix_recvmsg(b, 8, cmsg::space(12), MsgFlags::empty()).unwrap();
                s.sock_setopt(b, 1, 16, &1i64.to_le_bytes()).unwrap();
                s.unix_sendmsg(a, b"c", None, &[], MsgFlags::empty()).unwrap();
                let got = s.unix_recvmsg(b, 8, cmsg::space(12), MsgFlags::empty()).unwrap();
                let items = cmsg::parse(&got.control).unwrap();
                let ucred = items.first().map(|i| (i.level, i.kind, i.data.to_vec()));
                let mut want = Vec::new();
                want.extend_from_slice(&s.getpid().to_le_bytes());
                want.extend_from_slice(&s.getuid().to_le_bytes());
                want.extend_from_slice(&s.getgid().to_le_bytes());
                seen.push(format!("{}/{}", plain.control.len(), ucred == Some((1, 2, want))));
            }
            out(format!("{}\n", seen.join(" ")));
            0
        }
        "peercred" => {
            // `SO_PEERCRED`: o `socketpair` guarda as credenciais de quem o criou nas duas pontas; o `listen` as de
            // quem escuta, e o `connect` leva ao cliente as do servidor e ao aceito as do cliente; um socket sem
            // par devolve pid 0 e uid/gid -1.
            let me = |s: &dyn Syscalls| {
                let mut v = Vec::new();
                v.extend_from_slice(&s.getpid().to_le_bytes());
                v.extend_from_slice(&s.geteuid().to_le_bytes());
                v.extend_from_slice(&s.getegid().to_le_bytes());
                v
            };
            let want = me(&*s);
            let peer = |fd: Fd| s.sock_getopt(fd, 1, 17).unwrap().unwrap();
            let (a, b) = s.unix_socketpair(1, false, true).unwrap();
            let pair = peer(a) == want && peer(b) == want;
            let l = s.unix_socket(1, false, true).unwrap();
            let before_listen = peer(l);
            s.unix_bind(l, b"\0peercred").unwrap();
            s.unix_listen(l, 4).unwrap();
            let c = s.unix_socket(1, false, true).unwrap();
            let unset_client = peer(c);
            s.unix_connect(c, b"\0peercred").unwrap();
            let accepted = s.unix_accept(l, false, true).unwrap();
            let chain = peer(l) == want && peer(c) == want && peer(accepted) == want;
            let mut unset = Vec::new();
            unset.extend_from_slice(&0i32.to_le_bytes());
            unset.extend_from_slice(&u32::MAX.to_le_bytes());
            unset.extend_from_slice(&u32::MAX.to_le_bytes());
            out(format!("{pair} {} {chain} {}\n", before_listen == unset, unset_client == unset));
            0
        }
        _ => {
            ctx.error("cenário desconhecido");
            2
        }
    }
}

fn programs() -> Vec<Program> {
    vec![
        Program::bin("echo", p_echo),
        Program::bin("cat", p_cat),
        Program::bin("yes", p_yes),
        Program::bin("head", p_head),
        Program::bin("true", p_true),
        Program::bin("false", p_false),
        Program::bin("sleep", p_sleep),
        Program::bin("pipeline", p_pipeline),
        Program::bin("scenario", p_scenario),
    ]
}

fn sandbox() -> Sandbox {
    let k = Kernel::new(KernelConfig::default());
    k.create_sandbox(SandboxConfig { programs: programs(), ..SandboxConfig::default() }).unwrap()
}

fn argv(v: &[&str]) -> Vec<Vec<u8>> {
    v.iter().map(|s| s.as_bytes().to_vec()).collect()
}

fn run(sb: &Sandbox, v: &[&str]) -> kernel::RunOutput {
    sb.run(RunRequest { argv: argv(v), timeout: Some(Duration::from_secs(20)), ..RunRequest::default() }).unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn echo_runs_and_exits_zero() {
    let sb = sandbox();
    let r = run(&sb, &["echo", "hello", "world"]);
    assert_eq!(text(&r.stdout), "hello world\n");
    assert_eq!(r.status, WaitStatus::Exited(0));
    let r = run(&sb, &["false"]);
    assert_eq!(r.status, WaitStatus::Exited(1));
}

#[test]
fn stdin_reaches_the_process() {
    let sb = sandbox();
    let big: Vec<u8> = (0..300_000u32).map(|i| b'a' + (i % 26) as u8).collect();
    let r = sb.run(RunRequest { argv: argv(&["cat"]), stdin: big.clone(), ..RunRequest::default() }).unwrap();
    assert_eq!(r.stdout.len(), big.len());
    assert_eq!(r.stdout, big);
}

#[test]
fn yes_head_terminates_with_sigpipe() {
    let sb = sandbox();
    let r = run(&sb, &["pipeline", "yes", "|", "head", "-c", "10"]);
    assert_eq!(text(&r.stdout), "y\ny\ny\ny\ny\n");
    assert_eq!(text(&r.stderr), "yes: signal PIPE\nhead: exit 0\n");
    assert_eq!(r.status, WaitStatus::Exited(0));
}

#[test]
fn three_stage_pipeline() {
    let sb = sandbox();
    sb.fs().write_file(b"/tmp/in", b"one\ntwo\n", 0o644).unwrap();
    let r = run(&sb, &["pipeline", "cat", "/tmp/in", "|", "cat", "|", "cat"]);
    assert_eq!(text(&r.stdout), "one\ntwo\n");
    assert_eq!(text(&r.stderr), "cat: exit 0\ncat: exit 0\ncat: exit 0\n");
}

#[test]
fn redirection_by_fd_actions() {
    let sb = sandbox();
    let sp = sb
        .spawn(
            SpawnSpec {
                path: b"/usr/bin/echo".to_vec(),
                argv: argv(&["echo", "to", "file"]),
                attrs: ProcAttrs {
                    fd_actions: vec![FdAction::Open {
                        fd: Fd::STDOUT,
                        path: b"/tmp/out.txt".to_vec(),
                        flags: OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC,
                        mode: 0o644,
                    }],
                    ..ProcAttrs::default()
                },
            },
            kernel::HostStdio::default(),
        )
        .unwrap();
    let (st, _) = sb.wait(sp.pid, None).unwrap().unwrap();
    assert_eq!(st, WaitStatus::Exited(0));
    assert_eq!(sb.fs().read_file(b"/tmp/out.txt").unwrap(), b"to file\n");
    let st = sb.fs().stat(b"/tmp/out.txt").unwrap();
    assert_eq!(st.mode & 0o777, 0o644, "umask 022");
}

#[test]
fn kill_process_blocked_in_read() {
    let sb = sandbox();
    let mut sp = sb
        .spawn(SpawnSpec { path: b"/usr/bin/cat".to_vec(), argv: argv(&["cat"]), attrs: ProcAttrs::default() }, kernel::HostStdio::default())
        .unwrap();
    // Espera o cat bloquear no read.
    let deadline = Instant::now() + Duration::from_secs(5);
    while sb.processes().iter().find(|p| p.pid == sp.pid).map(|p| p.state) != Some('S') {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let t0 = Instant::now();
    sb.kill(KillTarget::Pid(sp.pid), Signal::SIGTERM).unwrap();
    let (st, _) = sb.wait(sp.pid, Some(Instant::now() + Duration::from_secs(5))).unwrap().unwrap();
    assert_eq!(st, WaitStatus::Signaled { signal: Signal::SIGTERM, core_dumped: false });
    assert!(t0.elapsed() < Duration::from_secs(1));
    // O stdout do cat fechou com a morte.
    let (_, eof) = sp.stdout.as_mut().unwrap().read_to_end(Some(Instant::now() + Duration::from_secs(1)));
    assert!(eof);
}

#[test]
fn zombie_until_wait() {
    let sb = sandbox();
    let r = run(&sb, &["scenario", "zombie"]);
    assert_eq!(text(&r.stdout), "true exit 3 Err(ECHILD) true\n", "stderr: {}", text(&r.stderr));
}

#[test]
fn orphan_is_adopted_by_init() {
    let sb = sandbox();
    let r = run(&sb, &["scenario", "orphan"]);
    assert_eq!(text(&r.stdout), "exit 0\nErr(ECHILD)\n", "stderr: {}", text(&r.stderr));
    let deadline = Instant::now() + Duration::from_secs(5);
    let content = loop {
        if let Ok(c) = sb.fs().read_file(b"/tmp/orphan")
            && !c.is_empty()
        {
            break c;
        }
        assert!(Instant::now() < deadline, "o neto não escreveu");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(text(&content), "true 1\n");
}

#[test]
fn sigpipe_ignored_gives_epipe() {
    let sb = sandbox();
    let r = run(&sb, &["scenario", "sigpipe-ignored"]);
    assert_eq!(text(&r.stdout), "Err(EPIPE)\n");
}

#[test]
fn caught_signal_interrupts_read_with_eintr() {
    let sb = sandbox();
    let r = run(&sb, &["scenario", "eintr"]);
    assert_eq!(text(&r.stdout), "Err(EINTR) [SIGUSR1] exit 0\n", "stderr: {}", text(&r.stderr));
}

#[test]
fn exit_unwinds_and_runs_drops() {
    let sb = sandbox();
    let r = run(&sb, &["scenario", "exit-drop"]);
    assert_eq!(text(&r.stdout), "drop ran\ndrop ran\ndrop ran\n");
    assert_eq!(r.status, WaitStatus::Exited(42));
}

#[test]
fn tcp_loopback_listen_connect_shutdown() {
    let sb = sandbox();
    let r = run(&sb, &["scenario", "tcp"]);
    assert_eq!(text(&r.stdout), "HELLO TCP Err(EADDRINUSE) Err(ECONNREFUSED) true true true\n", "stderr: {}", text(&r.stderr));
}

/// Roda um cenário de seqpacket e confere a linha de saída.
fn assert_scenario(name: &str, expected: &str) {
    let r = run(&sandbox(), &["scenario", name]);
    assert_eq!(text(&r.stdout), expected, "stderr: {}", text(&r.stderr));
}

#[test]
fn seqpacket_keeps_message_boundaries() {
    assert_scenario("seq-boundary", "abc defgh\n");
}

#[test]
fn seqpacket_truncates_and_discards_the_rest() {
    assert_scenario("seq-truncate", "0123 next abc zz\n");
}

#[test]
fn seqpacket_peek_keeps_the_message() {
    assert_scenario("seq-peek", "keep ke keep Err(EAGAIN)\n");
}

#[test]
fn seqpacket_empty_message_is_not_eof() {
    assert_scenario("seq-empty-message", "0 true false 0 Err(EAGAIN) false true 0\n");
}

#[test]
fn seqpacket_message_above_sndbuf_is_emsgsize() {
    assert_scenario("seq-msgsize", "Err(EMSGSIZE) Ok(212960)\n");
}

#[test]
fn seqpacket_full_queue_is_eagain_until_read() {
    assert_scenario("seq-full-queue", "true EAGAIN false Ok(1)\n");
}

#[test]
fn seqpacket_close_with_unread_resets_the_peer() {
    assert_scenario("seq-reset-on-unread-close", "true Err(ECONNRESET) Ok(0) Ok(0)\n");
}

#[test]
fn seqpacket_send_after_shutdown_is_epipe() {
    assert_scenario("seq-shutdown-epipe", "Err(EPIPE) Err(EPIPE) Ok(0)\n");
}

#[test]
fn seqpacket_data_survives_sender_close() {
    assert_scenario("seq-data-after-sender-close", "one two 0\n");
}

#[test]
fn seqpacket_raw_fd_sees_exact_bytes_without_framing() {
    assert_scenario("seq-raw-bytes", "true true 1\n");
}

// Linux 6.12 (net/ipv4/tcp.c, af_unix.c): os valores esperados abaixo saem do código-fonte, conferidos um a um
// com o `sk_state`, o `sk_shutdown` e o `sk_err` do socket em cada passo.

#[test]
fn tcp_send_without_connection_is_epipe_with_sigpipe() {
    // `sk_stream_wait_connect` em CLOSE e em LISTEN: EPIPE, SIGPIPE salvo MSG_NOSIGNAL; `write(2)` é `send` sem flags.
    assert_scenario("tcp-send-unconnected", "Err(EPIPE) 1 Err(EPIPE) 0 Err(EPIPE) 1 Err(EPIPE) 1\n");
}

#[test]
fn tcp_recv_without_connection_is_enotconn_and_shut_rd_reads_zero() {
    // CLOSE e LISTEN: ENOTCONN. Depois de `shutdown(SHUT_RD)`: "abc" (já na fila) e depois 0; o par ainda escreve (3).
    assert_scenario("tcp-recv-unconnected", "Err(ENOTCONN) Err(ENOTCONN) Err(ENOTCONN) abc Ok(0) Ok(3)\n");
}

#[test]
fn tcp_refused_connect_gives_error_once_then_eof() {
    // EINPROGRESS; poll `IN|OUT|HUP|RDHUP|ERR`; recv: ECONNREFUSED uma vez e depois 0; SO_ERROR 0; send EPIPE;
    // poll sem ERR; SO_ERROR lido antes: ECONNREFUSED e recv 0; `connect` bloqueante recusado: ECONNREFUSED e
    // depois ENOTCONN (o `tcp_disconnect` zera o `sk_shutdown`).
    assert_scenario("tcp-refused", "Err(EINPROGRESS) true Err(ECONNREFUSED) Ok(0) Ok(0) Err(EPIPE) true Ok(true) Ok(0) Err(ECONNREFUSED) Err(ENOTCONN)\n");
}

#[test]
fn tcp_close_with_unread_data_resets_the_peer() {
    // RST em ESTABLISHED: poll com ERR e HUP; recv ECONNRESET e então 0; send então EPIPE; SO_ERROR ECONNRESET e
    // recv 0; send ECONNRESET sem SIGPIPE e então EPIPE com SIGPIPE.
    assert_scenario("tcp-rst", "true Err(ECONNRESET) Ok(0) Err(EPIPE) Ok(true) Ok(0) Err(ECONNRESET) 0 Err(EPIPE) 1\n");
}

#[test]
fn tcp_write_after_peer_close_is_epipe_on_the_next_write() {
    // FIN: poll `IN|OUT|RDHUP`; a primeira escrita é aceita; o RST em CLOSE_WAIT deixa `sk_err = EPIPE`: recv 0,
    // poll com ERR e HUP, SO_ERROR EPIPE; sem o SO_ERROR, a escrita seguinte dá EPIPE com SIGPIPE.
    assert_scenario("tcp-peer-closed", "true Ok(1) Ok(0) true Ok(true) Ok(true) Err(EPIPE) 1\n");
}

#[test]
fn unix_stream_write_to_closed_peer_is_immediate_epipe() {
    assert_scenario("unix-stream-epipe", "Err(EPIPE) 1 Err(EPIPE) 0 Err(EPIPE) 1\n");
}

#[test]
fn tcp_recv_peek_dontwait_and_waitall() {
    assert_scenario(
        "tcp-recv-flags",
        "Err(EAGAIN) hel hello hello Err(EAGAIN) hello ab Ok(0) xy Err(EAGAIN) abcde abcde\n",
    );
}

#[test]
fn socket_info_and_options_are_read_back() {
    assert_scenario(
        "sock-info-opts",
        "[(2, 1, 6, false), (10, 1, 6, false), (2, 1, 6, true), (2, 2, 17, false), (1, 2, 0, false), (1, 1, 0, false)] Some([1, 0, 0, 0]) Some([1, 0, 0, 0]) None Err(ENOTSOCK)\n",
    );
}

#[test]
fn host_service_serves_connections_from_the_sandbox() {
    use std::io::{Read, Write};
    let sb = sandbox();
    // Eco em maiúsculas: lê até o FIN do convidado e responde; soltar o stream manda o FIN de volta.
    sb.host_service(
        "127.0.0.80".parse().unwrap(),
        4443,
        std::sync::Arc::new(|mut st: kernel::HostStream| {
            let mut data = Vec::new();
            st.read_to_end(&mut data).unwrap();
            st.write_all(&data.to_ascii_uppercase()).unwrap();
        }),
    )
    .unwrap();
    // A porta é do serviço: o mesmo endereço não registra duas vezes.
    let again = sb.host_service("127.0.0.80".parse().unwrap(), 4443, std::sync::Arc::new(|_s: kernel::HostStream| {}));
    assert_eq!(again, Err(Errno::EADDRINUSE));
    let r = run(&sb, &["scenario", "host-service"]);
    assert_eq!(
        text(&r.stdout),
        "PING NAMED Err(ECONNREFUSED) Err(EADDRINUSE) Err(EADDRINUSE) true true\n",
        "stderr: {}",
        text(&r.stderr)
    );
}

#[test]
fn net_connect_resolves_names_by_etc_hosts() {
    let sb = sandbox();
    sb.fs().write_file(b"/etc/hosts", b"127.0.0.1\tlocalhost\n::1\tlocalhost ip6-localhost ip6-loopback\n# 10.9.9.9 commented\n127.0.0.9 other alias.test # c\n10.1.1.1 remote.test\n", 0o644).unwrap();
    let r = run(&sb, &["scenario", "net-names"]);
    assert_eq!(
        text(&r.stdout),
        "Err(ECONNREFUSED) Err(ECONNREFUSED) Err(ECONNREFUSED) Err(ECONNREFUSED) Err(ECONNREFUSED) Err(EACCES) Err(EACCES) Err(ECONNREFUSED)\n",
        "stderr: {}",
        text(&r.stderr)
    );
}

#[test]
fn image_has_the_pypi_mirror_client_files() {
    let sb = sandbox();
    let fs = sb.fs();
    let ca: &[u8] = include_bytes!("../../mirror/certs/ca.crt");
    assert!(text(&fs.read_file(b"/etc/hosts").unwrap()).ends_with("\n127.0.0.80\tpypi.sandbox\n"));
    assert_eq!(fs.read_file(b"/etc/pip.conf").unwrap(), b"[global]\nindex-url = https://pypi.sandbox/simple/\n");
    assert_eq!(fs.stat(b"/etc/pip.conf").unwrap().mode & 0o7777, 0o644);
    assert_eq!(fs.read_file(b"/usr/local/share/ca-certificates/pypi-sandbox.crt").unwrap(), ca);
    assert_eq!(fs.readlink(b"/etc/ssl/certs/pypi-sandbox.pem").unwrap(), b"/usr/local/share/ca-certificates/pypi-sandbox.crt");
    assert_eq!(fs.readlink(b"/etc/ssl/certs/f1de26c0.0").unwrap(), b"pypi-sandbox.pem");
    assert!(fs.read_file(b"/etc/ssl/certs/ca-certificates.crt").unwrap().ends_with(ca));
}

#[test]
fn threads_join_and_exit_group() {
    let sb = sandbox();
    let r = run(&sb, &["scenario", "threads"]);
    assert_eq!(text(&r.stdout), "abcd true true true\n", "stderr: {}", text(&r.stderr));
    let r = run(&sb, &["scenario", "thread-exit"]);
    assert_eq!(r.status, WaitStatus::Exited(7));
}

#[test]
fn devices_and_proc_fd() {
    let sb = sandbox();
    let r = run(&sb, &["scenario", "dev"]);
    let out = text(&r.stdout);
    assert!(out.starts_with("0 [0, 0, 0, 0] Err(ENOSPC) Err(ENXIO) pipe:["), "{out} {}", text(&r.stderr));
}

#[test]
fn dev_stdin_reopens_the_pipe() {
    let sb = sandbox();
    let r = sb.run(RunRequest { argv: argv(&["cat", "/dev/stdin"]), stdin: b"via proc\n".to_vec(), ..RunRequest::default() }).unwrap();
    assert_eq!(text(&r.stdout), "via proc\n", "{}", text(&r.stderr));
}

#[test]
fn exec_errors_and_success() {
    let sb = sandbox();
    let r = run(&sb, &["scenario", "exec"]);
    assert_eq!(text(&r.stdout), "ENOENT\nEACCES\nafter exec\n");
    assert_eq!(r.status, WaitStatus::Exited(0));
}

#[test]
fn shebang_scripts() {
    let sb = sandbox();
    sb.fs().write_file(b"/tmp/s.sh", b"#!/usr/bin/echo hi\n", 0o755).unwrap();
    let r = run(&sb, &["/tmp/s.sh", "a", "b"]);
    assert_eq!(text(&r.stdout), "hi /tmp/s.sh a b\n");
    sb.fs().write_file(b"/tmp/noexec", b"#!/usr/bin/echo\n", 0o644).unwrap();
    let e = sb.run(RunRequest { argv: argv(&["/tmp/noexec"]), ..RunRequest::default() }).unwrap_err();
    assert_eq!(e, Errno::EACCES);
    sb.fs().write_file(b"/tmp/plain", b"just text\n", 0o755).unwrap();
    let e = sb.run(RunRequest { argv: argv(&["/tmp/plain"]), ..RunRequest::default() }).unwrap_err();
    assert_eq!(e, Errno::ENOEXEC);
    // Cópia do binário embutido continua executável.
    let bin = sb.fs().read_file(b"/usr/bin/echo").unwrap();
    sb.fs().write_file(b"/tmp/myecho", &bin, 0o755).unwrap();
    let r = run(&sb, &["/tmp/myecho", "copied"]);
    assert_eq!(text(&r.stdout), "copied\n");
    let e = sb.run(RunRequest { argv: argv(&["nosuchcmd"]), ..RunRequest::default() }).unwrap_err();
    assert_eq!(e, Errno::ENOENT);
}

#[test]
fn image_has_debian_layout() {
    let sb = sandbox();
    let fs = sb.fs();
    assert_eq!(fs.readlink(b"/bin").unwrap(), b"usr/bin");
    assert_eq!(fs.stat(b"/tmp").unwrap().mode & 0o7777, 0o1777);
    assert_eq!(fs.stat(b"/root").unwrap().mode & 0o7777, 0o700);
    assert!(text(&fs.read_file(b"/etc/os-release").unwrap()).contains("ID=debian\n"));
    assert!(text(&fs.read_file(b"/etc/passwd").unwrap()).starts_with("root:x:0:0:root:/root:/bin/bash\n"));
    assert_eq!(fs.lstat(b"/dev/null").unwrap().rdev, 0x103);
    assert_eq!(fs.readlink(b"/dev/stdin").unwrap(), b"/proc/self/fd/0");
    assert!(fs.stat(b"/usr/bin/cat").unwrap().mode & 0o111 != 0);
    let mounts = text(&fs.read_file(b"/proc/mounts").unwrap_or_default());
    let _ = mounts;
}

#[test]
fn snapshot_and_restore() {
    let sb = sandbox();
    let fs = sb.fs();
    fs.mkdir_all(b"/work/case", 0o755).unwrap();
    fs.write_file(b"/work/case/a.txt", b"one", 0o644).unwrap();
    let snap = sb.snapshot();
    fs.write_file(b"/work/case/a.txt", b"two", 0o600).unwrap();
    fs.symlink(b"a.txt", b"/work/case/link").unwrap();
    let tree = fs.tree(b"/work/case").unwrap();
    assert_eq!(
        tree,
        vec![
            (b"a.txt".to_vec(), TreeEntry::File { data: b"two".to_vec(), mode: 0o600 }),
            (b"link".to_vec(), TreeEntry::Symlink { target: b"a.txt".to_vec() }),
        ]
    );
    sb.restore(&snap);
    assert_eq!(fs.read_file(b"/work/case/a.txt").unwrap(), b"one");
    assert_eq!(fs.lstat(b"/work/case/link").unwrap_err(), Errno::ENOENT);
    // Sandbox derivado do snapshot.
    let k = Kernel::new(KernelConfig::default());
    let sb2 = k.create_sandbox_from(&snap, SandboxConfig { programs: programs(), ..SandboxConfig::default() }).unwrap();
    assert_eq!(sb2.fs().read_file(b"/work/case/a.txt").unwrap(), b"one");
    let r = run(&sb2, &["cat", "/work/case/a.txt"]);
    assert_eq!(text(&r.stdout), "one");
}

#[test]
fn timeout_kills_the_session() {
    let sb = sandbox();
    let t0 = Instant::now();
    let r = sb
        .run(RunRequest { argv: argv(&["pipeline", "yes", "|", "cat"]), timeout: Some(Duration::from_millis(300)), ..RunRequest::default() })
        .unwrap();
    assert!(r.timed_out);
    assert_eq!(r.status, WaitStatus::Signaled { signal: Signal::SIGKILL, core_dumped: false });
    assert!(t0.elapsed() < Duration::from_secs(5));
    // Ninguém sobrou.
    let deadline = Instant::now() + Duration::from_secs(5);
    while sb.usage().procs > 0 {
        assert!(Instant::now() < deadline, "{:?}", sb.processes());
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn tcp_sndbuf_grows_after_the_connection_is_established() {
    // Linux 6.12 `tcp_sndbuf_expand` no loopback: SO_SNDBUF lido depois do connect e do accept é 2626560 (não os
    // 16384 do `tcp_wmem`); com `SO_SNDBUF` definido antes (65536 vira 131072) o buffer fica travado, e o socket
    // aceito herda o valor do que escuta (8192 vira 16384); sem valor definido no que escuta, o aceito também cresce.
    assert_scenario("tcp-sndbuf-expand", "None Some(2626560) Some(2626560) Some(131072) Some(2626560) Some(16384)\n");
}

#[test]
fn shutdown_wakes_a_blocked_recv_and_a_blocked_accept() {
    // `recv` bloqueado e `shutdown(SHUT_RD)` do mesmo socket por outra thread: 0. `shutdown(SHUT_RD)` num
    // socket em escuta: o `accept` bloqueado dá EINVAL, o `connect` seguinte ECONNREFUSED, `SO_ACCEPTCONN` 0 e
    // o poll `OUT|HUP`.
    assert_scenario("tcp-shutdown-wakes", "Ok(0) Err(EINVAL) Err(ECONNREFUSED) false true\n");
}

#[test]
fn unix_stream_shutdown_rd_keeps_the_queue_and_refuses_the_peer() {
    // `unix_shutdown(SHUT_RD)`: "abc" ainda sai e depois vem 0; o par escreve e recebe EPIPE; poll `IN|OUT|RDHUP`
    // e HUP só com os dois sentidos encerrados; um `recv` bloqueado acorda com 0.
    assert_scenario("unix-shutdown-rd", "abc Ok(0) Err(EPIPE) true true Ok(0)\n");
}

#[test]
fn unix_connect_waiting_for_the_queue_wakes_when_the_listener_closes() {
    // Fila cheia (backlog 0, uma conexão além dele já está nela): o segundo `connect` espera vaga e, com o
    // socket que escuta fechado por outra thread, dá ECONNREFUSED.
    assert_scenario("unix-connect-wakes", "Ok(()) Err(ECONNREFUSED)\n");
}

#[test]
fn unix_stream_shutdown_of_an_unconnected_socket_succeeds() {
    assert_scenario("unix-shutdown-unconnected", "Ok(()) Ok(())\n");
}

#[test]
fn unix_listener_close_resets_the_pending_client() {
    assert_scenario("unix-listener-close-resets", "Ok(()) Err(ECONNRESET) Ok(0)\n");
}

#[test]
fn tcp_listener_close_resets_the_pending_client() {
    assert_scenario("tcp-listener-close-resets", "Err(ECONNRESET) Ok(0)\n");
}

#[test]
fn socket_rcvbuf_does_not_grow_after_connect() {
    // Linux 6.12 mede 131072 antes e depois do connect (o `SO_RCVBUF` não cresce, ao contrário do `SO_SNDBUF`).
    assert_scenario("socket-rcvbuf-fixed", "None None None\n");
}

#[test]
fn scm_rights_passes_a_pipe_end_over_a_unix_stream() {
    assert_scenario("scm-rights-stream", "Ok(1) x 1 1 0 hi\n");
}

#[test]
fn scm_rights_stream_read_stops_at_the_buffer_with_descriptors() {
    assert_scenario("scm-rights-stream-boundaries", "abcd 1 ef 0\n");
}

#[test]
fn scm_rights_are_delivered_with_the_first_partial_read() {
    assert_scenario("scm-rights-stream-partial-read", "wx 1 yz 0\n");
}

#[test]
fn scm_rights_without_room_in_msg_control_are_lost_with_ctrunc() {
    assert_scenario("scm-rights-truncated", "8/0 8/0 8/0 0/20\n");
}

#[test]
fn scm_rights_peek_on_a_datagram_installs_copies_and_keeps_the_message() {
    assert_scenario("scm-rights-dgram-peek", "true true true Err(EAGAIN)\n");
}

#[test]
fn scm_send_validates_the_control_message_like_scm_send() {
    assert_scenario("scm-rights-errors", "Err(EBADF) Err(EINVAL) Ok(1) Err(EINVAL) Err(EINVAL)\n");
}

#[test]
fn so_passcred_delivers_scm_credentials_on_every_unix_type() {
    assert_scenario("scm-credentials", "0/true 0/true 0/true\n");
}

#[test]
fn so_peercred_follows_listen_connect_and_socketpair() {
    assert_scenario("peercred", "true true true true\n");
}
