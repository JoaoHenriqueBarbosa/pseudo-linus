//! `strip` do GNU binutils 2.44 (Debian 13) para ELF64 x86_64 little-endian sem DWARF comprimido.
//!
//! Escrito a partir do comportamento observado e da man page (o código do binutils é GPL e não
//! foi consultado). Opções: `-s`/`--strip-all` (padrão), `-g`/`-S`/`-d`/`--strip-debug`,
//! `--strip-unneeded`, `-R`/`--remove-section`, `-o arquivo`, `-V`, `-h`.
//!
//! Estratégia: seções removidas somem; o conteúdo dos segmentos fica nos mesmos deslocamentos;
//! o que não é alocado e sobrevive vai para depois da última região fixa, seguido de
//! `.shstrtab` (reconstruída, com `.shstrtab` na frente e fusão de sufixos) e da tabela de
//! cabeçalhos de seção alinhada a 8.
//!
//! Divergências conhecidas: em objeto relocável com relocações `-s` mantém `.symtab` inteira
//! (o original descarta só o que não é preciso); `-x`, `-X`, `-K`, `-N`, `--only-keep-debug`,
//! `--strip-dwo`, `-p` e `-v` são aceitos sem efeito; arquivos `.a` não são abertos; a ordem
//! exata dos nomes em `.shstrtab` pode divergir do original.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, FileType, OFlags, sys};

use crate::strings::{TARGETS, expand_response_files};
use crate::util::io::{self, File};
use crate::util::{Getopt, HasArg, LongOpt};

const SHORTOPTS: &str = "I:O:F:K:N:R:o:sSgdxXpVvhHwDUM";

const ID_KEEP_SECTION: i32 = 256;
const ID_STRIP_DWO: i32 = 257;
const ID_UNNEEDED: i32 = 258;
const ID_ONLY_KEEP_DEBUG: i32 = 259;
const ID_NO_MERGE_NOTES: i32 = 260;
const ID_KEEP_SECTION_SYMBOLS: i32 = 261;
const ID_KEEP_FILE_SYMBOLS: i32 = 262;
const ID_INFO: i32 = 263;
const ID_REMOVE_RELOCS: i32 = 264;
const ID_STRIP_SECTION_HEADERS: i32 = 265;

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("input-target", HasArg::Required, 'I' as i32),
    LongOpt::new("output-target", HasArg::Required, 'O' as i32),
    LongOpt::new("target", HasArg::Required, 'F' as i32),
    LongOpt::new("preserve-dates", HasArg::No, 'p' as i32),
    LongOpt::new("enable-deterministic-archives", HasArg::No, 'D' as i32),
    LongOpt::new("disable-deterministic-archives", HasArg::No, 'U' as i32),
    LongOpt::new("remove-section", HasArg::Required, 'R' as i32),
    LongOpt::new("keep-section", HasArg::Required, ID_KEEP_SECTION),
    LongOpt::new("strip-all", HasArg::No, 's' as i32),
    LongOpt::new("strip-debug", HasArg::No, 'g' as i32),
    LongOpt::new("strip-dwo", HasArg::No, ID_STRIP_DWO),
    LongOpt::new("strip-unneeded", HasArg::No, ID_UNNEEDED),
    LongOpt::new("only-keep-debug", HasArg::No, ID_ONLY_KEEP_DEBUG),
    LongOpt::new("merge-notes", HasArg::No, 'M' as i32),
    LongOpt::new("no-merge-notes", HasArg::No, ID_NO_MERGE_NOTES),
    LongOpt::new("strip-symbol", HasArg::Required, 'N' as i32),
    LongOpt::new("keep-section-symbols", HasArg::No, ID_KEEP_SECTION_SYMBOLS),
    LongOpt::new("keep-symbol", HasArg::Required, 'K' as i32),
    LongOpt::new("keep-file-symbols", HasArg::No, ID_KEEP_FILE_SYMBOLS),
    LongOpt::new("wildcard", HasArg::No, 'w' as i32),
    LongOpt::new("discard-all", HasArg::No, 'x' as i32),
    LongOpt::new("discard-locals", HasArg::No, 'X' as i32),
    LongOpt::new("verbose", HasArg::No, 'v' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("info", HasArg::No, ID_INFO),
    LongOpt::new("remove-relocations", HasArg::Required, ID_REMOVE_RELOCS),
    LongOpt::new("strip-section-headers", HasArg::No, ID_STRIP_SECTION_HEADERS),
];

const USAGE_LINES: &[&str] = &[
    " Removes symbols and sections from files",
    " The options are:",
    "  -I --input-target=<bfdname>      Assume input file is in format <bfdname>",
    "  -O --output-target=<bfdname>     Create an output file in format <bfdname>",
    "  -F --target=<bfdname>            Set both input and output format to <bfdname>",
    "  -p --preserve-dates              Copy modified/access timestamps to the output",
    "  -D --enable-deterministic-archives",
    "                                   Produce deterministic output when stripping archives (default)",
    "  -U --disable-deterministic-archives",
    "                                   Disable -D behavior",
    "  -R --remove-section=<name>       Also remove section <name> from the output",
    "     --remove-relocations <name>   Remove relocations from section <name>",
    "     --strip-section-headers       Strip section headers from the output",
    "  -s --strip-all                   Remove all symbol and relocation information",
    "  -g -S -d --strip-debug           Remove all debugging symbols & sections",
    "     --strip-dwo                   Remove all DWO sections",
    "     --strip-unneeded              Remove all symbols not needed by relocations",
    "     --only-keep-debug             Strip everything but the debug information",
    "  -M  --merge-notes                Remove redundant entries in note sections (default)",
    "      --no-merge-notes             Do not attempt to remove redundant notes",
    "  -N --strip-symbol=<name>         Do not copy symbol <name>",
    "     --keep-section=<name>         Do not strip section <name>",
    "  -K --keep-symbol=<name>          Do not strip symbol <name>",
    "     --keep-section-symbols        Do not strip section symbols",
    "     --keep-file-symbols           Do not strip file symbol(s)",
    "  -w --wildcard                    Permit wildcard in symbol comparison",
    "  -x --discard-all                 Remove all non-global symbols",
    "  -X --discard-locals              Remove any compiler-generated symbols",
    "  -v --verbose                     List all object files modified",
    "  -V --version                     Display this program's version number",
    "  -h --help                        Display this output",
    "     --info                        List object formats & architectures supported",
    "  -o <file>                        Place stripped output into <file>",
];

const SHT_SYMTAB: u32 = 2;
const SHT_RELA: u32 = 4;
const SHT_NOBITS: u32 = 8;
const SHT_REL: u32 = 9;
const SHT_DYNSYM: u32 = 11;
const SHT_SYMTAB_SHNDX: u32 = 18;
const SHF_ALLOC: u64 = 2;
const SHF_INFO_LINK: u64 = 0x40;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    All,
    Debug,
}

struct Sec {
    name: Vec<u8>,
    kind: u32,
    flags: u64,
    addr: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    align: u64,
    entsize: u64,
}

struct Parsed {
    etype: u16,
    phoff: usize,
    phnum: usize,
    shstrndx: usize,
    secs: Vec<Sec>,
    segs: Vec<(u64, u64)>,
}

fn rd16(d: &[u8], at: usize) -> Option<u16> {
    let b = d.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn rd32(d: &[u8], at: usize) -> Option<u32> {
    let b = d.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn rd64(d: &[u8], at: usize) -> Option<u64> {
    let b = d.get(at..at.checked_add(8)?)?;
    let mut v = [0u8; 8];
    v.copy_from_slice(b);
    Some(u64::from_le_bytes(v))
}

fn cstr(table: &[u8], off: usize) -> Vec<u8> {
    let tail = table.get(off..).unwrap_or(&[]);
    tail[..tail.iter().position(|&b| b == 0).unwrap_or(tail.len())].to_vec()
}

fn parse(d: &[u8]) -> Option<Parsed> {
    if d.len() < 64 || &d[..4] != b"\x7fELF" || d[4] != 2 || d[5] != 1 {
        return None;
    }
    let etype = rd16(d, 16)?;
    let phoff = usize::try_from(rd64(d, 32)?).ok()?;
    let shoff = usize::try_from(rd64(d, 40)?).ok()?;
    let phentsize = usize::from(rd16(d, 54)?);
    let phnum = usize::from(rd16(d, 56)?);
    let shentsize = usize::from(rd16(d, 58)?);
    let mut shnum = usize::from(rd16(d, 60)?);
    let mut shstrndx = usize::from(rd16(d, 62)?);
    let mut secs = Vec::new();
    if shoff != 0 {
        if shentsize != 64 {
            return None;
        }
        if shnum == 0 {
            shnum = usize::try_from(rd64(d, shoff.checked_add(32)?)?).ok()?;
        }
        if shstrndx == 0xffff {
            shstrndx = rd32(d, shoff.checked_add(40)?)? as usize;
        }
        if shoff.checked_add(shnum.checked_mul(64)?)? > d.len() {
            return None;
        }
        let mut raw = Vec::new();
        for i in 0..shnum {
            let b = shoff + i * 64;
            raw.push((
                rd32(d, b)? as usize,
                Sec {
                    name: Vec::new(),
                    kind: rd32(d, b + 4)?,
                    flags: rd64(d, b + 8)?,
                    addr: rd64(d, b + 16)?,
                    offset: rd64(d, b + 24)?,
                    size: rd64(d, b + 32)?,
                    link: rd32(d, b + 40)?,
                    info: rd32(d, b + 44)?,
                    align: rd64(d, b + 48)?,
                    entsize: rd64(d, b + 56)?,
                },
            ));
        }
        let strtab: Vec<u8> = raw
            .get(shstrndx)
            .and_then(|(_, s)| {
                let (o, n) = (usize::try_from(s.offset).ok()?, usize::try_from(s.size).ok()?);
                d.get(o..o.checked_add(n)?)
            })
            .unwrap_or(&[])
            .to_vec();
        for (name_off, mut s) in raw {
            s.name = cstr(&strtab, name_off);
            secs.push(s);
        }
    }
    let mut segs = Vec::new();
    if phoff != 0 && phentsize == 56 {
        for i in 0..phnum {
            let b = phoff.checked_add(i * 56)?;
            segs.push((rd64(d, b + 8)?, rd64(d, b + 32)?));
        }
    }
    Some(Parsed {
        etype,
        phoff,
        phnum,
        shstrndx,
        secs,
        segs,
    })
}

fn is_debug_name(n: &[u8]) -> bool {
    n.starts_with(b".debug")
        || n.starts_with(b".zdebug")
        || n.starts_with(b".gnu.debuglto_")
        || n.starts_with(b".gnu.linkonce.wi.")
        || n == b".line"
        || n == b".stab"
        || n == b".stabstr"
        || n.starts_with(b".stab.")
}

fn align_up(v: usize, a: u64) -> usize {
    let a = usize::try_from(a.max(1)).unwrap_or(1);
    v.div_ceil(a) * a
}

/// `.shstrtab` com `.shstrtab` na frente e fusão de sufixos entre nomes.
fn build_shstrtab(names: &[Vec<u8>]) -> (Vec<u8>, Vec<usize>) {
    let mut tab: Vec<u8> = vec![0];
    let mut placed: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut ordered: Vec<(usize, &Vec<u8>)> = names.iter().enumerate().collect();
    // `.shstrtab` primeiro, os demais na ordem das seções.
    ordered.sort_by_key(|(_, n)| n.as_slice() != b".shstrtab");
    let mut result = vec![0usize; names.len()];
    let mut deferred: Vec<(usize, &Vec<u8>)> = Vec::new();
    for (i, n) in ordered {
        if n.is_empty() {
            continue;
        }
        // Nome que é sufixo próprio de outro nome da lista fica para depois, para reaproveitar.
        let is_suffix = names
            .iter()
            .any(|m| m.len() > n.len() && m.ends_with(n.as_slice()));
        if is_suffix {
            deferred.push((i, n));
            continue;
        }
        if let Some((off, s)) = placed.iter().find(|(_, s)| s.ends_with(n.as_slice())) {
            result[i] = off + s.len() - n.len();
            continue;
        }
        let o = tab.len();
        tab.extend_from_slice(n);
        tab.push(0);
        placed.push((o, n.clone()));
        result[i] = o;
    }
    for (i, n) in deferred {
        if let Some((off, s)) = placed.iter().find(|(_, s)| s.ends_with(n.as_slice())) {
            result[i] = off + s.len() - n.len();
        }
    }
    (tab, result)
}

fn strip_bytes(d: &[u8], mode: Mode, remove: &[Vec<u8>]) -> Option<Vec<u8>> {
    let p = parse(d)?;
    let n = p.secs.len();
    if n == 0 || p.shstrndx >= n {
        return Some(d.to_vec());
    }
    let reloc_obj = p.etype == 1;
    let has_rel = p.secs.iter().any(|s| s.kind == SHT_RELA || s.kind == SHT_REL);
    let mut rm = vec![false; n];
    for i in 1..n {
        let s = &p.secs[i];
        if is_debug_name(&s.name) {
            rm[i] = true;
        }
        if mode == Mode::All && !(reloc_obj && has_rel) {
            if s.kind == SHT_SYMTAB {
                rm[i] = true;
                let l = s.link as usize;
                let used_by_dynsym = p.secs.iter().any(|x| x.kind == SHT_DYNSYM && x.link as usize == l);
                if l < n && l != p.shstrndx && !used_by_dynsym {
                    rm[l] = true;
                }
            }
            if s.kind == SHT_SYMTAB_SHNDX {
                rm[i] = true;
            }
        }
        if remove.iter().any(|r| *r == s.name) {
            rm[i] = true;
        }
    }
    for i in 1..n {
        let s = &p.secs[i];
        if (s.kind == SHT_RELA || s.kind == SHT_REL) && (s.info as usize) < n && rm[s.info as usize] {
            rm[i] = true;
        }
    }
    rm[p.shstrndx] = false;
    rm[0] = false;
    // Nada a remover: o binutils reescreve o arquivo com o mesmo layout, então a saída é idêntica.
    if !rm.iter().any(|&x| x) {
        return Some(d.to_vec());
    }
    let mut map = vec![0u32; n];
    let mut count = 0u32;
    for i in 0..n {
        if !rm[i] {
            map[i] = count;
            count += 1;
        }
    }
    let kept: Vec<usize> = (0..n).filter(|&i| !rm[i]).collect();
    let mut out: Vec<u8>;
    let mut new_off: Vec<u64> = p.secs.iter().map(|s| s.offset).collect();
    let mut fixed_end = 64usize;
    if reloc_obj {
        out = d[..64].to_vec();
    } else {
        if p.phoff != 0 {
            fixed_end = fixed_end.max(p.phoff + p.phnum * 56);
        }
        for (o, f) in &p.segs {
            fixed_end = fixed_end.max(usize::try_from(o.saturating_add(*f)).unwrap_or(0));
        }
        for &i in &kept {
            let s = &p.secs[i];
            if s.flags & SHF_ALLOC != 0 && s.kind != SHT_NOBITS {
                fixed_end = fixed_end.max(usize::try_from(s.offset.saturating_add(s.size)).unwrap_or(0));
            }
        }
        fixed_end = fixed_end.min(d.len());
        out = d[..fixed_end].to_vec();
    }
    for &i in &kept {
        let s = &p.secs[i];
        if i == 0 || i == p.shstrndx || s.kind == SHT_NOBITS {
            continue;
        }
        let in_fixed = !reloc_obj && usize::try_from(s.offset).unwrap_or(usize::MAX) < fixed_end;
        if in_fixed {
            continue;
        }
        let o = usize::try_from(s.offset).ok()?;
        let sz = usize::try_from(s.size).ok()?;
        let data = d.get(o..o.checked_add(sz)?)?;
        let at = align_up(out.len(), s.align);
        out.resize(at, 0);
        new_off[i] = at as u64;
        out.extend_from_slice(data);
    }
    let names: Vec<Vec<u8>> = kept.iter().map(|&i| p.secs[i].name.clone()).collect();
    let (tab, name_offs) = build_shstrtab(&names);
    let shstr_off = out.len();
    out.extend_from_slice(&tab);
    let shoff = align_up(out.len(), 8);
    out.resize(shoff, 0);
    for (k, &i) in kept.iter().enumerate() {
        let s = &p.secs[i];
        let (off, size) = if i == p.shstrndx {
            (shstr_off as u64, tab.len() as u64)
        } else if i == 0 {
            (0, 0)
        } else {
            (new_off[i], s.size)
        };
        let link = if (s.link as usize) < n && !rm[s.link as usize] {
            map[s.link as usize]
        } else {
            0
        };
        let remap_info = matches!(s.kind, SHT_REL | SHT_RELA) || s.flags & SHF_INFO_LINK != 0;
        let info = if remap_info && (s.info as usize) < n && !rm[s.info as usize] {
            map[s.info as usize]
        } else if remap_info {
            0
        } else {
            s.info
        };
        out.extend_from_slice(&(name_offs[k] as u32).to_le_bytes());
        out.extend_from_slice(&s.kind.to_le_bytes());
        out.extend_from_slice(&s.flags.to_le_bytes());
        out.extend_from_slice(&s.addr.to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&link.to_le_bytes());
        out.extend_from_slice(&info.to_le_bytes());
        out.extend_from_slice(&s.align.to_le_bytes());
        out.extend_from_slice(&s.entsize.to_le_bytes());
    }
    out[40..48].copy_from_slice(&(shoff as u64).to_le_bytes());
    out[60..62].copy_from_slice(&(count as u16).to_le_bytes());
    out[62..64].copy_from_slice(&(map[p.shstrndx] as u16).to_le_bytes());
    Some(out)
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(prog: &str, to_stdout: bool) -> i32 {
    let mut text = format!("Usage: {prog} <option(s)> in-file(s)\n");
    text.push_str(&USAGE_LINES.join("\n"));
    text.push('\n');
    text.push_str(&format!(
        "{prog}: supported targets: {}\n",
        TARGETS.join(" ")
    ));
    if to_stdout {
        text.push_str("Report bugs to <https://sourceware.org/bugzilla/>\n");
        let mut out = io::stdout();
        let _ = out.write_all(text.as_bytes());
        0
    } else {
        io::eprint(text);
        1
    }
}

fn run(args: &[OsString]) -> i32 {
    let prog = io::argv0(args);
    let argv = match expand_response_files(&prog, io::args_bytes(args)) {
        Ok(a) => a,
        Err(c) => return c,
    };
    let rest: Vec<Vec<u8>> = argv.get(1..).unwrap_or(&[]).to_vec();
    let posix = sys::try_current().is_some_and(|s| s.getenv(b"POSIXLY_CORRECT").is_some());
    let mut g = Getopt::new(&rest, SHORTOPTS, LONGOPTS, posix);
    let mut mode = Mode::All;
    let mut remove: Vec<Vec<u8>> = Vec::new();
    let mut output: Option<Vec<u8>> = None;
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(x) => x,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&prog)));
                return usage(&prog, false);
            }
        };
        let arg = opt.arg.clone().unwrap_or_default();
        match opt.id {
            ID_UNNEEDED => mode = Mode::All,
            ID_INFO => {
                return usage(&prog, true);
            }
            ID_KEEP_SECTION | ID_STRIP_DWO | ID_ONLY_KEEP_DEBUG | ID_NO_MERGE_NOTES
            | ID_KEEP_SECTION_SYMBOLS | ID_KEEP_FILE_SYMBOLS | ID_REMOVE_RELOCS
            | ID_STRIP_SECTION_HEADERS => {}
            id => match u8::try_from(id).unwrap_or(0) {
                b's' => mode = Mode::All,
                b'g' | b'S' | b'd' => mode = Mode::Debug,
                b'R' => remove.push(arg),
                b'o' => output = Some(arg),
                b'h' | b'H' => return usage(&prog, true),
                b'V' => {
                    super::ar::print_version("strip");
                    return 0;
                }
                _ => {}
            },
        }
    }
    let files = g.operands();
    if files.is_empty() {
        return usage(&prog, false);
    }
    if output.is_some() && files.len() > 1 {
        io::eprint(format!(
            "{prog}: Warning: -o ignored: more than one input file given\n"
        ));
        output = None;
    }
    let mut status = 0;
    for f in &files {
        let dest = output.clone().unwrap_or_else(|| f.clone());
        if process(&prog, f, &dest, mode, &remove).is_err() {
            status = 1;
        }
    }
    status
}

fn process(
    prog: &str,
    path: &[u8],
    dest: &[u8],
    mode: Mode,
    remove: &[Vec<u8>],
) -> Result<(), ()> {
    let say = |parts: &[&[u8]]| {
        let mut m = format!("{prog}: ").into_bytes();
        for p in parts {
            m.extend_from_slice(p);
        }
        m.push(b'\n');
        io::eprint(m);
    };
    let st = match sys::stat(path) {
        Err(Errno::ENOENT) => {
            say(&[b"'", path, b"': No such file"]);
            return Err(());
        }
        Err(e) => {
            say(&[path, b": ", e.message().as_bytes()]);
            return Err(());
        }
        Ok(st) if st.file_type() == FileType::Directory => {
            say(&[b"Warning: '", path, b"' is a directory"]);
            return Err(());
        }
        Ok(st) => st,
    };
    let data = match io::read_path(path) {
        Ok(d) => d,
        Err(e) => {
            say(&[path, b": ", e.message().as_bytes()]);
            return Err(());
        }
    };
    let Some(result) = strip_bytes(&data, mode, remove) else {
        say(&[path, b": file format not recognized"]);
        return Err(());
    };
    let write = || -> Result<(), Errno> {
        let mut f = File::open_with(
            dest,
            OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC,
            st.mode & 0o777,
        )?;
        f.write_all(&result).map_err(|e| io::io_errno(&e))
    };
    if let Err(e) = write() {
        say(&[dest, b": ", e.message().as_bytes()]);
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shstrtab_merges_suffixes_and_starts_with_self() {
        let names = vec![
            Vec::new(),
            b".text".to_vec(),
            b".rela.text".to_vec(),
            b".shstrtab".to_vec(),
        ];
        let (tab, offs) = build_shstrtab(&names);
        assert_eq!(&tab[..10], b"\0.shstrtab");
        assert_eq!(offs[3], 1);
        assert_eq!(offs[0], 0);
        // `.text` é sufixo de `.rela.text`: aponta para dentro dele.
        assert_eq!(offs[1], offs[2] + 5);
    }

    #[test]
    fn debug_names() {
        assert!(is_debug_name(b".debug_info"));
        assert!(!is_debug_name(b".text"));
    }
}
