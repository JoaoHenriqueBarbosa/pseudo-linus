//! O `localStorage` de um navegador com `--user-data-dir` sobrevive a outra execução do navegador: a primeira grava,
//! fecha (o commit e a saída limpa do perfil), e a segunda, com o mesmo diretório, lê o que a primeira guardou, na
//! ordem de `Object.keys` que o Chromium do oráculo mostra (`ls-write.html` e `ls-read.html`).

mod common;

use std::ffi::OsString;
use std::io::Write;
use std::rc::Rc;
use std::sync::Arc;

use sysabi::testkit::TestKit;
use sysabi::{Ctx, Program};
use ul_chromium::disk_profile::{DiskProfile, Run};
use ul_chromium::session::{Profile, Tab};
use ul_chromium::session_storage::Exit;

const WRITE: &str = "<script>localStorage.setItem('k1', 'um'); localStorage.setItem('k2', 'dois'); localStorage.setItem('k3', 'tres'); console.log('wrote ' + localStorage.length);</script>";
const READ: &str = "<script>console.log(Object.keys(localStorage).join(',') + ' ' + localStorage.getItem('k2') + ' ' + localStorage.length);</script>";
const CLEAR_AND_WRITE_OTHER: &str = "<script>localStorage.clear(); localStorage.setItem('only', 'ç日'); console.log('cleared ' + localStorage.length);</script>";
const READ_ALL: &str = "<script>console.log(JSON.stringify(Object.entries(localStorage)));</script>";

/// Uma execução do navegador sobre `/tmp/profile`: sobe o perfil, carrega a página e sai limpo.
fn browser(ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let page = match argv[1].to_str().unwrap() {
        "write" => WRITE,
        "read" => READ,
        "clear" => CLEAR_AND_WRITE_OTHER,
        _ => READ_ALL,
    };
    let disk = DiskProfile::start(Some(b"/tmp/profile"), Run::DevTools);
    let profile = Profile::on_disk(Some(disk.local_storage().expect("o banco do Local Storage abriu")), disk.indexed_db());
    let tab = Rc::new(Tab::new(Arc::new(profile)));
    let log = common::run_in_tab("http://127.0.0.1:8000/", &[("/", page)], tab);
    disk.finish(Exit::Signal);
    let _ = ctx.stdout().write_all(log.as_bytes());
    0
}

fn kit() -> TestKit {
    TestKit::new().env("HOME", "/root").programs([Program::bin("browser", browser)])
}

fn run(kit: &TestKit, page: &str) -> String {
    let result = kit.run(&["browser", page], b"");
    assert_eq!(result.status.shell_status(), 0, "{}", result.stderr_str());
    result.stdout_str()
}

#[test]
fn what_the_first_run_stored_is_there_for_the_second() {
    let kit = kit();
    assert_eq!(run(&kit, "write"), "wrote 3\n");
    // A ordem é a que o oráculo mostra na segunda execução (`read:3:k2,k3,k1`).
    assert_eq!(run(&kit, "read"), "k2,k3,k1 dois 3\n");
    // E continua lá em quantas execuções vierem.
    assert_eq!(run(&kit, "read"), "k2,k3,k1 dois 3\n");
}

#[test]
fn a_clear_and_a_non_latin1_value_persist_too() {
    let kit = kit();
    assert_eq!(run(&kit, "write"), "wrote 3\n");
    assert_eq!(run(&kit, "clear"), "cleared 1\n");
    assert_eq!(run(&kit, "all"), "[[\"only\",\"ç日\"]]\n");
}

#[test]
fn the_database_files_are_the_leveldbs_and_the_log_holds_the_batches() {
    let kit = kit();
    run(&kit, "write");
    let dir = "/tmp/profile/Default/Local Storage/leveldb";
    let log = kit.read_file(&format!("{dir}/000003.log")).unwrap();
    // O VERSION (7 + 23 bytes) e o lote da página: três chaves, `METAACCESS` e `META`.
    assert_eq!(log.len(), 30 + 7 + 196 + 0, "{} bytes", log.len());
    assert!(String::from_utf8_lossy(&kit.read_file(&format!("{dir}/LOG")).unwrap()).contains("Creating DB /tmp/profile/Default/Local Storage/leveldb since it was missing."));
    run(&kit, "read");
    assert!(kit.read_file(&format!("{dir}/LOG.old")).is_some(), "a segunda execução guarda o LOG da primeira como LOG.old");
}
