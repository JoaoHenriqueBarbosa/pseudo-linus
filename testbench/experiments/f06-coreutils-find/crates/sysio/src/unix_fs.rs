//! Equivalentes de `std::os::unix::fs::{MetadataExt, PermissionsExt, FileTypeExt, DirEntryExt,
//! OpenOptionsExt, DirBuilderExt}` pros tipos do shim. Os do std são implementados só pros tipos do
//! std, então o porte troca o caminho do import.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::errno::*;
use crate::fs::{DirBuilder, DirEntry, FileType, Metadata, OpenOptions, Permissions};

pub use crate::fs::symlink;

fn secs(t: SystemTime) -> (i64, i64) {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => (d.as_secs() as i64, d.subsec_nanos() as i64),
        Err(e) => {
            let d = e.duration();
            let mut s = -(d.as_secs() as i64);
            let mut n = d.subsec_nanos() as i64;
            if n > 0 {
                s -= 1;
                n = 1_000_000_000 - n;
            }
            (s, n)
        }
    }
}

pub trait MetadataExt {
    fn dev(&self) -> u64;
    fn ino(&self) -> u64;
    fn mode(&self) -> u32;
    fn nlink(&self) -> u64;
    fn uid(&self) -> u32;
    fn gid(&self) -> u32;
    fn rdev(&self) -> u64;
    fn size(&self) -> u64;
    fn atime(&self) -> i64;
    fn atime_nsec(&self) -> i64;
    fn mtime(&self) -> i64;
    fn mtime_nsec(&self) -> i64;
    fn ctime(&self) -> i64;
    fn ctime_nsec(&self) -> i64;
    fn blksize(&self) -> u64;
    fn blocks(&self) -> u64;
}

impl MetadataExt for Metadata {
    fn dev(&self) -> u64 {
        self.st.dev
    }
    fn ino(&self) -> u64 {
        self.st.ino
    }
    fn mode(&self) -> u32 {
        self.st.mode
    }
    fn nlink(&self) -> u64 {
        self.st.nlink
    }
    fn uid(&self) -> u32 {
        self.st.uid
    }
    fn gid(&self) -> u32 {
        self.st.gid
    }
    fn rdev(&self) -> u64 {
        0
    }
    fn size(&self) -> u64 {
        self.st.size
    }
    fn atime(&self) -> i64 {
        secs(self.st.atime).0
    }
    fn atime_nsec(&self) -> i64 {
        secs(self.st.atime).1
    }
    fn mtime(&self) -> i64 {
        secs(self.st.mtime).0
    }
    fn mtime_nsec(&self) -> i64 {
        secs(self.st.mtime).1
    }
    fn ctime(&self) -> i64 {
        secs(self.st.ctime).0
    }
    fn ctime_nsec(&self) -> i64 {
        secs(self.st.ctime).1
    }
    fn blksize(&self) -> u64 {
        self.st.blksize
    }
    fn blocks(&self) -> u64 {
        self.st.blocks
    }
}

pub trait PermissionsExt {
    fn mode(&self) -> u32;
    fn set_mode(&mut self, mode: u32);
    fn from_mode(mode: u32) -> Self;
}

impl PermissionsExt for Permissions {
    fn mode(&self) -> u32 {
        self.mode
    }
    fn set_mode(&mut self, mode: u32) {
        self.mode = mode;
    }
    fn from_mode(mode: u32) -> Self {
        Permissions { mode }
    }
}

pub trait FileTypeExt {
    fn is_block_device(&self) -> bool;
    fn is_char_device(&self) -> bool;
    fn is_fifo(&self) -> bool;
    fn is_socket(&self) -> bool;
}

impl FileTypeExt for FileType {
    fn is_block_device(&self) -> bool {
        self.mode & S_IFMT == S_IFBLK
    }
    fn is_char_device(&self) -> bool {
        self.mode & S_IFMT == S_IFCHR
    }
    fn is_fifo(&self) -> bool {
        self.mode & S_IFMT == S_IFIFO
    }
    fn is_socket(&self) -> bool {
        self.mode & S_IFMT == S_IFSOCK
    }
}

pub trait DirEntryExt {
    fn ino(&self) -> u64;
}

impl DirEntryExt for DirEntry {
    fn ino(&self) -> u64 {
        DirEntry::ino(self)
    }
}

pub trait OpenOptionsExt {
    fn mode(&mut self, mode: u32) -> &mut Self;
}

impl OpenOptionsExt for OpenOptions {
    fn mode(&mut self, mode: u32) -> &mut Self {
        OpenOptions::mode(self, mode)
    }
}

pub trait DirBuilderExt {
    fn mode(&mut self, mode: u32) -> &mut Self;
}

impl DirBuilderExt for DirBuilder {
    fn mode(&mut self, mode: u32) -> &mut Self {
        DirBuilder::mode(self, mode)
    }
}
