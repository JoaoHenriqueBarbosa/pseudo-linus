//! Testes de integração do `epoll(7)` do kernel: `epoll_create1`, `epoll_ctl` e `epoll_wait` sobre pipes,
//! sockets e outros epolls, dentro de um processo do sandbox.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::time::{Duration, Instant};

use kernel::{Kernel, KernelConfig, RunRequest, Sandbox, SandboxConfig};
use sysabi::epoll as ev;
use sysabi::sys::{self, write_all};
use sysabi::*;

fn event(events: u32, data: u64) -> EpollEvent {
    EpollEvent { events, data }
}

/// Os eventos de um `epoll_wait` sem esperar, como texto `data:0xEVENTOS` separados por espaço.
fn ready(epfd: Fd) -> String {
    let r = sys::current().epoll_wait(epfd, 16, Some(Duration::ZERO)).unwrap();
    r.iter().map(|e| format!("{}:{:#x}", e.data, e.events)).collect::<Vec<_>>().join(" ")
}

fn errno_of<T>(r: Result<T, Errno>) -> String {
    match r {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("{e:?}"),
    }
}

fn p_epoll(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let s = sys::current();
    let name = String::from_utf8_lossy(args[1].as_bytes()).into_owned();
    let out = |t: String| write_all(Fd::STDOUT, format!("{t}\n").as_bytes()).unwrap();
    match name.as_str() {
        // Nível: o evento volta enquanto o dado não for lido; só os bits pedidos voltam (`EPOLLRDNORM` não).
        "level" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, r, event(ev::IN, 7)).unwrap();
            let none = ready(ep);
            s.write(w, b"x").unwrap();
            let first = ready(ep);
            let again = ready(ep);
            let mut b = [0u8; 8];
            s.read(r, &mut b).unwrap();
            let drained = ready(ep);
            out(format!("[{none}] [{first}] [{again}] [{drained}]"));
            0
        }
        // `EPOLLERR` e `EPOLLHUP` vêm sem pedir: o fim da escrita aparece como HUP.
        "hup" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, r, event(ev::IN, 1)).unwrap();
            s.close(w).unwrap();
            out(format!("[{}]", ready(ep)));
            0
        }
        // O ponto de escrita de um pipe vazio está pronto para `EPOLLOUT`.
        "out" => {
            let (_r, w) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, w, event(ev::OUT, 9)).unwrap();
            out(format!("[{}]", ready(ep)));
            0
        }
        // `EPOLLET`: só a transição para pronto; sem ler, a segunda espera não vê nada.
        "edge" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, r, event(ev::IN | ev::ET, 3)).unwrap();
            s.write(w, b"x").unwrap();
            let first = ready(ep);
            let second = ready(ep);
            // `EPOLL_CTL_MOD` reavalia a entrada.
            s.epoll_ctl(ep, ev::CTL_MOD, r, event(ev::IN | ev::ET, 3)).unwrap();
            let after_mod = ready(ep);
            out(format!("[{first}] [{second}] [{after_mod}]"));
            0
        }
        // `EPOLLONESHOT`: um evento e a entrada desarma, até o `EPOLL_CTL_MOD`.
        "oneshot" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, r, event(ev::IN | ev::ONESHOT, 4)).unwrap();
            s.write(w, b"x").unwrap();
            let first = ready(ep);
            let second = ready(ep);
            s.epoll_ctl(ep, ev::CTL_MOD, r, event(ev::IN | ev::ONESHOT, 4)).unwrap();
            let rearmed = ready(ep);
            out(format!("[{first}] [{second}] [{rearmed}]"));
            0
        }
        // Os erros do `epoll_ctl` e do `epoll_wait` e a ordem em que o kernel os confere.
        "errors" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            let reg = s.epoll_ctl(ep, ev::CTL_ADD, r, event(ev::IN, 0));
            let dup = s.epoll_ctl(ep, ev::CTL_ADD, r, event(ev::IN, 0));
            let del_missing = s.epoll_ctl(ep, ev::CTL_DEL, w, event(0, 0));
            let mod_missing = s.epoll_ctl(ep, ev::CTL_MOD, w, event(ev::IN, 0));
            let bad_op = s.epoll_ctl(ep, 9, w, event(ev::IN, 0));
            let on_self = s.epoll_ctl(ep, ev::CTL_ADD, ep, event(ev::IN, 0));
            let not_epoll = s.epoll_ctl(r, ev::CTL_ADD, w, event(ev::IN, 0));
            let bad_epfd = s.epoll_ctl(Fd(99), ev::CTL_ADD, r, event(ev::IN, 0));
            let bad_fd = s.epoll_ctl(ep, ev::CTL_ADD, Fd(99), event(ev::IN, 0));
            let file = sys::open(b"/tmp/regular", OFlags::RDONLY, 0).unwrap();
            let regular = s.epoll_ctl(ep, ev::CTL_ADD, file, event(ev::IN, 0));
            let exclusive_mod = s.epoll_ctl(ep, ev::CTL_MOD, r, event(ev::IN | ev::EXCLUSIVE, 0));
            let wait_zero = s.epoll_wait(ep, 0, Some(Duration::ZERO));
            let wait_bad = s.epoll_wait(Fd(99), 1, Some(Duration::ZERO));
            let wait_not_epoll = s.epoll_wait(r, 1, Some(Duration::ZERO));
            let read = s.read(ep, &mut [0u8; 4]);
            let write = s.write(ep, b"x");
            let seek = s.lseek(ep, 0, Whence::Set);
            out(
                [reg, dup, del_missing, mod_missing, bad_op, on_self, not_epoll, bad_epfd, bad_fd, regular, exclusive_mod]
                    .into_iter()
                    .map(errno_of)
                    .chain([errno_of(wait_zero), errno_of(wait_bad), errno_of(wait_not_epoll), errno_of(read), errno_of(write), errno_of(seek)])
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            0
        }
        // O fd aparece em `/proc/self/fd` como `anon_inode:[eventpoll]` e abre como arquivo regular `0600`.
        "link" => {
            let ep = s.epoll_create1(true).unwrap();
            let target = s.readlinkat(Fd::CWD, format!("/proc/self/fd/{}", ep.0).as_bytes()).unwrap();
            let st = s.fstat(ep).unwrap();
            let info = sys::read_file(format!("/proc/self/fdinfo/{}", ep.0).as_bytes()).unwrap();
            let flags = String::from_utf8_lossy(&info).lines().find(|l| l.starts_with("flags:")).unwrap_or("").to_string();
            out(format!("{} {:o} {}", String::from_utf8_lossy(&target), st.mode, flags));
            0
        }
        // O inode anônimo do oráculo: `st_dev` 16, `st_ino` 58 (o mesmo de todo anon_inode), modo `0600` sem bits de
        // tipo, e o `fdinfo` sem entradas só com as quatro linhas comuns.
        "stat" => {
            let ep = s.epoll_create1(true).unwrap();
            let st = s.fstat(ep).unwrap();
            let info = sys::read_file(format!("/proc/self/fdinfo/{}", ep.0).as_bytes()).unwrap();
            let info = String::from_utf8_lossy(&info).replace('\n', "/").replace('\t', " ");
            out(format!("{} {} {} {} {} {:o} {}", st.dev, st.ino, st.nlink, st.size, st.blksize, st.mode, info));
            0
        }
        // `ep_show_fdinfo`: uma linha `tfd:` por entrada, com `events` (os bits que o kernel acrescenta e o `EPOLLET`),
        // `data` em 16 colunas, a posição do alvo e o inode e o dispositivo dele em hexadecimal.
        "fdinfo" => {
            let (r, _w) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, r, event(ev::IN | ev::ET, r.0 as u64)).unwrap();
            let st = s.fstat(r).unwrap();
            let info = sys::read_file(format!("/proc/self/fdinfo/{}", ep.0).as_bytes()).unwrap();
            let info = String::from_utf8_lossy(&info).into_owned();
            let line = info.lines().find(|l| l.starts_with("tfd:")).unwrap_or("").to_string();
            let want = format!("tfd: {:8} events: {:8x} data: {:16x}  pos:0 ino:{:x} sdev:{:x}", r.0, 0x8000_0019u32, r.0, st.ino, st.dev);
            out(format!("{} {}", line == want, line));
            0
        }
        // `EPOLLET` completo: chegada nova com o fd já pronto reentrega; ler parte sem chegada nova não.
        "edge_arrival" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, r, event(ev::IN | ev::ET, 3)).unwrap();
            s.write(w, b"abc").unwrap();
            let first = ready(ep);
            let mut b = [0u8; 1];
            s.read(r, &mut b).unwrap();
            let partial = ready(ep);
            s.write(w, b"d").unwrap();
            let arrived = ready(ep);
            let settled = ready(ep);
            s.write(w, b"e").unwrap();
            s.write(w, b"f").unwrap();
            let twice = ready(ep);
            out(format!("[{first}] [{partial}] [{arrived}] [{settled}] [{twice}]"));
            0
        }
        // O `ep_poll_callback` filtra pela chave: o par ler o que enviamos acorda a fila com `EPOLLOUT`, e isso não
        // reentrega um `EPOLLIN|EPOLLET` ainda pronto; a chegada de dado (`EPOLLIN`) reentrega.
        "edge_other_direction" => {
            let (a, b) = s.unix_socketpair(1, false, true).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, a, event(ev::IN | ev::ET, 5)).unwrap();
            s.write(b, b"x").unwrap();
            let first = ready(ep);
            s.write(a, b"y").unwrap();
            let mut buf = [0u8; 8];
            s.read(b, &mut buf).unwrap();
            let out_wake = ready(ep);
            s.write(b, b"z").unwrap();
            let in_wake = ready(ep);
            out(format!("[{first}] [{out_wake}] [{in_wake}]"));
            0
        }
        // `EPOLLHUP` acorda a entrada `EPOLLET` sempre (o fechamento acorda sem chave, e o `ep_insert` soma `HUP`).
        "edge_hup" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, r, event(ev::IN | ev::ET, 1)).unwrap();
            s.write(w, b"a").unwrap();
            let first = ready(ep);
            s.close(w).unwrap();
            let hup = ready(ep);
            out(format!("[{first}] [{hup}]"));
            0
        }
        // A espera sem limite de uma entrada `EPOLLET` já pronta acorda com a chegada de dado novo.
        "edge_blocking" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, r, event(ev::IN | ev::ET, 6)).unwrap();
            s.write(w, b"a").unwrap();
            let first = ready(ep);
            let writer = s
                .spawn_fn(
                    ProcAttrs::default(),
                    b"writer".to_vec(),
                    Box::new(move || {
                        let s = sys::current();
                        s.nanosleep(Duration::from_millis(60)).unwrap();
                        s.write(w, b"b").unwrap();
                        0
                    }),
                )
                .unwrap();
            let started = Instant::now();
            let r = s.epoll_wait(ep, 4, None).unwrap();
            let waited = started.elapsed() >= Duration::from_millis(50);
            s.wait4(WaitTarget::Pid(writer), WaitOptions::empty()).unwrap().unwrap();
            out(format!("[{first}] {} {} {:#x}", r.len(), waited, r[0].events));
            0
        }
        // Epoll dentro de epoll: o de fora fica pronto quando o de dentro tem evento; ciclo dá ELOOP.
        "nested" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let inner = s.epoll_create1(true).unwrap();
            let outer = s.epoll_create1(true).unwrap();
            s.epoll_ctl(inner, ev::CTL_ADD, r, event(ev::IN, 1)).unwrap();
            s.epoll_ctl(outer, ev::CTL_ADD, inner, event(ev::IN, 2)).unwrap();
            let idle = ready(outer);
            s.write(w, b"x").unwrap();
            let busy = ready(outer);
            let cycle = s.epoll_ctl(inner, ev::CTL_ADD, outer, event(ev::IN, 0));
            out(format!("[{idle}] [{busy}] {}", errno_of(cycle)));
            0
        }
        // A entrada some sozinha quando a última referência à descrição fecha; um `dup` a mantém.
        "autoremove" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, r, event(ev::IN, 5)).unwrap();
            let r2 = s.dup(r).unwrap();
            s.write(w, b"x").unwrap();
            s.close(r).unwrap();
            let kept = ready(ep);
            s.close(r2).unwrap();
            let gone = ready(ep);
            out(format!("[{kept}] [{gone}]"));
            0
        }
        // `ep_remove`, `ep_free` e `eventpoll_release`: o DEL, o close do epoll e o close do alvo tiram o parker da
        // entrada das filas do arquivo. Laços longos não degradam, o dado depois do DEL não volta nada, o epoll
        // fechado não recebe evento e um ADD novo depois do DEL entrega a borda como entrada nova.
        "del_churn" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            let et = event(ev::IN | ev::ET, 1);
            for _ in 0..5000 {
                s.epoll_ctl(ep, ev::CTL_ADD, r, et).unwrap();
                s.epoll_ctl(ep, ev::CTL_DEL, r, et).unwrap();
                s.epoll_ctl(ep, ev::CTL_ADD, r, et).unwrap();
                let _ = ready(ep);
                s.epoll_ctl(ep, ev::CTL_DEL, r, et).unwrap();
            }
            s.write(w, b"x").unwrap();
            let after_del = ready(ep);
            for _ in 0..2000 {
                let other = s.epoll_create1(true).unwrap();
                s.epoll_ctl(other, ev::CTL_ADD, r, et).unwrap();
                let _ = ready(other);
                s.close(other).unwrap();
            }
            for _ in 0..2000 {
                let (r2, w2) = s.pipe2(OFlags::empty()).unwrap();
                s.epoll_ctl(ep, ev::CTL_ADD, r2, et).unwrap();
                let _ = ready(ep);
                s.close(r2).unwrap();
                s.close(w2).unwrap();
            }
            let after_close = ready(ep);
            s.epoll_ctl(ep, ev::CTL_ADD, r, et).unwrap();
            let fresh = ready(ep);
            let again = ready(ep);
            s.write(w, b"y").unwrap();
            let arrival = ready(ep);
            out(format!("[{after_del}] [{after_close}] [{fresh}] [{again}] [{arrival}]"));
            0
        }
        // Espera sem limite acorda quando outro processo escreve.
        "blocking" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, r, event(ev::IN, 8)).unwrap();
            let writer = s
                .spawn_fn(
                    ProcAttrs::default(),
                    b"writer".to_vec(),
                    Box::new(move || {
                        let s = sys::current();
                        s.nanosleep(Duration::from_millis(60)).unwrap();
                        s.write(w, b"x").unwrap();
                        0
                    }),
                )
                .unwrap();
            let started = Instant::now();
            let r = s.epoll_wait(ep, 4, None).unwrap();
            let waited = started.elapsed() >= Duration::from_millis(50);
            s.wait4(WaitTarget::Pid(writer), WaitOptions::empty()).unwrap().unwrap();
            out(format!("{} {} {:#x}", r.len(), waited, r[0].events));
            0
        }
        // Prazo vencido sem evento: lista vazia, depois do tempo pedido.
        "timeout" => {
            let (r, _w) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, r, event(ev::IN, 1)).unwrap();
            let started = Instant::now();
            let r = s.epoll_wait(ep, 4, Some(Duration::from_millis(40))).unwrap();
            out(format!("{} {}", r.len(), started.elapsed() >= Duration::from_millis(35)));
            0
        }
        // `maxevents` limita e as entradas de nível entregues giram para o fim da fila.
        "rotate" => {
            let (r1, w1) = s.pipe2(OFlags::empty()).unwrap();
            let (r2, w2) = s.pipe2(OFlags::empty()).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, r1, event(ev::IN, 1)).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, r2, event(ev::IN, 2)).unwrap();
            s.write(w1, b"x").unwrap();
            s.write(w2, b"x").unwrap();
            let one = |ep| s.epoll_wait(ep, 1, Some(Duration::ZERO)).unwrap()[0].data;
            out(format!("{} {} {}", one(ep), one(ep), one(ep)));
            0
        }
        // Socket TCP de loopback: pronto para leitura quando o par escreve.
        "tcp" => {
            let (listener, port) = s.tcp_listen(0, 4, false, true).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, listener, event(ev::IN, 1)).unwrap();
            let idle = ready(ep);
            let (client, _) = s.tcp_connect(port, false, true).unwrap();
            let pending = ready(ep);
            let (server, _) = s.tcp_accept(listener, false, true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, server, event(ev::IN | ev::RDHUP, 2)).unwrap();
            s.write(client, b"hi").unwrap();
            let readable = ready(ep);
            out(format!("[{idle}] [{pending}] [{readable}]"));
            0
        }
        _ => 2,
    }
}

fn sandbox() -> Sandbox {
    let k = Kernel::new(KernelConfig::default());
    let sb = k.create_sandbox(SandboxConfig { programs: vec![Program::bin("epoll_scenario", p_epoll)], ..SandboxConfig::default() }).unwrap();
    sb.fs().write_file(b"/tmp/regular", b"data", 0o644).unwrap();
    sb
}

fn scenario(name: &str) -> String {
    let sb = sandbox();
    let argv = ["epoll_scenario", name].iter().map(|s| s.as_bytes().to_vec()).collect();
    let r = sb.run(RunRequest { argv, timeout: Some(Duration::from_secs(20)), ..RunRequest::default() }).unwrap();
    assert_eq!(r.status, WaitStatus::Exited(0), "{name}: {}", String::from_utf8_lossy(&r.stderr));
    String::from_utf8_lossy(&r.stdout).trim_end().to_string()
}

#[test]
fn level_triggered_reports_until_drained() {
    assert_eq!(scenario("level"), "[] [7:0x1] [7:0x1] []");
}

#[test]
fn hangup_comes_without_asking() {
    assert_eq!(scenario("hup"), "[1:0x10]");
}

#[test]
fn write_end_is_ready_for_out() {
    assert_eq!(scenario("out"), "[9:0x4]");
}

#[test]
fn edge_triggered_reports_the_transition_once() {
    assert_eq!(scenario("edge"), "[3:0x1] [] [3:0x1]");
}

#[test]
fn oneshot_disarms_until_modified() {
    assert_eq!(scenario("oneshot"), "[4:0x1] [] [4:0x1]");
}

#[test]
fn ctl_and_wait_errors_follow_the_kernel_order() {
    assert_eq!(
        scenario("errors"),
        "ok EEXIST ENOENT ENOENT EINVAL EINVAL EINVAL EBADF EBADF EPERM EINVAL EINVAL EBADF EINVAL EINVAL EINVAL ESPIPE"
    );
}

#[test]
fn fd_is_an_anonymous_inode() {
    assert_eq!(scenario("link"), "anon_inode:[eventpoll] 600 flags:\t02000002");
}

#[test]
fn anonymous_inode_matches_the_oracle() {
    assert_eq!(scenario("stat"), "16 58 1 0 4096 600 pos: 0/flags: 02000002/mnt_id: 17/ino: 58/");
}

#[test]
fn fdinfo_lists_each_target() {
    let r = scenario("fdinfo");
    assert!(r.starts_with("true tfd:        3 events: 80000019 data:                3  pos:0 ino:"), "{r}");
}

#[test]
fn edge_triggered_rearms_on_every_arrival() {
    assert_eq!(scenario("edge_arrival"), "[3:0x1] [] [3:0x1] [] [3:0x1]");
}

#[test]
fn edge_triggered_ignores_wakeups_of_other_keys() {
    assert_eq!(scenario("edge_other_direction"), "[5:0x1] [] [5:0x1]");
}

#[test]
fn edge_triggered_hangup_always_wakes() {
    assert_eq!(scenario("edge_hup"), "[1:0x1] [1:0x11]");
}

#[test]
fn edge_triggered_blocking_wait_wakes_on_new_data() {
    assert_eq!(scenario("edge_blocking"), "[6:0x1] 1 true 0x1");
}

#[test]
fn nested_epoll_is_readable_and_cycles_are_refused() {
    assert_eq!(scenario("nested"), "[] [2:0x1] ELOOP");
}

#[test]
fn entry_disappears_with_the_last_reference() {
    assert_eq!(scenario("autoremove"), "[5:0x1] []");
}

#[test]
fn removed_entries_leave_the_wait_queues() {
    assert_eq!(scenario("del_churn"), "[] [] [1:0x1] [] [1:0x1]");
}

#[test]
fn infinite_wait_wakes_on_write_from_another_process() {
    assert_eq!(scenario("blocking"), "1 true 0x1");
}

#[test]
fn expired_deadline_returns_empty() {
    assert_eq!(scenario("timeout"), "0 true");
}

#[test]
fn level_entries_rotate_after_delivery() {
    assert_eq!(scenario("rotate"), "1 2 1");
}

#[test]
fn tcp_sockets_work_with_epoll() {
    assert_eq!(scenario("tcp"), "[] [1:0x1] [2:0x1]");
}
