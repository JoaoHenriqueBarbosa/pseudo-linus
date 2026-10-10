//! O `WriteBatch` do leveldb (`db/write_batch.cc`): a representação que vai para o log. Um cabeçalho de 12 bytes (a
//! sequência do primeiro registro, 8 bytes, e a quantidade, 4 bytes, ambos little endian) e, para cada operação, a
//! tag (1 = `Put`, 0 = `Delete`) seguida da chave e, no `Put`, do valor, cada um prefixado pelo tamanho.

use crate::coding::{put_length_prefixed, Input};
use crate::{Error, Result};

const HEADER: usize = 12;
const TAG_DELETE: u8 = 0;
const TAG_PUT: u8 = 1;

/// Uma operação de um lote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteBatch {
    rep: Vec<u8>,
}

impl WriteBatch {
    pub fn new() -> WriteBatch {
        WriteBatch { rep: vec![0; HEADER] }
    }

    /// Lê um lote do conteúdo de um registro de log.
    pub fn from_contents(contents: &[u8]) -> Result<WriteBatch> {
        if contents.len() < HEADER {
            return Err(Error::Corruption("lote menor que o cabeçalho".to_owned()));
        }
        Ok(WriteBatch { rep: contents.to_vec() })
    }

    pub fn put(&mut self, key: &[u8], value: &[u8]) {
        self.set_count(self.count() + 1);
        self.rep.push(TAG_PUT);
        put_length_prefixed(&mut self.rep, key);
        put_length_prefixed(&mut self.rep, value);
    }

    pub fn delete(&mut self, key: &[u8]) {
        self.set_count(self.count() + 1);
        self.rep.push(TAG_DELETE);
        put_length_prefixed(&mut self.rep, key);
    }

    pub fn sequence(&self) -> u64 {
        u64::from_le_bytes(self.rep[..8].try_into().unwrap_or([0; 8]))
    }

    pub fn set_sequence(&mut self, sequence: u64) {
        self.rep[..8].copy_from_slice(&sequence.to_le_bytes());
    }

    pub fn count(&self) -> u32 {
        u32::from_le_bytes(self.rep[8..12].try_into().unwrap_or([0; 4]))
    }

    fn set_count(&mut self, count: u32) {
        self.rep[8..12].copy_from_slice(&count.to_le_bytes());
    }

    /// O que vai para o log.
    pub fn contents(&self) -> &[u8] {
        &self.rep
    }

    /// As operações, na ordem; o erro é o do `WriteBatch::Iterate` (tag ruim, quantidade que não bate).
    pub fn ops(&self) -> Result<Vec<Op>> {
        let mut input = Input::new(&self.rep[HEADER..]);
        let mut ops = Vec::new();
        while !input.is_empty() {
            let tag = input.take(1)?[0];
            let key = input.length_prefixed()?.to_vec();
            match tag {
                TAG_PUT => ops.push(Op::Put { key, value: input.length_prefixed()?.to_vec() }),
                TAG_DELETE => ops.push(Op::Delete { key }),
                other => return Err(Error::Corruption(format!("tag {other} desconhecida no lote"))),
            }
        }
        if ops.len() != self.count() as usize {
            return Err(Error::Corruption("quantidade errada de operações no lote".to_owned()));
        }
        Ok(ops)
    }
}

impl Default for WriteBatch {
    fn default() -> WriteBatch {
        WriteBatch::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_and_delete_round_trip() {
        let mut batch = WriteBatch::new();
        batch.set_sequence(5);
        batch.put(b"VERSION", b"1");
        batch.delete(b"gone");
        assert_eq!(batch.count(), 2);
        assert_eq!(batch.sequence(), 5);
        let again = WriteBatch::from_contents(batch.contents()).unwrap();
        assert_eq!(again.ops().unwrap(), vec![Op::Put { key: b"VERSION".to_vec(), value: b"1".to_vec() }, Op::Delete { key: b"gone".to_vec() }]);
    }

    /// O primeiro registro do log do Local Storage do oráculo: `VERSION` = `1` com a sequência 1.
    #[test]
    fn matches_the_oracle_encoding_of_one_put() {
        let mut batch = WriteBatch::new();
        batch.set_sequence(1);
        batch.put(b"VERSION", b"1");
        let expected = [1u8, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 7, b'V', b'E', b'R', b'S', b'I', b'O', b'N', 1, b'1'];
        assert_eq!(batch.contents(), expected);
    }

    #[test]
    fn a_wrong_count_or_tag_is_corruption() {
        let mut batch = WriteBatch::new();
        batch.put(b"k", b"v");
        let mut wrong = batch.contents().to_vec();
        wrong[8] = 2;
        assert!(WriteBatch::from_contents(&wrong).unwrap().ops().is_err());
        let mut tag = batch.contents().to_vec();
        tag[12] = 9;
        assert!(WriteBatch::from_contents(&tag).unwrap().ops().is_err());
        assert!(WriteBatch::from_contents(&[0; 5]).is_err());
    }
}
