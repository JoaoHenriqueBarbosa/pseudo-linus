//! Ferramentas diversas do pseudo-linus, fiéis às do Debian 13 byte a byte: `bc` (GNU bc 1.07.1),
//! `file` (5.46), `column`, `hexdump`/`hd` e `more` (util-linux 2.41), `tree` (2.2.1), `xxd`
//! (vim 9.1), `strings` (binutils 2.44), `which` (debianutils 5.23), `envsubst` (gettext 0.23),
//! `less` não interativo, `clear`/`tput` (ncurses 6.5), `iconv`, `getconf` e `locale` (glibc 2.41).
//! Do util-linux 2.41 também: `getopt`.
//!
//! Tudo passa por `sysabi`; nada toca o host.

pub mod bc;
pub mod column;
pub mod envsubst;
pub mod file;
pub mod getopt_cmd;
pub mod hexdump;
pub mod pager;
pub mod rev;
pub mod strings;
pub mod tree;
pub mod util;
pub mod which;
pub mod xxd;

use sysabi::Program;

/// Tabela de programas do crate.
pub fn programs() -> Vec<Program> {
    vec![
        Program::bin("bc", bc::main),
        Program::bin("column", column::main),
        Program::bin("envsubst", envsubst::main),
        Program::bin("file", file::cli::main),
        Program::bin("getopt", getopt_cmd::main),
        Program::bin("hexdump", hexdump::main),
        Program::bin("hd", hexdump::main),
        Program::bin("less", pager::less_main),
        Program::bin("more", pager::more_main),
        Program::bin("rev", rev::main),
        Program::bin("strings", strings::main),
        Program::bin("tree", tree::main),
        Program::bin("which", which::main),
        Program::bin("xxd", xxd::main),
    ]
}
