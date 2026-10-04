// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (misc) uioerror

// Porte pseudo-linus: E/S, FS, ambiente, processos e threads do pseudo-processo (sysio).
use crate::filetime::FileTime;
use std::path::PathBuf;
use thiserror::Error;
use uucore::display::Quotable;
use uucore::error::{UError, UIoError};
use uucore::translate;

#[derive(Debug, Error)]
pub enum TouchError {
    #[error("{}", translate!("touch-error-unable-to-parse-date", "date" => .0))]
    InvalidDateFormat(String),

    /// The source time couldn't be converted to a [`jiff::Zoned`]
    #[error("{}", translate!("touch-error-invalid-filetime", "time" => .0))]
    InvalidFiletime(FileTime),

    /// The reference file's attributes could not be found or read
    #[error("{}", translate!("touch-error-reference-file-inaccessible", "path" => .0.quote(), "error" => to_uioerror(.1)))]
    ReferenceFileInaccessible(PathBuf, sysio::io::Error),

    /// An error getting a path to stdout on Windows
    #[error("{}", translate!("touch-error-windows-stdout-path-failed", "code" => .0))]
    WindowsStdoutPathError(String),

    /// A feature that is not available on the current platform
    #[error("{0}")]
    UnsupportedPlatformFeature(String),

    /// An error encountered on a specific file
    #[error("{error}")]
    TouchFileError {
        path: PathBuf,
        index: usize,
        error: Box<dyn UError>,
    },
}

fn to_uioerror(err: &sysio::io::Error) -> UIoError {
    let copy = if let Some(code) = err.raw_os_error() {
        sysio::io::Error::from_raw_os_error(code)
    } else {
        sysio::io::Error::from(err.kind())
    };
    UIoError::from(copy)
}

impl UError for TouchError {}
