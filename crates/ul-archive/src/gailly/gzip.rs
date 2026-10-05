//! Deflate do gzip 1.13 (deflate.c), empurrado: os dados chegam por [`GzDeflate::feed`] e o fim
//! por [`GzDeflate::finish`], porque o codec é um `Write`. O C puxa a entrada com `read_buf` e só
//! a pede no fim de cada passo (`while (lookahead < MIN_LOOKAHEAD && !eofile) fill_window()`);
//! aqui o passo para nesse ponto e continua quando chegarem mais dados. Como a saída não depende
//! de quanto cada `read` devolve, o resultado é o mesmo.
//!
//! O estado persiste entre os arquivos da mesma execução como no C, em que a janela é global: os
//! bytes velhos depois do fim da entrada entram na comparação do `longest_match` e podem decidir
//! qual correspondência vence.
//!
//! Diferenças para o deflate do zip 3.0: toda posição entra na tabela de hash (mesmo nas duas
//! últimas, com os dois bytes zerados depois do fim), o `nice_match` não é limitado ao que resta, a
//! busca exige `strstart <= window_size - MIN_LOOKAHEAD`, o arquivo nunca vira armazenado inteiro
//! (`seekable()` é 0) e existe o `--rsyncable`.

use super::trees::Trees;
use super::{BINARY, BlockSink, MAX_DIST, MAX_MATCH, MIN_LOOKAHEAD, MIN_MATCH, WSIZE};

const HASH_BITS: usize = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_MASK: u32 = (HASH_SIZE - 1) as u32;
const WMASK: u32 = (WSIZE - 1) as u32;
const WINDOW_SIZE: usize = 2 * WSIZE;
const NIL: u32 = 0;
const TOO_FAR: u32 = 4096;
const H_SHIFT: u32 = HASH_BITS.div_ceil(MIN_MATCH) as u32;
const RSYNC_WIN: u32 = 4096;
const NO_CHUNK_END: u64 = 0xFFFF_FFFF;
const DEFLATED: i32 = 8;

/// (good_length, max_lazy, nice_length, max_chain) por nível: `configuration_table`.
const CONFIG: [(u32, u32, u32, u32); 10] =
    [(0, 0, 0, 0), (4, 4, 8, 4), (4, 5, 16, 8), (4, 6, 32, 32), (4, 4, 16, 16), (8, 16, 32, 32), (8, 16, 128, 128), (8, 32, 128, 256), (32, 128, 258, 1024), (32, 258, 258, 4096)];

pub struct GzDeflate {
    window: Vec<u8>,
    prev: Vec<u16>,
    head: Vec<u16>,
    ct: Trees,
    level: u32,
    rsync: bool,
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
    nice_match: u32,
    rsync_sum: u64,
    rsync_chunk_end: u64,
    /// O `lm_init` ainda não terminou de encher a janela (o `ins_h` inicial vem depois).
    in_init: bool,
    /// Nada foi lido deste arquivo ainda: um fim aqui é o `read_buf` do `lm_init` devolvendo 0, que
    /// não zera os bytes depois do fim.
    fresh: bool,
    /// Locais do laço do C que sobrevivem entre as chamadas.
    match_available: bool,
    match_length: u32,
    done: bool,
}

impl Default for GzDeflate {
    fn default() -> Self {
        Self::new()
    }
}

impl GzDeflate {
    pub fn new() -> GzDeflate {
        GzDeflate {
            window: vec![0; WINDOW_SIZE],
            prev: vec![0; WSIZE],
            head: vec![0; HASH_SIZE],
            ct: Trees::new(),
            level: 6,
            rsync: false,
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
            rsync_sum: 0,
            rsync_chunk_end: NO_CHUNK_END,
            in_init: false,
            fresh: false,
            match_available: false,
            match_length: 0,
            done: false,
        }
    }

    /// Começa um arquivo: `bi_init`, `ct_init` e a parte do `lm_init` que não lê. `level` vai de 1 a
    /// 9 (o `lm_init` do C recusa os outros com "bad pack level").
    pub fn start(&mut self, level: u32, rsync: bool) {
        let level = level.clamp(1, 9);
        self.ct.ct_init(BINARY, DEFLATED);
        self.level = level;
        self.rsync = rsync;
        self.head.fill(NIL as u16);
        self.rsync_chunk_end = NO_CHUNK_END;
        self.rsync_sum = 0;
        let (good, lazy, nice, chain) = CONFIG[level as usize];
        self.max_lazy_match = lazy;
        self.good_match = good;
        self.nice_match = nice;
        self.max_chain_length = chain;
        self.strstart = 0;
        self.block_start = 0;
        self.lookahead = 0;
        self.eofile = false;
        self.in_init = true;
        self.fresh = true;
        self.match_available = false;
        self.match_length = if level <= 3 { 0 } else { (MIN_MATCH - 1) as u32 };
        self.prev_length = (MIN_MATCH - 1) as u32;
        self.done = false;
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

    /// `longest_match`: a maior correspondência a partir de `strstart`, dado o início da cadeia. Pode
    /// passar do fim da entrada (o chamador limita ao `lookahead`).
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

    /// Uma chamada do `fill_window`: desliza a janela se preciso e lê o que couber de `data`. Com
    /// `data` vazio (só no fim da entrada) marca o fim e zera os dois bytes seguintes.
    fn fill_window(&mut self, data: &mut &[u8]) {
        let mut more = (WINDOW_SIZE as u32) - self.lookahead - self.strstart;
        if self.strstart as usize >= WSIZE + MAX_DIST {
            self.window.copy_within(WSIZE..WINDOW_SIZE, 0);
            self.match_start = self.match_start.wrapping_sub(WSIZE as u32);
            self.strstart -= WSIZE as u32;
            if self.rsync_chunk_end != NO_CHUNK_END {
                self.rsync_chunk_end = self.rsync_chunk_end.wrapping_sub(WSIZE as u64);
            }
            self.block_start -= WSIZE as i64;
            for h in self.head.iter_mut() {
                *h = if *h as usize >= WSIZE { *h - WSIZE as u16 } else { NIL as u16 };
            }
            for p in self.prev.iter_mut() {
                *p = if *p as usize >= WSIZE { *p - WSIZE as u16 } else { NIL as u16 };
            }
            more += WSIZE as u32;
        }
        if self.eofile {
            return;
        }
        let a = (self.strstart + self.lookahead) as usize;
        let n = (more as usize).min(data.len());
        if n == 0 {
            self.eofile = true;
            self.window[a..a + (MIN_MATCH - 1)].fill(0);
        } else {
            self.window[a..a + n].copy_from_slice(&data[..n]);
            *data = &data[n..];
            self.lookahead += n as u32;
        }
    }

    /// `rsync_roll`: soma corrente dos últimos `RSYNC_WIN` bytes; marca o fim do pedaço quando ela é
    /// múltipla de `RSYNC_WIN`.
    fn rsync_roll(&mut self, mut start: u32, mut num: u32) {
        if !self.rsync {
            return;
        }
        if start < RSYNC_WIN {
            let mut i = start;
            while i < RSYNC_WIN {
                if i == start + num {
                    return;
                }
                self.rsync_sum = self.rsync_sum.wrapping_add(self.window[i as usize] as u64);
                i += 1;
            }
            num -= RSYNC_WIN - start;
            start = RSYNC_WIN;
        }
        for i in start..start + num {
            self.rsync_sum = self.rsync_sum.wrapping_add(self.window[i as usize] as u64);
            self.rsync_sum = self.rsync_sum.wrapping_sub(self.window[(i - RSYNC_WIN) as usize] as u64);
            if self.rsync_chunk_end == NO_CHUNK_END && self.rsync_sum.is_multiple_of(RSYNC_WIN as u64) {
                self.rsync_chunk_end = i as u64;
            }
        }
    }

    /// `FLUSH_BLOCK(eof)`, com `pad` = `flush - 1` (o bloco vazio do `--rsyncable`).
    fn flush_block<S: BlockSink + ?Sized>(&mut self, pad: bool, eof: bool, out: &mut S) {
        let stored_len = (self.strstart as i64 - self.block_start) as u64;
        let buf = if self.block_start >= 0 {
            let a = self.block_start as usize;
            Some(&self.window[a..a + stored_len as usize])
        } else {
            None
        };
        self.ct.flush_block(buf, stored_len, pad, eof, out);
    }

    /// O `rsync && strstart > rsync_chunk_end` do fim de cada passo: o pedaço acabou e o bloco
    /// tem de ser despejado com enchimento.
    fn rsync_chunk_done(&mut self) -> bool {
        if self.rsync && self.strstart as u64 > self.rsync_chunk_end {
            self.rsync_chunk_end = NO_CHUNK_END;
            return true;
        }
        false
    }

    /// Um passo do laço do `deflate_fast` (níveis 1 a 3, sem avaliação preguiçosa).
    fn fast_step<S: BlockSink + ?Sized>(&mut self, out: &mut S) {
        let hash_head = self.insert_string(self.strstart);
        if hash_head != NIL
            && self.strstart.wrapping_sub(hash_head) as usize <= MAX_DIST
            && self.strstart as usize <= WINDOW_SIZE - MIN_LOOKAHEAD
        {
            self.match_length = self.longest_match(hash_head);
            if self.match_length > self.lookahead {
                self.match_length = self.lookahead;
            }
        }
        let mut flush;
        if self.match_length as usize >= MIN_MATCH {
            let ml = self.match_length;
            flush = self.ct.tally(self.strstart.wrapping_sub(self.match_start), ml - MIN_MATCH as u32, self.level as i32, self.strstart, self.block_start) as u8;
            self.lookahead -= ml;
            self.rsync_roll(self.strstart, ml);
            // `max_insert_length` é o `max_lazy_match`.
            if ml <= self.max_lazy_match {
                self.match_length -= 1;
                loop {
                    self.strstart += 1;
                    self.insert_string(self.strstart);
                    self.match_length -= 1;
                    if self.match_length == 0 {
                        break;
                    }
                }
                self.strstart += 1;
            } else {
                self.strstart += ml;
                self.match_length = 0;
                self.ins_h = self.window[self.strstart as usize] as u32;
                let c = self.window[self.strstart as usize + 1];
                self.update_hash(c);
            }
        } else {
            let c = self.window[self.strstart as usize] as u32;
            flush = self.ct.tally(0, c, self.level as i32, self.strstart, self.block_start) as u8;
            self.rsync_roll(self.strstart, 1);
            self.lookahead -= 1;
            self.strstart += 1;
        }
        if self.rsync_chunk_done() {
            flush = 2;
        }
        if flush != 0 {
            self.flush_block(flush == 2, false, out);
            self.block_start = self.strstart as i64;
        }
    }

    /// Um passo do laço do `deflate` (níveis 4 a 9, com avaliação preguiçosa).
    fn lazy_step<S: BlockSink + ?Sized>(&mut self, out: &mut S) {
        let hash_head = self.insert_string(self.strstart);
        self.prev_length = self.match_length;
        let prev_match = self.match_start;
        self.match_length = (MIN_MATCH - 1) as u32;
        if hash_head != NIL
            && self.prev_length < self.max_lazy_match
            && self.strstart.wrapping_sub(hash_head) as usize <= MAX_DIST
            && self.strstart as usize <= WINDOW_SIZE - MIN_LOOKAHEAD
        {
            self.match_length = self.longest_match(hash_head);
            if self.match_length > self.lookahead {
                self.match_length = self.lookahead;
            }
            if self.match_length as usize == MIN_MATCH && self.strstart.wrapping_sub(self.match_start) > TOO_FAR {
                self.match_length -= 1;
            }
        }
        if self.prev_length as usize >= MIN_MATCH && self.match_length <= self.prev_length {
            let mut flush = self.ct.tally(self.strstart.wrapping_sub(1).wrapping_sub(prev_match), self.prev_length - MIN_MATCH as u32, self.level as i32, self.strstart, self.block_start) as u8;
            self.lookahead -= self.prev_length - 1;
            self.prev_length -= 2;
            self.rsync_roll(self.strstart, self.prev_length + 1);
            loop {
                self.strstart += 1;
                self.insert_string(self.strstart);
                self.prev_length -= 1;
                if self.prev_length == 0 {
                    break;
                }
            }
            self.match_available = false;
            self.match_length = (MIN_MATCH - 1) as u32;
            self.strstart += 1;
            if self.rsync_chunk_done() {
                flush = 2;
            }
            if flush != 0 {
                self.flush_block(flush == 2, false, out);
                self.block_start = self.strstart as i64;
            }
        } else if self.match_available {
            let c = self.window[self.strstart as usize - 1] as u32;
            let mut flush = self.ct.tally(0, c, self.level as i32, self.strstart, self.block_start) as u8;
            if self.rsync_chunk_done() {
                flush = 2;
            }
            if flush != 0 {
                self.flush_block(flush == 2, false, out);
                self.block_start = self.strstart as i64;
            }
            self.rsync_roll(self.strstart, 1);
            self.strstart += 1;
            self.lookahead -= 1;
        } else {
            if self.rsync_chunk_done() {
                self.flush_block(true, false, out);
                self.block_start = self.strstart as i64;
            }
            self.match_available = true;
            self.rsync_roll(self.strstart, 1);
            self.strstart += 1;
            self.lookahead -= 1;
        }
    }

    /// Entrega mais dados do arquivo; comprime o que já dá sem esperar o resto.
    pub fn feed<S: BlockSink + ?Sized>(&mut self, data: &[u8], out: &mut S) {
        self.run(data, false, out);
    }

    /// Fim da entrada: comprime o que falta e escreve o último bloco.
    pub fn finish<S: BlockSink + ?Sized>(&mut self, out: &mut S) {
        self.run(&[], true, out);
    }

    /// O laço do `deflate`/`deflate_fast`, a partir do ponto em que parou. Cada passo começa com a
    /// janela cheia o bastante (`lookahead >= MIN_LOOKAHEAD`) ou com a entrada no fim; sem isso e sem
    /// dados, devolve e espera o próximo `feed`.
    fn run<S: BlockSink + ?Sized>(&mut self, mut data: &[u8], finish: bool, out: &mut S) {
        if self.done {
            return;
        }
        loop {
            sysabi::sys::checkpoint();
            while (self.lookahead as usize) < MIN_LOOKAHEAD && !self.eofile {
                if data.is_empty() && !finish {
                    return;
                }
                if self.fresh && data.is_empty() {
                    // O `read_buf` do `lm_init` devolveu 0: fim sem zerar nada.
                    self.eofile = true;
                    break;
                }
                self.fresh = false;
                self.fill_window(&mut data);
            }
            if self.in_init {
                // Fim do `lm_init`: o hash começa com os dois primeiros bytes.
                self.in_init = false;
                if !self.fresh {
                    self.ins_h = 0;
                    for j in 0..(MIN_MATCH - 1) {
                        let c = self.window[j];
                        self.update_hash(c);
                    }
                }
            }
            if self.lookahead == 0 {
                break;
            }
            if self.level <= 3 {
                self.fast_step(out);
            } else {
                self.lazy_step(out);
            }
        }
        if !finish {
            return;
        }
        if self.level > 3 && self.match_available {
            let c = self.window[self.strstart as usize - 1] as u32;
            self.ct.tally(0, c, self.level as i32, self.strstart, self.block_start);
        }
        self.flush_block(false, true, out);
        self.done = true;
    }
}
