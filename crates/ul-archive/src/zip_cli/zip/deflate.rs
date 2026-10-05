//! Deflate do Info-ZIP (deflate.c): janela deslizante, cadeias de hash e correspondência preguiçosa.
//! Porte fiel do algoritmo, com a janela e as tabelas persistentes entre os arquivos da mesma
//! execução como no C (os bytes velhos da janela podem influenciar a escolha de uma correspondência
//! que passa do fim da entrada, então a saída só é idêntica se esse estado também for).

use super::consts::{MAX_DIST, MAX_MATCH, MIN_LOOKAHEAD, MIN_MATCH, WSIZE};
use super::trees::Trees;

const HASH_BITS: usize = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_MASK: u32 = (HASH_SIZE - 1) as u32;
const WMASK: u32 = (WSIZE - 1) as u32;
const NIL: u32 = 0;
const FAST: u16 = 4;
const SLOW: u16 = 2;
const TOO_FAR: u32 = 4096;
const H_SHIFT: u32 = ((HASH_BITS + MIN_MATCH - 1) / MIN_MATCH) as u32;

/// (good_length, max_lazy, nice_length, max_chain) por nível.
const CONFIG: [(u32, u32, i32, u32); 10] =
    [(0, 0, 0, 0), (4, 4, 8, 4), (4, 5, 16, 8), (4, 6, 32, 32), (4, 4, 16, 16), (8, 16, 32, 32), (8, 16, 128, 128), (8, 32, 128, 256), (32, 128, 258, 1024), (32, 258, 258, 4096)];

/// A ligação do deflate com o mundo: leitura da entrada (que atualiza o crc e o tamanho) e escrita da
/// saída (que cifra e conta os bytes).
pub trait DeflateIo {
    fn read(&mut self, buf: &mut [u8]) -> usize;
    fn write(&mut self, data: &[u8]);
    /// `fseekable(y)`: o `fseeko` da glibc despeja o buffer antes de tentar, então isto também
    /// despeja a saída.
    fn seekable(&mut self) -> bool;
    fn use_descriptors(&self) -> bool;
    /// Chamado a cada deslize da janela (pontos do `-dd`).
    fn slide(&mut self) {}
}

pub struct Deflate {
    window: Vec<u8>,
    prev: Vec<u16>,
    head: Vec<u16>,
    block_start: i64,
    ins_h: u32,
    prev_length: u32,
    strstart: u32,
    match_start: u32,
    eofile: bool,
    lookahead: u32,
    max_chain_length: u32,
    max_lazy_match: u32,
    good_match: u32,
    nice_match: i32,
    level: i32,
    pub ct: Trees,
}

impl Deflate {
    pub fn new() -> Deflate {
        Deflate {
            window: vec![0; 2 * WSIZE],
            prev: vec![0; WSIZE],
            head: vec![0; HASH_SIZE],
            block_start: 0,
            ins_h: 0,
            prev_length: 0,
            strstart: 0,
            match_start: 0,
            eofile: false,
            lookahead: 0,
            max_chain_length: 0,
            max_lazy_match: 0,
            good_match: 0,
            nice_match: 0,
            level: 6,
            ct: Trees::new(),
        }
    }

    fn update_hash(&mut self, c: u8) {
        self.ins_h = ((self.ins_h << H_SHIFT) ^ (c as u32)) & HASH_MASK;
    }

    /// `INSERT_STRING`: devolve a cabeça anterior da cadeia.
    fn insert_string(&mut self, s: u32) -> u32 {
        let c = self.window[s as usize + (MIN_MATCH - 1)];
        self.update_hash(c);
        let h = self.head[self.ins_h as usize] as u32;
        self.prev[(s & WMASK) as usize] = h as u16;
        self.head[self.ins_h as usize] = s as u16;
        h
    }

    /// `lm_init`: prepara a compressão de um arquivo (`flags` recebe os bits de velocidade).
    pub fn lm_init(&mut self, pack_level: i32, flags: &mut u16, io: &mut dyn DeflateIo) {
        self.level = pack_level;
        for h in self.head.iter_mut() {
            *h = 0;
        }
        let c = CONFIG[pack_level as usize];
        self.max_lazy_match = c.1;
        self.good_match = c.0;
        self.nice_match = c.2;
        self.max_chain_length = c.3;
        if pack_level <= 2 {
            *flags |= FAST;
        } else if pack_level >= 8 {
            *flags |= SLOW;
        }
        self.strstart = 0;
        self.block_start = 0;
        self.lookahead = io.read(&mut self.window[..2 * WSIZE]) as u32;
        if self.lookahead == 0 {
            self.eofile = true;
            self.lookahead = 0;
            return;
        }
        self.eofile = false;
        if (self.lookahead as usize) < MIN_LOOKAHEAD {
            self.fill_window(io);
        }
        self.ins_h = 0;
        for j in 0..(MIN_MATCH - 1) {
            let c = self.window[j];
            self.update_hash(c);
        }
    }

    /// `longest_match`: a maior correspondência a partir de `strstart`, dado o início da cadeia.
    fn longest_match(&mut self, mut cur_match: u32) -> u32 {
        let mut chain_length = self.max_chain_length;
        let strstart = self.strstart as usize;
        let mut best_len = self.prev_length as usize;
        let limit: u32 = if self.strstart > MAX_DIST as u32 { self.strstart - MAX_DIST as u32 } else { NIL };
        let win = &self.window;
        let mut scan_end1 = win[strstart + best_len - 1];
        let mut scan_end = win[strstart + best_len];
        if self.prev_length >= self.good_match {
            chain_length >>= 2;
        }
        let nice = self.nice_match as usize;
        loop {
            let m = cur_match as usize;
            let skip = win[m + best_len] != scan_end || win[m + best_len - 1] != scan_end1 || win[m] != win[strstart] || win[m + 1] != win[strstart + 1];
            if !skip {
                // Os bytes 0 e 1 conferem e o 2 é igual por construção do hash; compara a partir do 3.
                let mut len = 3usize;
                while len < MAX_MATCH && win[strstart + len] == win[m + len] {
                    len += 1;
                }
                if len > best_len {
                    self.match_start = cur_match;
                    best_len = len;
                    if len >= nice {
                        break;
                    }
                    scan_end1 = win[strstart + best_len - 1];
                    scan_end = win[strstart + best_len];
                }
            }
            cur_match = self.prev[(cur_match & WMASK) as usize] as u32;
            if cur_match <= limit {
                break;
            }
            chain_length -= 1;
            if chain_length == 0 {
                break;
            }
        }
        best_len as u32
    }

    fn flush_block_now(&mut self, eof: bool, io: &mut dyn DeflateIo) -> u64 {
        let stored_len = (self.strstart as i64 - self.block_start) as u64;
        let buf = if self.block_start >= 0 {
            let a = self.block_start as usize;
            Some(&self.window[a..a + stored_len as usize])
        } else {
            None
        };
        self.ct.flush_block(buf, stored_len, eof, io)
    }

    fn fill_window(&mut self, io: &mut dyn DeflateIo) {
        loop {
            let mut more = (2 * WSIZE) as u32 - self.lookahead - self.strstart;
            if self.strstart as usize >= WSIZE + MAX_DIST {
                self.window.copy_within(WSIZE..2 * WSIZE, 0);
                self.match_start = self.match_start.wrapping_sub(WSIZE as u32);
                self.strstart -= WSIZE as u32;
                self.block_start -= WSIZE as i64;
                for n in 0..HASH_SIZE {
                    let m = self.head[n] as u32;
                    self.head[n] = if m >= WSIZE as u32 { (m - WSIZE as u32) as u16 } else { NIL as u16 };
                }
                for n in 0..WSIZE {
                    let m = self.prev[n] as u32;
                    self.prev[n] = if m >= WSIZE as u32 { (m - WSIZE as u32) as u16 } else { NIL as u16 };
                }
                more += WSIZE as u32;
                io.slide();
            }
            if self.eofile {
                return;
            }
            let a = (self.strstart + self.lookahead) as usize;
            let n = io.read(&mut self.window[a..a + more as usize]);
            if n == 0 {
                self.eofile = true;
            } else {
                self.lookahead += n as u32;
            }
            if !((self.lookahead as usize) < MIN_LOOKAHEAD && !self.eofile) {
                break;
            }
        }
    }

    /// `deflate_fast` (níveis 1 a 3): sem avaliação preguiçosa.
    fn deflate_fast(&mut self, io: &mut dyn DeflateIo) -> u64 {
        let mut hash_head: u32 = NIL;
        let mut match_length: u32 = 0;
        self.prev_length = (MIN_MATCH - 1) as u32;
        while self.lookahead != 0 {
            if self.lookahead as usize >= MIN_MATCH {
                hash_head = self.insert_string(self.strstart);
            }
            if hash_head != NIL && self.strstart.wrapping_sub(hash_head) as usize <= MAX_DIST {
                if self.nice_match as u32 > self.lookahead {
                    self.nice_match = self.lookahead as i32;
                }
                match_length = self.longest_match(hash_head);
                if match_length > self.lookahead {
                    match_length = self.lookahead;
                }
            }
            let flush;
            if match_length as usize >= MIN_MATCH {
                flush = self.ct.tally(self.strstart.wrapping_sub(self.match_start), match_length - MIN_MATCH as u32, self.level, self.strstart, self.block_start);
                self.lookahead -= match_length;
                if match_length <= self.max_lazy_match && self.lookahead as usize >= MIN_MATCH {
                    match_length -= 1;
                    loop {
                        self.strstart += 1;
                        hash_head = self.insert_string(self.strstart);
                        match_length -= 1;
                        if match_length == 0 {
                            break;
                        }
                    }
                    self.strstart += 1;
                } else {
                    self.strstart += match_length;
                    match_length = 0;
                    self.ins_h = self.window[self.strstart as usize] as u32;
                    let c = self.window[self.strstart as usize + 1];
                    self.update_hash(c);
                }
            } else {
                let c = self.window[self.strstart as usize] as u32;
                flush = self.ct.tally(0, c, self.level, self.strstart, self.block_start);
                self.lookahead -= 1;
                self.strstart += 1;
            }
            if flush {
                self.flush_block_now(false, io);
                self.block_start = self.strstart as i64;
            }
            if (self.lookahead as usize) < MIN_LOOKAHEAD {
                self.fill_window(io);
            }
        }
        self.flush_block_now(true, io)
    }

    /// `deflate`: comprime a entrada inteira e devolve o tamanho comprimido.
    pub fn deflate(&mut self, io: &mut dyn DeflateIo) -> u64 {
        if self.level <= 3 {
            return self.deflate_fast(io);
        }
        let mut hash_head: u32 = NIL;
        let mut prev_match: u32;
        let mut match_available = false;
        let mut match_length: u32 = (MIN_MATCH - 1) as u32;
        while self.lookahead != 0 {
            if self.lookahead as usize >= MIN_MATCH {
                hash_head = self.insert_string(self.strstart);
            }
            self.prev_length = match_length;
            prev_match = self.match_start;
            match_length = (MIN_MATCH - 1) as u32;
            if hash_head != NIL && self.prev_length < self.max_lazy_match && self.strstart.wrapping_sub(hash_head) as usize <= MAX_DIST {
                if self.nice_match as u32 > self.lookahead {
                    self.nice_match = self.lookahead as i32;
                }
                match_length = self.longest_match(hash_head);
                if match_length > self.lookahead {
                    match_length = self.lookahead;
                }
                if match_length as usize == MIN_MATCH && self.strstart.wrapping_sub(self.match_start) > TOO_FAR {
                    match_length = (MIN_MATCH - 1) as u32;
                }
            }
            if self.prev_length as usize >= MIN_MATCH && match_length <= self.prev_length {
                let max_insert = self.strstart.wrapping_add(self.lookahead).wrapping_sub(MIN_MATCH as u32);
                let flush = self.ct.tally(self.strstart.wrapping_sub(1).wrapping_sub(prev_match), self.prev_length - MIN_MATCH as u32, self.level, self.strstart, self.block_start);
                self.lookahead -= self.prev_length - 1;
                self.prev_length -= 2;
                loop {
                    self.strstart += 1;
                    if self.strstart <= max_insert {
                        hash_head = self.insert_string(self.strstart);
                    }
                    self.prev_length -= 1;
                    if self.prev_length == 0 {
                        break;
                    }
                }
                self.strstart += 1;
                match_available = false;
                match_length = (MIN_MATCH - 1) as u32;
                if flush {
                    self.flush_block_now(false, io);
                    self.block_start = self.strstart as i64;
                }
            } else if match_available {
                let c = self.window[self.strstart as usize - 1] as u32;
                if self.ct.tally(0, c, self.level, self.strstart, self.block_start) {
                    self.flush_block_now(false, io);
                    self.block_start = self.strstart as i64;
                }
                self.strstart += 1;
                self.lookahead -= 1;
            } else {
                match_available = true;
                self.strstart += 1;
                self.lookahead -= 1;
            }
            if (self.lookahead as usize) < MIN_LOOKAHEAD {
                self.fill_window(io);
            }
        }
        if match_available {
            let c = self.window[self.strstart as usize - 1] as u32;
            self.ct.tally(0, c, self.level, self.strstart, self.block_start);
        }
        self.flush_block_now(true, io)
    }
}
