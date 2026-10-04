//! `git init`: cria (ou reinicializa) o repositório, copiando o diretório de modelos do sandbox
//! (`/usr/share/git-core/templates`) quando existe.

use super::Git;
use crate::error::{Fail, R, hint, warning};
use crate::opts::{self, Spec};
use crate::os;
use crate::refs;
use crate::repo::{self, Globals};

const SPECS: &[Spec] = &[
    opts::flag(Some(b'q'), "quiet", "quiet"),
    opts::flag(None, "bare", "bare"),
    opts::value(Some(b'b'), "initial-branch", "branch"),
    opts::value(None, "template", "template"),
    opts::value(None, "separate-git-dir", "separate"),
    opts::optional(None, "shared", "shared"),
    opts::value(None, "object-format", "object-format"),
    opts::value(None, "ref-format", "ref-format"),
];

const DESCRIPTION: &[u8] = b"Unnamed repository; edit this file 'description' to name the repository.\n";
const EXCLUDE: &[u8] = b"# git ls-files --others --exclude-from=.git/info/exclude\n# Lines that start with '#' are comments.\n# For a project mostly in C, the following would be a good set of\n# exclude patterns (uncomment them if you want to use them):\n# *.[oa]\n# *~\n";

const DEFAULT_BRANCH_HINT: &str = "Using 'master' as the name for the initial branch. This default branch name\nis subject to change. To configure the initial branch name to use in all\nof your new repositories, which will suppress this warning, call:\n\n\tgit config --global init.defaultBranch <name>\n\nNames commonly chosen instead of 'master' are 'main', 'trunk' and\n'development'. The just-created branch can be renamed via this command:\n\n\tgit branch -m <name>";

fn mkdir(p: &[u8]) -> R<()> {
    os::mkdir_p(p, 0o777).map_err(|e| Fail::Fatal(format!("cannot mkdir {}: {}", os::lossy(p), e.message())))
}

/// Copia o diretório de modelos (sem sobrescrever o que já existe).
fn copy_dir(src: &[u8], dst: &[u8]) -> R<()> {
    mkdir(dst)?;
    let Ok(entries) = os::read_dir(src) else { return Ok(()) };
    for e in entries {
        let s = os::join(src, &e.name);
        let d = os::join(dst, &e.name);
        if os::exists(&d) {
            continue;
        }
        let Ok(st) = os::lstat(&s) else { continue };
        match st.file_type() {
            sysabi::FileType::Directory => copy_dir(&s, &d)?,
            sysabi::FileType::Symlink => {
                if let Ok(t) = os::readlink(&s) {
                    let _ = os::symlink(&t, &d);
                }
            }
            _ => {
                let data = os::read(&s).map_err(|e| Fail::Fatal(format!("cannot open '{}': {}", os::lossy(&s), e.message())))?;
                os::write(&d, &data, st.mode & 0o777).map_err(|e| Fail::Fatal(format!("cannot copy '{}' to '{}': {}", os::lossy(&s), os::lossy(&d), e.message())))?;
            }
        }
    }
    Ok(())
}

fn builtin_templates(git_dir: &[u8]) -> R<()> {
    for d in ["branches", "hooks", "info"] {
        mkdir(&os::join(git_dir, d.as_bytes()))?;
    }
    let desc = os::join(git_dir, b"description");
    if !os::exists(&desc) {
        os::write(&desc, DESCRIPTION, 0o666).map_err(|e| Fail::Fatal(e.message().to_string()))?;
    }
    let ex = os::join(git_dir, b"info/exclude");
    if !os::exists(&ex) {
        os::write(&ex, EXCLUDE, 0o666).map_err(|e| Fail::Fatal(e.message().to_string()))?;
    }
    Ok(())
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(SPECS, args, 0, usage)?;
    let quiet = p.has("quiet");
    let mut bare = p.has("bare") || git.globals.bare;
    if p.args.len() > 1 {
        return Err(opts::usage_error(usage, "too many arguments"));
    }
    if let Some(dir) = p.args.first() {
        mkdir(dir)?;
        os::chdir(dir).map_err(|e| Fail::Fatal(format!("cannot chdir to {}: {}", os::lossy(dir), e.message())))?;
    }
    let cwd = os::getcwd().map_err(|e| Fail::Fatal(e.message().to_string()))?;
    let env_git_dir = git.globals.git_dir.clone().or_else(|| os::getenv("GIT_DIR"));
    let git_dir = match &env_git_dir {
        Some(d) => os::absolute(d),
        None => {
            if bare {
                cwd.clone()
            } else {
                os::join(&cwd, b".git")
            }
        }
    };
    let work_tree = git.globals.work_tree.clone().or_else(|| os::getenv("GIT_WORK_TREE"));
    if env_git_dir.is_some() && work_tree.is_none() && !p.has("bare") {
        // `GIT_DIR=x git init`: árvore de trabalho é o cwd, a não ser que pareça bare.
        bare = false;
    }
    let branch_opt = p.value_str("branch");
    if let Some(b) = &branch_opt
        && !refs::valid_branch_name(b)
    {
        return Err(Fail::Fatal(format!("invalid initial branch name: '{b}'")));
    }
    let reinit = repo::is_git_directory(&git_dir);
    let base_cfg = repo::full_config(&Globals { config: git.globals.config.clone(), ..Globals::default() }, None, None)?;

    // Modelos.
    let explicit_tpl = p.value("template").map(|t| t.to_vec()).or_else(|| os::getenv("GIT_TEMPLATE_DIR"));
    let tpl = explicit_tpl.clone().or_else(|| base_cfg.get_bytes("init.templatedir").map(|t| crate::ignore::expand_user(&t)));
    mkdir(&git_dir)?;
    match tpl {
        Some(t) if !t.is_empty() => {
            if os::is_dir(&t) {
                copy_dir(&t, &git_dir)?;
            } else {
                warning(&format!("templates not found in {}", os::lossy(&t)));
            }
        }
        Some(_) => {}
        None => {
            let sys = b"/usr/share/git-core/templates";
            if os::is_dir(sys) {
                copy_dir(sys, &git_dir)?;
            } else {
                builtin_templates(&git_dir)?;
            }
        }
    }
    for d in ["refs", "refs/heads", "refs/tags", "objects", "objects/info", "objects/pack"] {
        mkdir(&os::join(&git_dir, d.as_bytes()))?;
    }
    let head = os::join(&git_dir, b"HEAD");
    if !reinit || !os::exists(&head) {
        let branch = match branch_opt.clone().or_else(|| base_cfg.get("init.defaultbranch")) {
            Some(b) => {
                if !refs::valid_branch_name(&b) {
                    return Err(Fail::Fatal(format!("invalid initial branch name: '{b}'")));
                }
                b
            }
            None => {
                if !quiet && base_cfg.get_bool("advice.defaultbranchname")?.unwrap_or(true) {
                    hint(DEFAULT_BRANCH_HINT);
                }
                "master".to_string()
            }
        };
        os::write(&head, format!("ref: refs/heads/{branch}\n").as_bytes(), 0o666).map_err(|e| Fail::Fatal(e.message().to_string()))?;
    } else if let Some(b) = &branch_opt {
        warning(&format!("re-init: ignored --initial-branch={b}"));
    }
    let cfg_path = os::join(&git_dir, b"config");
    if !os::exists(&cfg_path) {
        let mut cfg = String::from("[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n");
        cfg.push_str(&format!("\tbare = {}\n", if bare { "true" } else { "false" }));
        if !bare {
            cfg.push_str("\tlogallrefupdates = true\n");
        }
        if let Some(w) = &work_tree
            && env_git_dir.is_some()
        {
            cfg.push_str(&format!("\tworktree = {}\n", os::lossy(&os::absolute(w))));
        }
        os::write(&cfg_path, cfg.as_bytes(), 0o666).map_err(|e| Fail::Fatal(e.message().to_string()))?;
    }
    if !quiet {
        let mut shown = git_dir.clone();
        if !shown.ends_with(b"/") {
            shown.push(b'/');
        }
        let verb = if reinit { "Reinitialized existing" } else { "Initialized empty" };
        os::outs(&format!("{verb} Git repository in {}\n", os::lossy(&shown)));
    }
    Ok(0)
}
