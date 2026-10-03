// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (vars) Passwd cstr fnam gecos ngroups egid

//! Get password/group file entry
//!
//! # Examples:
//!
//! ```
//! use uucore::entries::{self, Locate};
//!
//! let root_group = if cfg!(any(target_os = "linux", target_os = "android")) {
//!     "root"
//! } else {
//!     "wheel"
//! };
//!
//! assert_eq!("root", entries::uid2usr(0).unwrap());
//! assert_eq!(0, entries::usr2uid("root").unwrap());
//! assert!(entries::gid2grp(0).is_ok());
//! assert!(entries::grp2gid(root_group).is_ok());
//!
//! assert!(entries::Passwd::locate(0).is_ok());
//! assert!(entries::Passwd::locate("0").is_ok());
//! assert!(entries::Passwd::locate("root").is_ok());
//!
//! assert!(entries::Group::locate(0).is_ok());
//! assert!(entries::Group::locate("0").is_ok());
//! assert!(entries::Group::locate(root_group).is_ok());
//! ```

// Porte pseudo-linus: o original chamava getpwuid/getpwnam/getgrgid/getgrnam/getgrouplist da libc
// (unsafe, NSS do host, com uma trava global porque as funções não são reentrantes). Aqui as
// tabelas são o /etc/passwd e o /etc/group do VFS do pseudo-processo, lidos a cada consulta.

use std::io::Error as IOError;
use std::io::ErrorKind;
use std::io::Result as IOResult;

#[allow(non_camel_case_types)]
pub type uid_t = u32;
#[allow(non_camel_case_types)]
pub type gid_t = u32;

fn table(path: &str) -> Vec<Vec<String>> {
    sysio::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split(':').map(str::to_string).collect())
        .collect()
}

fn field(row: &[String], i: usize) -> Option<String> {
    row.get(i).cloned()
}

/// The list of group IDs returned from GNU's `groups` and GNU's `id --groups`
/// starts with the effective group ID (egid).
/// This is a wrapper for `get_groups()` to mimic this behavior.
///
/// If `arg_id` is `None` (default), `get_groups_gnu` moves the effective
/// group id (egid) to the first entry in the returned Vector.
/// If `arg_id` is `Some(x)`, `get_groups_gnu` moves the id with value `x`
/// to the first entry in the returned Vector. This might be necessary
/// for `id --groups --real` if `gid` and `egid` are not equal.
///
/// From: `<https://www.man7.org/linux/man-pages/man3/getgroups.3p.html>`
/// > As implied by the definition of supplementary groups, the
/// > effective group ID may appear in the array returned by
/// > getgroups() or it may be returned only by getegid().  Duplication
/// > may exist, but the application needs to call getegid() to be sure
/// > of getting all of the information. Various implementation
/// > variations and administrative sequences cause the set of groups
/// > appearing in the result of getgroups() to vary in order and as to
/// > whether the effective group ID is included, even when the set of
/// > groups is the same (in the mathematical sense of ``set''). (The
/// > history of a process and its parents could affect the details of
/// > the result.)
#[cfg(all(unix, not(target_os = "redox"), feature = "process"))]
pub fn get_groups_gnu(arg_id: Option<u32>) -> IOResult<Vec<rustix::process::RawGid>> {
    let groups = rustix::process::getgroups()
        .map(|g| g.into_iter().map(rustix::fs::Gid::as_raw).collect())?;
    let egid = arg_id.unwrap_or_else(|| rustix::process::getegid().as_raw());
    Ok(sort_groups(groups, egid))
}

#[cfg(all(unix, not(target_os = "redox"), feature = "process"))]
fn sort_groups(mut groups: Vec<gid_t>, egid: gid_t) -> Vec<gid_t> {
    if let Some(index) = groups.iter().position(|&x| x == egid) {
        groups[..=index].rotate_right(1);
    } else {
        groups.insert(0, egid);
    }
    groups
}

#[derive(Clone, Debug)]
pub struct Passwd {
    /// AKA passwd.pw_name
    pub name: String,
    /// AKA passwd.pw_uid
    pub uid: uid_t,
    /// AKA passwd.pw_gid
    pub gid: gid_t,
    /// AKA passwd.pw_gecos
    pub user_info: Option<String>,
    /// AKA passwd.pw_shell
    pub user_shell: Option<String>,
    /// AKA passwd.pw_dir
    pub user_dir: Option<String>,
    /// AKA passwd.pw_passwd
    #[expect(clippy::struct_field_names)]
    pub user_passwd: Option<String>,
    /// AKA passwd.pw_class
    #[cfg(any(target_vendor = "apple", target_os = "freebsd"))]
    pub user_access_class: Option<String>,
    /// AKA passwd.pw_change
    #[cfg(any(target_vendor = "apple", target_os = "freebsd"))]
    #[expect(clippy::struct_field_names)]
    pub passwd_change_time: time_t,
    /// AKA passwd.pw_expire
    #[cfg(any(target_vendor = "apple", target_os = "freebsd"))]
    pub expiration: time_t,
}

impl Passwd {
    /// Uma linha `nome:senha:uid:gid:gecos:home:shell` do /etc/passwd do VFS.
    fn from_row(row: &[String]) -> Option<Self> {
        Some(Self {
            name: field(row, 0)?,
            user_passwd: field(row, 1),
            uid: field(row, 2)?.parse().ok()?,
            gid: field(row, 3)?.parse().ok()?,
            user_info: field(row, 4),
            user_dir: field(row, 5),
            user_shell: field(row, 6),
        })
    }

    /// O que `getgrouplist` daria: o grupo primário mais os grupos em que o nome aparece na
    /// lista de membros do /etc/group.
    pub fn belongs_to(&self) -> Vec<gid_t> {
        let mut groups = vec![self.gid];
        for row in table("/etc/group") {
            let Some(gid) = field(&row, 2).and_then(|g| g.parse::<gid_t>().ok()) else {
                continue;
            };
            let members = field(&row, 3).unwrap_or_default();
            if members.split(',').any(|m| m == self.name) && !groups.contains(&gid) {
                groups.push(gid);
            }
        }
        groups
    }
}

#[derive(Clone, Debug)]
pub struct Group {
    /// AKA group.gr_name
    pub name: String,
    /// AKA group.gr_gid
    pub gid: gid_t,
}

impl Group {
    /// Uma linha `nome:senha:gid:membros` do /etc/group do VFS.
    fn from_row(row: &[String]) -> Option<Self> {
        Some(Self {
            name: field(row, 0)?,
            gid: field(row, 2)?.parse().ok()?,
        })
    }
}

/// Fetch desired entry.
pub trait Locate<K> {
    fn locate(key: K) -> IOResult<Self>
    where
        Self: ::std::marker::Sized;
}

// Porte pseudo-linus: sem a trava global (a leitura do arquivo do VFS é reentrante); a busca por
// nome e por id segue a mesma ordem do original (nome primeiro, depois o texto como número).
macro_rules! f {
    ($file:expr, $idcol:expr, $t:ident, $st:ident) => {
        impl Locate<$t> for $st {
            fn locate(k: $t) -> IOResult<Self> {
                table($file)
                    .iter()
                    .find(|row| field(row, $idcol).and_then(|v| v.parse::<$t>().ok()) == Some(k))
                    .and_then(|row| $st::from_row(row))
                    .ok_or_else(|| IOError::new(ErrorKind::NotFound, format!("No such id: {k}")))
            }
        }

        impl<'a> Locate<&'a str> for $st {
            fn locate(k: &'a str) -> IOResult<Self> {
                let rows = table($file);
                if let Some(found) = rows
                    .iter()
                    .find(|row| field(row, 0).as_deref() == Some(k))
                    .and_then(|row| $st::from_row(row))
                {
                    return Ok(found);
                }
                if let Ok(id) = k.parse::<$t>() {
                    Self::locate(id)
                } else {
                    Err(IOError::new(ErrorKind::NotFound, format!("Not found: {k}")))
                }
            }
        }
    };
}

f!("/etc/passwd", 2, uid_t, Passwd);
f!("/etc/group", 2, gid_t, Group);

#[inline]
pub fn uid2usr(id: uid_t) -> IOResult<String> {
    Passwd::locate(id).map(|p| p.name)
}

#[inline]
pub fn gid2grp(id: gid_t) -> IOResult<String> {
    Group::locate(id).map(|p| p.name)
}

#[inline]
pub fn usr2uid(name: &str) -> IOResult<uid_t> {
    Passwd::locate(name).map(|p| p.uid)
}

#[inline]
pub fn usr2gid(name: &str) -> IOResult<gid_t> {
    Passwd::locate(name).map(|p| p.gid)
}

#[inline]
pub fn grp2gid(name: &str) -> IOResult<gid_t> {
    Group::locate(name).map(|p| p.gid)
}

#[cfg(test)]
mod test {
    #[cfg(all(unix, not(target_os = "redox"), feature = "process"))]
    use super::*;

    #[test]
    #[cfg(all(unix, not(target_os = "redox"), feature = "process"))]
    fn test_sort_groups() {
        assert_eq!(sort_groups(vec![1, 2, 3], 4), vec![4, 1, 2, 3]);
        assert_eq!(sort_groups(vec![1, 2, 3], 3), vec![3, 1, 2]);
        assert_eq!(sort_groups(vec![1, 2, 3], 2), vec![2, 1, 3]);
        assert_eq!(sort_groups(vec![1, 2, 3], 1), vec![1, 2, 3]);
        assert_eq!(sort_groups(vec![1, 2, 3], 0), vec![0, 1, 2, 3]);
    }

    #[test]
    #[cfg(all(unix, not(target_os = "redox"), feature = "process"))]
    fn test_entries_get_groups_gnu() {
        if let Ok(mut groups) = rustix::process::getgroups().map(|g| {
            g.into_iter()
                .map(rustix::fs::Gid::as_raw)
                .collect::<Vec<_>>()
        }) && let Some(last) = groups.pop()
        {
            groups.insert(0, last);
            assert_eq!(get_groups_gnu(Some(last)).unwrap(), groups);
        }
    }
}
