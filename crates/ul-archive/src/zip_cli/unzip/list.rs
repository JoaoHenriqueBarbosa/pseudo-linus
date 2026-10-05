//! A listagem do unzip (`-l`, `-v`) e a data do membro mais novo (`-T`) (list.c).

use super::process::{self, EB_UT_FL_MTIME};
use super::{MSG_STDERR, PK_BADERR, PK_EOF, PK_FIND, PK_OK, PK_WARN, Uz, fileio, text};

/// Métodos de compressão conhecidos, na ordem dos nomes de [`METHOD_NAMES`] (`ComprIDs`).
pub const COMPR_IDS: [u16; 17] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14, 18, 19, 97, 98];
pub const METHOD_NAMES: [&str; 18] = [
    "Stored", "Shrunk", "Reduce1", "Reduce2", "Reduce3", "Reduce4", "Implode", "Token", "Defl:#", "Def64#", "ImplDCL", "BZip2",
    "LZMA", "Terse", "IBMLZ77", "WavPack", "PPMd", "Unk:###",
];

/// Índice do método em [`COMPR_IDS`], ou 17 se desconhecido (`find_compr_idx`).
pub fn find_compr_idx(method: u16) -> usize {
    COMPR_IDS.iter().position(|&m| m == method).unwrap_or(COMPR_IDS.len())
}

/// Nome do método como sai na listagem: o deflate com a letra do nível (N, X, F, S) e os
/// desconhecidos com o número (`Unk:099`, ou `Unk0400` em hexadecimal acima de 999).
pub fn method_name(method: u16, gpbf: u16) -> String {
    let idx = find_compr_idx(method);
    let mut name = METHOD_NAMES[idx].to_string();
    if method == 8 || method == 9 {
        name.replace_range(5..6, &(b"NXFS"[usize::from((gpbf >> 1) & 3)] as char).to_string());
    } else if idx >= COMPR_IDS.len() {
        name = if method <= 999 { format!("Unk:{method:03}") } else { format!("Unk{method:04X}") };
    }
    name
}

/// Fator de compressão em milésimos, negativo quando cresceu (`ratio`).
pub fn ratio(uc: u64, c: u64) -> i64 {
    if uc == 0 {
        return 0;
    }
    let (num, denom) = if uc > 2_000_000 { (1, uc / 1000) } else { (1000, uc) };
    if uc >= c { ((num * (uc - c) + (denom >> 1)) / denom) as i64 } else { -(((num * (c - uc) + (denom >> 1)) / denom) as i64) }
}

/// O fator em porcentagem com o sinal (`" 42%"`, `"-3%"`, `"100%"`).
pub fn cfactor_str(uc: u64, c: u64) -> String {
    let r = ratio(uc, c);
    let (sgn, f) = if r < 0 { ('-', (-r + 5) / 10) } else { (' ', (r + 5) / 10) };
    if f == 100 { "100%".to_string() } else { format!("{sgn}{f}%") }
}

impl Uz {
    /// Escreve o nome do membro filtrado e uma quebra de linha (`fnprint`).
    pub fn fnprint(&mut self) {
        let name = fileio::fnfilter(&self.filename);
        self.info(0, name);
        self.info(0, "\n");
    }

    /// O membro corrente foi pedido na linha de comando e não foi excluído com `-x`.
    pub fn member_selected(&self) -> bool {
        if self.process_all_files {
            return true;
        }
        let sepc = self.o.w_flag.then_some(b'/');
        let hit = |pats: &[Vec<u8>]| pats.iter().any(|p| super::matching::matches(&self.filename, p, self.o.c_flag, sepc));
        (self.pfnames.is_empty() || hit(&self.pfnames)) && !hit(&self.pxnames)
    }

    /// Ano, mês, dia, hora e minuto do membro: do bloco "UT" do diretório central no fuso local,
    /// ou da data DOS.
    pub fn member_time(&self) -> (u32, u32, u32, u32, u32) {
        let dos = self.crec.last_mod_dos_datetime;
        if let Some(ef) = &self.extra_field {
            let (flags, t, _) = process::ef_scan_for_izux(ef, true, dos, true, false);
            if flags & EB_UT_FL_MTIME != 0 {
                let (y, mo, d, h, mi, _) = crate::tz::civil(t.mtime, &self.tz);
                return (y as u32, mo, d, h, mi);
            }
        }
        ((dos >> 25 & 0x7f) + 1980, dos >> 21 & 0x0f, dos >> 16 & 0x1f, dos >> 11 & 0x1f, dos >> 5 & 0x3f)
    }

    /// `unzip -l` e `unzip -v` (`list_files`): uma linha por membro do diretório central e os totais.
    pub fn list_files(&mut self) -> i32 {
        let mut error_in_archive = PK_OK;
        let longhdr = self.o.vflag > 1;
        let (head, rule) = if longhdr {
            (" Length   Method    Size  Cmpr    Date    Time   CRC-32   Name", "--------  ------  ------- ---- ---------- ----- --------  ----")
        } else {
            ("  Length      Date    Time    Name", "---------  ---------- -----   ----")
        };
        if self.o.qflag < 2 {
            if self.o.l_flag != 0 {
                self.info(0, format!("{head} (\"^\" ==> case\n{rule}   conversion)\n"));
            } else {
                self.info(0, format!("{head}\n{rule}\n"));
            }
        }
        let (mut members, mut tot_csize, mut tot_ucsize) = (0u64, 0u64, 0u64);
        let mut j: u64 = 1;
        loop {
            let mut sig = [0u8; 4];
            if self.readbuf(&mut sig) == 0 {
                return PK_EOF;
            }
            self.sig = sig;
            if &sig != process::CENTRAL_HDR_SIG {
                if !self.end_of_central_dir(j - 1) {
                    return PK_BADERR;
                }
                break;
            }
            let error = self.process_cdir_file_hdr();
            if error != PK_OK {
                return error;
            }
            let error = self.read_filename(usize::from(self.crec.filename_length), false);
            if error != PK_OK {
                error_in_archive = error;
                if error > PK_WARN {
                    return error;
                }
            }
            let error = self.read_extra_field(usize::from(self.crec.extra_field_length));
            if error != PK_OK {
                error_in_archive = error;
                if error > PK_WARN {
                    return error;
                }
            }
            if !self.member_selected() {
                let error = self.skip_string(usize::from(self.crec.file_comment_length));
                if error != PK_OK {
                    error_in_archive = error;
                    if error > 1 {
                        return error;
                    }
                }
                j += 1;
                continue;
            }
            let (yr, mo, dy, hh, mm) = self.member_time();
            let mut csiz = self.crec.csize;
            if self.crec.general_purpose_bit_flag & 1 != 0 {
                csiz = csiz.wrapping_sub(12);
            }
            let lc = if self.pinfo.lcflag { '^' } else { ' ' };
            let line = if longhdr {
                format!(
                    "{:8}  {:<7}{:8} {:>4} {yr:02}-{mo:02}-{dy:02} {hh:02}:{mm:02} {:08x} {lc}",
                    self.crec.ucsize,
                    method_name(self.crec.compression_method, self.crec.general_purpose_bit_flag),
                    csiz,
                    cfactor_str(self.crec.ucsize, csiz),
                    self.crec.crc32
                )
            } else {
                format!("{:9}  {yr:02}-{mo:02}-{dy:02} {hh:02}:{mm:02}  {lc}", self.crec.ucsize)
            };
            self.info(0, line);
            self.fnprint();
            let len = usize::from(self.crec.file_comment_length);
            let error = if self.o.qflag == 0 { self.display_string_8(len) } else { self.skip_string(len) };
            if error != PK_OK {
                error_in_archive = error;
                if error > PK_WARN {
                    return error;
                }
            }
            tot_ucsize = tot_ucsize.wrapping_add(self.crec.ucsize);
            tot_csize = tot_csize.wrapping_add(csiz);
            members += 1;
            j += 1;
        }
        if self.o.qflag < 2 {
            let s = if members == 1 { "" } else { "s" };
            let trailer = if longhdr {
                format!(
                    "--------          -------  ---                            -------\n{tot_ucsize:8}         {tot_csize:8} {:>4}                            {members} file{s}\n",
                    cfactor_str(tot_ucsize, tot_csize)
                )
            } else {
                format!("---------                     -------\n{tot_ucsize:9}                     {members} file{s}\n")
            };
            self.info(0, trailer);
        }
        if error_in_archive <= PK_WARN {
            if !self.at_end_sig() {
                self.info(MSG_STDERR, "\nnote:  didn't find end-of-central-dir signature at end of central dir.\n");
                error_in_archive = PK_WARN;
            }
            if members == 0 && error_in_archive <= PK_WARN {
                error_in_archive = PK_FIND;
            }
        }
        error_in_archive
    }

    /// `unzip -T` (`get_time_stamp`): só o diretório central, atrás da data mais nova entre os
    /// membros pedidos que não são diretórios. A data e quantos membros contaram ficam em
    /// `time_stamp` desde já, como os ponteiros do C: um erro no meio ainda carimba o arquivo.
    pub fn get_time_stamp(&mut self) -> i32 {
        let mut error_in_archive = PK_OK;
        self.time_stamp = (0, 0);
        let mut j: u64 = 1;
        loop {
            let mut sig = [0u8; 4];
            if self.readbuf(&mut sig) == 0 {
                return PK_EOF;
            }
            self.sig = sig;
            if &sig != process::CENTRAL_HDR_SIG {
                // Aqui o C compara sempre módulo 2^16, mesmo com Zip64.
                if (j - 1) & 0xffff == self.ecrec.total_entries_central_dir & 0xffff {
                    break;
                }
                self.info(MSG_STDERR, format!("error:  expected central file header signature not found (file #{j}).\n"));
                self.info(MSG_STDERR, text::REPORT_MSG);
                return PK_BADERR;
            }
            let error = self.process_cdir_file_hdr();
            if error != PK_OK {
                return error;
            }
            for error in [
                self.read_filename(usize::from(self.crec.filename_length), false),
                self.read_extra_field(usize::from(self.crec.extra_field_length)),
            ] {
                if error != PK_OK {
                    error_in_archive = error;
                    if error > PK_WARN {
                        return error;
                    }
                }
            }
            if self.member_selected() && self.filename.last() != Some(&b'/') {
                let dos = self.crec.last_mod_dos_datetime;
                let ut = self.extra_field.as_deref().map(|ef| process::ef_scan_for_izux(ef, true, dos, true, false));
                let modtime = match ut {
                    Some((flags, t, _)) if flags & EB_UT_FL_MTIME != 0 => t.mtime,
                    _ => self.dos_to_unix_time(dos),
                };
                let (last, n) = &mut self.time_stamp;
                *last = (*last).max(modtime);
                *n += 1;
            }
            let error = self.skip_string(usize::from(self.crec.file_comment_length));
            if error != PK_OK {
                error_in_archive = error;
                if error > 1 {
                    return error;
                }
            }
            j += 1;
        }
        if &self.sig != b"PK\x05\x06" {
            self.info(MSG_STDERR, "\nnote:  didn't find end-of-central-dir signature at end of central dir.\n");
            error_in_archive = PK_WARN;
        }
        if self.time_stamp.1 == 0 && error_in_archive <= PK_WARN {
            error_in_archive = PK_FIND;
        }
        error_in_archive
    }

    /// Acabaram as assinaturas de cabeçalho central depois de `count` membros: confere com o total
    /// do registro de fim (módulo 2^16, ou 2^64 com Zip64) e reclama se não bate.
    pub fn end_of_central_dir(&mut self, count: u64) -> bool {
        let mask = if self.ecrec.have_ecr64 { u64::MAX } else { 0xffff };
        if count & mask == self.ecrec.total_entries_central_dir {
            return true;
        }
        self.info(MSG_STDERR, format!("error:  expected central file header signature not found (file #{}).\n", count + 1));
        self.info(MSG_STDERR, text::REPORT_MSG);
        false
    }

    /// A última assinatura lida é a do registro de fim (comum ou Zip64).
    pub fn at_end_sig(&self) -> bool {
        let want: &[u8; 4] = if self.ecrec.have_ecr64 { b"PK\x06\x06" } else { b"PK\x05\x06" };
        &self.sig == want || self.ecrec.is_zip64_archive || &self.sig == b"PK\x05\x06"
    }
}
