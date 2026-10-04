//! Grupo "data": números, formatação, codificação e somas (od, numfmt, factor, expr, seq, printf,
//! echo, yes, tee, sum, cksum, md5sum, sha*sum, b2sum, base64, base32, basenc).

use sysabi::Program;

use crate::run::uu_main;

uu_main!(echo_main, "echo", uu_echo);
uu_main!(printf_main, "printf", uu_printf);
uu_main!(seq_main, "seq", uu_seq);
uu_main!(yes_main, "yes", uu_yes);
uu_main!(tee_main, "tee", uu_tee);
uu_main!(od_main, "od", uu_od);
uu_main!(numfmt_main, "numfmt", uu_numfmt);
uu_main!(factor_main, "factor", uu_factor);
uu_main!(expr_main, "expr", uu_expr);
uu_main!(sum_main, "sum", uu_sum);
uu_main!(cksum_main, "cksum", uu_cksum);
uu_main!(md5sum_main, "md5sum", uu_md5sum);
uu_main!(sha1sum_main, "sha1sum", uu_sha1sum);
uu_main!(sha224sum_main, "sha224sum", uu_sha224sum);
uu_main!(sha256sum_main, "sha256sum", uu_sha256sum);
uu_main!(sha384sum_main, "sha384sum", uu_sha384sum);
uu_main!(sha512sum_main, "sha512sum", uu_sha512sum);
uu_main!(b2sum_main, "b2sum", uu_b2sum);
uu_main!(base64_main, "base64", uu_base64);
uu_main!(base32_main, "base32", uu_base32);
uu_main!(basenc_main, "basenc", uu_basenc);

pub(crate) fn programs() -> Vec<Program> {
    vec![
        Program::bin("echo", echo_main),
        Program::bin("printf", printf_main),
        Program::bin("seq", seq_main),
        Program::bin("yes", yes_main),
        Program::bin("tee", tee_main),
        Program::bin("od", od_main),
        Program::bin("numfmt", numfmt_main),
        Program::bin("factor", factor_main),
        Program::bin("expr", expr_main),
        Program::bin("sum", sum_main),
        Program::bin("cksum", cksum_main),
        Program::bin("md5sum", md5sum_main),
        Program::bin("sha1sum", sha1sum_main),
        Program::bin("sha224sum", sha224sum_main),
        Program::bin("sha256sum", sha256sum_main),
        Program::bin("sha384sum", sha384sum_main),
        Program::bin("sha512sum", sha512sum_main),
        Program::bin("b2sum", b2sum_main),
        Program::bin("base64", base64_main),
        Program::bin("base32", base32_main),
        Program::bin("basenc", basenc_main),
    ]
}
