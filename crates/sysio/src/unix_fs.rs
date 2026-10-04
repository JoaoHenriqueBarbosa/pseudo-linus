//! Equivalentes de `std::os::unix::fs` pros tipos da fachada: `MetadataExt`, `PermissionsExt`,
//! `FileTypeExt`, `DirEntryExt`, `OpenOptionsExt`, `DirBuilderExt`, `FileExt` e as funções
//! `symlink`, `chown`, `lchown`, `fchown`, `chroot`, `mkfifo`. Os traits do std só valem pros tipos
//! do std, então o porte troca o caminho do import.

use std::io;

use crate::fs::{DirBuilder, DirEntry, File, FileType, Metadata, OpenOptions, Permissions};

pub use crate::fs::{chown, chroot, fchown, lchown, mkfifo, symlink};

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
        self.st.rdev
    }
    fn size(&self) -> u64 {
        self.st.size
    }
    fn atime(&self) -> i64 {
        self.st.atime.sec
    }
    fn atime_nsec(&self) -> i64 {
        i64::from(self.st.atime.nsec)
    }
    fn mtime(&self) -> i64 {
        self.st.mtime.sec
    }
    fn mtime_nsec(&self) -> i64 {
        i64::from(self.st.mtime.nsec)
    }
    fn ctime(&self) -> i64 {
        self.st.ctime.sec
    }
    fn ctime_nsec(&self) -> i64 {
        i64::from(self.st.ctime.nsec)
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
        self.mode & sysabi::mode::S_IFMT == sysabi::mode::S_IFBLK
    }
    fn is_char_device(&self) -> bool {
        self.mode & sysabi::mode::S_IFMT == sysabi::mode::S_IFCHR
    }
    fn is_fifo(&self) -> bool {
        self.mode & sysabi::mode::S_IFMT == sysabi::mode::S_IFIFO
    }
    fn is_socket(&self) -> bool {
        self.mode & sysabi::mode::S_IFMT == sysabi::mode::S_IFSOCK
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
    fn custom_flags(&mut self, flags: i32) -> &mut Self;
}

impl OpenOptionsExt for OpenOptions {
    fn mode(&mut self, mode: u32) -> &mut Self {
        OpenOptions::mode(self, mode)
    }
    fn custom_flags(&mut self, flags: i32) -> &mut Self {
        OpenOptions::custom_flags(self, flags)
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

/// `std::os::unix::fs::FileExt`: `pread`/`pwrite`.
pub trait FileExt {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize>;
    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<usize>;

    fn read_exact_at(&self, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
        while !buf.is_empty() {
            match self.read_at(buf, offset) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => {
                    buf = &mut buf[n..];
                    offset += n as u64;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn write_all_at(&self, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
        while !buf.is_empty() {
            match self.write_at(buf, offset) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => {
                    buf = &buf[n..];
                    offset += n as u64;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

impl FileExt for File {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> io::Result<usize> {
        File::read_at(self, buf, offset)
    }
    fn write_at(&self, buf: &[u8], offset: u64) -> io::Result<usize> {
        File::write_at(self, buf, offset)
    }
}
