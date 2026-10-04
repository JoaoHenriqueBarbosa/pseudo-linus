//! Grupo "files": operações no sistema de arquivos (cp, mv, rm, mkdir, rmdir, ln, link, unlink, touch,
//! install, chmod, chown, chgrp, mktemp, truncate, sync, mkfifo, mknod, dd, du, df, stat, readlink,
//! realpath, basename, dirname, pathchk, dircolors, dir, vdir).
//!
//! cp, mv e dd ainda não estão aqui: o porte deles pro sysio está em andamento.

use sysabi::Program;

use crate::run::uu_main;

uu_main!(mkdir_main, "mkdir", uu_mkdir);
uu_main!(rmdir_main, "rmdir", uu_rmdir);
uu_main!(rm_main, "rm", uu_rm);
uu_main!(ln_main, "ln", uu_ln);
uu_main!(link_main, "link", uu_link);
uu_main!(unlink_main, "unlink", uu_unlink);
uu_main!(touch_main, "touch", uu_touch);
uu_main!(install_main, "install", uu_install);
uu_main!(chmod_main, "chmod", uu_chmod);
uu_main!(chown_main, "chown", uu_chown);
uu_main!(chgrp_main, "chgrp", uu_chgrp);
uu_main!(mktemp_main, "mktemp", uu_mktemp);
uu_main!(truncate_main, "truncate", uu_truncate);
uu_main!(sync_main, "sync", uu_sync);
uu_main!(mkfifo_main, "mkfifo", uu_mkfifo);
uu_main!(mknod_main, "mknod", uu_mknod);
uu_main!(du_main, "du", uu_du);
uu_main!(df_main, "df", uu_df);
uu_main!(stat_main, "stat", uu_stat);
uu_main!(readlink_main, "readlink", uu_readlink);
uu_main!(realpath_main, "realpath", uu_realpath);
uu_main!(basename_main, "basename", uu_basename);
uu_main!(dirname_main, "dirname", uu_dirname);
uu_main!(pathchk_main, "pathchk", uu_pathchk);
uu_main!(dircolors_main, "dircolors", uu_dircolors);
uu_main!(dir_main, "dir", uu_dir);
uu_main!(vdir_main, "vdir", uu_vdir);

pub(crate) fn programs() -> Vec<Program> {
    vec![
        Program::bin("mkdir", mkdir_main),
        Program::bin("rmdir", rmdir_main),
        Program::bin("rm", rm_main),
        Program::bin("ln", ln_main),
        Program::bin("link", link_main),
        Program::bin("unlink", unlink_main),
        Program::bin("touch", touch_main),
        Program::bin("install", install_main),
        Program::bin("chmod", chmod_main),
        Program::bin("chown", chown_main),
        Program::bin("chgrp", chgrp_main),
        Program::bin("mktemp", mktemp_main),
        Program::bin("truncate", truncate_main),
        Program::bin("sync", sync_main),
        Program::bin("mkfifo", mkfifo_main),
        Program::bin("mknod", mknod_main),
        Program::bin("du", du_main),
        Program::bin("df", df_main),
        Program::bin("stat", stat_main),
        Program::bin("readlink", readlink_main),
        Program::bin("realpath", realpath_main),
        Program::bin("basename", basename_main),
        Program::bin("dirname", dirname_main),
        Program::bin("pathchk", pathchk_main),
        Program::bin("dircolors", dircolors_main),
        Program::bin("dir", dir_main),
        Program::bin("vdir", vdir_main),
    ]
}
