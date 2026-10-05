//! `runuser` do util-linux 2.41: o `su` sem autenticação, só para root (ver `su`).

use std::ffi::OsString;

use crate::util::io;

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| crate::su::run(args, true))
}
