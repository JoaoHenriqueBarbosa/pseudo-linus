//! Integração do daemon: o binário de verdade (supervisor, workers em processos separados, HTTP),
//! falando JSON-RPC, com o backend falso no lugar do kernel.
//!
//! Cobre chaves (expiração, revogação, usuário desativado, troca pelo comando de admin com o daemon
//! rodando), quotas e admissão, dois usuários ao mesmo tempo, worker que cai e volta (com e sem
//! snapshot persistido), timeout de parede, sessões, streaming (HTTP e WebSocket) e tar.

mod common;

use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use common::*;
use host::client::Client;
use host::rpc::codes;
use serde_json::{Value, json};

#[test]
fn keys_expire_revoke_and_disable() {
    let d = Daemon::fake("");
    let token = d.user("bob", json!({}));
    let bob = d.client(&token);
    let me = bob.call("whoami", json!({})).unwrap();
    assert_eq!(me["user"], "bob");
    assert_eq!(me["role"], "user");

    // Usuário comum não chama método de admin.
    let e = bob.call("admin.users.list", json!({})).unwrap_err();
    assert_eq!(rpc_code(&e), codes::FORBIDDEN);

    // Chave mal formada, inexistente e ausente: 401 com JSON-RPC no corpo.
    for bad in ["nada", "plk_0000000000000000_0000000000000000000000000000000000000000000000000000000000000000"] {
        let e = d.client(bad).call("whoami", json!({})).unwrap_err();
        assert_eq!(rpc_code(&e), codes::UNAUTHORIZED);
    }

    // Chave que expira em 1 s, criada pelo comando de admin com o daemon rodando (outro processo).
    let short = admin_cli(&d.data_dir(), &["key", "create", "bob", "--expires", "1s"]);
    let short = d.client(short["token"].as_str().unwrap());
    assert_eq!(short.call("whoami", json!({})).unwrap()["user"], "bob");
    thread::sleep(Duration::from_millis(2100));
    let e = short.call("whoami", json!({})).unwrap_err();
    assert_eq!(e.rpc().unwrap().message, "chave de API expirada");

    // Revogação pelo RPC de admin vale na hora.
    let key_id = me["key_id"].as_str().unwrap();
    d.admin().call("admin.keys.revoke", json!({ "key_id": key_id })).unwrap();
    let e = bob.call("whoami", json!({})).unwrap_err();
    assert_eq!(e.rpc().unwrap().message, "chave de API revogada");

    // Revogação pelo comando local também.
    let k2 = d.admin().call("admin.keys.create", json!({ "user": "bob" })).unwrap();
    let bob2 = d.client(k2["token"].as_str().unwrap());
    assert!(bob2.call("whoami", json!({})).is_ok());
    admin_cli(&d.data_dir(), &["key", "revoke", k2["key_id"].as_str().unwrap()]);
    assert_eq!(rpc_code(&bob2.call("whoami", json!({})).unwrap_err()), codes::UNAUTHORIZED);

    // Usuário desativado: as chaves param; reativado, voltam.
    let k3 = d.admin().call("admin.keys.create", json!({ "user": "bob", "expires": "never" })).unwrap();
    let bob3 = d.client(k3["token"].as_str().unwrap());
    d.admin().call("admin.users.update", json!({ "name": "bob", "disabled": true })).unwrap();
    assert_eq!(bob3.call("whoami", json!({})).unwrap_err().rpc().unwrap().message, "usuário desativado");
    d.admin().call("admin.users.update", json!({ "name": "bob", "disabled": false })).unwrap();
    assert!(bob3.call("whoami", json!({})).is_ok());

    // O último admin não pode ser rebaixado.
    let e = d.admin().call("admin.users.update", json!({ "name": "admin", "role": "user" })).unwrap_err();
    assert_eq!(rpc_code(&e), codes::INVALID_PARAMS);

    // O arquivo de chaves não tem segredo nenhum.
    let auth = std::fs::read_to_string(d.data_dir().join("auth.json")).unwrap();
    assert!(!auth.contains(&d.admin_token[21..]));
    assert!(!auth.contains(&token[21..]));
}

#[test]
fn quotas_and_admission_control() {
    let d = Daemon::fake("[sandbox]\nmem_bytes = \"128MiB\"\n");
    let t = d.user(
        "carol",
        json!({ "max_sandboxes": 2, "mem_bytes": 300 << 20, "max_concurrent_execs": 1, "max_timeout_ms": 60_000, "max_sessions": 1 }),
    );
    let c = d.client(&t);
    let a = sandbox(&c);
    let b = sandbox(&c);
    let e = c.call("sandbox.create", json!({})).unwrap_err();
    assert_eq!(rpc_code(&e), codes::QUOTA_EXCEEDED);
    assert_eq!(e.rpc().unwrap().data.as_ref().unwrap()["resource"], "sandboxes");
    c.call("sandbox.destroy", json!({ "sandbox_id": b })).unwrap();
    // Memória: 128 MiB em uso, pedir 200 MiB passa dos 300 MiB.
    let e = c.call("sandbox.create", json!({ "limits": { "mem_bytes": 200 << 20 } })).unwrap_err();
    assert_eq!(e.rpc().unwrap().data.as_ref().unwrap()["resource"], "mem_bytes");
    let b = sandbox(&c);

    // Um exec por vez.
    let c2 = d.client(&t);
    let a2 = a.clone();
    let h = thread::spawn(move || exec(&c2, &a2, "sleep 1.5"));
    thread::sleep(Duration::from_millis(400));
    let e = c.call("exec", json!({ "sandbox_id": b, "command": "true" })).unwrap_err();
    assert_eq!(e.rpc().unwrap().data.as_ref().unwrap()["resource"], "concurrent_execs");
    assert_eq!(h.join().unwrap()["exit_code"], 0);
    assert_eq!(exec(&c, &b, "true")["exit_code"], 0, "a vaga volta quando o exec termina");

    // Timeout acima do máximo do usuário é recusado; sessões têm teto.
    let e = c.call("exec", json!({ "sandbox_id": a, "command": "true", "timeout_ms": 120_000 })).unwrap_err();
    assert_eq!(rpc_code(&e), codes::INVALID_PARAMS);
    c.call("session.open", json!({ "sandbox_id": a })).unwrap();
    let e = c.call("session.open", json!({ "sandbox_id": a })).unwrap_err();
    assert_eq!(e.rpc().unwrap().data.as_ref().unwrap()["resource"], "sessions");

    // Parâmetro desconhecido é erro, não silêncio.
    let e = c.call("exec", json!({ "sandbox_id": a, "comand": "true" })).unwrap_err();
    assert_eq!(rpc_code(&e), codes::INVALID_PARAMS);

    let who = c.call("whoami", json!({})).unwrap();
    assert_eq!(who["usage"]["sandboxes"], 2);
    assert_eq!(who["usage"]["mem_bytes"], 256 << 20);
}

#[test]
fn service_wide_admission() {
    let d = Daemon::fake("[service]\nmax_sandboxes = 2\n");
    let t1 = d.user("u1", json!({}));
    let t2 = d.user("u2", json!({}));
    sandbox(&d.client(&t1));
    sandbox(&d.client(&t1));
    let e = d.client(&t2).call("sandbox.create", json!({})).unwrap_err();
    assert_eq!(rpc_code(&e), codes::CAPACITY);
}

#[test]
fn two_users_concurrently_are_isolated() {
    let d = Daemon::fake("");
    let ta = d.user("alice", json!({ "max_concurrent_execs": 16 }));
    let tb = d.user("bruno", json!({ "max_concurrent_execs": 16 }));
    let (a, b) = (d.client(&ta), d.client(&tb));
    let sa = sandbox(&a);
    let sb = sandbox(&b);
    let la = a.call("sandbox.list", json!({})).unwrap();
    let lb = b.call("sandbox.list", json!({})).unwrap();
    assert_eq!(la["sandboxes"].as_array().unwrap().len(), 1);
    assert_eq!(lb["sandboxes"].as_array().unwrap().len(), 1);
    // Usuários diferentes vão pra workers diferentes (dois workers, um usuário em cada).
    assert_ne!(la["sandboxes"][0]["worker"], lb["sandboxes"][0]["worker"]);
    // A sandbox do outro não existe pra mim.
    let e = b.call("exec", json!({ "sandbox_id": sa, "command": "true" })).unwrap_err();
    assert_eq!(rpc_code(&e), codes::NOT_FOUND);
    let e = b.call("fs.read", json!({ "sandbox_id": sa, "path": "/etc/passwd" })).unwrap_err();
    assert_eq!(rpc_code(&e), codes::NOT_FOUND);

    let run = |tok: String, sb: String, who: &'static str| {
        let url = d.url.clone();
        thread::spawn(move || {
            let c = Client::new(&url, &tok);
            for i in 0..12 {
                let path = format!("/work/{who}-{i}");
                c.call("fs.write", json!({ "sandbox_id": sb, "path": path, "data": format!("{who} {i}") })).unwrap();
                let r = c.call("exec", json!({ "sandbox_id": sb, "argv": ["cat", path] })).unwrap();
                assert_eq!(r["stdout"], format!("{who} {i}"));
            }
        })
    };
    let mut hs = Vec::new();
    for _ in 0..4 {
        hs.push(run(ta.clone(), sa.clone(), "alice"));
        hs.push(run(tb.clone(), sb.clone(), "bruno"));
    }
    for h in hs {
        h.join().unwrap();
    }
    let la = a.call("fs.list", json!({ "sandbox_id": sa, "path": "/work" })).unwrap();
    assert!(la["entries"].as_array().unwrap().iter().all(|e| e["name"].as_str().unwrap().starts_with("alice")));
    // Admin enxerga tudo.
    let all = d.admin().call("sandbox.list", json!({ "all": true })).unwrap();
    assert_eq!(all["sandboxes"].as_array().unwrap().len(), 2);
}

#[test]
fn worker_crash_loses_or_recovers_sandboxes_and_comes_back() {
    let d = Daemon::fake("");
    let t = d.user("dora", json!({}));
    let c = d.client(&t);
    let keep = sandbox(&c);
    let lose = sandbox(&c);
    c.call("fs.write", json!({ "sandbox_id": keep, "path": "/work/f", "data": "sobrevive" })).unwrap();
    let snap = c.call("snapshot", json!({ "sandbox_id": keep, "persist": true, "name": "base" })).unwrap();
    assert!(snap["persisted_bytes"].as_u64().unwrap() > 0);
    c.call("fs.write", json!({ "sandbox_id": keep, "path": "/work/depois", "data": "se perde" })).unwrap();
    let sess = c.call("session.open", json!({ "sandbox_id": keep })).unwrap()["session_id"].as_str().unwrap().to_string();
    c.call("session.exec", json!({ "session_id": sess, "command": "cd /work; export B=2" })).unwrap();

    // O processo do worker aborta (o equivalente a um stack overflow que escapou do stacker).
    let e = c.call("exec", json!({ "sandbox_id": lose, "command": "crash" })).unwrap_err();
    assert_eq!(rpc_code(&e), codes::WORKER_CRASHED, "{e}");
    assert!(e.rpc().unwrap().message.contains("SIGABRT"), "{e}");

    // Sem snapshot: perdida, com o motivo.
    let e = c.call("exec", json!({ "sandbox_id": lose, "command": "true" })).unwrap_err();
    assert!(matches!(rpc_code(&e), codes::SANDBOX_LOST | codes::WORKER_UNAVAILABLE), "{e}");
    // O worker volta e a sandbox com snapshot persistido também, no estado do snapshot.
    d.wait_health(|v| v["status"] == "ok" && v["workers"].as_array().unwrap().iter().any(|w| w["restarts"] == 1), Duration::from_secs(20));
    let deadline = Instant::now() + Duration::from_secs(10);
    let info = loop {
        let l = c.call("sandbox.list", json!({})).unwrap();
        let s = l["sandboxes"].as_array().unwrap().iter().find(|s| s["sandbox_id"] == keep.as_str()).unwrap().clone();
        if s["state"] == "active" {
            break s;
        }
        assert!(Instant::now() < deadline, "não recuperou: {s}");
        thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(info["recovered_from"], snap["snapshot_id"]);
    let r = c.call("fs.read", json!({ "sandbox_id": keep, "path": "/work/f" })).unwrap();
    assert_eq!(r["data"], "sobrevive");
    // A sessão da sandbox recuperada volta no próximo comando, com o cwd e o ambiente de antes.
    let r = c.call("session.exec", json!({ "session_id": sess, "command": "pwd; printenv B" })).unwrap();
    assert_eq!(r["stdout"], "/work\n2\n", "{r}");
    assert_eq!(r["session_reset"], true);
    c.call("session.close", json!({ "session_id": sess })).unwrap();
    let e = c.call("fs.read", json!({ "sandbox_id": keep, "path": "/work/depois" })).unwrap_err();
    assert_eq!(rpc_code(&e), codes::OS_ERROR);
    let e = c.call("exec", json!({ "sandbox_id": lose, "command": "true" })).unwrap_err();
    assert_eq!(rpc_code(&e), codes::SANDBOX_LOST);
    assert!(e.rpc().unwrap().message.contains("caiu"), "{e}");

    // A sandbox perdida não conta mais na quota, e o usuário segue usando o serviço.
    assert_eq!(c.call("whoami", json!({})).unwrap()["usage"]["sandboxes"], 1);
    let fresh = sandbox(&c);
    assert_eq!(exec(&c, &fresh, "echo ok")["stdout"], "ok\n");
    c.call("sandbox.destroy", json!({ "sandbox_id": lose })).unwrap();
}

#[test]
fn wall_timeout_kills_the_whole_group() {
    let d = Daemon::fake("");
    let t = d.user("eva", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    let t0 = Instant::now();
    let r = c.call("exec", json!({ "sandbox_id": sb, "command": "sleep 30 &\nspin", "timeout_ms": 500 })).unwrap();
    assert!(t0.elapsed() < Duration::from_secs(5));
    assert_eq!(r["timed_out"], true);
    assert_eq!(r["status"], 137);
    assert_eq!(r["signal_name"], "SIGKILL");
    thread::sleep(Duration::from_millis(100));
    let ps = c.call("ps", json!({ "sandbox_id": sb })).unwrap();
    let alive: Vec<&Value> = ps["processes"].as_array().unwrap().iter().filter(|p| p["state"] != "Z").collect();
    assert!(alive.is_empty(), "{alive:?}");

    // Saída acima do limite: truncada, com o total.
    let r = c.call("exec", json!({ "sandbox_id": sb, "argv": ["bigout", "100000"], "output_limit_bytes": 100 })).unwrap();
    assert_eq!(r["stdout"].as_str().unwrap().len(), 100);
    assert_eq!(r["stdout_truncated"], true);
    assert_eq!(r["stdout_bytes"], 100_000);
}

#[test]
fn sessions_keep_state_and_reset_on_timeout() {
    let d = Daemon::fake("");
    let t = d.user("fabi", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    let s = c.call("session.open", json!({ "sandbox_id": sb, "env": { "A": "1" } })).unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let run = |cmd: &str, extra: Value| {
        let mut p = json!({ "session_id": s, "command": cmd });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        c.call("session.exec", p)
    };
    run("cd /work; export B=2", json!({})).unwrap();
    let r = run("pwd; printenv A; printenv B", json!({})).unwrap();
    assert_eq!(r["stdout"], "/work\n1\n2\n");
    assert_eq!(r["cwd"], "/work");
    let r = run("spin", json!({ "timeout_ms": 400 })).unwrap();
    assert_eq!(r["timed_out"], true);
    assert_eq!(r["session_reset"], true);
    let r = run("pwd; printenv B", json!({})).unwrap();
    assert_eq!(r["stdout"], "/work\n2\n");
    let r = run("exit 4", json!({})).unwrap();
    assert_eq!(r["session_closed"], true);
    assert_eq!(r["exit_code"], 4);
    let e = run("pwd", json!({})).unwrap_err();
    assert_eq!(rpc_code(&e), codes::SESSION_LOST);
    assert!(e.rpc().unwrap().message.contains("saiu com 4"), "{e}");
    c.call("session.close", json!({ "session_id": s })).unwrap();
}

#[test]
fn streaming_over_http_and_websocket() {
    let d = Daemon::fake("");
    let t = d.user("gabi", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    let mut got = String::new();
    let r = c
        .call_stream("exec.stream", json!({ "sandbox_id": sb, "command": "echo um; errout dois; echo três" }), |s, data| {
            if s == "stdout" {
                got.push_str(data);
            }
        })
        .unwrap();
    assert_eq!(got, "um\ntrês\n");
    assert_eq!(r["streamed"], true);
    assert_eq!(r["stdout_bytes"], "um\ntrês\n".len());

    // WebSocket: duas requisições ao mesmo tempo, notificações de streaming no meio.
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        use tokio_tungstenite::tungstenite::Message;
        let mut req = format!("{}/ws", d.url.replace("http://", "ws://")).into_client_request().unwrap();
        req.headers_mut().insert("Authorization", format!("Bearer {t}").parse().unwrap());
        let tcp = tokio::net::TcpStream::connect(d.url.trim_start_matches("http://")).await.unwrap();
        let (mut ws, _) = tokio_tungstenite::client_async(req, tcp).await.unwrap();
        ws.send(Message::text(
            json!({ "jsonrpc": "2.0", "id": "a", "method": "exec.stream", "params": { "sandbox_id": sb, "argv": ["bigout", "70000"] } }).to_string(),
        ))
        .await
        .unwrap();
        ws.send(Message::text(json!({ "jsonrpc": "2.0", "id": "b", "method": "whoami" }).to_string())).await.unwrap();
        let (mut streamed, mut done_a, mut done_b) = (0usize, false, false);
        while !(done_a && done_b) {
            let m = tokio::time::timeout(Duration::from_secs(10), ws.next()).await.unwrap().unwrap().unwrap();
            let v: Value = serde_json::from_str(m.to_text().unwrap()).unwrap();
            match (v.get("method").and_then(Value::as_str), v.get("id").and_then(Value::as_str)) {
                (Some("exec.output"), _) => {
                    assert_eq!(v["params"]["request_id"], "a");
                    streamed += v["params"]["data"].as_str().unwrap().len();
                }
                (_, Some("a")) => {
                    assert_eq!(v["result"]["stdout_bytes"], 70_000);
                    done_a = true;
                }
                (_, Some("b")) => {
                    assert_eq!(v["result"]["user"], "gabi");
                    done_b = true;
                }
                other => panic!("mensagem inesperada {other:?}: {v}"),
            }
        }
        assert_eq!(streamed, 70_000);
        ws.close(None).await.unwrap();
    });

    // WebSocket sem chave: 401 no upgrade.
    let e = rt.block_on(async {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let req = format!("{}/ws", d.url.replace("http://", "ws://")).into_client_request().unwrap();
        let tcp = tokio::net::TcpStream::connect(d.url.trim_start_matches("http://")).await.unwrap();
        tokio_tungstenite::client_async(req, tcp).await.err().unwrap().to_string()
    });
    assert!(e.contains("401"), "{e}");
}

fn osh(args: &[&str], stdin: &str, envs: &[(&str, &str)]) -> (i32, String, String) {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_osh"))
        .args(args)
        .env_remove("OSH_KEY")
        .env_remove("OSH_REMOTE")
        .envs(envs.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn osh_local_does_not_wait_for_stdin_eof() {
    // Stdin em pipe aberto e sem dados: o `bash -c` roda na hora, o osh também (antes lia até o EOF
    // e travava para sempre).
    let mut child = Command::new(env!("CARGO_BIN_EXE_osh"))
        .args(["--backend", "fake", "-c", "echo oi"])
        .env_remove("OSH_KEY")
        .env_remove("OSH_REMOTE")
        .env("PL_ALLOW_FAKE_BACKEND", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let held = child.stdin.take().unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(start.elapsed() < Duration::from_secs(20), "osh esperou o EOF do stdin");
        thread::sleep(Duration::from_millis(20));
    }
    drop(held);
    let mut out = String::new();
    child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    assert_eq!(out, "oi\n");
    // O que chega no pipe continua indo para o fd 0 do comando.
    let (rc, out, _) = osh(&["--backend", "fake", "-c", "cat"], "linha\n", &[("PL_ALLOW_FAKE_BACKEND", "1")]);
    assert_eq!((rc, out.as_str()), (0, "linha\n"));
}

#[test]
fn osh_remote_and_local() {
    let d = Daemon::fake("");
    let t = d.user("ivo", json!({}));
    let key_file = d.dir.path().join("ivo.key");
    std::fs::write(&key_file, &t).unwrap();
    let kf = key_file.to_str().unwrap();
    let (rc, out, err) = osh(&["--remote", &d.url, "--key-file", kf, "-c", "echo oi; errout ai; exit 3"], "", &[]);
    assert_eq!((rc, out.as_str(), err.as_str()), (3, "oi\n", "ai\n"));
    // Chave pela variável de ambiente; stdin do osh vira stdin do comando.
    let (rc, out, _) = osh(&["--remote", &d.url, "-c", "cat"], "pela entrada\n", &[("OSH_KEY", t.as_str())]);
    assert_eq!((rc, out.as_str()), (0, "pela entrada\n"));
    // Interativo com stdin em pipe: sessão persistente, `exit` define o status.
    let (rc, out, _) = osh(&["--remote", &d.url, "--key-file", kf], "cd /work\nexport Z=9\npwd; printenv Z\nexit 6\n", &[]);
    assert_eq!((rc, out.as_str()), (6, "/work\n9\n"));
    // O osh destrói as sandboxes que criou.
    let l = d.client(&t).call("sandbox.list", json!({})).unwrap();
    assert!(l["sandboxes"].as_array().unwrap().is_empty(), "{l}");
    // Chave errada: erro claro e status 2.
    let (rc, _, err) = osh(&["--remote", &d.url, "--key", "plk_errada", "-c", "true"], "", &[]);
    assert_eq!(rc, 2);
    assert!(err.contains("mal formada"), "{err}");
    // Modo local (sem daemon), com o backend dublê.
    let (rc, out, _) = osh(&["--backend", "fake", "-c", "echo local; exit 4"], "", &[("PL_ALLOW_FAKE_BACKEND", "1")]);
    assert_eq!((rc, out.as_str()), (4, "local\n"));
}

#[test]
fn hung_worker_is_killed_by_ping_timeout() {
    let d = Daemon::fake_with(&[("ping_interval_ms", 200), ("ping_timeout_ms", 1500)], "");
    let w = d.admin().call("admin.workers", json!({})).unwrap();
    let pid = w["workers"][0]["pid"].as_i64().unwrap() as i32;
    // SIGSTOP: o processo continua vivo mas não responde mais nada.
    rustix::process::kill_process(rustix::process::Pid::from_raw(pid).unwrap(), rustix::process::Signal::STOP).unwrap();
    let h = d.wait_health(|v| v["workers"][0]["restarts"] == 1 && v["workers"][0]["state"] == "ready", Duration::from_secs(20));
    assert_eq!(h["status"], "ok");
    let w = d.admin().call("admin.workers", json!({})).unwrap();
    assert!(w["workers"][0]["last_exit"].as_str().unwrap().contains("ping"), "{w}");
    assert_ne!(w["workers"][0]["pid"].as_i64().unwrap() as i32, pid);
}

#[test]
fn files_tar_and_snapshots() {
    let d = Daemon::fake("");
    let t = d.user("hugo", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    c.call("fs.mkdir", json!({ "sandbox_id": sb, "path": "/work/proj/src", "parents": true })).unwrap();
    c.call("fs.write", json!({ "sandbox_id": sb, "path": "/work/proj/src/main.rs", "data": "fn main() {}\n" })).unwrap();
    c.call("fs.write", json!({ "sandbox_id": sb, "path": "/work/proj/bin", "data_base64": "AAEC/w==", "mode": 0o755 })).unwrap();
    let st = c.call("fs.stat", json!({ "sandbox_id": sb, "path": "/work/proj/bin" })).unwrap();
    assert_eq!((st["size"].as_u64(), st["mode"].as_u64(), st["type"].as_str()), (Some(4), Some(0o755), Some("file")));
    let r = c.call("fs.read", json!({ "sandbox_id": sb, "path": "/work/proj/bin", "encoding": "base64" })).unwrap();
    assert_eq!(r["data"], "AAEC/w==");
    let r = c.call("fs.read", json!({ "sandbox_id": sb, "path": "/work/proj/bin" })).unwrap();
    assert_eq!(r["lossy"], true);
    let e = c.call("fs.read", json!({ "sandbox_id": sb, "path": "/work/nada" })).unwrap_err();
    assert_eq!(e.rpc().unwrap().message, "/work/nada: No such file or directory");
    assert_eq!(e.rpc().unwrap().data.as_ref().unwrap()["errno_name"], "ENOENT");

    // Snapshot em memória e restore.
    let snap = c.call("snapshot", json!({ "sandbox_id": sb })).unwrap();
    c.call("fs.remove", json!({ "sandbox_id": sb, "path": "/work/proj", "recursive": true })).unwrap();
    assert!(c.call("fs.stat", json!({ "sandbox_id": sb, "path": "/work/proj" })).is_err());
    c.call("restore", json!({ "sandbox_id": sb, "snapshot_id": snap["snapshot_id"] })).unwrap();
    assert!(c.call("fs.stat", json!({ "sandbox_id": sb, "path": "/work/proj/src/main.rs" })).is_ok());

    // Export de uma sandbox, import em outra.
    let tar = c.call("export", json!({ "sandbox_id": sb, "path": "/work/proj" })).unwrap();
    let other = sandbox(&c);
    let rep = c.call("import", json!({ "sandbox_id": other, "path": "/work/copia", "data_base64": tar["data_base64"] })).unwrap();
    assert_eq!(rep["report"]["files"], 2);
    let r = c.call("exec", json!({ "sandbox_id": other, "argv": ["cat", "/work/copia/src/main.rs"] })).unwrap();
    assert_eq!(r["stdout"], "fn main() {}\n");

    // Corpo declarado acima do teto: 413 com erro JSON-RPC, antes de o corpo ser lido.
    use std::io::Write;
    let mut tcp = std::net::TcpStream::connect(d.url.trim_start_matches("http://")).unwrap();
    write!(
        tcp,
        "POST /rpc HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {t}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        100u64 << 20
    )
    .unwrap();
    let mut resp = String::new();
    let _ = tcp.read_to_string(&mut resp);
    assert!(resp.starts_with("HTTP/1.1 413"), "{resp}");
    assert!(resp.contains("\"code\":-32008"), "{resp}");
}
