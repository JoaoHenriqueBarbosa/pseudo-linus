//! Leitor mínimo de ELF64 little-endian (x86_64) para `ar`, `ranlib`, `size` e `nm`.
//!
//! Tudo com acesso verificado a fatias, sem `unsafe`. Arquivo que não é um ELF64 LE reconhecível
//! devolve `None` (os programas respondem `file format not recognized`).

pub const SHT_SYMTAB: u32 = 2;
pub const SHT_NOBITS: u32 = 8;

pub const SHF_WRITE: u64 = 1;
pub const SHF_ALLOC: u64 = 2;
pub const SHF_EXECINSTR: u64 = 4;

pub const SHN_UNDEF: u16 = 0;
pub const SHN_LORESERVE: u16 = 0xff00;
pub const SHN_ABS: u16 = 0xfff1;
pub const SHN_COMMON: u16 = 0xfff2;

pub const STT_OBJECT: u8 = 1;
pub const STT_SECTION: u8 = 3;
pub const STT_FILE: u8 = 4;
pub const STT_GNU_IFUNC: u8 = 10;

pub const STB_LOCAL: u8 = 0;
pub const STB_GLOBAL: u8 = 1;
pub const STB_WEAK: u8 = 2;
pub const STB_GNU_UNIQUE: u8 = 10;

#[derive(Clone, Debug)]
pub struct Section {
    pub name: Vec<u8>,
    pub kind: u32,
    pub flags: u64,
    pub addr: u64,
    pub offset: u64,
    pub size: u64,
    pub link: u32,
}

#[derive(Clone, Debug)]
pub struct Sym {
    pub name: Vec<u8>,
    pub info: u8,
    pub shndx: u16,
    pub value: u64,
    pub size: u64,
}

impl Sym {
    pub fn bind(&self) -> u8 {
        self.info >> 4
    }

    pub fn kind(&self) -> u8 {
        self.info & 0xf
    }
}

pub struct Elf<'a> {
    data: &'a [u8],
    pub sections: Vec<Section>,
}

pub(crate) fn rd16(d: &[u8], at: usize) -> Option<u16> {
    let b = d.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

pub(crate) fn rd32(d: &[u8], at: usize) -> Option<u32> {
    let b = d.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

pub(crate) fn rd64(d: &[u8], at: usize) -> Option<u64> {
    let b = d.get(at..at.checked_add(8)?)?;
    let mut v = [0u8; 8];
    v.copy_from_slice(b);
    Some(u64::from_le_bytes(v))
}

/// O nome que começa em `off` numa tabela de strings, até o NUL.
pub(crate) fn cstr(table: &[u8], off: usize) -> Vec<u8> {
    let tail = table.get(off..).unwrap_or(&[]);
    tail[..tail.iter().position(|&b| b == 0).unwrap_or(tail.len())].to_vec()
}

pub(crate) fn cstr_lossy(table: &[u8], off: usize) -> String {
    String::from_utf8_lossy(&cstr(table, off)).into_owned()
}

/// `size` bytes a partir de `off`; fora do arquivo, vazio.
pub(crate) fn slice_at(d: &[u8], off: u64, size: u64) -> &[u8] {
    let (Ok(o), Ok(n)) = (usize::try_from(off), usize::try_from(size)) else {
        return &[];
    };
    o.checked_add(n).and_then(|end| d.get(o..end)).unwrap_or(&[])
}

/// Um cabeçalho de ELF64 little-endian, o único formato que estes programas leem.
pub(crate) fn is_elf64_le(d: &[u8]) -> bool {
    d.len() >= 64 && &d[..4] == b"\x7fELF" && d[4] == 2 && d[5] == 1
}

impl<'a> Elf<'a> {
    /// Interpreta cabeçalho e tabela de seções. `None` se não for ELF64 little-endian íntegro.
    pub fn parse(data: &'a [u8]) -> Option<Elf<'a>> {
        if !is_elf64_le(data) {
            return None;
        }
        let shoff = usize::try_from(rd64(data, 40)?).ok()?;
        let shentsize = usize::from(rd16(data, 58)?);
        let mut shnum = usize::from(rd16(data, 60)?);
        let mut shstrndx = usize::from(rd16(data, 62)?);
        if shoff == 0 {
            return Some(Elf {
                data,
                sections: Vec::new(),
            });
        }
        if shentsize != 64 {
            return None;
        }
        if shnum == 0 {
            shnum = usize::try_from(rd64(data, shoff.checked_add(32)?)?).ok()?;
        }
        if shstrndx == 0xffff {
            shstrndx = rd32(data, shoff.checked_add(40)?)? as usize;
        }
        let table_end = shoff.checked_add(shnum.checked_mul(64)?)?;
        if table_end > data.len() {
            return None;
        }
        let mut raw = Vec::with_capacity(shnum);
        for i in 0..shnum {
            let b = shoff + i * 64;
            raw.push((
                rd32(data, b)? as usize,
                Section {
                    name: Vec::new(),
                    kind: rd32(data, b + 4)?,
                    flags: rd64(data, b + 8)?,
                    addr: rd64(data, b + 16)?,
                    offset: rd64(data, b + 24)?,
                    size: rd64(data, b + 32)?,
                    link: rd32(data, b + 40)?,
                },
            ));
        }
        let mut elf = Elf {
            data,
            sections: Vec::new(),
        };
        let strtab: Vec<u8> = raw
            .get(shstrndx)
            .map(|(_, s)| {
                let (o, n) = (s.offset as usize, s.size as usize);
                o.checked_add(n)
                    .and_then(|e| data.get(o..e))
                    .unwrap_or(&[])
                    .to_vec()
            })
            .unwrap_or_default();
        for (name_off, mut s) in raw {
            s.name = cstr(&strtab, name_off);
            elf.sections.push(s);
        }
        Some(elf)
    }

    /// Conteúdo de uma seção (vazio para NOBITS ou intervalo fora do arquivo).
    pub fn section_data(&self, index: usize) -> &'a [u8] {
        let Some(s) = self.sections.get(index) else {
            return &[];
        };
        if s.kind == SHT_NOBITS {
            return &[];
        }
        let (Ok(o), Ok(n)) = (usize::try_from(s.offset), usize::try_from(s.size)) else {
            return &[];
        };
        o.checked_add(n)
            .and_then(|e| self.data.get(o..e))
            .unwrap_or(&[])
    }

    /// Símbolos de `.symtab` sem a entrada nula. `None` se não existe tabela de símbolos.
    pub fn symbols(&self) -> Option<Vec<Sym>> {
        let idx = self.sections.iter().position(|s| s.kind == SHT_SYMTAB)?;
        let strtab = self.section_data(self.sections[idx].link as usize);
        let tab = self.section_data(idx);
        let mut out = Vec::new();
        for ent in tab.chunks_exact(24).skip(1) {
            out.push(Sym {
                name: cstr(strtab, rd32(ent, 0)? as usize),
                info: ent[4],
                shndx: rd16(ent, 6)?,
                value: rd64(ent, 8)?,
                size: rd64(ent, 16)?,
            });
        }
        Some(out)
    }

    /// Nomes dos símbolos que entram no índice do arquivo `ar`: globais ou fracos definidos
    /// (inclusive absolutos e comuns), sem seção nem arquivo.
    pub fn archive_index_symbols(&self) -> Vec<Vec<u8>> {
        let Some(syms) = self.symbols() else {
            return Vec::new();
        };
        syms.into_iter()
            .filter(|s| {
                matches!(s.bind(), STB_GLOBAL | STB_WEAK | STB_GNU_UNIQUE)
                    && s.shndx != SHN_UNDEF
                    && !s.name.is_empty()
                    && !matches!(s.kind(), STT_SECTION | STT_FILE)
            })
            .map(|s| s.name)
            .collect()
    }
}

pub(crate) const SHT_VERDEF: u32 = 0x6fff_fffd;
pub(crate) const SHT_VERNEED: u32 = 0x6fff_fffe;
pub(crate) const SHT_VERSYM: u32 = 0x6fff_ffff;

/// Nome do tipo de segmento (`p_type`) como o readelf escreve; o objdump tira o `GNU_`.
pub(crate) fn segment_type_name(k: u32) -> Option<&'static str> {
    Some(match k {
        0 => "NULL",
        1 => "LOAD",
        2 => "DYNAMIC",
        3 => "INTERP",
        4 => "NOTE",
        5 => "SHLIB",
        6 => "PHDR",
        7 => "TLS",
        0x6474_e550 => "GNU_EH_FRAME",
        0x6474_e551 => "GNU_STACK",
        0x6474_e552 => "GNU_RELRO",
        0x6474_e553 => "GNU_PROPERTY",
        0x6474_e554 => "GNU_SFRAME",
        _ => return None,
    })
}

/// Percorre `count` registros encadeados (verdef, verneed e os auxiliares deles): cada um diz,
/// na posição `next_at`, quanto pular até o próximo, e zero encerra. `each` recebe o deslocamento
/// do registro; `None` (registro truncado) para a caminhada.
pub(crate) fn walk_chain(data: &[u8], start: usize, count: usize, next_at: usize, mut each: impl FnMut(usize) -> Option<()>) {
    let mut off = start;
    for _ in 0..count {
        let Some(next) = rd32(data, off + next_at) else { break };
        if each(off).is_none() || next == 0 {
            break;
        }
        off += next as usize;
    }
}

/// As versões dos símbolos dinâmicos (`.gnu.version`, `.gnu.version_r`, `.gnu.version_d`).
pub(crate) struct Versions {
    pub versym: Vec<u16>,
    /// (índice, nome, vem de verneed)
    pub names: Vec<(u16, String, bool)>,
    pub has_verdef: bool,
}

impl Versions {
    /// Lê as seções de versão; cada item é (tipo, `sh_info`, conteúdo, tabela de strings ligada).
    pub fn load<'a>(sections: impl IntoIterator<Item = (u32, u32, &'a [u8], &'a [u8])>) -> Versions {
        let mut v = Versions { versym: Vec::new(), names: Vec::new(), has_verdef: false };
        for (kind, info, data, strtab) in sections {
            match kind {
                SHT_VERSYM => v.versym = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(),
                SHT_VERNEED => walk_chain(data, 0, info as usize, 12, |off| {
                    let cnt = rd16(data, off + 2)?;
                    let aux = rd32(data, off + 8)?;
                    walk_chain(data, off + aux as usize, usize::from(cnt), 12, |a| {
                        let other = rd16(data, a + 6)?;
                        let name = rd32(data, a + 8)?;
                        v.names.push((other, cstr_lossy(strtab, name as usize), true));
                        Some(())
                    });
                    Some(())
                }),
                SHT_VERDEF => {
                    v.has_verdef = true;
                    walk_chain(data, 0, info as usize, 16, |off| {
                        let ndx = rd16(data, off + 4)?;
                        let aux = rd32(data, off + 12)?;
                        if let Some(name) = rd32(data, off + aux as usize) {
                            v.names.push((ndx, cstr_lossy(strtab, name as usize), false));
                        }
                        Some(())
                    });
                }
                _ => {}
            }
        }
        v
    }

    /// Versão do símbolo `n`: (índice, nome, vem de verneed, oculta). `None` se não houver versão.
    pub fn of(&self, n: usize) -> Option<(u16, &str, bool, bool)> {
        let vs = *self.versym.get(n)?;
        let idx = vs & 0x7fff;
        if idx < 2 {
            return None;
        }
        let (_, name, need) = self.names.iter().find(|(k, _, _)| *k == idx)?;
        Some((idx, name, *need, vs & 0x8000 != 0))
    }
}
