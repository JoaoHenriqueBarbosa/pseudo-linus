//! Conteúdo de arquivo regular.
//!
//! - [`ArcBytes`]: `Arc<Vec<u8>>`; a primeira escrita depois de um snapshot copia o arquivo inteiro.
//! - [`Blocks<S>`]: blocos de 4 KiB (`Arc<[u8]>`) numa sequência `S`; a primeira escrita depois de
//!   um snapshot copia um bloco e o que a sequência precisar (o vetor de ponteiros inteiro no
//!   `Vec`, o caminho na árvore nas estruturas persistentes). O último bloco tem o tamanho exato do
//!   resto do arquivo (arquivo de 100 bytes ocupa 100 bytes, não 4 KiB); blocos inteiros de zero
//!   (buracos) apontam pro mesmo bloco zerado compartilhado.

use std::sync::{Arc, OnceLock};

use archery::SharedPointerKind;

use crate::maps::PointerLabel;
use crate::radix::RadixMap;

pub const BLOCK: usize = 4096;
const BLOCK_U64: u64 = BLOCK as u64;

pub type Block = Arc<[u8]>;

pub trait Content: Clone + Default + Send + Sync + 'static {
    const LABEL: &'static str;
    fn len(&self) -> u64;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn read_at(&self, off: u64, buf: &mut [u8]) -> usize;
    /// Escreve em `off`, estendendo com zeros se `off` passar do fim.
    fn write_at(&mut self, off: u64, data: &[u8]);
    fn set_len(&mut self, len: u64);
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct ArcBytes(Arc<Vec<u8>>);

impl Content for ArcBytes {
    const LABEL: &'static str = "Arc<Vec<u8>>";

    fn len(&self) -> u64 {
        self.0.len() as u64
    }

    fn read_at(&self, off: u64, buf: &mut [u8]) -> usize {
        let len = self.0.len() as u64;
        if off >= len {
            return 0;
        }
        let start = off as usize;
        let n = buf.len().min(self.0.len() - start);
        buf[..n].copy_from_slice(&self.0[start..start + n]);
        n
    }

    fn write_at(&mut self, off: u64, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let v = Arc::make_mut(&mut self.0);
        let start = off as usize;
        let end = start + data.len();
        if end > v.len() {
            v.resize(end, 0);
        }
        v[start..end].copy_from_slice(data);
    }

    fn set_len(&mut self, len: u64) {
        if len as usize != self.0.len() {
            Arc::make_mut(&mut self.0).resize(len as usize, 0);
        }
    }
}

// ---------------------------------------------------------------------------------------------

/// Sequência de blocos com cópia na escrita.
pub trait BlockSeq: Clone + Default + Send + Sync + 'static {
    const LABEL: &'static str;
    fn count(&self) -> usize;
    fn get(&self, i: usize) -> &Block;
    fn get_mut(&mut self, i: usize) -> &mut Block;
    fn push(&mut self, block: Block);
    fn truncate(&mut self, n: usize);
}

/// Bloco inteiro de zeros compartilhado por todos os buracos.
pub fn zero_block() -> Block {
    static ZERO: OnceLock<Block> = OnceLock::new();
    Arc::clone(ZERO.get_or_init(|| Arc::from(vec![0u8; BLOCK])))
}

fn zeros(len: usize) -> Block {
    if len == BLOCK { zero_block() } else { std::iter::repeat_n(0u8, len).collect() }
}

/// Novo bloco com o conteúdo de `old` ajustado pra `len` bytes (corta ou completa com zeros).
fn resized(old: &[u8], len: usize) -> Block {
    if len <= old.len() {
        Arc::from(&old[..len])
    } else {
        old.iter().copied().chain(std::iter::repeat_n(0u8, len - old.len())).collect()
    }
}

#[derive(Clone, Default)]
pub struct Blocks<S> {
    seq: S,
    size: u64,
}

fn blocks_for(size: u64) -> usize {
    size.div_ceil(BLOCK_U64) as usize
}

impl<S: BlockSeq> Blocks<S> {
    fn grow(&mut self, new_size: u64) {
        let old_n = self.seq.count();
        let new_n = blocks_for(new_size);
        if old_n > 0 {
            let last = old_n - 1;
            let target = if new_n > old_n { BLOCK } else { (new_size - last as u64 * BLOCK_U64) as usize };
            let current = self.seq.get(last).len();
            if target > current {
                let block = resized(self.seq.get(last), target);
                *self.seq.get_mut(last) = block;
            }
        }
        for i in old_n..new_n {
            let len = if i + 1 == new_n { (new_size - i as u64 * BLOCK_U64) as usize } else { BLOCK };
            self.seq.push(zeros(len));
        }
        self.size = new_size;
    }

    pub fn block_count(&self) -> usize {
        self.seq.count()
    }
}

impl<S: BlockSeq> Content for Blocks<S> {
    const LABEL: &'static str = S::LABEL;

    fn len(&self) -> u64 {
        self.size
    }

    fn read_at(&self, off: u64, buf: &mut [u8]) -> usize {
        if off >= self.size {
            return 0;
        }
        let n = (buf.len() as u64).min(self.size - off) as usize;
        let mut done = 0;
        while done < n {
            let pos = off + done as u64;
            let bi = (pos / BLOCK_U64) as usize;
            let bo = (pos % BLOCK_U64) as usize;
            let block = self.seq.get(bi);
            let take = (n - done).min(block.len() - bo);
            buf[done..done + take].copy_from_slice(&block[bo..bo + take]);
            done += take;
        }
        n
    }

    fn write_at(&mut self, off: u64, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let end = off + data.len() as u64;
        if end > self.size {
            self.grow(end);
        }
        let mut done = 0;
        while done < data.len() {
            let pos = off + done as u64;
            let bi = (pos / BLOCK_U64) as usize;
            let bo = (pos % BLOCK_U64) as usize;
            let slot = self.seq.get_mut(bi);
            let take = (data.len() - done).min(slot.len() - bo);
            let src = &data[done..done + take];
            if bo == 0 && take == slot.len() {
                // Bloco reescrito por inteiro: aloca direto, sem copiar o antigo.
                *slot = Arc::from(src);
            } else {
                Arc::make_mut(slot)[bo..bo + take].copy_from_slice(src);
            }
            done += take;
        }
    }

    fn set_len(&mut self, len: u64) {
        if len > self.size {
            self.grow(len);
            return;
        }
        if len == self.size {
            return;
        }
        let n = blocks_for(len);
        self.seq.truncate(n);
        if n > 0 {
            let tail = (len - (n as u64 - 1) * BLOCK_U64) as usize;
            if self.seq.get(n - 1).len() != tail {
                let block = resized(self.seq.get(n - 1), tail);
                *self.seq.get_mut(n - 1) = block;
            }
        }
        self.size = len;
    }
}

// ---------------------------------------------------------------------------------------------
// Sequências candidatas.

impl BlockSeq for Vec<Block> {
    const LABEL: &'static str = "Vec<Arc<bloco>>";
    fn count(&self) -> usize {
        self.len()
    }
    fn get(&self, i: usize) -> &Block {
        &self[i]
    }
    fn get_mut(&mut self, i: usize) -> &mut Block {
        &mut self[i]
    }
    fn push(&mut self, block: Block) {
        Vec::push(self, block);
    }
    fn truncate(&mut self, n: usize) {
        Vec::truncate(self, n);
    }
}

impl<P> BlockSeq for imbl::GenericVector<Block, P>
where
    P: SharedPointerKind + Send + Sync + 'static + PointerLabel,
{
    const LABEL: &'static str = P::VECTOR_LABEL;
    fn count(&self) -> usize {
        self.len()
    }
    fn get(&self, i: usize) -> &Block {
        imbl::GenericVector::get(self, i).expect("bloco")
    }
    fn get_mut(&mut self, i: usize) -> &mut Block {
        imbl::GenericVector::get_mut(self, i).expect("bloco")
    }
    fn push(&mut self, block: Block) {
        self.push_back(block);
    }
    fn truncate(&mut self, n: usize) {
        imbl::GenericVector::truncate(self, n);
    }
}

impl BlockSeq for rpds::VectorSync<Block> {
    const LABEL: &'static str = "rpds::VectorSync<Arc<bloco>>";
    fn count(&self) -> usize {
        self.len()
    }
    fn get(&self, i: usize) -> &Block {
        rpds::Vector::get(self, i).expect("bloco")
    }
    fn get_mut(&mut self, i: usize) -> &mut Block {
        rpds::Vector::get_mut(self, i).expect("bloco")
    }
    fn push(&mut self, block: Block) {
        self.push_back_mut(block);
    }
    fn truncate(&mut self, n: usize) {
        while self.len() > n {
            self.drop_last_mut();
        }
    }
}

/// Sequência híbrida à mão: até [`INLINE_BLOCKS`] blocos num `Vec` (arquivo pequeno não paga nó de
/// árvore; copiar 32 ponteiros é barato), e acima disso a trie de raiz 64.
#[derive(Clone)]
pub enum RadixBlocks {
    Small(Vec<Block>),
    Large { map: RadixMap<Block>, count: usize },
}

pub const INLINE_BLOCKS: usize = 32;

impl Default for RadixBlocks {
    fn default() -> Self {
        RadixBlocks::Small(Vec::new())
    }
}

impl BlockSeq for RadixBlocks {
    const LABEL: &'static str = "Vec até 32 blocos + trie de raiz 64 à mão";
    fn count(&self) -> usize {
        match self {
            RadixBlocks::Small(v) => v.len(),
            RadixBlocks::Large { count, .. } => *count,
        }
    }
    fn get(&self, i: usize) -> &Block {
        match self {
            RadixBlocks::Small(v) => &v[i],
            RadixBlocks::Large { map, .. } => map.get(i as u64).expect("bloco"),
        }
    }
    fn get_mut(&mut self, i: usize) -> &mut Block {
        match self {
            RadixBlocks::Small(v) => &mut v[i],
            RadixBlocks::Large { map, .. } => map.get_mut(i as u64).expect("bloco"),
        }
    }
    fn push(&mut self, block: Block) {
        match self {
            RadixBlocks::Small(v) if v.len() < INLINE_BLOCKS => v.push(block),
            RadixBlocks::Small(v) => {
                let mut map = RadixMap::default();
                for (i, b) in v.drain(..).enumerate() {
                    map.insert(i as u64, b);
                }
                let count = map.len();
                map.insert(count as u64, block);
                *self = RadixBlocks::Large { map, count: count + 1 };
            }
            RadixBlocks::Large { map, count } => {
                map.insert(*count as u64, block);
                *count += 1;
            }
        }
    }
    fn truncate(&mut self, n: usize) {
        match self {
            RadixBlocks::Small(v) => v.truncate(n),
            RadixBlocks::Large { map, count } => {
                if n >= *count {
                    return;
                }
                if n <= INLINE_BLOCKS {
                    let v = (0..n).map(|i| Arc::clone(map.get(i as u64).expect("bloco"))).collect();
                    *self = RadixBlocks::Small(v);
                } else {
                    for i in n..*count {
                        map.remove(i as u64);
                    }
                    *count = n;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exercise<C: Content>() {
        let mut c = C::default();
        let mut r: Vec<u8> = Vec::new();
        let mut x: u64 = 12345;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for step in 0..3000 {
            let op = next() % 10;
            if op < 7 {
                let off = next() % 300_000;
                let len = (next() % 9000) as usize;
                let data: Vec<u8> = (0..len).map(|i| (i as u64 + step) as u8).collect();
                c.write_at(off, &data);
                let end = off as usize + len;
                if len > 0 {
                    if end > r.len() {
                        r.resize(end, 0);
                    }
                    r[off as usize..end].copy_from_slice(&data);
                }
            } else if op < 9 {
                let len = next() % 300_000;
                c.set_len(len);
                r.resize(len as usize, 0);
            } else {
                let snap = c.clone();
                let snap_r = r.clone();
                let off = (next() % 100_000) as usize;
                c.write_at(off as u64, b"xyz");
                let end = off + 3;
                if end > r.len() {
                    r.resize(end, 0);
                }
                r[off..end].copy_from_slice(b"xyz");
                let mut buf = vec![0; snap_r.len()];
                assert_eq!(snap.read_at(0, &mut buf), snap_r.len());
                assert_eq!(buf, snap_r, "snapshot alterado no passo {step}");
            }
            assert_eq!(c.len(), r.len() as u64);
            let off = next() % (r.len() as u64 + 10);
            let mut buf = vec![0; (next() % 10_000) as usize];
            let n = c.read_at(off, &mut buf);
            let want = r.get(off as usize..).map(|s| &s[..s.len().min(buf.len())]).unwrap_or(&[]);
            assert_eq!(&buf[..n], want, "passo {step}");
        }
        let mut all = vec![0; r.len()];
        c.read_at(0, &mut all);
        assert_eq!(all, r);
    }

    #[test]
    fn arc_bytes() {
        exercise::<ArcBytes>();
    }

    #[test]
    fn vec_blocks() {
        exercise::<Blocks<Vec<Block>>>();
    }

    #[test]
    fn imbl_blocks() {
        exercise::<Blocks<imbl::GenericVector<Block, archery::ArcK>>>();
    }

    #[test]
    fn rpds_blocks() {
        exercise::<Blocks<rpds::VectorSync<Block>>>();
    }

    #[test]
    fn radix_blocks() {
        exercise::<Blocks<RadixBlocks>>();
    }
}
