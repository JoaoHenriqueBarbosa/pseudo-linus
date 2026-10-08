//! `fts5_hash.c`: a tabela hash em memória que acumula `termo -> doclist` até o descarregamento
//! para um segmento de nível 0.
//!
//! Modelo v2: as entradas ficam num arena (`Vec<Fts5HashEntry>`) e os encadeamentos
//! (`pHashNext`, `pScanNext`) são índices do arena, não ponteiros. No C cada entrada é uma única
//! alocação `[estrutura][chave][dados]`; aqui a entrada tem `key` (o byte do índice mais o
//! termo, sem NUL) e `data` (o que vem depois da chave). Os campos de deslocamento do C
//! (`iSzPoslist`, `nData`) continuam sendo deslocamentos relativos ao início da estrutura
//! (contam os [`HASH_ENTRY_SIZE`] bytes do cabeçalho do C e a chave), porque o contador de bytes
//! pendentes decide quando o índice descarrega a tabela e isso aparece no layout do arquivo.
//!
//! O `int *pnByte` do C (o `nPendingData` do `Fts5Index`) é o campo [`Fts5Hash::n_byte`]: o
//! índice lê e zera esse campo no lugar de `p->nPendingData`.

use crate::consts::SQLITE_OK;

use super::int::{Fts5Config, FTS5_DETAIL_FULL, FTS5_DETAIL_NONE};
use super::varint::{fts5_append_varint, fts5_put_varint};

/// `sizeof(Fts5HashEntry)` no x86-64: 4 ponteiros/inteiros de 8 bytes mais os campos pequenos,
/// com `iRowid` alinhado em 8. Entra na contagem de bytes pendentes, igual ao C.
pub const HASH_ENTRY_SIZE: i32 = 48;

/// `Fts5HashEntry`.
#[derive(Debug)]
struct Fts5HashEntry {
    /// Próxima entrada com a mesma chave de hash (índice no arena).
    p_hash_next: Option<usize>,
    /// Próxima entrada na ordem do `ScanInit` (índice no arena).
    p_scan_next: Option<usize>,
    /// Tamanho total da alocação do C (a política de crescimento é preservada).
    n_alloc: i32,
    /// Deslocamento do espaço do tamanho da poslist (4 bytes no máximo); 0 se não há.
    i_sz_poslist: i32,
    /// Total de bytes de dados (incluindo a estrutura e a chave).
    n_data: i32,
    /// Marca de delete em `iSzPoslist`.
    b_del: u8,
    /// Marca de conteúdo (modo `detail=none`).
    b_content: u8,
    /// Coluna do último valor escrito.
    i_col: i16,
    /// Posição do último valor escrito.
    i_pos: i32,
    /// Rowid do último valor escrito.
    i_rowid: i64,
    /// O byte do índice seguido do termo (`nKey` é `key.len()`).
    key: Vec<u8>,
    /// Os dados depois da chave: rowid em varint, poslist sem terminador 0x00 e o espaço do
    /// tamanho da poslist anterior. `data.len() == n_data - HASH_ENTRY_SIZE - key.len()`.
    data: Vec<u8>,
}

impl Fts5HashEntry {
    /// O deslocamento (relativo à estrutura) onde `data[0]` fica.
    #[inline]
    fn n_hash_pre(&self) -> i32 {
        HASH_ENTRY_SIZE + self.key.len() as i32
    }

    /// `fts5HashAddPoslistSize(pHash, p, 0)`: fecha o espaço do tamanho da poslist na própria
    /// entrada e zera `iSzPoslist`, `bDel` e `bContent`.
    fn close_poslist_size(&mut self, e_detail: i32) {
        if self.i_sz_poslist != 0 {
            let mut data = std::mem::take(&mut self.data);
            let n_ret = add_poslist_size(e_detail, self, &mut data, 0);
            self.data = data;
            self.i_sz_poslist = 0;
            self.b_del = 0;
            self.b_content = 0;
            self.n_data += n_ret;
        }
    }
}

/// `Fts5Hash`.
#[derive(Debug)]
pub struct Fts5Hash {
    /// Cópia de `Fts5Config.eDetail`.
    e_detail: i32,
    /// O contador de bytes (`*pnByte` do C, o `nPendingData` do índice).
    pub n_byte: i32,
    /// Número de entradas na tabela.
    n_entry: i32,
    /// Entrada corrente da varredura ordenada.
    p_scan: Option<usize>,
    /// O arena de entradas.
    entries: Vec<Fts5HashEntry>,
    /// Os slots da tabela (`aSlot`); o tamanho é `nSlot`.
    a_slot: Vec<Option<usize>>,
}

fn hash_key(n_slot: usize, p: &[u8]) -> usize {
    let mut h: u32 = 13;
    for &b in p.iter().rev() {
        h = (h << 3) ^ h ^ b as u32;
    }
    (h % n_slot as u32) as usize
}

fn hash_key2(n_slot: usize, b: u8, p: &[u8]) -> usize {
    let mut h: u32 = 13;
    for &c in p.iter().rev() {
        h = (h << 3) ^ h ^ c as u32;
    }
    h = (h << 3) ^ h ^ b as u32;
    (h % n_slot as u32) as usize
}

/// `fts5HashAddPoslistSize`: fecha o espaço do tamanho da poslist de `p`. Escreve em `buf`, onde o
/// deslocamento `off` do C vive no índice `origin + (off - n_hash_pre)` (no arena, `buf` é
/// `p.data` e `origin` é 0; em `query` é a cópia com `origin = nPre`). Não altera `p`: quem chama
/// com `p2 == 0` atualiza os campos. Devolve a variação de `nData`.
fn add_poslist_size(e_detail: i32, p: &Fts5HashEntry, buf: &mut Vec<u8>, origin: usize) -> i32 {
    let mut n_ret = 0;
    if p.i_sz_poslist != 0 {
        let mut n_data = p.n_data;
        let sz_idx = origin + (p.i_sz_poslist - p.n_hash_pre()) as usize;
        if e_detail == FTS5_DETAIL_NONE {
            debug_assert!(n_data == p.i_sz_poslist);
            if p.b_del != 0 {
                buf.push(0x00);
                n_data += 1;
                if p.b_content != 0 {
                    buf.push(0x00);
                    n_data += 1;
                }
            }
        } else {
            let n_sz = n_data - p.i_sz_poslist - 1; /* Tamanho em bytes */
            let n_pos = n_sz * 2 + p.b_del as i32; /* Valor do campo nPos */
            debug_assert!(p.b_del == 0 || p.b_del == 1);
            if n_pos <= 127 {
                buf[sz_idx] = n_pos as u8;
            } else {
                let mut tmp = [0u8; 9];
                let n_byte = fts5_put_varint(&mut tmp, n_pos as u64) as usize;
                /* O byte reservado vira o varint; o resto da poslist desce n_byte-1 bytes. */
                buf.splice(sz_idx..sz_idx + 1, tmp[..n_byte].iter().copied());
                n_data += n_byte as i32 - 1;
            }
        }
        n_ret = n_data - p.n_data;
    }
    n_ret
}

/// `fts5HashEntryMerge`: junta duas listas encadeadas por `p_scan_next`, cada uma ordenada pela
/// chave, numa só, e devolve a primeira entrada.
fn entry_merge(
    entries: &mut [Fts5HashEntry],
    p_left: Option<usize>,
    p_right: Option<usize>,
) -> Option<usize> {
    let mut p1 = p_left;
    let mut p2 = p_right;
    let mut p_ret: Option<usize> = None;
    let mut tail: Option<usize> = None;
    /* `*ppOut = node`, onde `ppOut` é o `pScanNext` da cauda ou o resultado. */
    fn link(entries: &mut [Fts5HashEntry], p_ret: &mut Option<usize>, tail: Option<usize>, node: Option<usize>) {
        match tail {
            Some(t) => entries[t].p_scan_next = node,
            None => *p_ret = node,
        }
    }
    while p1.is_some() || p2.is_some() {
        match (p1, p2) {
            (None, _) => {
                link(entries, &mut p_ret, tail, p2);
                p2 = None;
            }
            (_, None) => {
                link(entries, &mut p_ret, tail, p1);
                p1 = None;
            }
            (Some(a), Some(b)) => {
                let cmp = entries[a].key.cmp(&entries[b].key);
                debug_assert!(cmp != std::cmp::Ordering::Equal);
                let chosen = if cmp == std::cmp::Ordering::Greater {
                    /* p2 é menor */
                    let next = entries[b].p_scan_next;
                    p2 = next;
                    b
                } else {
                    /* p1 é menor */
                    let next = entries[a].p_scan_next;
                    p1 = next;
                    a
                };
                link(entries, &mut p_ret, tail, Some(chosen));
                tail = Some(chosen);
                entries[chosen].p_scan_next = None;
            }
        }
    }
    p_ret
}

impl Fts5Hash {
    /// `sqlite3Fts5HashNew`: tabela nova (1024 slots), copiando `eDetail` da configuração.
    pub fn new(config: &Fts5Config) -> Fts5Hash {
        Fts5Hash {
            e_detail: config.e_detail,
            n_byte: 0,
            n_entry: 0,
            p_scan: None,
            entries: Vec::new(),
            a_slot: vec![None; 1024],
        }
    }

    /// `sqlite3Fts5HashClear`: esvazia a tabela mantendo-a viva.
    pub fn clear(&mut self) {
        self.entries.clear();
        for s in self.a_slot.iter_mut() {
            *s = None;
        }
        self.p_scan = None;
        self.n_entry = 0;
    }

    /// `fts5HashResize`: dobra o número de slots.
    fn resize(&mut self) -> i32 {
        let n_new = self.a_slot.len() * 2;
        let mut ap_new: Vec<Option<usize>> = vec![None; n_new];
        for i in 0..self.a_slot.len() {
            while let Some(ix) = self.a_slot[i] {
                self.a_slot[i] = self.entries[ix].p_hash_next;
                let i_hash = hash_key(n_new, &self.entries[ix].key);
                self.entries[ix].p_hash_next = ap_new[i_hash];
                ap_new[i_hash] = Some(ix);
            }
        }
        self.a_slot = ap_new;
        SQLITE_OK
    }

    /// `sqlite3Fts5HashWrite`: acrescenta a entrada `(b_byte || token) -> (rowid, col, pos)`. Se
    /// `i_col` é negativo, o valor é uma marca de delete.
    pub fn write(
        &mut self,
        i_rowid: i64,
        i_col: i32,
        i_pos: i32,
        b_byte: u8,
        token: &[u8],
    ) -> i32 {
        let n_token = token.len();
        let mut i_pos = i_pos;
        let mut n_incr: i32 = 0; /* Quanto somar a (*pHash->pnByte) */
        let mut b_new = self.e_detail == FTS5_DETAIL_FULL; /* Escrever a entrada não delete */

        /* Procura uma entrada existente */
        let mut i_hash = hash_key2(self.a_slot.len(), b_byte, token);
        let mut found: Option<usize> = None;
        let mut cur = self.a_slot[i_hash];
        while let Some(ix) = cur {
            let e = &self.entries[ix];
            if e.key[0] == b_byte && e.key.len() == n_token + 1 && &e.key[1..] == token {
                found = Some(ix);
                break;
            }
            cur = e.p_hash_next;
        }

        let ix = match found {
            None => {
                /* Não achou: cria uma nova. Quanto espaço alocar: */
                let mut n_byte = HASH_ENTRY_SIZE as i64 + (n_token as i64 + 1) + 1 + 64;
                if n_byte < 128 {
                    n_byte = 128;
                }

                /* Cresce o aSlot[] se for preciso. */
                if (self.n_entry * 2) as usize >= self.a_slot.len() {
                    let rc = self.resize();
                    if rc != SQLITE_OK {
                        return rc;
                    }
                    i_hash = hash_key2(self.a_slot.len(), b_byte, token);
                }

                /* Aloca a Fts5HashEntry e a põe na tabela. */
                let mut key = Vec::with_capacity(n_token + 1);
                key.push(b_byte);
                key.extend_from_slice(token);
                debug_assert!(i_hash == hash_key(self.a_slot.len(), &key));
                let mut e = Fts5HashEntry {
                    p_hash_next: self.a_slot[i_hash],
                    p_scan_next: None,
                    n_alloc: n_byte as i32,
                    i_sz_poslist: 0,
                    n_data: (n_token + 1) as i32 + HASH_ENTRY_SIZE,
                    b_del: 0,
                    b_content: 0,
                    i_col: 0,
                    i_pos: 0,
                    i_rowid: 0,
                    key,
                    data: Vec::new(),
                };
                let ix = self.entries.len();
                self.a_slot[i_hash] = Some(ix);
                self.n_entry += 1;

                /* Acrescenta o primeiro campo de rowid à entrada */
                e.n_data += fts5_append_varint(&mut e.data, i_rowid as u64);
                e.i_rowid = i_rowid;

                e.i_sz_poslist = e.n_data;
                if self.e_detail != FTS5_DETAIL_NONE {
                    e.data.push(0); /* espaço reservado para o tamanho da poslist */
                    e.n_data += 1;
                    e.i_col = if self.e_detail == FTS5_DETAIL_FULL { 0 } else { -1 };
                }
                self.entries.push(e);
                ix
            }
            Some(ix) => {
                /* Acrescentando a uma entrada existente. Confere se há espaço para a maior
                ** entrada nova possível. O pior caso é:
                **
                **     + 9 bytes para um rowid novo,
                **     + 4 bytes reservados para o varint do "tamanho da poslist",
                **     + 1 byte para um byte de "coluna nova",
                **     + 3 bytes para um número de coluna novo (16 bits no máximo) em varint,
                **     + 5 bytes para o deslocamento da posição nova (32 bits no máximo).
                */
                let p = &mut self.entries[ix];
                if (p.n_alloc - p.n_data) < (9 + 4 + 1 + 3 + 5) {
                    p.n_alloc *= 2;
                }
                n_incr -= p.n_data;
                ix
            }
        };
        let e_detail = self.e_detail;
        let p = &mut self.entries[ix];
        debug_assert!((p.n_alloc - p.n_data) >= (9 + 4 + 1 + 3 + 5));

        /* Se é um rowid novo, acrescenta o campo de 4 bytes de tamanho da entrada anterior e o
        ** rowid novo desta entrada. */
        if i_rowid != p.i_rowid {
            let i_diff = (i_rowid as u64).wrapping_sub(p.i_rowid as u64);
            p.close_poslist_size(e_detail);
            p.n_data += fts5_append_varint(&mut p.data, i_diff);
            p.i_rowid = i_rowid;
            b_new = true;
            p.i_sz_poslist = p.n_data;
            if e_detail != FTS5_DETAIL_NONE {
                p.data.push(0);
                p.n_data += 1;
                p.i_col = if e_detail == FTS5_DETAIL_FULL { 0 } else { -1 };
                p.i_pos = 0;
            }
        }

        if i_col >= 0 {
            if e_detail == FTS5_DETAIL_NONE {
                p.b_content = 1;
            } else {
                /* Acrescenta um valor de coluna novo, se preciso */
                if i_col != p.i_col as i32 {
                    if e_detail == FTS5_DETAIL_FULL {
                        p.data.push(0x01);
                        p.n_data += 1;
                        p.n_data += fts5_append_varint(&mut p.data, i_col as i64 as u64);
                        p.i_col = i_col as i16;
                        p.i_pos = 0;
                    } else {
                        b_new = true;
                        i_pos = i_col;
                        p.i_col = i_col as i16;
                    }
                }

                /* Acrescenta o deslocamento da posição nova, se preciso */
                if b_new {
                    p.n_data += fts5_append_varint(
                        &mut p.data,
                        i_pos.wrapping_sub(p.i_pos).wrapping_add(2) as i64 as u64,
                    );
                    p.i_pos = i_pos;
                }
            }
        } else {
            /* É um delete: liga a marca. */
            p.b_del = 1;
        }

        n_incr += p.n_data;
        self.n_byte += n_incr;
        SQLITE_OK
    }

    /// `fts5HashEntrySort`: liga todos os tokens (que começam por `term`; vazio vale todos) numa
    /// lista ordenada por `p_scan_next`, sem tirá-los da tabela. Devolve a cabeça da lista.
    fn entry_sort(&mut self, term: &[u8]) -> Option<usize> {
        const N_MERGE_SLOT: usize = 32;
        let mut ap: [Option<usize>; N_MERGE_SLOT] = [None; N_MERGE_SLOT];
        for i_slot in 0..self.a_slot.len() {
            let mut p_iter = self.a_slot[i_slot];
            while let Some(ix) = p_iter {
                let next = self.entries[ix].p_hash_next;
                if self.entries[ix].key.starts_with(term) {
                    let mut p_entry = Some(ix);
                    self.entries[ix].p_scan_next = None;
                    let mut i = 0;
                    while ap[i].is_some() {
                        p_entry = entry_merge(&mut self.entries, p_entry, ap[i]);
                        ap[i] = None;
                        i += 1;
                    }
                    ap[i] = p_entry;
                }
                p_iter = next;
            }
        }
        let mut p_list: Option<usize> = None;
        for slot in ap.iter() {
            p_list = entry_merge(&mut self.entries, p_list, *slot);
        }
        p_list
    }

    /// `sqlite3Fts5HashQuery`: o doclist do termo `term` (que inclui o byte do índice na frente).
    /// Devolve `None` se o termo não existe; senão um buffer com `n_pre` bytes livres no começo,
    /// seguidos do doclist, e o tamanho do doclist.
    pub fn query(&self, n_pre: usize, term: &[u8]) -> Option<(Vec<u8>, i32)> {
        let i_hash = hash_key(self.a_slot.len(), term);
        let mut cur = self.a_slot[i_hash];
        while let Some(ix) = cur {
            let e = &self.entries[ix];
            if e.key.as_slice() == term {
                let mut ret = vec![0u8; n_pre];
                ret.extend_from_slice(&e.data);
                let mut n_list = e.data.len() as i32;
                n_list += add_poslist_size(self.e_detail, e, &mut ret, n_pre);
                return Some((ret, n_list));
            }
            cur = e.p_hash_next;
        }
        None
    }

    /// `sqlite3Fts5HashScanInit`: inicia a varredura ordenada dos termos que começam por `term`
    /// (vazio vale todos; o C passa `pTerm == 0` nesse caso).
    pub fn scan_init(&mut self, term: &[u8]) -> i32 {
        self.p_scan = self.entry_sort(term);
        SQLITE_OK
    }

    /// `sqlite3Fts5HashIsEmpty`.
    pub fn is_empty(&self) -> bool {
        debug_assert!(self.n_entry as usize == self.entries.len());
        self.n_entry == 0
    }

    /// `sqlite3Fts5HashScanNext`.
    pub fn scan_next(&mut self) {
        debug_assert!(!self.scan_eof());
        if let Some(ix) = self.p_scan {
            self.p_scan = self.entries[ix].p_scan_next;
        }
    }

    /// `sqlite3Fts5HashScanEof`.
    pub fn scan_eof(&self) -> bool {
        self.p_scan.is_none()
    }

    /// `sqlite3Fts5HashScanEntry`: o termo (com o byte do índice na frente, sem NUL) e o doclist
    /// da entrada corrente; fecha o tamanho da poslist dela. `None` se a varredura acabou (o C
    /// zera as saídas).
    pub fn scan_entry(&mut self) -> Option<(&[u8], &[u8])> {
        let ix = self.p_scan?;
        let e_detail = self.e_detail;
        self.entries[ix].close_poslist_size(e_detail);
        let p = &self.entries[ix];
        Some((p.key.as_slice(), p.data.as_slice()))
    }
}
