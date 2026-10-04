// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
use sysio::io;
use std::path::Path;

pub(crate) fn create_symlink(source: &Path, dest: &Path) -> io::Result<()> {
    rustix::fs::symlink(source, dest).map_err(io::Error::from)
}
