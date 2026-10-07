//! `parse_size` do `lib/strutils.c` do util-linux (domínio público): a mesma conversão que as
//! ferramentas do util-linux usam (`crate::util::ul::parse_size`), aqui com os casos conferidos com
//! `hexdump -n` no oráculo.

pub use crate::util::ul::parse_size;

#[cfg(test)]
mod tests {
    use super::*;
    use sysabi::Errno;

    // Valores conferidos com `hexdump -n` no oráculo (ver os casos `hexdump-size-*`).
    #[test]
    fn sizes() {
        assert_eq!(parse_size(b"5"), Ok(5));
        assert_eq!(parse_size(b"0x10"), Ok(16));
        assert_eq!(parse_size(b"010"), Ok(8));
        assert_eq!(parse_size(b"1K"), Ok(1024));
        assert_eq!(parse_size(b"1KiB"), Ok(1024));
        assert_eq!(parse_size(b"1KB"), Ok(1000));
        assert_eq!(parse_size(b"1.5K"), Ok(1536));
        assert_eq!(parse_size(b"1.K"), Ok(1024));
        assert_eq!(parse_size(b"0.5MB"), Ok(500_000));
        assert_eq!(parse_size(b"0.5MiB"), Ok(524_288));
        assert_eq!(parse_size(b"2k"), Ok(2048));
        assert_eq!(parse_size(b"abc"), Err(Errno::EINVAL));
        assert_eq!(parse_size(b"-1"), Err(Errno::EINVAL));
        assert_eq!(parse_size(b"5x"), Err(Errno::EINVAL));
        assert_eq!(parse_size(b""), Err(Errno::EINVAL));
        assert_eq!(parse_size(b"1R"), Err(Errno::EINVAL));
        assert_eq!(parse_size(b"1.5"), Err(Errno::EINVAL));
        assert_eq!(parse_size(b"99999999999999999999999"), Err(Errno::ERANGE));
        assert_eq!(parse_size(b"1Y"), Err(Errno::ERANGE));
        assert_eq!(parse_size(b" +7"), Ok(7));
    }
}
