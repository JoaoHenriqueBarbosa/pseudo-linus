//! Testes de integração do `pidfd_open(2)` do kernel: o fd fica legível quando o processo vira zumbi, ganha
//! `HUP` quando o pai o colhe, e entra no `poll` e no `epoll`.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, Sandbox, SandboxConfig};
use sysabi::epoll as ev;
use sysabi::sys::{self, write_all};
use sysabi::*;

/// Os bits que o `poll(2)` vê no fd, sem esperar.
fn revents(fd: Fd) -> u16 {
    let mut p = [PollFd { fd, events: PollEvents::IN, revents: PollEvents::empty() }];
    sys::current().poll(&mut p, Some(Duration::ZERO)).unwrap();
    p[0].revents.bits()
}

fn p_pidfd(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let s = sys::current();
    let name = String::from_utf8_lossy(args[1].as_bytes()).into_owned();
    let out = |t: String| write_all(Fd::STDOUT, format!("{t}\n").as_bytes()).unwrap();
    match name.as_str() {
        // Legível só depois que o filho sai; `HUP` junto só depois do `wait4`.
        "lifecycle" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let pid = s.spawn_fn(ProcAttrs::default(), b"child".to_vec(), Box::new(move || {
                let mut b = [0u8; 1];
                let _ = sys::current().read(r, &mut b);
                0
            })).unwrap();
            let fd = s.pidfd_open(pid, 0).unwrap();
            let alive = revents(fd);
            s.write(w, b"x").unwrap();
            let mut p = [PollFd { fd, events: PollEvents::IN, revents: PollEvents::empty() }];
            s.poll(&mut p, Some(Duration::from_secs(5))).unwrap();
            let exited = p[0].revents.bits();
            s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap().unwrap();
            let reaped = revents(fd);
            out(format!("{alive:#x} {exited:#x} {reaped:#x}"));
            0
        }
        // O pidfd entra num epoll e acorda a espera bloqueada.
        "epoll" => {
            let (r, w) = s.pipe2(OFlags::empty()).unwrap();
            let pid = s.spawn_fn(ProcAttrs::default(), b"child".to_vec(), Box::new(move || {
                let mut b = [0u8; 1];
                let _ = sys::current().read(r, &mut b);
                5
            })).unwrap();
            let fd = s.pidfd_open(pid, 0).unwrap();
            let ep = s.epoll_create1(true).unwrap();
            s.epoll_ctl(ep, ev::CTL_ADD, fd, EpollEvent { events: ev::IN, data: 9 }).unwrap();
            let idle = s.epoll_wait(ep, 4, Some(Duration::ZERO)).unwrap().len();
            s.write(w, b"x").unwrap();
            let hit = s.epoll_wait(ep, 4, Some(Duration::from_secs(5))).unwrap();
            let (_, st) = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap().unwrap();
            out(format!("{idle} {}:{:#x} {st:?}", hit[0].data, hit[0].events));
            0
        }
        // Erros: pid inexistente, flags inválidas, pid não positivo; `read` dá EINVAL e o fd é anon_inode.
        "errors" => {
            let none = s.pidfd_open(999_999, 0).unwrap_err();
            let flags = s.pidfd_open(s.getpid(), 0x8000_0000).unwrap_err();
            let zero = s.pidfd_open(0, 0).unwrap_err();
            let fd = s.pidfd_open(s.getpid(), 0).unwrap();
            let mut b = [0u8; 1];
            let read = s.read(fd, &mut b).unwrap_err();
            let link = s.readlinkat(Fd::CWD, format!("/proc/self/fd/{}", fd.0).as_bytes()).unwrap();
            let info = sys::read_file(format!("/proc/self/fdinfo/{}", fd.0).as_bytes()).unwrap();
            let pid = s.getpid();
            let has = String::from_utf8_lossy(&info).contains(&format!("Pid:\t{pid}\nNSpid:\t{pid}\n"));
            out(format!("{none:?} {flags:?} {zero:?} {read:?} {} {has}", String::from_utf8_lossy(&link)));
            0
        }
        // Filho que já virou zumbi: o pidfd abre e já está legível.
        "zombie" => {
            let pid = s.spawn_fn(ProcAttrs::default(), b"child".to_vec(), Box::new(|| 0)).unwrap();
            loop {
                if s.list_processes().into_iter().find(|p| p.pid == pid).map(|p| p.state) == Some('Z') {
                    break;
                }
                s.nanosleep(Duration::from_millis(2)).unwrap();
            }
            let fd = s.pidfd_open(pid, 0).unwrap();
            let before = revents(fd);
            s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap().unwrap();
            out(format!("{before:#x} {:#x} {:?}", revents(fd), s.pidfd_open(pid, 0).unwrap_err()));
            0
        }
        _ => 2,
    }
}

fn scenario(name: &str) -> String {
    let k = Kernel::new(KernelConfig::default());
    let sb = k.create_sandbox(SandboxConfig { programs: vec![Program::bin("pidfd_scenario", p_pidfd)], ..SandboxConfig::default() }).unwrap();
    let argv = ["pidfd_scenario", name].iter().map(|s| s.as_bytes().to_vec()).collect();
    let r = sb.run(RunRequest { argv, timeout: Some(Duration::from_secs(20)), ..RunRequest::default() }).unwrap();
    assert_eq!(r.status, WaitStatus::Exited(0), "{name}: {}", String::from_utf8_lossy(&r.stderr));
    String::from_utf8_lossy(&r.stdout).trim_end().to_string()
}

#[test]
fn pidfd_turns_readable_on_exit_and_hangs_up_when_reaped() {
    assert_eq!(scenario("lifecycle"), "0x0 0x1 0x11");
}

#[test]
fn pidfd_wakes_epoll() {
    assert_eq!(scenario("epoll"), "0 9:0x1 Exited(5)");
}

#[test]
fn pidfd_errors_and_proc_links() {
    assert_eq!(scenario("errors"), "ESRCH EINVAL EINVAL EINVAL anon_inode:[pidfd] true");
}

#[test]
fn pidfd_of_a_zombie_is_readable_and_of_a_reaped_pid_is_esrch() {
    assert_eq!(scenario("zombie"), "0x1 0x11 ESRCH");
}
