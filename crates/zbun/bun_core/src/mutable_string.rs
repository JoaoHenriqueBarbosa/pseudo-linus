//! `bun_core::MutableString` (`string/MutableString.rs` do upstream): um buffer de bytes crescente, sem `unsafe`.
//! Saem `writable_n_bytes`, `expand_to_capacity` e o ponteiro `owns`, que só o código com `unsafe` do upstream usa.

use bun_alloc::AllocError;

use crate::strings;

#[derive(Default, Clone)]
pub struct MutableString {
    pub list: Vec<u8>,
}

impl MutableString {
    pub fn init2048() -> Result<MutableString, AllocError> {
        MutableString::init(2048)
    }

    pub fn init(capacity: usize) -> Result<MutableString, AllocError> {
        let mut list = Vec::new();
        list.try_reserve_exact(capacity).map_err(|_| AllocError)?;
        Ok(MutableString { list })
    }

    pub fn init_empty() -> MutableString {
        MutableString { list: Vec::new() }
    }

    pub(crate) fn init_copy(str: impl AsRef<[u8]>) -> Result<MutableString, AllocError> {
        let str = str.as_ref();
        let mut mutable = MutableString::init(str.len())?;
        mutable.list.extend_from_slice(str);
        Ok(mutable)
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    #[inline]
    pub fn grow_if_needed(&mut self, amount: usize) -> Result<(), AllocError> {
        self.list.try_reserve(amount).map_err(|_| AllocError)
    }

    #[inline]
    pub fn ensure_unused_capacity(&mut self, amount: usize) -> Result<(), AllocError> {
        self.grow_if_needed(amount)
    }

    #[inline]
    pub fn grow_by(&mut self, amount: usize) -> Result<(), AllocError> {
        self.grow_if_needed(amount)
    }

    #[inline]
    pub fn append_slice(&mut self, items: &[u8]) -> Result<(), AllocError> {
        self.list.extend_from_slice(items);
        Ok(())
    }

    #[inline]
    pub fn append_slice_exact(&mut self, items: &[u8]) -> Result<(), AllocError> {
        if items.is_empty() {
            return Ok(());
        }
        self.list.reserve_exact(items.len());
        self.list.extend_from_slice(items);
        Ok(())
    }

    #[inline]
    pub fn reset(&mut self) {
        self.list.clear();
    }

    /// Fixa o comprimento em `index`: encurta, ou completa com zeros quando `index` passa do comprimento (o upstream
    /// expõe os bytes não inicializados, aqui eles são zero).
    #[inline]
    pub fn reset_to(&mut self, index: usize) {
        if index <= self.list.len() {
            self.list.truncate(index);
        } else {
            self.list.resize(index, 0);
        }
    }

    pub fn inflate(&mut self, amount: usize) -> Result<(), AllocError> {
        self.list.resize(amount, 0);
        Ok(())
    }

    #[inline]
    pub fn append_char(&mut self, char: u8) -> Result<(), AllocError> {
        self.list.push(char);
        Ok(())
    }

    #[inline]
    pub fn append(&mut self, bytes: &[u8]) -> Result<(), AllocError> {
        self.list.extend_from_slice(bytes);
        Ok(())
    }

    #[inline]
    pub fn append_int(&mut self, int: u64) -> Result<(), AllocError> {
        let mut buf = crate::fmt::ItoaBuf::new();
        self.list.extend_from_slice(crate::fmt::itoa(&mut buf, int));
        Ok(())
    }

    #[inline]
    pub fn append_assume_capacity(&mut self, bytes: &[u8]) {
        self.list.extend_from_slice(bytes);
    }

    pub fn take_slice(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.list)
    }

    pub fn to_owned_slice(&mut self) -> Box<[u8]> {
        core::mem::take(&mut self.list).into_boxed_slice()
    }

    pub fn slice(&mut self) -> &mut [u8] {
        &mut self.list
    }

    pub fn write_all(&mut self, bytes: &[u8]) -> Result<usize, AllocError> {
        self.list.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    /// Converte `str` num identificador ES válido: cada trecho de pontos de código fora de `ID_Continue` vira um único
    /// `_`, e os válidos (não BMP incluídos) ficam. Palavra reservada do modo estrito é remapeada.
    pub fn ensure_valid_identifier(str: &[u8]) -> Result<Box<[u8]>, AllocError> {
        use crate::identifier::{is_identifier, is_identifier_part, is_identifier_start};

        if str.is_empty() {
            return Ok(Box::from(b"_".as_slice()));
        }
        let mut iterator = strings::CodepointIterator::init(str);
        let mut cursor = strings::Cursor::default();
        if !iterator.next(&mut cursor) {
            return Ok(Box::from(b"_".as_slice()));
        }

        // Caso comum: nenhuma lacuna, e o resultado é a própria entrada.
        let mut needs_gap = !is_identifier_start(cursor.c);
        let mut start_i: usize = 0;
        if !needs_gap {
            while iterator.next(&mut cursor) {
                if !is_identifier_part(cursor.c) {
                    needs_gap = true;
                    start_i = cursor.i as usize;
                    break;
                }
            }
        }
        if !needs_gap {
            let remapped = crate::lexer_tables::strict_mode_reserved_word_remap(str).unwrap_or(str);
            return Ok(Box::from(remapped));
        }

        let mut mutable = MutableString::init_copy(if start_i == 0 {
            // A primeira letra pode não ser início de identificador (oven-sh/bun#2946).
            b"_".as_slice()
        } else {
            &str[0..start_i]
        })?;
        needs_gap = false;

        let items = &str[start_i..];
        iterator = strings::CodepointIterator::init(items);
        cursor = strings::Cursor::default();
        while iterator.next(&mut cursor) {
            if is_identifier_part(cursor.c) {
                if needs_gap {
                    mutable.append_char(b'_')?;
                    needs_gap = false;
                }
                let i = cursor.i as usize;
                let w = cursor.width as usize;
                mutable.append(&items[i..i + w])?;
            } else if !needs_gap {
                // O ponto de código é pulado e vira um único `_`.
                needs_gap = true;
            }
        }
        // Terminou num emoji.
        if needs_gap {
            mutable.append_char(b'_')?;
        }
        debug_assert!(is_identifier(&mutable.list));
        Ok(mutable.to_owned_slice())
    }
}

impl crate::io::Write for MutableString {
    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> crate::CrateResult<()> {
        self.append(buf)?;
        Ok(())
    }
    #[inline]
    fn written_len(&self) -> usize {
        self.len()
    }
}

impl std::io::Write for MutableString {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.list.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid(s: &str) -> String {
        String::from_utf8(MutableString::ensure_valid_identifier(s.as_bytes()).unwrap().into_vec()).unwrap()
    }

    #[test]
    fn identifiers_are_repaired_like_the_upstream() {
        assert_eq!(valid(""), "_");
        assert_eq!(valid("abc"), "abc");
        assert_eq!(valid("1abc"), "_1abc");
        assert_eq!(valid("a-b--c"), "a_b_c");
        assert_eq!(valid("a-"), "a_");
        assert_eq!(valid("é-x"), "é_x");
        assert_eq!(valid("let"), "_let");
    }

    #[test]
    fn append_and_reset() {
        let mut m = MutableString::init(4).unwrap();
        m.append(b"abc").unwrap();
        m.append_int(42).unwrap();
        assert_eq!(&m.list, b"abc42");
        m.reset_to(3);
        assert_eq!(&m.list, b"abc");
        m.reset_to(5);
        assert_eq!(&m.list, b"abc\0\0");
        assert_eq!(m.take_slice(), b"abc\0\0");
        assert!(m.is_empty());
    }
}
