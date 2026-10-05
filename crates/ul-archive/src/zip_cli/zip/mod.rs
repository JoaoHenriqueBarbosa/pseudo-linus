//! `zip` do Info-ZIP 3.0 (Debian 3.0-15+deb13u1), portado do C com as opções de compilação do pacote
//! (`zip -v`): Unix, UTF-8, Zip64, bzip2, links simbólicos, horários UT, UID/GID e a cifra tradicional.
//! O deflate é o `deflate.c`/`trees.c` do próprio zip, para que o arquivo gerado seja idêntico byte a
//! byte ao do original.
//!
//! Um módulo por responsabilidade: `opts` (a tabela de opções e `get_option`), `run` (a leitura da
//! linha de comando), `exec` (o resto do `main`), `scan` e `names` (escolha dos arquivos), `zipread` e
//! `zipwrite` (zipfile.c), `zipup` (zipup.c), `deflate` e `trees` (a compressão), `crypt` (a cifra),
//! `matching` (curingas), `msg` (mensagens), `out` (arquivo de saída) e `helpers`.

mod consts;
mod crypt;
mod deflate;
mod exec;
mod extra;
mod helpers;
mod in_scan;
mod matching;
mod msg;
mod names;
mod opts;
mod out;
mod run;
mod scan;
mod state;
mod text;
mod times;
mod trees;
mod zipread;
mod zipup;
mod zipwrite;

use state::{Exit, Zip};

/// `zip`: o código de saída do programa.
pub fn main(argv: &[Vec<u8>]) -> i32 {
    let mut z = Zip::new(argv.first().cloned().unwrap_or_default());
    match z.run_main(argv.to_vec()) {
        Ok(code) => code,
        Err(Exit(code)) => code,
    }
}
