//! Entrada e saída (fileio.c) e a parte Unix delas (unix.c): textos de tamanho variável do arquivo,
//! criação dos arquivos de saída, nomes, diretórios e atributos.

use super::{MSG_STDERR, MSG_TNEWLN, PK_EOF, PK_ERR, PK_OK, PK_WARN, Uz};

/// Tamanho do buffer de saída no Unix (`OUTBUFSIZ` = `WSIZE`).
pub const OUTBUFSIZ: usize = 65536;
/// Janela do inflate, que também serve de rascunho na saída dos comentários (`WSIZE`).
pub const WSIZE: usize = 65536;

/// Os arquivos que casam com o nome pedido (`do_wild`): o próprio nome se não tem curinga, os do
/// diretório que casam (na ordem do diretório; `*` e `?` não casam o ponto inicial), ou o nome cru
/// se nada casa ou o diretório não abre.
pub fn do_wild(wildspec: &[u8]) -> Vec<Vec<u8>> {
    if !super::matching::is_wild(wildspec) {
        return vec![wildspec.to_vec()];
    }
    let (dirname, wildname): (&[u8], &[u8]) = match wildspec.iter().rposition(|&c| c == b'/') {
        Some(i) => (&wildspec[..=i], &wildspec[i + 1..]),
        None => (b".", wildspec),
    };
    let have_dirname = dirname != b".";
    let Ok(entries) = sysabi::sys::read_dir(dirname) else { return vec![wildspec.to_vec()] };
    let found: Vec<Vec<u8>> = entries
        .into_iter()
        .filter(|e| !(e.name.first() == Some(&b'.') && wildname.first() != Some(&b'.')))
        .filter(|e| super::matching::matches(&e.name, wildname, false, None))
        .map(|e| if have_dirname { [dirname, e.name.as_slice()].concat() } else { e.name })
        .collect();
    if found.is_empty() { vec![wildspec.to_vec()] } else { found }
}

impl Uz {
    /// Pula `length` bytes do arquivo (`do_string` com `SKIP`).
    pub fn skip_string(&mut self, length: usize) -> i32 {
        if length == 0 {
            return PK_OK;
        }
        let z = &self.zin;
        let here = z.bufstart - z.extra_bytes + z.inptr as i64;
        self.seek_zipf(here + length as i64);
        PK_OK
    }

    /// Lê o nome do membro (`do_string` com `DS_FN`, ou `DS_FN_L` no cabeçalho local): guarda o nome
    /// cru inteiro, corta em `FILNAMSIZ`, converte do código de página de origem e aplica `-L`.
    pub fn read_filename(&mut self, length: usize, local: bool) -> i32 {
        if length == 0 {
            // O C não toca no nome quando o tamanho é zero.
            return PK_OK;
        }
        let mut full = vec![0u8; length];
        if self.readbuf(&mut full) == 0 {
            return PK_EOF;
        }
        let mut error = PK_OK;
        let mut name = full[..full.iter().position(|&c| c == 0).unwrap_or(length)].to_vec();
        self.filename_full = full;
        if length >= FILNAMSIZ {
            self.info(MSG_STDERR, "warning:  filename too long--truncating.\n");
            error = PK_WARN;
            name.truncate(FILNAMSIZ - 1);
        }
        let p = &self.pinfo;
        ext_ascii_to_native(&mut name, p.hostnum, p.hostver, p.has_ux_att, local);
        if p.lcflag {
            name.make_ascii_lowercase();
        }
        if p.vollabel && length > 8 && name.get(8) == Some(&b'.') {
            name.remove(8);
        }
        self.filename = name;
        error
    }

    /// Lê o campo extra (`do_string` com `EXTRA_FIELD`): aplica o bloco Zip64 e troca o nome pelo
    /// nome Unicode quando há (o próprio nome com o bit 11, ou o bloco "Unicode Path").
    pub fn read_extra_field(&mut self, length: usize) -> i32 {
        self.extra_field = None;
        if length == 0 {
            return PK_OK;
        }
        let mut ef = vec![0u8; length];
        if self.readbuf(&mut ef) == 0 {
            return PK_EOF;
        }
        let mut error = PK_OK;
        if self.get_zip64_data(&ef) != PK_OK {
            self.info(MSG_STDERR, format!("warning:  extra field (type: 0x{:04x}) corrupt.  Continuing...\n", super::process::EF_PKSZ64));
            error = PK_WARN;
        }
        if self.o.u_flag < 2 {
            let full = &self.filename_full;
            let full = full[..full.iter().position(|&c| c == 0).unwrap_or(full.len())].to_vec();
            let unipath = if self.pinfo.gpf_is_utf8 {
                Some(full)
            } else {
                match self.get_unicode_data(&ef) {
                    Ok(Some(u)) if u.is_empty() => Some(full),
                    Ok(u) => u,
                    Err(()) => None,
                }
            };
            if let Some(u) = unipath {
                if self.native_is_utf8 && !self.unicode_escape_all {
                    self.filename = u[..u.len().min(FILNAMSIZ - 1)].to_vec();
                    if u.len() >= FILNAMSIZ {
                        self.info(MSG_STDERR, "warning:  filename too long (P1) -- truncating.\n");
                        error = PK_WARN;
                    }
                } else {
                    match utf8_to_local(&u, self.unicode_escape_all, self.native_is_utf8) {
                        None => {
                            self.info(MSG_STDERR, "error: Unicode filename corrupt.\n");
                            error = PK_ERR;
                        }
                        Some(mut fname) => {
                            if fname.len() >= FILNAMSIZ {
                                fname.truncate(FILNAMSIZ - 1);
                                self.info(MSG_STDERR, "warning:  filename too long (P1) -- truncating.\n");
                                error = PK_WARN;
                            }
                            self.filename = fname;
                        }
                    }
                }
            }
        }
        self.extra_field = Some(ef);
        error
    }
}

/// `PATH_MAX`: o nome guardado tem no máximo `FILNAMSIZ - 1` bytes.
pub const FILNAMSIZ: usize = 4096;

/// O nome em forma imprimível (`fnfilter`, compilado sem `isprint`): só os controles abaixo de 32
/// mudam, pra `^` seguido da letra. O corte em `WSIZE / 2` nunca acontece com nomes de até
/// `FILNAMSIZ`.
pub fn fnfilter(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    for &c in raw.iter().take_while(|&&c| c != 0) {
        if c < 32 {
            out.extend_from_slice(&[b'^', 64 + c]);
        } else {
            out.push(c);
        }
    }
    out
}

/// Decodifica o UTF-8 do jeito do unzip (`utf8_to_ucs4_string`): aceita sequências de até 6 bytes
/// sem conferir sobrelongas nem substitutos; `None` se algum byte não forma sequência.
fn utf8_to_ucs4(s: &[u8]) -> Option<Vec<u32>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() && s[i] != 0 {
        let lead = s[i];
        let n = match lead {
            0..0x80 => 1,
            0x80..0xC0 => return None,
            0xC0..0xE0 => 2,
            0xE0..0xF0 => 3,
            0xF0..0xF8 => 4,
            0xF8..0xFC => 5,
            0xFC..0xFE => 6,
            _ => return None,
        };
        if (1..n).any(|t| !matches!(s.get(i + t), Some(0x80..0xC0))) {
            return None;
        }
        let mut ch = if n == 1 { u32::from(lead) } else { u32::from(lead) & (0x7F >> n) };
        for t in 1..n {
            ch = (ch << 6) | u32::from(s[i + t] & 0x3F);
        }
        out.push(ch);
        i += n;
    }
    Some(out)
}

/// `#Uxxxx` (até 16 bits) ou `#Lxxxxxx` (`wide_to_escape_string`).
fn escape_wide(w: u32) -> String {
    let len = (32 - w.leading_zeros()).div_ceil(8).max(2);
    if len == 2 { format!("#U{w:04x}") } else { format!("#L{w:0width$x}", width = len as usize * 2) }
}

/// O nome em UTF-8 no conjunto de caracteres do locale (`utf8_to_local_string`): no locale C só o
/// ASCII passa e o resto vira escape; com `-U`, tudo que não é ASCII vira escape. `None` se o UTF-8
/// for inválido.
fn utf8_to_local(s: &[u8], escape_all: bool, utf8_locale: bool) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for w in utf8_to_ucs4(s)? {
        let ascii = w <= 0x7f;
        let encodable = ascii || (utf8_locale && char::from_u32(w).is_some());
        if ascii || (!escape_all && encodable) {
            let mut buf = [0u8; 4];
            out.extend_from_slice(char::from_u32(w).unwrap().encode_utf8(&mut buf).as_bytes());
        } else {
            out.extend_from_slice(escape_wide(w).as_bytes());
        }
    }
    Some(out)
}

/// Converte o nome do código de página da origem (`Ext_ASCII_TO_Native`): os do DOS (FAT, HPFS, e
/// NTFS do PKZIP 5.0) vêm em CP850 e viram ISO 8859-1; os outros já estão em ISO 8859-1 e ficam.
fn ext_ascii_to_native(name: &mut [u8], hostnum: u8, hostver: u8, has_ux_att: bool, local: bool) {
    use super::process::{FS_FAT, FS_HPFS, FS_NTFS};
    let oem = (hostnum == FS_FAT && !((local || has_ux_att) && matches!(hostver, 25 | 26 | 40)))
        || hostnum == FS_HPFS
        || (hostnum == FS_NTFS && hostver == 50);
    if oem {
        for c in name.iter_mut().filter(|c| **c & 0x80 != 0) {
            *c = OEM2ISO_850[usize::from(*c & 0x7f)];
        }
    }
}

/// CP850 pra ISO 8859-1, de 0x80 a 0xFF (`oem2iso_850` do ebcdic.h).
const OEM2ISO_850: [u8; 128] = [
    0xC7, 0xFC, 0xE9, 0xE2, 0xE4, 0xE0, 0xE5, 0xE7, 0xEA, 0xEB, 0xE8, 0xEF, 0xEE, 0xEC, 0xC4, 0xC5, //
    0xC9, 0xE6, 0xC6, 0xF4, 0xF6, 0xF2, 0xFB, 0xF9, 0xFF, 0xD6, 0xDC, 0xF8, 0xA3, 0xD8, 0xD7, 0x83, //
    0xE1, 0xED, 0xF3, 0xFA, 0xF1, 0xD1, 0xAA, 0xBA, 0xBF, 0xAE, 0xAC, 0xBD, 0xBC, 0xA1, 0xAB, 0xBB, //
    0xA6, 0xA6, 0xA6, 0xA6, 0xA6, 0xC1, 0xC2, 0xC0, 0xA9, 0xA6, 0xA6, 0x2B, 0x2B, 0xA2, 0xA5, 0x2B, //
    0x2B, 0x2D, 0x2D, 0x2B, 0x2D, 0x2B, 0xE3, 0xC3, 0x2B, 0x2B, 0x2D, 0x2D, 0xA6, 0x2D, 0x2B, 0xA4, //
    0xF0, 0xD0, 0xCA, 0xCB, 0xC8, 0x69, 0xCD, 0xCE, 0xCF, 0x2B, 0x2B, 0xA6, 0x5F, 0xA6, 0xCC, 0xAF, //
    0xD3, 0xDF, 0xD4, 0xD2, 0xF5, 0xD5, 0xB5, 0xFE, 0xDE, 0xDA, 0xDB, 0xD9, 0xFD, 0xDD, 0xAF, 0xB4, //
    0xAD, 0xB1, 0x3D, 0xBE, 0xB6, 0xA7, 0xF7, 0xB8, 0xB0, 0xA8, 0xB7, 0xB9, 0xB3, 0xB2, 0xA6, 0xA0, //
];

impl Uz {
    /// Mostra `length` bytes do arquivo como texto (`do_string` com `DISPLAY`): tira os CR e os ^S,
    /// para no primeiro NUL de cada pedaço, troca ESC por `^[`, e termina a linha.
    pub fn display_string(&mut self, length: usize) -> i32 {
        self.display_text(length, false)
    }

    /// O mesmo pro comentário de um membro (`DISPL_8`), convertido do código de página da origem.
    pub fn display_string_8(&mut self, length: usize) -> i32 {
        self.display_text(length, true)
    }

    fn display_text(&mut self, length: usize, ext_ascii: bool) -> i32 {
        if length == 0 {
            return PK_OK;
        }
        let mut left = length;
        let mut block = vec![0u8; OUTBUFSIZ.min(left)];
        while left > 0 {
            let want = OUTBUFSIZ.min(left);
            let n = self.readbuf(&mut block[..want]);
            if n == 0 {
                return PK_EOF;
            }
            left -= n;
            let text = &block[..n];
            let mut text: Vec<u8> = text[..text.iter().position(|&c| c == 0).unwrap_or(n)].iter().copied().filter(|&c| c != b'\r').collect();
            if ext_ascii {
                let p = &self.pinfo;
                ext_ascii_to_native(&mut text, p.hostnum, p.hostver, p.has_ux_att, false);
            }
            let mut out = Vec::with_capacity(text.len() + 8);
            for &c in &text {
                // ^S é a pausa do terminal do autor do arquivo: some da saída.
                if c == 0x13 {
                    continue;
                }
                if c == 0x1b {
                    out.extend_from_slice(b"^[");
                } else {
                    out.push(c);
                }
                if out.len() > WSIZE - 3 {
                    self.info(0, &out);
                    out.clear();
                }
            }
            self.info(0, &out);
        }
        self.info(MSG_TNEWLN, b"");
        PK_OK
    }
}
