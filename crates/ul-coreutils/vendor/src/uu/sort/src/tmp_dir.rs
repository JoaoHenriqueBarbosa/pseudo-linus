// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

#[cfg(not(any(target_os = "redox", target_os = "wasi")))]
use std::path::Path;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
// Porte pseudo-linus: arquivos do VFS. Saíram `tempfile` (cria diretório no FS do host) e `ctrlc`
// (instala handler de SIGINT no processo host inteiro e chama `std::process::exit`). O `mode()` de
// `OpenOptions`/`DirBuilder` do shim é inerente, sem os traits `*Ext`.
use sysio::fs::{DirBuilder, File, OpenOptions};

use uucore::error::UResult;

use crate::SortError;

/// Diretório temporário no VFS do pseudo-processo, apagado no `Drop` do `TmpDirWrapper`.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new_in(parent: &Path) -> std::io::Result<Self> {
        for n in 0..10_000u32 {
            let path = parent.join(format!("uutils_sort{n:06}"));
            let mut builder = DirBuilder::new();
            #[cfg(unix)]
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

/// A wrapper around [`TempDir`] that may only exist once in a process.
///
/// `TmpDirWrapper` handles the allocation of new temporary files in this temporary directory and
/// deleting the whole directory when `SIGINT` is received. Creating a second `TmpDirWrapper` will
/// fail because `ctrlc::set_handler()` fails when there's already a handler.
/// The directory is only created once the first file is requested.
pub struct TmpDirWrapper {
    temp_dir: Option<TempDir>,
    parent_path: PathBuf,
    size: usize,
    lock: Arc<Mutex<()>>,
}

// Porte pseudo-linus: aqui ficava o registro global (`static LazyLock`) e o handler de SIGINT do
// `ctrlc`, que apagava o diretório temporário e chamava `std::process::exit(2)`. Sinal de
// pseudo-processo é do pseudo-kernel, que ainda não oferece "handler" pra programa Rust; sem ele,
// um SIGINT no meio de um sort externo deixa o diretório temporário no VFS.

impl TmpDirWrapper {
    pub fn new(path: PathBuf) -> Self {
        Self {
            parent_path: path,
            size: 0,
            temp_dir: None,
            lock: Arc::default(),
        }
    }

    fn init_tmp_dir(&mut self) -> UResult<()> {
        assert!(self.temp_dir.is_none());
        assert_eq!(self.size, 0);
        // The chunks hold a copy of the input, so keep them out of reach of other
        // users instead of relying on whatever umask the process inherited.
        self.temp_dir = Some(TempDir::new_in(&self.parent_path).map_err(|_| {
            SortError::TmpFileCreationFailed {
                path: self.parent_path.clone(),
            }
        })?);
        Ok(())
    }

    pub fn next_file(&mut self) -> UResult<(File, PathBuf)> {
        if self.temp_dir.is_none() {
            self.init_tmp_dir()?;
        }

        let _lock = self.lock.lock().unwrap();
        let file_name = self.size.to_string();
        self.size += 1;
        let path = self.temp_dir.as_ref().unwrap().path().join(file_name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        // Restrict the chunks too, so a directory whose mode is later relaxed
        // doesn't expose them.
        #[cfg(unix)]
        options.mode(0o600);
        Ok((
            options
                .open(&path)
                .map_err(|error| SortError::OpenTmpFileFailed { error })?,
            path,
        ))
    }

    /// Function just waits if signal handler was called
    pub fn wait_if_signal(&self) {
        let _lock = self.lock.lock().unwrap();
    }
}

impl Drop for TmpDirWrapper {
    fn drop(&mut self) {
        // Explicitly attempt cleanup before TempDir's Drop runs silently.
        // TempDir::drop uses `let _ = remove_dir_all()` which silently
        // ignores errors, potentially leaking the directory.
        #[cfg(not(any(target_os = "redox", target_os = "wasi")))]
        if let Some(ref temp_dir) = self.temp_dir {
            let _ = remove_tmp_dir(temp_dir.path());
        }
    }
}

/// Remove the directory at `path` by deleting its child files and then itself.
/// Errors while deleting child files are ignored.
#[cfg(not(any(target_os = "redox", target_os = "wasi")))]
fn remove_tmp_dir(path: &Path) -> std::io::Result<()> {
    if let Ok(read_dir) = sysio::fs::read_dir(path) {
        for file in read_dir.flatten() {
            // if we fail to delete the file here it was probably deleted by another thread
            // in the meantime, but that's ok.
            let _ = sysio::fs::remove_file(file.path());
        }
    }
    sysio::fs::remove_dir(path)
}

#[cfg(all(test, unix))]
mod tests {
    use super::TmpDirWrapper;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// Restores the process umask on drop, so a panic in the test cannot leak the
    /// value into the rest of the binary.
    struct UmaskGuard(libc::mode_t);

    impl UmaskGuard {
        fn set(mask: libc::mode_t) -> Self {
            // SAFETY: umask(2) has no failure mode; it returns the previous value.
            Self(unsafe { libc::umask(mask) })
        }
    }

    impl Drop for UmaskGuard {
        fn drop(&mut self) {
            unsafe { libc::umask(self.0) };
        }
    }

    #[test]
    fn tmp_files_are_private_regardless_of_umask() {
        // Pin a permissive umask: under 0077 the umask alone would produce 0700 and
        // 0600, so the assertions would hold for a broken implementation too. The
        // guard restores it, and the only other test here that creates files sets
        // the modes it cares about explicitly.
        let _umask = UmaskGuard::set(0o022);

        let parent = tempfile::tempdir().unwrap();
        let mut wrapper = TmpDirWrapper::new(parent.path().to_owned());
        let (_file, path) = wrapper.next_file().unwrap();

        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&path), 0o600);
    }
}
