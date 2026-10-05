//! Grupo "system": processos, ambiente, identidade e sistema (env, printenv, timeout, nice, nohup,
//! stdbuf, chroot, sleep, true, false, test e `[`, pwd, id, whoami, groups, logname, users, who,
//! uname, arch, hostname, hostid, nproc, tty, date).

use sysabi::Program;

use crate::run::uu_main;

uu_main!(env_main, "env", uu_env);
uu_main!(printenv_main, "printenv", uu_printenv);
uu_main!(timeout_main, "timeout", uu_timeout);
uu_main!(nice_main, "nice", uu_nice);
uu_main!(nohup_main, "nohup", uu_nohup);
uu_main!(stdbuf_main, "stdbuf", uu_stdbuf);
uu_main!(chroot_main, "chroot", uu_chroot);
uu_main!(sleep_main, "sleep", uu_sleep);
uu_main!(true_main, "true", uu_true);
uu_main!(false_main, "false", uu_false);
uu_main!(test_main, "test", uu_test);
uu_main!(bracket_main, "[", uu_test);
uu_main!(pwd_main, "pwd", uu_pwd);
uu_main!(id_main, "id", uu_id);
uu_main!(whoami_main, "whoami", uu_whoami);
uu_main!(groups_main, "groups", uu_groups);
uu_main!(logname_main, "logname", uu_logname);
uu_main!(users_main, "users", uu_users);
uu_main!(who_main, "who", uu_who);
uu_main!(uname_main, "uname", uu_uname);
uu_main!(arch_main, "arch", uu_arch);
uu_main!(hostid_main, "hostid", uu_hostid);
uu_main!(nproc_main, "nproc", uu_nproc);
uu_main!(tty_main, "tty", uu_tty);
uu_main!(date_main, "date", uu_date);

pub(crate) fn programs() -> Vec<Program> {
    vec![
        Program::bin("env", env_main),
        Program::bin("printenv", printenv_main),
        Program::bin("timeout", timeout_main),
        Program::bin("nice", nice_main),
        Program::bin("nohup", nohup_main),
        Program::bin("stdbuf", stdbuf_main),
        Program::sbin("chroot", chroot_main),
        Program::bin("sleep", sleep_main),
        Program::bin("true", true_main),
        Program::bin("false", false_main),
        Program::bin("test", test_main),
        Program::bin("[", bracket_main),
        Program::bin("pwd", pwd_main),
        Program::bin("id", id_main),
        Program::bin("whoami", whoami_main),
        Program::bin("groups", groups_main),
        Program::bin("logname", logname_main),
        Program::bin("users", users_main),
        Program::bin("who", who_main),
        Program::bin("uname", uname_main),
        Program::bin("arch", arch_main),
        // O `hostname` do Debian vem do pacote hostname 3.25, não do coreutils (nem do uutils).
        Program::bin("hostname", crate::hostname::main),
        Program::bin("hostid", hostid_main),
        Program::bin("nproc", nproc_main),
        Program::bin("tty", tty_main),
        Program::bin("date", date_main),
    ]
}
