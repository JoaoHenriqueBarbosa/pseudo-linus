// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

//
// spell-checker:ignore IDLEN logind

//! Aims to provide platform-independent methods to obtain login records
//!
//! **ONLY** support linux, macos and freebsd for the time being
//!
//! # Examples:
//!
//! ```
//! use uucore::utmpx::Utmpx;
//! for ut in Utmpx::iter_all_records() {
//!     if ut.is_user_process() {
//!         println!("{}: {}", ut.host(), ut.user())
//!     }
//! }
//! ```
//!
//! Specifying the path to login record:
//!
//! ```
//! use uucore::utmpx::Utmpx;
//! for ut in Utmpx::iter_all_records_from("/some/where/else") {
//!     if ut.is_user_process() {
//!         println!("{}: {}", ut.host(), ut.user())
//!     }
//! }
//! ```

// Porte pseudo-linus: o original usava setutxent/getutxent/utmpxname da libc (estado global, unsafe,
// arquivo do host), o crate `time` com o fuso do host e o DNS do host no `canon_host`. Aqui os
// registros são lidos do arquivo utmp do pseudo-linus (formato binário da glibc no x86_64, 384 bytes
// por registro) pelo sysio, a hora sai no fuso do pseudo-processo (jiff) e não há DNS: o host fica
// como está, que é o que o GNU faz quando a consulta falha. Sem arquivo, não há registro nenhum
// (como o GNU sem /var/run/utmp).

use std::io::Result as IOResult;
use std::marker::PhantomData;
use std::path::Path;

#[cfg(feature = "feat_systemd_logind")]
use crate::features::systemd_logind;

pub use self::ut::*;

/// Texto de um campo `char[N]` (até o primeiro NUL).
fn field_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    bytes[..end].iter().map(|&b| b as char).collect()
}

#[cfg(target_os = "linux")]
mod ut {
    pub static DEFAULT_FILE: &str = "/var/run/utmp";

    pub const UT_HOSTSIZE: usize = 256;
    pub const UT_LINESIZE: usize = 32;
    pub const UT_NAMESIZE: usize = 32;
    pub const UT_IDSIZE: usize = 4;

    pub const EMPTY: i16 = 0;
    pub const RUN_LVL: i16 = 1;
    pub const BOOT_TIME: i16 = 2;
    pub const NEW_TIME: i16 = 3;
    pub const OLD_TIME: i16 = 4;
    pub const INIT_PROCESS: i16 = 5;
    pub const LOGIN_PROCESS: i16 = 6;
    pub const USER_PROCESS: i16 = 7;
    pub const DEAD_PROCESS: i16 = 8;
    pub const ACCOUNTING: i16 = 9;

    /// `sizeof(struct utmp)` da glibc no x86_64.
    pub const RECORD_SIZE: usize = 384;
}

/// `struct utmpx` da glibc no x86_64, decodificada.
#[derive(Clone, Debug, Default)]
#[allow(missing_docs)]
pub struct RawUtmpx {
    pub ut_type: i16,
    pub ut_pid: i32,
    pub ut_line: Vec<u8>,
    pub ut_id: Vec<u8>,
    pub ut_user: Vec<u8>,
    pub ut_host: Vec<u8>,
    pub e_termination: i16,
    pub e_exit: i16,
    pub ut_session: i32,
    pub tv_sec: i32,
    pub tv_usec: i32,
}

impl RawUtmpx {
    fn parse(r: &[u8]) -> Option<Self> {
        if r.len() < RECORD_SIZE {
            return None;
        }
        let i16_at = |o: usize| i16::from_le_bytes([r[o], r[o + 1]]);
        let i32_at = |o: usize| i32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]);
        Some(Self {
            ut_type: i16_at(0),
            ut_pid: i32_at(4),
            ut_line: r[8..40].to_vec(),
            ut_id: r[40..44].to_vec(),
            ut_user: r[44..76].to_vec(),
            ut_host: r[76..332].to_vec(),
            e_termination: i16_at(332),
            e_exit: i16_at(334),
            ut_session: i32_at(336),
            tv_sec: i32_at(340),
            tv_usec: i32_at(344),
        })
    }
}

#[cfg(target_vendor = "apple")]
mod ut {
    pub static DEFAULT_FILE: &str = "/var/run/utmpx";

    pub use libc::_UTX_HOSTSIZE as UT_HOSTSIZE;
    pub use libc::_UTX_IDSIZE as UT_IDSIZE;
    pub use libc::_UTX_LINESIZE as UT_LINESIZE;
    pub use libc::_UTX_USERSIZE as UT_NAMESIZE;

    pub use libc::ACCOUNTING;
    pub use libc::BOOT_TIME;
    pub use libc::DEAD_PROCESS;
    pub use libc::EMPTY;
    pub use libc::INIT_PROCESS;
    pub use libc::LOGIN_PROCESS;
    pub use libc::NEW_TIME;
    pub use libc::OLD_TIME;
    pub use libc::RUN_LVL;
    pub use libc::SHUTDOWN_TIME;
    pub use libc::SIGNATURE;
    pub use libc::USER_PROCESS;
}

#[cfg(target_os = "freebsd")]
mod ut {
    pub static DEFAULT_FILE: &str = "";

    pub const UT_LINESIZE: usize = 16;
    pub const UT_NAMESIZE: usize = 32;
    pub const UT_IDSIZE: usize = 8;
    pub const UT_HOSTSIZE: usize = 128;

    pub use libc::BOOT_TIME;
    pub use libc::DEAD_PROCESS;
    pub use libc::EMPTY;
    pub use libc::INIT_PROCESS;
    pub use libc::LOGIN_PROCESS;
    pub use libc::NEW_TIME;
    pub use libc::OLD_TIME;
    pub use libc::SHUTDOWN_TIME;
    pub use libc::USER_PROCESS;
}

#[cfg(target_os = "netbsd")]
mod ut {
    pub static DEFAULT_FILE: &str = "/var/run/utmpx";

    pub const SHUTDOWN_TIME: usize = 11;

    pub use libc::_UTX_HOSTSIZE as UT_HOSTSIZE;
    pub use libc::_UTX_IDSIZE as UT_IDSIZE;
    pub use libc::_UTX_LINESIZE as UT_LINESIZE;
    pub use libc::_UTX_USERSIZE as UT_NAMESIZE;

    pub use libc::ACCOUNTING;
    pub const BOOT_TIME: i16 = libc::BOOT_TIME as i16;
    pub const DEAD_PROCESS: i16 = libc::DEAD_PROCESS as i16;
    pub const EMPTY: i16 = libc::EMPTY as i16;
    pub const INIT_PROCESS: i16 = libc::INIT_PROCESS as i16;
    pub const LOGIN_PROCESS: i16 = libc::LOGIN_PROCESS as i16;
    pub const NEW_TIME: i16 = libc::NEW_TIME as i16;
    pub const OLD_TIME: i16 = libc::OLD_TIME as i16;
    pub const RUN_LVL: i16 = libc::RUN_LVL as i16;
    pub const SIGNATURE: i16 = libc::SIGNATURE as i16;
    pub const USER_PROCESS: i16 = libc::USER_PROCESS as i16;
}

#[cfg(target_os = "cygwin")]
mod ut {
    pub static DEFAULT_FILE: &str = "";

    pub use libc::UT_HOSTSIZE;
    pub use libc::UT_IDLEN;
    pub use libc::UT_LINESIZE;
    pub use libc::UT_NAMESIZE;

    pub use libc::BOOT_TIME;
    pub use libc::DEAD_PROCESS;
    pub use libc::INIT_PROCESS;
    pub use libc::LOGIN_PROCESS;
    pub use libc::NEW_TIME;
    pub use libc::OLD_TIME;
    pub use libc::RUN_LVL;
    pub use libc::USER_PROCESS;
}

/// A login record
pub struct Utmpx {
    inner: RawUtmpx,
}

impl Utmpx {
    fn ut_type(&self) -> i16 {
        self.inner.ut_type
    }
    fn ut_user(&self) -> String {
        field_string(&self.inner.ut_user)
    }
}

impl Utmpx {
    /// A.K.A. ut.ut_type
    pub fn record_type(&self) -> i16 {
        self.ut_type()
    }
    /// A.K.A. ut.ut_pid
    pub fn pid(&self) -> i32 {
        self.inner.ut_pid
    }
    /// A.K.A. ut.ut_id
    pub fn terminal_suffix(&self) -> String {
        field_string(&self.inner.ut_id)
    }
    ///  A.K.A. ut.ut_user / ut.ut_name (NetBSD)
    pub fn user(&self) -> String {
        self.ut_user()
    }
    /// A.K.A. ut.ut_host
    pub fn host(&self) -> String {
        field_string(&self.inner.ut_host)
    }
    /// A.K.A. ut.ut_line
    pub fn tty_device(&self) -> String {
        field_string(&self.inner.ut_line)
    }
    /// A.K.A. ut.ut_tv, no fuso do pseudo-processo
    pub fn login_time(&self) -> jiff::Zoned {
        let ts = jiff::Timestamp::new(i64::from(self.inner.tv_sec), self.inner.tv_usec.clamp(0, 999_999) * 1000)
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH);
        ts.to_zoned(crate::time::process_time_zone())
    }
    /// A.K.A. ut.ut_exit
    ///
    /// Return (e_termination, e_exit)
    #[cfg(target_os = "linux")]
    pub fn exit_status(&self) -> (i16, i16) {
        (self.inner.e_termination, self.inner.e_exit)
    }
    /// A.K.A. ut.ut_exit
    ///
    /// Return (0, 0) on Non-Linux platform
    #[cfg(not(target_os = "linux"))]
    pub fn exit_status(&self) -> (i16, i16) {
        (0, 0)
    }
    /// Consumes the `Utmpx`, returning the underlying C struct utmpx
    pub fn into_inner(self) -> RawUtmpx {
        self.inner
    }
    /// check if the record is a user process
    pub fn is_user_process(&self) -> bool {
        !self.user().is_empty() && self.record_type() == USER_PROCESS
    }

    /// Canonicalize host name using DNS
    ///
    /// Porte pseudo-linus: o sandbox não resolve nomes; o GNU, quando a consulta falha, devolve o
    /// nome do host sem a parte do display, e é isso que sai aqui.
    pub fn canon_host(&self) -> IOResult<String> {
        let host = self.host();
        let (hostname, _display) = host.split_once(':').unwrap_or((&host, ""));
        if !hostname.is_empty() {
            return Ok(hostname.to_string());
        }
        Ok(host)
    }

    /// Iterate through all the utmp records.
    ///
    /// This will use the default location, or the path [`Utmpx::iter_all_records_from`]
    /// was most recently called with.
    ///
    /// On systems with systemd-logind feature enabled at compile time,
    /// this will use systemd-logind instead of traditional utmp files.
    ///
    /// Only one instance of [`UtmpxIter`] may be active at a time. This
    /// function will block as long as one is still active. Beware!
    pub fn iter_all_records() -> UtmpxIter {
        #[cfg(feature = "feat_systemd_logind")]
        {
            // Use systemd-logind instead of traditional utmp when feature is enabled
            UtmpxIter::new_systemd()
        }

        #[cfg(not(feature = "feat_systemd_logind"))]
        {
            let path = current_file().lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
            UtmpxIter::new(&path)
        }
    }

    /// Iterate through all the utmp records from a specific file.
    ///
    /// No failure is reported or detected.
    ///
    /// This function affects subsequent calls to [`Utmpx::iter_all_records`].
    ///
    /// On systems with systemd-logind feature enabled at compile time,
    /// if the path matches the default utmp file, this will use systemd-logind
    /// instead of traditional utmp files.
    ///
    /// The same caveats as for [`Utmpx::iter_all_records`] apply.
    pub fn iter_all_records_from<P: AsRef<Path>>(path: P) -> UtmpxIter {
        #[cfg(feature = "feat_systemd_logind")]
        {
            // Use systemd-logind for default utmp file when feature is enabled
            if path.as_ref() == Path::new(DEFAULT_FILE) {
                return UtmpxIter::new_systemd();
            }
        }

        // GNU who on Debian seems to output nothing if an invalid filename
        // is specified, no warning or anything (utmpxname não falha).
        *current_file().lock().unwrap_or_else(std::sync::PoisonError::into_inner) = path.as_ref().to_path_buf();
        UtmpxIter::new(path.as_ref())
    }
}

/// O arquivo que o `utmpxname` escolheu, por pseudo-processo.
fn current_file() -> std::sync::Arc<std::sync::Mutex<std::path::PathBuf>> {
    sysio::proc::proc_local(|| std::sync::Mutex::new(std::path::PathBuf::from(DEFAULT_FILE)))
}

// Porte pseudo-linus: sem a trava global do getutxent (não há estado da libc); o iterador lê o
// arquivo inteiro de uma vez e devolve os registros em ordem.

/// Iterator of login records
pub struct UtmpxIter {
    records: std::vec::IntoIter<RawUtmpx>,
    /// Ensure UtmpxIter is !Send, como no original.
    phantom: PhantomData<std::rc::Rc<()>>,
    #[cfg(feature = "feat_systemd_logind")]
    systemd_iter: Option<systemd_logind::SystemdUtmpxIter>,
}

impl UtmpxIter {
    fn new(path: &Path) -> Self {
        let data = sysio::fs::read(path).unwrap_or_default();
        let records: Vec<RawUtmpx> = data.chunks(RECORD_SIZE).filter_map(RawUtmpx::parse).collect();
        Self {
            records: records.into_iter(),
            phantom: PhantomData,
            #[cfg(feature = "feat_systemd_logind")]
            systemd_iter: None,
        }
    }

    #[cfg(feature = "feat_systemd_logind")]
    fn new_systemd() -> Self {
        let systemd_iter = match systemd_logind::SystemdUtmpxIter::new() {
            Ok(iter) => iter,
            Err(_) => {
                // Like GNU coreutils: graceful degradation, not fallback to traditional utmp
                // Return empty iterator rather than falling back  (GNU coreutils also returns 0 when /var/run/utmp is not present, so we don't need to propagate the error here)
                systemd_logind::SystemdUtmpxIter::empty()
            }
        };
        Self {
            records: Vec::new().into_iter(),
            phantom: PhantomData,
            systemd_iter: Some(systemd_iter),
        }
    }
}

/// Wrapper type that can hold either traditional utmpx records or systemd records
pub enum UtmpxRecord {
    Traditional(Box<Utmpx>),
    #[cfg(feature = "feat_systemd_logind")]
    Systemd(systemd_logind::SystemdUtmpxCompat),
}

impl UtmpxRecord {
    /// A.K.A. ut.ut_type
    pub fn record_type(&self) -> i16 {
        match self {
            Self::Traditional(utmpx) => utmpx.record_type(),
            #[cfg(feature = "feat_systemd_logind")]
            Self::Systemd(systemd) => systemd.record_type(),
        }
    }

    /// A.K.A. ut.ut_pid
    pub fn pid(&self) -> i32 {
        match self {
            Self::Traditional(utmpx) => utmpx.pid(),
            #[cfg(feature = "feat_systemd_logind")]
            Self::Systemd(systemd) => systemd.pid(),
        }
    }

    /// A.K.A. ut.ut_id
    pub fn terminal_suffix(&self) -> String {
        match self {
            Self::Traditional(utmpx) => utmpx.terminal_suffix(),
            #[cfg(feature = "feat_systemd_logind")]
            Self::Systemd(systemd) => systemd.terminal_suffix(),
        }
    }

    /// A.K.A. ut.ut_user
    pub fn user(&self) -> String {
        match self {
            Self::Traditional(utmpx) => utmpx.user(),
            #[cfg(feature = "feat_systemd_logind")]
            Self::Systemd(systemd) => systemd.user(),
        }
    }

    /// A.K.A. ut.ut_host
    pub fn host(&self) -> String {
        match self {
            Self::Traditional(utmpx) => utmpx.host(),
            #[cfg(feature = "feat_systemd_logind")]
            Self::Systemd(systemd) => systemd.host(),
        }
    }

    /// A.K.A. ut.ut_line
    pub fn tty_device(&self) -> String {
        match self {
            Self::Traditional(utmpx) => utmpx.tty_device(),
            #[cfg(feature = "feat_systemd_logind")]
            Self::Systemd(systemd) => systemd.tty_device(),
        }
    }

    /// A.K.A. ut.ut_tv (Porte pseudo-linus: `jiff::Zoned` no fuso do pseudo-processo)
    pub fn login_time(&self) -> jiff::Zoned {
        match self {
            Self::Traditional(utmpx) => utmpx.login_time(),
            #[cfg(feature = "feat_systemd_logind")]
            Self::Systemd(systemd) => systemd.login_time(),
        }
    }

    /// A.K.A. ut.ut_exit
    ///
    /// Return (e_termination, e_exit)
    pub fn exit_status(&self) -> (i16, i16) {
        match self {
            Self::Traditional(utmpx) => utmpx.exit_status(),
            #[cfg(feature = "feat_systemd_logind")]
            Self::Systemd(systemd) => systemd.exit_status(),
        }
    }

    /// check if the record is a user process
    pub fn is_user_process(&self) -> bool {
        match self {
            Self::Traditional(utmpx) => utmpx.is_user_process(),
            #[cfg(feature = "feat_systemd_logind")]
            Self::Systemd(systemd) => systemd.is_user_process(),
        }
    }

    /// Canonicalize host name using DNS
    pub fn canon_host(&self) -> IOResult<String> {
        match self {
            Self::Traditional(utmpx) => utmpx.canon_host(),
            #[cfg(feature = "feat_systemd_logind")]
            Self::Systemd(systemd) => Ok(systemd.canon_host()),
        }
    }
}

impl Iterator for UtmpxIter {
    type Item = UtmpxRecord;
    fn next(&mut self) -> Option<Self::Item> {
        #[cfg(feature = "feat_systemd_logind")]
        {
            if let Some(ref mut systemd_iter) = self.systemd_iter {
                // We have a systemd iterator - use it exclusively (never fall back to traditional utmp)
                return systemd_iter.next().map(UtmpxRecord::Systemd);
            }
        }

        // Traditional utmp path (Porte pseudo-linus: registros já lidos do arquivo)
        self.records.next().map(|inner| UtmpxRecord::Traditional(Box::new(Utmpx { inner })))
    }
}
