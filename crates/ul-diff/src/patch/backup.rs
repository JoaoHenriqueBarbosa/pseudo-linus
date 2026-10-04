//! Nome do arquivo de backup como o GNU patch 2.8 escolhe: `-B` (prefixo no caminho), `-Y` (prefixo
//! no último componente), `-z`/`SIMPLE_BACKUP_SUFFIX` (sufixo, `.orig` por padrão) e `-V`/
//! `VERSION_CONTROL` (simples, numerado `.~N~`, ou numerado só se já existe um numerado).

use super::opts::{Opts, VersionControl};
use crate::sysutil;

fn split_dir(name: &[u8]) -> (&[u8], &[u8]) {
    match name.iter().rposition(|&c| c == b'/') {
        Some(p) => (&name[..=p], &name[p + 1..]),
        None => (b"", name),
    }
}

/// Maior N entre os `base.~N~` que existem no diretório do arquivo.
fn highest_numbered(name: &[u8]) -> usize {
    let (dir, base) = split_dir(name);
    let dir_path: &[u8] = if dir.is_empty() { b"." } else { dir };
    let Ok(entries) = sysabi::sys::read_dir(dir_path) else { return 0 };
    let mut best = 0usize;
    for e in entries {
        let Some(rest) = e.name.strip_prefix(base) else { continue };
        let Some(num) = rest.strip_prefix(b".~").and_then(|r| r.strip_suffix(b"~")) else { continue };
        if num.is_empty() || !num.iter().all(|c| c.is_ascii_digit()) || num[0] == b'0' {
            continue;
        }
        if let Ok(n) = std::str::from_utf8(num).unwrap_or("0").parse::<usize>() {
            best = best.max(n);
        }
    }
    best
}

/// Caminho do backup de `name`.
pub fn backup_name(o: &Opts, name: &[u8]) -> Vec<u8> {
    let simple = || -> Vec<u8> {
        if let Some(p) = &o.prefix {
            let mut v = p.clone();
            v.extend_from_slice(name);
            if let Some(s) = &o.suffix {
                v.extend_from_slice(s);
            }
            return v;
        }
        if let Some(p) = &o.basename_prefix {
            let (dir, base) = split_dir(name);
            let mut v = dir.to_vec();
            v.extend_from_slice(p);
            v.extend_from_slice(base);
            if let Some(s) = &o.suffix {
                v.extend_from_slice(s);
            }
            return v;
        }
        let mut v = name.to_vec();
        v.extend_from_slice(o.suffix.as_deref().unwrap_or(b".orig"));
        v
    };
    let numbered = |next: usize| -> Vec<u8> {
        let mut v = name.to_vec();
        v.extend_from_slice(format!(".~{next}~").as_bytes());
        v
    };
    match o.version_control {
        None | Some(VersionControl::Simple) => simple(),
        Some(VersionControl::Numbered) => numbered(highest_numbered(name) + 1),
        Some(VersionControl::Existing) => {
            let h = highest_numbered(name);
            if h > 0 { numbered(h + 1) } else { simple() }
        }
    }
}

/// Cria os diretórios que faltam até o pai de `path` (modo 0777 menos a umask).
pub fn make_parents(path: &[u8]) {
    let mut i = 0;
    while let Some(p) = path[i..].iter().position(|&c| c == b'/') {
        let end = i + p;
        if end > 0 {
            let dir = &path[..end];
            if !sysutil::exists(dir) {
                let _ = sysabi::sys::current().mkdirat(sysabi::Fd::CWD, dir, 0o777);
            }
        }
        i = end + 1;
    }
}
