//! Testes de integração do `waitid(2)` e do job control do `wait4`: `WNOWAIT` deixa o evento esperável, parada e
//! continuação são relatadas uma vez só, e as validações de argumento são as do `kernel_waitid`.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::sys::{self, write_all};
use sysabi::*;

/// Um filho que espera um byte no pipe e sai com `code`.
fn blocked_child(code: i32) -> (Pid, Fd) {
    let s = sys::current();
    let (r, w) = s.pipe2(OFlags::empty()).unwrap();
    let pid = s
        .spawn_fn(ProcAttrs::default(), b"child".to_vec(), Box::new(move || {
            let mut b = [0u8; 1];
            let _ = sys::current().read(r, &mut b);
            code
        }))
        .unwrap();
    (pid, w)
}

fn p_waitid(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let s = sys::current();
    let name = String::from_utf8_lossy(args[1].as_bytes()).into_owned();
    let out = |t: String| write_all(Fd::STDOUT, format!("{t}\n").as_bytes()).unwrap();
    let exited = WaitOptions::EXITED;
    match name.as_str() {
        // `WNOWAIT` não colhe: o mesmo término sai duas vezes e depois o `wait4` o leva.
        "nowait" => {
            let pid = s.spawn_fn(ProcAttrs::default(), b"child".to_vec(), Box::new(|| 7)).unwrap();
            let opts = exited | WaitOptions::NOWAIT;
            let a = s.waitid(WaitIdTarget::Pid(pid), opts).unwrap().unwrap();
            let b = s.waitid(WaitIdTarget::Pid(pid), opts).unwrap().unwrap();
            let c = s.waitid(WaitIdTarget::All, exited).unwrap().unwrap();
            let gone = s.waitid(WaitIdTarget::Pid(pid), exited).unwrap_err();
            out(format!("{} {:?} {} {:?} {} {:?}", a.pid == pid, a.status, b.pid == pid, b.status, c.pid == pid, gone));
            0
        }
        // Parada e continuação: uma vez por `wait4`, e `waitid` com `WNOWAIT` só espia.
        "job-control" => {
            let (pid, w) = blocked_child(0);
            s.kill(KillTarget::Pid(pid), Signal::SIGSTOP).unwrap();
            let stopped = s.wait4(WaitTarget::Pid(pid), WaitOptions::UNTRACED).unwrap().unwrap();
            let again = s.wait4(WaitTarget::Pid(pid), WaitOptions::UNTRACED | WaitOptions::NOHANG).unwrap();
            s.kill(KillTarget::Pid(pid), Signal::SIGCONT).unwrap();
            let peek = WaitOptions::CONTINUED | WaitOptions::NOWAIT;
            let p1 = s.waitid(WaitIdTarget::Pid(pid), peek).unwrap().unwrap();
            let p2 = s.waitid(WaitIdTarget::Pid(pid), peek).unwrap().unwrap();
            let cont = s.wait4(WaitTarget::Pid(pid), WaitOptions::CONTINUED).unwrap().unwrap();
            let cont_again = s.wait4(WaitTarget::Pid(pid), WaitOptions::CONTINUED | WaitOptions::NOHANG).unwrap();
            s.write(w, b"x").unwrap();
            let done = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap().unwrap();
            out(format!(
                "{:?} {:?} {:?} {:?} {:?} {:?} {:?}",
                stopped.1, again, p1.status, p2.status, cont.1, cont_again, done.1
            ));
            0
        }
        // `WSTOPPED` relata a parada, e um `wait4` sem `WUNTRACED` não a vê.
        "stopped-event" => {
            let (pid, w) = blocked_child(3);
            s.kill(KillTarget::Pid(pid), Signal::SIGSTOP).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let info = loop {
                if let Some(i) = s.waitid(WaitIdTarget::Pid(pid), WaitOptions::UNTRACED | WaitOptions::NOHANG).unwrap() {
                    break i;
                }
                assert!(std::time::Instant::now() < deadline, "parada não relatada");
                s.nanosleep(Duration::from_millis(2)).unwrap();
            };
            let plain = s.wait4(WaitTarget::Pid(pid), WaitOptions::NOHANG).unwrap();
            s.kill(KillTarget::Pid(pid), Signal::SIGCONT).unwrap();
            s.write(w, b"x").unwrap();
            let done = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap().unwrap();
            out(format!("{:?} {plain:?} {:?}", info.status, done.1));
            0
        }
        // Sem evento com `WNOHANG` dá `None`; sem filhos, ECHILD; e os argumentos inválidos, EINVAL.
        "errors" => {
            let none_yet = s.waitid(WaitIdTarget::All, exited).unwrap_err();
            let (pid, w) = blocked_child(0);
            let hang = s.waitid(WaitIdTarget::All, exited | WaitOptions::NOHANG).unwrap();
            let no_event = s.waitid(WaitIdTarget::All, WaitOptions::NOHANG).unwrap_err();
            let bad_pid = s.waitid(WaitIdTarget::Pid(0), exited).unwrap_err();
            let bad_group = s.waitid(WaitIdTarget::Group(-1), exited).unwrap_err();
            let bad_fd = s.waitid(WaitIdTarget::Pidfd(Fd(99)), exited).unwrap_err();
            let other = s.waitid(WaitIdTarget::Pid(pid + 1000), exited | WaitOptions::NOHANG).unwrap_err();
            let fd = s.pidfd_open(pid, 0).unwrap();
            s.write(w, b"x").unwrap();
            let via_fd = s.waitid(WaitIdTarget::Pidfd(fd), exited).unwrap().unwrap();
            out(format!(
                "{none_yet:?} {hang:?} {no_event:?} {bad_pid:?} {bad_group:?} {bad_fd:?} {other:?} {} {:?} {}",
                via_fd.pid == pid,
                via_fd.status,
                via_fd.uid == s.getuid()
            ));
            0
        }
        // O `wait4` leva o rusage do filho colhido e o `RUSAGE_CHILDREN` soma os já colhidos.
        "rusage" => {
            let before = s.getrusage(RusageWho::Children).unwrap();
            let spin = || {
                let end = std::time::Instant::now() + Duration::from_millis(30);
                while std::time::Instant::now() < end {}
                0
            };
            let a = s.spawn_fn(ProcAttrs::default(), b"child".to_vec(), Box::new(spin)).unwrap();
            let first = s.wait4_info(WaitTarget::Pid(a), WaitOptions::empty()).unwrap().unwrap();
            let after_a = s.getrusage(RusageWho::Children).unwrap();
            let b = s.spawn_fn(ProcAttrs::default(), b"child".to_vec(), Box::new(spin)).unwrap();
            let second = s.wait4_info(WaitTarget::Pid(b), WaitOptions::empty()).unwrap().unwrap();
            let after_b = s.getrusage(RusageWho::Children).unwrap();
            out(format!(
                "{} {} {} {} {}",
                before == Rusage::default(),
                first.rusage.utime > Duration::ZERO,
                after_a.utime == first.rusage.utime,
                after_b.utime == first.rusage.utime + second.rusage.utime,
                after_b.maxrss_kib == first.rusage.maxrss_kib.max(second.rusage.maxrss_kib)
            ));
            0
        }
        _ => 2,
    }
}

fn scenario(name: &str) -> String {
    let k = Kernel::new(KernelConfig::default());
    let sb = k.create_sandbox(SandboxConfig { programs: vec![Program::bin("waitid_scenario", p_waitid)], ..SandboxConfig::default() }).unwrap();
    let argv = ["waitid_scenario", name].iter().map(|s| s.as_bytes().to_vec()).collect();
    let r = sb.run(RunRequest { argv, timeout: Some(Duration::from_secs(20)), ..RunRequest::default() }).unwrap();
    assert_eq!(r.status, WaitStatus::Exited(0), "{name}: {}", String::from_utf8_lossy(&r.stderr));
    String::from_utf8_lossy(&r.stdout).trim_end().to_string()
}

#[test]
fn waitid_with_wnowait_leaves_the_zombie() {
    assert_eq!(scenario("nowait"), "true Exited(7) true Exited(7) true ECHILD");
}

#[test]
fn stop_and_continue_are_reported_once() {
    let line = scenario("job-control");
    assert_eq!(line, "Stopped(SIGSTOP) None Continued Continued Continued None Exited(0)");
}

#[test]
fn waitid_reports_stops_that_wait4_without_wuntraced_does_not_see() {
    assert_eq!(scenario("stopped-event"), "Stopped(SIGSTOP) None Exited(3)");
}

#[test]
fn waitid_errors_and_pidfd_target() {
    assert_eq!(scenario("errors"), "ECHILD None EINVAL EINVAL EINVAL EBADF ECHILD true Exited(0) true");
}

#[test]
fn wait4_carries_the_reaped_childs_rusage_into_rusage_children() {
    assert_eq!(scenario("rusage"), "true true true true true");
}
