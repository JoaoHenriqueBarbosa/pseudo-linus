//! Testes de integração de `alarm(2)`, `setitimer(2)` e `getitimer(2)`. Os resultados esperados são os do
//! Linux 6.12: o timer é do processo, o filho do `fork` nasce sem ele, o `execve` o mantém, o intervalo o
//! rearma e os valores inválidos dão `EINVAL`. Nenhum caso depende de tempo exato: só de o sinal chegar (ou
//! não) e de faixas largas do que resta.

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::sys::{self, write_all};
use sysabi::*;

const REAL: i32 = Itimer::Real as i32;
const VIRTUAL: i32 = Itimer::Virtual as i32;
const PROF: i32 = Itimer::Prof as i32;

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

fn millis(ms: i64) -> Itimerval {
    Itimerval { value_sec: ms / 1000, value_usec: ms % 1000 * 1000, ..Itimerval::default() }
}

/// O que `getitimer` devolve, em segundos inteiros (valor, intervalo).
fn left_secs(which: i32) -> (i64, i64) {
    let t = sys::current().getitimer(which).unwrap();
    (t.value_sec, t.interval_sec)
}

fn scenario_alarm() -> String {
    let s = sys::current();
    let first = s.alarm(0).unwrap();
    let second = s.alarm(100).unwrap();
    let armed = left_secs(REAL);
    // Falta pouco menos de 100 s: arredonda para 100 (sobra mais de meio segundo).
    let third = s.alarm(50).unwrap();
    let swapped = left_secs(REAL);
    let cancelled = s.alarm(0).unwrap();
    let after = left_secs(REAL);
    // O excedente de `INT_MAX` é cortado.
    s.alarm(u32::MAX).unwrap();
    let clamped = left_secs(REAL).0 >= i64::from(i32::MAX) - 2;
    s.alarm(0).unwrap();
    format!("{first} {second} {} {third} {} {cancelled} {} {clamped}", armed.0 >= 99, swapped.0 >= 49, after.0)
}

fn scenario_validation() -> String {
    let s = sys::current();
    let bad = |v: Itimerval| verdict(s.setitimer(REAL, v));
    let r = [
        verdict(s.setitimer(3, millis(1))),
        verdict(s.setitimer(-1, millis(1))),
        verdict(s.getitimer(3)),
        verdict(s.getitimer(-1)),
        bad(Itimerval { value_usec: 1_000_000, ..Itimerval::default() }),
        bad(Itimerval { interval_usec: 1_000_000, ..Itimerval::default() }),
        bad(Itimerval { value_sec: -1, ..Itimerval::default() }),
        bad(Itimerval { interval_usec: -1, ..Itimerval::default() }),
        bad(Itimerval { value_sec: 100, ..Itimerval::default() }),
    ];
    // Valor zero desarma, mas o intervalo fica guardado.
    s.setitimer(REAL, Itimerval { interval_sec: 5, ..Itimerval::default() }).unwrap();
    let kept = s.getitimer(REAL).unwrap();
    s.setitimer(REAL, Itimerval::default()).unwrap();
    format!("{} {}:{}:{}", r.join(" "), kept.value_sec, kept.value_usec, kept.interval_sec)
}

fn scenario_rounding() -> String {
    let s = sys::current();
    s.setitimer(REAL, Itimerval { value_sec: 2, value_usec: 400_000, ..Itimerval::default() }).unwrap();
    let low = s.alarm(1).unwrap();
    // Sobra quase um segundo: arredonda para cima.
    let one = s.alarm(0).unwrap();
    s.setitimer(REAL, Itimerval { value_sec: 2, value_usec: 700_000, ..Itimerval::default() }).unwrap();
    let high = s.alarm(0).unwrap();
    format!("{low} {one} {high}")
}

fn scenario_fire() -> String {
    let s = sys::current();
    s.sigaction(Signal::SIGALRM, SigDisposition::Catch).unwrap();
    let begin = s.clock_gettime(Clock::Monotonic).unwrap();
    s.setitimer(REAL, millis(20)).unwrap();
    let slept = verdict(s.nanosleep(Duration::from_secs(30)));
    let elapsed = s.clock_gettime(Clock::Monotonic).unwrap().sec - begin.sec;
    let caught = s.take_caught_signals();
    let left = s.getitimer(REAL).unwrap();
    format!("{slept} {} {} {}:{}", caught == [Signal::SIGALRM], elapsed < 10, left.value_sec, left.value_usec)
}

fn scenario_periodic() -> String {
    let s = sys::current();
    s.sigaction(Signal::SIGALRM, SigDisposition::Catch).unwrap();
    s.setitimer(REAL, Itimerval { value_usec: 10_000, interval_usec: 10_000, ..Itimerval::default() }).unwrap();
    let mut interrupted = 0;
    for _ in 0..3 {
        if s.nanosleep(Duration::from_secs(30)) == Err(Errno::EINTR) {
            interrupted += 1;
        }
    }
    let t = s.getitimer(REAL).unwrap();
    let armed = t.value_sec == 0 && t.value_usec > 0 && t.value_usec <= 10_000;
    // `alarm` desfaz o intervalo.
    s.alarm(0).unwrap();
    let after = s.getitimer(REAL).unwrap();
    format!("{interrupted} {armed} {} {}", t.interval_usec, after.interval_usec)
}

fn scenario_ignored() -> String {
    let s = sys::current();
    s.sigaction(Signal::SIGALRM, SigDisposition::Ignore).unwrap();
    s.setitimer(REAL, millis(20)).unwrap();
    let slept = verdict(s.nanosleep(Duration::from_millis(400)));
    let caught = s.take_caught_signals().len();
    let left = s.getitimer(REAL).unwrap();
    format!("{slept} {caught} {}", left.value_sec + left.value_usec)
}

/// Roda `f` num filho e devolve o que ele escreveu num pipe, mais como ele terminou.
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

fn scenario_fork() -> String {
    let s = sys::current();
    s.alarm(100).unwrap();
    s.setitimer(VIRTUAL, Itimerval { value_sec: 100, interval_sec: 3, ..Itimerval::default() }).unwrap();
    // O filho nasce sem timers e o dele não mexe no do pai.
    let child = in_child(|| {
        let s = sys::current();
        let before = [REAL, VIRTUAL].map(|w| s.getitimer(w).unwrap());
        s.alarm(7).unwrap();
        format!("{} {} {}:{}", before[0] == Itimerval::default(), before[1] == Itimerval::default(), before[0].interval_sec, before[1].interval_sec)
    });
    let virtual_left = left_secs(VIRTUAL);
    let parent = (left_secs(REAL).0 >= 99, virtual_left.0 >= 99, virtual_left.1);
    s.alarm(0).unwrap();
    s.setitimer(VIRTUAL, Itimerval::default()).unwrap();
    format!("{child} {parent:?}")
}

fn scenario_exec() -> String {
    let s = sys::current();
    s.alarm(100).unwrap();
    let e = s.execve(b"/usr/bin/itimer_scenario", &[b"itimer_scenario".to_vec(), b"after_exec".to_vec()], None);
    format!("execve falhou: {e:?}")
}

fn scenario_after_exec() -> String {
    let s = sys::current();
    let kept = left_secs(REAL).0 >= 99;
    s.alarm(0).unwrap();
    format!("{kept}")
}

fn scenario_default_kill() -> String {
    let s = sys::current();
    let pid = s
        .spawn_fn(ProcAttrs::default(), b"child".to_vec(), Box::new(|| {
            let s = sys::current();
            s.setitimer(REAL, millis(20)).unwrap();
            let _ = s.nanosleep(Duration::from_secs(30));
            0
        }))
        .unwrap();
    match s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap().unwrap().1 {
        WaitStatus::Signaled { signal, core_dumped } => format!("signaled {} {core_dumped}", signal.0),
        other => format!("{other:?}"),
    }
}

/// Gasta CPU até o sinal do timer de CPU chegar (ou 20 s).
fn scenario_cpu() -> String {
    let s = sys::current();
    let mut seen = Vec::new();
    for (which, sig) in [(VIRTUAL, Signal::SIGVTALRM), (PROF, Signal::SIGPROF)] {
        s.sigaction(sig, SigDisposition::Catch).unwrap();
        s.setitimer(which, millis(30)).unwrap();
        let begin = std::time::Instant::now();
        let mut spin = 0u64;
        while begin.elapsed() < Duration::from_secs(20) && !s.take_caught_signals().contains(&sig) {
            for _ in 0..10_000 {
                spin = std::hint::black_box(spin.wrapping_add(1));
            }
            s.checkpoint();
        }
        let left = s.getitimer(which).unwrap();
        seen.push(format!("{} {}", begin.elapsed() < Duration::from_secs(20), left.value_sec + left.value_usec));
    }
    seen.join(" ")
}

fn p_itimer(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let name = String::from_utf8_lossy(args[1].as_bytes()).into_owned();
    let text = match name.as_str() {
        "alarm" => scenario_alarm(),
        "validation" => scenario_validation(),
        "rounding" => scenario_rounding(),
        "fire" => scenario_fire(),
        "periodic" => scenario_periodic(),
        "ignored" => scenario_ignored(),
        "fork" => scenario_fork(),
        "exec" => scenario_exec(),
        "after_exec" => scenario_after_exec(),
        "default_kill" => scenario_default_kill(),
        "cpu" => scenario_cpu(),
        _ => return 2,
    };
    out(text);
    0
}

fn scenario(name: &str) -> String {
    let k = Kernel::new(KernelConfig::default());
    let sb = k.create_sandbox(SandboxConfig { programs: vec![Program::bin("itimer_scenario", p_itimer)], ..SandboxConfig::default() }).unwrap();
    let argv = ["itimer_scenario", name].iter().map(|s| s.as_bytes().to_vec()).collect();
    let r = sb.run(RunRequest { argv, timeout: Some(Duration::from_secs(60)), ..RunRequest::default() }).unwrap();
    assert_eq!(r.status, WaitStatus::Exited(0), "{name}: {}", String::from_utf8_lossy(&r.stderr));
    String::from_utf8_lossy(&r.stdout).trim_end().to_string()
}

#[test]
fn alarm_returns_the_previous_remainder_and_zero_cancels() {
    assert_eq!(scenario("alarm"), "0 0 true 100 true 50 0 true");
}

#[test]
fn invalid_arguments_are_einval_and_zero_keeps_the_interval() {
    assert_eq!(scenario("validation"), "EINVAL EINVAL EINVAL EINVAL EINVAL EINVAL EINVAL EINVAL ok 0:0:5");
}

#[test]
fn alarm_rounds_the_remainder_at_half_a_second() {
    assert_eq!(scenario("rounding"), "2 1 3");
}

#[test]
fn real_timer_delivers_sigalrm_and_interrupts_a_sleep() {
    assert_eq!(scenario("fire"), "EINTR true true 0:0");
}

#[test]
fn interval_rearms_the_timer_until_alarm_cancels_it() {
    assert_eq!(scenario("periodic"), "3 true 10000 0");
}

#[test]
fn ignored_sigalrm_does_not_interrupt() {
    assert_eq!(scenario("ignored"), "ok 0 0");
}

#[test]
fn fork_clears_the_timers_in_the_child_only() {
    assert_eq!(scenario("fork"), "true true 0:0 (true, true, 3)");
}

#[test]
fn execve_keeps_the_timers() {
    assert_eq!(scenario("exec"), "true");
}

#[test]
fn default_action_of_sigalrm_kills_the_process() {
    assert_eq!(scenario("default_kill"), "signaled 14 false");
}

#[test]
fn cpu_timers_count_the_cpu_time_of_the_process() {
    assert_eq!(scenario("cpu"), "true 0 true 0");
}
