//! O perfil em disco do navegador contra a árvore que o Chromium 154 do oráculo cria (`testbench/golden/chromium/
//! profile/`, medida por `testbench/oracle/measure-profile.sh`): os mesmos caminhos, tipos, modos e tamanhos, ignorando
//! só o que o oráculo varia de uma execução para outra (o nome do `Session_*`, do `Tabs_*` e do diretório de `blob_storage`, o
//! tamanho dos JSON, os arquivos temporários que o SIGTERM deixa).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use serde_json::Value;
use sysabi::testkit::{TestKit, TreeEntry};
use sysabi::{Ctx, Program};
use ul_chromium::disk_profile::{DiskProfile, Run};
use ul_chromium::session_storage::Exit;

const USER_DATA_DIR: &[u8] = b"/tmp/profile";

fn start(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    let _profile = DiskProfile::start(Some(USER_DATA_DIR), Run::DevTools);
    0
}

fn start_and_finish(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    let profile = DiskProfile::start(Some(USER_DATA_DIR), Run::DevTools);
    profile.finish(Exit::Signal);
    0
}

fn start_and_finish_dump(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    let profile = DiskProfile::start(Some(USER_DATA_DIR), Run::DumpDom);
    profile.finish(Exit::DumpDom);
    0
}

fn start_without_user_data_dir(_ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    let _profile = DiskProfile::start(None, Run::DevTools);
    0
}

fn kit() -> TestKit {
    TestKit::new()
        .env("HOME", "/root")
        .programs([Program::bin("start", start), Program::bin("finish", start_and_finish), Program::bin("finish-dump", start_and_finish_dump), Program::bin("bare", start_without_user_data_dir)])
}

#[derive(Debug, PartialEq)]
struct Item {
    kind: char,
    mode: u32,
    /// Só de arquivo.
    size: Option<usize>,
}

/// O nome que varia por execução vira um nome fixo, nos dois lados.
fn normalize(path: &str) -> String {
    let mut parts: Vec<String> = path.split('/').map(str::to_owned).collect();
    for index in 0..parts.len() {
        for prefix in ["Session_", "Tabs_"] {
            if parts[index].starts_with(prefix) && parts[index][prefix.len()..].chars().all(|c| c.is_ascii_digit()) {
                parts[index] = format!("{prefix}N");
            }
        }
        if index > 0 && parts[index - 1] == "blob_storage" && parts[index].len() == 36 {
            parts[index] = "<uuid>".to_owned();
        }
    }
    parts.join("/")
}

/// O que o oráculo deixa de volta e o molde não leva: os `Singleton*` (conferidos à parte), os temporários do
/// shutdown e o `Network Persistent State`, que só existe em parte das execuções.
fn excluded(path: &str) -> bool {
    matches!(path, "SingletonCookie" | "SingletonLock" | "SingletonSocket" | "Default/Network Persistent State") || path.contains(".org.chromium.Chromium.")
}

fn golden(name: &str) -> BTreeMap<String, Item> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testbench/golden/chromium/profile").join(name);
    let mut out = BTreeMap::new();
    for line in std::fs::read_to_string(file).unwrap().lines() {
        let entry: Value = serde_json::from_str(line).unwrap();
        let path = entry["path"].as_str().unwrap();
        if excluded(path) {
            continue;
        }
        let kind = entry["type"].as_str().unwrap().chars().next().unwrap();
        let mode = u32::from_str_radix(entry["mode"].as_str().unwrap().trim_start_matches("0o"), 8).unwrap();
        let size = (kind == 'f').then(|| entry["size"].as_u64().unwrap() as usize);
        out.insert(normalize(path), Item { kind, mode, size });
    }
    out
}

fn actual(kit: &TestKit, root: &str) -> BTreeMap<String, Item> {
    let mut out = BTreeMap::new();
    for (path, entry) in kit.tree(root) {
        let path = String::from_utf8(path).unwrap();
        if excluded(&path) {
            continue;
        }
        let item = match entry {
            TreeEntry::File { data, mode } => Item { kind: 'f', mode: mode & 0o7777, size: Some(data.len()) },
            TreeEntry::Dir { mode } => Item { kind: 'd', mode: mode & 0o7777, size: None },
            TreeEntry::Symlink { .. } => Item { kind: 'l', mode: 0o777, size: None },
            TreeEntry::Other { mode } => Item { kind: 'o', mode: mode & 0o7777, size: None },
        };
        out.insert(normalize(&path), item);
    }
    out.remove("");
    out
}

/// Os JSON do perfil mudam de tamanho com o número de dígitos dos valores aleatórios.
fn size_tolerance(path: &str) -> usize {
    match path {
        "Local State" | "Default/Preferences" => 16,
        _ => 0,
    }
}

fn assert_matches_oracle(actual: &BTreeMap<String, Item>, expected: &BTreeMap<String, Item>) {
    let missing: Vec<&String> = expected.keys().filter(|p| !actual.contains_key(*p)).collect();
    let extra: Vec<&String> = actual.keys().filter(|p| !expected.contains_key(*p)).collect();
    assert!(missing.is_empty() && extra.is_empty(), "faltam {missing:?}, sobram {extra:?}");
    let mut wrong = Vec::new();
    for (path, want) in expected {
        let got = &actual[path];
        let size_ok = match (got.size, want.size) {
            (Some(a), Some(b)) => a.abs_diff(b) <= size_tolerance(path),
            _ => true,
        };
        if got.kind != want.kind || got.mode != want.mode || !size_ok {
            wrong.push(format!("{path}: {got:?} em vez de {want:?}"));
        }
    }
    assert!(wrong.is_empty(), "{} diferenças:\n{}", wrong.len(), wrong.join("\n"));
}

#[test]
fn first_use_tree_matches_the_oracle() {
    let kit = kit();
    assert_eq!(kit.run(&["start"], b"").status.shell_status(), 0);
    assert_matches_oracle(&actual(&kit, "/tmp/profile"), &golden("live-tree.jsonl"));
}

#[test]
fn clean_exit_tree_matches_the_oracle() {
    let kit = kit();
    assert_eq!(kit.run(&["finish"], b"").status.shell_status(), 0);
    assert_matches_oracle(&actual(&kit, "/tmp/profile"), &golden("exit-tree.jsonl"));
}

/// A saída do `--dump-dom` (medida com `measure-profile.sh dump-copy`): apaga os bancos que o carregamento curto não
/// chega a usar, os `Singleton*` e o diretório do soquete em `/tmp`.
#[test]
fn dump_dom_exit_tree_matches_the_oracle() {
    let kit = kit();
    assert_eq!(kit.run(&["finish-dump"], b"").status.shell_status(), 0);
    assert_matches_oracle(&actual(&kit, "/tmp/profile"), &golden("dump-exit-tree.jsonl"));
    for name in ["SingletonCookie", "SingletonLock", "SingletonSocket"] {
        assert!(!kit.tree("/tmp/profile").iter().any(|(path, _)| path == name.as_bytes()), "{name} sobrou");
    }
    assert!(!kit.tree("/tmp").iter().any(|(path, _)| path.starts_with(b"org.chromium.Chromium.")), "o diretório do soquete sobrou");
}

fn symlink_target(kit: &TestKit, root: &str, name: &str) -> String {
    kit.tree(root)
        .into_iter()
        .find_map(|(path, entry)| match entry {
            TreeEntry::Symlink { target } if path == name.as_bytes() => Some(String::from_utf8(target).unwrap()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("falta o symlink {root}/{name}"))
}

#[test]
fn singletons_have_the_oracle_shape() {
    let kit = kit();
    assert_eq!(kit.run(&["start"], b"").status.shell_status(), 0);
    let cookie = symlink_target(&kit, "/tmp/profile", "SingletonCookie");
    assert!((19..=20).contains(&cookie.len()) && cookie.chars().all(|c| c.is_ascii_digit()), "{cookie}");
    let lock = symlink_target(&kit, "/tmp/profile", "SingletonLock");
    let (host, pid) = lock.rsplit_once('-').expect("hostname-pid");
    assert!(!host.is_empty() && pid.chars().all(|c| c.is_ascii_digit()) && !pid.is_empty(), "{lock}");
    let socket = symlink_target(&kit, "/tmp/profile", "SingletonSocket");
    assert!(socket.starts_with("/tmp/org.chromium.Chromium.") && socket.ends_with("/SingletonSocket") && socket.len() == 49, "{socket}");
    // O diretório do soquete: 0700, com o mesmo cookie.
    let dir = &socket["/tmp/".len()..socket.len() - "/SingletonSocket".len()];
    assert!(kit.tree("/tmp").iter().any(|(path, entry)| path == dir.as_bytes() && matches!(entry, TreeEntry::Dir { mode } if mode & 0o7777 == 0o700)));
    assert_eq!(symlink_target(&kit, &format!("/tmp/{dir}"), "SingletonCookie"), cookie);
}

#[test]
fn crashpad_database_is_created_without_a_user_data_dir() {
    let kit = kit();
    assert_eq!(kit.run(&["bare"], b"").status.shell_status(), 0);
    let tree = actual(&kit, "/root/.config");
    for dir in ["chromium", "chromium/Crash Reports", "chromium/Crash Reports/attachments", "chromium/Crash Reports/completed", "chromium/Crash Reports/new", "chromium/Crash Reports/pending"] {
        assert_eq!(tree.get(dir), Some(&Item { kind: 'd', mode: 0o700, size: None }), "{dir}");
    }
    assert_eq!(tree.get("chromium/Crash Reports/settings.dat"), Some(&Item { kind: 'f', mode: 0o600, size: Some(40) }));
    assert!(kit.tree("/tmp/profile").is_empty(), "sem --user-data-dir não há perfil em /tmp/profile");
}

#[test]
fn volatile_values_follow_the_clock() {
    // O TestKit tem relógio e aleatório determinísticos: duas execuções em instantes diferentes têm de gravar os
    // tempos do instante em que rodaram (o tempo WebKit de `profile.creation_time` acompanha o relógio).
    let first = kit().time(1_791_665_279);
    let second = kit().time(1_800_000_000);
    first.run(&["finish"], b"");
    second.run(&["finish"], b"");
    let read = |kit: &TestKit| {
        kit.tree("/tmp/profile").into_iter().find_map(|(path, entry)| match entry {
            TreeEntry::File { data, .. } if path == b"Default/Preferences" => Some(String::from_utf8(data).unwrap()),
            _ => None,
        })
    };
    let (a, b) = (read(&first).unwrap(), read(&second).unwrap());
    assert!(a.contains("\"creation_time\":\"13436138879"), "{}", &a[..300]);
    assert!(b.contains("\"creation_time\":\"13444473600"), "{}", &b[..300]);
    assert!(a.contains("enterprise_profile_guid"));
}
