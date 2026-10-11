//! O console e o sistema de arquivos de módulos do zjsc sobre o pseudo-processo: o stdout, o stderr e o stdin são os
//! fds 1, 2 e 0, e `import`, `require` e `fetch('file://...')` leem o VFS do sandbox.

use std::cell::{Cell, RefCell};

use sysabi::{sys, Errno, Fd};
use zjsc::runtime::console_host::ConsoleHost;
use zjsc::runtime::module_fs::{EntryKind, ModuleFs};

/// Os fds do processo. A escrita não tem buffer, como a do bun: a ordem entre stdout e stderr (`2>&1`) é a do programa.
#[derive(Debug, Default)]
pub struct FdConsole {
    pending: RefCell<Vec<u8>>,
    eof: Cell<bool>,
}

impl FdConsole {
    /// Lê mais um pedaço do stdin para `pending`; `false` em EOF ou erro.
    fn fill(&self) -> bool {
        if self.eof.get() {
            return false;
        }
        let mut chunk = [0u8; 8192];
        loop {
            match sys::read(Fd::STDIN, &mut chunk) {
                Ok(0) => break,
                Ok(count) => {
                    self.pending.borrow_mut().extend_from_slice(&chunk[..count]);
                    return true;
                }
                Err(Errno::EINTR) => {}
                Err(_) => break,
            }
        }
        self.eof.set(true);
        false
    }
}

impl ConsoleHost for FdConsole {
    fn write_stdout(&self, bytes: &[u8]) {
        let _ = sys::write_all(Fd::STDOUT, bytes);
    }

    fn write_stderr(&self, bytes: &[u8]) {
        let _ = sys::write_all(Fd::STDERR, bytes);
    }

    fn read_stdin_line(&self) -> Option<String> {
        let mut searched = 0;
        loop {
            let newline = self.pending.borrow()[searched..].iter().position(|&byte| byte == b'\n');
            if let Some(offset) = newline {
                let mut pending = self.pending.borrow_mut();
                let line = String::from_utf8_lossy(&pending[..searched + offset]).into_owned();
                pending.drain(..=searched + offset);
                return Some(line);
            }
            searched = self.pending.borrow().len();
            if !self.fill() {
                return None;
            }
        }
    }

    fn read_stdin_rest(&self) -> Vec<u8> {
        while self.fill() {}
        std::mem::take(&mut *self.pending.borrow_mut())
    }
}

/// Os arquivos do sandbox, como o carregador de módulos os pede: caminhos absolutos e normalizados.
#[derive(Debug, Default)]
pub struct SandboxFs;

impl ModuleFs for SandboxFs {
    fn read_file(&self, path: &str) -> Option<String> {
        String::from_utf8(sysio::fs::read(path).ok()?).ok()
    }

    fn read_bytes(&self, path: &str) -> Option<Vec<u8>> {
        sysio::fs::read(path).ok()
    }

    fn stat(&self, path: &str) -> Option<EntryKind> {
        let metadata = sysio::fs::metadata(path).ok()?;
        if metadata.is_dir() {
            Some(EntryKind::Directory)
        } else if metadata.is_file() {
            Some(EntryKind::File)
        } else {
            None
        }
    }

    fn realpath(&self, path: &str) -> Option<String> {
        Some(sysio::fs::canonicalize(path).ok()?.to_string_lossy().into_owned())
    }
}
