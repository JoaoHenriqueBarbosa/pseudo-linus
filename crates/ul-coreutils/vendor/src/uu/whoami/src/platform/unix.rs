// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
use std::ffi::OsString;
use sysio::io;

use uucore::entries::uid2usr;

pub fn get_username() -> io::Result<OsString> {
    // uid2usr should arguably return an OsString but currently doesn't
    // Porte pseudo-linus: geteuid(2) do pseudo-processo.
    uid2usr(sysio::users::geteuid()).map(Into::into)
}
