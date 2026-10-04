//! Estresse do caminho de tokens de CPU: pipelines curtos repetidos; se um travar, imprime o estado do
//! escalonador e dos processos.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::time::{Duration, Instant};

use kernel::{HostStdio, Kernel, KernelConfig, SandboxConfig};
use sysabi::sys::{self, write_all};
use sysabi::*;

fn p_yes(_ctx: &mut Ctx, _args: &[OsString]) -> i32 {
    let buf = b"y\n".repeat(4096);
    loop {
        if write_all(Fd::STDOUT, &buf).is_err() {
            return 1;
        }
    }
}

fn p_head(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let n: usize = std::str::from_utf8(args[1].as_bytes()).unwrap().parse().unwrap();
    let mut left = n;
    let mut buf = vec![0u8; 4096];
    while left > 0 {
        let got = sys::read(Fd::STDIN, &mut buf[..left.min(4096)]).unwrap();
        if got == 0 {
            break;
        }
        write_all(Fd::STDOUT, &buf[..got]).unwrap();
        left -= got;
    }
    0
}

/// `pair`: yes | head 10, esperando os dois.
fn p_pair(_ctx: &mut Ctx, _args: &[OsString]) -> i32 {
    let s = sys::current();
    let (r, w) = s.pipe2(OFlags::CLOEXEC).unwrap();
    let a = s
        .spawn(SpawnSpec {
            path: b"/usr/bin/yes".to_vec(),
            argv: vec![b"yes".to_vec()],
            attrs: ProcAttrs { fd_actions: vec![FdAction::Dup2 { from: w, to: Fd::STDOUT }], ..ProcAttrs::default() },
        })
        .unwrap();
    let b = s
        .spawn(SpawnSpec {
            path: b"/usr/bin/head".to_vec(),
            argv: vec![b"head".to_vec(), b"10".to_vec()],
            attrs: ProcAttrs { fd_actions: vec![FdAction::Dup2 { from: r, to: Fd::STDIN }], ..ProcAttrs::default() },
        })
        .unwrap();
    s.close(r).unwrap();
    s.close(w).unwrap();
    s.wait4(WaitTarget::Pid(a), WaitOptions::empty()).unwrap();
    s.wait4(WaitTarget::Pid(b), WaitOptions::empty()).unwrap();
    0
}

#[test]
fn many_short_pipelines_never_stall() {
    let n: usize = std::env::var("SCHED_STRESS_N").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
    let k = Kernel::new(KernelConfig::default());
    let sb = k
        .create_sandbox(SandboxConfig {
            programs: vec![Program::bin("yes", p_yes), Program::bin("head", p_head), Program::bin("pair", p_pair)],
            ..SandboxConfig::default()
        })
        .unwrap();
    for i in 0..n {
        let sp = sb
            .spawn(SpawnSpec { path: b"/usr/bin/pair".to_vec(), argv: vec![b"pair".to_vec()], attrs: ProcAttrs::default() }, HostStdio::default())
            .unwrap();
        match sb.wait(sp.pid, Some(Instant::now() + Duration::from_secs(10))).unwrap() {
            Some(_) => {}
            None => {
                let procs = sb.processes();
                panic!("rodada {i} travou\nprocessos: {procs:#?}\n{}", k.debug_scheduler_state());
            }
        }
    }
}
