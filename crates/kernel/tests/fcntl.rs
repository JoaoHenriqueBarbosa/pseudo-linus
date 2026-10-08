//! Testes de integração do `fcntl(2)` de travas (`F_SETLK`, `F_GETLK`, `F_OFD_*`), do `flock(2)`, do tamanho do
//! pipe (`F_GETPIPE_SZ`, `F_SETPIPE_SZ`), do `FIONREAD` e das ações de `posix_spawn` que dependem do kernel
//! (`closefrom`, `RESETIDS`, escalonador). Os resultados esperados são os do Linux 6.12.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::fcntl::{F_RDLCK, F_UNLCK, F_WRLCK, LOCK_EX, LOCK_MAND, LOCK_NB, LOCK_SH, LOCK_UN, SEEK_CUR, SEEK_END, SEEK_SET};
use sysabi::sys::{self, write_all};
use sysabi::*;

const FILE: &[u8] = b"/tmp/lockfile";

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

fn open_file(flags: OFlags) -> Fd {
    sys::open(FILE, flags | OFlags::CREAT, 0o644).unwrap()
}

/// `len` bytes a partir de `start`, com o tipo de trava dado.
fn span(l_type: i16, start: i64, len: i64) -> Flock {
    Flock { l_type, whence: SEEK_SET, start, len, pid: 0 }
}

/// Roda `f` num processo filho (que herda os fds) e devolve o que ele escreveu num pipe.
fn in_child(f: impl FnOnce() -> String + Send + 'static) -> String {
    let s = sys::current();
    let (r, w) = s.pipe2(OFlags::empty()).unwrap();
    let pid = s
        .spawn_fn(ProcAttrs::default(), b"child".to_vec(), Box::new(move || {
            write_all(w, f().as_bytes()).unwrap();
            0
        }))
        .unwrap();
    s.close(w).unwrap();
    let mut got = Vec::new();
    let mut buf = [0u8; 256];
    loop {
        let n = s.read(r, &mut buf).unwrap();
        if n == 0 {
            break;
        }
        got.extend_from_slice(&buf[..n]);
    }
    s.close(r).unwrap();
    s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap().unwrap();
    String::from_utf8(got).unwrap()
}

fn fd_list() -> String {
    let s = sys::current();
    let dir = s.openat(Fd::CWD, b"/proc/self/fd", OFlags::RDONLY | OFlags::DIRECTORY, 0).unwrap();
    let mut names = Vec::new();
    loop {
        let batch = s.getdents(dir).unwrap();
        if batch.is_empty() {
            break;
        }
        names.extend(batch.into_iter().map(|e| String::from_utf8(e.name).unwrap()));
    }
    names.retain(|n| n != "." && n != ".." && *n != dir.0.to_string());
    names.sort_by_key(|n| n.parse::<i32>().unwrap());
    names.join(",")
}

fn scenario_flock() -> String {
    let s = sys::current();
    let a = open_file(OFlags::RDWR);
    let b = open_file(OFlags::RDWR);
    let r = [
        verdict(s.flock(a, LOCK_EX | LOCK_NB)),
        // Outra descrição do mesmo arquivo conflita.
        verdict(s.flock(b, LOCK_SH | LOCK_NB)),
        // A mesma descrição converte EX em SH.
        verdict(s.flock(a, LOCK_SH)),
        verdict(s.flock(b, LOCK_SH | LOCK_NB)),
        // A conversão de SH em EX falha (b segura SH) e já soltou a trava de `a`.
        verdict(s.flock(a, LOCK_EX | LOCK_NB)),
        verdict(s.flock(b, LOCK_EX | LOCK_NB)),
        verdict(s.flock(a, 16)),
        verdict(s.flock(a, LOCK_MAND | LOCK_SH)),
        verdict(s.flock(Fd(99), LOCK_SH)),
        // O `flock` e a trava de faixa não se enxergam.
        verdict(s.fcntl_lock(a, LockCmd::Set, span(F_WRLCK, 0, 10))),
        verdict(s.flock(b, LOCK_UN)),
        verdict(s.flock(a, LOCK_EX | LOCK_NB)),
    ];
    r.join(" ")
}

fn scenario_posix() -> String {
    let s = sys::current();
    let a = open_file(OFlags::RDWR);
    let b = open_file(OFlags::RDWR);
    s.write(a, b"0123456789").unwrap();
    let ro = open_file(OFlags::RDONLY);
    let me = s.getpid();
    let first = [
        verdict(s.fcntl_lock(a, LockCmd::Set, span(F_WRLCK, 0, 10))),
        // O mesmo processo por outro fd: o dono é o processo, não há conflito.
        verdict(s.fcntl_lock(b, LockCmd::Set, span(F_WRLCK, 0, 10))),
    ];
    let from_child = in_child(move || {
        let s = sys::current();
        let c = open_file(OFlags::RDWR);
        let set = verdict(s.fcntl_lock(c, LockCmd::Set, span(F_WRLCK, 0, 10)));
        let got = s.fcntl_lock(c, LockCmd::Get, span(F_WRLCK, 0, 10)).unwrap();
        let ofd = verdict(s.fcntl_lock(c, LockCmd::OfdSet, span(F_RDLCK, 0, 10)));
        format!("{set} {}:{}:{} {ofd}", got.l_type, got.pid == me, got.len)
    });
    let errors = [
        // A trava OFD e a POSIX do mesmo processo conflitam.
        verdict(s.fcntl_lock(a, LockCmd::OfdSet, span(F_WRLCK, 0, 10))),
        verdict(s.fcntl_lock(a, LockCmd::OfdSet, Flock { pid: 1, ..span(F_WRLCK, 0, 10) })),
        verdict(s.fcntl_lock(a, LockCmd::Set, Flock { whence: 5, ..span(F_WRLCK, 0, 10) })),
        verdict(s.fcntl_lock(a, LockCmd::Set, span(F_WRLCK, -1, 10))),
        verdict(s.fcntl_lock(a, LockCmd::Set, span(F_WRLCK, i64::MAX, 2))),
        verdict(s.fcntl_lock(ro, LockCmd::Set, span(F_WRLCK, 0, 10))),
        verdict(s.fcntl_lock(a, LockCmd::Get, span(F_UNLCK, 0, 10))),
        verdict(s.fcntl_lock(a, LockCmd::Set, span(7, 0, 10))),
    ];
    // Fechar qualquer fd do arquivo solta as travas POSIX do processo nele.
    s.close(b).unwrap();
    let after_close = in_child(move || {
        let s = sys::current();
        let c = open_file(OFlags::RDWR);
        let set = verdict(s.fcntl_lock(c, LockCmd::Set, span(F_WRLCK, 0, 10)));
        let got = s.fcntl_lock(c, LockCmd::Get, span(F_WRLCK, 0, 10)).unwrap();
        format!("{set} {}:{}", got.l_type, got.pid == me)
    });
    [first.join(" "), from_child, errors.join(" "), after_close].join(" | ")
}

fn scenario_whence() -> String {
    let s = sys::current();
    let a = open_file(OFlags::RDWR);
    s.write(a, b"0123456789").unwrap();
    s.lseek(a, 4, Whence::Set).unwrap();
    // O trecho que o filho vê conflitar: `SEEK_CUR` com `l_start` -2 é o byte 2, `SEEK_END` com -3 é o 7.
    s.fcntl_lock(a, LockCmd::Set, Flock { l_type: F_WRLCK, whence: SEEK_CUR, start: -2, len: 1, pid: 0 }).unwrap();
    s.fcntl_lock(a, LockCmd::Set, Flock { l_type: F_WRLCK, whence: SEEK_END, start: -3, len: 1, pid: 0 }).unwrap();
    // `l_len` negativo termina antes de `l_start`: `5 - 2 = 3` até o byte 4, que se junta ao byte 2 (adjacentes).
    s.fcntl_lock(a, LockCmd::Set, span(F_WRLCK, 5, -2)).unwrap();
    in_child(|| {
        let s = sys::current();
        let c = open_file(OFlags::RDWR);
        let probe = |byte: i64| {
            let got = s.fcntl_lock(c, LockCmd::Get, span(F_WRLCK, byte, 1)).unwrap();
            if got.l_type == F_UNLCK { "-".to_string() } else { format!("{}+{}", got.start, got.len) }
        };
        // Os bytes 2, 3 e 4 são uma trava só (2+3) e o 7 outra.
        (0..10).map(probe).collect::<Vec<_>>().join(" ")
    })
}

fn scenario_deadlock() -> String {
    let s = sys::current();
    let a = open_file(OFlags::RDWR);
    s.write(a, b"0123456789").unwrap();
    s.fcntl_lock(a, LockCmd::Set, span(F_WRLCK, 0, 1)).unwrap();
    let (r, w) = s.pipe2(OFlags::empty()).unwrap();
    let pid = s
        .spawn_fn(ProcAttrs::default(), b"child".to_vec(), Box::new(move || {
            let s = sys::current();
            let c = open_file(OFlags::RDWR);
            s.fcntl_lock(c, LockCmd::Set, span(F_WRLCK, 1, 1)).unwrap();
            write_all(w, b"x").unwrap();
            // Bloqueia no byte 0 do pai até ele soltar.
            if s.fcntl_lock(c, LockCmd::SetWait, span(F_WRLCK, 0, 1)).is_ok() { 0 } else { 3 }
        }))
        .unwrap();
    s.close(w).unwrap();
    let mut b = [0u8; 1];
    s.read(r, &mut b).unwrap();
    // O filho já tem o byte 1 e está pedindo o 0: esperar o 1 fecharia o ciclo.
    s.nanosleep(Duration::from_millis(300)).unwrap();
    let dead = verdict(s.fcntl_lock(a, LockCmd::SetWait, span(F_WRLCK, 1, 1)));
    s.fcntl_lock(a, LockCmd::Set, span(F_UNLCK, 0, 1)).unwrap();
    let status = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap().unwrap().1;
    format!("{dead} {status:?}")
}

fn scenario_pipe() -> String {
    let s = sys::current();
    let (r, w) = s.pipe2(OFlags::NONBLOCK).unwrap();
    let mut v = vec![s.pipe_size(r).unwrap().to_string()];
    v.push(s.set_pipe_size(w, 100_000).unwrap().to_string());
    v.push(s.pipe_size(r).unwrap().to_string());
    v.push(s.set_pipe_size(w, 1).unwrap().to_string());
    // Uma página: cabe 4096 e o byte seguinte espera.
    v.push(s.write(w, &[0u8; 4096]).unwrap().to_string());
    v.push(verdict(s.write(w, b"x")));
    let mut sink = [0u8; 8192];
    s.read(r, &mut sink).unwrap();
    v.push(s.set_pipe_size(w, 65536).unwrap().to_string());
    s.write(w, &[1u8; 5000]).unwrap();
    // Duas páginas ocupadas: não cabe em uma.
    v.push(verdict(s.set_pipe_size(w, 4096)));
    s.read(r, &mut sink).unwrap();
    v.push(verdict(s.set_pipe_size(w, 4096)));
    let file = open_file(OFlags::RDWR);
    v.push(verdict(s.pipe_size(file)));
    v.push(verdict(s.set_pipe_size(w, 0x8000_0001)));
    v.push(String::from_utf8(sys::read_file(b"/proc/sys/fs/pipe-max-size").unwrap()).unwrap().trim().to_string());
    // Sem privilégio, passar de `pipe-max-size` é EPERM (e o limite em si, não); com ele, não.
    v.push(in_child(move || {
        let s = sys::current();
        s.setresuid(1000, 1000, 1000).unwrap();
        let big = verdict(s.set_pipe_size(w, 2 << 20));
        let limit = s.set_pipe_size(w, 1 << 20).unwrap();
        format!("{big} {limit}")
    }));
    v.push(s.set_pipe_size(w, 2 << 20).unwrap().to_string());
    v.join(" ")
}

fn scenario_fionread() -> String {
    let s = sys::current();
    let (r, w) = s.pipe2(OFlags::empty()).unwrap();
    s.write(w, b"hello").unwrap();
    let mut v = vec![s.fionread(r).unwrap().to_string()];
    let mut two = [0u8; 2];
    s.read(r, &mut two).unwrap();
    v.push(s.fionread(r).unwrap().to_string());
    let f = open_file(OFlags::RDWR | OFlags::TRUNC);
    s.write(f, b"0123456789").unwrap();
    s.lseek(f, 3, Whence::Set).unwrap();
    v.push(s.fionread(f).unwrap().to_string());
    s.lseek(f, 14, Whence::Set).unwrap();
    v.push(s.fionread(f).unwrap().to_string());
    let null = s.openat(Fd::CWD, b"/dev/null", OFlags::RDWR, 0).unwrap();
    v.push(verdict(s.fionread(null)));
    let dir = s.openat(Fd::CWD, b"/tmp", OFlags::RDONLY | OFlags::DIRECTORY, 0).unwrap();
    v.push(verdict(s.fionread(dir)));
    v.push(verdict(s.fionread(Fd(99))));
    for ty in [1u8, 2, 5] {
        let (x, y) = s.unix_socketpair(ty, false, false).unwrap();
        s.write(x, b"abc").unwrap();
        s.write(x, b"defg").unwrap();
        v.push(s.fionread(y).unwrap().to_string());
    }
    v.join(" ")
}

fn scenario_getfl() -> String {
    let s = sys::current();
    let f = open_file(OFlags::RDWR);
    let (r, w) = s.pipe2(OFlags::NONBLOCK).unwrap();
    let flags = |fd| format!("{:#o}", s.get_status_flags(fd).unwrap().bits());
    [flags(f), flags(r), flags(w)].join(" ")
}

fn scenario_spawn() -> String {
    let s = sys::current();
    let keep = open_file(OFlags::RDWR);
    let (_, _) = (open_file(OFlags::RDWR), open_file(OFlags::RDWR));
    let spawn = |attrs: ProcAttrs, mode: &str| {
        s.spawn(SpawnSpec { path: b"/usr/bin/fcntl_scenario".to_vec(), argv: vec![b"fcntl_scenario".to_vec(), mode.as_bytes().to_vec()], attrs })
    };
    let wait = |pid: Pid| s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap().unwrap().1;
    let mut v = Vec::new();
    // `closefrom(keep + 1)`: o filho fica com o stdio e `keep`.
    let pid = spawn(ProcAttrs { fd_actions: vec![FdAction::CloseFrom(Fd(keep.0 + 1))], ..ProcAttrs::default() }, "fdlist").unwrap();
    v.push(format!("{:?}", wait(pid)));
    v.push(verdict(spawn(ProcAttrs { fd_actions: vec![FdAction::Close(Fd(i32::MAX))], ..ProcAttrs::default() }, "noop")));
    v.push(verdict(spawn(ProcAttrs { fd_actions: vec![FdAction::CloseFrom(Fd(-1))], ..ProcAttrs::default() }, "noop")));
    let pid = spawn(ProcAttrs { fd_actions: vec![FdAction::Close(Fd(50))], ..ProcAttrs::default() }, "noop").unwrap();
    v.push(format!("{:?}", wait(pid)));
    // O fd aberto pela ação nunca herda `FD_CLOEXEC`, mesmo com `O_CLOEXEC`.
    let open_action = FdAction::Open { fd: Fd(20), path: FILE.to_vec(), flags: OFlags::RDONLY | OFlags::CLOEXEC, mode: 0 };
    let pid = spawn(ProcAttrs { fd_actions: vec![open_action], ..ProcAttrs::default() }, "fdlist").unwrap();
    v.push(format!("{:?}", wait(pid)));
    // O escalonador: FIFO precisa de `CAP_SYS_NICE`, que o contêiner padrão não dá; OTHER e só a prioridade passam.
    let sched = |policy, priority| ProcAttrs { scheduler: Some(SpawnSched { policy, param: SchedParam { priority } }), ..ProcAttrs::default() };
    v.push(verdict(spawn(sched(Some(1), 1), "noop")));
    let pid = spawn(sched(Some(0), 0), "noop").unwrap();
    v.push(format!("{:?}", wait(pid)));
    let pid = spawn(sched(None, 0), "noop").unwrap();
    v.push(format!("{:?}", wait(pid)));
    // `RESETIDS`: o uid efetivo volta ao real.
    s.setresuid(0, 1000, 0).unwrap();
    let pid = spawn(ProcAttrs::default(), "ids").unwrap();
    v.push(format!("{:?}", wait(pid)));
    let pid = spawn(ProcAttrs { reset_ids: true, ..ProcAttrs::default() }, "ids").unwrap();
    v.push(format!("{:?}", wait(pid)));
    v.join("\n")
}

fn p_fcntl(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let name = String::from_utf8_lossy(args[1].as_bytes()).into_owned();
    let text = match name.as_str() {
        "flock" => scenario_flock(),
        "posix" => scenario_posix(),
        "whence" => scenario_whence(),
        "deadlock" => scenario_deadlock(),
        "pipe" => scenario_pipe(),
        "fionread" => scenario_fionread(),
        "getfl" => scenario_getfl(),
        "spawn" => scenario_spawn(),
        "fdlist" => fd_list(),
        "ids" => format!("{:?}", sys::current().getresuid()),
        "noop" => return 0,
        _ => return 2,
    };
    out(text);
    0
}

fn scenario(name: &str) -> String {
    let k = Kernel::new(KernelConfig::default());
    let sb = k.create_sandbox(SandboxConfig { programs: vec![Program::bin("fcntl_scenario", p_fcntl)], ..SandboxConfig::default() }).unwrap();
    let argv = ["fcntl_scenario", name].iter().map(|s| s.as_bytes().to_vec()).collect();
    let r = sb.run(RunRequest { argv, timeout: Some(Duration::from_secs(30)), ..RunRequest::default() }).unwrap();
    assert_eq!(r.status, WaitStatus::Exited(0), "{name}: {}", String::from_utf8_lossy(&r.stderr));
    String::from_utf8_lossy(&r.stdout).trim_end().to_string()
}

#[test]
fn flock_conflicts_converts_and_ignores_range_locks() {
    assert_eq!(scenario("flock"), "ok EAGAIN ok ok EAGAIN ok EINVAL ok EBADF ok ok ok");
}

#[test]
fn posix_locks_belong_to_the_process_and_clash_with_ofd_locks() {
    assert_eq!(
        scenario("posix"),
        "ok ok | EAGAIN 1:true:10 EAGAIN | EAGAIN EINVAL EINVAL EINVAL EOVERFLOW EBADF EINVAL EINVAL | ok 2:false"
    );
}

#[test]
fn posix_lock_ranges_resolve_whence_and_negative_lengths() {
    assert_eq!(scenario("whence"), "- - 2+3 2+3 2+3 - - 7+1 - -");
}

#[test]
fn posix_lock_wait_detects_a_deadlock_between_processes() {
    // No Linux x86_64 `EDEADLK` e `EDEADLOCK` valem 35; o nome do `Debug` é o do alias.
    assert_eq!(scenario("deadlock"), "EDEADLOCK Exited(0)");
}

#[test]
fn pipe_size_rounds_to_a_power_of_two_and_obeys_the_limits() {
    assert_eq!(
        scenario("pipe"),
        "65536 131072 131072 4096 4096 EAGAIN 65536 EBUSY ok EBADF EINVAL 1048576 EPERM 1048576 2097152"
    );
}

#[test]
fn fionread_counts_the_unread_bytes_of_each_kind_of_file() {
    assert_eq!(scenario("fionread"), "5 3 7 -4 ENOTTY ENOTTY EBADF 7 3 7");
}

#[test]
fn getfl_reports_the_open_flags_with_largefile_on_files_only() {
    assert_eq!(scenario("getfl"), "0o100002 0o4000 0o4001");
}

#[test]
fn spawn_actions_closefrom_open_scheduler_and_resetids() {
    // O stdout é o do pai: os filhos escrevem as listas e os ids antes de o pai escrever o que colheu.
    assert_eq!(
        scenario("spawn"),
        "0,1,2,3\n0,1,2,3,4,5,20\n(0, 1000, 0)\n(0, 0, 0)\nExited(0)\nEBADF\nEBADF\nExited(0)\nExited(0)\nEPERM\nExited(0)\nExited(0)\nExited(0)\nExited(0)"
    );
}
