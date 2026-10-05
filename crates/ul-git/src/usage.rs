//! Textos de uso (`git <comando> -h`) do git 2.47.3, capturados do oráculo. Aparecem no `-h` (stdout)
//! e depois de erro de opção (stderr), sempre com exit 129.

pub const GIT: &str = include_str!("usage/_git.txt");

/// Só a parte `usage: git [...]` do `git` sem argumentos (o que vem depois de `unknown option`).
pub fn git_short() -> &'static str {
    let end = GIT.find("\n\n").map(|i| i + 1).unwrap_or(GIT.len());
    &GIT[..end]
}

pub fn of(cmd: &str) -> &'static str {
    match cmd {
        "add" => include_str!("usage/add.txt"),
        "am" => include_str!("usage/am.txt"),
        "apply" => include_str!("usage/apply.txt"),
        "blame" => include_str!("usage/blame.txt"),
        "branch" => include_str!("usage/branch.txt"),
        "cat-file" => include_str!("usage/cat-file.txt"),
        "check-ignore" => include_str!("usage/check-ignore.txt"),
        "checkout" => include_str!("usage/checkout.txt"),
        "cherry-pick" => include_str!("usage/cherry-pick.txt"),
        "cherry" => include_str!("usage/cherry.txt"),
        "clean" => include_str!("usage/clean.txt"),
        "clone" => include_str!("usage/clone.txt"),
        "commit-tree" => include_str!("usage/commit-tree.txt"),
        "commit" => include_str!("usage/commit.txt"),
        "config" => include_str!("usage/config.txt"),
        "count-objects" => include_str!("usage/count-objects.txt"),
        "describe" => include_str!("usage/describe.txt"),
        "diff" => include_str!("usage/diff.txt"),
        "for-each-ref" => include_str!("usage/for-each-ref.txt"),
        "format-patch" => include_str!("usage/format-patch.txt"),
        "fsck" => include_str!("usage/fsck.txt"),
        "gc" => include_str!("usage/gc.txt"),
        "grep" => include_str!("usage/grep.txt"),
        "hash-object" => include_str!("usage/hash-object.txt"),
        "help" => include_str!("usage/help.txt"),
        "init" => include_str!("usage/init.txt"),
        "log" => include_str!("usage/log.txt"),
        "ls-files" => include_str!("usage/ls-files.txt"),
        "ls-tree" => include_str!("usage/ls-tree.txt"),
        "merge-base" => include_str!("usage/merge-base.txt"),
        "merge" => include_str!("usage/merge.txt"),
        "mv" => include_str!("usage/mv.txt"),
        "notes" => include_str!("usage/notes.txt"),
        "read-tree" => include_str!("usage/read-tree.txt"),
        "rebase" => include_str!("usage/rebase.txt"),
        "reflog" => include_str!("usage/reflog.txt"),
        "remote" => include_str!("usage/remote.txt"),
        "reset" => include_str!("usage/reset.txt"),
        "restore" => include_str!("usage/restore.txt"),
        "revert" => include_str!("usage/revert.txt"),
        "rev-list" => include_str!("usage/rev-list.txt"),
        "rev-parse" => include_str!("usage/rev-parse.txt"),
        "rm" => include_str!("usage/rm.txt"),
        "shortlog" => include_str!("usage/shortlog.txt"),
        "show-ref" => include_str!("usage/show-ref.txt"),
        "show" => include_str!("usage/show.txt"),
        "stash" => include_str!("usage/stash.txt"),
        "status" => include_str!("usage/status.txt"),
        "switch" => include_str!("usage/switch.txt"),
        "symbolic-ref" => include_str!("usage/symbolic-ref.txt"),
        "tag" => include_str!("usage/tag.txt"),
        "update-index" => include_str!("usage/update-index.txt"),
        "update-ref" => include_str!("usage/update-ref.txt"),
        "var" => include_str!("usage/var.txt"),
        "version" => include_str!("usage/version.txt"),
        "worktree" => include_str!("usage/worktree.txt"),
        "write-tree" => include_str!("usage/write-tree.txt"),
        _ => "",
    }
}
