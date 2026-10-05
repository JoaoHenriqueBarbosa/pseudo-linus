//! Mensagens do zip (zip.c): `zipmessage`, `zipwarn`, `ziperr` e o arquivo de log (`-lf`).
//!
//! O `mesg` do C é o stdout (ou o stderr quando o zip sai pelo stdout) e é descarregado a cada
//! mensagem; aqui cada escrita vai direto ao descritor, o que dá a mesma ordem de saída.

use sysabi::sys;
use sysabi::Fd;

use super::consts::*;
use super::state::{Exit, Zip};

impl Zip {
    fn mesg_fd(&self) -> Fd {
        if self.mesg_to_stderr { Fd::STDERR } else { Fd::STDOUT }
    }

    /// `fprintf(mesg, ...)` sem o teste de `noisy`.
    pub fn mesg_raw(&mut self, s: &[u8]) {
        if s.is_empty() {
            return;
        }
        let fd = self.mesg_fd();
        let _ = sys::write_all(fd, s);
    }

    /// `fprintf(logfile, ...)`, se há arquivo de log.
    pub fn log_raw(&mut self, s: &[u8]) {
        if let Some(fd) = self.logfile
            && !s.is_empty() {
                let _ = sys::write_all(fd, s);
            }
    }

    /// `fprintf(stderr, ...)`.
    pub fn stderr_raw(&mut self, s: &[u8]) {
        let _ = sys::write_all(Fd::STDERR, s);
    }

    pub fn strerror(&self) -> String {
        match self.last_errno {
            Some(e) => e.message(),
            None => "Success".to_string(),
        }
    }

    /// `zipmessage_nl`: mensagem sem quebra de linha final (ou com, se `nl`).
    pub fn zipmessage_nl(&mut self, a: &[u8], nl: bool) {
        if self.noisy {
            if !a.is_empty() {
                self.mesg_raw(a);
                self.mesg_line_started = true;
            }
            if nl {
                if self.mesg_line_started {
                    self.mesg_raw(b"\n");
                    self.mesg_line_started = false;
                }
            } else if !a.is_empty() {
                self.mesg_line_started = true;
            }
        }
        if self.logfile.is_some() {
            if !a.is_empty() {
                self.log_raw(a);
                self.logfile_line_started = true;
            }
            if nl {
                if self.logfile_line_started {
                    self.log_raw(b"\n");
                    self.logfile_line_started = false;
                }
            } else if !a.is_empty() {
                self.logfile_line_started = true;
            }
        }
    }

    /// `zipmessage(a, b)`: as duas partes juntas numa linha, quebrando a linha corrente antes.
    pub fn zipmessage(&mut self, a: impl AsRef<[u8]>, b: impl AsRef<[u8]>) {
        let mut line = a.as_ref().to_vec();
        line.extend_from_slice(b.as_ref());
        line.push(b'\n');
        if self.noisy {
            if self.mesg_line_started {
                self.mesg_raw(b"\n");
            }
            self.mesg_raw(&line);
            self.mesg_line_started = false;
        }
        if self.logfile.is_some() {
            if self.logfile_line_started {
                self.log_raw(b"\n");
            }
            self.log_raw(&line);
            self.logfile_line_started = false;
        }
    }

    /// `zipwarn`: "\tzip warning: a b".
    pub fn zipwarn(&mut self, a: impl AsRef<[u8]>, b: impl AsRef<[u8]>) {
        let mut line = b"\tzip warning: ".to_vec();
        line.extend_from_slice(a.as_ref());
        line.extend_from_slice(b.as_ref());
        line.push(b'\n');
        if self.noisy {
            if self.mesg_line_started {
                self.mesg_raw(b"\n");
            }
            self.mesg_raw(&line);
            self.mesg_line_started = false;
        }
        if self.logfile.is_some() {
            if self.logfile_line_started {
                self.log_raw(b"\n");
            }
            self.log_raw(&line);
            self.logfile_line_started = false;
        }
    }

    /// `ziperr`: imprime o erro, limpa (apaga o zip temporário, ou restaura o arquivo no `-g`) e
    /// devolve o código de saída, que o chamador propaga com `return Err(...)`.
    pub fn ziperr(&mut self, c: i32, h: &str) -> Exit {
        self.error_level += 1;
        if self.error_level > 1 {
            return Exit(ZE_LOGIC);
        }
        if self.mesg_line_started {
            self.mesg_raw(b"\n");
            self.mesg_line_started = false;
        }
        if self.logfile.is_some() && self.logfile_line_started {
            self.log_raw(b"\n");
            self.logfile_line_started = false;
        }
        let mut text = String::new();
        if ze_perr(c) {
            text.push_str(&format!("zip I/O error: {}", self.strerror()));
        }
        self.mesg_raw(text.as_bytes());
        let err_line = format!("\nzip error: {} ({})\n", ze_string(c), h);
        self.mesg_raw(err_line.as_bytes());
        if self.logfile.is_some() {
            if ze_perr(c) {
                let l = format!("zip I/O error: {}\n", self.strerror());
                self.log_raw(l.as_bytes());
            }
            self.log_raw(err_line.as_bytes());
            self.logfile_line_started = false;
        }
        if let Some(tz) = self.tempzip.clone() {
            if tz != self.zipfile {
                if let Some(mut y) = self.y.take()
                    && tz != b"-" {
                        y.close();
                    }
                if tz != b"-" {
                    let _ = sys::current().unlinkat(Fd::CWD, &tz, sysabi::AtFlags::empty());
                }
            } else if self.y.is_some() {
                // `-g`: tenta restaurar o arquivo ao estado anterior.
                let m = format!("attempting to restore {} to its previous state\n", String::from_utf8_lossy(&self.zipfile));
                self.mesg_raw(m.as_bytes());
                self.log_raw(m.as_bytes());
                let cb = self.cenbeg;
                if let Some(y) = self.y.as_mut() {
                    y.seek_set(cb);
                }
                self.tempzn = cb;
                self.bytes_this_split = cb;
                let mut k: u64 = 0;
                let zf = std::mem::take(&mut self.zfiles);
                for z in &zf {
                    let mut z = z.clone();
                    let _ = self.putcentral(&mut z);
                    self.tempzn += 4 + CENHEAD as u64 + z.iname.len() as u64 + z.cextra.len() as u64 + z.comment.len() as u64;
                    k += 1;
                }
                let t = self.tempzn - cb;
                let zc = self.zcomment.clone();
                let _ = self.putend(k, t, cb, &zc);
                if let Some(mut y) = self.y.take() {
                    y.close();
                }
            }
        }
        if let Some(fd) = self.logfile.take() {
            let _ = sys::close(fd);
        }
        Exit(c)
    }
}
