//! Grupo "text": processamento de texto (tail, cut, tr, uniq, nl, paste, join, comm, fold, fmt,
//! expand, unexpand, tac, tsort, ptx, pr, shuf, split, csplit).

use sysabi::Program;

use crate::run::uu_main;

uu_main!(tail_main, "tail", uu_tail);
uu_main!(cut_main, "cut", uu_cut);
uu_main!(tr_main, "tr", uu_tr);
uu_main!(uniq_main, "uniq", uu_uniq);
uu_main!(nl_main, "nl", uu_nl);
uu_main!(paste_main, "paste", uu_paste);
uu_main!(join_main, "join", uu_join);
uu_main!(comm_main, "comm", uu_comm);
uu_main!(fold_main, "fold", uu_fold);
uu_main!(fmt_main, "fmt", uu_fmt);
uu_main!(expand_main, "expand", uu_expand);
uu_main!(unexpand_main, "unexpand", uu_unexpand);
uu_main!(tac_main, "tac", uu_tac);
uu_main!(tsort_main, "tsort", uu_tsort);
uu_main!(ptx_main, "ptx", uu_ptx);
uu_main!(pr_main, "pr", uu_pr);
uu_main!(shuf_main, "shuf", uu_shuf);
uu_main!(split_main, "split", uu_split);
uu_main!(csplit_main, "csplit", uu_csplit);

pub(crate) fn programs() -> Vec<Program> {
    vec![
        Program::bin("tail", tail_main),
        Program::bin("cut", cut_main),
        Program::bin("tr", tr_main),
        Program::bin("uniq", uniq_main),
        Program::bin("nl", nl_main),
        Program::bin("paste", paste_main),
        Program::bin("join", join_main),
        Program::bin("comm", comm_main),
        Program::bin("fold", fold_main),
        Program::bin("fmt", fmt_main),
        Program::bin("expand", expand_main),
        Program::bin("unexpand", unexpand_main),
        Program::bin("tac", tac_main),
        Program::bin("tsort", tsort_main),
        Program::bin("ptx", ptx_main),
        Program::bin("pr", pr_main),
        Program::bin("shuf", shuf_main),
        Program::bin("split", split_main),
        Program::bin("csplit", csplit_main),
    ]
}
