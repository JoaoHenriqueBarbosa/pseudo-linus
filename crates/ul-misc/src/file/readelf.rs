// Copyright (c) Christos Zoulas 2003.
// All Rights Reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
// 1. Redistributions of source code must retain the above copyright
//    notice immediately at the beginning of the file, without modification,
//    this list of conditions, and the following disclaimer.
// 2. Redistributions in binary form must reproduce the above copyright
//    notice, this list of conditions and the following disclaimer in the
//    documentation and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
// ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
// ARE DISCLAIMED. IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE FOR
// ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
// OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
// HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
// OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
// SUCH DAMAGE.

//! `readelf.c` do file 5.46: o que as regras do banco não conseguem dizer de um ELF (ligação
//! dinâmica ou estática, interpretador, notas de sistema e de build-id, `stripped`), lido dos
//! cabeçalhos de programa e de seção.
//!
//! Fora do porte, por ora: arquivos de core (`ET_CORE`, `dophn_core`, notas de processo e auxv),
//! capacidades do SunOS (`SHT_SUNW_cap`) e as notas de PaX, memtag do Android e versões de
//! NetBSD, FreeBSD, DragonFly e Android, que só aparecem em binários desses sistemas.

use super::magic::{MAGIC_APPLE, MAGIC_EXTENSION, MAGIC_MIME, MagicSet};
use super::softmagic::Buffer;

const ET_REL: u16 = 1;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PT_NOTE: u32 = 4;
const SHT_SYMTAB: u32 = 2;
const SHT_NOTE: u32 = 7;
const DT_NEEDED: u64 = 1;
const DT_FLAGS_1: u64 = 0x6fff_fffb;
const DF_1_PIE: u64 = 0x0800_0000;
const NT_GNU_VERSION: u32 = 1;
const NT_GNU_BUILD_ID: u32 = 3;
const NT_GO_BUILD_ID: u32 = 4;
const NT_OPENBSD_VERSION: u32 = 1;
const GNU_OS_LINUX: u32 = 0;
const GNU_OS_HURD: u32 = 1;
const GNU_OS_SOLARIS: u32 = 2;
const GNU_OS_KFREEBSD: u32 = 3;
const GNU_OS_KNETBSD: u32 = 4;
/// `NBUFSIZE`: quanto de um segmento `PT_NOTE`, `PT_INTERP` ou `PT_DYNAMIC` é lido.
const NBUFSIZE: usize = 8192;
const SIZE_UNKNOWN: u64 = u64::MAX;

const FLAGS_DID_OS_NOTE: u32 = 0x0004;
const FLAGS_DID_BUILD_ID: u32 = 0x0010;

/// O arquivo inteiro visto pelo `pread`: o começo já lido e, além dele, o descritor.
struct Reader<'a> {
    b: &'a Buffer<'a>,
}

impl Reader<'_> {
    /// `pread(fd, buf, len, off)`: quantos bytes vieram (menos que `len` no fim do arquivo).
    fn pread(&self, buf: &mut [u8], off: u64) -> Option<usize> {
        let fb = self.b.fbuf;
        if (off as usize) < fb.len() && off as usize + buf.len() <= fb.len() {
            buf.copy_from_slice(&fb[off as usize..off as usize + buf.len()]);
            return Some(buf.len());
        }
        let Some(fd) = self.b.fd else {
            // Sem descritor só o começo existe.
            if (off as usize) >= fb.len() {
                return Some(0);
            }
            let n = (fb.len() - off as usize).min(buf.len());
            buf[..n].copy_from_slice(&fb[off as usize..off as usize + n]);
            return Some(n);
        };
        let sys = sysabi::sys::try_current()?;
        let mut got = 0;
        while got < buf.len() {
            match sys.pread(fd, &mut buf[got..], off + got as u64) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(sysabi::Errno::EINTR) => {}
                Err(_) => return None,
            }
        }
        Some(got)
    }

    /// Lê exatamente `len` bytes, ou `None`.
    fn exact(&self, len: usize, off: u64) -> Option<Vec<u8>> {
        let mut v = vec![0u8; len];
        (self.pread(&mut v, off)? == len).then_some(v)
    }
}

/// Leitura de inteiros na ordem de bytes do arquivo.
#[derive(Clone, Copy)]
struct Elf {
    class64: bool,
    big: bool,
}

impl Elf {
    fn u16(self, b: &[u8], o: usize) -> u16 {
        let a = [b[o], b[o + 1]];
        if self.big { u16::from_be_bytes(a) } else { u16::from_le_bytes(a) }
    }
    fn u32(self, b: &[u8], o: usize) -> u32 {
        let a = [b[o], b[o + 1], b[o + 2], b[o + 3]];
        if self.big { u32::from_be_bytes(a) } else { u32::from_le_bytes(a) }
    }
    fn u64(self, b: &[u8], o: usize) -> u64 {
        let mut a = [0u8; 8];
        a.copy_from_slice(&b[o..o + 8]);
        if self.big { u64::from_be_bytes(a) } else { u64::from_le_bytes(a) }
    }
    /// `elf_getu`: palavra do tamanho da classe.
    fn word(self, b: &[u8], o: usize) -> u64 {
        if self.class64 { self.u64(b, o) } else { u64::from(self.u32(b, o)) }
    }
    fn ehdr_size(self) -> usize {
        if self.class64 { 64 } else { 52 }
    }
    fn phdr_size(self) -> usize {
        if self.class64 { 56 } else { 32 }
    }
    fn shdr_size(self) -> usize {
        if self.class64 { 64 } else { 40 }
    }
    fn dyn_size(self) -> usize {
        if self.class64 { 16 } else { 8 }
    }
    /// `ELF_ALIGN`: notas são alinhadas a 4 bytes nas duas classes.
    fn align4(v: usize) -> usize {
        (v + 3) & !3
    }
}

/// Campos de um cabeçalho de programa.
struct Phdr {
    p_type: u32,
    offset: u64,
    filesz: u64,
    align: u64,
}

fn phdr(e: Elf, b: &[u8]) -> Phdr {
    if e.class64 {
        Phdr { p_type: e.u32(b, 0), offset: e.u64(b, 8), filesz: e.u64(b, 32), align: e.u64(b, 48) }
    } else {
        Phdr { p_type: e.u32(b, 0), offset: u64::from(e.u32(b, 4)), filesz: u64::from(e.u32(b, 16)), align: u64::from(e.u32(b, 28)) }
    }
}

/// Campos de um cabeçalho de seção.
struct Shdr {
    name: u32,
    sh_type: u32,
    offset: u64,
    size: u64,
}

fn shdr(e: Elf, b: &[u8]) -> Shdr {
    if e.class64 {
        Shdr { name: e.u32(b, 0), sh_type: e.u32(b, 4), offset: e.u64(b, 24), size: e.u64(b, 32) }
    } else {
        Shdr { name: e.u32(b, 0), sh_type: e.u32(b, 4), offset: u64::from(e.u32(b, 16)), size: u64::from(e.u32(b, 20)) }
    }
}

/// `file_tryelf()`: 0 se não é ELF, 1 se é, -1 em erro. O texto vai para a saída corrente do
/// `ms` (o chamador empilha um buffer e o anexa depois da descrição das regras).
pub fn file_tryelf(ms: &mut MagicSet, b: &Buffer<'_>) -> i32 {
    if ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0 {
        return 0;
    }
    let buf = b.fbuf;
    if buf.len() < 6 || buf[0] != 0x7f || (buf[1] != b'E' && buf[1] != b'e') || buf[2] != b'L' || buf[3] != b'F' {
        return 0;
    }
    if b.fd.is_none() && b.st_size as usize > buf.len() {
        // Sem descritor e sem o arquivo inteiro na mão: o C desiste do mesmo jeito (fd == -1).
        return 0;
    }
    let fsize = if b.st_mode & 0o170000 == 0o100000 || b.st_size != 0 { b.st_size } else { SIZE_UNKNOWN };
    let class64 = match buf[4] {
        1 => false,
        2 => true,
        c => {
            return if ms.print_str(&format!(", unknown class {c}")).is_err() { -1 } else { 0 };
        }
    };
    let e = Elf { class64, big: buf[5] == 2 };
    if buf.len() <= e.ehdr_size() {
        return 0;
    }
    let r = Reader { b };
    let e_type = e.u16(buf, 16);
    let (phoff, shoff) = if class64 { (e.u64(buf, 32), e.u64(buf, 40)) } else { (u64::from(e.u32(buf, 28)), u64::from(e.u32(buf, 32))) };
    let base = if class64 { 54 } else { 42 };
    let phentsize = usize::from(e.u16(buf, base));
    let phnum = e.u16(buf, base + 2);
    let shentsize = usize::from(e.u16(buf, base + 4));
    let shnum = e.u16(buf, base + 6);
    let shstrndx = e.u16(buf, base + 8);
    let mut flags = 0u32;
    let mut notecount = ms.params.elf_notes_max;
    match e_type {
        ET_EXEC | ET_DYN | ET_REL => {
            if e_type != ET_REL {
                if phnum > ms.params.elf_phnum_max {
                    return toomany(ms, "program", phnum);
                }
                if shnum > ms.params.elf_shnum_max {
                    return toomany(ms, "section", shnum);
                }
                if dophn_exec(ms, e, &r, phoff, phnum, phentsize, fsize, shnum, &mut flags, &mut notecount) == -1 {
                    return -1;
                }
            }
            if shnum > ms.params.elf_shnum_max {
                return toomany(ms, "section headers", shnum);
            }
            if doshn(ms, e, &r, shoff, shnum, shentsize, fsize, shstrndx, &mut flags, &mut notecount) == -1 {
                return -1;
            }
        }
        _ => {}
    }
    if notecount == 0 {
        return toomany(ms, "notes", ms.params.elf_notes_max);
    }
    1
}

fn toomany(ms: &mut MagicSet, name: &str, num: u16) -> i32 {
    if ms.flags & MAGIC_MIME != 0 {
        return 1;
    }
    if ms.print_str(&format!(", too many {name} ({num})")).is_err() { -1 } else { 1 }
}

#[allow(clippy::too_many_arguments)]
fn dophn_exec(
    ms: &mut MagicSet,
    e: Elf,
    r: &Reader<'_>,
    mut off: u64,
    num: u16,
    size: usize,
    fsize: u64,
    sh_num: u16,
    flags: &mut u32,
    notecount: &mut u16,
) -> i32 {
    let mime = ms.flags & MAGIC_MIME != 0;
    if num == 0 {
        return if ms.print_str(", no program header").is_err() { -1 } else { 0 };
    }
    if size != e.phdr_size() {
        return if ms.print_str(", corrupted program header size").is_err() { -1 } else { 0 };
    }
    let mut interp: Option<Vec<u8>> = None;
    let (mut pie, mut dynamic, mut need) = (false, false, 0usize);
    for _ in 0..num {
        let Some(raw) = r.exact(size, off) else {
            return if ms.print_str(&format!(", can't read elf program headers at {off}")).is_err() { -1 } else { 0 };
        };
        off += size as u64;
        let ph = phdr(e, &raw);
        let mut align = 4usize;
        let doread = match ph.p_type {
            PT_DYNAMIC => true,
            PT_NOTE => {
                if sh_num != 0 {
                    // Feito pelos cabeçalhos de seção.
                    continue;
                }
                align = ph.align as usize;
                if ph.align & 0x8000_0000 != 0 || align < 4 {
                    if ms.print_str(&format!(", invalid note alignment {:#x}", ph.align)).is_err() {
                        return -1;
                    }
                    align = 4;
                }
                true
            }
            PT_INTERP => true,
            _ => {
                if fsize != SIZE_UNKNOWN && ph.offset > fsize {
                    continue;
                }
                false
            }
        };
        let mut nbuf = Vec::new();
        if doread {
            let len = (ph.filesz as usize).min(NBUFSIZE);
            nbuf = vec![0u8; len];
            match r.pread(&mut nbuf, ph.offset) {
                Some(n) => nbuf.truncate(n),
                None => {
                    return if ms.print_str(&format!(", can't read section at {}", ph.offset)).is_err() { -1 } else { 0 };
                }
            }
        }
        match ph.p_type {
            PT_DYNAMIC => {
                dynamic = true;
                // O DF_1 decide se é PIE.
                ms.mode &= !0o111;
                let ds = e.dyn_size();
                let mut o = 0;
                while o + ds <= nbuf.len() {
                    let tag = e.word(&nbuf, o);
                    let val = e.word(&nbuf, o + ds / 2);
                    o += ds;
                    match tag {
                        DT_FLAGS_1 => {
                            if val & DF_1_PIE != 0 {
                                pie = true;
                                ms.mode |= 0o111;
                            } else {
                                ms.mode &= !0o111;
                            }
                        }
                        DT_NEEDED => need += 1,
                        _ => {}
                    }
                }
            }
            PT_INTERP => {
                need += 1;
                if mime {
                    continue;
                }
                interp = Some(if !nbuf.is_empty() && nbuf[0] != 0 {
                    let end = nbuf.iter().position(|c| *c == 0).unwrap_or(nbuf.len() - 1);
                    nbuf[..end.min(127)].to_vec()
                } else {
                    b"*empty*".to_vec()
                });
            }
            PT_NOTE => {
                if mime {
                    return 0;
                }
                let mut o = 0;
                while o < nbuf.len() {
                    o = donote(ms, e, &nbuf, o, align, flags, notecount);
                    if o == 0 {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    if mime {
        return 0;
    }
    let style = if dynamic {
        if pie && need == 0 { "static-pie" } else { "dynamically" }
    } else {
        "statically"
    };
    if ms.print_str(&format!(", {style} linked")).is_err() {
        return -1;
    }
    if let Some(i) = interp {
        let mut s = b", interpreter ".to_vec();
        s.extend(printable(&i));
        if ms.print(&s).is_err() {
            return -1;
        }
    }
    0
}

/// `file_printable()`: o que não é imprimível vira `\ooo`.
fn printable(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        if (0x20..0x7f).contains(&c) {
            out.push(c);
        } else {
            out.extend_from_slice(format!("\\{c:03o}").as_bytes());
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn doshn(
    ms: &mut MagicSet,
    e: Elf,
    r: &Reader<'_>,
    mut off: u64,
    num: u16,
    size: usize,
    fsize: u64,
    strtab: u16,
    flags: &mut u32,
    notecount: &mut u16,
) -> i32 {
    if ms.flags & MAGIC_MIME != 0 {
        return 0;
    }
    if num == 0 {
        return if ms.print_str(", no section header").is_err() { -1 } else { 0 };
    }
    if size != e.shdr_size() {
        return if ms.print_str(", corrupted section header size").is_err() { -1 } else { 0 };
    }
    let offs = off + (size as u64) * u64::from(strtab);
    let Some(raw) = r.exact(size, offs) else {
        return if ms.print_str(&format!(", missing section headers at {offs}")).is_err() { -1 } else { 0 };
    };
    let name_off = shdr(e, &raw).offset;
    if fsize != SIZE_UNKNOWN && fsize < name_off {
        return if ms.print_str(&format!(", too large section header offset {name_off}")).is_err() { -1 } else { 0 };
    }
    let (mut stripped, mut has_debug_info) = (true, false);
    for _ in 0..num {
        let mut name = [0u8; 49];
        let Some(raw) = r.exact(size, off) else {
            return if ms.print_str(&format!(", can't read elf section at {off}")).is_err() { -1 } else { 0 };
        };
        let sh = shdr(e, &raw);
        let noffs = name_off + u64::from(sh.name);
        let Some(n) = r.pread(&mut name, noffs) else {
            return if ms.print_str(&format!(", can't read name of elf section at {noffs}")).is_err() { -1 } else { 0 };
        };
        let nm = &name[..n];
        let nm = &nm[..nm.iter().position(|c| *c == 0).unwrap_or(nm.len())];
        if nm == b".debug_info" {
            has_debug_info = true;
            stripped = false;
        }
        off += size as u64;
        if sh.sh_type == SHT_SYMTAB {
            stripped = false;
        } else if fsize != SIZE_UNKNOWN && sh.offset > fsize {
            continue;
        }
        if sh.sh_type == SHT_NOTE {
            if sh.size.saturating_add(sh.offset) > fsize {
                let msg = format!(", note offset/size {:#x}+{:#x} exceeds file size {:#x}", sh.offset, sh.size, fsize);
                return if ms.print_str(&msg).is_err() { -1 } else { 0 };
            }
            if sh.size as usize > ms.params.elf_shsize_max {
                let msg = format!("Note section size too big ({} > {})", sh.size, ms.params.elf_shsize_max);
                ms.error_msg(0, &msg, 0);
                return -1;
            }
            let Some(nbuf) = r.exact(sh.size as usize, sh.offset) else {
                return if ms.print_str(&format!(", can't read elf note at {}", sh.offset)).is_err() { -1 } else { 0 };
            };
            let mut o = 0;
            while o < nbuf.len() {
                o = donote(ms, e, &nbuf, o, 4, flags, notecount);
                if o == 0 {
                    break;
                }
            }
        }
    }
    if has_debug_info && ms.print_str(", with debug_info").is_err() {
        return -1;
    }
    if ms.print_str(if stripped { ", stripped" } else { ", not stripped" }).is_err() {
        return -1;
    }
    0
}

/// `NAMEEQUALS`: o nome da nota é `s` seguido de NUL.
fn name_eq(nbuf: &[u8], noff: usize, s: &[u8]) -> bool {
    nbuf.len() > noff + s.len() && &nbuf[noff..noff + s.len()] == s && nbuf[noff + s.len()] == 0
}

/// `donote()`: uma nota; devolve o offset da próxima (0 para parar).
fn donote(ms: &mut MagicSet, e: Elf, nbuf: &[u8], mut offset: usize, _align: usize, flags: &mut u32, notecount: &mut u16) -> usize {
    if *notecount == 0 {
        return 0;
    }
    *notecount -= 1;
    let size = nbuf.len();
    if offset + 12 > size {
        return offset + 12;
    }
    let namesz = e.u32(nbuf, offset) as usize;
    let descsz = e.u32(nbuf, offset + 4) as usize;
    let ntype = e.u32(nbuf, offset + 8);
    offset += 12;
    if namesz == 0 && descsz == 0 {
        return if offset >= size { offset } else { size };
    }
    if namesz & 0x8000_0000 != 0 {
        let _ = ms.print_str(&format!(", bad note name size {namesz:#x}"));
        return 0;
    }
    if descsz & 0x8000_0000 != 0 {
        let _ = ms.print_str(&format!(", bad note description size {descsz:#x}"));
        return 0;
    }
    let noff = offset;
    let doff = Elf::align4(offset + namesz);
    if offset + namesz > size {
        return doff;
    }
    offset = Elf::align4(doff + descsz);
    if doff + descsz > size {
        return if offset >= size { offset } else { size };
    }
    if *flags & FLAGS_DID_OS_NOTE == 0 && do_os_note(ms, e, nbuf, ntype, descsz, noff, doff, flags) {
        return offset;
    }
    if *flags & FLAGS_DID_BUILD_ID == 0 && do_bid_note(ms, nbuf, ntype, namesz, descsz, noff, doff, flags) {
        return offset;
    }
    offset
}

#[allow(clippy::too_many_arguments)]
fn do_os_note(ms: &mut MagicSet, e: Elf, nbuf: &[u8], ntype: u32, descsz: usize, noff: usize, doff: usize, flags: &mut u32) -> bool {
    if name_eq(nbuf, noff, b"SuSE") && ntype == NT_GNU_VERSION && descsz == 2 {
        *flags |= FLAGS_DID_OS_NOTE;
        let _ = ms.print_str(&format!(", for SuSE {}.{}", nbuf[doff], nbuf[doff + 1]));
        return true;
    }
    if name_eq(nbuf, noff, b"GNU") && ntype == NT_GNU_VERSION && descsz == 16 {
        *flags |= FLAGS_DID_OS_NOTE;
        let os = match e.u32(nbuf, doff) {
            GNU_OS_LINUX => "Linux",
            GNU_OS_HURD => "Hurd",
            GNU_OS_SOLARIS => "Solaris",
            GNU_OS_KFREEBSD => "kFreeBSD",
            GNU_OS_KNETBSD => "kNetBSD",
            _ => "<unknown>",
        };
        let (a, b, c) = (e.u32(nbuf, doff + 4) as i32, e.u32(nbuf, doff + 8) as i32, e.u32(nbuf, doff + 12) as i32);
        let _ = ms.print_str(&format!(", for GNU/{os} {a}.{b}.{c}"));
        return true;
    }
    if name_eq(nbuf, noff, b"OpenBSD") && ntype == NT_OPENBSD_VERSION && descsz == 4 {
        *flags |= FLAGS_DID_OS_NOTE;
        let _ = ms.print_str(", for OpenBSD");
        return true;
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn do_bid_note(ms: &mut MagicSet, nbuf: &[u8], ntype: u32, namesz: usize, descsz: usize, noff: usize, doff: usize, flags: &mut u32) -> bool {
    if name_eq(nbuf, noff, b"GNU") && ntype == NT_GNU_BUILD_ID && (4..=20).contains(&descsz) {
        *flags |= FLAGS_DID_BUILD_ID;
        let btype = match descsz {
            8 => "xxHash",
            16 => "md5/uuid",
            20 => "sha1",
            _ => "unknown",
        };
        let mut s = format!(", BuildID[{btype}]=");
        for b in &nbuf[doff..doff + descsz] {
            s.push_str(&format!("{b:02x}"));
        }
        let _ = ms.print_str(&s);
        return true;
    }
    if namesz == 4 && nbuf.len() >= noff + 3 && &nbuf[noff..noff + 3] == b"Go" && ntype == NT_GO_BUILD_ID && descsz < 128 {
        let d = &nbuf[doff..doff + descsz];
        let d = &d[..d.iter().position(|c| *c == 0).unwrap_or(d.len())];
        let mut s = b", Go BuildID=".to_vec();
        s.extend_from_slice(d);
        let _ = ms.print(&s);
        return true;
    }
    false
}
