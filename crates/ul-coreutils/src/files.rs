//! Grupo "files": operações no sistema de arquivos (cp, mv, rm, mkdir, rmdir, ln, link, unlink, touch,
//! install, chmod, chown, chgrp, mktemp, truncate, sync, mkfifo, mknod, dd, du, df, stat, readlink,
//! realpath, basename, dirname, pathchk, dircolors, dir, vdir).

use sysabi::Program;

use crate::run::uu_main;

uu_main!(mkdir_main, "mkdir", uu_mkdir);

pub(crate) fn programs() -> Vec<Program> {
    vec![Program::bin("mkdir", mkdir_main)]
}
