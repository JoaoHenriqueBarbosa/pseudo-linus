// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// mods ~ cross-platforms modules (core/bundler file)

pub mod clap_localization;
pub mod display;
pub mod error;
// Porte pseudo-linus: linha de comando com a semântica e as mensagens do getopt_long da glibc.
pub mod gnu_getopt;
#[cfg(feature = "fs")]
pub mod io;
pub mod line_ending;
pub mod locale;
pub mod os;
pub mod panic;
pub mod posix;
