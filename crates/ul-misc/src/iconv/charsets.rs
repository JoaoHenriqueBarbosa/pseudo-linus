//! Os conjuntos de caracteres que o nosso `iconv` conhece, com os nomes e apelidos do
//! `gconv-modules` e do `gconv_builtin.h` da glibc 2.41, e as tabelas de bytes dos de 8 bits.
//!
//! Tudo passa pelo UCS-4 (o `INTERNAL` da glibc): um decodificador lê o texto de origem e devolve
//! pontos de código, um codificador escreve no destino. A ordem de bytes dos formatos sem
//! marcador segue a glibc numa máquina little-endian: `UCS-2` é a ordem do host (LE), `UCS-4` é
//! big-endian, `UTF-16` e `UTF-32` sem BOM se leem como big-endian e se escrevem com BOM na ordem
//! do host.

/// Como um conjunto de caracteres se codifica.
#[derive(Copy, Clone, Debug)]
pub enum Kind {
    Utf8,
    /// UTF-16 com BOM (lido: BOM ou big-endian; escrito: BOM e little-endian).
    Utf16,
    Utf16Le,
    Utf16Be,
    Utf32,
    Utf32Le,
    Utf32Be,
    /// `ISO-10646/UCS2/`: ordem do host, little-endian.
    Ucs2,
    /// `UNICODEBIG`, `UCS-2BE`.
    Ucs2Be,
    /// `ISO-10646/UCS4/`: big-endian.
    Ucs4,
    Ucs4Le,
    /// `INTERNAL` (`WCHAR_T`): UCS-4 na ordem do host.
    Internal,
    Ascii,
    Latin1,
    /// ISO-8859-x: 0x80 a 0x9F são os controles C1; a tabela cobre 0xA0 a 0xFF (0 = indefinido).
    Iso(&'static [u16; 96]),
    /// Páginas de código de 8 bits: a tabela cobre 0x80 a 0xFF (0 = indefinido).
    Cp(&'static [u16; 128]),
}

/// Um conjunto de caracteres: o nome do módulo (como o `iconv -l` lista) e os apelidos.
#[derive(Debug)]
pub struct Charset {
    pub names: &'static [&'static str],
    pub kind: Kind,
}

/// Lê um byte de 0x80 a 0xFF de uma tabela de página de código.
const fn cp_from_rows(rows: [[u16; 16]; 8]) -> [u16; 128] {
    let mut t = [0u16; 128];
    let mut r = 0;
    while r < 8 {
        let mut c = 0;
        while c < 16 {
            t[r * 16 + c] = rows[r][c];
            c += 1;
        }
        r += 1;
    }
    t
}

const fn iso_from_rows(rows: [[u16; 16]; 6]) -> [u16; 96] {
    let mut t = [0u16; 96];
    let mut r = 0;
    while r < 6 {
        let mut c = 0;
        while c < 16 {
            t[r * 16 + c] = rows[r][c];
            c += 1;
        }
        r += 1;
    }
    t
}

/// ISO-8859-1 de 0xA0 a 0xFF, base de várias outras.
const fn latin1_upper() -> [u16; 96] {
    let mut t = [0u16; 96];
    let mut i = 0;
    while i < 96 {
        t[i] = 0xA0 + i as u16;
        i += 1;
    }
    t
}

/// Preenche `t[from..=to]` com pontos de código consecutivos a partir de `start`.
const fn fill(mut t: [u16; 96], from: usize, to: usize, start: u16) -> [u16; 96] {
    let mut i = from;
    while i <= to {
        t[i] = start + (i - from) as u16;
        i += 1;
    }
    t
}

const fn fill128(mut t: [u16; 128], from: usize, to: usize, start: u16) -> [u16; 128] {
    let mut i = from;
    while i <= to {
        t[i] = start + (i - from) as u16;
        i += 1;
    }
    t
}

static ISO8859_2: [u16; 96] = iso_from_rows([
    [
        0x00A0, 0x0104, 0x02D8, 0x0141, 0x00A4, 0x013D, 0x015A, 0x00A7, 0x00A8, 0x0160, 0x015E,
        0x0164, 0x0179, 0x00AD, 0x017D, 0x017B,
    ],
    [
        0x00B0, 0x0105, 0x02DB, 0x0142, 0x00B4, 0x013E, 0x015B, 0x02C7, 0x00B8, 0x0161, 0x015F,
        0x0165, 0x017A, 0x02DD, 0x017E, 0x017C,
    ],
    [
        0x0154, 0x00C1, 0x00C2, 0x0102, 0x00C4, 0x0139, 0x0106, 0x00C7, 0x010C, 0x00C9, 0x0118,
        0x00CB, 0x011A, 0x00CD, 0x00CE, 0x010E,
    ],
    [
        0x0110, 0x0143, 0x0147, 0x00D3, 0x00D4, 0x0150, 0x00D6, 0x00D7, 0x0158, 0x016E, 0x00DA,
        0x0170, 0x00DC, 0x00DD, 0x0162, 0x00DF,
    ],
    [
        0x0155, 0x00E1, 0x00E2, 0x0103, 0x00E4, 0x013A, 0x0107, 0x00E7, 0x010D, 0x00E9, 0x0119,
        0x00EB, 0x011B, 0x00ED, 0x00EE, 0x010F,
    ],
    [
        0x0111, 0x0144, 0x0148, 0x00F3, 0x00F4, 0x0151, 0x00F6, 0x00F7, 0x0159, 0x016F, 0x00FA,
        0x0171, 0x00FC, 0x00FD, 0x0163, 0x02D9,
    ],
]);

static ISO8859_3: [u16; 96] = iso_from_rows([
    [
        0x00A0, 0x0126, 0x02D8, 0x00A3, 0x00A4, 0, 0x0124, 0x00A7, 0x00A8, 0x0130, 0x015E, 0x011E,
        0x0134, 0x00AD, 0, 0x017B,
    ],
    [
        0x00B0, 0x0127, 0x00B2, 0x00B3, 0x00B4, 0x00B5, 0x0125, 0x00B7, 0x00B8, 0x0131, 0x015F,
        0x011F, 0x0135, 0x00BD, 0, 0x017C,
    ],
    [
        0x00C0, 0x00C1, 0x00C2, 0, 0x00C4, 0x010A, 0x0108, 0x00C7, 0x00C8, 0x00C9, 0x00CA, 0x00CB,
        0x00CC, 0x00CD, 0x00CE, 0x00CF,
    ],
    [
        0, 0x00D1, 0x00D2, 0x00D3, 0x00D4, 0x0120, 0x00D6, 0x00D7, 0x011C, 0x00D9, 0x00DA, 0x00DB,
        0x00DC, 0x016C, 0x015C, 0x00DF,
    ],
    [
        0x00E0, 0x00E1, 0x00E2, 0, 0x00E4, 0x010B, 0x0109, 0x00E7, 0x00E8, 0x00E9, 0x00EA, 0x00EB,
        0x00EC, 0x00ED, 0x00EE, 0x00EF,
    ],
    [
        0, 0x00F1, 0x00F2, 0x00F3, 0x00F4, 0x0121, 0x00F6, 0x00F7, 0x011D, 0x00F9, 0x00FA, 0x00FB,
        0x00FC, 0x016D, 0x015D, 0x02D9,
    ],
]);

static ISO8859_4: [u16; 96] = iso_from_rows([
    [
        0x00A0, 0x0104, 0x0138, 0x0156, 0x00A4, 0x0128, 0x013B, 0x00A7, 0x00A8, 0x0160, 0x0112,
        0x0122, 0x0166, 0x00AD, 0x017D, 0x00AF,
    ],
    [
        0x00B0, 0x0105, 0x02DB, 0x0157, 0x00B4, 0x0129, 0x013C, 0x02C7, 0x00B8, 0x0161, 0x0113,
        0x0123, 0x0167, 0x014A, 0x017E, 0x014B,
    ],
    [
        0x0100, 0x00C1, 0x00C2, 0x00C3, 0x00C4, 0x00C5, 0x00C6, 0x012E, 0x010C, 0x00C9, 0x0118,
        0x00CB, 0x0116, 0x00CD, 0x00CE, 0x012A,
    ],
    [
        0x0110, 0x0145, 0x014C, 0x0136, 0x00D4, 0x00D5, 0x00D6, 0x00D7, 0x00D8, 0x0172, 0x00DA,
        0x00DB, 0x00DC, 0x0168, 0x016A, 0x00DF,
    ],
    [
        0x0101, 0x00E1, 0x00E2, 0x00E3, 0x00E4, 0x00E5, 0x00E6, 0x012F, 0x010D, 0x00E9, 0x0119,
        0x00EB, 0x0117, 0x00ED, 0x00EE, 0x012B,
    ],
    [
        0x0111, 0x0146, 0x014D, 0x0137, 0x00F4, 0x00F5, 0x00F6, 0x00F7, 0x00F8, 0x0173, 0x00FA,
        0x00FB, 0x00FC, 0x0169, 0x016B, 0x02D9,
    ],
]);

const fn iso8859_5() -> [u16; 96] {
    let mut t = [0u16; 96];
    t[0] = 0x00A0;
    t = fill(t, 0x01, 0x0C, 0x0401);
    t[0x0D] = 0x00AD;
    t[0x0E] = 0x040E;
    t[0x0F] = 0x040F;
    t = fill(t, 0x10, 0x4F, 0x0410);
    t[0x50] = 0x2116;
    t = fill(t, 0x51, 0x5C, 0x0451);
    t[0x5D] = 0x00A7;
    t[0x5E] = 0x045E;
    t[0x5F] = 0x045F;
    t
}
static ISO8859_5: [u16; 96] = iso8859_5();

const fn iso8859_6() -> [u16; 96] {
    let mut t = [0u16; 96];
    t[0x00] = 0x00A0;
    t[0x04] = 0x00A4;
    t[0x0C] = 0x060C;
    t[0x0D] = 0x00AD;
    t[0x1B] = 0x061B;
    t[0x1F] = 0x061F;
    t = fill(t, 0x21, 0x3A, 0x0621);
    t = fill(t, 0x40, 0x52, 0x0640);
    t
}
static ISO8859_6: [u16; 96] = iso8859_6();

const fn iso8859_7() -> [u16; 96] {
    let mut t = [0u16; 96];
    let first: [u16; 32] = [
        0x00A0, 0x2018, 0x2019, 0x00A3, 0x20AC, 0x20AF, 0x00A6, 0x00A7, 0x00A8, 0x00A9, 0x037A,
        0x00AB, 0x00AC, 0x00AD, 0, 0x2015, 0x00B0, 0x00B1, 0x00B2, 0x00B3, 0x0384, 0x0385, 0x0386,
        0x00B7, 0x0388, 0x0389, 0x038A, 0x00BB, 0x038C, 0x00BD, 0x038E, 0x038F,
    ];
    let mut i = 0;
    while i < 32 {
        t[i] = first[i];
        i += 1;
    }
    t = fill(t, 0x20, 0x31, 0x0390);
    t = fill(t, 0x33, 0x5E, 0x03A3);
    t
}
static ISO8859_7: [u16; 96] = iso8859_7();

const fn iso8859_8() -> [u16; 96] {
    let mut t = [0u16; 96];
    t[0x00] = 0x00A0;
    t = fill(t, 0x02, 0x09, 0x00A2);
    t[0x0A] = 0x00D7;
    t = fill(t, 0x0B, 0x19, 0x00AB);
    t[0x1A] = 0x00F7;
    t = fill(t, 0x1B, 0x1E, 0x00BB);
    t[0x3F] = 0x2017;
    t = fill(t, 0x40, 0x5A, 0x05D0);
    t[0x5D] = 0x200E;
    t[0x5E] = 0x200F;
    t
}
static ISO8859_8: [u16; 96] = iso8859_8();

const fn iso8859_9() -> [u16; 96] {
    let mut t = latin1_upper();
    t[0x30] = 0x011E;
    t[0x3D] = 0x0130;
    t[0x3E] = 0x015E;
    t[0x50] = 0x011F;
    t[0x5D] = 0x0131;
    t[0x5E] = 0x015F;
    t
}
static ISO8859_9: [u16; 96] = iso8859_9();

static ISO8859_10: [u16; 96] = iso_from_rows([
    [
        0x00A0, 0x0104, 0x0112, 0x0122, 0x012A, 0x0128, 0x0136, 0x00A7, 0x013B, 0x0110, 0x0160,
        0x0166, 0x017D, 0x00AD, 0x016A, 0x014A,
    ],
    [
        0x00B0, 0x0105, 0x0113, 0x0123, 0x012B, 0x0129, 0x0137, 0x00B7, 0x013C, 0x0111, 0x0161,
        0x0167, 0x017E, 0x2015, 0x016B, 0x014B,
    ],
    [
        0x0100, 0x00C1, 0x00C2, 0x00C3, 0x00C4, 0x00C5, 0x00C6, 0x012E, 0x010C, 0x00C9, 0x0118,
        0x00CB, 0x0116, 0x00CD, 0x00CE, 0x00CF,
    ],
    [
        0x00D0, 0x0145, 0x014C, 0x00D3, 0x00D4, 0x00D5, 0x00D6, 0x0168, 0x00D8, 0x0172, 0x00DA,
        0x00DB, 0x00DC, 0x00DD, 0x00DE, 0x00DF,
    ],
    [
        0x0101, 0x00E1, 0x00E2, 0x00E3, 0x00E4, 0x00E5, 0x00E6, 0x012F, 0x010D, 0x00E9, 0x0119,
        0x00EB, 0x0117, 0x00ED, 0x00EE, 0x00EF,
    ],
    [
        0x00F0, 0x0146, 0x014D, 0x00F3, 0x00F4, 0x00F5, 0x00F6, 0x0169, 0x00F8, 0x0173, 0x00FA,
        0x00FB, 0x00FC, 0x00FD, 0x00FE, 0x0138,
    ],
]);

const fn iso8859_11() -> [u16; 96] {
    let mut t = [0u16; 96];
    t[0] = 0x00A0;
    t = fill(t, 0x01, 0x3A, 0x0E01);
    t = fill(t, 0x3F, 0x5B, 0x0E3F);
    t
}
static ISO8859_11: [u16; 96] = iso8859_11();

static ISO8859_13: [u16; 96] = iso_from_rows([
    [
        0x00A0, 0x201D, 0x00A2, 0x00A3, 0x00A4, 0x201E, 0x00A6, 0x00A7, 0x00D8, 0x00A9, 0x0156,
        0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x00C6,
    ],
    [
        0x00B0, 0x00B1, 0x00B2, 0x00B3, 0x201C, 0x00B5, 0x00B6, 0x00B7, 0x00F8, 0x00B9, 0x0157,
        0x00BB, 0x00BC, 0x00BD, 0x00BE, 0x00E6,
    ],
    [
        0x0104, 0x012E, 0x0100, 0x0106, 0x00C4, 0x00C5, 0x0118, 0x0112, 0x010C, 0x00C9, 0x0179,
        0x0116, 0x0122, 0x0136, 0x012A, 0x013B,
    ],
    [
        0x0160, 0x0143, 0x0145, 0x00D3, 0x014C, 0x00D5, 0x00D6, 0x00D7, 0x0172, 0x0141, 0x015A,
        0x016A, 0x00DC, 0x017B, 0x017D, 0x00DF,
    ],
    [
        0x0105, 0x012F, 0x0101, 0x0107, 0x00E4, 0x00E5, 0x0119, 0x0113, 0x010D, 0x00E9, 0x017A,
        0x0117, 0x0123, 0x0137, 0x012B, 0x013C,
    ],
    [
        0x0161, 0x0144, 0x0146, 0x00F3, 0x014D, 0x00F5, 0x00F6, 0x00F7, 0x0173, 0x0142, 0x015B,
        0x016B, 0x00FC, 0x017C, 0x017E, 0x2019,
    ],
]);

const fn iso8859_14() -> [u16; 96] {
    let mut t = latin1_upper();
    let first: [u16; 32] = [
        0x00A0, 0x1E02, 0x1E03, 0x00A3, 0x010A, 0x010B, 0x1E0A, 0x00A7, 0x1E80, 0x00A9, 0x1E82,
        0x1E0B, 0x1EF2, 0x00AD, 0x00AE, 0x0178, 0x1E1E, 0x1E1F, 0x0120, 0x0121, 0x1E40, 0x1E41,
        0x00B6, 0x1E56, 0x1E81, 0x1E57, 0x1E83, 0x1E60, 0x1EF3, 0x1E84, 0x1E85, 0x1E61,
    ];
    let mut i = 0;
    while i < 32 {
        t[i] = first[i];
        i += 1;
    }
    t[0x30] = 0x0174;
    t[0x37] = 0x1E6A;
    t[0x3E] = 0x0176;
    t[0x50] = 0x0175;
    t[0x57] = 0x1E6B;
    t[0x5E] = 0x0177;
    t
}
static ISO8859_14: [u16; 96] = iso8859_14();

const fn iso8859_15() -> [u16; 96] {
    let mut t = latin1_upper();
    t[0x04] = 0x20AC;
    t[0x06] = 0x0160;
    t[0x08] = 0x0161;
    t[0x14] = 0x017D;
    t[0x18] = 0x017E;
    t[0x1C] = 0x0152;
    t[0x1D] = 0x0153;
    t[0x1E] = 0x0178;
    t
}
static ISO8859_15: [u16; 96] = iso8859_15();

static ISO8859_16: [u16; 96] = iso_from_rows([
    [
        0x00A0, 0x0104, 0x0105, 0x0141, 0x20AC, 0x201E, 0x0160, 0x00A7, 0x0161, 0x00A9, 0x0218,
        0x00AB, 0x0179, 0x00AD, 0x017A, 0x017B,
    ],
    [
        0x00B0, 0x00B1, 0x010C, 0x0142, 0x017D, 0x201D, 0x00B6, 0x00B7, 0x017E, 0x010D, 0x0219,
        0x00BB, 0x0152, 0x0153, 0x0178, 0x017C,
    ],
    [
        0x00C0, 0x00C1, 0x00C2, 0x0102, 0x00C4, 0x0106, 0x00C6, 0x00C7, 0x00C8, 0x00C9, 0x00CA,
        0x00CB, 0x00CC, 0x00CD, 0x00CE, 0x00CF,
    ],
    [
        0x0110, 0x0143, 0x00D2, 0x00D3, 0x00D4, 0x0150, 0x00D6, 0x015A, 0x0170, 0x00D9, 0x00DA,
        0x00DB, 0x00DC, 0x0118, 0x021A, 0x00DF,
    ],
    [
        0x00E0, 0x00E1, 0x00E2, 0x0103, 0x00E4, 0x0107, 0x00E6, 0x00E7, 0x00E8, 0x00E9, 0x00EA,
        0x00EB, 0x00EC, 0x00ED, 0x00EE, 0x00EF,
    ],
    [
        0x0111, 0x0144, 0x00F2, 0x00F3, 0x00F4, 0x0151, 0x00F6, 0x015B, 0x0171, 0x00F9, 0x00FA,
        0x00FB, 0x00FC, 0x0119, 0x021B, 0x00FF,
    ],
]);

/// As duas primeiras linhas (0x80 a 0x9F) da família Windows.
const WIN_1252_LOW: [[u16; 16]; 2] = [
    [
        0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
        0x0152, 0, 0x017D, 0,
    ],
    [
        0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A,
        0x0153, 0, 0x017E, 0x0178,
    ],
];

/// Junta 0x80 a 0x9F com uma metade superior de ISO-8859.
const fn win(low: [[u16; 16]; 2], upper: [u16; 96]) -> [u16; 128] {
    let mut t = [0u16; 128];
    let mut i = 0;
    while i < 32 {
        t[i] = low[i / 16][i % 16];
        i += 1;
    }
    while i < 128 {
        t[i] = upper[i - 32];
        i += 1;
    }
    t
}

static CP1252: [u16; 128] = win(WIN_1252_LOW, latin1_upper());

const fn cp1250() -> [u16; 128] {
    let low = [
        [
            0x20AC, 0, 0x201A, 0, 0x201E, 0x2026, 0x2020, 0x2021, 0, 0x2030, 0x0160, 0x2039,
            0x015A, 0x0164, 0x017D, 0x0179,
        ],
        [
            0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0, 0x2122, 0x0161, 0x203A,
            0x015B, 0x0165, 0x017E, 0x017A,
        ],
    ];
    let mut upper = ISO8859_2_CONST;
    let a0: [u16; 32] = [
        0x00A0, 0x02C7, 0x02D8, 0x0141, 0x00A4, 0x0104, 0x00A6, 0x00A7, 0x00A8, 0x00A9, 0x015E,
        0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x017B, 0x00B0, 0x00B1, 0x02DB, 0x0142, 0x00B4, 0x00B5,
        0x00B6, 0x00B7, 0x00B8, 0x0105, 0x015F, 0x00BB, 0x013D, 0x02DD, 0x013E, 0x017C,
    ];
    let mut i = 0;
    while i < 32 {
        upper[i] = a0[i];
        i += 1;
    }
    win(low, upper)
}
/// Cópia constante da ISO-8859-2 (o `static` não pode ser lido em `const fn`).
const ISO8859_2_CONST: [u16; 96] = iso_from_rows([
    [
        0x00A0, 0x0104, 0x02D8, 0x0141, 0x00A4, 0x013D, 0x015A, 0x00A7, 0x00A8, 0x0160, 0x015E,
        0x0164, 0x0179, 0x00AD, 0x017D, 0x017B,
    ],
    [
        0x00B0, 0x0105, 0x02DB, 0x0142, 0x00B4, 0x013E, 0x015B, 0x02C7, 0x00B8, 0x0161, 0x015F,
        0x0165, 0x017A, 0x02DD, 0x017E, 0x017C,
    ],
    [
        0x0154, 0x00C1, 0x00C2, 0x0102, 0x00C4, 0x0139, 0x0106, 0x00C7, 0x010C, 0x00C9, 0x0118,
        0x00CB, 0x011A, 0x00CD, 0x00CE, 0x010E,
    ],
    [
        0x0110, 0x0143, 0x0147, 0x00D3, 0x00D4, 0x0150, 0x00D6, 0x00D7, 0x0158, 0x016E, 0x00DA,
        0x0170, 0x00DC, 0x00DD, 0x0162, 0x00DF,
    ],
    [
        0x0155, 0x00E1, 0x00E2, 0x0103, 0x00E4, 0x013A, 0x0107, 0x00E7, 0x010D, 0x00E9, 0x0119,
        0x00EB, 0x011B, 0x00ED, 0x00EE, 0x010F,
    ],
    [
        0x0111, 0x0144, 0x0148, 0x00F3, 0x00F4, 0x0151, 0x00F6, 0x00F7, 0x0159, 0x016F, 0x00FA,
        0x0171, 0x00FC, 0x00FD, 0x0163, 0x02D9,
    ],
]);
static CP1250: [u16; 128] = cp1250();

static CP1251: [u16; 128] = fill128(
    cp_from_rows([
        [
            0x0402, 0x0403, 0x201A, 0x0453, 0x201E, 0x2026, 0x2020, 0x2021, 0x20AC, 0x2030, 0x0409,
            0x2039, 0x040A, 0x040C, 0x040B, 0x040F,
        ],
        [
            0x0452, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0, 0x2122, 0x0459,
            0x203A, 0x045A, 0x045C, 0x045B, 0x045F,
        ],
        [
            0x00A0, 0x040E, 0x045E, 0x0408, 0x00A4, 0x0490, 0x00A6, 0x00A7, 0x0401, 0x00A9, 0x0404,
            0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x0407,
        ],
        [
            0x00B0, 0x00B1, 0x0406, 0x0456, 0x0491, 0x00B5, 0x00B6, 0x00B7, 0x0451, 0x2116, 0x0454,
            0x00BB, 0x0458, 0x0405, 0x0455, 0x0457,
        ],
        [0; 16],
        [0; 16],
        [0; 16],
        [0; 16],
    ]),
    0x40,
    0x7F,
    0x0410,
);

const fn cp1253() -> [u16; 128] {
    let mut t = cp_from_rows([
        [
            0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0, 0x2030, 0, 0x2039, 0, 0,
            0, 0,
        ],
        [
            0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0, 0x2122, 0, 0x203A, 0, 0,
            0, 0,
        ],
        [
            0x00A0, 0x0385, 0x0386, 0x00A3, 0x00A4, 0x00A5, 0x00A6, 0x00A7, 0x00A8, 0x00A9, 0,
            0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x2015,
        ],
        [
            0x00B0, 0x00B1, 0x00B2, 0x00B3, 0x0384, 0x00B5, 0x00B6, 0x00B7, 0x0388, 0x0389, 0x038A,
            0x00BB, 0x038C, 0x00BD, 0x038E, 0x038F,
        ],
        [0; 16],
        [0; 16],
        [0; 16],
        [0; 16],
    ]);
    t = fill128(t, 0x40, 0x51, 0x0390);
    t = fill128(t, 0x53, 0x7E, 0x03A3);
    t
}
static CP1253: [u16; 128] = cp1253();

const fn cp1254() -> [u16; 128] {
    let mut low = WIN_1252_LOW;
    low[0][0x0E] = 0;
    low[1][0x0E] = 0;
    win(low, iso8859_9())
}
static CP1254: [u16; 128] = cp1254();

const fn cp1255() -> [u16; 128] {
    let mut t = cp_from_rows([
        [
            0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0, 0x2039,
            0, 0, 0, 0,
        ],
        [
            0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0, 0x203A,
            0, 0, 0, 0,
        ],
        [
            0x00A0, 0x00A1, 0x00A2, 0x00A3, 0x20AA, 0x00A5, 0x00A6, 0x00A7, 0x00A8, 0x00A9, 0x00D7,
            0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x00AF,
        ],
        [
            0x00B0, 0x00B1, 0x00B2, 0x00B3, 0x00B4, 0x00B5, 0x00B6, 0x00B7, 0x00B8, 0x00B9, 0x00F7,
            0x00BB, 0x00BC, 0x00BD, 0x00BE, 0x00BF,
        ],
        [0; 16],
        [0; 16],
        [0; 16],
        [0; 16],
    ]);
    t = fill128(t, 0x40, 0x53, 0x05B0);
    t = fill128(t, 0x54, 0x58, 0x05F0);
    t = fill128(t, 0x60, 0x7A, 0x05D0);
    t[0x7D] = 0x200E;
    t[0x7E] = 0x200F;
    t
}
static CP1255: [u16; 128] = cp1255();

static CP1256: [u16; 128] = cp_from_rows([
    [
        0x20AC, 0x067E, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0679,
        0x2039, 0x0152, 0x0686, 0x0698, 0x0688,
    ],
    [
        0x06AF, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x06A9, 0x2122, 0x0691,
        0x203A, 0x0153, 0x200C, 0x200D, 0x06BA,
    ],
    [
        0x00A0, 0x060C, 0x00A2, 0x00A3, 0x00A4, 0x00A5, 0x00A6, 0x00A7, 0x00A8, 0x00A9, 0x06BE,
        0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x00AF,
    ],
    [
        0x00B0, 0x00B1, 0x00B2, 0x00B3, 0x00B4, 0x00B5, 0x00B6, 0x00B7, 0x00B8, 0x00B9, 0x061B,
        0x00BB, 0x00BC, 0x00BD, 0x00BE, 0x061F,
    ],
    [
        0x06C1, 0x0621, 0x0622, 0x0623, 0x0624, 0x0625, 0x0626, 0x0627, 0x0628, 0x0629, 0x062A,
        0x062B, 0x062C, 0x062D, 0x062E, 0x062F,
    ],
    [
        0x0630, 0x0631, 0x0632, 0x0633, 0x0634, 0x0635, 0x0636, 0x00D7, 0x0637, 0x0638, 0x0639,
        0x063A, 0x0640, 0x0641, 0x0642, 0x0643,
    ],
    [
        0x00E0, 0x0644, 0x00E2, 0x0645, 0x0646, 0x0647, 0x0648, 0x00E7, 0x00E8, 0x00E9, 0x00EA,
        0x00EB, 0x0649, 0x064A, 0x00EE, 0x00EF,
    ],
    [
        0x064B, 0x064C, 0x064D, 0x064E, 0x00F4, 0x064F, 0x0650, 0x00F7, 0x0651, 0x00F9, 0x0652,
        0x00FB, 0x00FC, 0x200E, 0x200F, 0x06D2,
    ],
]);

const fn cp1257() -> [u16; 128] {
    let low = [
        [
            0x20AC, 0, 0x201A, 0, 0x201E, 0x2026, 0x2020, 0x2021, 0, 0x2030, 0, 0x2039, 0, 0x00A8,
            0x02C7, 0x00B8,
        ],
        [
            0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0, 0x2122, 0, 0x203A, 0,
            0x00AF, 0x02DB, 0,
        ],
    ];
    let mut upper = ISO8859_13_CONST;
    let a0: [u16; 32] = [
        0x00A0, 0, 0x00A2, 0x00A3, 0x00A4, 0, 0x00A6, 0x00A7, 0x00D8, 0x00A9, 0x0156, 0x00AB,
        0x00AC, 0x00AD, 0x00AE, 0x00C6, 0x00B0, 0x00B1, 0x00B2, 0x00B3, 0x00B4, 0x00B5, 0x00B6,
        0x00B7, 0x00F8, 0x00B9, 0x0157, 0x00BB, 0x00BC, 0x00BD, 0x00BE, 0x00E6,
    ];
    let mut i = 0;
    while i < 32 {
        upper[i] = a0[i];
        i += 1;
    }
    upper[0x5F] = 0x02D9;
    win(low, upper)
}
const ISO8859_13_CONST: [u16; 96] = iso_from_rows([
    [0; 16],
    [0; 16],
    [
        0x0104, 0x012E, 0x0100, 0x0106, 0x00C4, 0x00C5, 0x0118, 0x0112, 0x010C, 0x00C9, 0x0179,
        0x0116, 0x0122, 0x0136, 0x012A, 0x013B,
    ],
    [
        0x0160, 0x0143, 0x0145, 0x00D3, 0x014C, 0x00D5, 0x00D6, 0x00D7, 0x0172, 0x0141, 0x015A,
        0x016A, 0x00DC, 0x017B, 0x017D, 0x00DF,
    ],
    [
        0x0105, 0x012F, 0x0101, 0x0107, 0x00E4, 0x00E5, 0x0119, 0x0113, 0x010D, 0x00E9, 0x017A,
        0x0117, 0x0123, 0x0137, 0x012B, 0x013C,
    ],
    [
        0x0161, 0x0144, 0x0146, 0x00F3, 0x014D, 0x00F5, 0x00F6, 0x00F7, 0x0173, 0x0142, 0x015B,
        0x016B, 0x00FC, 0x017C, 0x017E, 0x2019,
    ],
]);
static CP1257: [u16; 128] = cp1257();

const fn cp1258() -> [u16; 128] {
    let mut low = WIN_1252_LOW;
    low[0][0x0A] = 0;
    low[0][0x0E] = 0;
    low[1][0x0A] = 0;
    low[1][0x0E] = 0;
    let mut upper = latin1_upper();
    upper[0x23] = 0x0102;
    upper[0x2C] = 0x0300;
    upper[0x30] = 0x0110;
    upper[0x32] = 0x0309;
    upper[0x35] = 0x01A0;
    upper[0x3D] = 0x01AF;
    upper[0x3E] = 0x0303;
    upper[0x43] = 0x0103;
    upper[0x4C] = 0x0301;
    upper[0x50] = 0x0111;
    upper[0x52] = 0x0323;
    upper[0x55] = 0x01A1;
    upper[0x5D] = 0x01B0;
    upper[0x5E] = 0x20AB;
    win(low, upper)
}
static CP1258: [u16; 128] = cp1258();

const CP437_CONST: [u16; 128] = cp_from_rows([
    [
        0x00C7, 0x00FC, 0x00E9, 0x00E2, 0x00E4, 0x00E0, 0x00E5, 0x00E7, 0x00EA, 0x00EB, 0x00E8,
        0x00EF, 0x00EE, 0x00EC, 0x00C4, 0x00C5,
    ],
    [
        0x00C9, 0x00E6, 0x00C6, 0x00F4, 0x00F6, 0x00F2, 0x00FB, 0x00F9, 0x00FF, 0x00D6, 0x00DC,
        0x00A2, 0x00A3, 0x00A5, 0x20A7, 0x0192,
    ],
    [
        0x00E1, 0x00ED, 0x00F3, 0x00FA, 0x00F1, 0x00D1, 0x00AA, 0x00BA, 0x00BF, 0x2310, 0x00AC,
        0x00BD, 0x00BC, 0x00A1, 0x00AB, 0x00BB,
    ],
    [
        0x2591, 0x2592, 0x2593, 0x2502, 0x2524, 0x2561, 0x2562, 0x2556, 0x2555, 0x2563, 0x2551,
        0x2557, 0x255D, 0x255C, 0x255B, 0x2510,
    ],
    [
        0x2514, 0x2534, 0x252C, 0x251C, 0x2500, 0x253C, 0x255E, 0x255F, 0x255A, 0x2554, 0x2569,
        0x2566, 0x2560, 0x2550, 0x256C, 0x2567,
    ],
    [
        0x2568, 0x2564, 0x2565, 0x2559, 0x2558, 0x2552, 0x2553, 0x256B, 0x256A, 0x2518, 0x250C,
        0x2588, 0x2584, 0x258C, 0x2590, 0x2580,
    ],
    [
        0x03B1, 0x00DF, 0x0393, 0x03C0, 0x03A3, 0x03C3, 0x00B5, 0x03C4, 0x03A6, 0x0398, 0x03A9,
        0x03B4, 0x221E, 0x03C6, 0x03B5, 0x2229,
    ],
    [
        0x2261, 0x00B1, 0x2265, 0x2264, 0x2320, 0x2321, 0x00F7, 0x2248, 0x00B0, 0x2219, 0x00B7,
        0x221A, 0x207F, 0x00B2, 0x25A0, 0x00A0,
    ],
]);
static CP437: [u16; 128] = CP437_CONST;

static CP850: [u16; 128] = cp_from_rows([
    [
        0x00C7, 0x00FC, 0x00E9, 0x00E2, 0x00E4, 0x00E0, 0x00E5, 0x00E7, 0x00EA, 0x00EB, 0x00E8,
        0x00EF, 0x00EE, 0x00EC, 0x00C4, 0x00C5,
    ],
    [
        0x00C9, 0x00E6, 0x00C6, 0x00F4, 0x00F6, 0x00F2, 0x00FB, 0x00F9, 0x00FF, 0x00D6, 0x00DC,
        0x00F8, 0x00A3, 0x00D8, 0x00D7, 0x0192,
    ],
    [
        0x00E1, 0x00ED, 0x00F3, 0x00FA, 0x00F1, 0x00D1, 0x00AA, 0x00BA, 0x00BF, 0x00AE, 0x00AC,
        0x00BD, 0x00BC, 0x00A1, 0x00AB, 0x00BB,
    ],
    [
        0x2591, 0x2592, 0x2593, 0x2502, 0x2524, 0x00C1, 0x00C2, 0x00C0, 0x00A9, 0x2563, 0x2551,
        0x2557, 0x255D, 0x00A2, 0x00A5, 0x2510,
    ],
    [
        0x2514, 0x2534, 0x252C, 0x251C, 0x2500, 0x253C, 0x00E3, 0x00C3, 0x255A, 0x2554, 0x2569,
        0x2566, 0x2560, 0x2550, 0x256C, 0x00A4,
    ],
    [
        0x00F0, 0x00D0, 0x00CA, 0x00CB, 0x00C8, 0x0131, 0x00CD, 0x00CE, 0x00CF, 0x2518, 0x250C,
        0x2588, 0x2584, 0x00A6, 0x00CC, 0x2580,
    ],
    [
        0x00D3, 0x00DF, 0x00D4, 0x00D2, 0x00F5, 0x00D5, 0x00B5, 0x00FE, 0x00DE, 0x00DA, 0x00DB,
        0x00D9, 0x00FD, 0x00DD, 0x00AF, 0x00B4,
    ],
    [
        0x00AD, 0x00B1, 0x2017, 0x00BE, 0x00B6, 0x00A7, 0x00F7, 0x00B8, 0x00B0, 0x00A8, 0x00B7,
        0x00B9, 0x00B3, 0x00B2, 0x25A0, 0x00A0,
    ],
]);

const KOI8_R_CONST: [u16; 128] = cp_from_rows([
    [
        0x2500, 0x2502, 0x250C, 0x2510, 0x2514, 0x2518, 0x251C, 0x2524, 0x252C, 0x2534, 0x253C,
        0x2580, 0x2584, 0x2588, 0x258C, 0x2590,
    ],
    [
        0x2591, 0x2592, 0x2593, 0x2320, 0x25A0, 0x2219, 0x221A, 0x2248, 0x2264, 0x2265, 0x00A0,
        0x2321, 0x00B0, 0x00B2, 0x00B7, 0x00F7,
    ],
    [
        0x2550, 0x2551, 0x2552, 0x0451, 0x2553, 0x2554, 0x2555, 0x2556, 0x2557, 0x2558, 0x2559,
        0x255A, 0x255B, 0x255C, 0x255D, 0x255E,
    ],
    [
        0x255F, 0x2560, 0x2561, 0x0401, 0x2562, 0x2563, 0x2564, 0x2565, 0x2566, 0x2567, 0x2568,
        0x2569, 0x256A, 0x256B, 0x256C, 0x00A9,
    ],
    [
        0x044E, 0x0430, 0x0431, 0x0446, 0x0434, 0x0435, 0x0444, 0x0433, 0x0445, 0x0438, 0x0439,
        0x043A, 0x043B, 0x043C, 0x043D, 0x043E,
    ],
    [
        0x043F, 0x044F, 0x0440, 0x0441, 0x0442, 0x0443, 0x0436, 0x0432, 0x044C, 0x044B, 0x0437,
        0x0448, 0x044D, 0x0449, 0x0447, 0x044A,
    ],
    [
        0x042E, 0x0410, 0x0411, 0x0426, 0x0414, 0x0415, 0x0424, 0x0413, 0x0425, 0x0418, 0x0419,
        0x041A, 0x041B, 0x041C, 0x041D, 0x041E,
    ],
    [
        0x041F, 0x042F, 0x0420, 0x0421, 0x0422, 0x0423, 0x0416, 0x0412, 0x042C, 0x042B, 0x0417,
        0x0428, 0x042D, 0x0429, 0x0427, 0x042A,
    ],
]);
static KOI8_R: [u16; 128] = KOI8_R_CONST;

const fn koi8_u() -> [u16; 128] {
    let mut t = KOI8_R_CONST;
    t[0x24] = 0x0454;
    t[0x26] = 0x0456;
    t[0x27] = 0x0457;
    t[0x2D] = 0x0491;
    t[0x34] = 0x0404;
    t[0x36] = 0x0406;
    t[0x37] = 0x0407;
    t[0x3D] = 0x0490;
    t
}
static KOI8_U: [u16; 128] = koi8_u();

static MACINTOSH: [u16; 128] = cp_from_rows([
    [
        0x00C4, 0x00C5, 0x00C7, 0x00C9, 0x00D1, 0x00D6, 0x00DC, 0x00E1, 0x00E0, 0x00E2, 0x00E4,
        0x00E3, 0x00E5, 0x00E7, 0x00E9, 0x00E8,
    ],
    [
        0x00EA, 0x00EB, 0x00ED, 0x00EC, 0x00EE, 0x00EF, 0x00F1, 0x00F3, 0x00F2, 0x00F4, 0x00F6,
        0x00F5, 0x00FA, 0x00F9, 0x00FB, 0x00FC,
    ],
    [
        0x2020, 0x00B0, 0x00A2, 0x00A3, 0x00A7, 0x2022, 0x00B6, 0x00DF, 0x00AE, 0x00A9, 0x2122,
        0x00B4, 0x00A8, 0x2260, 0x00C6, 0x00D8,
    ],
    [
        0x221E, 0x00B1, 0x2264, 0x2265, 0x00A5, 0x00B5, 0x2202, 0x2211, 0x220F, 0x03C0, 0x222B,
        0x00AA, 0x00BA, 0x2126, 0x00E6, 0x00F8,
    ],
    [
        0x00BF, 0x00A1, 0x00AC, 0x221A, 0x0192, 0x2248, 0x2206, 0x00AB, 0x00BB, 0x2026, 0x00A0,
        0x00C0, 0x00C3, 0x00D5, 0x0152, 0x0153,
    ],
    [
        0x2013, 0x2014, 0x201C, 0x201D, 0x2018, 0x2019, 0x00F7, 0x25CA, 0x00FF, 0x0178, 0x2044,
        0x00A4, 0x2039, 0x203A, 0xFB01, 0xFB02,
    ],
    [
        0x2021, 0x00B7, 0x201A, 0x201E, 0x2030, 0x00C2, 0x00CA, 0x00C1, 0x00CB, 0x00C8, 0x00CD,
        0x00CE, 0x00CF, 0x00CC, 0x00D3, 0x00D4,
    ],
    [
        0xF8FF, 0x00D2, 0x00DA, 0x00DB, 0x00D9, 0x0131, 0x02C6, 0x02DC, 0x00AF, 0x02D8, 0x02D9,
        0x02DA, 0x00B8, 0x02DD, 0x02DB, 0x02C7,
    ],
]);

/// Todos os conjuntos, com os nomes exatamente como o `iconv -l` da glibc os escreve (o primeiro
/// é o nome do módulo). `INTERNAL` fica fora da listagem.
pub static CHARSETS: &[Charset] = &[
    Charset {
        names: &[
            "ISO-10646/UTF8/",
            "UTF-8//",
            "UTF8//",
            "ISO-10646/UTF-8/",
            "ISO-IR-193//",
            "OSF05010001//",
        ],
        kind: Kind::Utf8,
    },
    Charset {
        names: &["UTF-16//", "UTF16//"],
        kind: Kind::Utf16,
    },
    Charset {
        names: &["UTF-16LE//", "UTF16LE//"],
        kind: Kind::Utf16Le,
    },
    Charset {
        names: &["UTF-16BE//", "UTF16BE//"],
        kind: Kind::Utf16Be,
    },
    Charset {
        names: &["UTF-32//", "UTF32//"],
        kind: Kind::Utf32,
    },
    Charset {
        names: &["UTF-32LE//", "UTF32LE//"],
        kind: Kind::Utf32Le,
    },
    Charset {
        names: &["UTF-32BE//", "UTF32BE//"],
        kind: Kind::Utf32Be,
    },
    Charset {
        names: &[
            "ISO-10646/UCS2/",
            "UCS-2//",
            "UCS2//",
            "OSF00010100//",
            "OSF00010101//",
            "OSF00010102//",
            "UNICODELITTLE//",
            "UCS-2LE//",
        ],
        kind: Kind::Ucs2,
    },
    Charset {
        names: &["UNICODEBIG//", "UCS-2BE//"],
        kind: Kind::Ucs2Be,
    },
    Charset {
        names: &[
            "ISO-10646/UCS4/",
            "UCS-4//",
            "UCS4//",
            "UCS-4BE//",
            "CSUCS4//",
            "ISO-10646//",
            "10646-1:1993//",
            "10646-1:1993/UCS4/",
            "OSF00010104//",
            "OSF00010105//",
            "OSF00010106//",
        ],
        kind: Kind::Ucs4,
    },
    Charset {
        names: &["UCS-4LE//"],
        kind: Kind::Ucs4Le,
    },
    Charset {
        names: &["INTERNAL", "WCHAR_T//"],
        kind: Kind::Internal,
    },
    Charset {
        names: &[
            "ANSI_X3.4-1968//",
            "ISO-IR-6//",
            "ANSI_X3.4-1986//",
            "ISO_646.IRV:1991//",
            "ASCII//",
            "ISO646-US//",
            "US-ASCII//",
            "US//",
            "IBM367//",
            "CP367//",
            "CSASCII//",
            "OSF00010020//",
            "ANSI_X3.4//",
        ],
        kind: Kind::Ascii,
    },
    Charset {
        names: &[
            "ISO-8859-1//",
            "ISO-IR-100//",
            "ISO_8859-1:1987//",
            "ISO_8859-1//",
            "ISO8859-1//",
            "ISO88591//",
            "LATIN1//",
            "L1//",
            "IBM819//",
            "CP819//",
            "CSISOLATIN1//",
            "8859_1//",
            "OSF00010001//",
        ],
        kind: Kind::Latin1,
    },
    Charset {
        names: &[
            "ISO-8859-2//",
            "ISO-IR-101//",
            "ISO_8859-2:1987//",
            "ISO_8859-2//",
            "ISO8859-2//",
            "ISO88592//",
            "LATIN2//",
            "L2//",
            "CSISOLATIN2//",
            "8859_2//",
            "OSF00010002//",
            "IBM912//",
            "CP912//",
        ],
        kind: Kind::Iso(&ISO8859_2),
    },
    Charset {
        names: &[
            "ISO-8859-3//",
            "ISO-IR-109//",
            "ISO_8859-3:1988//",
            "ISO_8859-3//",
            "ISO8859-3//",
            "ISO88593//",
            "LATIN3//",
            "L3//",
            "CSISOLATIN3//",
            "8859_3//",
            "OSF00010003//",
        ],
        kind: Kind::Iso(&ISO8859_3),
    },
    Charset {
        names: &[
            "ISO-8859-4//",
            "ISO-IR-110//",
            "ISO_8859-4:1988//",
            "ISO_8859-4//",
            "ISO8859-4//",
            "ISO88594//",
            "LATIN4//",
            "L4//",
            "CSISOLATIN4//",
            "8859_4//",
            "OSF00010004//",
        ],
        kind: Kind::Iso(&ISO8859_4),
    },
    Charset {
        names: &[
            "ISO-8859-5//",
            "ISO-IR-144//",
            "ISO_8859-5:1988//",
            "ISO_8859-5//",
            "ISO8859-5//",
            "ISO88595//",
            "CYRILLIC//",
            "CSISOLATINCYRILLIC//",
            "8859_5//",
            "OSF00010005//",
            "IBM915//",
            "CP915//",
        ],
        kind: Kind::Iso(&ISO8859_5),
    },
    Charset {
        names: &[
            "ISO-8859-6//",
            "ISO-IR-127//",
            "ISO_8859-6:1987//",
            "ISO_8859-6//",
            "ISO8859-6//",
            "ISO88596//",
            "ECMA-114//",
            "ASMO-708//",
            "ARABIC//",
            "CSISOLATINARABIC//",
            "8859_6//",
            "OSF00010006//",
            "IBM1089//",
            "CP1089//",
        ],
        kind: Kind::Iso(&ISO8859_6),
    },
    Charset {
        names: &[
            "ISO-8859-7//",
            "ISO-IR-126//",
            "ISO_8859-7:1987//",
            "ISO_8859-7:2003//",
            "ISO_8859-7//",
            "ISO8859-7//",
            "ISO88597//",
            "ELOT_928//",
            "ECMA-118//",
            "GREEK//",
            "GREEK8//",
            "CSISOLATINGREEK//",
            "8859_7//",
            "OSF00010007//",
            "IBM813//",
            "CP813//",
        ],
        kind: Kind::Iso(&ISO8859_7),
    },
    Charset {
        names: &[
            "ISO-8859-8//",
            "ISO-IR-138//",
            "ISO_8859-8:1988//",
            "ISO_8859-8//",
            "ISO8859-8//",
            "ISO88598//",
            "HEBREW//",
            "CSISOLATINHEBREW//",
            "8859_8//",
            "OSF00010008//",
            "IBM916//",
            "CP916//",
        ],
        kind: Kind::Iso(&ISO8859_8),
    },
    Charset {
        names: &[
            "ISO-8859-9//",
            "ISO-IR-148//",
            "ISO_8859-9:1989//",
            "ISO_8859-9//",
            "ISO8859-9//",
            "ISO88599//",
            "LATIN5//",
            "L5//",
            "CSISOLATIN5//",
            "8859_9//",
            "OSF00010009//",
            "IBM920//",
            "CP920//",
            "TS-5881//",
        ],
        kind: Kind::Iso(&ISO8859_9),
    },
    Charset {
        names: &[
            "ISO-8859-10//",
            "ISO-IR-157//",
            "ISO_8859-10:1992//",
            "ISO_8859-10//",
            "ISO8859-10//",
            "ISO885910//",
            "LATIN6//",
            "L6//",
            "CSISOLATIN6//",
            "OSF0001000A//",
        ],
        kind: Kind::Iso(&ISO8859_10),
    },
    Charset {
        names: &[
            "ISO-8859-11//",
            "ISO_8859-11//",
            "ISO8859-11//",
            "ISO885911//",
        ],
        kind: Kind::Iso(&ISO8859_11),
    },
    Charset {
        names: &[
            "ISO-8859-13//",
            "ISO-IR-179//",
            "ISO_8859-13//",
            "ISO8859-13//",
            "ISO885913//",
            "LATIN7//",
            "L7//",
            "BALTIC//",
        ],
        kind: Kind::Iso(&ISO8859_13),
    },
    Charset {
        names: &[
            "ISO-8859-14//",
            "ISO-CELTIC//",
            "ISO-IR-199//",
            "ISO_8859-14:1998//",
            "ISO_8859-14//",
            "ISO8859-14//",
            "ISO885914//",
            "LATIN8//",
            "L8//",
        ],
        kind: Kind::Iso(&ISO8859_14),
    },
    Charset {
        names: &[
            "ISO-8859-15//",
            "ISO-IR-203//",
            "ISO_8859-15//",
            "ISO8859-15//",
            "ISO885915//",
            "ISO_8859-15:1998//",
            "LATIN-9//",
            "LATIN9//",
        ],
        kind: Kind::Iso(&ISO8859_15),
    },
    Charset {
        names: &[
            "ISO-8859-16//",
            "ISO-IR-226//",
            "ISO_8859-16:2001//",
            "ISO_8859-16//",
            "ISO8859-16//",
            "ISO885916//",
            "LATIN10//",
            "L10//",
        ],
        kind: Kind::Iso(&ISO8859_16),
    },
    Charset {
        names: &["CP1250//", "MS-EE//", "WINDOWS-1250//"],
        kind: Kind::Cp(&CP1250),
    },
    Charset {
        names: &["CP1251//", "MS-CYRL//", "WINDOWS-1251//"],
        kind: Kind::Cp(&CP1251),
    },
    Charset {
        names: &["CP1252//", "MS-ANSI//", "WINDOWS-1252//"],
        kind: Kind::Cp(&CP1252),
    },
    Charset {
        names: &["CP1253//", "MS-GREEK//", "WINDOWS-1253//"],
        kind: Kind::Cp(&CP1253),
    },
    Charset {
        names: &["CP1254//", "MS-TURK//", "WINDOWS-1254//"],
        kind: Kind::Cp(&CP1254),
    },
    Charset {
        names: &["CP1255//", "MS-HEBR//", "WINDOWS-1255//"],
        kind: Kind::Cp(&CP1255),
    },
    Charset {
        names: &["CP1256//", "MS-ARAB//", "WINDOWS-1256//"],
        kind: Kind::Cp(&CP1256),
    },
    Charset {
        names: &["CP1257//", "WINBALTRIM//", "WINDOWS-1257//"],
        kind: Kind::Cp(&CP1257),
    },
    Charset {
        names: &["CP1258//", "WINDOWS-1258//"],
        kind: Kind::Cp(&CP1258),
    },
    Charset {
        names: &["IBM437//", "437//", "CP437//", "CSPC8CODEPAGE437//"],
        kind: Kind::Cp(&CP437),
    },
    Charset {
        names: &["IBM850//", "850//", "CP850//", "CSPC850MULTILINGUAL//"],
        kind: Kind::Cp(&CP850),
    },
    Charset {
        names: &["KOI8-R//", "CSKOI8R//", "KOI8R//"],
        kind: Kind::Cp(&KOI8_R),
    },
    Charset {
        names: &["KOI8-U//", "KOI8U//"],
        kind: Kind::Cp(&KOI8_U),
    },
    Charset {
        names: &["MACINTOSH//", "MAC//", "CSMACINTOSH//"],
        kind: Kind::Cp(&MACINTOSH),
    },
];

/// O `strip` do gconv: só letras, dígitos e `_-.,:/`, em maiúsculas.
pub fn normalize(name: &[u8]) -> Vec<u8> {
    name.iter()
        .filter(|b| b.is_ascii_alphanumeric() || matches!(**b, b'_' | b'-' | b'.' | b',' | b':' | b'/'))
        .map(|b| b.to_ascii_uppercase())
        .collect()
}

/// Acha o conjunto pelo nome (sem os sufixos `//TRANSLIT` etc.), sem diferenciar caixa.
pub fn lookup(name: &[u8]) -> Option<&'static Charset> {
    let n = normalize(name);
    if n.is_empty() {
        return None;
    }
    let mut with_two = n.clone();
    with_two.extend_from_slice(b"//");
    let mut with_one = n.clone();
    with_one.push(b'/');
    CHARSETS.iter().find(|cs| {
        cs.names.iter().any(|alias| {
            let a = alias.as_bytes();
            a == with_two.as_slice() || a == with_one.as_slice() || a == n.as_slice()
        })
    })
}

/// Resultado de decodificar um caractere.
#[derive(Debug, PartialEq, Eq)]
pub enum Decoded {
    /// Ponto de código e quantos bytes ele ocupou.
    Char(u32, usize),
    /// Bytes a pular sem produzir nada (o BOM inicial do UTF-16 e do UTF-32).
    Skip(usize),
    /// Sequência inválida; o número é quanto o `//IGNORE` pula.
    Invalid(usize),
    /// O texto acaba no meio de um caractere.
    Incomplete,
}

/// Decodificador com estado (a ordem de bytes que o BOM decidiu).
#[derive(Debug)]
pub struct Decoder {
    kind: Kind,
    big: bool,
    bom_pending: bool,
}

impl Decoder {
    pub fn new(kind: Kind) -> Decoder {
        let big = matches!(
            kind,
            Kind::Utf16Be | Kind::Utf32Be | Kind::Ucs2Be | Kind::Ucs4
        );
        Decoder {
            kind,
            big,
            bom_pending: matches!(kind, Kind::Utf16 | Kind::Utf32),
        }
    }

    fn u16_at(&self, d: &[u8]) -> u32 {
        if self.big {
            u32::from(u16::from_be_bytes([d[0], d[1]]))
        } else {
            u32::from(u16::from_le_bytes([d[0], d[1]]))
        }
    }

    fn u32_at(&self, d: &[u8]) -> u32 {
        let b = [d[0], d[1], d[2], d[3]];
        if self.big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        }
    }

    /// Decodifica o primeiro caractere de `d` (não vazio).
    pub fn decode(&mut self, d: &[u8]) -> Decoded {
        match self.kind {
            Kind::Utf8 => decode_utf8(d),
            Kind::Utf16 | Kind::Utf16Le | Kind::Utf16Be => {
                if d.len() < 2 {
                    return Decoded::Incomplete;
                }
                if self.bom_pending {
                    self.bom_pending = false;
                    match (d[0], d[1]) {
                        (0xFE, 0xFF) => {
                            self.big = true;
                            return Decoded::Skip(2);
                        }
                        (0xFF, 0xFE) => {
                            self.big = false;
                            return Decoded::Skip(2);
                        }
                        _ => self.big = false,
                    }
                }
                let u1 = self.u16_at(d);
                if !(0xD800..0xE000).contains(&u1) {
                    return Decoded::Char(u1, 2);
                }
                if u1 >= 0xDC00 {
                    return Decoded::Invalid(2);
                }
                if d.len() < 4 {
                    return Decoded::Incomplete;
                }
                let u2 = self.u16_at(&d[2..]);
                if !(0xDC00..0xE000).contains(&u2) {
                    return Decoded::Invalid(2);
                }
                Decoded::Char(0x10000 + ((u1 - 0xD800) << 10) + (u2 - 0xDC00), 4)
            }
            Kind::Utf32 | Kind::Utf32Le | Kind::Utf32Be => {
                if d.len() < 4 {
                    return Decoded::Incomplete;
                }
                if self.bom_pending {
                    self.bom_pending = false;
                    match d[..4] {
                        [0x00, 0x00, 0xFE, 0xFF] => {
                            self.big = true;
                            return Decoded::Skip(4);
                        }
                        [0xFF, 0xFE, 0x00, 0x00] => {
                            self.big = false;
                            return Decoded::Skip(4);
                        }
                        _ => self.big = false,
                    }
                }
                let c = self.u32_at(d);
                if c > 0x10FFFF || (0xD800..0xE000).contains(&c) {
                    return Decoded::Invalid(4);
                }
                Decoded::Char(c, 4)
            }
            Kind::Ucs2 | Kind::Ucs2Be => {
                if d.len() < 2 {
                    return Decoded::Incomplete;
                }
                let c = self.u16_at(d);
                if (0xD800..0xE000).contains(&c) {
                    return Decoded::Invalid(2);
                }
                Decoded::Char(c, 2)
            }
            Kind::Ucs4 | Kind::Ucs4Le | Kind::Internal => {
                if d.len() < 4 {
                    return Decoded::Incomplete;
                }
                let c = self.u32_at(d);
                if c > 0x7FFF_FFFF {
                    return Decoded::Invalid(4);
                }
                Decoded::Char(c, 4)
            }
            Kind::Ascii => {
                if d[0] < 0x80 {
                    Decoded::Char(u32::from(d[0]), 1)
                } else {
                    Decoded::Invalid(1)
                }
            }
            Kind::Latin1 => Decoded::Char(u32::from(d[0]), 1),
            Kind::Iso(t) => match d[0] {
                b if b < 0xA0 => Decoded::Char(u32::from(b), 1),
                b => match t[usize::from(b - 0xA0)] {
                    0 => Decoded::Invalid(1),
                    c => Decoded::Char(u32::from(c), 1),
                },
            },
            Kind::Cp(t) => match d[0] {
                b if b < 0x80 => Decoded::Char(u32::from(b), 1),
                b => match t[usize::from(b - 0x80)] {
                    0 => Decoded::Invalid(1),
                    c => Decoded::Char(u32::from(c), 1),
                },
            },
        }
    }
}

/// O laço `from_utf8` do `gconv_simple.c`.
fn decode_utf8(d: &[u8]) -> Decoded {
    let c = d[0];
    if c < 0x80 {
        return Decoded::Char(u32::from(c), 1);
    }
    let (cnt, mut ch) = if (0xC2..0xE0).contains(&c) {
        (2, u32::from(c & 0x1F))
    } else if c & 0xF0 == 0xE0 {
        (3, u32::from(c & 0x0F))
    } else if c & 0xF8 == 0xF0 {
        (4, u32::from(c & 0x07))
    } else {
        // Byte de início inválido: pula ele e as continuações que vierem atrás.
        let mut i = 1;
        while i < d.len() && d[i] & 0xC0 == 0x80 && i < 5 {
            i += 1;
        }
        return Decoded::Invalid(i);
    };
    if d.len() < cnt {
        let mut i = 1;
        while i < d.len() && d[i] & 0xC0 == 0x80 {
            i += 1;
        }
        if i == d.len() {
            return Decoded::Incomplete;
        }
        return Decoded::Invalid(i);
    }
    let mut i = 1;
    while i < cnt {
        let b = d[i];
        if b & 0xC0 != 0x80 {
            break;
        }
        ch = (ch << 6) | u32::from(b & 0x3F);
        i += 1;
    }
    if i < cnt {
        return Decoded::Invalid(i);
    }
    let overlong = (cnt == 3 && ch < 0x800) || (cnt == 4 && ch < 0x10000);
    if overlong || (0xD800..0xE000).contains(&ch) || ch > 0x10FFFF {
        return Decoded::Invalid(i);
    }
    Decoded::Char(ch, cnt)
}

/// Codificador com estado (o BOM que o `UTF-16` e o `UTF-32` escrevem uma vez).
#[derive(Debug)]
pub struct Encoder {
    kind: Kind,
    bom_pending: bool,
}

impl Encoder {
    pub fn new(kind: Kind) -> Encoder {
        Encoder {
            kind,
            bom_pending: matches!(kind, Kind::Utf16 | Kind::Utf32),
        }
    }

    /// Escreve o BOM da primeira conversão, quando o formato tem.
    pub fn start(&mut self, out: &mut Vec<u8>) {
        if !self.bom_pending {
            return;
        }
        self.bom_pending = false;
        match self.kind {
            Kind::Utf16 => out.extend_from_slice(&[0xFF, 0xFE]),
            Kind::Utf32 => out.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x00]),
            _ => {}
        }
    }

    /// Codifica `c`; `false` se o destino não tem o caractere.
    pub fn encode(&self, c: u32, out: &mut Vec<u8>) -> bool {
        let surrogate = (0xD800..0xE000).contains(&c);
        match self.kind {
            Kind::Utf8 => {
                if surrogate || c > 0x7FFF_FFFF {
                    return false;
                }
                push_utf8(c, out);
                true
            }
            Kind::Utf16 | Kind::Utf16Le | Kind::Utf16Be => {
                if surrogate || c > 0x10FFFF {
                    return false;
                }
                let big = matches!(self.kind, Kind::Utf16Be);
                let mut put = |u: u16| {
                    if big {
                        out.extend_from_slice(&u.to_be_bytes());
                    } else {
                        out.extend_from_slice(&u.to_le_bytes());
                    }
                };
                if c >= 0x10000 {
                    let v = c - 0x10000;
                    put(0xD800 + (v >> 10) as u16);
                    put(0xDC00 + (v & 0x3FF) as u16);
                } else {
                    put(c as u16);
                }
                true
            }
            Kind::Utf32 | Kind::Utf32Le | Kind::Utf32Be => {
                if surrogate || c > 0x10FFFF {
                    return false;
                }
                if matches!(self.kind, Kind::Utf32Be) {
                    out.extend_from_slice(&c.to_be_bytes());
                } else {
                    out.extend_from_slice(&c.to_le_bytes());
                }
                true
            }
            Kind::Ucs2 | Kind::Ucs2Be => {
                if surrogate || c > 0xFFFF {
                    return false;
                }
                let u = c as u16;
                if matches!(self.kind, Kind::Ucs2Be) {
                    out.extend_from_slice(&u.to_be_bytes());
                } else {
                    out.extend_from_slice(&u.to_le_bytes());
                }
                true
            }
            Kind::Ucs4 => {
                if c > 0x7FFF_FFFF {
                    return false;
                }
                out.extend_from_slice(&c.to_be_bytes());
                true
            }
            Kind::Ucs4Le | Kind::Internal => {
                if c > 0x7FFF_FFFF {
                    return false;
                }
                out.extend_from_slice(&c.to_le_bytes());
                true
            }
            Kind::Ascii => {
                if c < 0x80 {
                    out.push(c as u8);
                    true
                } else {
                    false
                }
            }
            Kind::Latin1 => {
                if c < 0x100 {
                    out.push(c as u8);
                    true
                } else {
                    false
                }
            }
            Kind::Iso(t) => {
                if c < 0xA0 {
                    out.push(c as u8);
                    return true;
                }
                match t.iter().position(|&u| u != 0 && u32::from(u) == c) {
                    Some(i) => {
                        out.push(0xA0 + i as u8);
                        true
                    }
                    None => false,
                }
            }
            Kind::Cp(t) => {
                if c < 0x80 {
                    out.push(c as u8);
                    return true;
                }
                match t.iter().position(|&u| u != 0 && u32::from(u) == c) {
                    Some(i) => {
                        out.push(0x80 + i as u8);
                        true
                    }
                    None => false,
                }
            }
        }
    }
}

/// UTF-8 até 6 bytes, como o `internal_utf8` da glibc.
fn push_utf8(c: u32, out: &mut Vec<u8>) {
    if c < 0x80 {
        out.push(c as u8);
        return;
    }
    let cnt = match c {
        0..=0x7FF => 2,
        0x800..=0xFFFF => 3,
        0x10000..=0x1F_FFFF => 4,
        0x20_0000..=0x3FF_FFFF => 5,
        _ => 6,
    };
    let lead: u8 = [0, 0, 0xC0, 0xE0, 0xF0, 0xF8, 0xFC][cnt];
    let mut bytes = [0u8; 6];
    let mut v = c;
    for i in (1..cnt).rev() {
        bytes[i] = 0x80 | (v & 0x3F) as u8;
        v >>= 6;
    }
    bytes[0] = lead | v as u8;
    out.extend_from_slice(&bytes[..cnt]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_is_case_insensitive() {
        assert!(matches!(lookup(b"utf-8").unwrap().kind, Kind::Utf8));
        assert!(matches!(lookup(b"latin1").unwrap().kind, Kind::Latin1));
        assert!(matches!(lookup(b"Windows-1252").unwrap().kind, Kind::Cp(_)));
        assert!(matches!(lookup(b"ISO-10646/UCS4/").unwrap().kind, Kind::Ucs4));
        assert!(lookup(b"BOGUS").is_none());
        assert!(lookup(b"INTERNAL").is_some());
    }

    #[test]
    fn single_byte_tables_round_trip() {
        for cs in CHARSETS {
            let mut dec = Decoder::new(cs.kind);
            let enc = Encoder::new(cs.kind);
            if !matches!(cs.kind, Kind::Iso(_) | Kind::Cp(_)) {
                continue;
            }
            for b in 0u8..=255 {
                if let Decoded::Char(c, 1) = dec.decode(&[b]) {
                    let mut out = Vec::new();
                    assert!(enc.encode(c, &mut out), "{} {b:#x}", cs.names[0]);
                    assert_eq!(out, vec![b], "{} {b:#x}", cs.names[0]);
                }
            }
        }
    }

    #[test]
    fn known_bytes() {
        let cp1252 = lookup(b"CP1252").unwrap();
        assert_eq!(Decoder::new(cp1252.kind).decode(&[0x80]), Decoded::Char(0x20AC, 1));
        let koi = lookup(b"KOI8-R").unwrap();
        assert_eq!(Decoder::new(koi.kind).decode(&[0xC1]), Decoded::Char(0x0430, 1));
        let l5 = lookup(b"ISO-8859-5").unwrap();
        assert_eq!(Decoder::new(l5.kind).decode(&[0xF0]), Decoded::Char(0x2116, 1));
        let gr = lookup(b"CP1253").unwrap();
        assert_eq!(Decoder::new(gr.kind).decode(&[0xC1]), Decoded::Char(0x0391, 1));
    }

    #[test]
    fn utf8_errors() {
        assert_eq!(decode_utf8(b"\xff"), Decoded::Invalid(1));
        assert_eq!(decode_utf8(b"\xc3"), Decoded::Incomplete);
        assert_eq!(decode_utf8(b"\xc3("), Decoded::Invalid(1));
        assert_eq!(decode_utf8(b"\xed\xa0\x80"), Decoded::Invalid(3));
        assert_eq!(decode_utf8("é".as_bytes()), Decoded::Char(0xE9, 2));
    }
}
