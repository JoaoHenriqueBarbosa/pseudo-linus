//! A saída de um membro: o `flush` do fileio.c (CRC, conversão de fim de linha do `-a`, escrita no
//! arquivo ou no stdout do `-c`/`-p`), a pergunta de disco cheio, e a abertura e o fechamento do
//! arquivo de saída (`open_outfile` do fileio.c, `close_outfile` do unix.c).

use sysabi::{AtFlags, Fd, OFlags};

use super::fileio::fnfilter;
use super::unix::{perror, set_times, strerror, Slink};
use super::{sys, Uz, PK_DISK, PK_OK};

const CR: u8 = b'\r';
const LF: u8 = b'\n';
const CTRLZ: u8 = 0x1a;

/// A compressão de bits dos blocos "IM" (`decompress_bits`): bit 0 é um byte zero, bit 1 seguido
/// de 8 bits é um byte literal. Lê zeros depois do fim da entrada.
fn decompress_bits(src: &[u8], needlen: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(needlen);
    let mut bitbuf: u64 = 0;
    let mut bitcnt: i32 = 0;
    let mut i = 0;
    let mut fill = |bitbuf: &mut u64, bitcnt: &mut i32| {
        *bitbuf |= u64::from(src.get(i).copied().unwrap_or(0)) << *bitcnt;
        i += 1;
        *bitcnt += 8;
    };
    for _ in 0..needlen {
        if bitcnt <= 0 {
            fill(&mut bitbuf, &mut bitcnt);
        }
        if bitbuf & 1 != 0 {
            bitbuf >>= 1;
            bitcnt -= 1;
            if bitcnt < 8 {
                fill(&mut bitbuf, &mut bitcnt);
            }
            out.push(bitbuf as u8);
            bitcnt -= 8;
            bitbuf >>= 8;
        } else {
            out.push(0);
            bitcnt -= 1;
            bitbuf >>= 1;
        }
    }
    out
}

impl Uz {
    /// Os bytes descomprimidos de um pedaço do membro (`flush`): soma no CRC e, fora do teste,
    /// escreve, convertendo os fins de linha no modo texto (CR/LF, CR sozinho e LF viram LF; os ^Z
    /// somem).
    pub fn flush(&mut self, raw: &[u8]) -> i32 {
        self.x.crc.update(raw);
        if self.o.tflag != 0 || raw.is_empty() {
            return PK_OK;
        }
        if self.x.disk_full != 0 {
            return PK_DISK;
        }
        if !self.pinfo.textmode {
            return self.write_out(raw);
        }
        if self.x.newfile {
            self.x.did_cr_last = false;
            self.x.newfile = false;
        }
        let mut p = 0;
        if raw[0] == LF && self.x.did_cr_last {
            p = 1;
        }
        self.x.did_cr_last = false;
        if self.x.vms_line_state >= 0 {
            let out = self.vms_text_conv(raw);
            if out.is_empty() {
                return PK_OK;
            }
            return self.write_out(&out);
        }
        let mut out = Vec::with_capacity(raw.len());
        while p < raw.len() {
            match raw[p] {
                CR => {
                    out.push(LF);
                    if p == raw.len() - 1 {
                        self.x.did_cr_last = true;
                    } else if raw[p + 1] == LF {
                        p += 1;
                    }
                }
                LF => out.push(LF),
                CTRLZ => {}
                c => out.push(c),
            }
            p += 1;
        }
        if out.is_empty() {
            return PK_OK;
        }
        self.write_out(&out)
    }

    /// O texto de registro variável do VMS (o ramo `VMS_TEXT_CONV` do `flush`): cada registro é um
    /// comprimento de 2 bytes, os bytes da linha e, se o comprimento é ímpar, um byte de
    /// alinhamento; o comprimento some e a linha ganha o LF. O estado atravessa os pedaços, e o
    /// fim de linha só sai quando chega o byte seguinte (a última linha do arquivo fica sem ele).
    fn vms_text_conv(&mut self, raw: &[u8]) -> Vec<u8> {
        let x = &mut self.x;
        let mut out = Vec::with_capacity(raw.len());
        let mut p = 0;
        while p < raw.len() {
            match x.vms_line_state {
                0 => {
                    if p == raw.len() - 1 {
                        x.vms_line_length = u32::from(raw[p]);
                        p += 1;
                        x.vms_line_state = 1;
                    } else {
                        x.vms_line_length = u32::from(u16::from_le_bytes([raw[p], raw[p + 1]]));
                        p += 2;
                        x.vms_line_state = 2;
                    }
                    x.vms_line_pad = x.vms_line_length & 1 != 0;
                }
                1 => {
                    x.vms_line_length += u32::from(raw[p]) << 8;
                    p += 1;
                    x.vms_line_state = 2;
                }
                2 => {
                    let mut remaining = raw.len() - p;
                    if (x.vms_line_length as usize) < remaining {
                        remaining = x.vms_line_length as usize;
                        x.vms_line_state = 3;
                    }
                    x.vms_line_length -= remaining as u32;
                    out.extend_from_slice(&raw[p..p + remaining]);
                    p += remaining;
                }
                3 => {
                    out.push(LF);
                    x.vms_line_state = if x.vms_line_pad { 4 } else { 0 };
                }
                _ => {
                    p += 1;
                    x.vms_line_state = 0;
                }
            }
        }
        out
    }

    /// O membro veio do VMS com formato de registro variável (`is_vms_varlen_txt`): o atributo de
    /// registro do bloco de VMS da PKWARE (com o CRC conferido) ou o FAB do bloco "IM" do Info-ZIP.
    pub fn is_vms_varlen_txt(&mut self, mut ef: &[u8]) -> bool {
        const EF_PKVMS: u16 = 0x000c;
        const EF_IZVMS: u16 = 0x4d49;
        const VMSATR_C_RECATTR: u16 = 4;
        const VMS_FABSIG: u32 = 0x4241_4656;
        const VMSFAB_B_RFM: usize = 31;
        const VMSREC_C_VAR: u8 = 2;
        let word = |b: &[u8], o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let long = |b: &[u8], o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let mut rectype = 0u8;
        while ef.len() >= 4 {
            let id = word(ef, 0);
            let len = usize::from(word(ef, 2));
            if len > ef.len() - 4 {
                return false;
            }
            let data = &ef[4..4 + len];
            match id {
                EF_PKVMS if len >= 4 => {
                    let mut items = &data[4..];
                    if long(data, 0) != crc32fast::hash(items) {
                        self.info(1, "[Warning: CRC error, discarding PKWARE extra field]\n");
                    } else {
                        while items.len() > 4 {
                            let fldsize = usize::from(word(items, 2));
                            if word(items, 0) == VMSATR_C_RECATTR && fldsize >= 1 {
                                rectype = items[4] & 15;
                            }
                            // O C segue com o tamanho sem sinal; um item que passa do fim encerra.
                            let Some(rest) = items.get(fldsize + 4..) else { break };
                            items = rest;
                        }
                    }
                }
                EF_IZVMS if len >= 4 && long(data, 0) == VMS_FABSIG => {
                    if let Some(fab) = self.extract_izvms_block(data)
                        && fab.len() > VMSFAB_B_RFM
                    {
                        rectype = fab[VMSFAB_B_RFM] & 15;
                    }
                }
                _ => {}
            }
            ef = &ef[4 + len..];
        }
        rectype == VMSREC_C_VAR
    }

    /// Os dados de um bloco "IM" do Info-ZIP (`extract_izvms_block`): guardados, comprimidos pelo
    /// esquema de bits do Info-ZIP (`decompress_bits`) ou deflate (`memextract`).
    fn extract_izvms_block(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        const EB_IZVMS_HLEN: usize = 12;
        if data.len() < EB_IZVMS_HLEN {
            return None;
        }
        let cmptype = u16::from_le_bytes([data[4], data[5]]) & 7;
        let src = &data[EB_IZVMS_HLEN..];
        let usiz = if cmptype == 0 { src.len() } else { usize::from(u16::from_le_bytes([data[6], data[7]])) };
        match cmptype {
            0 => Some(src.to_vec()),
            1 => Some(decompress_bits(src, usiz)),
            2 => Some(self.vms_memextract(usiz, src)),
            _ => None,
        }
    }

    /// O `memextract` fora do teste: os erros viram mensagem e o buffer (zerado onde não foi
    /// escrito) volta mesmo assim, como no C, que ignora o retorno.
    fn vms_memextract(&mut self, tgtsize: usize, src: &[u8]) -> Vec<u8> {
        use super::extract::{DEFLATED, ENHDEFLATED, STORED};
        let mut padded = src.to_vec();
        if padded.len() < 6 {
            padded.resize(6, 0);
        }
        let method = u16::from_le_bytes([padded[0], padded[1]]);
        let crc_expected = u32::from_le_bytes([padded[2], padded[3], padded[4], padded[5]]);
        let data = padded[6..].to_vec();
        let test = self.o.tflag != 0;
        let mut ok = true;
        let out = match method {
            STORED => data,
            DEFLATED | ENHDEFLATED => {
                let (r, out) = self.inflate_in_memory(&data, tgtsize, method == ENHDEFLATED);
                if r != 0 {
                    ok = false;
                    if !test {
                        let what = if r == 3 { "not enough memory to " } else { "invalid compressed data to " };
                        self.info(0x401, format!("\n  error:  {what}inflate\n"));
                    }
                }
                out
            }
            _ => {
                ok = false;
                if !test {
                    self.info(0x401, format!("\nerror:  unsupported extra-field compression type ({method})--skipping\n"));
                }
                Vec::new()
            }
        };
        if ok {
            let crcval = crc32fast::hash(&out);
            if crcval != crc_expected && !test {
                let mut m = b"error [".to_vec();
                m.extend_from_slice(&self.zipfn);
                m.extend_from_slice(format!("]:  bad extra-field CRC {crcval:08x} (should be {crc_expected:08x})\n").as_bytes());
                self.info(0x401, m);
            }
        }
        let mut buf = out;
        buf.resize(tgtsize, 0);
        buf
    }

    /// Escreve no arquivo de saída (o `write` cru, sem stdio) ou, com `-c`/`-p`, pelo `Info` sem
    /// flags, que mantém o controle de começo de linha.
    fn write_out(&mut self, data: &[u8]) -> i32 {
        if self.o.cflag {
            self.info(0, data);
            return PK_OK;
        }
        let Some(fd) = self.x.outfile else { return PK_OK };
        // O `WriteError` compara o retorno de um único `write` com o tamanho.
        match sys().write(fd, data) {
            Ok(n) if n == data.len() => PK_OK,
            _ => self.disk_error(),
        }
    }

    /// `"NOME:  write error (disk full?).  Continue? (y/n/^C) "`: com "y" segue pros outros membros,
    /// senão para. No fim do stdin vale a resposta anterior, como no `fgets` que não toca no buffer.
    fn disk_error(&mut self) -> i32 {
        let mut m = fnfilter(&self.filename);
        m.extend_from_slice(b":  write error (disk full?).  Continue? (y/n/^C) ");
        self.info(0x4a1, m);
        if let Some(a) = self.fgets(10) {
            self.x.answerbuf = a;
        }
        self.x.disk_full = if self.x.answerbuf.first() == Some(&b'y') { 1 } else { 2 };
        PK_DISK
    }

    /// Abre o arquivo de saída (`open_outfile`). Um que já existe sai do caminho antes: com `-B`
    /// vira backup ("~", e depois "~1", "~2"... se o backup também existir), senão é apagado. O
    /// arquivo nasce com 0600 (o `umask(0077)` do C) e com leitura, pra que o link simbólico possa
    /// ser relido. `true` é falha.
    pub fn open_outfile(&mut self) -> bool {
        use super::fileio::FILNAMSIZ;
        let s = sys();
        let exists = sysabi::sys::stat(&self.filename).is_ok() || sysabi::sys::lstat(&self.filename).is_ok();
        if exists {
            if self.o.b_flag {
                let suffix = b"~";
                let mut flen = self.filename.len();
                let mut tlen = flen + suffix.len() + 6;
                let mut tname = self.filename.clone();
                if tlen >= FILNAMSIZ {
                    tlen = FILNAMSIZ - 1 - suffix.len();
                    tname.truncate(tlen);
                    flen = flen.min(tlen);
                    tlen = FILNAMSIZ;
                }
                tname.truncate(flen);
                tname.extend_from_slice(suffix);
                if self.x.overwrite_mode == super::extract::Overwrite::Always {
                    if sysabi::sys::stat(&tname).is_ok() {
                        let _ = s.unlinkat(Fd::CWD, &tname, AtFlags::empty());
                    }
                } else {
                    let maxtail: u32 = match tlen as i64 - flen as i64 - suffix.len() as i64 - 1 {
                        4 => 9999,
                        3 => 999,
                        2 => 99,
                        1 => 9,
                        0 => 0,
                        _ => 99999,
                    };
                    let base = tname.len();
                    let mut i = 0;
                    while i < maxtail && sysabi::sys::stat(&tname).is_ok() {
                        i += 1;
                        tname.truncate(base);
                        tname.extend_from_slice(i.to_string().as_bytes());
                    }
                }
                if let Err(e) = s.renameat2(Fd::CWD, &self.filename, Fd::CWD, &tname, sysabi::RenameFlags::empty()) {
                    let mut m = b"error:  cannot rename old ".to_vec();
                    m.extend(fnfilter(&self.filename));
                    m.extend_from_slice(format!("\n        {}\n", strerror(e)).as_bytes());
                    self.info(0x401, m);
                    return true;
                }
            } else if let Err(e) = s.unlinkat(Fd::CWD, &self.filename, AtFlags::empty()) {
                let mut m = b"error:  cannot delete old ".to_vec();
                m.extend(fnfilter(&self.filename));
                m.extend_from_slice(format!("\n        {}\n", strerror(e)).as_bytes());
                self.info(0x401, m);
                return true;
            }
        }
        let saved = s.umask(0o077);
        let r = s.openat(Fd::CWD, &self.filename, OFlags::RDWR | OFlags::CREAT | OFlags::TRUNC, 0o666);
        s.umask(saved);
        match r {
            Ok(fd) => {
                self.x.outfile = Some(fd);
                false
            }
            Err(e) => {
                let mut m = b"error:  cannot create ".to_vec();
                m.extend(fnfilter(&self.filename));
                m.extend_from_slice(format!("\n        {}\n", strerror(e)).as_bytes());
                self.info(0x401, m);
                true
            }
        }
    }

    /// Fecha o arquivo de saída (`close_outfile` do unix.c). Um link simbólico tem os dados relidos
    /// e vira entrada adiada (o arquivo fica como marcador); um arquivo comum ganha dono (`-X`),
    /// permissões e datas.
    pub fn close_outfile(&mut self) {
        let s = sys();
        let Some(fd) = self.x.outfile.take() else { return };
        let (mtime, atime, uidgid) = self.get_extattribs();
        if self.x.symlnk {
            let ucsize = self.lrec.ucsize as usize;
            let mut target = vec![0u8; ucsize];
            let mut got = 0;
            while got < ucsize {
                match s.pread(fd, &mut target[got..], got as u64) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => got += n,
                }
            }
            let _ = s.close(fd);
            if got != ucsize {
                let mut m = b"warning:  symbolic link (".to_vec();
                m.extend(fnfilter(&self.filename));
                m.extend_from_slice(b") failed\n");
                self.info(0x201, m);
                return;
            }
            if self.o.qflag == 0 {
                let shown = &target[..target.iter().position(|&c| c == 0).unwrap_or(target.len())];
                let mut m = b"-> ".to_vec();
                m.extend(fnfilter(shown));
                m.push(b' ');
                self.info(0, m);
            }
            self.x.slinks.push(Slink { fname: self.filename.clone(), target, perms: self.pinfo.file_attr, uidgid });
            return;
        }
        if let Some((uid, gid)) = uidgid
            && uid <= u64::from(u32::MAX) && gid <= u64::from(u32::MAX)
                && let Err(e) = s.fchownat(fd, b"", Some(uid as u32), Some(gid as u32), AtFlags::EMPTY_PATH) {
                    let m = if self.o.qflag != 0 {
                        let mut m = format!("warning:  cannot set UID {uid} and/or GID {gid} for ").into_bytes();
                        m.extend(fnfilter(&self.filename));
                        m.extend_from_slice(format!("\n          {}\n", strerror(e)).as_bytes());
                        m
                    } else {
                        format!(" (warning) cannot set UID {uid} and/or GID {gid}\n          {}", strerror(e)).into_bytes()
                    };
                    self.info(0x201, m);
                }
        if let Err(e) = s.fchmod(fd, self.filtattr(self.pinfo.file_attr)) {
            perror("fchmod (file attributes) error", e);
        }
        let _ = s.close(fd);
        if self.o.d_flag <= 1
            && let Err(e) = set_times(&self.filename, atime, mtime, AtFlags::empty()) {
                let m = if self.o.qflag != 0 {
                    let mut m = b"warning:  cannot set modif./access times for ".to_vec();
                    m.extend(fnfilter(&self.filename));
                    m.extend_from_slice(format!("\n          {}\n", strerror(e)).as_bytes());
                    m
                } else {
                    format!(" (warning) cannot set modif./access times\n          {}", strerror(e)).into_bytes()
                };
                self.info(0x201, m);
            }
    }
}
