//! Programas de teste do host (feature `test-programs`), instalados em `/usr/local/bin` da sandbox.
//! Exercitam o que o daemon precisa provar com o kernel de verdade e que o userland não oferece de
//! propósito: laço de CPU que só morre por sinal, saída arbitrária, filho em segundo plano segurando o
//! stdout, e a queda do processo do host.

use std::ffi::OsString;

use sysabi::{Ctx, Fd, Program, ProcAttrs, SpawnSpec, sys};

fn arg_u64(argv: &[OsString], i: usize, default: u64) -> u64 {
    argv.get(i).and_then(|a| a.to_str()).and_then(|s| s.parse().ok()).unwrap_or(default)
}

/// `pl-spin`: laço de CPU com checkpoint; só termina por sinal.
fn spin(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    let mut x: u64 = 1;
    loop {
        for _ in 0..10_000 {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        }
        std::hint::black_box(x);
        sys::checkpoint();
    }
}

/// `pl-bigout N`: escreve N bytes `x` no stdout.
fn bigout(_ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let mut n = arg_u64(argv, 1, 0);
    let chunk = [b'x'; 65536];
    while n > 0 {
        let k = n.min(chunk.len() as u64) as usize;
        if sys::write_all(Fd::STDOUT, &chunk[..k]).is_err() {
            return 1;
        }
        n -= k as u64;
    }
    0
}

/// `pl-sleep N`: dorme N segundos (acorda com sinal).
fn sleep(_ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let secs = arg_u64(argv, 1, 1);
    match sys::current().nanosleep(std::time::Duration::from_secs(secs)) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

/// `pl-exit N`: sai com N.
fn exit(_ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    arg_u64(argv, 1, 0) as i32
}

/// `pl-bg N`: deixa um `pl-sleep N` em segundo plano com o stdout herdado e sai na hora.
fn bg(ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let secs = argv.get(1).and_then(|a| a.to_str()).unwrap_or("30").as_bytes().to_vec();
    let spec = SpawnSpec {
        path: b"/usr/local/bin/pl-sleep".to_vec(),
        argv: vec![b"pl-sleep".to_vec(), secs],
        attrs: ProcAttrs::default(),
    };
    match ctx.sys().spawn(spec) {
        Ok(_) => 0,
        Err(e) => {
            ctx.error(format!("spawn: {}", e.message()));
            1
        }
    }
}

/// `pl-crash`: aborta o processo do host inteiro (o equivalente a um stack overflow que escapou do
/// stacker), pra provar que o supervisor isola a queda num worker.
fn crash(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    std::process::abort()
}

pub fn programs() -> Vec<Program> {
    vec![
        Program { name: "pl-spin", dir: "/usr/local/bin", main: spin },
        Program { name: "pl-bigout", dir: "/usr/local/bin", main: bigout },
        Program { name: "pl-bg", dir: "/usr/local/bin", main: bg },
        Program { name: "pl-sleep", dir: "/usr/local/bin", main: sleep },
        Program { name: "pl-exit", dir: "/usr/local/bin", main: exit },
        Program { name: "pl-crash", dir: "/usr/local/bin", main: crash },
    ]
}
