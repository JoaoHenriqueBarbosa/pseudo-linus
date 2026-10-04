//! Bloco de cabeçalho de 512 bytes dos formatos de tar (v7, ustar, gnu, oldgnu, pax), escrito a partir
//! do POSIX (ustar e pax) e do apêndice de formato do manual do GNU tar, e conferido byte a byte contra o
//! GNU tar 1.35 do oráculo.
//!
//! Disposição dos campos (deslocamento, tamanho):
//! name 0/100, mode 100/8, uid 108/8, gid 116/8, size 124/12, mtime 136/12, chksum 148/8,
//! typeflag 156/1, linkname 157/100, magic 257/6, version 263/2, uname 265/32, gname 297/32,
//! devmajor 329/8, devminor 337/8, e então prefix 345/155 (ustar) ou, no gnu/oldgnu, atime 345/12,
//! ctime 357/12, offset 369/12, longnames 381/4, sparse 386/4x24, isextended 482/1, realsize 483/12.

pub const BLOCK: usize = 512;

pub const NAME: (usize, usize) = (0, 100);
pub const MODE: (usize, usize) = (100, 8);
pub const UID: (usize, usize) = (108, 8);
pub const GID: (usize, usize) = (116, 8);
pub const SIZE: (usize, usize) = (124, 12);
pub const MTIME: (usize, usize) = (136, 12);
pub const CHKSUM: (usize, usize) = (148, 8);
pub const TYPEFLAG: usize = 156;
pub const LINKNAME: (usize, usize) = (157, 100);
pub const MAGIC: (usize, usize) = (257, 6);
pub const VERSION: (usize, usize) = (263, 2);
pub const UNAME: (usize, usize) = (265, 32);
pub const GNAME: (usize, usize) = (297, 32);
pub const DEVMAJOR: (usize, usize) = (329, 8);
pub const DEVMINOR: (usize, usize) = (337, 8);
pub const PREFIX: (usize, usize) = (345, 155);
pub const GNU_ATIME: (usize, usize) = (345, 12);
pub const GNU_CTIME: (usize, usize) = (357, 12);
pub const GNU_OFFSET: (usize, usize) = (369, 12);
pub const GNU_SPARSE: usize = 386;
pub const GNU_ISEXTENDED: usize = 482;
pub const GNU_REALSIZE: (usize, usize) = (483, 12);

/// Tipos de entrada (`typeflag`).
pub mod kind {
    pub const REG: u8 = b'0';
    pub const AREG: u8 = 0;
    pub const LNK: u8 = b'1';
    pub const SYM: u8 = b'2';
    pub const CHR: u8 = b'3';
    pub const BLK: u8 = b'4';
    pub const DIR: u8 = b'5';
    pub const FIFO: u8 = b'6';
    pub const CONT: u8 = b'7';
    /// Cabeçalho estendido pax de um membro.
    pub const XHD: u8 = b'x';
    /// Cabeçalho estendido pax global.
    pub const XGL: u8 = b'g';
    /// GNU: nome longo do próximo membro.
    pub const GNU_LONGNAME: u8 = b'L';
    /// GNU: alvo longo do próximo membro.
    pub const GNU_LONGLINK: u8 = b'K';
    /// GNU: arquivo esparso no formato antigo.
    pub const GNU_SPARSE: u8 = b'S';
    /// GNU: rótulo de volume.
    pub const GNU_VOLHDR: u8 = b'V';
    /// GNU: continuação de multivolume.
    pub const GNU_MULTIVOL: u8 = b'M';
    /// GNU: lista de diretório (incremental).
    pub const GNU_DUMPDIR: u8 = b'D';
    /// Solaris: cabeçalho estendido.
    pub const SOLARIS_XHD: u8 = b'X';
}

/// Formato de arquivo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    V7,
    OldGnu,
    Gnu,
    Ustar,
    Pax,
}

impl Format {
    pub fn parse(s: &[u8]) -> Option<Format> {
        Some(match s {
            b"v7" => Format::V7,
            b"oldgnu" => Format::OldGnu,
            b"gnu" => Format::Gnu,
            b"ustar" => Format::Ustar,
            b"pax" | b"posix" => Format::Pax,
            _ => return None,
        })
    }

    /// Formato com campos numéricos em base 256 quando não cabem em octal.
    pub fn allows_base256(self) -> bool {
        matches!(self, Format::Gnu | Format::OldGnu)
    }
}

pub type Block = [u8; BLOCK];

pub fn zero_block() -> Block {
    [0u8; BLOCK]
}

pub fn is_zero(b: &Block) -> bool {
    b.iter().all(|&c| c == 0)
}

/// Bytes do campo até o primeiro NUL.
pub fn field_str(b: &Block, (off, len): (usize, usize)) -> &[u8] {
    let f = &b[off..off + len];
    match f.iter().position(|&c| c == 0) {
        Some(p) => &f[..p],
        None => f,
    }
}

/// Grava bytes num campo (truncando no tamanho), sem NUL obrigatório no fim.
pub fn put_bytes(b: &mut Block, (off, len): (usize, usize), data: &[u8]) {
    let n = data.len().min(len);
    b[off..off + n].copy_from_slice(&data[..n]);
}

/// Valor numérico de um campo: octal (com espaços ou NUL nas pontas) ou base 256 (bit alto no primeiro
/// byte, como o GNU grava). `None` quando o campo não é número.
pub fn parse_number(b: &Block, (off, len): (usize, usize)) -> Option<i128> {
    let f = &b[off..off + len];
    if f[0] & 0x80 != 0 {
        // Base 256, complemento de dois: 0x80 positivo, 0xff negativo.
        let negative = f[0] & 0x40 != 0;
        let mut v: i128 = (f[0] & 0x3f) as i128;
        if negative {
            v -= 0x40;
        }
        for &c in &f[1..] {
            v = v.checked_mul(256)?.checked_add(c as i128)?;
        }
        return Some(v);
    }
    let mut i = 0;
    while i < f.len() && (f[i] == b' ' || f[i] == 0) {
        if f[i] == 0 && f[i..].iter().all(|&c| c == 0) {
            return Some(0);
        }
        i += 1;
    }
    let start = i;
    let mut v: i128 = 0;
    while i < f.len() && (b'0'..=b'7').contains(&f[i]) {
        v = v.checked_mul(8)?.checked_add((f[i] - b'0') as i128)?;
        i += 1;
    }
    if i == start {
        return if f[start..].iter().all(|&c| c == 0 || c == b' ') { Some(0) } else { None };
    }
    if f[i..].iter().all(|&c| c == 0 || c == b' ') { Some(v) } else { None }
}

/// Grava um número em octal com zeros à esquerda e NUL no fim, como o GNU (`len - 1` dígitos).
/// Devolve `false` se não couber.
pub fn put_octal(b: &mut Block, (off, len): (usize, usize), v: u64) -> bool {
    let digits = len - 1;
    let s = format!("{v:0digits$o}");
    if s.len() > digits {
        return false;
    }
    b[off..off + digits].copy_from_slice(s.as_bytes());
    b[off + digits] = 0;
    true
}

/// Grava em base 256 (o GNU usa quando o octal não cabe): primeiro byte 0x80 e o valor em big-endian.
pub fn put_base256(b: &mut Block, (off, len): (usize, usize), v: i128) {
    let mut x = v;
    for i in (0..len).rev() {
        b[off + i] = (x & 0xff) as u8;
        x >>= 8;
    }
    if v < 0 {
        b[off] |= 0xc0;
    } else {
        b[off] = 0x80;
    }
}

/// Maior valor que cabe em octal num campo de `len` bytes.
pub fn octal_max(len: usize) -> u64 {
    let digits = (len - 1) as u32;
    if digits * 3 >= 64 { u64::MAX } else { (1u64 << (digits * 3)) - 1 }
}

/// Soma de verificação: todos os bytes, com o campo chksum contado como espaços.
pub fn checksum(b: &Block) -> (u64, i64) {
    let mut unsigned: u64 = 0;
    let mut signed: i64 = 0;
    for (i, &c) in b.iter().enumerate() {
        let c = if (CHKSUM.0..CHKSUM.0 + CHKSUM.1).contains(&i) { b' ' } else { c };
        unsigned += c as u64;
        signed += (c as i8) as i64;
    }
    (unsigned, signed)
}

/// Grava a soma no formato do GNU: seis dígitos octais, NUL e espaço.
pub fn put_checksum(b: &mut Block) {
    let (sum, _) = checksum(b);
    let s = format!("{sum:06o}");
    b[CHKSUM.0..CHKSUM.0 + 6].copy_from_slice(s.as_bytes());
    b[CHKSUM.0 + 6] = 0;
    b[CHKSUM.0 + 7] = b' ';
}

/// O cabeçalho confere (soma sem sinal ou com sinal, como os leitores tolerantes aceitam).
pub fn checksum_ok(b: &Block) -> bool {
    let Some(stored) = parse_number(b, CHKSUM) else { return false };
    let (u, s) = checksum(b);
    stored == u as i128 || stored == s as i128
}

/// Tipo de cabeçalho pela magia.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Magic {
    /// `ustar\0` + `00`.
    Ustar,
    /// `ustar  \0` (gnu e oldgnu).
    Gnu,
    /// Sem magia (v7).
    None,
}

pub fn magic(b: &Block) -> Magic {
    let m = &b[MAGIC.0..MAGIC.0 + 8];
    if m == b"ustar  \0" {
        Magic::Gnu
    } else if &m[..6] == b"ustar\0" || &m[..5] == b"ustar" {
        Magic::Ustar
    } else {
        Magic::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octal_and_base256_roundtrip() {
        let mut b = zero_block();
        assert!(put_octal(&mut b, SIZE, 3));
        assert_eq!(&b[124..136], b"00000000003\0");
        assert_eq!(parse_number(&b, SIZE), Some(3));
        assert!(!put_octal(&mut b, UID, 1 << 21));
        put_base256(&mut b, UID, 1 << 21);
        assert_eq!(b[108], 0x80);
        assert_eq!(parse_number(&b, UID), Some(1 << 21));
        put_base256(&mut b, MTIME, -1);
        assert_eq!(parse_number(&b, MTIME), Some(-1));
    }

    #[test]
    fn checksum_like_gnu() {
        let mut b = zero_block();
        put_bytes(&mut b, NAME, b"d/");
        put_octal(&mut b, MODE, 0o755);
        put_octal(&mut b, UID, 0);
        put_octal(&mut b, GID, 0);
        put_octal(&mut b, SIZE, 0);
        put_octal(&mut b, MTIME, 1_768_478_400);
        b[TYPEFLAG] = kind::DIR;
        put_bytes(&mut b, (257, 8), b"ustar  \0");
        put_bytes(&mut b, UNAME, b"root");
        put_bytes(&mut b, GNAME, b"root");
        put_checksum(&mut b);
        // Valor observado no GNU tar 1.35 pra este mesmo cabeçalho.
        assert_eq!(&b[148..156], b"007770\0 ");
        assert!(checksum_ok(&b));
        assert_eq!(magic(&b), Magic::Gnu);
    }
}
