//! Programas do GNU binutils 2.44 (Debian 13) que o pseudo-linus porta, um módulo por programa:
//! `ar` e `ranlib` (formato ar GNU), `size`, `nm`, `readelf` e `strip` sobre ELF64 x86_64, e
//! `c++filt` (demangler Itanium em [`demangle`]). O `strings` fica em [`crate::strings`], de
//! antes desta pasta.

pub mod addr2line;
pub mod ar;
pub mod cxxfilt;
pub mod demangle;
pub mod elf;
pub mod elfedit;
pub mod gas;
pub mod gprof;
pub mod gprofng;
pub mod ld;
pub mod nm;
pub mod objcopy;
pub mod objdump;
pub mod readelf;
pub mod size;
pub mod strip;
