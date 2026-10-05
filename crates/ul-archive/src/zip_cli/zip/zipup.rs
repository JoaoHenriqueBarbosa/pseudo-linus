//! Compressão de uma entrada (zipup.c): escolha do método, cabeçalho local, dados armazenados,
//! deflate, bzip2, cifra, descritor de dados e a reescrita do cabeçalho com os tamanhos finais.

use sysabi::mode::{S_IFLNK, S_IFMT};
use sysabi::sys;
use sysabi::{Errno, Fd, OFlags};

use super::consts::*;
use super::crypt::{crc32_update, crypthead};
use super::deflate::{Deflate, DeflateIo};
use super::extra::{copy_nondup_extra_fields, put_lg, put_sh};
use super::state::{Exit, R, Zip, Zlist};
use crate::gailly::BlockSink;
use crate::sysutil;

/// `percent`: a redução percentual de `n` para `m`, só com inteiros.
pub fn percent(n: u64, m: u64) -> i32 {
    if n != 0 {
        let (n, m) = (n as i64, m as i64);
        (((200 * (n - m)) / n + 1) / 2) as i32
    } else {
        0
    }
}

/// `suffixes`: o nome termina em algum dos sufixos da lista (separados por `:` ou `;`)?
pub fn suffixes(a: &[u8], s: &[u8]) -> bool {
    let mut m = true;
    let mut q: i64 = a.len() as i64 - 1;
    let mut p: i64 = s.len() as i64 - 1;
    while p >= 0 {
        let c = s[p as usize];
        if c == b':' || c == b';' {
            if m {
                return true;
            }
            m = true;
            q = a.len() as i64 - 1;
        } else {
            m = m && q >= 0 && c == a[q as usize];
            q -= 1;
        }
        p -= 1;
    }
    m
}

/// `is_text_buf`: o buffer parece texto?
pub fn is_text_buf(buf: &[u8]) -> bool {
    let mut result = false;
    for &c in buf {
        if c >= 32 {
            result = true;
        } else if c <= 6 || (14..=25).contains(&c) || (28..=31).contains(&c) {
            return false;
        }
    }
    result
}

fn zread(fd: Fd, buf: &mut [u8]) -> usize {
    loop {
        match sys::read(fd, buf) {
            Ok(n) => return n,
            Err(Errno::EINTR) => continue,
            Err(_) => return 0,
        }
    }
}

/// O estado de leitura do arquivo em compressão: crc, tamanho lido e a detecção de binário.
pub struct ReadState {
    pub fd: Fd,
    pub translate_eol: i32,
    pub crc: u32,
    pub isize: i64,
    pub file_binary: i32,
    pub overflow: bool,
}

impl ReadState {
    /// `file_read`: lê um bloco, traduz o fim de linha se pedido e atualiza o crc e o tamanho.
    pub fn file_read(&mut self, buf: &mut [u8]) -> usize {
        let size = buf.len();
        let len: usize;
        if self.translate_eol == 0 {
            len = zread(self.fd, buf);
            if len == 0 {
                return 0;
            }
        } else if self.translate_eol == 1 {
            // LF para CR LF.
            let half = size >> 1;
            let mut tmp = vec![0u8; half];
            let n = zread(self.fd, &mut tmp);
            if n == 0 {
                return 0;
            }
            tmp.truncate(n);
            if self.file_binary == -1 {
                self.file_binary = if is_text_buf(&tmp) { 0 } else { 1 };
            }
            if self.file_binary != 1 {
                let mut o = 0usize;
                for &c in &tmp {
                    if c == b'\n' {
                        buf[o] = b'\r';
                        buf[o + 1] = b'\n';
                        o += 2;
                    } else {
                        buf[o] = c;
                        o += 1;
                    }
                }
                len = o;
            } else {
                buf[..n].copy_from_slice(&tmp);
                len = n;
            }
        } else {
            // CR LF para LF, sem o ^Z final.
            let want = size - 1;
            let mut tmp = vec![0u8; want];
            let n = zread(self.fd, &mut tmp);
            if n == 0 {
                return 0;
            }
            tmp.truncate(n);
            if self.file_binary == -1 {
                self.file_binary = if is_text_buf(&tmp) { 0 } else { 1 };
            }
            if self.file_binary != 1 {
                let mut o = 0usize;
                for i in 0..n {
                    let c = tmp[i];
                    // O C compara com o byte seguinte; depois do último há uma sentinela LF.
                    let next = if i + 1 < n { tmp[i + 1] } else { b'\n' };
                    if c == b'\r' && next == b'\n' {
                        continue;
                    }
                    buf[o] = c;
                    o += 1;
                }
                if o == 0 {
                    // Mantém um \r solitário no fim do arquivo.
                    buf[0] = b'\r';
                    let mut one = [0u8; 1];
                    if zread(self.fd, &mut one) == 1 {
                        buf[0] = one[0];
                    }
                    o = 1;
                } else if buf[o - 1] == 0x1a {
                    o -= 1;
                }
                len = o;
            } else {
                buf[..n].copy_from_slice(&tmp);
                len = n;
            }
        }
        self.crc = crc32_update(self.crc, &buf[..len]);
        let prev = self.isize;
        self.isize += len as i64;
        if self.isize < prev {
            self.overflow = true;
        }
        len
    }
}

/// A ligação do deflate com o arquivo de entrada e com o zip de saída.
struct ZipIo<'a> {
    zip: &'a mut Zip,
    rd: &'a mut ReadState,
    err: Option<Exit>,
}

impl BlockSink for ZipIo<'_> {
    fn write(&mut self, data: &[u8]) {
        if self.err.is_some() {
            return;
        }
        if let Err(e) = self.zip.zfwrite(data) {
            self.err = Some(e);
        }
    }

    /// `fseekable(y)`: o `fseeko` da glibc despeja o buffer antes de tentar, então isto também
    /// despeja a saída.
    fn seekable(&mut self) -> bool {
        self.zip.y.as_mut().map(|y| y.fseekable()).unwrap_or(true)
    }

    fn use_descriptors(&self) -> bool {
        self.zip.use_descriptors
    }
}

impl DeflateIo for ZipIo<'_> {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        self.rd.file_read(buf)
    }

    fn slide(&mut self) {
        let z = &mut *self.zip;
        if z.dot_size > 0 && !z.display_globaldots {
            if z.noisy && z.dot_count == -1 {
                z.mesg_raw(b" ");
                z.dot_count += 1;
            }
            z.dot_count += 1;
            if z.dot_size <= (z.dot_count + 1) * WSIZE as i64 {
                z.dot_count = 0;
            }
        }
        if (z.verbose > 0 || z.noisy) && z.dot_size != 0 && z.dot_count == 0 {
            z.mesg_raw(b".");
            z.mesg_line_started = true;
        }
    }
}

impl Zip {
    /// `set_new_unix_extra_field`: o campo "ux" com UID e GID de 4 bytes.
    fn set_new_unix_extra_field(z: &mut Zlist, uid: u32, gid: u32) {
        let mut ux = Vec::new();
        put_sh(&mut ux, EF_IZUNIX3);
        put_sh(&mut ux, 11);
        ux.push(1);
        ux.push(4);
        put_lg(&mut ux, uid);
        ux.push(4);
        put_lg(&mut ux, gid);
        z.extra.extend_from_slice(&ux);
        z.cextra.extend_from_slice(&ux);
    }

    /// `set_extra_field` do Unix: horários UT (completo no local, só mtime no central) e o "ux".
    fn set_extra_field(&mut self, z: &mut Zlist) -> i32 {
        let mut name = z.name.clone();
        if name.last() == Some(&b'/') {
            name.pop();
        }
        let st = if self.linkput { sysutil::lstat(&name) } else { sysutil::stat(&name) };
        let Ok(s) = st else { return ZE_OPEN };
        let (mtime, atime) = (s.mtime.sec as u32, s.atime.sec as u32);
        let mut local = Vec::new();
        put_sh(&mut local, EF_TIME);
        put_sh(&mut local, 9);
        local.push((EB_UT_FL_MTIME | EB_UT_FL_ATIME) as u8);
        put_lg(&mut local, mtime);
        put_lg(&mut local, atime);
        let mut central = Vec::new();
        put_sh(&mut central, EF_TIME);
        put_sh(&mut central, 5);
        central.push((EB_UT_FL_MTIME | EB_UT_FL_ATIME) as u8);
        put_lg(&mut central, mtime);
        z.extra = local;
        z.cextra = central;
        Zip::set_new_unix_extra_field(z, s.uid, s.gid);
        ZE_OK
    }

    fn zipup_stats(&mut self, m: i32, isize: u64, s: u64) {
        let line: String = if m == BZIP2 {
            format!(" (bzipped {}%)\n", percent(isize, s))
        } else if m == DEFLATE {
            format!(" (deflated {}%)\n", percent(isize, s))
        } else {
            " (stored 0%)\n".to_string()
        };
        if self.noisy {
            if self.verbose > 0 {
                let v = format!("\t(in={}) (out={})", isize, s);
                self.mesg_raw(v.as_bytes());
            }
            self.mesg_raw(line.as_bytes());
            self.mesg_line_started = false;
        }
        if self.logall {
            self.log_raw(line.as_bytes());
            self.logfile_line_started = false;
        }
    }

    /// `zipup`: comprime o arquivo `z.name` na entrada `z` e a escreve na saída. Devolve `ZE_OK`,
    /// `ZE_OPEN` ou `ZE_MISS` (erros que o chamador trata); o resto é fatal.
    pub fn zipup(&mut self, z: &mut Zlist) -> R<i32> {
        let isdir = z.iname.last() == Some(&b'/');
        let Some(info) = self.filetime(&z.name.clone())? else { return Ok(ZE_OPEN) };
        let tim = info.tim;
        let a = info.attr;
        let mut q: i64 = info.size;
        if isdir != ((a & MSDOS_DIR_ATTR) != 0) {
            // Não troca um diretório por um arquivo nem o contrário.
            return Ok(ZE_MISS);
        }
        if !self.display_globaldots {
            self.dot_count = -1;
        }
        let uq: u64 = if q < 0 { 0 } else { q as u64 };
        if self.noisy && self.display_usize {
            let l = format!(" ({})", display_num(uq));
            self.mesg_raw(l.as_bytes());
            self.mesg_line_started = true;
        }
        if self.logall && self.display_usize {
            let l = format!(" ({})", display_num(uq));
            self.log_raw(l.as_bytes());
            self.logfile_line_started = true;
        }
        z.len = uq;
        z.att = UNKNOWN;
        z.atx = 0;

        let (mut tempextra, mut tempcextra) = (Vec::new(), Vec::new());
        if self.extra_fields == 2 {
            tempextra = z.extra.clone();
            tempcextra = z.cextra.clone();
        }
        z.extra.clear();
        z.cextra.clear();

        let mut m: i32 = match &self.special {
            Some(sp) if suffixes(&z.name, sp) => STORE,
            _ => self.method,
        };
        let mut ifile: Option<Fd> = None;
        let mut is_link = false;
        let stdin_input = z.name == b"-";
        if stdin_input {
            ifile = Some(Fd::STDIN);
            z.tim = tim;
        } else {
            if self.extra_fields != 0 {
                self.set_extra_field(z);
            }
            is_link = ((a >> 16) as u32 & S_IFMT) == S_IFLNK;
            if is_link {
                m = STORE;
            } else if isdir {
                m = STORE;
                q = 0;
            } else {
                match sys::open(&z.name, OFlags::RDONLY, 0) {
                    Ok(fd) => ifile = Some(fd),
                    Err(e) => {
                        self.last_errno = Some(e);
                        return Ok(ZE_OPEN);
                    }
                }
            }
            z.tim = tim;
        }
        if self.extra_fields == 2 {
            z.extra = copy_nondup_extra_fields(&tempextra, &z.extra);
            z.cextra = copy_nondup_extra_fields(&tempcextra, &z.cextra);
        }
        if q == 0 {
            m = STORE;
        }
        if m == BEST {
            m = DEFLATE;
        }

        z.vem = if self.dosify { VEM_DOS } else { VEM_UNIX };
        z.ver = if m == STORE { 10 } else { 20 };
        if self.method == BZIP2 {
            z.ver = if m == STORE { 10 } else { 46 };
        }
        z.crc = 0;
        z.flg = if isdir { 0 } else { 8 };
        let encrypting = !isdir && self.key.is_some();
        if encrypting {
            z.flg |= 1;
            z.crc = z.tim << 16;
        }
        z.lflg = z.flg;
        z.how = m as u16;
        z.siz = if m == STORE && q >= 0 { q as u64 } else { 0 };
        z.len = if q != -1 { q.max(0) as u64 } else { 0 };
        let mut set_type = false;
        if z.att == UNKNOWN {
            z.att = BINARY;
            set_type = true;
        }
        z.atx = if self.dosify { a & 0xff } else { a | (z.atx & 0x0000_ff00) };

        if let Err(e) = self.putlocal(z, PUTLOCAL_WRITE) {
            if let Some(fd) = ifile
                && !stdin_input {
                    let _ = sys::close(fd);
                }
            return Err(e);
        }
        z.off = self.current_local_offset;
        z.dsk = self.current_local_disk;
        self.tempzn += 4 + LOCHEAD as u64 + z.iname.len() as u64 + z.extra.len() as u64;

        if encrypting {
            let mut random = [0u8; RAND_HEAD_LEN - 2];
            let _ = sys::current().getrandom(&mut random);
            let key = self.key.clone().unwrap_or_default();
            let (header, keys) = crypthead(&key, z.crc as u32, &random);
            self.keys = Some(keys);
            self.bfwrite(&header, BFWRITE_DATA)?;
            z.siz += RAND_HEAD_LEN as u64;
            self.tempzn += RAND_HEAD_LEN as u64;
        }

        let mut rd = ReadState { fd: ifile.unwrap_or(Fd::STDIN), translate_eol: self.translate_eol, crc: 0, isize: 0, file_binary: -1, overflow: false };
        let mut s: u64 = 0;
        if isdir {
            // Nada a escrever.
        } else if m != STORE {
            if set_type {
                z.att = UNKNOWN;
            }
            if m == BZIP2 {
                s = self.bzfilecompress(z, &mut m, &mut rd)?;
            } else {
                s = self.filecompress(z, &mut m, &mut rd)?;
            }
            if z.att == BINARY && self.translate_eol != 0 && rd.file_binary != 0 {
                if self.translate_eol == 1 {
                    self.zipwarn("has binary so -l ignored", "");
                } else {
                    self.zipwarn("has binary so -ll ignored", "");
                }
            } else if z.att == BINARY && self.translate_eol != 0 {
                if self.translate_eol == 1 {
                    self.zipwarn("-l used on binary file - corrupted?", "");
                } else {
                    self.zipwarn("-ll used on binary file - corrupted?", "");
                }
            }
        } else if is_link {
            let target = sys::current().readlinkat(Fd::CWD, &z.name).unwrap_or_default();
            let k = target.len().min(SBSZ);
            rd.crc = crc32_update(rd.crc, &target[..k]);
            self.zfwrite(&target[..k])?;
            rd.isize = k as i64;
            s = k as u64;
        } else {
            let mut b = vec![0u8; SBSZ];
            loop {
                let k = rd.file_read(&mut b);
                if k == 0 {
                    break;
                }
                self.zfwrite(&b[..k])?;
                if !self.display_globaldots && self.dot_size > 0 {
                    if self.noisy && self.dot_count == -1 {
                        self.mesg_raw(b" ");
                        self.dot_count += 1;
                    }
                    self.dot_count += 1;
                    if self.dot_size <= (self.dot_count + 1) * SBSZ as i64 {
                        self.dot_count = 0;
                    }
                }
                if !self.display_globaldots && (self.verbose > 0 || self.noisy) && self.dot_size != 0 && self.dot_count == 0 {
                    self.mesg_raw(b".");
                    self.mesg_line_started = true;
                }
            }
            s = rd.isize as u64;
        }
        if rd.overflow {
            return Err(self.ziperr(ZE_BIG, "overflow in byte count"));
        }
        if let Some(fd) = ifile
            && !stdin_input {
                let _ = sys::close(fd);
            }
        self.tempzn += s;
        if self.translate_eol == 0 && q != -1 && rd.isize != q {
            self.zipwarn(" file size changed while zipping ", &z.name);
        }
        let isize = rd.isize.max(0) as u64;

        if isdir {
            z.siz = 0;
            z.len = 0;
            z.how = STORE as u16;
            z.ver = 10;
            z.flg &= !8;
            z.lflg &= !8;
        } else {
            z.crc = rd.crc as u64;
            z.siz = s;
            if encrypting {
                z.siz += RAND_HEAD_LEN as u64;
            }
            z.len = isize;
            let seekable = self.y.as_ref().map(|y| y.seekable).unwrap_or(false);
            if self.use_descriptors || !seekable {
                if z.how != m as u16 {
                    return Err(self.ziperr(ZE_LOGIC, "can't rewrite method"));
                }
                if m == STORE && q < 0 {
                    return Err(self.ziperr(ZE_PARMS, "zip -0 not supported for I/O on pipes or devices"));
                }
                self.putextended(z)?;
                self.tempzn += if self.zip64_entry { 24 } else { 16 };
                z.flg = z.lflg;
            } else {
                let expect = if encrypting { s + 12 } else { s };
                if self.bytes_this_entry != expect {
                    let msg = format!(" s={}, actual={}", s, self.bytes_this_entry);
                    self.mesg_raw(msg.as_bytes());
                    return Err(self.ziperr(ZE_LOGIC, "incorrect compressed size"));
                }
                z.how = m as u16;
                z.ver = match m {
                    STORE => 10,
                    DEFLATE => 20,
                    BZIP2 => 46,
                    _ => z.ver,
                };
                if z.flg & 1 == 0 {
                    z.flg &= !8;
                }
                z.lflg = z.flg;
                self.putlocal(z, PUTLOCAL_REWRITE)?;
                if z.flg & 1 != 0 {
                    self.putextended(z)?;
                    self.tempzn += if self.zip64_entry { 24 } else { 16 };
                }
            }
        }
        z.extra.clear();
        self.zipup_stats(m, isize, s);
        Ok(ZE_OK)
    }

    /// `filecompress`: deflate pelo `deflate.c`/`trees.c` do Info-ZIP.
    fn filecompress(&mut self, z: &mut Zlist, m: &mut i32, rd: &mut ReadState) -> R<u64> {
        if self.level < 1 || self.level > 9 {
            return Err(self.ziperr(ZE_LOGIC, "bad pack level"));
        }
        let mut d = self.deflater.take().unwrap_or_else(|| Box::new(Deflate::new()));
        d.ct.ct_init(z.att, *m);
        let level = self.level;
        let mut io = ZipIo { zip: &mut *self, rd: &mut *rd, err: None };
        d.lm_init(level, &mut z.flg, &mut io);
        let s = d.deflate(&mut io);
        let err = io.err.take();
        z.att = d.ct.file_type;
        *m = d.ct.file_method;
        // No C o método volta como DEFLATE ou STORE; `ct_init` o recebeu como DEFLATE.
        self.deflater = Some(d);
        match err {
            Some(e) => Err(e),
            None => Ok(s),
        }
    }

    /// `bzfilecompress`: bzip2 em blocos de 16K, voltando a armazenar se não reduz o tamanho.
    fn bzfilecompress(&mut self, z: &mut Zlist, m: &mut i32, rd: &mut ReadState) -> R<u64> {
        use bzip2::{Action, Compress, Compression};
        const IBUF: usize = SBSZ;
        let lvl = self.level.clamp(1, 9) as u32;
        let mut bz = Compress::new(Compression::new(lvl), 30);
        let mut ibuf = std::mem::take(&mut self.bz_ibuf);
        if ibuf.len() != IBUF {
            ibuf = vec![0u8; IBUF];
        }
        let mut file_binary_final = false;
        let mut maybe_stored = false;
        let mut avail = rd.file_read(&mut ibuf);
        if !file_binary_final && !is_text_buf(&ibuf) {
            file_binary_final = true;
        }
        if avail < IBUF {
            let more = rd.file_read(&mut ibuf[avail..]);
            if more == 0 {
                maybe_stored = true;
            } else {
                avail += more;
            }
        }
        let mut out: Vec<u8> = Vec::with_capacity(IBUF);
        let mut written: u64 = 0;
        let mut stored_raw = false;
        if !maybe_stored {
            while avail != 0 {
                let mut off = 0usize;
                while off < avail {
                    let before = bz.total_in();
                    out.clear();
                    let _ = bz.compress_vec(&ibuf[off..avail], &mut out, Action::Run);
                    off += (bz.total_in() - before) as usize;
                    if !out.is_empty() {
                        self.zfwrite(&out)?;
                        written += out.len() as u64;
                    }
                }
                avail = rd.file_read(&mut ibuf);
                if !file_binary_final && !is_text_buf(&ibuf) {
                    file_binary_final = true;
                }
            }
        }
        z.att = if file_binary_final { BINARY } else { ASCII };
        // Fim: termina a corrente.
        let mut off = 0usize;
        let input_len = if maybe_stored { avail } else { 0 };
        loop {
            out.clear();
            let before = bz.total_in();
            let input: &[u8] = if maybe_stored { &ibuf[off..input_len] } else { &[] };
            let st = bz.compress_vec(input, &mut out, Action::Finish);
            off += (bz.total_in() - before) as usize;
            if maybe_stored
                && matches!(st, Ok(bzip2::Status::StreamEnd))
                && bz.total_out() >= bz.total_in()
                && self.y.as_mut().map(|y| y.fseekable()).unwrap_or(true)
            {
                // O bzip2 não reduziu: grava a entrada como está.
                let raw = ibuf[..input_len].to_vec();
                self.zfwrite(&raw)?;
                written = input_len as u64;
                *m = STORE;
                stored_raw = true;
                break;
            }
            maybe_stored = false;
            if !out.is_empty() {
                self.zfwrite(&out)?;
                written += out.len() as u64;
            }
            match st {
                Ok(bzip2::Status::FinishOk) => continue,
                Ok(bzip2::Status::StreamEnd) => break,
                _ => break,
            }
        }
        let _ = stored_raw;
        self.bz_ibuf = ibuf;
        Ok(written)
    }
}

/// `DisplayNumString`: o número com sufixo (K, M, G...) em até 3 dígitos, sem os brancos iniciais.
pub fn display_num(num: u64) -> String {
    let s = write_num_string(num);
    s.trim_start().to_string()
}

/// `WriteNumString`: no máximo 3 dígitos mais um multiplicador.
pub fn write_num_string(mut num: u64) -> String {
    let mut mult = 0;
    while num >= 10240 {
        num >>= 10;
        mult += 1;
    }
    let mut digits: Vec<u8> = vec![b'0'];
    let i: usize;
    if num >= 1000 {
        num *= 10;
        num >>= 10;
        mult += 1;
        digits = vec![(num % 10) as u8 + b'0', b'.', (num / 10) as u8 + b'0'];
        i = 3;
    } else {
        let mut d: Vec<u8> = Vec::new();
        while num != 0 {
            d.push((num % 10) as u8 + b'0');
            num /= 10;
        }
        i = if d.is_empty() { 1 } else { d.len() };
        if !d.is_empty() {
            digits = d;
        }
    }
    let mut out = String::new();
    for j in (0..i).rev() {
        out.push(*digits.get(j).unwrap_or(&b' ') as char);
    }
    out.push_str(match mult {
        0 => "",
        1 => "K",
        2 => "M",
        3 => "G",
        4 => "T",
        _ => "?",
    });
    out
}
