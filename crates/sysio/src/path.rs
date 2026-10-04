//! Os métodos inerentes de `std::path::Path` que tocam o FS (`exists`, `is_dir`, `metadata`...)
//! chamam o kernel do host e não podem ser sombreados por import (método inerente ganha de método de
//! trait). O porte troca `p.exists()` por `p.sys_exists()` com este trait; o nome diferente deixa a
//! troca mecânica e fácil de conferir com grep.

use std::io;
use std::path::{Path, PathBuf};

use crate::fs::{self, Metadata, ReadDir};

pub trait PathExt {
    fn sys_exists(&self) -> bool;
    fn sys_try_exists(&self) -> io::Result<bool>;
    fn sys_is_dir(&self) -> bool;
    fn sys_is_file(&self) -> bool;
    fn sys_is_symlink(&self) -> bool;
    fn sys_metadata(&self) -> io::Result<Metadata>;
    fn sys_symlink_metadata(&self) -> io::Result<Metadata>;
    fn sys_read_link(&self) -> io::Result<PathBuf>;
    fn sys_canonicalize(&self) -> io::Result<PathBuf>;
    fn sys_read_dir(&self) -> io::Result<ReadDir>;
}

impl PathExt for Path {
    fn sys_exists(&self) -> bool {
        fs::exists(self)
    }
    fn sys_try_exists(&self) -> io::Result<bool> {
        fs::try_exists(self)
    }
    fn sys_is_dir(&self) -> bool {
        fs::is_dir(self)
    }
    fn sys_is_file(&self) -> bool {
        fs::is_file(self)
    }
    fn sys_is_symlink(&self) -> bool {
        fs::is_symlink(self)
    }
    fn sys_metadata(&self) -> io::Result<Metadata> {
        fs::metadata(self)
    }
    fn sys_symlink_metadata(&self) -> io::Result<Metadata> {
        fs::symlink_metadata(self)
    }
    fn sys_read_link(&self) -> io::Result<PathBuf> {
        fs::read_link(self)
    }
    fn sys_canonicalize(&self) -> io::Result<PathBuf> {
        fs::canonicalize(self)
    }
    fn sys_read_dir(&self) -> io::Result<ReadDir> {
        fs::read_dir(self)
    }
}

impl PathExt for PathBuf {
    fn sys_exists(&self) -> bool {
        self.as_path().sys_exists()
    }
    fn sys_try_exists(&self) -> io::Result<bool> {
        self.as_path().sys_try_exists()
    }
    fn sys_is_dir(&self) -> bool {
        self.as_path().sys_is_dir()
    }
    fn sys_is_file(&self) -> bool {
        self.as_path().sys_is_file()
    }
    fn sys_is_symlink(&self) -> bool {
        self.as_path().sys_is_symlink()
    }
    fn sys_metadata(&self) -> io::Result<Metadata> {
        self.as_path().sys_metadata()
    }
    fn sys_symlink_metadata(&self) -> io::Result<Metadata> {
        self.as_path().sys_symlink_metadata()
    }
    fn sys_read_link(&self) -> io::Result<PathBuf> {
        self.as_path().sys_read_link()
    }
    fn sys_canonicalize(&self) -> io::Result<PathBuf> {
        self.as_path().sys_canonicalize()
    }
    fn sys_read_dir(&self) -> io::Result<ReadDir> {
        self.as_path().sys_read_dir()
    }
}
