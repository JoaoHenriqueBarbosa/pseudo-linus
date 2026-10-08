//! `loadext.c`: `load_extension()`, `sqlite3_enable_load_extension()` e as extensões automáticas
//! (`sqlite3_auto_extension()` e companhia).
//!
//! Opções do Debian 13: `ENABLE_LOAD_EXTENSION` está LIGADA, então o carregador existe e falha
//! exatamente como o do sqlite3 do oráculo: o `dlopen` é do VFS (`Vfs::dl_open`), e a mensagem de
//! erro é o `dlerror()` que `sqlite3OsDlError` grava por cima do texto inicial.
//!
//! Desvios do C, decorrentes do modelo v2 (CONVENTIONS.md):
//!
//! - `sqlite3_api_routines` (a tabela `sqlite3Apis`, que o C entrega à extensão como último
//!   argumento do ponto de entrada) não existe: a extensão em Rust é um `fn(&mut Connection,
//!   &mut Option<Vec<u8>>) -> i32` que chama as funções do crate direto ([`LoadextEntry`]);
//! - um `DlSymbol` do VFS é opaco. Para um símbolo virar código chamável o dono do VFS o
//!   registra com [`register_dl_entry`]; o VFS do Linux simulado não carrega ELF e nunca devolve
//!   símbolo, então nada é registrado e todo carregamento termina em "unable to open shared
//!   library";
//! - a lista de extensões automáticas é um `Mutex` estático (o `SQLITE_MUTEX_STATIC_MAIN` do C);
//! - [`reset_auto_extension`] não chama `sqlite3_initialize()`: o `sqlite3_shutdown()` a chama com
//!   a configuração global travada e a lista não depende da inicialização.

use std::sync::{Mutex, PoisonError};

use crate::connection::Connection;
use crate::consts::{
    SQLITE_ERROR, SQLITE_LOAD_EXTENSION, SQLITE_LOAD_EXT_FUNC, SQLITE_MAX_PATHLEN,
    SQLITE_NOMEM_BKPT, SQLITE_OK, SQLITE_OK_LOAD_PERMANENTLY,
};
use crate::ctype::{is_alpha, to_lower};
use crate::main::{api_exit, error_with_msg, initialize};
use crate::os::{DlHandle, DlSymbol, VfsRef};
use crate::printf::{mprintf, PrintfArg};
use crate::util::strnicmp;

/// `sqlite3_loadext_entry`: o ponto de entrada de uma extensão, `int(*)(sqlite3*, char**,
/// const sqlite3_api_routines*)` no C. Devolve `SQLITE_OK` (ou `SQLITE_OK_LOAD_PERMANENTLY`) ou
/// um erro, com a mensagem em `Option<Vec<u8>>`.
pub type LoadextEntry = fn(&mut Connection, &mut Option<Vec<u8>>) -> i32;

/// `sqlite3Autoext`: as extensões que toda conexão nova carrega.
static AUTO_EXT: Mutex<Vec<LoadextEntry>> = Mutex::new(Vec::new());

/// Os símbolos de VFS que viram código chamável (ver [`register_dl_entry`]).
static DL_ENTRIES: Mutex<Vec<(DlSymbol, LoadextEntry)>> = Mutex::new(Vec::new());

/// O endereço de um ponto de entrada, para comparar `xInit` como o C compara ponteiros.
fn entry_addr(x_init: LoadextEntry) -> usize {
    x_init as usize
}

/// Liga o símbolo opaco `sym`, que um `Vfs::dl_sym` devolve, ao ponto de entrada `x_init`.
pub fn register_dl_entry(sym: DlSymbol, x_init: LoadextEntry) {
    let mut entries = DL_ENTRIES.lock().unwrap_or_else(PoisonError::into_inner);
    entries.retain(|(s, _)| *s != sym);
    entries.push((sym, x_init));
}

/// O ponto de entrada que `sym` representa, se o dono do VFS o registrou.
fn entry_of_symbol(sym: DlSymbol) -> Option<LoadextEntry> {
    let entries = DL_ENTRIES.lock().unwrap_or_else(PoisonError::into_inner);
    entries.iter().find(|(s, _)| *s == sym).map(|(_, f)| *f)
}

/// `sqlite3OsDlError` sobre o texto `msg`: o `dlerror()` do VFS, quando existe, SUBSTITUI o texto
/// (o `unixDlError` faz `sqlite3_snprintf(nBuf, zBufOut, "%s", zErr)` no mesmo buffer).
fn dl_error_over(vfs: Option<&VfsRef>, n_byte: usize, msg: &mut Vec<u8>) {
    if let Some(vfs) = vfs {
        let mut out = Vec::new();
        vfs.dl_error(n_byte as i32, &mut out);
        if !out.is_empty() {
            *msg = out;
        }
    }
}

/// O texto de `extension_not_found`: "unable to open shared library [ARQUIVO]", trocado pelo
/// `dlerror()` quando há um.
fn extension_not_found(vfs: Option<&VfsRef>, z_file: &[u8]) -> Vec<u8> {
    let n_msg = z_file.len() + 300;
    let shown = &z_file[..z_file.len().min(SQLITE_MAX_PATHLEN)];
    let mut msg = mprintf(
        b"unable to open shared library [%s]",
        &[PrintfArg::Text(Some(shown.to_vec()))],
    )
    .unwrap_or_default();
    dl_error_over(vfs, n_msg - 1, &mut msg);
    msg
}

/// O ponto de entrada derivado do nome do arquivo quando a extensão não tem `sqlite3_extension_init`
/// e o chamador não deu um: "sqlite3_X_init", com X as letras ASCII minúsculas do nome do arquivo
/// depois da última "/" até o primeiro ".", sem as três primeiras se forem "lib". Exemplos:
///
/// ```text
///   /usr/local/lib/libExample5.4.3.so  ==>  sqlite3_example_init
///   mathfuncs.so                       ==>  sqlite3_mathfuncs_init
/// ```
fn default_entry_name(z_file: &[u8]) -> Vec<u8> {
    let nc_file = z_file.len();
    let mut i_file = nc_file;
    while i_file > 0 && z_file[i_file - 1] != b'/' {
        i_file -= 1;
    }
    if strnicmp(Some(&z_file[i_file..]), Some(&b"lib"[..]), 3) == 0 {
        i_file += 3;
    }
    let mut z_alt_entry = b"sqlite3_".to_vec();
    while i_file < nc_file && z_file[i_file] != b'.' {
        if is_alpha(z_file[i_file]) {
            z_alt_entry.push(to_lower(z_file[i_file]));
        }
        i_file += 1;
    }
    z_alt_entry.extend_from_slice(b"_init");
    z_alt_entry
}

/// `sqlite3OsDlClose`.
fn dl_close(vfs: Option<&VfsRef>, handle: DlHandle) {
    if let Some(vfs) = vfs {
        vfs.dl_close(handle);
    }
}

/// `sqlite3LoadExtension`: carrega a biblioteca `z_file`, cujo ponto de entrada é `z_proc`
/// (`sqlite3_extension_init` se `None`). Devolve `SQLITE_OK` ou `SQLITE_ERROR`, com a mensagem em
/// `pz_err_msg`.
fn load_extension_inner(
    db: &mut Connection,
    z_file: &[u8],
    z_proc: Option<&[u8]>,
    pz_err_msg: &mut Option<Vec<u8>>,
) -> i32 {
    // Bibliotecas compartilhadas a tentar se `z_file` não carrega como veio.
    const AZ_ENDINGS: [&[u8]; 1] = [b"so"];
    let vfs = db.p_vfs.clone();
    let n_msg = z_file.len();

    *pz_err_msg = None;

    // Ticket #1863. Para não criar problemas de segurança em aplicativos antigos que religam com
    // um SQLite novo, o `load_extension` vem desligado: é preciso chamar
    // `sqlite3_enable_load_extension(db)` (ou `sqlite3_db_config` com
    // `SQLITE_DBCONFIG_ENABLE_LOAD_EXTENSION`) para ligá-lo.
    if db.flags & SQLITE_LOAD_EXTENSION == 0 {
        *pz_err_msg = Some(b"not authorized".to_vec());
        return SQLITE_ERROR;
    }

    let mut z_entry: Vec<u8> = z_proc.map_or_else(|| b"sqlite3_extension_init".to_vec(), <[u8]>::to_vec);

    // tag-20210611-1. Alguns `dlopen()` dão segfault com nome de arquivo grande demais. A maioria
    // dos sistemas de arquivos limita o caminho a 4K, então o nome da extensão fica em cerca do
    // dobro. Reserva-se 6 bytes para o sufixo (2023-03-25).
    //
    // Também não se permite ligar com uma cópia do aplicativo em execução por um nome vazio.
    if n_msg > SQLITE_MAX_PATHLEN || n_msg == 0 {
        *pz_err_msg = Some(extension_not_found(vfs.as_ref(), z_file));
        return SQLITE_ERROR;
    }

    let mut handle: Option<DlHandle> = vfs.as_ref().and_then(|v| v.dl_open(z_file));
    for z_ending in AZ_ENDINGS {
        if handle.is_some() {
            break;
        }
        let Some(z_alt_file) = mprintf(
            b"%s.%s",
            &[PrintfArg::Text(Some(z_file.to_vec())), PrintfArg::Text(Some(z_ending.to_vec()))],
        ) else {
            return SQLITE_NOMEM_BKPT;
        };
        if n_msg + z_ending.len() + 1 <= SQLITE_MAX_PATHLEN {
            handle = vfs.as_ref().and_then(|v| v.dl_open(&z_alt_file));
        }
    }
    let Some(handle) = handle else {
        *pz_err_msg = Some(extension_not_found(vfs.as_ref(), z_file));
        return SQLITE_ERROR;
    };
    let mut x_init: Option<LoadextEntry> = vfs
        .as_ref()
        .and_then(|v| v.dl_sym(handle, &z_entry))
        .and_then(entry_of_symbol);

    // Sem ponto de entrada dado e sem o `sqlite3_extension_init` legado, monta-se o nome
    // "sqlite3_X_init" a partir do nome do arquivo.
    if x_init.is_none() && z_proc.is_none() {
        z_entry = default_entry_name(z_file);
        x_init = vfs
            .as_ref()
            .and_then(|v| v.dl_sym(handle, &z_entry))
            .and_then(entry_of_symbol);
    }
    let Some(x_init) = x_init else {
        let n_msg = n_msg + z_entry.len() + 300;
        let mut msg = mprintf(
            b"no entry point [%s] in shared library [%s]",
            &[PrintfArg::Text(Some(z_entry)), PrintfArg::Text(Some(z_file.to_vec()))],
        )
        .unwrap_or_default();
        dl_error_over(vfs.as_ref(), n_msg - 1, &mut msg);
        *pz_err_msg = Some(msg);
        dl_close(vfs.as_ref(), handle);
        return SQLITE_ERROR;
    };
    let mut z_errmsg: Option<Vec<u8>> = None;
    let rc = x_init(db, &mut z_errmsg);
    if rc != 0 {
        if rc == SQLITE_OK_LOAD_PERMANENTLY {
            return SQLITE_OK;
        }
        *pz_err_msg = mprintf(
            b"error during initialization: %s",
            &[PrintfArg::Text(z_errmsg)],
        );
        dl_close(vfs.as_ref(), handle);
        return SQLITE_ERROR;
    }

    // Acrescenta o handle da nova biblioteca a `db.a_extension`.
    db.a_extension.push(handle);
    SQLITE_OK
}

/// `sqlite3_load_extension`: carrega a biblioteca `z_file` na conexão `db`. `z_proc` é o ponto de
/// entrada (`sqlite3_extension_init` se `None`). Em erro, `pz_err_msg` recebe a mensagem.
pub fn load_extension(
    db: &mut Connection,
    z_file: &[u8],
    z_proc: Option<&[u8]>,
    pz_err_msg: &mut Option<Vec<u8>>,
) -> i32 {
    let rc = load_extension_inner(db, z_file, z_proc, pz_err_msg);
    api_exit(db, rc)
}

/// `sqlite3CloseExtensions`: chamada ao fechar a conexão, descarrega as extensões carregadas.
pub fn close_extensions(db: &mut Connection) {
    let vfs = db.p_vfs.clone();
    for handle in std::mem::take(&mut db.a_extension) {
        dl_close(vfs.as_ref(), handle);
    }
}

/// `sqlite3_enable_load_extension`: liga ou desliga o carregamento de extensões, desligado por
/// padrão para não abrir buracos de segurança em aplicativos antigos.
pub fn enable_load_extension(db: &mut Connection, onoff: bool) -> i32 {
    if onoff {
        db.flags |= SQLITE_LOAD_EXTENSION | SQLITE_LOAD_EXT_FUNC;
    } else {
        db.flags &= !(SQLITE_LOAD_EXTENSION | SQLITE_LOAD_EXT_FUNC);
    }
    SQLITE_OK
}

/// `sqlite3_auto_extension`: registra uma extensão ligada estaticamente que toda conexão nova
/// carrega automaticamente.
pub fn auto_extension(x_init: LoadextEntry) -> i32 {
    let rc = initialize();
    if rc != SQLITE_OK {
        return rc;
    }
    let mut list = AUTO_EXT.lock().unwrap_or_else(PoisonError::into_inner);
    if !list.iter().any(|f| entry_addr(*f) == entry_addr(x_init)) {
        list.push(x_init);
    }
    SQLITE_OK
}

/// `sqlite3_cancel_auto_extension`: tira `x_init` do conjunto de rotinas chamadas a cada conexão
/// nova, se ela estiver lá. Devolve 1 se a tirou e 0 se não estava na lista.
pub fn cancel_auto_extension(x_init: LoadextEntry) -> i32 {
    let mut list = AUTO_EXT.lock().unwrap_or_else(PoisonError::into_inner);
    for i in (0..list.len()).rev() {
        if entry_addr(list[i]) == entry_addr(x_init) {
            // O C troca o elemento pelo último, o que muda a ordem da lista.
            list.swap_remove(i);
            return 1;
        }
    }
    0
}

/// `sqlite3_reset_auto_extension`: esvazia a lista de extensões automáticas.
pub fn reset_auto_extension() {
    AUTO_EXT.lock().unwrap_or_else(PoisonError::into_inner).clear();
}

/// `sqlite3AutoLoadExtensions`: carrega todas as extensões automáticas. Se algo falha, grava o
/// erro na conexão e para.
pub fn auto_load_extensions(db: &mut Connection) {
    // Caso comum: sai sem tomar o mutex mais do que o necessário.
    if AUTO_EXT.lock().unwrap_or_else(PoisonError::into_inner).is_empty() {
        return;
    }
    let mut i = 0usize;
    loop {
        let x_init = AUTO_EXT.lock().unwrap_or_else(PoisonError::into_inner).get(i).copied();
        let Some(x_init) = x_init else {
            break;
        };
        let mut z_errmsg: Option<Vec<u8>> = None;
        let rc = x_init(db, &mut z_errmsg);
        if rc != 0 {
            error_with_msg(
                db,
                rc,
                b"automatic extension loading failed: %s",
                &[PrintfArg::Text(z_errmsg)],
            );
            break;
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_entry_name_follows_the_file_name() {
        assert_eq!(default_entry_name(b"/usr/local/lib/libExample5.4.3.so"), b"sqlite3_example_init");
        assert_eq!(default_entry_name(b"mathfuncs.so"), b"sqlite3_mathfuncs_init");
        assert_eq!(default_entry_name(b"LIBfoo-bar"), b"sqlite3_foobar_init");
    }

    #[test]
    fn auto_extension_list_keeps_one_copy_and_cancels() {
        fn one(_db: &mut Connection, _err: &mut Option<Vec<u8>>) -> i32 {
            0
        }
        fn two(_db: &mut Connection, _err: &mut Option<Vec<u8>>) -> i32 {
            1
        }
        reset_auto_extension();
        assert_eq!(auto_extension(one), SQLITE_OK);
        assert_eq!(auto_extension(one), SQLITE_OK);
        assert_eq!(auto_extension(two), SQLITE_OK);
        assert_eq!(cancel_auto_extension(one), 1);
        assert_eq!(cancel_auto_extension(one), 0);
        assert_eq!(cancel_auto_extension(two), 1);
    }
}
