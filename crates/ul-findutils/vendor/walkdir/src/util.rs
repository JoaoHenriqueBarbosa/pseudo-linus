use std::io;
use std::path::Path;

#[cfg(unix)]
pub fn device_num<P: AsRef<Path>>(path: P) -> io::Result<u64> {
    // Porte pseudo-linus: `Path::metadata()` faria stat no FS do host.
    use sysio::os::unix::fs::MetadataExt;

    sysio::fs::metadata(path.as_ref()).map(|md| md.dev())
}

#[cfg(windows)]
pub fn device_num<P: AsRef<Path>>(path: P) -> io::Result<u64> {
    use winapi_util::{file, Handle};

    let h = Handle::from_path_any(path)?;
    file::information(h).map(|info| info.volume_serial_number())
}

#[cfg(not(any(unix, windows)))]
pub fn device_num<P: AsRef<Path>>(_: P) -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Other,
        "walkdir: same_file_system option not supported on this platform",
    ))
}
