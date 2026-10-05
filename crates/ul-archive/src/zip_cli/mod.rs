//! `zip` (Info-ZIP 3.0) e `unzip` (Info-ZIP 6.0).

pub mod unzip;
pub mod zip;

use std::ffi::OsString;

use sysabi::Ctx;

use crate::sysutil;

pub fn zip_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    zip::main(&sysutil::args_bytes(args))
}

pub fn unzip_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    unzip::main(&sysutil::args_bytes(args))
}
