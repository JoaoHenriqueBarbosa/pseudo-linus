//! Linha de listagem (`tar -tv`, `-cvv`, `-xvv`) no formato do GNU tar 1.35:
//! `modo dono/grupo tamanho data nome[ -> alvo | link to alvo]`, com a largura do campo dono/tamanho
//! crescendo conforme aparecem valores maiores (começa em 19 colunas).

use jiff::tz::TimeZone;
use ul_common::fsutil::mode_string;

use super::member::{Kind, Member, Time};
use super::quote::{self, Quoting};

/// Estado da listagem (a largura cresce ao longo do arquivo).
pub struct Lister {
    ugswidth: usize,
    pub full_time: bool,
    pub utc: bool,
    pub numeric: bool,
    tz: TimeZone,
}

impl Lister {
    pub fn new(full_time: bool, utc: bool, numeric: bool) -> Lister {
        let tz = if utc { TimeZone::UTC } else { crate::tz::local() };
        Lister { ugswidth: 19, full_time, utc, numeric, tz }
    }

    /// Caractere de tipo do começo da linha.
    pub fn type_char(m: &Member) -> char {
        use super::header::kind;
        match m.typeflag {
            kind::GNU_VOLHDR => 'V',
            kind::GNU_MULTIVOL => 'M',
            kind::LNK => 'h',
            kind::REG | kind::AREG | kind::GNU_SPARSE => {
                if m.name.ends_with(b"/") {
                    'd'
                } else {
                    '-'
                }
            }
            kind::GNU_DUMPDIR | kind::DIR => 'd',
            kind::SYM => 'l',
            kind::BLK => 'b',
            kind::CHR => 'c',
            kind::FIFO => 'p',
            kind::CONT => 'C',
            _ => '?',
        }
    }

    /// Data no formato do tar (`%Y-%m-%d %H:%M`, ou com segundos e fração no `--full-time`).
    pub fn time(&self, t: Time) -> String {
        if self.full_time {
            let base = crate::tz::format(t.sec, 0, &self.tz, "%Y-%m-%d %H:%M:%S");
            if t.nsec != 0 {
                let mut frac = format!("{:09}", t.nsec);
                while frac.ends_with('0') {
                    frac.pop();
                }
                format!("{base}.{frac}")
            } else {
                base
            }
        } else {
            crate::tz::format(t.sec, 0, &self.tz, "%Y-%m-%d %H:%M")
        }
    }

    /// Linha completa (sem o `\n`).
    pub fn line(&mut self, m: &Member, display_name: &[u8], q: &Quoting) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(mode_string(Self::type_char(m), m.mode).as_bytes());
        out.push(b' ');
        let user: Vec<u8> = if !m.uname.is_empty() && !self.numeric {
            m.uname.clone()
        } else {
            m.uid.to_string().into_bytes()
        };
        let group: Vec<u8> = if !m.gname.is_empty() && !self.numeric {
            m.gname.clone()
        } else {
            m.gid.to_string().into_bytes()
        };
        let size = match m.kind() {
            Kind::CharDev | Kind::BlockDev => format!("{},{}", m.devmajor, m.devminor),
            _ => {
                let s = if m.sparse.is_some() { m.real_size } else { m.size };
                s.to_string()
            }
        };
        let pad = user.len() + 1 + group.len() + 1 + size.len();
        if pad > self.ugswidth {
            self.ugswidth = pad;
        }
        out.extend_from_slice(&user);
        out.push(b'/');
        out.extend_from_slice(&group);
        out.push(b' ');
        out.resize(out.len() + (self.ugswidth - pad), b' ');
        out.extend_from_slice(size.as_bytes());
        out.push(b' ');
        out.extend_from_slice(self.time(m.mtime).as_bytes());
        out.push(b' ');
        out.extend_from_slice(&quote::quote_with(display_name, q, false));
        match m.kind() {
            Kind::Symlink => {
                out.extend_from_slice(b" -> ");
                out.extend_from_slice(&quote::quote_with(&m.linkname, q, false));
            }
            Kind::HardLink => {
                out.extend_from_slice(b" link to ");
                out.extend_from_slice(&quote::quote_with(&m.linkname, q, false));
            }
            Kind::Volume => out.extend_from_slice(b"--Volume Header--"),
            Kind::Multivolume => {
                out.extend_from_slice(format!("--Continued at byte {}--", m.size).as_bytes());
            }
            Kind::Other(c) if !matches!(c, b'0'..=b'7') && c != 0 => {
                out.extend_from_slice(format!(" unknown file type '{}'", c as char).as_bytes());
            }
            _ => {}
        }
        out
    }
}
