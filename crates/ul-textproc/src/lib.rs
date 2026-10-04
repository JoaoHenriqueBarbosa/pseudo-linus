//! ul-textproc: `grep`, `egrep`, `fgrep`, `rgrep` (GNU grep 3.11) e `sed` (GNU sed 4.9) do
//! pseudo-linus, sobre o motor `regex-posix`. Toda E/S passa pelo `sysabi`.

pub mod getopt;
pub mod grep;
pub mod io;
pub mod sed;

use sysabi::Program;

/// Os programas deste crate.
pub fn programs() -> Vec<Program> {
    vec![
        Program::bin("grep", grep::main),
        Program::bin("egrep", grep::egrep_main),
        Program::bin("fgrep", grep::fgrep_main),
        Program::bin("rgrep", grep::rgrep_main),
        Program::bin("sed", sed::main),
    ]
}
