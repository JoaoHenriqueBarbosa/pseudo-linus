//! A leitura dos dados comprimidos de um membro, byte a byte como no fileio.c: o buffer de
//! entrada é limitado ao `csize` restante (`defer_leftover_input`), relido em blocos de
//! `INBUFSIZ` (`readbyte`, `fillinbuf`) e, no fim, devolvido ao estado de leitura normal com a
//! posição exata do que foi consumido (`undefer_input`).

use super::process::INBUFSIZ;
use super::text::READ_ERROR;
use super::{Uz, MSG_STDERR};

impl Uz {
    /// Limita o buffer aos bytes do membro: o que passar do `csize` fica guardado à parte.
    pub fn defer_leftover_input(&mut self) {
        let z = &mut self.zin;
        if z.incnt > self.csize {
            if self.csize < 0 {
                self.csize = 0;
            }
            z.inptr_leftover = z.inptr + self.csize as usize;
            z.incnt_leftover = z.incnt - self.csize;
            z.incnt = self.csize;
        } else {
            z.incnt_leftover = 0;
        }
        self.csize -= z.incnt;
    }

    /// Devolve ao buffer o que sobrou dos dados do membro e o que foi guardado à parte.
    pub fn undefer_input(&mut self) {
        let z = &mut self.zin;
        if z.incnt > 0 {
            self.csize += z.incnt;
        }
        if z.incnt_leftover > 0 {
            if self.csize < 0 {
                self.csize = 0;
            }
            z.incnt = z.incnt_leftover + self.csize;
            z.inptr = z.inptr_leftover - self.csize as usize;
            z.incnt_leftover = 0;
        } else if z.incnt < 0 {
            z.incnt = 0;
        }
    }

    /// `NEXTBYTE`: o próximo byte dos dados do membro, ou `None` no fim deles.
    pub fn next_byte(&mut self) -> Option<u8> {
        let z = &mut self.zin;
        z.incnt -= 1;
        if z.incnt >= 0 {
            let c = z.buf[z.inptr];
            z.inptr += 1;
            return Some(c);
        }
        self.readbyte()
    }

    /// Relê o buffer quando ele acaba (`readbyte`). Depois do fim dos dados o `csize` continua
    /// descendo, o que o explode usa pra saber quanto leu a mais.
    fn readbyte(&mut self) -> Option<u8> {
        if self.x.mem.is_some() {
            return None;
        }
        if self.csize <= 0 {
            self.csize -= 1;
            self.zin.incnt = 0;
            return None;
        }
        if self.zin.incnt <= 0 && !self.refill_member_input() {
            return None;
        }
        let z = &mut self.zin;
        z.incnt -= 1;
        let c = z.buf[z.inptr];
        z.inptr += 1;
        Some(c)
    }

    /// `fillinbuf`: lê o próximo bloco do membro e devolve quantos bytes dele há no buffer.
    pub fn fillinbuf(&mut self) -> i64 {
        if !self.refill_member_input() {
            return 0;
        }
        self.zin.incnt
    }

    /// Lê o bloco seguinte do arquivo pro buffer, limita ao membro e decifra. `false` no fim do
    /// arquivo. Um erro de leitura encerra o programa no C (`EXIT(PK_BADERR)` no `readbyte`): aqui
    /// ele marca `read_failed`, que corta o resto do processamento sem outras mensagens.
    fn refill_member_input(&mut self) -> bool {
        if self.x.read_failed {
            return false;
        }
        let z = &mut self.zin;
        z.incnt = z.read_inbuf(INBUFSIZ);
        if z.incnt == 0 {
            return false;
        }
        if z.incnt < 0 {
            self.info(MSG_STDERR, READ_ERROR);
            self.x.read_failed = true;
            self.zin.incnt = 0;
            return false;
        }
        z.bufstart += INBUFSIZ as i64;
        z.inptr = 0;
        self.defer_leftover_input();
        if self.pinfo.encrypted {
            let (from, n) = (self.zin.inptr, self.zin.incnt.max(0) as usize);
            self.decrypt_in_place(from, n);
        }
        true
    }
}
