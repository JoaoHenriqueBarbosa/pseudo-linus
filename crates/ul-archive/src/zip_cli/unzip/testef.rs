//! A conferência do campo extra no `-t` (`TestExtraField`): blocos com tamanho incoerente, os
//! blocos de atributos comprimidos (OS/2, ACL, Mac, BeOS, AtheOS, segurança do NT), que são
//! descomprimidos em memória e têm o CRC conferido (`test_compr_eb`, `memextract`), e o CRC do
//! bloco de VMS da PKWARE.

use super::extract::{pad22, DEFLATED, ENHDEFLATED, STORED};
use super::fileio::fnfilter;
use super::{Uz, PK_ERR, PK_MEM3, PK_OK, PK_WARN};

const EF_OS2: u16 = 0x0009;
const EF_PKVMS: u16 = 0x000c;
const EF_MAC3: u16 = 0x334d;
const EF_ACL: u16 = 0x4c41;
const EF_NTSD: u16 = 0x4453;
const EF_ATHEOS: u16 = 0x7441;
const EF_BEOS: u16 = 0x6542;
const EB_HEADSIZE: usize = 4;
const EB_CMPRHEADLEN: usize = 6;
const EB_FLGS_OFFS: usize = 4;
const EB_OS2_HLEN: usize = 4;
const EB_BEOS_HLEN: usize = 5;
const EB_BE_FL_UNCMPR: u8 = 0x01;
const EB_MAC3_HLEN: usize = 14;
const EB_M3_FL_UNCMPR: u16 = 0x04;
const EB_NTSD_L_LEN: usize = 5;
const EB_NTSD_VERSION: usize = 4;
const EB_NTSD_MAX_VER: u8 = 0;
/// Campo extra local truncado (`IZ_EF_TRUNC`) e falta de memória na descompressão em memória.
const IZ_EF_TRUNC: i32 = 79;
const PK_MEM4: i32 = 7;

fn word(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn long(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

impl Uz {
    /// Percorre os blocos do campo extra; sem problema, fecha a linha com " OK".
    pub fn test_extra_field(&mut self, mut ef: &[u8]) -> i32 {
        while ef.len() >= EB_HEADSIZE {
            let id = word(ef, 0);
            let len = usize::from(word(ef, 2));
            let rest = ef.len() - EB_HEADSIZE;
            if len > rest {
                self.name_if_quiet();
                self.info(1, format!("bad extra-field entry:\n      EF block length ({len} bytes) exceeds remaining EF data ({rest} bytes)\n"));
                return PK_ERR;
            }
            match id {
                EF_OS2 | EF_ACL | EF_MAC3 | EF_BEOS | EF_ATHEOS => {
                    let offs = match id {
                        EF_MAC3 => {
                            if len >= EB_MAC3_HLEN && word(ef, EB_HEADSIZE + EB_FLGS_OFFS) & EB_M3_FL_UNCMPR != 0 && long(ef, EB_HEADSIZE) as usize == len - EB_MAC3_HLEN {
                                0
                            } else {
                                EB_MAC3_HLEN
                            }
                        }
                        EF_BEOS | EF_ATHEOS => {
                            if len >= EB_BEOS_HLEN && ef[EB_HEADSIZE + EB_FLGS_OFFS] & EB_BE_FL_UNCMPR != 0 && long(ef, EB_HEADSIZE) as usize == len - EB_BEOS_HLEN {
                                0
                            } else {
                                EB_BEOS_HLEN
                            }
                        }
                        _ => EB_OS2_HLEN,
                    };
                    let r = self.test_compr_eb(ef, len, offs);
                    if r != PK_OK {
                        self.name_if_quiet();
                        self.eb_failure(r, len, offs, " compressed EA data missing");
                        return r;
                    }
                }
                EF_NTSD => {
                    let r = if len < EB_NTSD_L_LEN {
                        IZ_EF_TRUNC
                    } else if ef[EB_HEADSIZE + EB_NTSD_VERSION] > EB_NTSD_MAX_VER {
                        PK_WARN | 0x4000
                    } else {
                        // Sem o teste do descritor de segurança, que só existe no Windows.
                        self.test_compr_eb(ef, len, EB_NTSD_L_LEN)
                    };
                    if r != PK_OK {
                        self.name_if_quiet();
                        if r == PK_WARN | 0x4000 {
                            self.info(1, format!(" unsupported NTSD EAs version {}\n", ef[EB_HEADSIZE + EB_NTSD_VERSION]));
                            return PK_WARN;
                        }
                        self.eb_failure(r, len, EB_NTSD_L_LEN, " compressed WinNT security data missing");
                        return r;
                    }
                }
                EF_PKVMS => {
                    if len < 4 {
                        self.info(1, format!("bad extra-field entry:\n      EF block length ({len} bytes) invalid (< 4)\n"));
                    } else if long(ef, EB_HEADSIZE) != crc32fast::hash(&ef[EB_HEADSIZE + 4..EB_HEADSIZE + len]) {
                        self.info(1, " bad CRC for extended attributes\n");
                    }
                }
                _ => {}
            }
            ef = &ef[len + EB_HEADSIZE..];
        }
        if self.o.qflag == 0 {
            self.info(0, " OK\n");
        }
        PK_OK
    }

    /// Com `-q` o nome ainda não saiu: `"%-22s "` antes do problema.
    fn name_if_quiet(&mut self) {
        if self.o.qflag != 0 {
            let mut m = pad22(&fnfilter(&self.filename));
            m.push(b' ');
            self.info(1, m);
        }
    }

    /// A mensagem de um bloco comprimido que não passou.
    fn eb_failure(&mut self, r: i32, len: usize, offs: usize, trunc: &str) {
        match r {
            IZ_EF_TRUNC => {
                let missing = len as i64 - (offs + EB_CMPRHEADLEN) as i64;
                self.info(1, format!("{trunc} ({missing} bytes)\n"));
            }
            PK_ERR => self.info(1, " invalid compressed data for EAs\n"),
            PK_MEM3 | PK_MEM4 => self.info(1, " out of memory while inflating EAs\n"),
            _ if r & 0xff != PK_ERR => self.info(1, " unknown error on extended attributes\n"),
            _ => {
                let m = (r >> 8) as u16;
                if m == DEFLATED {
                    self.info(1, " bad CRC for extended attributes\n");
                } else {
                    self.info(1, format!(" unknown compression method for EAs ({m})\n"));
                }
            }
        }
    }

    /// Um bloco com dados comprimidos a partir de `offs` (`test_compr_eb`): confere os tamanhos
    /// e descomprime em memória.
    fn test_compr_eb(&mut self, eb: &[u8], size: usize, offs: usize) -> i32 {
        if offs < 4 {
            return PK_OK;
        }
        if size < 4 {
            return IZ_EF_TRUNC;
        }
        let ucsize = long(eb, EB_HEADSIZE) as usize;
        if ucsize == 0 || size <= offs + EB_CMPRHEADLEN {
            return IZ_EF_TRUNC;
        }
        let method = word(eb, EB_HEADSIZE + offs);
        if method == STORED && size != offs + EB_CMPRHEADLEN + ucsize {
            return PK_ERR;
        }
        let src = &eb[EB_HEADSIZE + offs..EB_HEADSIZE + size];
        self.memextract(ucsize, src)
    }

    /// Descomprime um bloco do campo extra que está todo em memória (`memextract`) e confere o
    /// CRC dele. Só o `-t` chega aqui no Unix, então os erros voltam codificados em vez de
    /// mensagem.
    fn memextract(&mut self, tgtsize: usize, src: &[u8]) -> i32 {
        let method = word(src, 0);
        let crc_expected = long(src, 2);
        let data = src[6..].to_vec();
        let mut error = PK_OK;
        let out = match method {
            STORED => {
                if data.len() > tgtsize {
                    error = PK_ERR;
                    Vec::new()
                } else {
                    data
                }
            }
            DEFLATED | ENHDEFLATED => {
                let (r, out) = self.inflate_in_memory(&data, tgtsize, method == ENHDEFLATED);
                if r != 0 {
                    error = if r == 3 { PK_MEM3 } else { PK_ERR };
                }
                out
            }
            _ => {
                error = PK_ERR | (i32::from(method) << 8);
                Vec::new()
            }
        };
        if error == PK_OK && crc32fast::hash(&out) != crc_expected {
            error = PK_ERR | (i32::from(DEFLATED) << 8);
        }
        error
    }
}
