//! `tar`, compressores (`gzip`, `bzip2`, `xz`/`lzma`, `lzip`, `zstd`) e `zip`/`unzip` do pseudo-linus.
//!
//! Alvo: as ferramentas do Debian 13 (GNU tar 1.35, gzip 1.13, bzip2 1.0.8, xz-utils 5.8.1, lzip 1.25,
//! zstd 1.5.7, Info-ZIP zip 3.0 e unzip 6.0), byte a byte nas saídas de texto, mensagens e códigos de
//! saída, e interoperáveis nos dois sentidos nos arquivos que geram e leem. Os codecs são crates em
//! Rust puro (F08/H31); CLIs, enquadramento e extração sobre o VFS são nossos. Todo I/O passa pelo
//! `sysabi`.

pub mod codec;
pub mod gailly;
pub mod getopt;
pub mod sysutil;
pub mod tz;

// Os módulos dos CLIs não se chamam `tar` e `zip` pra não esconder as crates de mesmo nome.
pub mod compress;
pub mod tar_cli;
pub mod zip_cli;

use sysabi::Program;

/// Tabela de programas deste crate.
pub fn programs() -> Vec<Program> {
    vec![
        Program::bin("tar", tar_cli::main),
        Program::bin("gzip", compress::gzip_main),
        Program::bin("gunzip", compress::gunzip_main),
        Program::bin("zcat", compress::zcat_main),
        Program::bin("bzip2", compress::bzip2_main),
        Program::bin("bunzip2", compress::bunzip2_main),
        Program::bin("bzcat", compress::bzcat_main),
        Program::bin("xz", compress::xz_main),
        Program::bin("unxz", compress::unxz_main),
        Program::bin("xzcat", compress::xzcat_main),
        Program::bin("lzma", compress::lzma_main),
        Program::bin("unlzma", compress::unlzma_main),
        Program::bin("lzcat", compress::lzcat_main),
        Program::bin("zstd", compress::zstd_main),
        Program::bin("unzstd", compress::unzstd_main),
        Program::bin("zstdcat", compress::zstdcat_main),
        Program::bin("zstdmt", compress::zstd_main),
        Program::bin("lzip", compress::lzip_main),
        Program::bin("zip", zip_cli::zip_main),
        Program::bin("unzip", zip_cli::unzip_main),
        Program::bin("zipinfo", zip_cli::unzip_main),
    ]
}
