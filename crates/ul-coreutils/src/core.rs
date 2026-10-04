//! Grupo "core": os utilitários que vieram do porte do F06 (cat, head, wc, sort, ls), agora sobre o
//! sysio do sysabi.

use sysabi::Program;

use crate::run::uu_main;

uu_main!(cat_main, "cat", uu_cat);
uu_main!(head_main, "head", uu_head);
uu_main!(wc_main, "wc", uu_wc);
uu_main!(sort_main, "sort", uu_sort);
uu_main!(ls_main, "ls", uu_ls);

pub(crate) fn programs() -> Vec<Program> {
    vec![
        Program::bin("cat", cat_main),
        Program::bin("head", head_main),
        Program::bin("wc", wc_main),
        Program::bin("sort", sort_main),
        Program::bin("ls", ls_main),
    ]
}
