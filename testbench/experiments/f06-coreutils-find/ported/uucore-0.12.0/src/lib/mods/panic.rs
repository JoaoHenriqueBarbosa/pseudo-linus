// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

//! Custom panic hooks that allow silencing certain types of errors.
//!
//! Use the [`mute_sigpipe_panic`] function to silence panics caused by
//! broken pipe errors. This can happen when a process is still
//! producing data when the consuming process terminates and closes the
//! pipe. For example,
//!
//! ```sh
//! $ seq inf | head -n 1
//! ```
//!
use std::panic::PanicHookInfo;

/// Decide whether a panic was caused by a broken pipe (SIGPIPE) error.
fn is_broken_pipe(info: &PanicHookInfo) -> bool {
    info.payload()
        .downcast_ref::<String>()
        .is_some_and(|res| res.contains("BrokenPipe") || res.contains("Broken pipe"))
}

/// Terminate without error on panics that occur due to broken pipe errors.
///
/// For background discussions on `SIGPIPE` handling, see
///
/// * `<https://github.com/uutils/coreutils/issues/374>`
/// * `<https://github.com/uutils/coreutils/pull/1106>`
/// * `<https://github.com/rust-lang/rust/issues/62569>`
/// * `<https://github.com/BurntSushi/ripgrep/issues/200>`
/// * `<https://github.com/crev-dev/cargo-crev/issues/287>`
///
/// Porte pseudo-linus: o hook de panic é global do processo host (vale pra todos os
/// pseudo-processos e pro próprio kernel). Quem decide o que fazer com panic é o pseudo-kernel;
/// aqui só fica a classificação.
pub fn mute_sigpipe_panic() {}

/// Diz se um panic veio de escrita em pipe fechado (o pseudo-kernel usa pra mapear em SIGPIPE).
pub fn panic_is_broken_pipe(info: &PanicHookInfo) -> bool {
    is_broken_pipe(info)
}

/// Preserve inherited SIGPIPE settings from parent process.
///
/// Porte pseudo-linus: `signal(SIGPIPE)` e `remove_var` mexiam no processo host (unsafe). A
/// disposição de SIGPIPE de um pseudo-processo é estado do pseudo-kernel.
pub fn preserve_inherited_sigpipe() {}
