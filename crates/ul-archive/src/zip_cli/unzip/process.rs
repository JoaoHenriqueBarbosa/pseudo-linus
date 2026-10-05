//! O laço sobre os arquivos zip e a localização do diretório central (process.c), mais o leitor do
//! arquivo (`seek_zipf`, `readbuf` e `readbyte` do fileio.c).
//!
//! O leitor reproduz o do C bloco a bloco: lê de 8192 em 8192 bytes alinhados no arquivo, e o que
//! é achado ou não (uma assinatura que cruza a fronteira de um bloco, um registro truncado) depende
//! disso.

use sysabi::Fd;

use super::{IZ_DIR, MSG_STDERR, PK_BADERR, PK_EOF, PK_ERR, PK_FIND, PK_NOZIP, PK_OK, PK_WARN, Uz, fileio, matching, text};

/// Tamanho do buffer de entrada (`INBUFSIZ`).
pub const INBUFSIZ: usize = 8192;

/// Tamanhos dos registros de fim sem a assinatura.
const ECREC_SIZE: i64 = 18;
const ECLOC64_SIZE: usize = 16;
const ECREC64_SIZE: usize = 52;
/// Cabeçalho central e local sem a assinatura.
const CREC_SIZE: usize = 42;
pub const LREC_SIZE: usize = 26;

pub const LOCAL_HDR_SIG: &[u8; 4] = b"PK\x03\x04";
pub const CENTRAL_HDR_SIG: &[u8; 4] = b"PK\x01\x02";
const END_CENTRAL_SIG: &[u8; 4] = b"PK\x05\x06";
const END_CENTLOC64_SIG: &[u8; 4] = b"PK\x06\x07";
const END_CENTRAL64_SIG: &[u8; 4] = b"PK\x06\x06";

const CENT_DIR_END_SIG_NOT_FOUND: &str = "  End-of-central-directory signature not found.  Either this file is not\n  a zipfile, or it constitutes one disk of a multi-part archive.  In the\n  latter case the central directory and zipfile comment will be found on\n  the last disk(s) of this archive.\n";
const CENT64_END_SIG_SEARCH_ERR: &str = "fatal error: read failure while seeking for End-of-centdir-64 signature.\n  This zipfile is corrupt.\n";
const CENT64_END_SIG_SEARCH_OFF: &str = "error: End-of-centdir-64 signature not where expected (prepended bytes?)\n  (attempting to process anyway)\n";

/// O registro de fim do diretório central (`ecdir_rec`), já com os valores do Zip64 quando há.
#[derive(Default, Clone)]
pub struct Ecrec {
    pub number_this_disk: u64,
    pub num_disk_start_cdir: u64,
    pub num_entries_centrl_dir_ths_disk: u64,
    pub total_entries_central_dir: u64,
    pub size_central_directory: u64,
    pub offset_start_central_directory: u64,
    pub zipfile_comment_length: u16,
    pub ec_start: i64,
    pub ec_end: i64,
    pub ec64_start: i64,
    pub ec64_end: i64,
    pub have_ecr64: bool,
    pub is_zip64_archive: bool,
}

/// Um cabeçalho do diretório central (`cdir_file_hdr`), com os tamanhos e o offset já
/// substituídos pelos do Zip64 quando o campo extra traz.
#[derive(Default, Clone)]
pub struct Crec {
    pub version_made_by: [u8; 2],
    pub version_needed_to_extract: [u8; 2],
    pub general_purpose_bit_flag: u16,
    pub compression_method: u16,
    pub last_mod_dos_datetime: u32,
    pub crc32: u32,
    pub csize: u64,
    pub ucsize: u64,
    pub filename_length: u16,
    pub extra_field_length: u16,
    pub file_comment_length: u16,
    pub disk_number_start: u64,
    pub internal_file_attributes: u16,
    pub external_file_attributes: u32,
    pub relative_offset_local_header: u64,
}

/// Um cabeçalho local (`local_file_hdr`).
#[derive(Default, Clone)]
pub struct Lrec {
    pub version_needed_to_extract: [u8; 2],
    pub general_purpose_bit_flag: u16,
    pub compression_method: u16,
    pub last_mod_dos_datetime: u32,
    pub crc32: u32,
    pub csize: u64,
    pub ucsize: u64,
    pub filename_length: u16,
    pub extra_field_length: u16,
}

/// Sistemas de origem (`version_made_by[1]`, os `*_` do unzpriv.h).
pub const FS_FAT: u8 = 0;
pub const AMIGA: u8 = 1;
pub const VMS: u8 = 2;
pub const UNIX: u8 = 3;
pub const VM_CMS: u8 = 4;
pub const ATARI: u8 = 5;
pub const FS_HPFS: u8 = 6;
pub const CPM: u8 = 9;
pub const TOPS20: u8 = 10;
pub const FS_NTFS: u8 = 11;
pub const QDOS: u8 = 12;
pub const ACORN: u8 = 13;
pub const FS_VFAT: u8 = 14;
pub const MVS: u8 = 15;
pub const BEOS: u8 = 16;
pub const TANDEM: u8 = 17;
pub const THEOS: u8 = 18;
pub const ATHEOS: u8 = 30;
pub const NUM_HOSTS: u8 = 31;

/// O que se sabe de um membro além do cabeçalho (`min_info`).
#[derive(Default, Clone)]
pub struct MinInfo {
    pub hostver: u8,
    pub hostnum: u8,
    /// Passar o nome pra minúsculas (`-L`).
    pub lcflag: bool,
    pub vollabel: bool,
    pub has_ux_att: bool,
    pub gpf_is_utf8: bool,
    pub symlink: bool,
    pub encrypted: bool,
    pub textmode: bool,
    pub textfile: bool,
    pub file_attr: u32,
    pub offset: i64,
    pub zip64: bool,
    /// O que o `store_info` copia do cabeçalho central pra extração: descritor de dados depois dos
    /// dados (bit 3), CRC e tamanhos, disco inicial e o nome central (pra comparar com o local).
    pub ext_loc_hdr: bool,
    pub crc: u32,
    pub compr_size: u64,
    pub uncompr_size: u64,
    pub diskstart: u64,
    pub cfilname: Option<Vec<u8>>,
}

/// O arquivo zip aberto e o buffer de entrada (`G.zipfd`, `G.inbuf`, `G.inptr`, `G.incnt`...).
pub struct ZipIn {
    pub fd: Option<Fd>,
    /// A posição do descritor: o `read()` do C avança ela e o `lseek()` muda.
    pub pos: u64,
    pub ziplen: u64,
    /// O `inbuf`, com os 4 bytes de `hold` no fim (onde a busca de trás pra frente enxerga o começo
    /// do bloco seguinte).
    pub buf: Vec<u8>,
    pub inptr: usize,
    pub incnt: i64,
    /// Posição no arquivo do começo do buffer (`cur_zipfile_bufstart`).
    pub bufstart: i64,
    /// Bytes a mais (ou a menos, negativo) antes do arquivo zip propriamente dito.
    pub extra_bytes: i64,
    /// O resto do buffer depois dos dados do membro, guardado pelo `defer_leftover_input`.
    pub incnt_leftover: i64,
    pub inptr_leftover: usize,
}

impl Default for ZipIn {
    fn default() -> ZipIn {
        ZipIn { fd: None, pos: 0, ziplen: 0, buf: vec![0; INBUFSIZ + 4], inptr: 0, incnt: 0, bufstart: 0, extra_bytes: 0, incnt_leftover: 0, inptr_leftover: 0 }
    }
}

impl ZipIn {
    /// O `read(zipfd, dst, n)`: lê da posição corrente até encher ou acabar o arquivo; -1 em erro.
    fn read_at_pos(&mut self, dst: &mut [u8]) -> i64 {
        let Some(fd) = self.fd else { return -1 };
        let sys = super::sys();
        let mut got = 0;
        while got < dst.len() {
            match sys.pread(fd, &mut dst[got..], self.pos + got as u64) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(sysabi::Errno::EINTR) => {}
                Err(_) if got == 0 => return -1,
                Err(_) => break,
            }
        }
        self.pos += got as u64;
        got as i64
    }

    /// `read()` pro começo do `inbuf`.
    pub fn read_inbuf(&mut self, n: usize) -> i64 {
        let mut tmp = std::mem::take(&mut self.buf);
        let r = self.read_at_pos(&mut tmp[..n]);
        self.buf = tmp;
        r
    }

    /// `lseek(zipfd, off, SEEK_SET)`.
    fn lseek(&mut self, off: i64) -> i64 {
        self.pos = off.max(0) as u64;
        off
    }

    /// Fecha o arquivo (`CLOSE_INFILE`).
    pub fn close(&mut self) {
        if let Some(fd) = self.fd.take() {
            let _ = sysabi::sys::close(fd);
        }
    }

    /// A posição de leitura no arquivo (`cur_zipfile_bufstart + (inptr - inbuf)`).
    pub fn tell(&self) -> i64 {
        self.bufstart + self.inptr as i64
    }

    /// Volta a um ponto salvo (`bufstart`, `inptr`, `incnt`), relendo o bloco inteiro como o C faz
    /// depois de cada lote de membros.
    pub fn restore(&mut self, bufstart: i64, inptr: usize, incnt: i64) {
        self.bufstart = self.lseek(bufstart);
        self.read_inbuf(INBUFSIZ);
        self.inptr = inptr;
        self.incnt = incnt;
    }

    /// Esquece o bloco em memória: o próximo `seek_zipf` relê do arquivo.
    pub fn invalidate(&mut self) {
        self.bufstart = -1;
        self.incnt = 0;
    }

    /// Posiciona no offset absoluto `request` (já com os bytes extras), relendo o bloco só se ele
    /// não for o que está em memória; `false` se a leitura não trouxe nada (a busca embutida em
    /// `extract_or_test_entrylist`).
    pub fn seek_raw(&mut self, request: i64) -> bool {
        let inbuf_offset = request % INBUFSIZ as i64;
        let bufstart = request - inbuf_offset;
        if bufstart != self.bufstart {
            self.bufstart = self.lseek(bufstart);
            self.incnt = self.read_inbuf(INBUFSIZ);
            if self.incnt <= 0 {
                return false;
            }
            self.incnt -= inbuf_offset;
        } else {
            self.incnt += self.inptr as i64 - inbuf_offset;
        }
        self.inptr = inbuf_offset as usize;
        true
    }
}

impl Uz {
    /// Processa cada arquivo que casa com o nome pedido; se só um foi tentado e não existe, tenta
    /// de novo com `.zip` e `.ZIP`. Com curinga no nome, termina com o resumo (`process_zipfiles`).
    pub fn process_zipfiles(&mut self) -> i32 {
        let (mut win, mut lose, mut warn, mut miss_dirs, mut miss_files) = (0, 0, 0, 0, 0);
        let mut error = 0;
        let mut error_in_archive = 0;
        let mut lastzipfn = None;
        // `-n` ganha de `-o`: das duas, a mais segura.
        self.x.overwrite_mode = if self.o.overwrite_none {
            super::extract::Overwrite::Never
        } else if self.o.overwrite_all != 0 {
            super::extract::Overwrite::Always
        } else {
            super::extract::Overwrite::Query
        };
        for name in fileio::do_wild(&self.wildzipfn) {
            self.zipfn = name.clone();
            lastzipfn = Some(name);
            if self.o.qflag == 0
                && error != PK_NOZIP
                && error != IZ_DIR
                && (!self.o.t_flag || self.o.zipinfo_mode)
                && win + lose + warn + miss_files > 0
            {
                self.info(0, "\n");
            }
            error = self.do_seekable(false);
            match error {
                PK_WARN => warn += 1,
                IZ_DIR => miss_dirs += 1,
                PK_NOZIP => miss_files += 1,
                PK_OK => win += 1,
                _ => lose += 1,
            }
            if error != IZ_DIR && error > error_in_archive {
                error_in_archive = error;
            }
        }
        if win + warn + lose == 0 && miss_dirs + miss_files == 1
            && let Some(last) = lastzipfn {
                (miss_dirs, miss_files) = (0, 0);
                error_in_archive = PK_OK;
                self.zipfn = [last.as_slice(), b".zip"].concat();
                error = self.do_seekable(false);
                if error == PK_NOZIP || error == IZ_DIR {
                    if error == IZ_DIR {
                        miss_dirs += 1;
                    }
                    self.zipfn = [last.as_slice(), b".ZIP"].concat();
                    error = self.do_seekable(true);
                }
                match error {
                    PK_WARN => warn += 1,
                    IZ_DIR => {
                        miss_dirs += 1;
                        error = PK_NOZIP;
                    }
                    // O C não conta de novo aqui (contaria "1 file had no zipfile directory").
                    PK_NOZIP => {}
                    PK_OK => win += 1,
                    _ => lose += 1,
                }
                error_in_archive = error_in_archive.max(error);
            }
        let o = &self.o;
        if matching::is_wild(&self.wildzipfn) && o.qflag < 3 && !(o.t_flag && !o.zipinfo_mode && o.qflag > 1) {
            if (miss_files + lose + warn > 0 || win != 1) && !(o.t_flag && !o.zipinfo_mode && o.qflag != 0) && !(o.tflag != 0 && o.qflag > 1) {
                self.info(MSG_STDERR, "\n");
            }
            let s = |n: i32| if n == 1 { "" } else { "s" };
            if win > 1 || (win == 1 && miss_dirs + miss_files + lose + warn > 0) {
                self.info(MSG_STDERR, format!("{win} archive{} successfully processed.\n", if win == 1 { " was" } else { "s were" }));
            }
            if warn > 0 {
                self.info(MSG_STDERR, format!("{warn} archive{} had warnings but no fatal errors.\n", s(warn)));
            }
            if lose > 0 {
                self.info(MSG_STDERR, format!("{lose} archive{} had fatal errors.\n", s(lose)));
            }
            if miss_files > 0 {
                self.info(MSG_STDERR, format!("{miss_files} file{} had no zipfile directory.\n", s(miss_files)));
            }
            if miss_dirs == 1 {
                self.info(MSG_STDERR, "1 \"zipfile\" was a directory.\n");
            } else if miss_dirs > 0 {
                self.info(MSG_STDERR, format!("{miss_dirs} \"zipfiles\" were directories.\n"));
            }
            if win + lose + warn == 0 {
                self.info(MSG_STDERR, "No zipfiles found.\n");
            }
        }
        error_in_archive
    }

    /// Processa um arquivo zip (`do_seekable`). `lastchance` é a última tentativa de nome, a que
    /// reclama de não achar o arquivo.
    fn do_seekable(&mut self, lastchance: bool) -> i32 {
        self.time_stamp = (0, 0);
        let st = match sysabi::sys::stat(&self.zipfn) {
            Ok(st) if st.file_type() != sysabi::FileType::Directory => st,
            other => {
                let is_dir = other.is_ok();
                if lastchance && self.o.qflag < 3 {
                    let prog: &[u8] = if self.o.zipinfo_mode { b"zipinfo" } else { b"unzip" };
                    let w = self.wildzipfn.clone();
                    let msg = if self.no_ecrec {
                        let pad: &[u8] = if self.o.zipinfo_mode { b"  " } else { b"" };
                        [prog, b":  cannot find zipfile directory in one of ", &w, b" or\n        ", pad, &w, b".zip, and cannot find ", &self.zipfn, b", period.\n"].concat()
                    } else {
                        [prog, b":  cannot find or open ", &w, b", ", &w, b".zip or ", &self.zipfn, b".\n"].concat()
                    };
                    self.info(MSG_STDERR, msg);
                }
                return if is_dir { IZ_DIR } else { PK_NOZIP };
            }
        };
        self.zin.ziplen = st.size;
        let maybe_exe = st.mode & 0o100 != 0;
        match sysabi::sys::open(&self.zipfn, sysabi::OFlags::RDONLY, 0) {
            Ok(fd) => self.zin.fd = Some(fd),
            Err(e) => {
                let msg = [b"error:  cannot open zipfile [ ".as_slice(), &self.zipfn, b" ]\n        ", e.message().as_bytes(), b"\n"].concat();
                self.info(MSG_STDERR, msg);
                return PK_NOZIP;
            }
        }
        self.zin.pos = 0;
        self.zin.bufstart = 0;
        self.zin.inptr = 0;
        let o = &self.o;
        if (!o.zipinfo_mode && o.qflag == 0 && !o.t_flag) || (o.zipinfo_mode && o.hflag != 0) {
            let msg = [b"Archive:  ".as_slice(), &self.zipfn, b"\n"].concat();
            self.info(0, msg);
        }
        let ziplen = self.zin.ziplen as i64;
        let searchlen = if self.o.zipinfo_mode { ziplen } else { ziplen.min(66000) };
        let mut error_in_archive = self.find_ecrec(searchlen);
        if error_in_archive > PK_WARN {
            self.zin.close();
            if maybe_exe {
                let msg = [b"note:  ".as_slice(), &self.zipfn, b" may be a plain executable, not an archive\n"].concat();
                self.info(MSG_STDERR, msg);
            }
            if lastchance {
                return error_in_archive;
            }
            self.no_ecrec = true;
            return PK_NOZIP;
        }
        if self.o.zflag > 0 && !self.o.zipinfo_mode {
            self.zin.close();
            return error_in_archive;
        }
        let r = self.check_ecrec(error_in_archive);
        error_in_archive = r;
        self.zin.close();
        let (stamp, nmember) = self.time_stamp;
        if self.o.t_flag && !self.o.zipinfo_mode && nmember > 0 {
            // `stamp_file`: `utime` com acesso e modificação iguais.
            let zipfn = self.zipfn.clone();
            if super::unix::set_times(&zipfn, stamp, stamp, sysabi::AtFlags::empty()).is_err() {
                if self.o.qflag < 3 {
                    self.info(0x201, [b"warning:  cannot set time for ".as_slice(), &zipfn, b"\n"].concat());
                }
                error_in_archive = error_in_archive.max(PK_WARN);
            } else if self.o.qflag == 0 {
                self.info(0, [b"Updated time stamp for ".as_slice(), &zipfn, b".\n"].concat());
            }
        }
        error_in_archive
    }

    /// Confere o registro de fim (disco, bytes a mais ou a menos, arquivo vazio), acha o começo do
    /// diretório central e processa os membros: o miolo do `do_seekable`.
    fn check_ecrec(&mut self, mut error_in_archive: i32) -> i32 {
        let zipfn = self.zipfn.clone();
        let e = self.ecrec.clone();
        let multi = !self.o.zipinfo_mode && e.number_this_disk != 0;
        if self.o.zipinfo_mode && e.number_this_disk != e.num_disk_start_cdir {
            if e.number_this_disk > e.num_disk_start_cdir {
                let msg = format!(
                    "\n   [{}]:\n     Zipfile is disk {} of a multi-disk archive, and this is not the disk on\n     which the central zipfile directory begins (disk {}).\n",
                    String::from_utf8_lossy(&zipfn),
                    e.number_this_disk,
                    e.num_disk_start_cdir
                );
                self.info(MSG_STDERR, msg);
                return PK_FIND;
            }
            let msg = format!(
                "\nwarning [{}]:  end-of-central-directory record claims this\n  is disk {} but that the central directory starts on disk {}; this is a\n  contradiction.  Attempting to process anyway.\n",
                String::from_utf8_lossy(&zipfn),
                e.number_this_disk,
                e.num_disk_start_cdir
            );
            self.info(MSG_STDERR, msg);
            error_in_archive = PK_WARN;
        }
        if multi {
            let msg = [b"warning [".as_slice(), &zipfn, b"]:  zipfile claims to be last disk of a multi-part archive;\n  attempting to process anyway, assuming all parts have been concatenated\n  together in order.  Expect \"errors\" and warnings...true multi-part support\n  doesn't exist yet (coming soon).\n"].concat();
            self.info(MSG_STDERR, msg);
            error_in_archive = PK_WARN;
        }
        self.zin.extra_bytes = self.real_ecrec_offset - self.expect_ecrec_offset;
        if self.zin.extra_bytes < 0 {
            let msg = [b"error [".as_slice(), &zipfn, format!("]:  missing {} bytes in zipfile\n  (attempting to process anyway)\n", -self.zin.extra_bytes).as_bytes()].concat();
            self.info(MSG_STDERR, msg);
            error_in_archive = PK_ERR;
        } else if self.zin.extra_bytes > 0 {
            if e.offset_start_central_directory == 0 && e.size_central_directory != 0 {
                let msg = [b"error [".as_slice(), &zipfn, b"]:  NULL central directory offset\n  (attempting to process anyway)\n"].concat();
                self.info(MSG_STDERR, msg);
                self.ecrec.offset_start_central_directory = self.zin.extra_bytes as u64;
                self.zin.extra_bytes = 0;
                error_in_archive = PK_ERR;
            } else {
                let n = self.zin.extra_bytes;
                let msg = [b"warning [".as_slice(), &zipfn, format!("]:  {n} extra byte{} at beginning or within zipfile\n  (attempting to process anyway)\n", if n == 1 { "" } else { "s" }).as_bytes()].concat();
                self.info(MSG_STDERR, msg);
                error_in_archive = PK_WARN;
            }
        }
        if self.expect_ecrec_offset == 0 && e.size_central_directory == 0 {
            if self.o.zipinfo_mode {
                let pad = if self.o.lflag > 9 { "\n  " } else { "" };
                self.info(0, format!("{pad}Empty zipfile.\n"));
            } else {
                let msg = [b"warning [".as_slice(), &zipfn, b"]:  zipfile is empty\n"].concat();
                self.info(MSG_STDERR, msg);
            }
            return error_in_archive.max(PK_WARN);
        }
        let cdir = self.ecrec.offset_start_central_directory as i64;
        let error = self.seek_zipf(cdir);
        if error == PK_BADERR {
            return PK_BADERR;
        }
        let mut sig = [0u8; 4];
        if error != PK_OK || self.readbuf(&mut sig) == 0 || &sig != CENTRAL_HDR_SIG {
            // Talvez o tamanho do diretório central esteja errado (STZip, ZIPSPLIT): tenta sem os
            // bytes a mais.
            let tmp = self.zin.extra_bytes;
            self.zin.extra_bytes = 0;
            let error = self.seek_zipf(cdir);
            if error != PK_OK || self.readbuf(&mut sig) == 0 || &sig != CENTRAL_HDR_SIG {
                if error != PK_BADERR {
                    let msg = [b"error [".as_slice(), &zipfn, b"]:  start of central directory not found;\n  zipfile corrupt.\n", text::REPORT_MSG.as_bytes()].concat();
                    self.info(MSG_STDERR, msg);
                }
                return if error != PK_OK { error } else { PK_BADERR };
            }
            let msg = [b"error [".as_slice(), &zipfn, format!("]:  reported length of central directory is\n  {} bytes too long (Atari STZip zipfile?  J.H.Holm ZIPSPLIT 1.1\n  zipfile?).  Compensating...\n", -tmp).as_bytes()].concat();
            self.info(MSG_STDERR, msg);
            error_in_archive = PK_ERR;
        }
        let error = self.seek_zipf(cdir);
        if error != PK_OK {
            return error;
        }
        error_in_archive.max(self.process_members())
    }

    /// Lista, mostra no formato do zipinfo, data do mais novo (`-T`), ou extrai e testa.
    fn process_members(&mut self) -> i32 {
        if self.o.zipinfo_mode {
            return self.zipinfo();
        }
        if self.o.t_flag {
            return self.get_time_stamp();
        }
        if self.o.vflag != 0 && self.o.tflag == 0 && !self.o.cflag {
            return self.list_files();
        }
        self.extract_or_test_files()
    }

    /// Lê o próximo cabeçalho central depois da assinatura (`get_cdir_ent`).
    fn get_cdir_ent(&mut self) -> i32 {
        let mut b = [0u8; CREC_SIZE];
        if self.readbuf(&mut b) == 0 {
            return PK_EOF;
        }
        let word = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let long = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        self.crec = Crec {
            version_made_by: [b[0], b[1]],
            version_needed_to_extract: [b[2], b[3]],
            general_purpose_bit_flag: word(4),
            compression_method: word(6),
            last_mod_dos_datetime: long(8),
            crc32: long(12),
            csize: u64::from(long(16)),
            ucsize: u64::from(long(20)),
            filename_length: word(24),
            extra_field_length: word(26),
            file_comment_length: word(28),
            disk_number_start: u64::from(word(30)),
            internal_file_attributes: word(32),
            external_file_attributes: long(34),
            relative_offset_local_header: u64::from(long(38)),
        };
        PK_OK
    }

    /// Lê o cabeçalho central e deduz o que vale pra ele: sistema de origem, conversão pra
    /// minúsculas, rótulo de volume, nome em UTF-8 (`process_cdir_file_hdr`).
    pub fn process_cdir_file_hdr(&mut self) -> i32 {
        let error = self.get_cdir_ent();
        if error != PK_OK {
            return error;
        }
        let c = &mut self.crec;
        let p = &mut self.pinfo;
        p.hostver = c.version_made_by[0];
        p.hostnum = c.version_made_by[1].min(NUM_HOSTS);
        p.lcflag = match self.o.l_flag {
            1 => matches!(p.hostnum, FS_FAT | CPM | VM_CMS | MVS | TANDEM | TOPS20 | VMS),
            n => n > 1,
        };
        // O bit de verificação da PKWARE: só o último byte dos atributos externos vale.
        if c.internal_file_attributes & 0x0004 != 0 {
            c.external_file_attributes &= 0xff;
        }
        p.vollabel = c.external_file_attributes & 0x08 != 0 && matches!(p.hostnum, FS_FAT | FS_HPFS | FS_NTFS | ATARI);
        if p.vollabel {
            p.lcflag = false;
        }
        p.has_ux_att = c.external_file_attributes & 0xffff_0000 != 0;
        p.gpf_is_utf8 = c.general_purpose_bit_flag & (1 << 11) != 0;
        p.symlink = false;
        PK_OK
    }
    /// Procura `sig` de trás pra frente no buffer, de `from` até o começo; acha também a que cruza o
    /// fim do buffer, pelos bytes de `hold`. Ajusta `inptr` e `incnt` como o C.
    fn scan_back(&mut self, from: i64, sig: &[u8; 4]) -> bool {
        let z = &mut self.zin;
        let mut i = from;
        while i >= 0 {
            let p = i as usize;
            if &z.buf[p..p + 4] == sig {
                z.inptr = p;
                z.incnt -= i;
                return true;
            }
            i -= 1;
        }
        false
    }

    /// Busca um registro de fim nos últimos `searchlen` bytes, um bloco de 8192 por vez a partir do
    /// fim do arquivo: 0 achou, 1 não achou, 2 erro de leitura (`rec_find`).
    fn rec_find(&mut self, searchlen: i64, sig: &[u8; 4], rec_size: i64) -> i32 {
        let blk = INBUFSIZ as i64;
        let ziplen = self.zin.ziplen as i64;
        let tail_len = ziplen % blk;
        let mut found = false;
        if tail_len > rec_size {
            let z = &mut self.zin;
            z.bufstart = z.lseek(ziplen - tail_len);
            z.incnt = z.read_inbuf(tail_len as usize);
            if z.incnt != tail_len {
                return 2;
            }
            found = self.scan_back(tail_len - (rec_size + 4), sig);
            self.zin.buf.copy_within(0..3, INBUFSIZ);
        } else {
            self.zin.bufstart = ziplen - tail_len;
        }
        let numblks = (searchlen - tail_len + blk - 1) / blk;
        let mut i = 1;
        while !found && i <= numblks {
            let z = &mut self.zin;
            z.bufstart -= blk;
            z.lseek(z.bufstart);
            z.incnt = z.read_inbuf(INBUFSIZ);
            if z.incnt != blk {
                return 2;
            }
            found = self.scan_back(blk - 1, sig);
            self.zin.buf.copy_within(0..3, INBUFSIZ);
            i += 1;
        }
        if found { 0 } else { 1 }
    }

    /// Acha e lê o registro de fim (e o do Zip64, se houver) e mostra o comentário do arquivo
    /// (`find_ecrec`).
    fn find_ecrec(&mut self, searchlen: i64) -> i32 {
        let ziplen = self.zin.ziplen as i64;
        let found = if ziplen <= INBUFSIZ as i64 {
            let z = &mut self.zin;
            z.lseek(0);
            z.incnt = z.read_inbuf(ziplen as usize);
            z.incnt == ziplen && self.scan_back(ziplen - (ECREC_SIZE + 4), END_CENTRAL_SIG)
        } else {
            self.rec_find(searchlen, END_CENTRAL_SIG, ECREC_SIZE) == 0
        };
        if !found {
            if self.o.qflag != 0 || self.o.zipinfo_mode {
                let msg = [b"[".as_slice(), &self.zipfn, b"]\n"].concat();
                self.info(MSG_STDERR, msg);
            }
            self.info(MSG_STDERR, CENT_DIR_END_SIG_NOT_FOUND);
            return PK_ERR;
        }
        self.real_ecrec_offset = self.zin.bufstart + self.zin.inptr as i64;
        let mut rec = [0u8; ECREC_SIZE as usize + 4];
        if self.readbuf(&mut rec) == 0 {
            return PK_EOF;
        }
        let word = |o: usize| u64::from(u16::from_le_bytes([rec[o], rec[o + 1]]));
        let long = |o: usize| u64::from(u32::from_le_bytes([rec[o], rec[o + 1], rec[o + 2], rec[o + 3]]));
        self.ecrec = Ecrec {
            number_this_disk: word(4),
            num_disk_start_cdir: word(6),
            num_entries_centrl_dir_ths_disk: word(8),
            total_entries_central_dir: word(10),
            size_central_directory: long(12),
            offset_start_central_directory: long(16),
            zipfile_comment_length: word(20) as u16,
            ec_start: self.real_ecrec_offset,
            ec_end: self.real_ecrec_offset + 22 + word(20) as i64,
            ..Ecrec::default()
        };
        let error_in_archive = self.process_zip_cmmnt();
        if error_in_archive > PK_WARN {
            return error_in_archive;
        }
        let result = self.find_ecrec64(searchlen + 76);
        if result != PK_OK {
            return error_in_archive.max(result);
        }
        self.expect_ecrec_offset = (self.ecrec.offset_start_central_directory + self.ecrec.size_central_directory) as i64;
        if self.o.zipinfo_mode {
            self.zi_end_central();
        }
        error_in_archive
    }

    /// Mostra o comentário do arquivo quando é o caso (`process_zip_cmmnt`): sempre no zipinfo
    /// detalhado (`-v`), com `-z`, e no unzip sem `-q` nem `-T`.
    fn process_zip_cmmnt(&mut self) -> i32 {
        let len = usize::from(self.ecrec.zipfile_comment_length);
        let o = &self.o;
        if o.zipinfo_mode && o.lflag > 9 {
            if len == 0 {
                self.info(0, "There is no zipfile comment.\n");
                return PK_OK;
            }
            self.info(0, format!("The zipfile comment is {len} bytes long and contains the following text:\n"));
            self.info(0, "======================== zipfile comment begins ==========================\n");
            let truncated = self.display_string(len) != PK_OK;
            self.info(0, "========================= zipfile comment ends ===========================\n");
            if truncated {
                self.info(0, "\n  The zipfile comment is truncated.\n");
                return PK_WARN;
            }
            return PK_OK;
        }
        let show = len != 0
            && (o.zipinfo_mode && o.zflag > 0 || !o.zipinfo_mode && (o.zflag > 0 || o.zflag == 0 && !o.t_flag && o.qflag == 0));
        if show && self.display_string(len) != PK_OK {
            self.info(MSG_STDERR, "\ncaution:  zipfile comment truncated\n");
            return PK_WARN;
        }
        PK_OK
    }

    /// O erro fatal da busca do registro Zip64 (com o nome do arquivo antes, em `-q` e no zipinfo).
    fn cent64_error(&mut self, msg: &str) {
        if self.o.qflag != 0 || self.o.zipinfo_mode {
            let line = [b"[".as_slice(), &self.zipfn, b"]\n"].concat();
            self.info(MSG_STDERR, line);
        }
        self.info(MSG_STDERR, msg);
    }

    /// Lê `n` bytes direto do descritor, fora do buffer (o `read()` em `byterec` do C, que mesmo
    /// assim deixa a contagem em `incnt`).
    fn read_raw_at(&mut self, off: i64, n: usize) -> Option<Vec<u8>> {
        let z = &mut self.zin;
        z.bufstart = z.lseek(off);
        let mut rec = vec![0u8; n];
        z.incnt = z.read_at_pos(&mut rec);
        (z.incnt == n as i64).then_some(rec)
    }

    /// Procura o localizador e o registro de fim do Zip64 logo antes do registro de fim comum, e
    /// completa o `ecrec` com eles quando os dois concordam (`find_ecrec64`).
    fn find_ecrec64(&mut self, _searchlen: i64) -> i32 {
        let ecloc64_start = self.real_ecrec_offset - (ECLOC64_SIZE as i64 + 4);
        if ecloc64_start < 0 {
            return PK_OK;
        }
        let Some(loc) = self.read_raw_at(ecloc64_start, ECLOC64_SIZE + 4) else {
            self.cent64_error(CENT64_END_SIG_SEARCH_ERR);
            return PK_ERR;
        };
        if &loc[..4] != END_CENTLOC64_SIG {
            return PK_OK;
        }
        let long = |b: &[u8], o: usize| u64::from(u32::from_le_bytes(b[o..o + 4].try_into().unwrap()));
        let int64 = |b: &[u8], o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
        let start_disk = long(&loc, 4);
        let mut ec64_start = int64(&loc, 8);
        let total_disks = long(&loc, 16);
        let e = &self.ecrec;
        if e.number_this_disk != 0xFFFF && total_disks != 0 && e.number_this_disk != total_disks - 1 {
            return PK_OK;
        }
        if ec64_start > ecloc64_start as u64 {
            self.cent64_error(CENT64_END_SIG_SEARCH_ERR);
            return PK_ERR;
        }
        let Some(mut rec) = self.read_raw_at(ec64_start as i64, ECREC64_SIZE + 4) else {
            self.cent64_error(CENT64_END_SIG_SEARCH_ERR);
            return PK_ERR;
        };
        if &rec[..4] != END_CENTRAL64_SIG {
            // Talvez haja bytes antes do arquivo (um prefixo de autoextraível): tenta logo antes do
            // localizador.
            ec64_start = (ecloc64_start - ECREC64_SIZE as i64 - 4) as u64;
            match self.read_raw_at(ec64_start as i64, ECREC64_SIZE + 4) {
                Some(r) if &r[..4] == END_CENTRAL64_SIG => rec = r,
                _ => {
                    self.cent64_error(CENT64_END_SIG_SEARCH_ERR);
                    return PK_ERR;
                }
            }
            self.cent64_error(CENT64_END_SIG_SEARCH_OFF);
        }
        if long(&rec, 16) != start_disk {
            return PK_OK;
        }
        let disk_cdstart = long(&rec, 20);
        let this_entries = int64(&rec, 24);
        let tot_entries = int64(&rec, 32);
        let cdirsize = int64(&rec, 40);
        let offs_cdstart = int64(&rec, 48);
        let e = &mut self.ecrec;
        let agrees = |v: u64, all_ones: u64, v64: u64| v == all_ones || v == v64;
        if !agrees(e.num_disk_start_cdir, 0xFFFF, disk_cdstart)
            || !agrees(e.num_entries_centrl_dir_ths_disk, 0xFFFF, this_entries)
            || !agrees(e.total_entries_central_dir, 0xFFFF, tot_entries)
            || !agrees(e.size_central_directory, 0xFFFF_FFFF, cdirsize)
            || !agrees(e.offset_start_central_directory, 0xFFFF_FFFF, offs_cdstart)
        {
            return PK_OK;
        }
        e.have_ecr64 = true;
        e.ec_start -= ECLOC64_SIZE as i64 + 4;
        e.ec64_start = ec64_start as i64;
        e.ec64_end = ec64_start as i64 + 12 + int64(&rec, 4) as i64;
        self.real_ecrec_offset = ec64_start as i64;
        // Os campos "todos uns" do registro comum valem o do Zip64.
        let mut zip64 = false;
        let mut upgrade = |field: &mut u64, all_ones: u64, v64: u64| {
            if *field == all_ones {
                *field = v64;
                zip64 |= v64 != all_ones;
            }
        };
        upgrade(&mut e.number_this_disk, 0xFFFF, start_disk);
        upgrade(&mut e.num_disk_start_cdir, 0xFFFF, disk_cdstart);
        upgrade(&mut e.num_entries_centrl_dir_ths_disk, 0xFFFF, this_entries);
        upgrade(&mut e.total_entries_central_dir, 0xFFFF, tot_entries);
        upgrade(&mut e.size_central_directory, 0xFFFF_FFFF, cdirsize);
        upgrade(&mut e.offset_start_central_directory, 0xFFFF_FFFF, offs_cdstart);
        e.is_zip64_archive |= zip64;
        PK_OK
    }
    /// Posiciona o buffer no bloco que contém `abs_offset` (mais os `extra_bytes`), relendo só se
    /// for outro bloco (`seek_zipf`).
    pub fn seek_zipf(&mut self, abs_offset: i64) -> i32 {
        let z = &mut self.zin;
        let request = abs_offset + z.extra_bytes;
        let inbuf_offset = request % INBUFSIZ as i64;
        let bufstart = request - inbuf_offset;
        if request < 0 {
            let msg = [b"error [".as_slice(), &self.zipfn, b"]:  attempt to seek before beginning of zipfile\n", text::REPORT_MSG.as_bytes()].concat();
            self.info(MSG_STDERR, msg);
            return PK_BADERR;
        }
        if bufstart != z.bufstart {
            z.bufstart = z.lseek(bufstart);
            z.incnt = z.read_inbuf(INBUFSIZ);
            if z.incnt <= 0 {
                return PK_EOF;
            }
            z.incnt -= inbuf_offset;
        } else {
            z.incnt += z.inptr as i64 - inbuf_offset;
        }
        z.inptr = inbuf_offset as usize;
        PK_OK
    }

    /// Copia `dst.len()` bytes do buffer, relendo blocos; devolve quantos copiou (`readbuf`).
    pub fn readbuf(&mut self, dst: &mut [u8]) -> usize {
        let mut done = 0;
        while done < dst.len() {
            let z = &mut self.zin;
            if z.incnt <= 0 {
                z.incnt = z.read_inbuf(INBUFSIZ);
                if z.incnt == 0 {
                    return done;
                }
                if z.incnt < 0 {
                    self.info(MSG_STDERR, text::READ_ERROR);
                    return 0;
                }
                z.bufstart += INBUFSIZ as i64;
                z.inptr = 0;
            }
            let count = (dst.len() - done).min(z.incnt as usize);
            dst[done..done + count].copy_from_slice(&z.buf[z.inptr..z.inptr + count]);
            z.inptr += count;
            z.incnt -= count as i64;
            done += count;
        }
        done
    }

    /// Tamanhos, offset e disco de 64 bits do bloco Zip64 do campo extra (`getZip64Data`). Como no
    /// C, os limites são conferidos contra o resto do campo extra, não contra o bloco.
    pub fn get_zip64_data(&mut self, ef: &[u8]) -> i32 {
        const Z64FLGS: u64 = 0xffff;
        const Z64FLGL: u64 = 0xffff_ffff;
        self.zip64 = false;
        let mut rest = ef;
        while rest.len() >= EB_HEADSIZE {
            let (id, len) = (ef_word(rest, 0), usize::from(ef_word(rest, 2)));
            if len > rest.len() - EB_HEADSIZE {
                break;
            }
            if id == EF_PKSZ64 {
                let mut off = EB_HEADSIZE;
                let take8 = |off: &mut usize| -> Option<u64> {
                    let v = rest.get(*off..*off + 8)?;
                    *off += 8;
                    Some(u64::from_le_bytes(v.try_into().unwrap()))
                };
                if self.crec.ucsize == Z64FLGL || self.lrec.ucsize == Z64FLGL {
                    let Some(v) = take8(&mut off) else { return PK_ERR };
                    (self.crec.ucsize, self.lrec.ucsize) = (v, v);
                }
                if self.crec.csize == Z64FLGL || self.lrec.csize == Z64FLGL {
                    let Some(v) = take8(&mut off) else { return PK_ERR };
                    (self.csize, self.crec.csize, self.lrec.csize) = (v as i64, v, v);
                }
                if self.crec.relative_offset_local_header == Z64FLGL {
                    let Some(v) = take8(&mut off) else { return PK_ERR };
                    self.crec.relative_offset_local_header = v;
                }
                if self.crec.disk_number_start == Z64FLGS {
                    let Some(v) = rest.get(off..off + 4) else { return PK_ERR };
                    self.crec.disk_number_start = u64::from(u32::from_le_bytes(v.try_into().unwrap()));
                }
            }
            rest = &rest[EB_HEADSIZE + len..];
        }
        PK_OK
    }

    /// O nome em UTF-8 do bloco "Unicode Path" (`getUnicodeData`): `Ok(None)` sem o bloco,
    /// `Ok(Some(vazio))` quando o nome comum já é UTF-8, e `Err` com versão desconhecida ou CRC do
    /// nome comum diferente (o nome foi mudado depois).
    pub fn get_unicode_data(&mut self, ef: &[u8]) -> Result<Option<Vec<u8>>, ()> {
        let mut found = None;
        let mut rest = ef;
        while rest.len() >= EB_HEADSIZE {
            let (id, len) = (ef_word(rest, 0), usize::from(ef_word(rest, 2)));
            if len > rest.len() - EB_HEADSIZE {
                break;
            }
            if id == EF_UNIPATH {
                if rest.get(EB_HEADSIZE).copied().unwrap_or(0) > 1 {
                    self.info(MSG_STDERR, "\nwarning:  Unicode Path version > 1\n");
                    return Err(());
                }
                let sum = rest.get(EB_HEADSIZE + 1..EB_HEADSIZE + 5).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
                let full = &self.filename_full;
                let full = &full[..full.iter().position(|&c| c == 0).unwrap_or(full.len())];
                if crc32fast::hash(full) != sum {
                    self.info(MSG_STDERR, "\nwarning:  Unicode Path checksum invalid\n");
                    return Err(());
                }
                let ulen = (len as u16).wrapping_sub(5) as usize;
                let data = rest.get(EB_HEADSIZE + 5..).unwrap_or_default();
                let data = &data[..ulen.min(data.len())];
                found = Some(data[..data.iter().position(|&c| c == 0).unwrap_or(data.len())].to_vec());
                self.zip64 = true;
            }
            rest = &rest[EB_HEADSIZE + len..];
        }
        Ok(found)
    }
}

/// Cabeçalho de um bloco do campo extra: id e tamanho.
pub const EB_HEADSIZE: usize = 4;
pub const EF_PKSZ64: u16 = 0x0001;
pub const EF_UNIPATH: u16 = 0x7075;
const EF_PKUNIX: u16 = 0x000d;
const EF_IZUNIX: u16 = 0x5855;
const EF_IZUNIX2: u16 = 0x7855;
const EF_IZUNIX3: u16 = 0x7875;
const EF_TIME: u16 = 0x5455;

/// Bits devolvidos pelo [`ef_scan_for_izux`]: as datas presentes e o UID/GID.
pub const EB_UT_FL_MTIME: u32 = 1 << 0;
pub const EB_UT_FL_ATIME: u32 = 1 << 1;
pub const EB_UT_FL_CTIME: u32 = 1 << 2;
pub const EB_UX2_VALID: u32 = 1 << 8;

const DOSTIME_2038_01_18: u32 = 0x7432_0000;

/// Datas Unix de um membro (`iztimes`).
#[derive(Default, Clone, Copy)]
pub struct IzTimes {
    pub atime: i64,
    pub mtime: i64,
    pub ctime: i64,
}

/// Procura nos blocos "UT", "ux", "Ux", "UX" e PKUNIX as datas Unix e o dono (`ef_scan_for_izux`).
/// No cabeçalho central (`central`) o "UT" só traz o mtime. Uma data com o bit 31 ligado só vale
/// se a data DOS for de 2038 em diante; senão o bloco inteiro de datas é ignorado.
pub fn ef_scan_for_izux(ef: &[u8], central: bool, dos_mdatetime: u32, want_times: bool, want_ids: bool) -> (u32, IzTimes, [u64; 2]) {
    let mut flags = 0u32;
    let mut t = IzTimes::default();
    let mut ids = [0u64; 2];
    if ef.is_empty() || (!want_times && !want_ids) {
        return (0, t, ids);
    }
    let mut have_new_type_eb = 0;
    let mut compatible = false;
    let long = |b: &[u8], o: usize| i64::from(u32::from_le_bytes(b[o..o + 4].try_into().unwrap()));
    let word = |b: &[u8], o: usize| u64::from(u16::from_le_bytes([b[o], b[o + 1]]));
    let mut rest = ef;
    while rest.len() >= EB_HEADSIZE {
        let (id, len) = (ef_word(rest, 0), usize::from(ef_word(rest, 2)));
        if len > rest.len() - EB_HEADSIZE {
            break;
        }
        let d = &rest[EB_HEADSIZE..EB_HEADSIZE + len];
        match id {
            EF_TIME => 'ut: {
                flags &= !0xff;
                have_new_type_eb = 1;
                if len < 1 || !want_times {
                    break 'ut;
                }
                let mut idx = 1;
                flags |= u32::from(d[0]);
                if flags & EB_UT_FL_MTIME != 0 {
                    if idx + 4 <= len {
                        let v = long(d, idx);
                        idx += 4;
                        if v & 0x8000_0000 != 0 {
                            compatible = dos_mdatetime >= DOSTIME_2038_01_18;
                            if !compatible {
                                flags &= !0xff;
                                break 'ut;
                            }
                        } else {
                            compatible = false;
                        }
                        t.mtime = v;
                    } else {
                        flags &= !EB_UT_FL_MTIME;
                    }
                }
                if central {
                    break 'ut;
                }
                for (bit, slot) in [(EB_UT_FL_ATIME, &mut t.atime), (EB_UT_FL_CTIME, &mut t.ctime)] {
                    if flags & bit == 0 {
                        continue;
                    }
                    if idx + 4 <= len {
                        let v = long(d, idx);
                        idx += 4;
                        if v & 0x8000_0000 != 0 && !compatible {
                            flags &= !bit;
                        } else {
                            *slot = v;
                        }
                    } else {
                        flags &= !bit;
                    }
                }
            }
            EF_IZUNIX2 => {
                if have_new_type_eb == 0 {
                    have_new_type_eb = 1;
                }
                if have_new_type_eb <= 1 {
                    flags &= 0xff;
                    if len == 4 && want_ids {
                        ids = [word(d, 0), word(d, 2)];
                        flags |= EB_UX2_VALID;
                    }
                }
            }
            EF_IZUNIX3 => {
                have_new_type_eb = 2;
                flags &= 0xff;
                if len >= 7 && want_ids && d[0] == 1 {
                    let uid_size = usize::from(d[1]);
                    if 5 + uid_size <= len {
                        let gid_size = usize::from(d[uid_size + 2]);
                        if 3 + uid_size + gid_size == len {
                            ids = [ux3_value(&d[2..], uid_size, ids[0]), ux3_value(&d[uid_size + 3..], gid_size, ids[1])];
                            flags |= EB_UX2_VALID;
                        }
                    }
                }
            }
            EF_IZUNIX | EF_PKUNIX if len >= 8 && have_new_type_eb == 0 => {
                if want_times {
                    flags |= EB_UT_FL_MTIME | EB_UT_FL_ATIME;
                    let m = long(d, 4);
                    if m & 0x8000_0000 != 0 {
                        compatible = dos_mdatetime >= DOSTIME_2038_01_18;
                        if !compatible {
                            flags &= !0xff;
                        }
                    } else {
                        compatible = false;
                    }
                    t.mtime = m;
                    let a = long(d, 0);
                    if a & 0x8000_0000 != 0 && !compatible && flags & 0xff != 0 {
                        flags &= !EB_UT_FL_ATIME;
                    } else {
                        t.atime = a;
                    }
                }
                if len >= 12 && want_ids {
                    ids = [word(d, 8), word(d, 10)];
                    flags |= EB_UX2_VALID;
                }
            }
            _ => {}
        }
        rest = &rest[EB_HEADSIZE + len..];
    }
    (flags, t, ids)
}

/// Um UID ou GID de 2, 4 ou 8 bytes do bloco "ux" (`read_ux3_value`; com `ulg` de 64 bits o de 8
/// bytes sempre cabe). Outros tamanhos deixam o valor anterior.
fn ux3_value(b: &[u8], size: usize, prev: u64) -> u64 {
    match size {
        2 => u64::from(u16::from_le_bytes([b[0], b[1]])),
        4 => u64::from(u32::from_le_bytes(b[..4].try_into().unwrap())),
        8 => u64::from_le_bytes(b[..8].try_into().unwrap()),
        _ => prev,
    }
}

pub fn ef_word(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
