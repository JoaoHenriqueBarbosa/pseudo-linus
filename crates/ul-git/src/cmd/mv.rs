//! `git mv`: renomeia arquivos e diretórios na árvore de trabalho e no índice, com as verificações
//! e as mensagens do git (`bad source`, `destination exists`...).

use super::Git;
use crate::error::{Fail, R, warning};
use crate::index::{IEntry, Index};
use crate::opts::{self, Spec};
use crate::os;
use crate::pathspec;
use crate::repo::Repo;

const SPECS: &[Spec] = &[
    opts::flag(Some(b'v'), "verbose", "verbose"),
    opts::flag(Some(b'n'), "dry-run", "dry-run"),
    opts::flag(Some(b'f'), "force", "force"),
    opts::short_flag(b'k', "skip"),
    opts::flag(None, "sparse", "sparse"),
];

/// O que fazer com um par origem/destino na hora de renomear.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Mode {
    /// Arquivo: renomeia no disco e no índice.
    Both,
    /// Diretório: só no disco (as entradas do índice vêm como itens separados).
    WorkDir,
    /// Entrada do índice sob um diretório: só no índice.
    Index,
}

struct Item {
    src: Vec<u8>,
    dst: Vec<u8>,
    mode: Mode,
}

/// Caminho dado pelo usuário como caminho relativo ao topo (mantém a barra final).
fn top_relative(repo: &Repo, arg: &[u8]) -> R<Vec<u8>> {
    let outside = || {
        Fail::Fatal(format!(
            "{}: '{}' is outside repository at '{}'",
            os::lossy(arg),
            os::lossy(arg),
            os::lossy(repo.work_tree.as_deref().unwrap_or_default())
        ))
    };
    if arg.starts_with(b"/") {
        return match repo.rel_from_prefix(arg) {
            Some(mut r) => {
                if arg.ends_with(b"/") && !r.is_empty() {
                    r.push(b'/');
                }
                Ok(r)
            }
            None => Err(outside()),
        };
    }
    let joined = [repo.prefix.as_slice(), arg].concat();
    pathspec::normalize_rel(&joined).ok_or_else(outside)
}

fn strip_slashes(p: &[u8]) -> Vec<u8> {
    let mut end = p.len();
    while end > 0 && p[end - 1] == b'/' {
        end -= 1;
    }
    p[..end].to_vec()
}

fn is_dir_nofollow(p: &[u8]) -> bool {
    os::lstat(p).map(|s| s.file_type() == sysabi::FileType::Directory).unwrap_or(false)
}

pub fn run(git: &mut Git, args: &[Vec<u8>]) -> R<i32> {
    let usage = git.usage();
    let p = opts::parse(SPECS, args, 0, usage)?;
    let repo = git.repo()?;
    if p.args.len() < 2 {
        opts::usage_to_stderr(usage);
        return Err(Fail::Exit(129));
    }
    let verbose = p.has("verbose");
    let dry = p.has("dry-run");
    let force = p.has("force");
    let skip_errors = p.has("skip");
    let ipath = repo.index_path();
    let mut idx = Index::load(&ipath)?;

    let (src_args, dest_arg) = p.args.split_at(p.args.len() - 1);
    let sources: Vec<Vec<u8>> = {
        let mut v = Vec::new();
        for a in src_args {
            v.push(strip_slashes(&top_relative(repo, a)?));
        }
        v
    };
    let dest_raw = top_relative(repo, &dest_arg[0])?;

    // Cada origem ganha seu destino: dentro do diretório destino, ou o próprio caminho dado.
    let mut destinations: Vec<Vec<u8>> = Vec::new();
    if dest_raw.is_empty() {
        let base = strip_slashes(&repo.prefix);
        for s in &sources {
            destinations.push(os::join(&base, os::basename(s)));
        }
    } else if is_dir_nofollow(&dest_raw) {
        let mut d = dest_raw.clone();
        if !d.ends_with(b"/") {
            d.push(b'/');
        }
        for s in &sources {
            destinations.push([d.as_slice(), os::basename(s)].concat());
        }
    } else {
        if sources.len() != 1 {
            return Err(Fail::Fatal(format!("destination '{}' is not a directory", os::lossy(&dest_raw))));
        }
        destinations.push(dest_raw.clone());
    }

    let mut items: Vec<Item> = Vec::new();
    let mut expanded: Vec<Item> = Vec::new();
    let mut dsts_seen: Vec<Vec<u8>> = Vec::new();
    for (src, dst) in sources.iter().zip(destinations.iter()) {
        if dry {
            os::outs(&format!("Checking rename of '{}' to '{}'\n", os::lossy(src), os::lossy(dst)));
        }
        let mut mode = Mode::Both;
        let mut sub: Vec<Item> = Vec::new();
        let bad: Option<&str> = match os::lstat(src) {
            Err(_) => Some("bad source"),
            Ok(st) => {
                let is_dir = st.file_type() == sysabi::FileType::Directory;
                if dst.starts_with(src.as_slice()) && (dst.len() == src.len() || dst[src.len()] == b'/') {
                    Some("can not move directory into itself")
                } else if is_dir && os::lstat(dst).is_ok() {
                    Some("cannot move directory over file")
                } else if is_dir {
                    if idx.get(src).is_some() {
                        // Submódulo (gitlink): renomeia o diretório e a entrada.
                        None
                    } else {
                        let r = idx.dir_range(src);
                        if r.is_empty() {
                            Some("source directory is empty")
                        } else {
                            mode = Mode::WorkDir;
                            for e in &idx.entries[r] {
                                let rest = &e.path[src.len() + 1..];
                                let mut d = dst.clone();
                                d.push(b'/');
                                d.extend_from_slice(rest);
                                sub.push(Item { src: e.path.clone(), dst: d, mode: Mode::Index });
                            }
                            None
                        }
                    }
                } else if !idx.has_path(src) {
                    Some("not under version control")
                } else if idx.get(src).is_none() {
                    Some("conflicted")
                } else if os::lstat(dst).is_ok() {
                    if force {
                        if verbose {
                            warning(&format!("overwriting '{}'", os::lossy(dst)));
                        }
                        None
                    } else {
                        Some("destination exists")
                    }
                } else if dsts_seen.contains(dst) {
                    Some("multiple sources for the same target")
                } else if dst.ends_with(b"/") {
                    Some("destination directory does not exist")
                } else {
                    None
                }
            }
        };
        match bad {
            None => {
                dsts_seen.push(dst.clone());
                items.push(Item { src: src.clone(), dst: dst.clone(), mode });
                expanded.extend(sub);
            }
            Some(msg) => {
                if !skip_errors {
                    return Err(Fail::Fatal(format!("{msg}, source={}, destination={}", os::lossy(src), os::lossy(dst))));
                }
            }
        }
    }
    items.extend(expanded);

    let mut changed = false;
    for it in &items {
        if dry || verbose {
            os::outs(&format!("Renaming {} to {}\n", os::lossy(&it.src), os::lossy(&it.dst)));
        }
        if dry {
            continue;
        }
        if it.mode != Mode::Index
            && let Err(e) = os::rename(&it.src, &it.dst)
        {
            if skip_errors {
                continue;
            }
            return Err(Fail::Fatal(format!("renaming '{}' failed: {}", os::lossy(&it.src), e.message())));
        }
        if it.mode == Mode::WorkDir {
            continue;
        }
        let moved: Vec<IEntry> = idx.stages(&it.src).into_iter().cloned().collect();
        if moved.is_empty() {
            continue;
        }
        idx.remove(&it.src);
        for mut e in moved {
            e.path = it.dst.clone();
            if e.stage == 0 {
                idx.add(e);
            } else {
                idx.insert_raw(e);
            }
        }
        changed = true;
    }
    if changed && !dry {
        idx.write(&ipath)?;
    }
    Ok(0)
}
