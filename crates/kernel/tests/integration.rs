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
