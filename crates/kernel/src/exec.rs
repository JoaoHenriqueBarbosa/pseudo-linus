//! `execve`: resolve o arquivo, reconhece o programa embutido ou o `#!`, monta o argv final.
//!
//! Um programa embutido é um arquivo regular com um cabeçalho ELF64 válido (64 bytes, `ET_DYN`, x86-64)
//! seguido de [`BUILTIN_MARKER`] e do caminho do programa na tabela do sandbox. Como o programa é
//! identificado pelo conteúdo, `cp /usr/bin/ls /tmp/x && /tmp/x` funciona como no Linux, e hardlink e
//! symlink também.
//!
//! `#!` segue o `binfmt_script` da 6.12: a linha vai até o `\n` dentro dos primeiros 256 bytes (sem `\n`,
//! o nome do interpretador não pode estar truncado), espaços e tabs no fim são cortados, o resto depois
//! do nome vira um único argumento opcional, e o argv fica `[interp, arg?, caminho do script, argv[1..]]`.
//! Até 5 níveis de interpretador; mais que isso dá ELOOP.

use sysabi::{Errno, Program};
use vfs::{Caller, Loc, Start};

use crate::sandbox::SbInner;

/// Marca que vem logo depois do cabeçalho ELF num executável embutido.
pub const BUILTIN_MARKER: &[u8] = b"PSEUDO-LINUS-BUILTIN\0";
const ELF_HEADER_LEN: usize = 64;
/// `MAX_ARG_STRLEN`: 32 páginas por string (com o NUL).
const MAX_ARG_STRLEN: usize = 32 * 4096;
/// `ARG_MAX` efetivo com a pilha de 8 MiB do Debian (um quarto da pilha).
const ARG_MAX: usize = 2 * 1024 * 1024;

/// O `/usr/bin/true` real do Debian 13 (coreutils 9.7), copiado do oráculo: as ferramentas de ELF
/// (readelf, size, strip) precisam de um binário verdadeiro para inspecionar.
pub(crate) const REAL_TRUE: &[u8] = include_bytes!("../real/true.elf");
const REAL_TRUE_PATH: &str = "/usr/bin/true";
/// Quanto do início do arquivo identifica o `true` real.
const REAL_TRUE_PREFIX: usize = 256;

/// Conteúdo do arquivo de um programa embutido.
pub(crate) fn builtin_file(path: &str) -> Vec<u8> {
    if path == REAL_TRUE_PATH {
        return REAL_TRUE.to_vec();
    }
    let mut h = vec![0u8; ELF_HEADER_LEN];
    h[..4].copy_from_slice(b"\x7fELF");
    h[4] = 2; // ELFCLASS64
    h[5] = 1; // ELFDATA2LSB
    h[6] = 1; // EV_CURRENT
    h[7] = 0; // ELFOSABI_SYSV
    h[16..18].copy_from_slice(&3u16.to_le_bytes()); // ET_DYN (PIE)
    h[18..20].copy_from_slice(&0x3eu16.to_le_bytes()); // EM_X86_64
    h[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
    h[52..54].copy_from_slice(&(ELF_HEADER_LEN as u16).to_le_bytes()); // e_ehsize
    h[54..56].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
    h[58..60].copy_from_slice(&64u16.to_le_bytes()); // e_shentsize
    h.extend_from_slice(BUILTIN_MARKER);
    h.extend_from_slice(path.as_bytes());
    h.push(0);
    h
}

/// Caminho do programa embutido, se o cabeçalho for de um.
fn parse_builtin(head: &[u8]) -> Option<&[u8]> {
    if head.len() >= REAL_TRUE_PREFIX && head[..REAL_TRUE_PREFIX] == REAL_TRUE[..REAL_TRUE_PREFIX] {
        return Some(REAL_TRUE_PATH.as_bytes());
    }
    if head.len() <= ELF_HEADER_LEN + BUILTIN_MARKER.len() || !head.starts_with(b"\x7fELF") {
        return None;
    }
    let rest = head[ELF_HEADER_LEN..].strip_prefix(BUILTIN_MARKER)?;
    let end = rest.iter().position(|b| *b == 0)?;
    Some(&rest[..end])
}

fn spacetab(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// `load_script`: (interpretador, argumento opcional).
fn parse_shebang(head: &[u8]) -> Result<(Vec<u8>, Option<Vec<u8>>), Errno> {
    let buf = &head[..head.len().min(256)];
    let body = &buf[2..];
    let line_end = match body.iter().position(|b| *b == b'\n') {
        Some(i) => i,
        None => {
            // Sem \n: só vale se o nome do interpretador termina antes do fim do buffer.
            let first = body.iter().position(|b| !spacetab(*b)).ok_or(Errno::ENOEXEC)?;
            let has_term = body[first..].iter().any(|b| spacetab(*b) || *b == 0);
            if !has_term {
                return Err(Errno::ENOEXEC);
            }
            body.len()
        }
    };
    let mut line = &body[..line_end];
    // O NUL também termina a linha pro kernel (as strings são de C).
    if let Some(z) = line.iter().position(|b| *b == 0) {
        line = &line[..z];
    }
    while let Some((&last, rest)) = line.split_last() {
        if spacetab(last) {
            line = rest;
        } else {
            break;
        }
    }
    let start = line.iter().position(|b| !spacetab(*b)).ok_or(Errno::ENOEXEC)?;
    let line = &line[start..];
    if line.is_empty() {
        return Err(Errno::ENOEXEC);
    }
    match line.iter().position(|b| spacetab(*b)) {
        None => Ok((line.to_vec(), None)),
        Some(sep) => {
            let name = line[..sep].to_vec();
            let rest = &line[sep..];
            let a = rest.iter().position(|b| !spacetab(*b));
            Ok((name, a.map(|i| rest[i..].to_vec())))
        }
    }
}

/// E2BIG como o `copy_strings` e o `bprm_stack_limits`.
pub(crate) fn check_args(argv: &[Vec<u8>], env: &[Vec<u8>]) -> Result<(), Errno> {
    let mut total = 0usize;
    for s in argv.iter().chain(env.iter()) {
        if s.len() + 1 > MAX_ARG_STRLEN {
            return Err(Errno::E2BIG);
        }
        total += s.len() + 1;
    }
    let ptrs = 8 * (argv.len().max(1) + env.len());
    if ptrs >= ARG_MAX || total > ARG_MAX - ptrs {
        return Err(Errno::E2BIG);
    }
    Ok(())
}

/// Imagem pronta pra rodar.
#[derive(Clone)]
pub(crate) struct Image {
    pub program: Program,
    pub argv: Vec<Vec<u8>>,
    pub env: Vec<Vec<u8>>,
    /// Caminho passado ao `execve` (o `comm` sai do basename dele).
    pub filename: Vec<u8>,
    /// Arquivo executado de fato (o interpretador, num script): `/proc/<pid>/exe`.
    pub exe: Loc,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({:?}, argv {:?})", self.program, self.argv.iter().map(|a| String::from_utf8_lossy(a)).collect::<Vec<_>>())
    }
}

/// Carrega o que `execve(path, argv, env)` rodaria, no contexto `cx`.
pub(crate) fn load(sb: &SbInner, cx: &Caller, path: &[u8], argv: Vec<Vec<u8>>, env: Vec<Vec<u8>>) -> Result<Image, Errno> {
    check_args(&argv, &env)?;
    let filename = path.to_vec();
    let mut cur = path.to_vec();
    let mut argv = argv;
    for _depth in 0..=5 {
        let f = sb.ns.exec_open(cx, &Start::Cwd, &cur)?;
        if let Some(name) = parse_builtin(&f.head) {
            let program = sb.program(name).ok_or(Errno::ENOEXEC)?;
            check_args(&argv, &env)?;
            return Ok(Image { program, argv, env, filename, exe: f.loc });
        }
        if f.head.starts_with(b"#!") {
            let (interp, arg) = parse_shebang(&f.head)?;
            let mut nargv = Vec::with_capacity(argv.len() + 2);
            nargv.push(interp.clone());
            if let Some(a) = arg {
                nargv.push(a);
            }
            nargv.push(cur.clone());
            nargv.extend(argv.into_iter().skip(1));
            argv = nargv;
            cur = interp;
            continue;
        }
        return Err(Errno::ENOEXEC);
    }
    Err(Errno::ELOOP)
}

/// `comm` de um caminho: basename, até 15 bytes.
pub(crate) fn comm_of(path: &[u8]) -> Vec<u8> {
    let base = path.rsplit(|b| *b == b'/').next().unwrap_or(path);
    let mut c = base.to_vec();
    c.truncate(15);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_header_roundtrip() {
        let f = builtin_file("/usr/bin/cat");
        assert!(f.starts_with(b"\x7fELF\x02\x01\x01"));
        assert_eq!(parse_builtin(&f), Some(&b"/usr/bin/cat"[..]));
        assert_eq!(parse_builtin(b"\x7fELF garbage"), None);
    }

    #[test]
    fn shebang_like_binfmt_script() {
        assert_eq!(parse_shebang(b"#!/bin/sh\necho").unwrap(), (b"/bin/sh".to_vec(), None));
        assert_eq!(parse_shebang(b"#! /usr/bin/env  bash -x  \n").unwrap(), (b"/usr/bin/env".to_vec(), Some(b"bash -x".to_vec())));
        assert_eq!(parse_shebang(b"#!\n").unwrap_err(), Errno::ENOEXEC);
        assert_eq!(parse_shebang(b"#!   \t\n").unwrap_err(), Errno::ENOEXEC);
        let long = [b"#!/".as_slice(), &[b'a'; 300]].concat();
        assert_eq!(parse_shebang(&long).unwrap_err(), Errno::ENOEXEC, "nome truncado");
        let mut ok = b"#!/bin/sh ".to_vec();
        ok.extend_from_slice(&[b'x'; 300]);
        assert_eq!(parse_shebang(&ok).unwrap().0, b"/bin/sh".to_vec());
    }

    #[test]
    fn e2big_limits() {
        assert!(check_args(&[b"a".to_vec()], &[]).is_ok());
        assert_eq!(check_args(&[vec![b'a'; MAX_ARG_STRLEN]], &[]), Err(Errno::E2BIG));
        let many: Vec<Vec<u8>> = (0..20).map(|_| vec![b'a'; 120_000]).collect();
        assert_eq!(check_args(&many, &[]), Err(Errno::E2BIG));
    }

    #[test]
    fn comm_is_basename_truncated() {
        assert_eq!(comm_of(b"/usr/bin/a-very-long-program-name"), b"a-very-long-pro".to_vec());
        assert_eq!(comm_of(b"ls"), b"ls".to_vec());
    }
}
