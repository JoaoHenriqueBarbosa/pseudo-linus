//! `git init [-q] [-b <ramo>]`: layout do `.git` igual ao do git 2.47 (sem os hooks de exemplo).

use harness::Entry;

use super::out;
use crate::shell::Ctx;

const HINT: &str = "hint: Using 'master' as the name for the initial branch. This default branch name
hint: is subject to change. To configure the initial branch name to use in all
hint: of your new repositories, which will suppress this warning, call:
hint:
hint: \tgit config --global init.defaultBranch <name>
hint:
hint: Names commonly chosen instead of 'master' are 'main', 'trunk' and
hint: 'development'. The just-created branch can be renamed via this command:
hint:
hint: \tgit branch -m <name>
";

const CONFIG: &str = "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n";

const EXCLUDE: &str = "# git ls-files --others --exclude-from=.git/info/exclude
# Lines that start with '#' are comments.
# For a project mostly in C, the following would be a good set of
# exclude patterns (uncomment them if you want to use them):
# *.[oa]
# *~
";

pub fn run(args: &[String], ctx: &mut Ctx<'_>) -> i32 {
    let mut quiet = false;
    let mut branch: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-q" | "--quiet" => quiet = true,
            "-b" | "--initial-branch" => {
                i += 1;
                branch = args.get(i).cloned();
            }
            _ if a.starts_with("--initial-branch=") => branch = Some(a["--initial-branch=".len()..].to_string()),
            "." => {}
            _ => {
                ctx.stderr.extend_from_slice(format!("fatal: git init {a}: só o diretório atual é suportado\n").as_bytes());
                return 128;
            }
        }
        i += 1;
    }
    let existed = ctx.fs.get(".git/HEAD").is_some();
    if !existed {
        if branch.is_none() && !quiet {
            ctx.stderr.extend_from_slice(HINT.as_bytes());
        }
        let b = branch.unwrap_or_else(|| "master".into());
        let fs = &mut *ctx.fs;
        fs.insert(".git/HEAD", Entry::file(format!("ref: refs/heads/{b}\n").into_bytes(), 0o644));
        fs.insert(".git/config", Entry::file(CONFIG.as_bytes().to_vec(), 0o644));
        fs.insert(
            ".git/description",
            Entry::file(b"Unnamed repository; edit this file 'description' to name the repository.\n".to_vec(), 0o644),
        );
        for d in ["branches", "hooks", "objects/info", "objects/pack", "refs/heads", "refs/tags"] {
            fs.insert(&format!(".git/{d}"), Entry::dir(0o755));
        }
        fs.insert(".git/info/exclude", Entry::file(EXCLUDE.as_bytes().to_vec(), 0o644));
    }
    if !quiet {
        let verb = if existed { "Reinitialized existing" } else { "Initialized empty" };
        out(ctx, &format!("{verb} Git repository in {}/.git/\n", harness::CASE_DIR));
    }
    0
}
