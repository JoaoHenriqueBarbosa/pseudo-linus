//! A decifração tradicional do PKZIP (crypt.c): as três chaves, o cabeçalho de 12 bytes que
//! confere a senha, a senha do `-P` ou perguntada no `/dev/tty` (`UzpPassword`, `getp`), e a
//! decifração dos bytes à medida que entram no buffer.

use sysabi::{Fd, OFlags};

use super::fileio::fnfilter;
use super::{sys, Uz, PK_ERR, PK_OK, PK_WARN};

/// Tamanho do cabeçalho de cifra (`RAND_HEAD_LEN`) e da senha (`IZ_PWLEN`).
const RAND_HEAD_LEN: usize = 12;
const IZ_PWLEN: usize = 80;
/// Falta de memória no `decrypt` (`PK_MEM2`).
const PK_MEM2: i32 = 5;

/// O que a pergunta da senha devolve (`IZ_PW_*`).
#[derive(PartialEq, Eq)]
enum Pw {
    Entered,
    CancelAll,
    Error,
}

/// A tabela do CRC-32 que as chaves usam.
fn crc_table() -> &'static [u32; 256] {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, slot) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    })
}

/// `CRC32(c, b)`: um passo do CRC.
fn crc32_step(c: u32, b: u32) -> u32 {
    crc_table()[((c ^ b) & 0xff) as usize] ^ (c >> 8)
}

/// As três chaves da cifra.
#[derive(Clone, Copy, Default)]
pub struct Keys([u32; 3]);

impl Keys {
    /// `init_keys`: as constantes iniciais misturadas com a senha.
    fn new(passwd: &[u8]) -> Keys {
        let mut k = Keys([305_419_896, 591_751_049, 878_082_192]);
        for &c in passwd {
            k.update(c);
        }
        k
    }

    /// `decrypt_byte`: o próximo byte da sequência pseudoaleatória.
    fn decrypt_byte(&self) -> u8 {
        let temp = (self.0[2] & 0xffff) | 2;
        ((temp.wrapping_mul(temp ^ 1) >> 8) & 0xff) as u8
    }

    /// `update_keys` com o byte já decifrado.
    fn update(&mut self, c: u8) {
        self.0[0] = crc32_step(self.0[0], u32::from(c));
        self.0[1] = self.0[1].wrapping_add(self.0[0] & 0xff).wrapping_mul(134_775_813).wrapping_add(1);
        self.0[2] = crc32_step(self.0[2], self.0[1] >> 24);
    }

    /// `zdecode`: decifra um byte e avança as chaves.
    fn decode(&mut self, c: &mut u8) {
        *c ^= self.decrypt_byte();
        self.update(*c);
    }
}

impl Uz {
    /// Decifra `n` bytes do buffer de entrada a partir de `from`.
    pub fn decrypt_in_place(&mut self, from: usize, n: usize) {
        let mut k = self.x.keys;
        for c in &mut self.zin.buf[from..from + n] {
            k.decode(c);
        }
        self.x.keys = k;
    }

    /// Lê o cabeçalho de cifra do membro e acha a senha que o abre (`decrypt`): a do `-P`, a já
    /// usada num membro anterior, ou até três tentativas no terminal. `PK_WARN` é senha errada.
    pub fn decrypt_member(&mut self) -> i32 {
        self.pinfo.encrypted = false;
        self.defer_leftover_input();
        let mut h = [0u8; RAND_HEAD_LEN];
        for slot in &mut h {
            match self.next_byte() {
                Some(b) => *slot = b,
                None => return PK_ERR,
            }
        }
        self.undefer_input();
        self.pinfo.encrypted = true;
        if self.x.newzip {
            self.x.newzip = false;
            if let Some(p) = self.o.pwdarg.clone() {
                if self.x.key.is_none() {
                    self.x.key = Some(p);
                    self.x.nopwd = true;
                }
            } else {
                self.x.key = None;
            }
        }
        if self.x.key.is_some() {
            if self.testp(&h) {
                return PK_OK;
            }
            if self.x.nopwd {
                return PK_WARN;
            }
        }
        let mut n = 0;
        loop {
            let (r, pw) = self.ask_password(&mut n);
            if r == Pw::Error {
                self.x.key = None;
                return PK_MEM2;
            }
            if r != Pw::Entered {
                self.x.key = Some(Vec::new());
                n = 0;
            } else {
                self.x.key = Some(pw);
            }
            if self.testp(&h) {
                return PK_OK;
            }
            if r == Pw::CancelAll {
                self.x.nopwd = true;
            }
            if n == 0 {
                return PK_WARN;
            }
        }
    }

    /// `testp`/`testkey`: decifra o cabeçalho com a senha corrente e compara o último byte com o
    /// byte alto do CRC (ou da hora, com descritor de dados). Se bater, decifra o que já está no
    /// buffer.
    fn testp(&mut self, h: &[u8; RAND_HEAD_LEN]) -> bool {
        let key = self.x.key.clone().unwrap_or_default();
        let mut keys = Keys::new(&key);
        let mut hh = *h;
        for c in &mut hh {
            keys.decode(c);
        }
        let want = if self.pinfo.ext_loc_hdr { ((self.lrec.last_mod_dos_datetime & 0xffff) >> 8) as u8 } else { (self.lrec.crc32 >> 24) as u8 };
        if hh[RAND_HEAD_LEN - 1] != want {
            return false;
        }
        self.x.keys = keys;
        let n = if self.zin.incnt > self.csize { self.csize } else { self.zin.incnt };
        let from = self.zin.inptr;
        self.decrypt_in_place(from, n.max(0) as usize);
        true
    }

    /// `UzpPassword`: a pergunta da senha, com o nome do zip e do membro na primeira vez.
    fn ask_password(&mut self, rcnt: &mut i32) -> (Pw, Vec<u8>) {
        let prompt = if *rcnt == 0 {
            *rcnt = 2;
            let (zf, ef) = (fnfilter(&self.zipfn), fnfilter(&self.filename));
            let fits = 2 * super::fileio::FILNAMSIZ >= zf.len() && 2 * super::fileio::FILNAMSIZ - zf.len() >= ef.len();
            if fits {
                [b"[".as_slice(), &zf, b"] ", &ef, b" password: "].concat()
            } else {
                b"Enter password: ".to_vec()
            }
        } else {
            *rcnt -= 1;
            b"password incorrect--reenter: ".to_vec()
        };
        match getp(&prompt, IZ_PWLEN + 1) {
            None => (Pw::Error, Vec::new()),
            Some(p) if p.is_empty() => (Pw::CancelAll, p),
            Some(p) => (Pw::Entered, p),
        }
    }
}

/// `getp` do Unix: pergunta no stderr e lê uma linha do `/dev/tty`, repetindo se passar do
/// tamanho. Sem terminal controlador o `open` falha e não há senha. (O sandbox não tem termios,
/// então o eco não é desligado.)
fn getp(prompt: &[u8], n: usize) -> Option<Vec<u8>> {
    let s = sys();
    let f = s.openat(Fd::CWD, b"/dev/tty", OFlags::empty(), 0).ok()?;
    let mut warn: &[u8] = b"";
    let line = loop {
        let _ = sysabi::sys::write_all(Fd::STDERR, warn);
        let _ = sysabi::sys::write_all(Fd::STDERR, prompt);
        let mut p = Vec::new();
        let mut c = [0u8; 1];
        loop {
            match s.read(f, &mut c) {
                Ok(1) => {}
                // No fim da entrada o C relê pra sempre o último byte; aqui a linha acaba.
                _ => c[0] = b'\n',
            }
            if p.len() < n {
                p.push(c[0]);
            }
            if c[0] == b'\n' {
                break;
            }
        }
        let _ = sysabi::sys::write_all(Fd::STDERR, b"\n");
        warn = b"(line too long--try again)\n";
        if p.last() == Some(&b'\n') {
            p.pop();
            break p;
        }
    };
    let _ = s.close(f);
    Some(line)
}
