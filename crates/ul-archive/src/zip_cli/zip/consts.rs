//! Constantes do Info-ZIP zip 3.0 (zip.h, ziperr.h, zipfile.c, tailor.h): códigos de erro `ZE_*`,
//! assinaturas, tamanhos de cabeçalho, métodos e os parâmetros do deflate.

/// Códigos de saída e de erro (`ziperr.h`).
pub const ZE_MISS: i32 = -1;
pub const ZE_OK: i32 = 0;
pub const ZE_EOF: i32 = 2;
pub const ZE_FORM: i32 = 3;
pub const ZE_LOGIC: i32 = 5;
pub const ZE_BIG: i32 = 6;
pub const ZE_TEST: i32 = 8;
pub const ZE_ABORT: i32 = 9;
pub const ZE_TEMP: i32 = 10;
pub const ZE_READ: i32 = 11;
pub const ZE_NONE: i32 = 12;
pub const ZE_NAME: i32 = 13;
pub const ZE_WRITE: i32 = 14;
pub const ZE_CREAT: i32 = 15;
pub const ZE_PARMS: i32 = 16;
pub const ZE_OPEN: i32 = 18;
pub const ZE_COMPERR: i32 = 19;

/// A mensagem de `ziperrors[c].string` e se o erro imprime também o `strerror` (`ZE_S_PERR`).
pub fn ze_string(c: i32) -> &'static str {
    match c {
        0 => "Normal successful completion",
        2 => "Unexpected end of zip file",
        3 => "Zip file structure invalid",
        4 => "Out of memory",
        5 => "Internal logic error",
        6 => "Entry too big to split, read, or write",
        7 => "Invalid comment format",
        8 => "Zip file invalid, could not spawn unzip, or wrong unzip",
        9 => "Interrupted",
        10 => "Temporary file failure",
        11 => "Input file read failure",
        12 => "Nothing to do!",
        13 => "Missing or empty zip file",
        14 => "Output file write failure",
        15 => "Could not create output file",
        16 => "Invalid command arguments",
        18 => "File not found or no read permission",
        19 => "Not supported",
        20 => "Attempt to read unsupported Zip64 archive",
        _ => "",
    }
}

pub fn ze_perr(c: i32) -> bool {
    matches!(c, ZE_TEMP | ZE_READ | ZE_WRITE | ZE_CREAT | ZE_OPEN)
}

/// Assinaturas dos registros do zip.
pub const LOCSIG: u32 = 0x0403_4b50;
pub const CENSIG: u32 = 0x0201_4b50;
pub const ENDSIG: u32 = 0x0605_4b50;
pub const EXTLOCSIG: u32 = 0x0807_4b50;
pub const ZIP64_CENTRAL_DIR_TAIL_SIG: u32 = 0x0606_4b50;
pub const ZIP64_CENTRAL_DIR_TAIL_END_SIG: u32 = 0x0706_4b50;
pub const ZIP64_CENTRAL_DIR_TAIL_SIZE: u64 = 44;

/// Tamanhos dos registros sem a assinatura.
pub const LOCHEAD: usize = 26;
pub const CENHEAD: usize = 42;
pub const ENDHEAD: usize = 18;
pub const EC64LOC: usize = 16;
pub const EC64REC: usize = 52;

pub const ZIP_UWORD16_MAX: u64 = 0xFFFF;
pub const ZIP_UWORD32_MAX: u64 = 0xFFFF_FFFF;
pub const ZIP_EF_HEADER_SIZE: usize = 4;
pub const ZIP64_EF_TAG: u16 = 0x0001;
pub const UTF8_PATH_EF_TAG: u16 = 0x7075;
pub const ZIP64_MIN_VER: u16 = 45;
pub const UTF8_BIT: u16 = 1 << 11;

/// Campos extras do Unix.
pub const EF_IZUNIX: u16 = 0x5855;
pub const EF_IZUNIX2: u16 = 0x7855;
/// O "ux" novo (UID e GID de tamanho variável), o que o zip 3.0 grava.
pub const EF_IZUNIX3: u16 = 0x7875;
pub const EF_TIME: u16 = 0x5455;
pub const EB_HEADSIZE: usize = 4;
pub const EB_UX_MINLEN: usize = 8;
pub const EB_UT_MINLEN: usize = 1;
pub const EB_UT_FL_MTIME: i32 = 1;
pub const EB_UT_FL_ATIME: i32 = 2;
pub const EB_UT_FL_CTIME: i32 = 4;

/// Atributo interno do arquivo.
pub const UNKNOWN: u16 = 0xFFFF;
pub const BINARY: u16 = 0;
pub const ASCII: u16 = 1;

/// Métodos de compressão.
pub const BEST: i32 = -1;
pub const STORE: i32 = 0;
pub const DEFLATE: i32 = 8;
pub const BZIP2: i32 = 12;

/// Ações (`action`).
pub const ADD: i32 = 1;
pub const UPDATE: i32 = 2;
pub const FRESHEN: i32 = 3;
pub const ARCHIVE: i32 = 4;
pub const DELETE: i32 = 0;

/// Modos de `bfwrite`.
pub const BFWRITE_DATA: i32 = 0;
pub const BFWRITE_LOCALHEADER: i32 = 1;
pub const BFWRITE_CENTRALHEADER: i32 = 2;
pub const BFWRITE_HEADER: i32 = 3;

/// Modos de `putlocal`.
pub const PUTLOCAL_WRITE: i32 = 0;
pub const PUTLOCAL_REWRITE: i32 = 1;

/// Bit de diretório nos atributos do MS-DOS e o menor horário DOS (1980-01-01 00:00:00).
pub const MSDOS_DIR_ATTR: u64 = 0x10;
pub const DOSTIME_MINIMUM: u64 = 0x0021_0000;

/// `OS_CODE + Z_MAJORVER * 10 + Z_MINORVER`: Unix, zip 3.0.
pub const VEM_UNIX: u16 = 0x300 + 30;
/// Versão do MS-DOS usada por `-k`.
pub const VEM_DOS: u16 = 20;

/// Tamanho do cabeçalho aleatório da cifra tradicional.
pub const RAND_HEAD_LEN: usize = 12;
pub const IZ_PWLEN: usize = 80;

/// Buffers do zip (`CBSZ`, `ZBSZ`, `SBSZ`) e do deflate.
pub const SBSZ: usize = 16384;
pub const CBSZ: usize = 16384;
pub const MAXCOM: usize = 256;

pub const MIN_MATCH: usize = 3;
pub const MAX_MATCH: usize = 258;
pub const WSIZE: usize = 0x8000;
pub const MIN_LOOKAHEAD: usize = MAX_MATCH + MIN_MATCH + 1;
pub const MAX_DIST: usize = WSIZE - MIN_LOOKAHEAD;
