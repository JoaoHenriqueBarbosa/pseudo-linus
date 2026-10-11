//! O `Host` de rede do zjsc (`net_*`) sobre o kernel real: loopback TCP de verdade entre dois descritores do mesmo
//! processo, o desenho do `Bun.serve` com `fetch` na mesma máquina. O `TestKit` não tem TCP, então este teste (e os casos
//! de rede do corpus) roda no `kernel`.

use std::ffi::OsString;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::sys::write_all;
use sysabi::{Ctx, Fd, Program, WaitStatus};
use ul_bun::host::SysabiHost;
use zjsc::runtime::host::{Host, NetWatch, EAGAIN};

const ECONNREFUSED: i32 = 111;

fn out(text: &str) {
    write_all(Fd::STDOUT, format!("{text}\n").as_bytes()).unwrap();
}

fn verdict<T>(result: Result<T, i32>) -> String {
    match result {
        Ok(_) => "ok".to_string(),
        Err(errno) => format!("errno {errno}"),
    }
}

/// Um servidor e um cliente no mesmo processo: o que o laço de eventos do `Bun.serve` faz sobre o `Host`.
fn scenario(_ctx: &mut Ctx, _args: &[OsString]) -> i32 {
    let host = SysabiHost;
    let (listener, port) = host.net_listen(0, 16).unwrap();
    out(&format!("porta efêmera {}", port != 0));
    out(&format!("accept vazio {}", verdict(host.net_accept(listener)) == format!("errno {EAGAIN}")));
    let idle = host.net_poll(&[NetWatch { fd: listener, read: true, write: false }], Some(Duration::ZERO)).unwrap();
    out(&format!("ouvinte ocioso {}", idle[0].any()));

    let (client, local) = host.net_connect(port).unwrap();
    let ready = host.net_poll(&[NetWatch { fd: listener, read: true, write: false }], Some(Duration::from_secs(1))).unwrap();
    out(&format!("ouvinte com conexão {}", ready[0].readable));
    let (server, peer) = host.net_accept(listener).unwrap();
    out(&format!("portas {}", peer == local));
    out(&format!("recv vazio {}", verdict(host.net_recv(server, 16)) == format!("errno {EAGAIN}")));

    out(&format!("send {}", host.net_send(client, b"ping").unwrap()));
    let ready = host.net_poll(&[NetWatch { fd: server, read: true, write: true }], Some(Duration::from_secs(1))).unwrap();
    out(&format!("servidor legível {} gravável {}", ready[0].readable, ready[0].writable));
    out(&format!("recv {}", String::from_utf8_lossy(&host.net_recv(server, 16).unwrap())));
    out(&format!("send {}", host.net_send(server, b"pong!").unwrap()));
    out(&format!("recv parcial {}", String::from_utf8_lossy(&host.net_recv(client, 2).unwrap())));
    out(&format!("recv resto {}", String::from_utf8_lossy(&host.net_recv(client, 16).unwrap())));

    host.net_shutdown(client, false, true).unwrap();
    let ready = host.net_poll(&[NetWatch { fd: server, read: true, write: false }], Some(Duration::from_secs(1))).unwrap();
    out(&format!("fim legível {}", ready[0].readable));
    out(&format!("fim {}", host.net_recv(server, 16).unwrap().is_empty()));
    host.fs_close(server).unwrap();
    host.fs_close(client).unwrap();
    host.fs_close(listener).unwrap();
    out(&format!("recusada {}", verdict(host.net_connect(port)) == format!("errno {ECONNREFUSED}")));
    0
}

#[test]
fn net_host_loopback() {
    let kernel = Kernel::new(KernelConfig::default());
    let sandbox = kernel.create_sandbox(SandboxConfig { programs: vec![Program::bin("net_scenario", scenario)], ..SandboxConfig::default() }).unwrap();
    let run = sandbox.run(RunRequest { argv: vec![b"net_scenario".to_vec()], timeout: Some(Duration::from_secs(30)), ..RunRequest::default() }).unwrap();
    assert_eq!(run.status, WaitStatus::Exited(0), "{}", String::from_utf8_lossy(&run.stderr));
    let expected = "porta efêmera true\naccept vazio true\nouvinte ocioso false\nouvinte com conexão true\nportas true\nrecv vazio true\nsend 4\n\
servidor legível true gravável true\nrecv ping\nsend 5\nrecv parcial po\nrecv resto ng!\nfim legível true\nfim true\nrecusada true\n";
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected);
}
