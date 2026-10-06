//! Integração do daemon com o kernel de verdade (`crates/kernel` + `crates/userland` + os programas de
//! teste do host em `/usr/local/bin/pl-*`), com Landlock e seccomp aplicados nas spawner threads.
//!
//! O shell ainda não exporta `programs()`, então tudo aqui usa `argv` (sem `bash -c`); as sessões e o
//! `command` entram quando o `bash` estiver na tabela.

mod common;

use std::thread;
use std::time::{Duration, Instant};

use common::*;
use host::client::Client;
use host::rpc::codes;
use serde_json::{Value, json};

fn run(c: &Client, sb: &str, argv: &[&str], extra: Value) -> Value {
    let mut p = json!({ "sandbox_id": sb, "argv": argv });
    p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    c.call("exec", p).unwrap()
}

#[test]
fn userland_runs_with_isolation_applied() {
    let d = Daemon::kernel("");
    let t = d.user("ana", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    let r = run(&c, &sb, &["cat"], json!({ "stdin": "linha 2\nlinha 1\n" }));
    assert_eq!(r["stdout"], "linha 2\nlinha 1\n");
    let r = run(&c, &sb, &["sort"], json!({ "stdin": "b\na\nc\n" }));
    assert_eq!(r["stdout"], "a\nb\nc\n");
    let r = run(&c, &sb, &["wc", "-l"], json!({ "stdin": "1\n2\n3\n" }));
    assert_eq!(r["stdout"], "3\n");
    let r = run(&c, &sb, &["ls", "/usr/local/bin"], json!({}));
    assert!(r["stdout"].as_str().unwrap().contains("pl-spin"), "{r}");
    // Erro de programa do userland: mensagem e status do GNU.
    let r = run(&c, &sb, &["cat", "/nao/existe"], json!({}));
    assert_eq!(r["exit_code"], 1);
    assert_eq!(r["stderr"], "cat: /nao/existe: No such file or directory\n");
    let r = run(&c, &sb, &["pl-exit", "7"], json!({}));
    assert_eq!(r["status"], 7);
    // Programa inexistente: o erro do execvp.
    let e = c.call("exec", json!({ "sandbox_id": sb, "argv": ["nao-existe"] })).unwrap_err();
    assert_eq!(rpc_code(&e), codes::OS_ERROR);
    assert_eq!(e.rpc().unwrap().message, "nao-existe: No such file or directory");
    // O hook da spawner rodou: o worker informa o isolamento aplicado.
    let w = d.admin().call("admin.workers", json!({})).unwrap();
    let iso: Vec<String> = w["workers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w["backend"]["isolation"].as_str().map(str::to_string))
        .collect();
    assert!(iso.iter().any(|s| s.contains("seccomp=on") && s.contains("landlock=fully_enforced")), "{iso:?}");
}

#[test]
fn timeout_output_limit_and_background() {
    let d = Daemon::kernel("[sandbox]\nmax_discard_bytes = 1048576\n");
    let t = d.user("bia", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    let t0 = Instant::now();
    let r = run(&c, &sb, &["pl-spin"], json!({ "timeout_ms": 400 }));
    assert!(t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());
    assert_eq!(r["timed_out"], true);
    assert_eq!(r["signal_name"], "SIGKILL");
    assert_eq!(r["status"], 137);
    thread::sleep(Duration::from_millis(100));
    assert!(live_processes(&c, &sb).is_empty(), "{:?}", live_processes(&c, &sb));

    let r = run(&c, &sb, &["pl-bigout", "300000"], json!({ "output_limit_bytes": 1000 }));
    assert_eq!(r["stdout"].as_str().unwrap().len(), 1000);
    assert_eq!(r["stdout_truncated"], true);
    assert_eq!(r["stdout_bytes"], 300_000);
    assert_eq!(r["exit_code"], 0);
    // Saída sem fim: passou do teto de descarte, o host fecha o pipe e o escritor leva SIGPIPE.
    let r = run(&c, &sb, &["pl-bigout", "100000000000"], json!({ "output_limit_bytes": 1000 }));
    assert_eq!(r["output_closed"], true, "{r}");
    assert_eq!(r["signal_name"], "SIGPIPE", "{r}");

    // Filho em segundo plano segurando o stdout: o exec volta depois do escoamento.
    let t0 = Instant::now();
    let r = run(&c, &sb, &["pl-bg", "30"], json!({}));
    assert!(t0.elapsed() < Duration::from_secs(3), "{:?}", t0.elapsed());
    assert_eq!(r["exit_code"], 0);
    assert_eq!(r["background_detached"], true);
    let alive = live_processes(&c, &sb);
    assert_eq!(alive.len(), 1, "{alive:?}");
    let pid = alive[0]["pid"].as_i64().unwrap();
    c.call("kill", json!({ "sandbox_id": sb, "pid": pid, "signal": "KILL" })).unwrap();
    thread::sleep(Duration::from_millis(100));
    assert!(live_processes(&c, &sb).is_empty());
}

#[test]
fn files_snapshots_and_tar_on_the_real_vfs() {
    let d = Daemon::kernel("");
    let t = d.user("caio", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    c.call("fs.write", json!({ "sandbox_id": sb, "path": "/work/p/a.txt", "data": "alfa\n", "create_parents": true })).unwrap();
    let r = run(&c, &sb, &["cat", "/work/p/a.txt"], json!({}));
    assert_eq!(r["stdout"], "alfa\n");
    let st = c.call("fs.stat", json!({ "sandbox_id": sb, "path": "/work/p/a.txt" })).unwrap();
    assert_eq!((st["size"].as_u64(), st["mode"].as_u64()), (Some(5), Some(0o644)));
    let l = c.call("fs.list", json!({ "sandbox_id": sb, "path": "/etc" })).unwrap();
    assert!(l["entries"].as_array().unwrap().iter().any(|e| e["name"] == "os-release"), "{l}");

    let snap = c.call("snapshot", json!({ "sandbox_id": sb })).unwrap();
    c.call("fs.remove", json!({ "sandbox_id": sb, "path": "/work/p", "recursive": true })).unwrap();
    assert_eq!(run(&c, &sb, &["cat", "/work/p/a.txt"], json!({}))["exit_code"], 1);
    c.call("restore", json!({ "sandbox_id": sb, "snapshot_id": snap["snapshot_id"] })).unwrap();
    assert_eq!(run(&c, &sb, &["cat", "/work/p/a.txt"], json!({}))["stdout"], "alfa\n");

    let tar = c.call("export", json!({ "sandbox_id": sb, "path": "/work/p" })).unwrap();
    let other = sandbox(&c);
    c.call("import", json!({ "sandbox_id": other, "path": "/work/q", "data_base64": tar["data_base64"] })).unwrap();
    assert_eq!(run(&c, &other, &["cat", "/work/q/a.txt"], json!({}))["stdout"], "alfa\n");
}

/// Listagem de metadados de uma árvore (tipo, modo, tamanho, alvo do link, mtime, nlink), ordenada.
fn tree_meta(c: &Client, sb: &str, root: &str) -> String {
    let r = run(c, sb, &["find", root, "-printf", "%p|%y|%m|%s|%l|%T@|%n|%U|%G\\n"], json!({}));
    assert_eq!(r["exit_code"], 0, "{r}");
    let mut lines: Vec<&str> = r["stdout"].as_str().unwrap().lines().collect();
    lines.sort_unstable();
    lines.join("\n")
}

#[test]
fn persisted_snapshot_restores_the_whole_tree_after_a_crash() {
    let d = Daemon::kernel("");
    let t = d.user("elis", json!({}));
    let c = d.client(&t);
    let keep = sandbox(&c);
    let lose = sandbox(&c);
    // Binário com todos os bytes, arquivo grande, vazio, diretório vazio, links, modos e mtime fixo.
    let big: Vec<u8> = (0..300_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD;
    c.call("fs.write", json!({ "sandbox_id": keep, "path": "/home/user/t/big.bin", "data_base64": b64.encode(&big), "create_parents": true })).unwrap();
    c.call("fs.write", json!({ "sandbox_id": keep, "path": "/home/user/t/empty", "data": "" })).unwrap();
    c.call("fs.write", json!({ "sandbox_id": keep, "path": "/home/user/t/ação.txt", "data": "utf8\n" })).unwrap();
    for argv in [
        vec!["mkdir", "-p", "/home/user/t/d/vazio"],
        vec!["ln", "-s", "ação.txt", "/home/user/t/sym"],
        vec!["ln", "-s", "/nao/existe", "/home/user/t/dangling"],
        vec!["ln", "/home/user/t/ação.txt", "/home/user/t/hard"],
        vec!["chmod", "600", "/home/user/t/empty"],
        vec!["chmod", "4755", "/home/user/t/big.bin"],
        vec!["chmod", "1777", "/home/user/t/d"],
        vec!["touch", "-d", "2020-02-29 12:34:56", "/home/user/t/ação.txt"],
        vec!["touch", "-d", "2001-09-09 01:46:40", "/home/user/t/d"],
    ] {
        let r = run(&c, &keep, &argv, json!({}));
        assert_eq!(r["exit_code"], 0, "{argv:?}: {r}");
    }
    let before = tree_meta(&c, &keep, "/home/user/t");
    let exported = c.call("export", json!({ "sandbox_id": keep, "path": "/home/user/t" })).unwrap();
    let raw = b64.decode(exported["data_base64"].as_str().unwrap()).unwrap();
    let listing: Vec<String> = tar::Archive::new(&raw[..])
        .entries()
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            format!("{:?} {} -> {:?}", e.header().entry_type(), String::from_utf8_lossy(&e.path_bytes()), e.link_name_bytes().map(|l| String::from_utf8_lossy(&l).into_owned()))
        })
        .collect();
    assert!(listing.iter().any(|l| l.starts_with("Link ")), "o export não gerou hardlink: {listing:#?}");
    let snap = c.call("snapshot", json!({ "sandbox_id": keep, "persist": true })).unwrap();
    // Mudança posterior ao snapshot: tem que sumir na recuperação.
    c.call("fs.write", json!({ "sandbox_id": keep, "path": "/home/user/t/depois", "data": "x" })).unwrap();

    let e = c.call("exec", json!({ "sandbox_id": lose, "argv": ["pl-crash"] })).unwrap_err();
    assert_eq!(rpc_code(&e), codes::WORKER_CRASHED, "{e}");
    d.wait_health(|v| v["status"] == "ok" && v["workers"].as_array().unwrap().iter().any(|w| w["restarts"] == 1), Duration::from_secs(30));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let l = c.call("sandbox.list", json!({})).unwrap();
        let s = l["sandboxes"].as_array().unwrap().iter().find(|s| s["sandbox_id"] == keep.as_str()).unwrap().clone();
        if s["state"] == "active" {
            assert_eq!(s["recovered_from"], snap["snapshot_id"]);
            break;
        }
        assert!(Instant::now() < deadline, "não recuperou: {s}\n{}", d.log());
        thread::sleep(Duration::from_millis(100));
    }
    let after = run(&c, &keep, &["find", "/home/user/t", "-name", "depois"], json!({}));
    assert_eq!(after["stdout"], "", "o que veio depois do snapshot não pode voltar");
    let mut after_meta = tree_meta(&c, &keep, "/home/user/t");
    // `depois` não existe mais; `before` foi tirado antes dela, então a comparação é direta.
    assert_eq!(after_meta, before, "metadados divergem após a recuperação");
    let r = c.call("fs.read", json!({ "sandbox_id": keep, "path": "/home/user/t/big.bin", "encoding": "base64" })).unwrap();
    assert_eq!(b64.decode(r["data"].as_str().unwrap()).unwrap(), big);
    after_meta.clear();
}

#[test]
fn session_restores_the_whole_shell_state_after_a_reset() {
    let d = Daemon::kernel("");
    let t = d.user("elis", json!({}));
    let c = d.client(&t);
    let sb = sandbox(&c);
    let s = c.call("session.open", json!({ "sandbox_id": sb })).unwrap()["session_id"].as_str().unwrap().to_string();
    let exec = |cmd: &str, extra: Value| {
        let mut p = json!({ "session_id": s, "command": cmd });
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        c.call("session.exec", p).unwrap()
    };
    // Variável não exportada, array, função, alias, shopt e set -o: nada disso está no ambiente.
    exec(
        "plain=local; arr=(a b c); greet() { echo \"oi $1\"; }; alias ll='ls -l'; shopt -s expand_aliases; set -o noclobber",
        json!({}),
    );
    // Um timeout mata o shell e sobe outro só com o que foi guardado.
    let r = exec("sleep 30", json!({ "timeout_ms": 500 }));
    assert_eq!(r["session_reset"], true, "{r}");
    let r = exec(
        "echo \"$plain ${arr[1]}\"; greet mundo; alias ll; shopt -q expand_aliases && echo shopt; set +o | grep noclobber",
        json!({}),
    );
    assert_eq!(r["stdout"], "local b\noi mundo\nalias ll='ls -l'\nshopt\nset -o noclobber\n", "{r}");
    c.call("session.close", json!({ "session_id": s })).unwrap();
}

#[test]
fn autosave_recovers_the_sandbox_and_the_session_without_any_manual_snapshot() {
    let d = Daemon::kernel("[sandbox]\nautosave_secs = 1\n");
    let t = d.user("iara", json!({}));
    let c = d.client(&t);
    let keep = sandbox(&c);
    let lose = sandbox(&c);
    // Nenhum `snapshot` pedido: quem persiste é o host.
    c.call("fs.write", json!({ "sandbox_id": keep, "path": "/work/f", "data": "autosalvo" })).unwrap();
    let sess = c.call("session.open", json!({ "sandbox_id": keep })).unwrap()["session_id"].as_str().unwrap().to_string();
    c.call("session.exec", json!({ "session_id": sess, "command": "cd /work; plain=local; export B=2" })).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let l = c.call("sandbox.list", json!({})).unwrap();
        let s = l["sandboxes"].as_array().unwrap().iter().find(|s| s["sandbox_id"] == keep.as_str()).unwrap().clone();
        if s["snapshots"].as_array().is_some_and(|a| a.iter().any(|x| x["persisted"] == true)) {
            break;
        }
        assert!(Instant::now() < deadline, "o autosave não aconteceu: {s}\n{}", d.log());
        thread::sleep(Duration::from_millis(200));
    }
    let e = c.call("exec", json!({ "sandbox_id": lose, "argv": ["pl-crash"] })).unwrap_err();
    assert_eq!(rpc_code(&e), codes::WORKER_CRASHED, "{e}");
    d.wait_health(|v| v["status"] == "ok" && v["workers"].as_array().unwrap().iter().any(|w| w["restarts"] == 1), Duration::from_secs(30));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let l = c.call("sandbox.list", json!({})).unwrap();
        let s = l["sandboxes"].as_array().unwrap().iter().find(|s| s["sandbox_id"] == keep.as_str()).unwrap().clone();
        if s["state"] == "active" {
            break;
        }
        assert!(Instant::now() < deadline, "não recuperou: {s}\n{}", d.log());
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(run(&c, &keep, &["cat", "/work/f"], json!({}))["stdout"], "autosalvo");
    let r = c.call("session.exec", json!({ "session_id": sess, "command": "pwd; echo $B $plain" })).unwrap();
    assert_eq!(r["stdout"], "/work\n2 local\n", "{r}");
}

#[test]
fn daemon_restart_brings_back_sandboxes_files_and_sessions() {
    // Autosave de 1 hora: o que volta aqui só pode vir do salvamento feito no desligamento.
    let mut d = Daemon::kernel("[sandbox]\nautosave_secs = 3600\n");
    let t = d.user("jade", json!({}));
    let c = d.client(&t);
    let sb = c.call("sandbox.create", json!({ "labels": { "projeto": "site" } })).unwrap()["sandbox_id"].as_str().unwrap().to_string();
    let gone = sandbox(&c);
    c.call("fs.write", json!({ "sandbox_id": sb, "path": "/work/a.txt", "data": "alfa", "create_parents": true })).unwrap();
    let sess = c.call("session.open", json!({ "sandbox_id": sb })).unwrap()["session_id"].as_str().unwrap().to_string();
    c.call(
        "session.exec",
        json!({ "session_id": sess, "command": "cd /work; plain=viva; export B=2; greet() { echo \"oi $1\"; }" }),
    )
    .unwrap();
    c.call("sandbox.destroy", json!({ "sandbox_id": gone })).unwrap();

    d.restart();

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let l = c.call("sandbox.list", json!({})).unwrap();
        let all = l["sandboxes"].as_array().unwrap();
        assert_eq!(all.len(), 1, "só a sandbox viva volta; a destruída não: {l}");
        if all[0]["state"] == "active" {
            assert_eq!(all[0]["sandbox_id"], sb.as_str());
            assert_eq!(all[0]["labels"]["projeto"], "site");
            break;
        }
        assert!(Instant::now() < deadline, "não voltou: {l}\n{}", d.log());
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(run(&c, &sb, &["cat", "/work/a.txt"], json!({}))["stdout"], "alfa");
    let r = c.call("session.exec", json!({ "session_id": sess, "command": "pwd; echo $B $plain; greet mundo" })).unwrap();
    assert_eq!(r["stdout"], "/work\n2 viva\noi mundo\n", "{r}");
    // Outro usuário não enxerga a sandbox de volta.
    let other = d.user("kai", json!({}));
    assert!(d.client(&other).call("exec", json!({ "sandbox_id": sb, "argv": ["cat", "/work/a.txt"] })).is_err());
    // E continua persistindo: um segundo restart ainda a encontra.
    c.call("fs.write", json!({ "sandbox_id": sb, "path": "/work/b.txt", "data": "beta" })).unwrap();
    d.restart();
    let deadline = Instant::now() + Duration::from_secs(30);
    while c.call("sandbox.list", json!({})).unwrap()["sandboxes"][0]["state"] != "active" {
        assert!(Instant::now() < deadline, "não voltou no segundo restart\n{}", d.log());
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(run(&c, &sb, &["cat", "/work/b.txt"], json!({}))["stdout"], "beta");
}

#[test]
fn worker_crash_with_real_kernel() {
    let d = Daemon::kernel("");
    let t = d.user("davi", json!({}));
    let c = d.client(&t);
    let keep = sandbox(&c);
    let lose = sandbox(&c);
    c.call("fs.write", json!({ "sandbox_id": keep, "path": "/work/f", "data": "fica" })).unwrap();
    c.call("snapshot", json!({ "sandbox_id": keep, "persist": true })).unwrap();
    let sess = c.call("session.open", json!({ "sandbox_id": keep })).unwrap()["session_id"].as_str().unwrap().to_string();
    c.call(
        "session.exec",
        json!({ "session_id": sess, "command": "cd /work; plain=local; greet() { echo \"oi $1\"; }; export B=2" }),
    )
    .unwrap();
    let e = c.call("exec", json!({ "sandbox_id": lose, "argv": ["pl-crash"] })).unwrap_err();
    assert_eq!(rpc_code(&e), codes::WORKER_CRASHED, "{e}");
    d.wait_health(|v| v["status"] == "ok" && v["workers"].as_array().unwrap().iter().any(|w| w["restarts"] == 1), Duration::from_secs(30));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let l = c.call("sandbox.list", json!({})).unwrap();
        let s = l["sandboxes"].as_array().unwrap().iter().find(|s| s["sandbox_id"] == keep.as_str()).unwrap().clone();
        if s["state"] == "active" {
            break;
        }
        assert!(Instant::now() < deadline, "não recuperou: {s}\n{}", d.log());
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(run(&c, &keep, &["cat", "/work/f"], json!({}))["stdout"], "fica");
    // A sessão volta com o shell de antes: cwd, exportada, não exportada e função.
    let r = c
        .call("session.exec", json!({ "session_id": sess, "command": "pwd; echo $B $plain; greet mundo" }))
        .unwrap();
    assert_eq!(r["stdout"], "/work\n2 local\noi mundo\n", "{r}");
    assert_eq!(r["session_reset"], true);
    c.call("session.close", json!({ "session_id": sess })).unwrap();
    // Os programas do userland continuam lá depois da recuperação (vieram pelo tar).
    assert_eq!(run(&c, &keep, &["wc", "-c"], json!({ "stdin": "abc" }))["stdout"], "3\n");
    let e = c.call("exec", json!({ "sandbox_id": lose, "argv": ["cat"] })).unwrap_err();
    assert_eq!(rpc_code(&e), codes::SANDBOX_LOST);
}

#[test]
fn process_limit_of_the_sandbox() {
    let d = Daemon::kernel("");
    let t = d.user("eli", json!({}));
    let c = d.client(&t);
    let sb = c.call("sandbox.create", json!({ "limits": { "max_procs": 1 } })).unwrap()["sandbox_id"].as_str().unwrap().to_string();
    // O pl-bg ocupa o único processo e não consegue criar o filho.
    let r = run(&c, &sb, &["pl-bg", "5"], json!({}));
    assert_eq!(r["exit_code"], 1, "{r}");
    assert!(r["stderr"].as_str().unwrap().contains("Resource temporarily unavailable"), "{r}");
}

#[test]
fn two_users_in_parallel_on_the_kernel() {
    let d = Daemon::kernel("");
    let ta = d.user("fred", json!({ "max_concurrent_execs": 8 }));
    let tb = d.user("gil", json!({ "max_concurrent_execs": 8 }));
    let sa = sandbox(&d.client(&ta));
    let sb = sandbox(&d.client(&tb));
    let mut hs = Vec::new();
    for (tok, sbx, who) in [(ta.clone(), sa.clone(), "fred"), (tb.clone(), sb.clone(), "gil")] {
        for k in 0..4 {
            let (url, tok, sbx) = (d.url.clone(), tok.clone(), sbx.clone());
            hs.push(thread::spawn(move || {
                let c = Client::new(&url, &tok);
                for i in 0..8 {
                    let path = format!("/work/{who}-{k}-{i}");
                    c.call("fs.write", json!({ "sandbox_id": sbx, "path": path, "data": format!("{who}{k}{i}") })).unwrap();
                    let r = c.call("exec", json!({ "sandbox_id": sbx, "argv": ["cat", path] })).unwrap();
                    assert_eq!(r["stdout"], format!("{who}{k}{i}"));
                }
            }));
        }
    }
    for h in hs {
        h.join().unwrap();
    }
    let la = d.client(&ta).call("fs.list", json!({ "sandbox_id": sa, "path": "/work" })).unwrap();
    assert_eq!(la["entries"].as_array().unwrap().len(), 32);
    assert!(la["entries"].as_array().unwrap().iter().all(|e| e["name"].as_str().unwrap().starts_with("fred")));
}
