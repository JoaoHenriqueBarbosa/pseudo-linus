//! `zip` (Info-ZIP 3.0) e `unzip` (Info-ZIP 6.0).

pub mod unzip;
pub mod zip;

mod funzip;
mod zipcloak;
mod zipnote;
mod zipsplit;
mod ztools;

use std::ffi::OsString;

use sysabi::Ctx;

use crate::sysutil;

pub fn zip_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    zip::main(&sysutil::args_bytes(args))
}

pub fn zipnote_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    zipnote::main(&sysutil::args_bytes(args))
}

pub fn zipcloak_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    zipcloak::main(&sysutil::args_bytes(args))
}

pub fn unzipsfx_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    unzip::main_sfx(&sysutil::args_bytes(args))
}

pub fn zipsplit_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    zipsplit::main(&sysutil::args_bytes(args))
}

pub fn funzip_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    funzip::main(&sysutil::args_bytes(args))
}

pub fn unzip_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    unzip::main(&sysutil::args_bytes(args))
}
