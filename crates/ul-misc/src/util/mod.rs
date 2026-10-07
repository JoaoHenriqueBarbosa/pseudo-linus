//! Peças comuns dos programas do crate: `getopt_long` da glibc, E/S sobre `sysabi`, hora local e
//! largura de exibição.

pub mod io;
pub mod time;
pub mod tzif;
pub mod ul;

// O `getopt_long` mora no `ul-common`; o caminho `util::getopt` segue valendo pros programas daqui e
// do `ul-procps`.
pub use ul_common::getopt;
pub use ul_common::getopt::{Getopt, GetoptError, HasArg, LongOpt, Opt};

// Largura de exibição em C.UTF-8 (o `wcswidth` da glibc, e o `mbsnwidth` do gnulib pros bytes): mora
// no `ul-common`; o caminho `util::display_width` segue valendo pros programas daqui.
pub use ul_common::width::{display_width, display_width_bytes};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        assert_eq!(display_width("ção"), 3);
        assert_eq!(display_width("日本"), 4);
        assert_eq!(display_width_bytes(b"a\xffb"), 3);
    }
}
