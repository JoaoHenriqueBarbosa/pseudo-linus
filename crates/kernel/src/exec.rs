//! `execve`: resolve o arquivo, reconhece o programa embutido ou o `#!`, monta o argv final.
//!
//! Um programa embutido é uma cópia do `/usr/bin/true` real do Debian com o build-id trocado por um
//! derivado do caminho do programa na tabela do sandbox (ver [`builtin_file`]). Como o programa é
//! identificado pelo conteúdo, `cp /usr/bin/ls /tmp/x && /tmp/x` funciona como no Linux, e hardlink e
//! symlink também; e nada no arquivo diz que ele é embutido.
//!
//! `#!` segue o `binfmt_script` da 6.12: a linha vai até o `\n` dentro dos primeiros 256 bytes (sem `\n`,
//! o nome do interpretador não pode estar truncado), espaços e tabs no fim são cortados, o resto depois
//! do nome vira um único argumento opcional, e o argv fica `[interp, arg?, caminho do script, argv[1..]]`.
//! Até 5 níveis de interpretador; mais que isso dá ELOOP.

use sysabi::{Errno, Program};
use vfs::{Caller, Loc, Start};

use crate::sandbox::SbInner;

/// `MAX_ARG_STRLEN`: 32 páginas por string (com o NUL).
const MAX_ARG_STRLEN: usize = 32 * 4096;
/// `ARG_MAX` efetivo com a pilha de 8 MiB do Debian (um quarto da pilha).
const ARG_MAX: usize = 2 * 1024 * 1024;

/// O `/usr/bin/true` real do Debian 13 (coreutils 9.7), copiado do oráculo: é o molde de todo
/// executável embutido, e as ferramentas de ELF (readelf, size, strip) inspecionam um binário de
/// verdade.
pub(crate) const REAL_TRUE: &[u8] = include_bytes!("../real/true.elf");
const REAL_TRUE_PATH: &str = "/usr/bin/true";

/// Conteúdo do arquivo de um programa embutido: o `/usr/bin/true` real do Debian com o build-id
/// trocado por um derivado do caminho do programa. Nada no arquivo denuncia que ele é embutido:
/// `file`, `readelf -n` e `cat` veem um ELF do Debian como outro qualquer; o build-id é o que o
/// kernel usa para saber qual programa da tabela rodar.
pub(crate) fn builtin_file(path: &str) -> Vec<u8> {
    let mut f = REAL_TRUE.to_vec();
    if path != REAL_TRUE_PATH {
        f[BUILD_ID_OFFSET..BUILD_ID_OFFSET + BUILD_ID_LEN].copy_from_slice(&build_id(path.as_bytes()));
    }
    f
}

/// Onde fica o descritor do `NT_GNU_BUILD_ID` no `true` real (`readelf -n`), e o tamanho dele.
const BUILD_ID_OFFSET: usize = 896;
const BUILD_ID_LEN: usize = 20;

/// Build-id de um programa embutido: 160 bits estáveis derivados do caminho (FNV-1a de 64 bits com
/// três sementes, com mistura final), com cara de SHA-1 como os do Debian.
pub(crate) fn build_id(path: &[u8]) -> [u8; BUILD_ID_LEN] {
    let mut out = [0u8; 24];
    for (i, seed) in [0xcbf2_9ce4_8422_2325u64, 0x9e37_79b9_7f4a_7c15, 0xc2b2_ae3d_27d4_eb4f].into_iter().enumerate() {
        let mut h = seed;
        for b in path {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // Mistura final do splitmix64, para que caminhos parecidos não deem ids parecidos.
        h ^= h >> 30;
        h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h ^= h >> 27;
        h = h.wrapping_mul(0x94d0_49bb_1331_11eb);
        h ^= h >> 31;
        out[i * 8..i * 8 + 8].copy_from_slice(&h.to_le_bytes());
    }
    let mut id = [0u8; BUILD_ID_LEN];
    id.copy_from_slice(&out[..BUILD_ID_LEN]);
    id
}

/// O build-id de um ELF embutido, se o arquivo for um (o começo igual ao do `true` real).
fn parse_builtin(head: &[u8]) -> Option<[u8; BUILD_ID_LEN]> {
    if head.len() < BUILD_ID_OFFSET + BUILD_ID_LEN || head[..BUILD_ID_OFFSET] != REAL_TRUE[..BUILD_ID_OFFSET] {
        return None;
    }
    let mut id = [0u8; BUILD_ID_LEN];
    id.copy_from_slice(&head[BUILD_ID_OFFSET..BUILD_ID_OFFSET + BUILD_ID_LEN]);
    Some(id)
}

/// O build-id é o do `true` real do Debian.
pub(crate) fn is_real_true_id(id: &[u8; BUILD_ID_LEN]) -> bool {
    id[..] == REAL_TRUE[BUILD_ID_OFFSET..BUILD_ID_OFFSET + BUILD_ID_LEN]
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
        if let Some(id) = parse_builtin(&f.head) {
            let name = sb.builtin_path(&id).ok_or(Errno::ENOEXEC)?;
            let program = sb.program(&name).ok_or(Errno::ENOEXEC)?;
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
    fn builtin_elf_is_a_debian_elf_with_its_own_build_id() {
        let cat = builtin_file("/usr/bin/cat");
        let ls = builtin_file("/usr/bin/ls");
        assert_eq!(cat.len(), REAL_TRUE.len());
        assert_eq!(parse_builtin(&cat), Some(build_id(b"/usr/bin/cat")));
        assert_ne!(parse_builtin(&cat), parse_builtin(&ls));
        // Só o build-id muda em relação ao `true` real.
        let diff: Vec<usize> = (0..cat.len()).filter(|&i| cat[i] != REAL_TRUE[i]).collect();
        assert!(diff.iter().all(|&i| (BUILD_ID_OFFSET..BUILD_ID_OFFSET + BUILD_ID_LEN).contains(&i)), "{diff:?}");
        assert_eq!(builtin_file("/usr/bin/true"), REAL_TRUE);
        assert!(is_real_true_id(&parse_builtin(REAL_TRUE).unwrap()));
        // Nenhum resto do formato antigo nem do nome do projeto.
        for f in [&cat, &ls] {
            let s = String::from_utf8_lossy(f).to_lowercase();
            assert!(!s.contains("pseudo") && !s.contains("builtin") && !s.contains("/usr/bin/cat"));
        }
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
