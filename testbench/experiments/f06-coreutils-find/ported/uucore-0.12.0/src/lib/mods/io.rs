// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

//! Encapsulates differences between OSs regarding the access to
//! file handles / descriptors.
//! This is useful when dealing with lower level stdin/stdout access.
//!
//! In detail:
//! On unix like OSs, file _descriptors_ are used in this context.
//! On windows OSs, file _handles_ are used.
//!
//! Even though they are distinct classes, they share common functionality.
//! Access to this common functionality is provided in `OwnedFileDescriptorOrHandle`.

use std::io;

// Porte pseudo-linus: RawReader/RawWriter faziam read(2)/write(2) direto no fd (`AsFd` + rustix)
// pra pular o buffer do std. No pseudo-processo não há fd do host: viram embrulhos sobre qualquer
// Read/Write. `OwnedFileDescriptorOrHandle` (dup de fd e conversão pra `std::process::Stdio`) saiu,
// nenhum dos utilitários portados usa.

// create reader without buffering
pub struct RawReader<T: io::Read>(pub T);
impl<T: io::Read> io::Read for RawReader<T> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.0.read(b)
    }
}

// create writer without buffering
pub struct RawWriter<T: io::Write>(pub T);
impl<T: io::Write> io::Write for RawWriter<T> {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Read and discard up to `n` bytes from `reader`, using a `buf_size` buffer.
///
/// Returns the number of bytes actually read; a value less than `n` means the
/// reader hit EOF first. Reads are retried on [`io::ErrorKind::Interrupted`].
/// This is used to skip over the start of an input that cannot be `seek`ed.
pub fn read_and_discard<R: io::Read>(reader: &mut R, n: u64, buf_size: usize) -> io::Result<u64> {
    use io::Read;
    let mut buf = Vec::new();
    buf.try_reserve(buf_size.min(n as usize))?;
    let mut total = 0u64;
    while total < n {
        let to_read = (n - total).min(buf_size as u64);
        buf.clear();
        match reader.by_ref().take(to_read).read_to_end(&mut buf) {
            Ok(0) => break, // EOF
            Ok(read) => total += read as u64,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::read_and_discard;
    use std::io::Cursor;

    #[test]
    fn discard_within_input() {
        let mut r = Cursor::new(b"abcdefgh".to_vec());
        assert_eq!(read_and_discard(&mut r, 3, 4).unwrap(), 3);
        assert_eq!(r.position(), 3);
    }

    #[test]
    fn discard_stops_at_eof() {
        let mut r = Cursor::new(b"abc".to_vec());
        // Asking for more than is available returns only what was read.
        assert_eq!(read_and_discard(&mut r, 100, 4).unwrap(), 3);
    }
}
