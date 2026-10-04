// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// Specific implementation for OpenBSD: tool unsupported (utmpx not supported)

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
#[allow(unused_imports)]
use sysio::{println};
use crate::uu_app;

use uucore::error::UResult;
use uucore::translate;

pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let _matches = uucore::clap_localization::handle_clap_result(uu_app(), args)?;
    println!("{}", translate!("who-unsupported-openbsd"));
    Ok(())
}
