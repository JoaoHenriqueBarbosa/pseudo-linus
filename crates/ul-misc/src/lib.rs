//! Ferramentas diversas do pseudo-linus, fiéis às do Debian 13 byte a byte: `bc` e `dc` (GNU bc
//! 1.07.1), `file` (5.46), `column`, `hexdump`/`hd` e `more` (util-linux 2.41), `tree` (2.2.1), `xxd`
//! (vim 9.1), `strings` (binutils 2.44), `which` (debianutils 5.23), `envsubst`, `gettext` e
//! `ngettext` (gettext 0.23), `less` não interativo, `lessecho` (less 668), `tput`, `clear`, `tset`/`reset`, `tabs`,
//! `infocmp` e `toe` (ncurses 6.5.20250216, sobre o banco terminfo do ncurses-base), `getconf`,
//! `getent`, `locale` e `iconv` (glibc 2.41). Do dpkg 1.22 também o `update-alternatives`.
//! Do util-linux 2.41 também: `getopt`, `look`, `col`, `colrm`, `colcrt`, `ul`, `namei`, `rename.ul`,
//! `whereis`, `mcookie`, `hardlink`, `mountpoint`, `setsid`, `fallocate`, `renice`, `setarch` (e links), `chrt`, `choom`, `lsns` e `prlimit`. Da glibc 2.41 também o `zdump` (tzcode). Do debianutils 5.23 também: `tempfile`, `run-parts`, `ischroot`
//! e, como os scripts originais rodando no `sh`, `savelog`, `add-shell` e `remove-shell`.
//!
//! Tudo passa por `sysabi`; nada toca o host.

pub mod bc;
pub mod blkdiscard;
pub mod blkid;
pub mod blockdev;
pub mod bzip2recover;
pub mod choom;
pub mod chrt;
pub mod col;
pub mod colcrt;
pub mod colrm;
pub mod column;
pub mod dc;
pub mod debscripts;
pub mod envsubst;
pub mod dmesg;
pub mod fallocate;
pub mod file;
pub mod fsfreeze;
pub mod fstrim;
pub mod mount;
pub mod pivot_root;
pub mod script;
pub mod scriptlive;
pub mod setterm;
pub mod switch_root;
pub mod umount;
pub mod wall;
pub mod findfs;
pub mod findmnt;
pub mod flock;
pub mod fstab_decode;
pub mod getconf;
pub mod getent;
pub mod getopt_cmd;
pub mod gettext;
pub mod groupmgmt;
pub mod hardlink;
pub mod hexdump;
pub mod iconv;
pub mod ionice;
pub mod ipcmk;
pub mod ipcrm;
pub mod ipcs;
pub mod ischroot;
pub mod isosize;
pub mod lessecho;
pub mod lesskey;
pub mod locale;
pub mod logger;
pub mod look;
pub mod lscpu;
pub mod lsipc;
pub mod lslocks;
pub mod lslogins;
pub mod lsmem;
pub mod lsns;
pub mod mcookie;
pub mod mountpoint;
pub mod namei;
pub mod nologin;
pub mod nsenter;
pub mod pager;
pub mod prlimit;
pub mod pwck;
pub mod rename_ul;
pub mod renice;
pub mod lzmainfo;
pub mod rev;
pub mod run_parts;
pub mod scriptreplay;
pub mod setarch;
pub mod setpriv;
pub mod setsid;
pub mod shadowconv;
pub mod shadowmisc;
pub mod start_stop_daemon;
pub mod strings;
pub mod swaplabel;
pub mod taskset;
pub mod tempfile;
pub mod term;
pub mod tree;
pub mod underline;
pub mod unshare;
pub mod update_alternatives;
pub mod useradd;
pub mod usermgmt;
pub mod util;
pub mod whereis;
pub mod which;
pub mod wipefs;
pub mod xxd;
pub mod zdump;

use sysabi::Program;

/// Tabela de programas do crate.
pub fn programs() -> Vec<Program> {
    vec![
        Program::bin("bc", bc::main),
        Program::bin("bzip2recover", bzip2recover::main),
        Program::bin("chage", shadowmisc::chage_main),
        Program::bin("captoinfo", term::tic::main),
        Program::bin("choom", choom::main),
        Program::bin("chrt", chrt::main),
        Program::bin("clear", term::clear::main),
        Program::bin("col", col::main),
        Program::bin("colcrt", colcrt::main),
        Program::bin("column", column::main),
        Program::bin("colrm", colrm::main),
        Program::bin("dc", dc::main),
        Program::bin("dmesg", dmesg::main),
        Program::bin("mount", mount::main),
        Program::bin("script", script::main),
        Program::bin("scriptlive", scriptlive::main),
        Program::bin("setterm", setterm::main),
        Program::bin("umount", umount::main),
        Program::bin("wall", wall::main),
        Program::bin("envsubst", envsubst::main),
        Program::bin("expiry", shadowconv::expiry_main),
        Program::bin("fallocate", fallocate::main),
        Program::bin("file", file::cli::main),
        Program::bin("findfs", findfs::main),
        Program::bin("findmnt", findmnt::main),
        Program::bin("flock", flock::main),
        Program::bin("fstab-decode", fstab_decode::main),
        Program::bin("getconf", getconf::main),
        Program::bin("getent", getent::main),
        Program::bin("getopt", getopt_cmd::main),
        Program::bin("gettext", gettext::gettext_main),
        Program::bin("gpasswd", shadowmisc::gpasswd_main),
        Program::bin("hardlink", hardlink::main),
        Program::bin("hexdump", hexdump::main),
        Program::bin("hd", hexdump::main),
        Program::bin("iconv", iconv::main),
        Program::bin("infocmp", term::infocmp::main),
        Program::bin("infotocap", term::tic::main),
        Program::bin("ionice", ionice::main),
        Program::bin("ipcmk", ipcmk::main),
        Program::bin("ipcrm", ipcrm::main),
        Program::bin("ipcs", ipcs::main),
        Program::bin("ischroot", ischroot::main),
        Program::bin("isosize", isosize::main),
        Program::bin("less", pager::less_main),
        Program::bin("lessecho", lessecho::main),
        Program::bin("lesskey", lesskey::main),
        Program::bin("locale", locale::main),
        Program::bin("logger", logger::main),
        Program::bin("look", look::main),
        Program::bin("lscpu", lscpu::main),
        Program::bin("lsipc", lsipc::main),
        Program::bin("lslocks", lslocks::main),
        Program::bin("lslogins", lslogins::main),
        Program::bin("lsmem", lsmem::main),
        Program::bin("lsns", lsns::main),
        Program::bin("mcookie", mcookie::main),
        Program::bin("nsenter", nsenter::main),
        Program::bin("prlimit", prlimit::main),
        Program::bin("mountpoint", mountpoint::main),
        Program::bin("namei", namei::main),
        Program::bin("ngettext", gettext::ngettext_main),
        Program::bin("more", pager::more_main),
        Program::bin("rename.ul", rename_ul::main),
        Program::bin("renice", renice::main),
        Program::bin("reset", term::tset::main),
        Program::bin("lzmainfo", lzmainfo::main),
        Program::bin("rev", rev::main),
        Program::bin("run-parts", run_parts::main),
        Program::bin("savelog", debscripts::savelog_main),
        Program::bin("scriptreplay", scriptreplay::main),
        Program::bin("setarch", setarch::main),
        Program::bin("linux32", setarch::main),
        Program::bin("linux64", setarch::main),
        Program::bin("i386", setarch::main),
        Program::bin("x86_64", setarch::main),
        Program::bin("uname26", setarch::main),
        Program::bin("setpriv", setpriv::main),
        Program::bin("setsid", setsid::main),
        Program::bin("strings", strings::main),
        Program::bin("tabs", term::tabs::main),
        Program::bin("taskset", taskset::main),
        Program::bin("tempfile", tempfile::main),
        Program::bin("tic", term::tic::main),
        Program::bin("toe", term::toe::main),
        Program::bin("tput", term::tput::main),
        Program::bin("tree", tree::main),
        Program::bin("tset", term::tset::main),
        Program::bin("ul", underline::main),
        Program::bin("unshare", unshare::main),
        Program::bin("update-alternatives", update_alternatives::main),
        Program::bin("whereis", whereis::main),
        Program::bin("which", which::main),
        Program::bin("xxd", xxd::main),
        Program::bin("zdump", zdump::main),
        Program::sbin("add-shell", debscripts::add_shell_main),
        Program::sbin("blkdiscard", blkdiscard::main),
        Program::sbin("fsfreeze", fsfreeze::main),
        Program::sbin("fstrim", fstrim::main),
        Program::sbin("pivot_root", pivot_root::main),
        Program::sbin("switch_root", switch_root::main),
        Program::sbin("blkid", blkid::main),
        Program::sbin("blockdev", blockdev::main),
        Program::sbin("swaplabel", swaplabel::main),
        Program::sbin("wipefs", wipefs::main),
        Program::sbin("chgpasswd", shadowconv::chgpasswd_main),
        Program::sbin("chpasswd", shadowconv::chpasswd_main),
        Program::sbin("grpconv", shadowconv::grpconv_main),
        Program::sbin("grpunconv", shadowconv::grpunconv_main),
        Program::sbin("pwconv", shadowconv::pwconv_main),
        Program::sbin("pwunconv", shadowconv::pwunconv_main),
        Program::sbin("groupadd", groupmgmt::groupadd_main),
        Program::sbin("groupdel", groupmgmt::groupdel_main),
        Program::sbin("groupmod", groupmgmt::groupmod_main),
        Program::sbin("grpck", groupmgmt::grpck_main),
        Program::sbin("nologin", nologin::main),
        Program::sbin("pwck", pwck::main),
        Program::sbin("remove-shell", debscripts::remove_shell_main),
        Program::sbin("start-stop-daemon", start_stop_daemon::main),
        Program::sbin("useradd", useradd::main),
        Program::sbin("userdel",usermgmt::userdel_main),
        Program::sbin("usermod", usermgmt::usermod_main),
    ]
}
