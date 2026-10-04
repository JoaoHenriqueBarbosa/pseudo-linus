//! Grupo "data": números, formatação, codificação e somas (od, numfmt, factor, expr, seq, printf,
//! echo, yes, tee, sum, cksum, md5sum, sha*sum, b2sum, base64, base32, basenc).

use sysabi::Program;

use crate::run::uu_main;

uu_main!(echo_main, "echo", uu_echo);
uu_main!(printf_main, "printf", uu_printf);
uu_main!(seq_main, "seq", uu_seq);
uu_main!(yes_main, "yes", uu_yes);
uu_main!(tee_main, "tee", uu_tee);

pub(crate) fn programs() -> Vec<Program> {
    vec![
        Program::bin("echo", echo_main),
        Program::bin("printf", printf_main),
        Program::bin("seq", seq_main),
        Program::bin("yes", yes_main),
        Program::bin("tee", tee_main),
    ]
}
