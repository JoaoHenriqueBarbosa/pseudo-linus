//! `diff`, `cmp`, `diff3`, `sdiff` e `patch` do pseudo-linus.
//!
//! Alvo: GNU diffutils 3.10 e GNU patch 2.8 do Debian 13, byte a byte (saída, mensagens, códigos de
//! saída). Escrito a partir de especificação, documentação e comportamento observado no oráculo, nunca
//! do código GPL. Todo I/O passa pelo `sysabi`.

pub mod getopt;
pub mod sysutil;
pub mod tz;

pub mod cmp;
pub mod diff;
pub mod diff3;
pub mod patch;
pub mod sdiff;

use sysabi::Program;

/// Tabela de programas deste crate.
pub fn programs() -> Vec<Program> {
    vec![
        Program::bin("diff", diff::main),
        Program::bin("cmp", cmp::main),
        Program::bin("diff3", diff3::main),
        Program::bin("sdiff", sdiff::main),
        Program::bin("patch", patch::main),
    ]
}
