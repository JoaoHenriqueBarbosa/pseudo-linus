//! Grupo "system": processos, ambiente, identidade e sistema (env, printenv, timeout, nice, nohup,
//! stdbuf, chroot, sleep, true, false, test e `[`, pwd, id, whoami, groups, logname, users, who,
//! uname, arch, hostname, hostid, nproc, tty, date).

use sysabi::Program;

use crate::run::uu_main;

uu_main!(sleep_main, "sleep", uu_sleep);

pub(crate) fn programs() -> Vec<Program> {
    vec![Program::bin("sleep", sleep_main)]
}
