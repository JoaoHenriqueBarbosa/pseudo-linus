//! O modo zipinfo (zipinfo.c): o registro de fim, os cabeçalhos do diretório central em vários
//! formatos e os totais.

use ul_common::ctype::cstr;

use super::{Uz, PK_ERR, PK_OK, PK_WARN};

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Abreviação do sistema de origem no formato curto (`os[]` do zi_short).
const OS_SHORT: [&str; 32] = [
    "fat", "ami", "vms", "unx", "cms", "atr", "hpf", "mac", "zzz", "cpm", "t20", "ntf", "qds", "aco", "vft", "mvs", "be ", "nsk",
    "ths", "osx", "???", "???", "???", "???", "???", "???", "???", "???", "???", "???", "ath", "???",
];

/// Método no formato curto, na ordem de `COMPR_IDS`.
const METHOD_SHORT: [&str; 18] =
    ["stor", "shrk", "re:1", "re:2", "re:3", "re:4", "i#:#", "tokn", "def#", "d64#", "dcli", "bzp2", "lzma", "ters", "lz77", "wavp", "ppmd", "u###"];

/// Sistema de origem no formato detalhado (`os[]` do zi_long); `None` são os números sem nome.
const OS_LONG: [Option<&str>; 31] = [
    Some("MS-DOS, OS/2 or NT FAT"),
    Some("Amiga"),
    Some("VMS"),
    Some("Unix"),
    Some("VM/CMS"),
    Some("Atari ST"),
    Some("OS/2 or NT HPFS"),
    Some("Macintosh HFS"),
    Some("Z-System"),
    Some("CP/M"),
    Some("TOPS-20"),
    Some("NTFS"),
    Some("SMS/QDOS"),
    Some("Acorn RISC OS"),
    Some("Win32 VFAT"),
    Some("MVS"),
    Some("BeOS"),
    Some("Tandem NSK"),
    Some("Theos"),
    Some("Mac OS/X (Darwin)"),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    Some("AtheOS"),
];

/// Método no formato detalhado, na ordem de `COMPR_IDS`.
const METHOD_LONG: [&str; 17] = [
    "none (stored)",
    "shrunk",
    "reduced (factor 1)",
    "reduced (factor 2)",
    "reduced (factor 3)",
    "reduced (factor 4)",
    "imploded",
    "tokenized",
    "deflated",
    "deflated (enhanced-64k)",
    "imploded (PK DCL)",
    "bzipped",
    "LZMA-ed",
    "tersed (IBM)",
    "LZ77-compressed (IBM)",
    "WavPacked",
    "PPMd-ed",
];

const DEFLATE_LONG: [&str; 4] = ["normal", "maximum", "fast", "superfast"];

/// Monta uma string no buffer de tamanho fixo do C: `sprintf` em `at` escreve o texto e um NUL, e
/// o resultado é o buffer até o primeiro NUL.
fn put(buf: &mut [u8; 16], at: usize, s: &str) {
    for (i, &b) in s.as_bytes().iter().enumerate() {
        if at + i < buf.len() {
            buf[at + i] = b;
        }
    }
    if at + s.len() < buf.len() {
        buf[at + s.len()] = 0;
    }
}

/// O nome de um tipo de bloco do campo extra, como o zipinfo o chama.
fn ef_name(id: u16) -> &'static str {
    match id {
        0x0001 => "PKWARE 64-bit sizes",
        0x0007 => "PKWARE AV",
        0x0009 => "OS/2",
        0x4c41 => "OS/2 ACL",
        0x4453 => "Security Descriptor",
        0x000c => "PKWARE VMS",
        0x4d49 => "Info-ZIP VMS",
        0x000a => "PKWARE Win32",
        0x000d => "PKWARE Unix",
        0x5855 => "old Info-ZIP Unix/OS2/NT",
        0x7855 => "Unix UID/GID (16-bit)",
        0x7875 => "Unix UID/GID (any size)",
        0x5455 => "universal time",
        0x7075 => "UTF8 path name",
        0x6375 => "UTF8 entry comment",
        0x334d => "new Info-ZIP Macintosh",
        0x07c8 => "old Info-ZIP Macintosh",
        0x2605 => "ZipIt Macintosh",
        0x2705 => "ZipIt Macintosh (short)",
        0x4704 => "VM/CMS",
        0x470f => "MVS",
        0x7441 => "AtheOS",
        0x6542 => "BeOS",
        0xfb4a => "SMS/QDOS",
        0x5356 => "AOS/VS",
        0x4341 => "Acorn SparkFS",
        0x4b46 => "Fred Kantor MD5",
        0x756e => "ASi Unix",
        0x4154 => "Tandem NSK",
        0x4d63 => "SmartZip Macintosh",
        0x6854 => "Theos",
        _ => "unknown",
    }
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes(b[..4].try_into().unwrap())
}

impl Uz {
    /// A data do membro como o zipinfo mostra (`zi_time`): do bloco "UT" (`modtime`, no fuso local
    /// ou em UTC com `gmt`) ou da data DOS; longa no `-v`, decimal com `-T`, curta nos outros.
    pub fn zi_time(&self, dos: u32, modtime: Option<i64>, gmt: bool) -> String {
        let (yr, mo, dy, hh, mm, ss) = match modtime {
            Some(t) => {
                let tz = if gmt { jiff::tz::TimeZone::UTC } else { self.tz.clone() };
                let (y, mo, d, h, mi, s) = crate::tz::civil(t, &tz);
                (y - 1900, mo, d, h, mi, s)
            }
            None => (i64::from((dos >> 25) & 0x7f) + 80, dos >> 21 & 0x0f, dos >> 16 & 0x1f, dos >> 11 & 0x1f, dos >> 5 & 0x3f, dos << 1 & 0x3e),
        };
        let month = if mo == 0 || mo > 12 { format!("{mo:03}") } else { MONTHS[mo as usize - 1].to_string() };
        if self.o.lflag > 9 {
            format!("{} {month} {dy} {hh:02}:{mm:02}:{ss:02}", yr + 1900)
        } else if self.o.t_flag {
            format!("{:04}{mo:02}{dy:02}.{hh:02}{mm:02}{ss:02}", yr + 1900)
        } else {
            format!("{:02}-{month}-{dy:02} {hh:02}:{mm:02}", yr.rem_euclid(100))
        }
    }

    /// Os 15 caracteres de atributos do formato curto, conforme o sistema de origem.
    fn short_attribs(&self) -> String {
        use super::process::{ACORN, AMIGA, FS_FAT, FS_HPFS, FS_NTFS, MVS, THEOS, VM_CMS, VMS};
        let hostnum = self.pinfo.hostnum;
        let hostver = u32::from(self.pinfo.hostver);
        let ver = format!("{}.{}", hostver / 10, hostver % 10);
        let ext = self.crec.external_file_attributes;
        let xattr = ext >> 16 & 0xffff;
        let mut a = [b' '; 16];
        a[15] = 0;
        let bit = |m: u32, yes: u8, no: u8| if xattr & m != 0 { yes } else { no };
        match hostnum {
            VMS => {
                let mut ws = [0u8; 12];
                let groups = [(0o400, 0o200, 0o100), (0o040, 0o020, 0o010), (0o004, 0o002, 0o001)];
                for (g, &(r, w, x)) in groups.iter().enumerate() {
                    if xattr & r != 0 {
                        ws[g * 4] = b'R';
                    }
                    if xattr & w != 0 {
                        ws[g * 4 + 1] = b'W';
                        ws[g * 4 + 3] = b'D';
                    }
                    if xattr & x != 0 {
                        ws[g * 4 + 2] = b'E';
                    }
                }
                let mut p = 0;
                for g in 0..3 {
                    for &c in ws[g * 4..g * 4 + 4].iter().filter(|&&c| c != 0) {
                        a[p] = c;
                        p += 1;
                    }
                    a[p] = b',';
                    p += 1;
                }
                p -= 1;
                a[p] = b' ';
                if p < 12 {
                    put(&mut a, 12, &ver);
                }
            }
            AMIGA => {
                a[0] = match xattr & 0o6000 {
                    0o4000 => b'd',
                    0o2000 => b'-',
                    _ => b'?',
                };
                let flags = [(0o200, b'h'), (0o100, b's'), (0o040, b'p'), (0o020, b'a'), (0o010, b'r'), (0o004, b'w'), (0o002, b'e'), (0o001, b'd')];
                for (i, &(m, c)) in flags.iter().enumerate() {
                    a[i + 1] = bit(m, c, b'-');
                }
                put(&mut a, 12, &ver);
            }
            THEOS => {
                a[0] = match xattr & 0xF000 {
                    0x5000 => b'L',
                    0x4000 => b'D',
                    0x2000 => b'C',
                    0x8000 => b'S',
                    0x9000 => b'R',
                    0xA000 => b'K',
                    0xB000 => b'I',
                    0xD000 => b'P',
                    0xE000 => b'2',
                    0xF000 => b'3',
                    _ => b'?',
                };
                let flags = [(0x0400, b'H'), (0x0800, b'M'), (0x0002, b'W'), (0x0004, b'R'), (0x0200, b'E'), (0x0040, b'X'), (0x0080, b'W'), (0x0100, b'R')];
                for (i, &(m, c)) in flags.iter().enumerate() {
                    a[i + 1] = bit(m, b'.', c);
                }
                put(&mut a, 12, &ver);
            }
            FS_FAT | FS_HPFS | FS_NTFS | VM_CMS | MVS | ACORN | super::process::FS_VFAT
                if hostnum != FS_FAT || xattr & 0o700 != (0o400 | (u32::from(ext & 1 == 0) << 7) | ((ext & 0x10) << 2)) =>
            {
                let x = ext & 0xff;
                put(&mut a, 0, &format!(".r.-...     {ver}"));
                a[2] = if x & 0x01 != 0 { b'-' } else { b'w' };
                a[5] = if x & 0x02 != 0 { b'h' } else { b'-' };
                a[6] = if x & 0x04 != 0 { b's' } else { b'-' };
                a[4] = if x & 0x20 != 0 { b'a' } else { b'-' };
                if x & 0x10 != 0 {
                    a[0] = b'd';
                    a[3] = b'x';
                } else {
                    a[0] = b'-';
                }
                if x & 0x08 != 0 {
                    a[0] = b'V';
                } else if let Some(dot) = self.filename.iter().rposition(|&c| c == b'.') {
                    let e = &self.filename[dot + 1..];
                    let e = &e[..e.len().min(3)];
                    if [b"com", b"exe", b"btm", b"cmd", b"bat"].iter().any(|s| e.eq_ignore_ascii_case(*s)) {
                        a[3] = b'x';
                    }
                }
            }
            _ => {
                a[0] = match xattr & 0o170000 {
                    0o040000 => b'd',
                    0o100000 => b'-',
                    0o120000 => b'l',
                    0o060000 => b'b',
                    0o020000 => b'c',
                    0o010000 => b'p',
                    0o140000 => b's',
                    _ => b'?',
                };
                a[1] = bit(0o400, b'r', b'-');
                a[4] = bit(0o040, b'r', b'-');
                a[7] = bit(0o004, b'r', b'-');
                a[2] = bit(0o200, b'w', b'-');
                a[5] = bit(0o020, b'w', b'-');
                a[8] = bit(0o002, b'w', b'-');
                let special = |x: u32, s: u32, lo: u8, up: u8, plain: u8| match (xattr & x != 0, xattr & s != 0) {
                    (true, true) => lo,
                    (true, false) => plain,
                    (false, true) => up,
                    (false, false) => b'-',
                };
                a[3] = special(0o100, 0o4000, b's', b'S', b'x');
                a[6] = special(0o010, 0o2000, b's', b'S', b'x');
                a[9] = special(0o001, 0o1000, b't', b'T', b'x');
                put(&mut a, 11, &format!("{:2}.{}", hostver / 10, hostver % 10));
            }
        }
        String::from_utf8_lossy(cstr(&a)).into_owned()
    }

    /// Uma linha do formato curto (`-s`, `-m`, `-l`): atributos, sistema, tamanho, texto ou
    /// binário, campo extra, fator ou tamanho comprimido, método, data e nome (`zi_short`).
    fn zi_short(&mut self) -> i32 {
        let c = self.crec.clone();
        let methnum = super::list::find_compr_idx(c.compression_method);
        let mut meth = METHOD_SHORT[methnum].as_bytes().to_vec();
        match c.compression_method {
            6 => {
                meth[1] = if c.general_purpose_bit_flag & 2 != 0 { b'8' } else { b'4' };
                meth[3] = if c.general_purpose_bit_flag & 4 != 0 { b'3' } else { b'2' };
            }
            8 | 9 => meth[3] = b"NXFS"[usize::from((c.general_purpose_bit_flag >> 1) & 3)],
            m if methnum >= super::list::COMPR_IDS.len() => {
                meth = if m <= 999 { format!("u{m:03}") } else { format!("{m:04X}") }.into_bytes();
            }
            _ => {}
        }
        let hostnum = self.pinfo.hostnum;
        let mut line = format!("{} {} {:8} ", self.short_attribs(), OS_SHORT[usize::from(hostnum)], c.ucsize).into_bytes();
        let text = c.internal_file_attributes & 1 != 0;
        line.push(match (c.general_purpose_bit_flag & 1 != 0, text) {
            (true, true) => b'T',
            (true, false) => b'B',
            (false, true) => b't',
            (false, false) => b'b',
        });
        use super::process::{FS_HPFS, FS_NTFS, UNIX};
        let has_extra = c.extra_field_length != 0 || (c.external_file_attributes & 0x8000 != 0 && matches!(hostnum, UNIX | FS_HPFS | FS_NTFS));
        let ext_hdr = c.general_purpose_bit_flag & 8 != 0;
        line.push(match (has_extra, ext_hdr) {
            (true, true) => b'X',
            (true, false) => b'x',
            (false, true) => b'l',
            (false, false) => b'-',
        });
        if self.o.lflag == 4 {
            let csiz = if c.general_purpose_bit_flag & 1 != 0 { c.csize.wrapping_sub(12) } else { c.csize };
            line.extend_from_slice(format!("{:3}%", (super::list::ratio(c.ucsize, csiz) + 5) / 10).as_bytes());
        } else if self.o.lflag == 5 {
            line.extend_from_slice(format!(" {:8}", c.csize).as_bytes());
        }
        let modtime = self.central_mtime();
        let when = self.zi_time(c.last_mod_dos_datetime, modtime, false);
        line.extend_from_slice(b" ");
        line.extend_from_slice(&meth);
        line.extend_from_slice(format!(" {when} ").as_bytes());
        self.info(0, line);
        self.fnprint();
        self.skip_comment()
    }

    /// O mtime do bloco "UT" do campo extra central, se há.
    fn central_mtime(&self) -> Option<i64> {
        let ef = self.extra_field.as_ref()?;
        let (flags, t, _) = super::process::ef_scan_for_izux(ef, true, self.crec.last_mod_dos_datetime, true, false);
        (flags & super::process::EB_UT_FL_MTIME != 0).then_some(t.mtime)
    }

    /// Pula o comentário do membro (o `SKIP_` do C).
    fn skip_comment(&mut self) -> i32 {
        self.skip_string(usize::from(self.crec.file_comment_length))
    }

    /// Um membro no formato detalhado (`-v`): tudo do cabeçalho central, os atributos, cada bloco
    /// do campo extra e o comentário (`zi_long`). `endprev` é o fim do membro anterior, pra avisar
    /// de bytes entre os dois.
    fn zi_long(&mut self, endprev: &mut u64, error_in_archive: i32) -> i32 {
        let c = self.crec.clone();
        if c.relative_offset_local_header != *endprev && *endprev > 0 {
            let gap = c.relative_offset_local_header.wrapping_sub(*endprev) as i64;
            self.info(0, format!("  There are an extra {gap} bytes preceding this file.\n\n"));
        }
        *endprev = c.relative_offset_local_header
            + 4
            + super::process::LREC_SIZE as u64
            + u64::from(c.filename_length)
            + u64::from(c.extra_field_length)
            + c.csize;
        let hostnum = self.pinfo.hostnum;
        let hostver = u32::from(self.pinfo.hostver);
        let extnum = c.version_needed_to_extract[1].min(super::process::NUM_HOSTS);
        let extver = u32::from(c.version_needed_to_extract[0]);
        let methnum = super::list::find_compr_idx(c.compression_method);
        self.info(0, "  ");
        self.fnprint();
        let off = c.relative_offset_local_header;
        self.info(0, format!("\n  offset of local header from start of archive:   {off}\n                                                  ({off:016X}h) bytes\n"));
        // Origem sem nome na tabela sai "(null)" (o printf da glibc com `%s` nulo); só o 31 (o
        // limite) vira "unknown". No mínimo requerido, os dois viram "unknown".
        let host = match OS_LONG.get(usize::from(hostnum)) {
            Some(Some(s)) => s.to_string(),
            Some(None) => "(null)".to_string(),
            None => format!("unknown ({})", c.version_made_by[1]),
        };
        let ext = match OS_LONG.get(usize::from(extnum)).copied().flatten() {
            Some(s) => s.to_string(),
            None => format!("unknown ({})", c.version_needed_to_extract[1]),
        };
        self.info(0, format!("  file system or operating system of origin:      {host}\n"));
        self.info(0, format!("  version of encoding software:                   {}.{}\n", hostver / 10, hostver % 10));
        self.info(0, format!("  minimum file system compatibility required:     {ext}\n"));
        self.info(0, format!("  minimum software version required to extract:   {}.{}\n", extver / 10, extver % 10));
        let method = METHOD_LONG.get(methnum).map_or_else(|| format!("unknown ({})", c.compression_method), |s| s.to_string());
        self.info(0, format!("  compression method:                             {method}\n"));
        let gp = c.general_purpose_bit_flag;
        match c.compression_method {
            6 => {
                self.info(0, format!("  size of sliding dictionary (implosion):         {}K\n", if gp & 2 != 0 { '8' } else { '4' }));
                self.info(0, format!("  number of Shannon-Fano trees (implosion):       {}\n", if gp & 4 != 0 { '3' } else { '2' }));
            }
            8 | 9 => {
                self.info(0, format!("  compression sub-type (deflation):               {}\n", DEFLATE_LONG[usize::from((gp >> 1) & 3)]));
            }
            _ => {}
        }
        self.info(0, format!("  file security status:                           {}encrypted\n", if gp & 1 != 0 { "" } else { "not " }));
        self.info(0, format!("  extended local header:                          {}\n", if gp & 8 != 0 { "yes" } else { "no" }));
        let dos = c.last_mod_dos_datetime;
        self.info(0, format!("  file last modified on (DOS date/time):          {}\n", self.zi_time(dos, None, false)));
        if let Some(mtime) = self.central_mtime() {
            self.info(0, format!("  file last modified on (UT extra field modtime): {} local\n", self.zi_time(dos, Some(mtime), false)));
            self.info(0, format!("  file last modified on (UT extra field modtime): {} UTC\n", self.zi_time(dos, Some(mtime), true)));
        }
        self.info(0, format!("  32-bit CRC value (hex):                         {:08x}\n", c.crc32));
        self.info(0, format!("  compressed size:                                {} bytes\n", c.csize));
        self.info(0, format!("  uncompressed size:                              {} bytes\n", c.ucsize));
        self.info(0, format!("  length of filename:                             {} characters\n", c.filename_length));
        self.info(0, format!("  length of extra field:                          {} bytes\n", c.extra_field_length));
        self.info(0, format!("  length of file comment:                         {} characters\n", c.file_comment_length));
        self.info(0, format!("  disk number on which file begins:               disk {}\n", c.disk_number_start + 1));
        let ftype = if c.internal_file_attributes & 1 != 0 {
            "text"
        } else if c.internal_file_attributes & 2 != 0 {
            "ebcdic"
        } else {
            "binary"
        };
        self.info(0, format!("  apparent file type:                             {ftype}\n"));
        self.long_attribs();
        let error = self.long_extra_field(endprev, error_in_archive);
        if error != PK_OK {
            return error;
        }
        self.long_tail(endprev, error_in_archive)
    }

    /// As linhas de atributos do formato detalhado: as do sistema de origem e as do MS-DOS.
    fn long_attribs(&mut self) {
        use super::process::{ACORN, AMIGA, FS_FAT, FS_HPFS, FS_NTFS, FS_VFAT, MVS, THEOS, VM_CMS, VMS};
        let ext = self.crec.external_file_attributes;
        let xattr = ext >> 16 & 0xffff;
        let bit = |m: u32, yes: char, no: char| if xattr & m != 0 { yes } else { no };
        match self.pinfo.hostnum {
            VMS => {
                let group = |r: u32, w: u32, x: u32| {
                    let mut g = String::new();
                    if xattr & r != 0 {
                        g.push('R');
                    }
                    if xattr & w != 0 {
                        g.push('W');
                    }
                    if xattr & x != 0 {
                        g.push('E');
                    }
                    if xattr & w != 0 {
                        g.push('D');
                    }
                    g
                };
                let owner = group(0o400, 0o200, 0o100);
                // Sistema e dono têm as mesmas permissões.
                let s = format!("({owner},{owner},{},{})", group(0o040, 0o020, 0o010), group(0o004, 0o002, 0o001));
                self.info(0, format!("  VMS file attributes ({xattr:06o} octal):             {s}\n"));
            }
            AMIGA => {
                let t = match xattr & 0o6000 {
                    0o4000 => 'd',
                    0o2000 => '-',
                    _ => '?',
                };
                let flags = [(0o200, 'h'), (0o100, 's'), (0o040, 'p'), (0o020, 'a'), (0o010, 'r'), (0o004, 'w'), (0o002, 'e'), (0o001, 'd')];
                let s: String = std::iter::once(t).chain(flags.iter().map(|&(m, ch)| bit(m, ch, '-'))).collect();
                self.info(0, format!("  Amiga file attributes ({xattr:06o} octal):           {s}\n"));
            }
            THEOS => {
                let t = match xattr & 0xF000 {
                    0x5000 => "Library     ",
                    0x4000 => "Directory   ",
                    0x8000 => "Sequential  ",
                    0x9000 => "Direct      ",
                    0xA000 => "Keyed       ",
                    0xB000 => "Indexed     ",
                    0xD000 => " 86 program ",
                    0xE000 => "286 program ",
                    0xF000 => "386 program ",
                    _ => "???         ",
                };
                let flags = [(0x0400, 'H'), (0x0800, 'M'), (0x0002, 'W'), (0x0004, 'R'), (0x0200, 'E'), (0x0040, 'X'), (0x0080, 'W'), (0x0100, 'R')];
                let s: String = t.chars().chain(flags.iter().map(|&(m, ch)| bit(m, '.', ch))).collect();
                self.info(0, format!("  Theos file attributes ({xattr:04X} hex):               {s}\n"));
            }
            FS_FAT | FS_HPFS | FS_NTFS | FS_VFAT | ACORN | VM_CMS | MVS => {
                self.info(0, format!("  non-MSDOS external file attributes:             {:06X} hex\n", ext >> 8));
            }
            _ => {
                let t = match xattr & 0o170000 {
                    0o040000 => 'd',
                    0o100000 => '-',
                    0o120000 => 'l',
                    0o060000 => 'b',
                    0o020000 => 'c',
                    0o010000 => 'p',
                    0o140000 => 's',
                    _ => '?',
                };
                let special = |x: u32, s: u32, lo: char, up: char| match (xattr & x != 0, xattr & s != 0) {
                    (true, true) => lo,
                    (true, false) => 'x',
                    (false, true) => up,
                    (false, false) => '-',
                };
                let s: String = [
                    t,
                    bit(0o400, 'r', '-'),
                    bit(0o200, 'w', '-'),
                    special(0o100, 0o4000, 's', 'S'),
                    bit(0o040, 'r', '-'),
                    bit(0o020, 'w', '-'),
                    // No formato detalhado o setgid sem execução é "l" (trava obrigatória).
                    special(0o010, 0o2000, 's', 'l'),
                    bit(0o004, 'r', '-'),
                    bit(0o002, 'w', '-'),
                    special(0o001, 0o1000, 't', 'T'),
                ]
                .iter()
                .collect();
                self.info(0, format!("  Unix file attributes ({xattr:06o} octal):            {s}\n"));
            }
        }
        let x = ext & 0xff;
        let line = match x {
            0 => format!("  MS-DOS file attributes ({x:02X} hex):                none\n"),
            1 => format!("  MS-DOS file attributes ({x:02X} hex):                read-only\n"),
            _ => {
                let names = ["rdo ", "hid ", "sys ", "lab ", "dir ", "arc ", "lnk ", "exe"];
                let s: String = names.iter().enumerate().filter(|(i, _)| x & (1 << i) != 0).map(|(_, n)| *n).collect();
                format!("  MS-DOS file attributes ({x:02X} hex):                {s}\n")
            }
        };
        self.info(0, line);
    }

    /// Tipo e código de criador de um arquivo Macintosh (`zi_showMacTypeCreator`).
    fn mac_type_creator(&mut self, b: &[u8]) {
        if b.len() < 8 {
            return;
        }
        if b[..8].iter().all(|&c| (0x20..0x7f).contains(&c)) {
            let s = |r: &[u8]| String::from_utf8_lossy(r).into_owned();
            self.info(0, format!(".\n    The associated file has type code `{}' and creator code `{}'", s(&b[..4]), s(&b[4..8])));
        } else {
            let t = u32::from_be_bytes(b[..4].try_into().unwrap());
            let c = u32::from_be_bytes(b[4..8].try_into().unwrap());
            self.info(0, format!(".\n    The associated file has type code `0x{t:x}' and creator code `0x{c:x}'"));
        }
    }

    /// Os bytes do bloco em hexadecimal: todos até 24, senão os 20 primeiros.
    fn ef_hex_dump(&mut self, d: &[u8]) {
        if d.is_empty() {
            return;
        }
        let (head, n) = if d.len() <= 24 { (":\n   ", d.len()) } else { (".  The first\n    20 are:  ", 20) };
        let mut s = head.to_string();
        for b in &d[..n] {
            s.push_str(&format!(" {b:02x}"));
        }
        self.info(0, s);
    }

    /// O que o zipinfo sabe dizer de cada tipo de bloco; o que não reconhece vai em hexadecimal.
    fn ef_details(&mut self, id: u16, d: &[u8], endprev: &mut u64) {
        if !self.ef_known(id, d, endprev) {
            self.ef_hex_dump(d);
        }
    }

    /// Os tipos com descrição própria; `false` quando o bloco não tem o tamanho que ela pede.
    fn ef_known(&mut self, id: u16, d: &[u8], endprev: &mut u64) -> bool {
        let n = d.len();
        match id {
            0x0009 | 0x4c41 if n >= 4 => {
                let what = if id == 0x0009 {
                    ".\n    The local extra field has {} bytes of OS/2 extended attributes.\n    (May not match OS/2 \"dir\" amount due to storage method)"
                } else {
                    ".\n    The local extra field has {} bytes of access control list information"
                };
                self.info(0, what.replace("{}", &le32(d).to_string()));
                *endprev = 0;
            }
            0x4453 if n >= 4 => {
                self.info(0, format!(".\n    The local extra field has {} bytes of NT security descriptor data", le32(d)));
                *endprev = 0;
            }
            0x4d49 if n >= 8 => self.ef_izvms(d),
            0x5455 => {
                if n > 0 {
                    let names = [(1, "modification"), (2, "access"), (4, "creation")];
                    let mut types: Vec<&str> = Vec::new();
                    for (bit, name) in names {
                        if d[0] & bit != 0 {
                            types.push(name);
                            if bit != 1 && *endprev > 0 {
                                *endprev += 4;
                            }
                        }
                    }
                    if !types.is_empty() {
                        let s = if types.len() == 1 { "" } else { "s" };
                        self.info(0, format!(".\n    The local extra field has UTC/GMT {} time{s}", types.join("/")));
                    }
                }
            }
            0x7075 | 0x6375 if n >= 5 => {
                let (v, crc) = (d[0], le32(&d[1..]));
                let (head, end) = if n <= 29 {
                    (format!(".\n    The UTF8 data of the extra field (V{v}, ASCII name CRC `{crc:08x}') are:\n   "), n)
                } else {
                    (format!(". The first\n    24 UTF8 bytes in the extra field (V{v}, ASCII name CRC `{crc:08x}') are:\n   "), 29)
                };
                let mut s = head;
                for b in &d[5..end] {
                    s.push_str(&format!(" {b:02x}"));
                }
                self.info(0, s);
            }
            _ => return self.ef_known_rare(id, d, endprev),
        }
        true
    }

    /// O bloco de VMS do Info-ZIP: compressão, tamanho e qual estrutura do RMS ele guarda.
    fn ef_izvms(&mut self, d: &[u8]) {
        const COMP: [&str; 4] = ["stored", "run-length encoded", "deflated", "compressed(?)"];
        let compr = usize::from(u16::from_le_bytes([d[4], d[5]]) & 7).min(3);
        let mut q: Vec<u8> = Vec::new();
        let p = match le32(d) {
            0x4241_4656 => "FAB",
            0x4C4C_4156 => "XABALL",
            0x4348_4656 => "XABFHC",
            0x5441_4456 => "XABDAT",
            0x5444_5256 => "XABRDT",
            0x4F52_5056 => "XABPRO",
            0x5945_4B56 => "XABKEY",
            0x5653_4D56 => {
                if d.len() >= 16 {
                    // strncpy de 4 bytes seguido do ')': um NUL no meio corta o resto, inclusive o ')'.
                    let v = &d[12..16];
                    q.extend_from_slice(b" (");
                    match v.iter().position(|&c| c == 0) {
                        Some(k) => q.extend_from_slice(&v[..k]),
                        None => {
                            q.extend_from_slice(v);
                            q.push(b')');
                        }
                    }
                }
                "version"
            }
            _ => "unknown",
        };
        let ucsiz = u16::from_le_bytes([d[6], d[7]]);
        let mut msg = format!(".  The extra\n    field is {} and has {ucsiz} bytes of VMS {p} information", COMP[compr]).into_bytes();
        msg.extend_from_slice(&q);
        self.info(0, msg);
    }

    /// Os blocos de Macintosh, BeOS, QDOS, AOS/VS, Tandem e MD5.
    fn ef_known_rare(&mut self, id: u16, d: &[u8], endprev: &mut u64) -> bool {
        let n = d.len();
        match id {
            0x334d if n >= 14 => {
                let uc = le32(d);
                let flags = u16::from_le_bytes([d[4], d[5]]);
                let is_uc = flags & 0x04 != 0;
                let un = if is_uc { "un" } else { "" };
                self.info(0, format!(".\n    The local extra field has {uc} bytes of {un}compressed Macintosh\n    finder attributes"));
                self.mac_endprev(is_uc, uc, endprev);
                let fork = if flags & 0x01 != 0 { "Data-fork" } else { "Resource-fork" };
                let bits = if flags & 0x08 != 0 { 64 } else { 32 };
                self.info(0, format!(".\n    File is marked as {fork}, File Dates are in {bits} Bit"));
                self.mac_type_creator(&d[6..]);
            }
            0x2705 if n >= 5 && le32(d) == 0x5449_505A => {
                if n >= 12 {
                    self.mac_type_creator(&d[4..]);
                }
            }
            0x2605 if n >= 5 && le32(d) == 0x5449_505A => {
                let fnlen = usize::from(d[4]);
                if n >= fnlen + 13 {
                    let mut msg = b".\n    The Mac long filename is ".to_vec();
                    msg.extend_from_slice(cstr(&d[5..5 + fnlen]));
                    self.info(0, msg);
                    self.mac_type_creator(&d[fnlen + 5..]);
                }
            }
            0x07c8 if n >= 40 && le32(d) == 0x4545_4C4A => {
                self.mac_type_creator(&d[4..]);
                let fork = if d[31] & 1 != 0 { "Data-fork" } else { "Resource-fork" };
                self.info(0, format!(".\n    File is marked as {fork}"));
            }
            0x4d63 if n == 64 && le32(d) == 0x7069_5A64 => {
                self.mac_type_creator(&d[4..]);
                let len = usize::from(d[32]).min(31);
                let mut msg = b".\n    The Mac long filename is ".to_vec();
                msg.extend_from_slice(cstr(&d[33..33 + len]));
                self.info(0, msg);
            }
            0x7441 | 0x6542 if n >= 5 => {
                let uc = le32(d);
                let is_uc = d[4] & 0x01 != 0;
                let os = if id == 0x7441 { "AtheOS" } else { "BeOS" };
                let un = if is_uc { "un" } else { "" };
                self.info(0, format!(".\n    The local extra field has {uc} bytes of {un}compressed {os} file attributes"));
                self.mac_endprev(is_uc, uc, endprev);
            }
            0xfb4a if n >= 4 => {
                let mut msg = b".\n    The QDOS extra field subtype is `".to_vec();
                msg.extend_from_slice(&d[..4]);
                msg.push(b'\'');
                self.info(0, msg);
            }
            0x5356 if n >= 5 => {
                self.info(0, format!(".\n    The AOS/VS extra field revision is {}.{}", d[4] / 10, d[4] % 10));
            }
            0x4154 if n == 20 => {
                const FORMATS: [&str; 6] =
                    ["Unstructured", "Relative", "Entry Sequenced", "Key Sequenced", "Edit", "Object"];
                let mut kind = usize::from((d[18] & 0x60) >> 5);
                let code = u16::from_be_bytes([d[0], d[1]]);
                if kind == 0 {
                    match code {
                        101 => kind = 4,
                        100 => kind = 5,
                        _ => {}
                    }
                }
                self.info(0, format!(".\n    The file was originally a Tandem {} file, with file code {code}", FORMATS[kind]));
            }
            0x4b46 if n >= 19 => {
                let md5: String = (0..16).map(|i| format!("{:02x}", d[15 - i])).collect();
                self.info(0, format!(".\n    The 128-bit MD5 signature is {md5}"));
            }
            _ => return false,
        }
        true
    }

    /// Atributos sem compressão somam ao tamanho do campo extra local; comprimidos tornam o tamanho
    /// do local imprevisível.
    fn mac_endprev(&self, is_uc: bool, uc: u32, endprev: &mut u64) {
        if is_uc {
            if *endprev > 0 {
                *endprev += u64::from(uc);
            }
        } else {
            *endprev = 0;
        }
    }

    /// O campo extra local que os bits altos dos atributos externos anunciam e o comentário do
    /// membro: o fim de `zi_long`.
    fn long_tail(&mut self, endprev: &mut u64, mut error_in_archive: i32) -> i32 {
        use super::process::{FS_FAT, FS_HPFS, FS_NTFS, UNIX};
        const LOCAL: &str = "old Info-ZIP Unix/OS2/NT";
        const UID: &str = "GMT modification/access times and Unix UID/GID";
        const NOUID: &str = "GMT modification/access times only";
        let hostnum = self.pinfo.hostnum;
        let xattr = (self.crec.external_file_attributes & 0xC000) >> 12;
        if xattr & 8 != 0 {
            if hostnum == UNIX || hostnum == FS_HPFS || hostnum == FS_NTFS {
                let what = if xattr & 4 != 0 { UID } else { NOUID };
                self.info(
                    0,
                    format!("\n  There is a local extra field with ID 0x5855 ({LOCAL}) and\n  {} data bytes ({what}).\n", xattr & 12),
                );
                if *endprev > 0 {
                    *endprev += u64::from(xattr & 12);
                }
            } else if hostnum == FS_FAT && xattr & 4 == 0 {
                self.info(0, format!("\n  There may be a local extra field with ID 0x5855 ({LOCAL}) and\n  8 data bytes ({NOUID}).\n"));
            }
        }
        if self.crec.file_comment_length == 0 {
            self.info(0, "\n  There is no file comment.\n");
        } else {
            self.info(0, "\n------------------------- file comment begins ----------------------------\n");
            let error = self.display_string_8(usize::from(self.crec.file_comment_length));
            if error != PK_OK {
                error_in_archive = error;
                if error > PK_WARN {
                    return error;
                }
            }
            self.info(0, "-------------------------- file comment ends -----------------------------\n");
        }
        error_in_archive
    }

    /// Cada bloco do campo extra central, com o nome e o que se sabe dele.
    fn long_extra_field(&mut self, endprev: &mut u64, error_in_archive: i32) -> i32 {
        use super::process::UNIX;
        if self.crec.extra_field_length == 0 {
            return PK_OK;
        }
        if error_in_archive > PK_WARN {
            return error_in_archive;
        }
        let Some(ef) = self.extra_field.clone() else { return PK_ERR };
        let hostnum = self.pinfo.hostnum;
        self.info(0, "\n  The central-directory extra field contains:");
        let mut rest: &[u8] = &ef[..ef.len().min(usize::from(self.crec.extra_field_length))];
        while rest.len() >= 4 {
            let id = u16::from_le_bytes([rest[0], rest[1]]);
            let mut len = usize::from(u16::from_le_bytes([rest[2], rest[3]]));
            rest = &rest[4..];
            if len > rest.len() {
                // 0x421: no stderr, ou no stdout com -t, sem esperar o fim da linha.
                self.info(
                    super::MSG_STDERR | super::MSG_LNEWLN,
                    format!(
                        "\n  error: EF data block (type 0x{id:04x}) size {len} exceeds remaining extra field\n         space {}; block length has been truncated.\n",
                        rest.len()
                    ),
                );
                len = rest.len();
            }
            let d = &rest[..len];
            let name = ef_name(id);
            if id == 0x0001 && self.crec.relative_offset_local_header & !0xFFFF_FFFF != 0 {
                *endprev = endprev.wrapping_sub(if len == 8 { 12 } else { 8 });
            }
            if (id == 0x5855 && hostnum == UNIX || id == 0x7855) && *endprev > 0 {
                *endprev += 4;
            }
            self.info(0, format!("\n  - A subfield with ID 0x{id:04x} ({name}) and {len} data bytes"));
            self.ef_details(id, d, endprev);
            self.info(0, ".");
            rest = &rest[len..];
        }
        self.info(0, "\n");
        PK_OK
    }

    /// Percorre o diretório central no formato pedido e termina com os totais e os padrões que não
    /// casaram com nada (`zipinfo`).
    pub fn zipinfo(&mut self) -> i32 {
        use super::{MSG_STDERR, PK_BADERR, PK_EOF, PK_FIND, PK_OK, PK_WARN};
        let mut error_in_archive = PK_OK;
        let mut fn_matched = vec![false; self.pfnames.len()];
        let mut xn_matched = vec![false; self.pxnames.len()];
        let (mut members, mut tot_csize, mut tot_ucsize) = (0u64, 0u64, 0u64);
        self.o.l_flag = 0;
        self.pinfo.textmode = false;
        let mut endprev: u64 = if self.crec.relative_offset_local_header == 4 { 4 } else { 0 };
        let mut j: u64 = 1;
        loop {
            let mut sig = [0u8; 4];
            if self.readbuf(&mut sig) == 0 {
                error_in_archive = PK_EOF;
                break;
            }
            self.sig = sig;
            if &sig != super::process::CENTRAL_HDR_SIG {
                if !self.end_of_central_dir(j - 1) {
                    error_in_archive = PK_BADERR;
                }
                break;
            }
            let error = self.process_cdir_file_hdr();
            if error != PK_OK {
                error_in_archive = error;
                break;
            }
            let error = self.read_filename(usize::from(self.crec.filename_length), false);
            if error != PK_OK {
                error_in_archive = error_in_archive.max(error);
                if error > PK_WARN {
                    break;
                }
            }
            let mut selected = true;
            if !self.process_all_files {
                let sepc = self.o.w_flag.then_some(b'/');
                let m = |p: &Vec<u8>| super::matching::matches(&self.filename, p, self.o.c_flag, sepc);
                if !self.pfnames.is_empty() {
                    selected = match self.pfnames.iter().position(m) {
                        Some(i) => {
                            fn_matched[i] = true;
                            true
                        }
                        None => false,
                    };
                }
                if selected
                    && let Some(i) = self.pxnames.iter().position(m) {
                        xn_matched[i] = true;
                        selected = false;
                    }
            }
            if !selected {
                for len in [self.crec.extra_field_length, self.crec.file_comment_length] {
                    let error = self.skip_string(usize::from(len));
                    if error != PK_OK {
                        error_in_archive = error;
                        if error > 1 {
                            return error;
                        }
                    }
                }
                endprev = 0;
                j += 1;
                continue;
            }
            // Como no C, `error` guarda o último do_string que rodou: o SKIP_ de um comentário vazio
            // não mexe nela, e é ela que decide o `break` depois do switch.
            let mut error = self.read_extra_field(usize::from(self.crec.extra_field_length));
            if error != PK_OK {
                self.extra_field = None;
                error_in_archive = error;
            }
            let comment_len = usize::from(self.crec.file_comment_length);
            match self.o.lflag {
                3..=5 => {
                    error = self.zi_short();
                    if error != PK_OK {
                        error_in_archive = error;
                    }
                }
                10 => {
                    self.info(0, format!("\nCentral directory entry #{j}:\n---------------------------\n\n"));
                    error = self.zi_long(&mut endprev, error_in_archive);
                    if error != PK_OK {
                        error_in_archive = error;
                    }
                }
                lflag => {
                    if lflag == 1 || lflag == 2 {
                        self.fnprint();
                    }
                    if comment_len != 0 {
                        error = self.skip_string(comment_len);
                        if error != PK_OK {
                            error_in_archive = error;
                            if error > 1 {
                                return error;
                            }
                        }
                    }
                }
            }
            if error > PK_WARN {
                break;
            }
            tot_csize = tot_csize.wrapping_add(self.crec.csize);
            tot_ucsize = tot_ucsize.wrapping_add(self.crec.ucsize);
            if self.crec.general_purpose_bit_flag & 1 != 0 {
                tot_csize = tot_csize.wrapping_sub(12);
            }
            members += 1;
            j += 1;
        }
        if error_in_archive <= PK_WARN && self.o.tflag != 0 {
            let r = super::list::ratio(tot_ucsize, tot_csize);
            let (sgn, r) = if r < 0 { ("-", -r) } else { ("", r) };
            let s = if members == 1 { "" } else { "s" };
            self.info(
                0,
                format!("{members} file{s}, {tot_ucsize} bytes uncompressed, {tot_csize} bytes compressed:  {sgn}{}.{}%\n", r / 10, r % 10),
            );
        }
        if error_in_archive <= PK_WARN {
            for (i, hit) in fn_matched.iter().enumerate() {
                if !hit {
                    let msg = [super::text::FILENAME_NOT_MATCHED.as_bytes(), &self.pfnames[i], b"\n"].concat();
                    self.info(MSG_STDERR, msg);
                }
            }
            for (i, hit) in xn_matched.iter().enumerate() {
                if !hit {
                    let msg = [super::text::EXCL_FILENAME_NOT_MATCHED.as_bytes(), &self.pxnames[i], b"\n"].concat();
                    self.info(MSG_STDERR, msg);
                }
            }
            if !self.at_end_sig() {
                self.info(MSG_STDERR, "\nnote:  didn't find end-of-central-dir signature at end of central dir.\n");
                error_in_archive = PK_WARN;
            }
            if members == 0 && error_in_archive <= PK_WARN {
                error_in_archive = PK_FIND;
            }
            if self.o.lflag >= 10 {
                self.info(0, "\n");
            }
        }
        error_in_archive
    }
    /// O que o zipinfo conta do registro de fim: tudo no formato detalhado (`-v`), só o tamanho e o
    /// número de entradas no cabeçalho dos outros (`zi_end_central`).
    pub fn zi_end_central(&mut self) {
        let e = self.ecrec.clone();
        let ziplen = self.zin.ziplen;
        if self.o.lflag > 9 {
            self.info(0, "\nEnd-of-central-directory record:\n");
            self.info(0, "-------------------------------\n\n");
            self.info(0, format!("  Zip archive file size:               {ziplen:11} ({ziplen:016X}h)\n"));
            let (real, expect) = (self.real_ecrec_offset as u64, self.expect_ecrec_offset as u64);
            self.info(
                0,
                format!(
                    "  Actual end-cent-dir record offset:   {real:11} ({real:016X}h)\n  Expected end-cent-dir record offset: {expect:11} ({expect:016X}h)\n  (based on the length of the central directory and its expected offset)\n\n"
                ),
            );
            let plural = |n: u64| if n == 1 { "entry" } else { "entries" };
            let (size, offset) = (e.size_central_directory, e.offset_start_central_directory);
            if e.number_this_disk == 0 {
                let total = e.total_entries_central_dir;
                self.info(
                    0,
                    format!(
                        "  This zipfile constitutes the sole disk of a single-part archive; its\n  central directory contains {total} {}.\n  The central directory is {size} ({size:016X}h) bytes long,\n",
                        plural(total)
                    ),
                );
                self.info(
                    0,
                    format!("  and its (expected) offset in bytes from the beginning of the zipfile\n  is {offset} ({offset:016X}h).\n\n"),
                );
            } else {
                self.info(
                    0,
                    format!(
                        "  This zipfile constitutes disk {} of a multi-part archive.  The central\n  directory starts on disk {} at an offset within that archive part\n",
                        e.number_this_disk + 1,
                        e.num_disk_start_cdir + 1
                    ),
                );
                self.info(
                    0,
                    format!("  of {offset} ({offset:016X}h) bytes.  The entire\n  central directory is {size} ({size:016X}h) bytes long.\n"),
                );
                let (this, total) = (e.num_entries_centrl_dir_ths_disk, e.total_entries_central_dir);
                self.info(
                    0,
                    format!(
                        "  {this} of the archive entries {} contained within this zipfile volume,\n  out of a total of {total} {}.\n\n",
                        if this == 1 { "is" } else { "are" },
                        plural(total)
                    ),
                );
            }
        } else if self.o.hflag != 0 {
            self.info(0, format!("Zip file size: {ziplen} bytes, number of entries: {}\n", e.total_entries_central_dir));
        }
    }
}
