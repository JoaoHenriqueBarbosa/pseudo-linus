//! `bun_core::io`: o `Write` de bytes do upstream (`bun_io::Write`, definido em `bun_core::util`), sem a ponte para o
//! vtable `Writer` do stdout e sem o `BabyVec` do arena, que o porte não tem.

use core::fmt;

use crate::{CrateError, CrateResult};

/// Resultado de escrita: o `bun_io::Result` do upstream.
pub type Result<T> = CrateResult<T>;

/// Sorvedouro de bytes. Só o `write_all` é obrigatório; o resto tem padrão em função dele. Objeto-seguro: `&mut dyn
/// Write` funciona, e os ajudantes genéricos levam `where Self: Sized`.
pub trait Write {
    /// Escreve o buffer inteiro.
    fn write_all(&mut self, buf: &[u8]) -> CrateResult<()>;

    /// Descarrega o buffer interno; sorvedouro sem buffer deixa o padrão.
    #[inline]
    fn flush(&mut self) -> CrateResult<()> {
        Ok(())
    }

    /// Total de bytes escritos até agora; só os sorvedouros em memória ou contadores implementam, o padrão entra em pânico.
    #[inline]
    fn written_len(&self) -> usize {
        panic!("io::Write::written_len: writer does not track bytes written");
    }

    #[inline]
    fn write_byte(&mut self, byte: u8) -> CrateResult<()> {
        self.write_all(core::slice::from_ref(&byte))
    }

    #[inline]
    fn write_str(&mut self, s: &str) -> CrateResult<()> {
        self.write_all(s.as_bytes())
    }

    /// Escreve `n` cópias de `byte`.
    fn splat_byte_all(&mut self, byte: u8, n: usize) -> CrateResult<()> {
        let chunk = [byte; 256];
        let mut remain = n;
        while remain > 0 {
            let take = remain.min(chunk.len());
            self.write_all(&chunk[..take])?;
            remain -= take;
        }
        Ok(())
    }

    /// Escrita formatada: habilita `write!(w, ...)`.
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> CrateResult<()> {
        struct Bridge<'a, W: ?Sized> {
            sink: &'a mut W,
            err: Option<CrateError>,
        }
        impl<W: Write + ?Sized> fmt::Write for Bridge<'_, W> {
            #[inline]
            fn write_str(&mut self, s: &str) -> fmt::Result {
                match self.sink.write_all(s.as_bytes()) {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        self.err = Some(e);
                        Err(fmt::Error)
                    }
                }
            }
        }
        let mut bridge = Bridge { sink: self, err: None };
        match fmt::write(&mut bridge, args) {
            Ok(()) => Ok(()),
            Err(_) => Err(bridge.err.unwrap_or(CrateError::FmtError)),
        }
    }

    /// Apelido do [`write_fmt`](Write::write_fmt).
    #[inline]
    fn print(&mut self, args: fmt::Arguments<'_>) -> CrateResult<()> {
        self.write_fmt(args)
    }

    /// Escreve um inteiro em little-endian.
    #[inline]
    fn write_int_le<I: IntLe>(&mut self, val: I) -> CrateResult<()>
    where
        Self: Sized,
    {
        self.write_all(val.to_le_bytes().as_ref())
    }
}

/// Repassa por `&mut W`, para `&mut dyn Write` e `&mut impl Write` aninharem.
impl<W: Write + ?Sized> Write for &mut W {
    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> CrateResult<()> {
        (**self).write_all(buf)
    }
    #[inline]
    fn flush(&mut self) -> CrateResult<()> {
        (**self).flush()
    }
    #[inline]
    fn write_byte(&mut self, byte: u8) -> CrateResult<()> {
        (**self).write_byte(byte)
    }
    #[inline]
    fn splat_byte_all(&mut self, byte: u8, n: usize) -> CrateResult<()> {
        (**self).splat_byte_all(byte, n)
    }
    #[inline]
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> CrateResult<()> {
        (**self).write_fmt(args)
    }
    #[inline]
    fn written_len(&self) -> usize {
        (**self).written_len()
    }
}

impl<W: Write + ?Sized> Write for Box<W> {
    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> CrateResult<()> {
        (**self).write_all(buf)
    }
    #[inline]
    fn flush(&mut self) -> CrateResult<()> {
        (**self).flush()
    }
    #[inline]
    fn written_len(&self) -> usize {
        (**self).written_len()
    }
}

/// Sorvedouro crescente em memória.
impl Write for Vec<u8> {
    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> CrateResult<()> {
        self.extend_from_slice(buf);
        Ok(())
    }
    #[inline]
    fn written_len(&self) -> usize {
        self.len()
    }
}

/// Inteiros que o [`Write::write_int_le`] escreve em little-endian.
pub trait IntLe: Copy {
    type Bytes: AsRef<[u8]> + AsMut<[u8]> + Default;
    fn to_le_bytes(self) -> Self::Bytes;
    fn from_le_bytes(bytes: Self::Bytes) -> Self;
}

macro_rules! impl_int_le {
    ($($t:ty),* $(,)?) => {$(
        impl IntLe for $t {
            type Bytes = [u8; core::mem::size_of::<$t>()];
            #[inline]
            fn to_le_bytes(self) -> Self::Bytes { <$t>::to_le_bytes(self) }
            #[inline]
            fn from_le_bytes(bytes: Self::Bytes) -> Self { <$t>::from_le_bytes(bytes) }
        }
    )*};
}
impl_int_le!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec_sink_formats_and_splats() {
        let mut out: Vec<u8> = Vec::new();
        write!(out, "a{}", 1).unwrap();
        out.splat_byte_all(b'-', 300).unwrap();
        out.write_int_le(0x0102u16).unwrap();
        assert_eq!(&out[..2], b"a1");
        assert_eq!(out.len(), 2 + 300 + 2);
        assert_eq!(&out[302..], &[2, 1]);
        assert_eq!(out.written_len(), 304);
    }
}
