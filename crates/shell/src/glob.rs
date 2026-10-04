//! Expansão de caminhos (glob), componente por componente, como o `glob_filename` do bash:
//! `*`, `?`, colchetes e extglob dentro de cada componente; `**` com `globstar`; ponto inicial só
//! com `dotglob` ou explícito; resultado ordenado (no C.UTF-8 a colação é por código, igual aos
//! bytes).

use sysabi::{AtFlags, Fd, FileType};

use crate::pattern::{Pattern, has_glob_meta, unescape};
use crate::shell::{Shell, sys};

/// Limite de entradas visitadas numa expansão (proteção contra `**` em árvore enorme).
const MAX_VISITED: usize = 2_000_000;

/// Expande `pat` (com `\` escapando o que era literal). Devolve vazio se nada casar.
pub fn glob(sh: &Shell, pat: &[u8]) -> Vec<Vec<u8>> {
    let opts = sh.match_opts(true);
    let globstar = sh.opts.shopt("globstar");
    let extglob = opts.extglob;
    let absolute = pat.starts_with(b"/");
    let trailing_slash = pat.ends_with(b"/") && pat.len() > 1;
    let comps: Vec<&[u8]> = split_components(pat).into_iter().filter(|c| !c.is_empty()).collect();
    if comps.is_empty() {
        return Vec::new();
    }
    // Caminhos parciais: (caminho mostrado, é diretório conhecido).
    let mut current: Vec<Vec<u8>> = vec![if absolute { b"/".to_vec() } else { Vec::new() }];
    let mut visited = 0usize;
    for (ci, comp) in comps.iter().enumerate() {
        let last = ci + 1 == comps.len();
        let mut next: Vec<Vec<u8>> = Vec::new();
        if globstar && *comp == b"**" {
            for base in &current {
                // `**` casa zero ou mais diretórios (e, no fim, também os arquivos).
                let mut found = Vec::new();
                walk(base, last, opts.dotglob, &mut found, &mut visited, 0);
                if !last {
                    next.push(base.clone());
                }
                next.extend(found);
            }
            if last {
                current = next.into_iter().filter(|p| !p.is_empty()).collect();
                if trailing_slash {
                    current = current.into_iter().filter(|p| is_dir(p)).map(|mut p| {
                        p.push(b'/');
                        p
                    }).collect();
                }
                current.sort();
                current.dedup();
                return current;
            }
            current = next;
            continue;
        }
        if !has_glob_meta(comp, extglob) {
            let lit = unescape(comp);
            for base in &current {
                let p = join(base, &lit);
                if last || is_dir(&p) || !trailing_slash {
                    next.push(p);
                }
            }
            current = next;
            continue;
        }
        let pattern = Pattern::new(comp, opts);
        for base in &current {
            let dir: &[u8] = if base.is_empty() { b"." } else { base };
            if !base.is_empty() && !is_dir(base) {
                continue;
            }
            let Ok(entries) = sysabi::sys::read_dir(dir) else { continue };
            let mut names: Vec<Vec<u8>> = Vec::new();
            for e in entries {
                visited += 1;
                if visited > MAX_VISITED {
                    break;
                }
                if pattern.matches(&e.name) {
                    let is_d = e.kind == FileType::Directory || (e.kind == FileType::Symlink && is_dir(&join(base, &e.name)));
                    if !last && !is_d {
                        continue;
                    }
                    names.push(e.name);
                }
            }
            names.sort();
            for n in names {
                next.push(join(base, &n));
            }
        }
        current = next;
    }
    // Componentes literais no meio só existem se o arquivo existir.
    let s = sys();
    let mut out: Vec<Vec<u8>> = current
        .into_iter()
        .filter(|p| !p.is_empty() && s.fstatat(Fd::CWD, p, AtFlags::SYMLINK_NOFOLLOW).is_ok())
        .collect();
    if trailing_slash {
        out = out
            .into_iter()
            .filter(|p| is_dir(p))
            .map(|mut p| {
                p.push(b'/');
                p
            })
            .collect();
    }
    out.sort();
    out
}

fn split_components(pat: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < pat.len() {
        if pat[i] == b'\\' {
            i += 2;
            continue;
        }
        if pat[i] == b'/' {
            out.push(&pat[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    out.push(&pat[start.min(pat.len())..]);
    out
}

fn join(base: &[u8], name: &[u8]) -> Vec<u8> {
    if base.is_empty() {
        return name.to_vec();
    }
    let mut p = base.to_vec();
    if !p.ends_with(b"/") {
        p.push(b'/');
    }
    p.extend_from_slice(name);
    p
}

fn is_dir(p: &[u8]) -> bool {
    let path: &[u8] = if p.is_empty() { b"." } else { p };
    sys().fstatat(Fd::CWD, path, AtFlags::empty()).map(|st| st.file_type() == FileType::Directory).unwrap_or(false)
}

/// Desce recursivamente (o `**`): diretórios sempre, arquivos só quando `include_files`.
fn walk(base: &[u8], include_files: bool, dotglob: bool, out: &mut Vec<Vec<u8>>, visited: &mut usize, depth: usize) {
    if depth > 64 || *visited > MAX_VISITED {
        return;
    }
    let dir: &[u8] = if base.is_empty() { b"." } else { base };
    let Ok(entries) = sysabi::sys::read_dir(dir) else { return };
    let mut items: Vec<(Vec<u8>, bool)> = Vec::new();
    for e in entries {
        *visited += 1;
        if e.name.starts_with(b".") && !dotglob {
            continue;
        }
        let path = join(base, &e.name);
        // O bash não segue symlink de diretório no `**`.
        let is_d = e.kind == FileType::Directory;
        items.push((path, is_d));
    }
    items.sort();
    for (path, is_d) in items {
        if is_d {
            out.push(path.clone());
            walk(&path, include_files, dotglob, out, visited, depth + 1);
        } else if include_files {
            out.push(path);
        }
    }
}
