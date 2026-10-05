//! `zip` (Info-ZIP 3.0) e `unzip` (Info-ZIP 6.0).

pub mod unzip;

use std::ffi::OsString;

use sysabi::Ctx;

use crate::sysutil;

pub fn zip_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv = sysutil::args_bytes(args);
    sysutil::error(&sysutil::argv0(&argv), "em construção");
    2
}

pub fn unzip_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    unzip::main(&sysutil::args_bytes(args))
}
