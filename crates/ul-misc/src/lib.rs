//! Ferramentas diversas do pseudo-linus, fiéis às do Debian 13 byte a byte: `bc` (GNU bc 1.07.1),
//! `file` (5.46), `column`, `hexdump`/`hd` e `more` (util-linux 2.41), `tree` (2.2.1), `xxd`
//! (vim 9.1), `strings` (binutils 2.44), `which` (debianutils 5.23), `envsubst` (gettext 0.23),
//! `less` não interativo, `clear`/`tput` (ncurses 6.5), `iconv`, `getconf` e `locale` (glibc 2.41).
//! Do util-linux 2.41 também: `getopt`, `look`, `col`, `colrm`, `colcrt`, `ul`, `namei`, `rename.ul`,
//! `whereis`, `mcookie`. Do debianutils 5.23 também: `tempfile`, `run-parts`, `savelog`, `ischroot`,
//! `add-shell`, `remove-shell` (os três últimos são os scripts originais rodando no `sh`).
//!
//! Tudo passa por `sysabi`; nada toca o host.

pub mod bc;
pub mod col;
pub mod colcrt;
pub mod column;
pub mod colrm;
pub mod debscripts;
pub mod envsubst;
pub mod file;
pub mod getopt_cmd;
pub mod hexdump;
pub mod ischroot;
pub mod look;
pub mod mcookie;
pub mod namei;
pub mod pager;
pub mod rename_ul;
pub mod rev;
pub mod run_parts;
pub mod strings;
pub mod tempfile;
pub mod tree;
pub mod underline;
pub mod util;
pub mod whereis;
pub mod which;
pub mod xxd;

use sysabi::Program;

/// Tabela de programas do crate.
pub fn programs() -> Vec<Program> {
    vec![
        Program::bin("bc", bc::main),
        Program::bin("col", col::main),
        Program::bin("colcrt", colcrt::main),
        Program::bin("column", column::main),
        Program::bin("colrm", colrm::main),
        Program::bin("envsubst", envsubst::main),
        Program::bin("file", file::cli::main),
        Program::bin("getopt", getopt_cmd::main),
        Program::bin("hexdump", hexdump::main),
        Program::bin("hd", hexdump::main),
        Program::bin("ischroot", ischroot::main),
        Program::bin("less", pager::less_main),
        Program::bin("look", look::main),
        Program::bin("mcookie", mcookie::main),
        Program::bin("namei", namei::main),
        Program::bin("more", pager::more_main),
        Program::bin("rename.ul", rename_ul::main),
        Program::bin("rev", rev::main),
        Program::bin("run-parts", run_parts::main),
        Program::bin("savelog", debscripts::savelog_main),
        Program::bin("strings", strings::main),
        Program::bin("tempfile", tempfile::main),
        Program::bin("tree", tree::main),
        Program::bin("ul", underline::main),
        Program::bin("whereis", whereis::main),
        Program::bin("which", which::main),
        Program::bin("xxd", xxd::main),
        Program::sbin("add-shell", debscripts::add_shell_main),
        Program::sbin("remove-shell", debscripts::remove_shell_main),
    ]
}
