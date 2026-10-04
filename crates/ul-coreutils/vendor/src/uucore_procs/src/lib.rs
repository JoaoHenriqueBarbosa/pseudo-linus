// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

//
// spell-checker:ignore SIGSEGV

//! A collection of procedural macros for uutils.
#![deny(missing_docs)]

use proc_macro::TokenStream;
use quote::quote;

//## rust proc-macro background info
//* ref: <https://dev.to/naufraghi/procedural-macro-in-rust-101-k3f> @@ <http://archive.is/Vbr5e>
//* ref: [path construction from LitStr](https://oschwald.github.io/maxminddb-rust/syn/struct.LitStr.html) @@ <http://archive.is/8YDua>

/// A procedural macro to define the main function of a uutils binary.
///
/// This macro handles:
/// - SIGPIPE state capture at process startup (before Rust runtime overrides it)
/// - SIGPIPE restoration to default if parent didn't explicitly ignore it
/// - Disabling Rust signal handlers for proper core dumps
/// - Error handling and exit code management
#[proc_macro_attribute]
pub fn main(args: TokenStream, stream: TokenStream) -> TokenStream {
    let stream = proc_macro2::TokenStream::from(stream);
    // Porte pseudo-linus: sem `.init_array`, sem mexer em disposição de sinal do processo host
    // (SIGPIPE, SIGSEGV, SIGBUS valem pra todos os pseudo-processos) e com stderr do pseudo-processo.
    let _ = args;

    let new = quote!(
        pub fn uumain(args: impl uucore::Args) -> i32 {
            #stream

            let result = uumain(args);
            match result {
                Ok(()) => uucore::error::get_exit_code(),
                Err(e) => {
                    let s = format!("{e}");
                    if s != "" {
                        uucore::show_error!("{s}");
                    }
                    if e.usage() {
                        use std::io::Write as _;
                        let _ = writeln!(uucore::sysio::io::stderr(),"Try '{} --help' for more information.", uucore::execution_phrase());
                    }
                    e.code()
                }
            }
        }
    );

    TokenStream::from(new)
}
