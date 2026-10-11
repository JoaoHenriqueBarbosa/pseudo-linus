//! Valida pager + btree + os_unix de ponta a ponta contra o oráculo: dentro do kernel simulado
//! (testkit), o programa `zbt` cria um banco de dados SQLite só com as camadas de baixo
//! (b-tree, pager, journal, VFS), sem SQL: grava a linha de `sqlite_master` à mão e milhares de
//! linhas numa tabela, com payloads que forçam divisão de páginas, vários níveis e overflow. O
//! arquivo resultante é então aberto pelo sqlite3 3.46.1 da imagem do oráculo, que confere
//! `PRAGMA integrity_check` e o conteúdo.
//!
//! O teste usa docker (imagem `pseudo-linus-oracle:894fe4065523`); sem docker ele se declara
//! ignorado em vez de passar em falso.

use crate::btree::btree_open;
use crate::btree_cursor::*;
use crate::btree_types::*;
use crate::btree_write::*;
use crate::consts::*;
use crate::os::vfs_find;
use crate::util::put_varint;
use std::ffi::OsString;
use sysabi::testkit::TestKit;
use sysabi::{Ctx, Program};

const N_ROWS: i64 = 6000;

/// Texto do payload da linha `i`: tamanho variável, com um caso de overflow a cada 500 linhas.
fn row_text(i: i64) -> Vec<u8> {
    let len = if i % 500 == 0 { 9000 } else { 10 + (i as usize * 7) % 120 };
    (0..len).map(|k| b'a' + ((i as usize + k) % 26) as u8).collect()
}

/// Registro de uma coluna de texto.
fn text_record(text: &[u8]) -> Vec<u8> {
    let mut serial = [0u8; 9];
    let n = put_varint(&mut serial, 13 + 2 * text.len() as u64) as usize;
    let mut rec = Vec::new();
    rec.push((1 + n) as u8);
    rec.extend_from_slice(&serial[..n]);
    rec.extend_from_slice(text);
    rec
}

/// Registro de `sqlite_master`: (type, name, tbl_name, rootpage, sql).
fn master_record(root: u8) -> Vec<u8> {
    let sql = b"CREATE TABLE tt(a)";
    let mut rec = vec![
        6u8,
        13 + 2 * 5,
        13 + 2 * 2,
        13 + 2 * 2,
        1, // inteiro de 1 byte
        13 + 2 * sql.len() as u8,
    ];
    rec.extend_from_slice(b"table");
    rec.extend_from_slice(b"tt");
    rec.extend_from_slice(b"tt");
    rec.push(root);
    rec.extend_from_slice(sql);
    rec
}

fn insert_row(p: &mut Btree, root: u32, rowid: i64, rec: &[u8]) -> i32 {
    let id = match btree_cursor(p, root, 1, None) {
        Ok(id) => id,
        Err(rc) => return rc,
    };
    let mut cur = p.bt.cursors.take(id).expect("cursor");
    let payload = BtreePayload {
        p_key: None,
        n_key: rowid,
        p_data: Some(rec),
        a_mem: &[],
        n_mem: 0,
        n_data: rec.len() as i32,
        n_zero: 0,
    };
    let rc = btree_insert(&mut cur, &mut p.bt, &payload, 0, 0, false);
    p.bt.cursors.put(id, cur);
    if rc == SQLITE_OK {
        btree_close_cursor(p, id);
    }
    rc
}

fn zbt_main(_ctx: &mut Ctx, _args: &[OsString]) -> i32 {
    crate::os_unix::os_init();
    let vfs = vfs_find(None).expect("vfs padrão");
    let mut p = match btree_open(vfs, Some(b"/t.db"), 0, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_MAIN_DB) {
        Ok(p) => p,
        Err(rc) => return 100 + rc,
    };
    let mut db = BtDb::default();
    let mut rc = btree_begin_trans(&mut p, 2, None, &mut db);
    if rc != SQLITE_OK {
        return 200 + rc;
    }
    let mut root = 0u32;
    rc = btree_create_table(&mut p, BTREE_INTKEY as i32, &mut root);
    if rc != SQLITE_OK || root != 2 {
        return 300 + rc;
    }
    for (idx, v) in [(BTREE_SCHEMA_VERSION, 1u32), (BTREE_FILE_FORMAT, 4), (BTREE_TEXT_ENCODING, 1)] {
        rc = btree_update_meta(&mut p, idx as i32, v);
        if rc != SQLITE_OK {
            return 400 + rc;
        }
    }
    rc = insert_row(&mut p, 1, 1, &master_record(root as u8));
    if rc != SQLITE_OK {
        return 500 + rc;
    }
    for i in 1..=N_ROWS {
        rc = insert_row(&mut p, root, i, &text_record(&row_text(i)));
        if rc != SQLITE_OK {
            return 600 + rc;
        }
    }
    rc = btree_commit_phase_one(&mut p, None, &mut db);
    if rc != SQLITE_OK {
        return 700 + rc;
    }
    rc = btree_commit_phase_two(&mut p, false, &db);
    if rc != SQLITE_OK {
        return 800 + rc;
    }
    0
}

fn have_docker() -> bool {
    std::process::Command::new("docker").args(["image", "inspect", "pseudo-linus-oracle:894fe4065523"]).output().map(|o| o.status.success()).unwrap_or(false)
}

#[test]
fn btree_database_is_accepted_by_the_oracle() {
    if !have_docker() {
        eprintln!("docker ou imagem do oráculo ausente: teste ignorado");
        return;
    }
    let kit = TestKit::new().programs([Program::bin("zbt", zbt_main)]);
    let r = kit.run(&["zbt"], b"");
    assert_eq!(r.status, sysabi::WaitStatus::Exited(0), "zbt falhou: {}", r.stderr_str());
    let bytes = kit.read_file("/t.db").expect("arquivo gerado");

    let dir = std::env::temp_dir().join(format!("zsqlite-bt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("t.db"), &bytes).unwrap();
    let mut want_sum: i64 = 0;
    for i in 1..=N_ROWS {
        want_sum += row_text(i).len() as i64;
    }
    let sql = format!(
        "pragma integrity_check; select count(*), sum(length(a)) from tt; select length(a) from tt where rowid=3000; select a from tt where rowid=1234;"
    );
    let out = std::process::Command::new("docker")
        .args(["run", "--rm", "-v"])
        .arg(format!("{}:/w", dir.display()))
        .args(["pseudo-linus-oracle:894fe4065523", "sqlite3", "/w/t.db", &sql])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    let expected = format!("ok\n{}|{}\n{}\n{}\n", N_ROWS, want_sum, row_text(3000).len(), String::from_utf8(row_text(1234)).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(text, expected, "stderr do oráculo: {err}");
}
