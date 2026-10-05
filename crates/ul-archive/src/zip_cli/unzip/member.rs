//! A extração ou o teste de um membro já posicionado nos dados (`extract_or_test_member`): a
//! linha de progresso, o descompressor do método, o fechamento do arquivo, a conferência do CRC
//! e o descritor de dados que vem depois dos dados quando o bit 3 está ligado.

use super::extract::{pad22, BZIPPED, DEFLATED, ENHDEFLATED, IMPLODED, SHRUNK, STORED};
use super::fileio::fnfilter;
use super::inflate::WSIZE;
use super::{Uz, PK_DISK, PK_ERR, PK_MEM3, PK_OK, PK_WARN};

/// Os rótulos do `-a` na linha de progresso.
const NUL: &str = "[empty] ";
const TXT: &str = "[text]  ";
const BIN: &str = "[binary]";

impl Uz {
    /// Extrai (pro disco ou, com `-c`/`-p`, pro stdout) ou testa o membro corrente.
    pub fn extract_or_test_member(&mut self) -> i32 {
        let mut error = PK_OK;
        self.x.newfile = true;
        self.x.crc = crc32fast::Hasher::new();
        self.x.symlnk = self.pinfo.symlink && self.o.tflag == 0 && !self.o.cflag && self.lrec.ucsize > 0;
        if self.o.tflag != 0 {
            if self.o.qflag == 0 {
                self.extract_msg("test", "");
            }
        } else if !self.o.cflag && self.open_outfile() {
            return PK_DISK;
        }
        self.defer_leftover_input();
        let quiet_enough = (self.o.tflag != 0 && self.o.qflag != 0) || (self.o.tflag == 0 && self.o.qflag != 0);
        match self.lrec.compression_method {
            STORED => {
                if self.o.tflag == 0 && self.o.qflag == 0 {
                    if self.x.symlnk {
                        self.extract_msg("link", "");
                    } else {
                        let label = if self.o.aflag != 1 {
                            ""
                        } else if self.lrec.ucsize == 0 {
                            NUL
                        } else if self.pinfo.textfile {
                            TXT
                        } else {
                            BIN
                        };
                        self.extract_msg("extract", label);
                    }
                }
                error = self.unstore();
            }
            DEFLATED | ENHDEFLATED => {
                self.method_msg("inflat");
                let r = self.inflate(self.lrec.compression_method == ENHDEFLATED);
                if r != 0 {
                    error = self.decompress_failure(r, "inflate", quiet_enough);
                }
            }
            SHRUNK => {
                self.method_msg("unshrink");
                let r = self.unshrink();
                if r != PK_OK {
                    if r < PK_DISK {
                        self.failure_msg(r == PK_MEM3, "unshrink", quiet_enough);
                    }
                    error = r;
                }
            }
            IMPLODED => {
                self.method_msg("explod");
                let r = self.explode();
                if r == 5 {
                    error = self.length_warning(quiet_enough);
                } else if r != 0 {
                    error = self.decompress_failure(r, "explode", quiet_enough);
                }
            }
            BZIPPED => {
                self.method_msg("bunzipp");
                let r = self.bunzip2();
                if r != 0 {
                    error = self.decompress_failure(r, "bunzip", quiet_enough);
                }
            }
            _ => {
                let mut m = fnfilter(&self.filename);
                m.extend_from_slice(b":  unknown compression method\n");
                self.info(0x401, m);
                self.undefer_input();
                return PK_WARN;
            }
        }
        if self.x.read_failed {
            return super::PK_BADERR;
        }
        if self.o.tflag == 0 && !self.o.cflag {
            self.close_outfile();
        }
        if self.x.disk_full != 0 {
            if self.x.disk_full > 1 {
                let mut m = b"warning:  ".to_vec();
                m.extend(fnfilter(&self.filename));
                m.extend_from_slice(b" is probably truncated\n");
                self.info(0x421, m);
                error = PK_DISK;
            } else {
                error = PK_WARN;
            }
        }
        if error > PK_WARN {
            self.undefer_input();
            return error;
        }
        let crc = self.x.crc.clone().finalize();
        if crc != self.lrec.crc32 {
            if quiet_enough {
                let mut m = pad22(&fnfilter(&self.filename));
                m.push(b' ');
                self.info(0x401, m);
            }
            self.info(0x401, format!(" bad CRC {crc:08x}  (should be {:08x})\n", self.lrec.crc32));
            if self.pinfo.encrypted {
                self.info(0x401, "    (may instead be incorrect password)\n");
            }
            error = PK_ERR;
        } else if self.o.tflag != 0 {
            if let Some(ef) = self.extra_field.clone() {
                let ef = &ef[..ef.len().min(usize::from(self.lrec.extra_field_length))];
                let r = self.test_extra_field(ef);
                if r > error {
                    error = r;
                }
            } else if self.o.qflag == 0 {
                self.info(0, " OK\n");
            }
        } else if self.o.qflag == 0 && error == PK_OK {
            self.info(0, "\n");
        }
        self.undefer_input();
        if self.lrec.general_purpose_bit_flag & 8 != 0 && self.skip_data_descriptor() {
            error = PK_ERR;
        }
        error
    }

    /// `"%8sing: %-22s  %s%s"`: o verbo, o nome, o rótulo do `-a` e, com `-c`, a quebra de linha.
    fn extract_msg(&mut self, verb: &str, label: &str) {
        let mut m = format!("{verb:>8}ing: ").into_bytes();
        m.extend(pad22(&fnfilter(&self.filename)));
        m.extend_from_slice(b"  ");
        m.extend_from_slice(label.as_bytes());
        if self.o.cflag && verb != "test" {
            m.push(b'\n');
        }
        self.info(0, m);
    }

    /// A linha de progresso dos métodos comprimidos (fora do teste, sem `-q`).
    fn method_msg(&mut self, verb: &str) {
        if self.o.tflag == 0 && self.o.qflag == 0 {
            let label = if self.o.aflag != 1 {
                ""
            } else if self.pinfo.textfile {
                TXT
            } else {
                BIN
            };
            self.extract_msg(verb, label);
        }
    }

    /// O erro de um descompressor: abaixo de `PK_DISK` vira "invalid compressed data to X" (ou
    /// "not enough memory to X"), com o nome do membro se ele ainda não apareceu.
    fn decompress_failure(&mut self, r: i32, what: &str, quiet_enough: bool) -> i32 {
        if r >= PK_DISK {
            return r;
        }
        self.failure_msg(r == 3, what, quiet_enough);
        if r == 3 { PK_MEM3 } else { PK_ERR }
    }

    /// `ErrUnzipFile` ou `ErrUnzipNoFile`.
    fn failure_msg(&mut self, no_mem: bool, what: &str, quiet_enough: bool) {
        let why = if no_mem { "not enough memory to " } else { "invalid compressed data to " };
        let m = if quiet_enough {
            let mut m = format!("  error:  {why}{what} ").into_bytes();
            m.extend(fnfilter(&self.filename));
            m.push(b'\n');
            m
        } else {
            format!("\n  error:  {why}{what}\n").into_bytes()
        };
        self.info(0x401, m);
    }

    /// O explode leu um número de bytes diferente do `csize` (`LengthMsg`): aviso se usou menos,
    /// erro se usou mais.
    fn length_warning(&mut self, quiet_enough: bool) -> i32 {
        let used = self.x.used_csize;
        let warning = used >= 0 && used as u64 <= self.lrec.csize;
        let word = if warning { "warning" } else { "error" };
        let pad = if warning { "  " } else { "" };
        let (lead, open, close) = if quiet_enough { ("", " [", "]") } else { ("\n", "", ".") };
        let mut m = format!(
            "{lead}  {word}:  {used} bytes required to uncompress to {} bytes;\n    {pad}      supposed to require {} bytes",
            self.lrec.ucsize, self.lrec.csize
        )
        .into_bytes();
        m.extend_from_slice(open.as_bytes());
        if quiet_enough {
            m.extend(fnfilter(&self.filename));
        }
        m.extend_from_slice(close.as_bytes());
        m.push(b'\n');
        self.info(0x401, m);
        if warning { PK_WARN } else { PK_ERR }
    }

    /// STORED: copia os bytes em blocos do tamanho da janela.
    fn unstore(&mut self) -> i32 {
        let mut slide = std::mem::take(&mut self.x.slide);
        slide.resize(WSIZE, 0);
        let mut error = PK_OK;
        let mut outcnt = 0;
        while let Some(b) = self.next_byte() {
            slide[outcnt] = b;
            outcnt += 1;
            if outcnt == WSIZE {
                error = self.flush(&slide[..outcnt]);
                outcnt = 0;
                if error != PK_OK || self.x.disk_full != 0 {
                    break;
                }
            }
        }
        if outcnt != 0 {
            let r = self.flush(&slide[..outcnt]);
            if error < r {
                error = r;
            }
        }
        self.x.slide = slide;
        error
    }

    /// Pula o descritor de dados depois dos dados (CRC e tamanhos, com ou sem a assinatura
    /// opcional, que só se distingue comparando com os valores do membro). `true` se faltaram
    /// bytes.
    fn skip_data_descriptor(&mut self) -> bool {
        const SIG: u32 = 0x0807_4b50;
        const LOW: u64 = 0xffff_ffff;
        let mut buf = [0u8; 12];
        let mut shy = 12 - self.readbuf(&mut buf);
        let long = |o: usize| u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
        let (crc, clen, ulen) = if shy != 0 { (0, 0, 0) } else { (long(0), long(4), long(8)) };
        let l = &self.lrec;
        let has_sig = crc == SIG
            && (l.crc32 != SIG
                || (clen == SIG
                    && (l.csize & LOW != u64::from(SIG)
                        || (ulen == SIG && (if self.zip64 { l.csize >> 32 } else { l.ucsize }) != u64::from(SIG)))));
        if has_sig {
            shy += 4 - self.readbuf(&mut buf[..4]);
        }
        if self.zip64 {
            let mut b8 = [0u8; 8];
            shy += 8 - self.readbuf(&mut b8);
        }
        shy != 0
    }

    /// BZIPPED (`UZbunzip2`): alimenta o libbz2 direto do buffer de entrada e despeja a janela a
    /// cada volta. No fim a posição de leitura fica onde o libbz2 parou, com a mesma conta do C
    /// (que desconta do `incnt` o deslocamento inteiro do ponteiro, não só o consumido).
    fn bunzip2(&mut self) -> i32 {
        use bzip2::{Decompress, Error, Status};
        if self.zin.incnt <= 0 && self.csize <= 0 {
            return 2;
        }
        let mut slide = std::mem::take(&mut self.x.slide);
        slide.resize(WSIZE, 0);
        let mut d = Decompress::new(false);
        let mut in_pos = self.zin.inptr;
        let mut avail_in = self.zin.incnt.max(0) as usize;
        let mut out_pos = 0usize;
        let mut ended = false;
        // Uma chamada ao `BZ2_bzDecompress`: avança a entrada e a saída pelo que ele consumiu e
        // produziu. `Err` traz o código de retorno do C (2 dados, 3 memória).
        let mut call = |g: &mut Uz, slide: &mut [u8], in_pos: &mut usize, avail_in: &mut usize, out_pos: &mut usize, ended: &mut bool| -> Result<bool, i32> {
            let (tin, tout) = (d.total_in(), d.total_out());
            let r = d.decompress(&g.zin.buf[*in_pos..*in_pos + *avail_in], &mut slide[*out_pos..]);
            let used = (d.total_in() - tin) as usize;
            let made = (d.total_out() - tout) as usize;
            *in_pos += used;
            *avail_in -= used;
            *out_pos += made;
            match r {
                Ok(Status::StreamEnd) => {
                    *ended = true;
                    Ok(true)
                }
                Ok(Status::MemNeeded) => Err(3),
                Ok(_) => Ok(used != 0 || made != 0),
                Err(Error::Data | Error::DataMagic) => Err(2),
                Err(_) => Ok(false),
            }
        };
        let mut retval = 0;
        'run: {
            while self.csize > 0 && !ended {
                while out_pos < WSIZE {
                    if let Err(r) = call(self, &mut slide, &mut in_pos, &mut avail_in, &mut out_pos, &mut ended) {
                        retval = r;
                        break 'run;
                    }
                    if self.csize <= 0 || ended {
                        break;
                    }
                    if avail_in == 0 {
                        if self.fillinbuf() == 0 {
                            retval = 2;
                            break 'run;
                        }
                        in_pos = self.zin.inptr;
                        avail_in = self.zin.incnt.max(0) as usize;
                    }
                }
                retval = self.flush(&slide[..out_pos]);
                if retval != 0 {
                    break 'run;
                }
                out_pos = 0;
            }
            while !ended {
                match call(self, &mut slide, &mut in_pos, &mut avail_in, &mut out_pos, &mut ended) {
                    Err(r) => {
                        retval = r;
                        break 'run;
                    }
                    // Sem entrada e sem fim do fluxo o C repete pra sempre; aqui vira dado inválido.
                    Ok(false) if !ended => {
                        retval = 2;
                        break 'run;
                    }
                    Ok(_) => {}
                }
                retval = self.flush(&slide[..out_pos]);
                if retval != 0 {
                    break 'run;
                }
                out_pos = 0;
            }
            self.zin.inptr = in_pos;
            self.zin.incnt -= in_pos as i64;
        }
        self.x.slide = slide;
        retval
    }
}
