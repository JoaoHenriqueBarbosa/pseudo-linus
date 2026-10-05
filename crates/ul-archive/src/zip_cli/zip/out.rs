//! Entrada e saída do arquivo zip: o arquivo de saída com buffer e posição lógica (`y`), o arquivo
//! zip antigo lido por posição (`in_file`) e `bfwrite`, que também mantém os contadores do diretório
//! central.

use sysabi::sys;
use sysabi::{Errno, Fd, OFlags, Whence};

use super::consts::*;
use super::state::{R, Zip};

const OUT_BUF: usize = 16384;

/// O arquivo de saída (`y`): escrita com buffer, posição lógica e reescrita por posição.
pub struct OutFile {
    pub fd: Fd,
    buf: Vec<u8>,
    pos: u64,
    pub seekable: bool,
    pub err: Option<Errno>,
}

impl OutFile {
    pub fn from_fd(fd: Fd) -> OutFile {
        let cur = sys::current().lseek(fd, 0, Whence::Cur);
        OutFile { fd, buf: Vec::new(), pos: cur.unwrap_or(0), seekable: cur.is_ok(), err: None }
    }

    pub fn write(&mut self, data: &[u8]) {
        if self.err.is_some() {
            return;
        }
        self.buf.extend_from_slice(data);
        self.pos += data.len() as u64;
        if self.buf.len() >= OUT_BUF {
            self.flush();
        }
    }

    pub fn flush(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let data = std::mem::take(&mut self.buf);
        if self.err.is_none() {
            if let Err(e) = sys::write_all(self.fd, &data) {
                self.err = Some(e);
            }
        }
    }

    pub fn tell(&self) -> u64 {
        self.pos
    }

    /// Reescreve `data` na posição `off` sem mexer na posição de escrita.
    pub fn write_at(&mut self, off: u64, data: &[u8]) -> bool {
        self.flush();
        if self.err.is_some() {
            return false;
        }
        let s = sys::current();
        let mut done = 0usize;
        while done < data.len() {
            match s.pwrite(self.fd, &data[done..], off + done as u64) {
                Ok(0) => {
                    self.err = Some(Errno::EIO);
                    return false;
                }
                Ok(n) => done += n,
                Err(Errno::EINTR) => {}
                Err(e) => {
                    self.err = Some(e);
                    return false;
                }
            }
        }
        true
    }

    /// `fseeko(y, off, SEEK_SET)`.
    pub fn seek_set(&mut self, off: u64) -> bool {
        self.flush();
        match sys::current().lseek(self.fd, off as i64, Whence::Set) {
            Ok(_) => {
                self.pos = off;
                true
            }
            Err(e) => {
                self.err = Some(e);
                false
            }
        }
    }

    /// Descarrega e fecha. Devolve se tudo foi escrito.
    pub fn close(&mut self) -> bool {
        self.flush();
        let ok = self.err.is_none();
        let _ = sys::close(self.fd);
        ok
    }
}

/// O arquivo zip antigo, lido por posição.
pub struct InFile {
    pub fd: Fd,
    pub size: u64,
}

impl InFile {
    pub fn open(path: &[u8]) -> Result<InFile, Errno> {
        let fd = sys::open(path, OFlags::RDONLY | OFlags::CLOEXEC, 0)?;
        match sys::current().fstat(fd) {
            Ok(st) => Ok(InFile { fd, size: st.size }),
            Err(e) => {
                let _ = sys::close(fd);
                Err(e)
            }
        }
    }

    /// Lê até `len` bytes a partir de `off`; devolve menos no fim do arquivo.
    pub fn read_at(&self, off: u64, len: usize) -> Result<Vec<u8>, Errno> {
        let s = sys::current();
        let mut out = vec![0u8; len];
        let mut got = 0usize;
        while got < len {
            match s.pread(self.fd, &mut out[got..], off + got as u64) {
                Ok(0) => break,
                Ok(n) => got += n,
                Err(Errno::EINTR) => {}
                Err(e) => return Err(e),
            }
        }
        out.truncate(got);
        Ok(out)
    }

    pub fn close(self) {
        let _ = sys::close(self.fd);
    }
}

impl Zip {
    /// `bfwrite`: escreve no arquivo de saída e mantém `bytes_this_split` (posição de escrita),
    /// `bytes_this_entry` e os contadores do diretório central. Devolve a quantidade escrita.
    pub fn bfwrite(&mut self, data: &[u8], mode: i32) -> R<usize> {
        if mode == BFWRITE_LOCALHEADER {
            self.bytes_this_entry = 0;
            self.current_local_disk = self.current_disk;
            self.current_local_offset = self.bytes_this_split;
        }
        if mode == BFWRITE_CENTRALHEADER {
            if self.cd_start_disk == -1 {
                self.cd_start_disk = self.current_disk as i64;
                self.cd_start_offset = self.bytes_this_split;
            }
            self.cd_entries_this_disk += 1;
            self.total_cd_entries += 1;
        }
        let n = data.len();
        if let Some(y) = self.y.as_mut() {
            y.write(data);
            if y.err.is_some() {
                let e = y.err;
                self.last_errno = e;
                return Err(self.ziperr(ZE_WRITE, "write error on zip file"));
            }
        }
        self.bytes_this_split += n as u64;
        if mode == BFWRITE_DATA {
            self.bytes_this_entry += n as u64;
        }
        if self.display_globaldots {
            if self.dot_size > 0 {
                if self.dot_count == -1 {
                    self.mesg_raw(b" ");
                    self.dot_count = 1;
                }
                if n > 1000 {
                    self.dot_count += 1;
                    if self.dot_size <= self.dot_count * n as i64 {
                        self.dot_count = 0;
                    }
                }
            }
            if self.dot_size != 0 && self.dot_count == 0 {
                self.dot_count += 1;
                self.mesg_raw(b".");
                self.mesg_line_started = true;
            }
        }
        Ok(n)
    }

    /// `zfwrite`: cifra o buffer se há senha e o escreve como dados.
    pub fn zfwrite(&mut self, data: &[u8]) -> R<usize> {
        if let Some(keys) = self.keys.as_mut() {
            let enc: Vec<u8> = data.iter().map(|&c| keys.encode(c)).collect();
            return self.bfwrite(&enc, BFWRITE_DATA);
        }
        self.bfwrite(data, BFWRITE_DATA)
    }

    /// Fecha o arquivo de saída; o erro de gravação vira o código de saída dado.
    pub fn close_out(&mut self, code: i32) -> R<()> {
        if let Some(mut y) = self.y.take() {
            if !y.close() {
                let name = self.tempzip.clone().unwrap_or_default();
                return Err(self.ziperr(code, &String::from_utf8_lossy(&name)));
            }
        }
        Ok(())
    }
}
