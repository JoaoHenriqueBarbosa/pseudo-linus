//! A tabela de campos do top (`Fieldstab` e `Head_nlstab`): os nomes que `-o` e `-O` usam, as
//! larguras e o valor de ordenação de cada campo.

use super::{Task, Top};
use crate::ps::Item;
use crate::ps::display::Key;
use crate::ps::proc::{P_G_SZ, Pt};

pub const EU_PID: usize = 0;
pub const EU_UEN: usize = 3;
pub const EU_PRI: usize = 14;
pub const EU_NCE: usize = 15;
pub const EU_CPU: usize = 18;
pub const EU_TM2: usize = 20;
pub const EU_MEM: usize = 21;
pub const EU_VRT: usize = 22;
pub const EU_RES: usize = 24;
pub const EU_SHR: usize = 27;
pub const EU_STA: usize = 31;
pub const EU_CMD: usize = 32;
pub const EU_FV1: usize = 42;
pub const EU_FV2: usize = 43;
/// `EU_MAXPFLGS`: quantos campos existem.
pub const EU_MAXPFLGS: usize = 78;

/// Os campos visíveis do padrão do Debian (`ORIG_TOPDEFS`), na ordem de `DEF_FIELDS`.
pub const DEFAULT_FIELDS: [usize; 12] = [0, 3, 14, 15, 22, 24, 27, 31, 18, 21, 20, 32];

/// `Head_nlstab`: o cabeçalho de cada campo, que é também o nome aceito por `-o`.
pub const NAMES: [&str; EU_MAXPFLGS] = [
    "PID", "PPID", "UID", "USER", "RUID", "RUSER", "SUID", "SUSER", "GID", "GROUP", "PGRP", "TTY", "TPGID", "SID", "PR",
    "NI", "nTH", "P", "%CPU", "TIME", "TIME+", "%MEM", "VIRT", "SWAP", "RES", "CODE", "DATA", "SHR", "nMaj", "nMin",
    "nDRT", "S", "COMMAND", "WCHAN", "Flags", "CGROUPS", "SUPGIDS", "SUPGRPS", "TGID", "OOMa", "OOMs", "ENVIRON", "vMj",
    "vMn", "USED", "nsIPC", "nsMNT", "nsNET", "nsPID", "nsUSER", "nsUTS", "LXC", "RSan", "RSfd", "RSlk", "RSsh",
    "CGNAME", "NU", "LOGID", "EXE", "RSS", "PSS", "PSan", "PSfd", "PSsh", "USS", "ioR", "ioRop", "ioW", "ioWop", "AGID",
    "AGNI", "STARTED", "ELAPSED", "%CUU", "%CUC", "nsCGROUP", "nsTIME",
];

/// Largura fixa de um campo (`Fieldstab[].width`), -1 para os de largura variável. O PID e os
/// campos de grupo de processos valem 5 ou o número de dígitos do maior pid (ver `zap_fieldstab`).
pub fn width(f: usize, pid_width: i32) -> i32 {
    match f {
        0 | 1 | 10 | 12 | 13 | 38 => pid_width,
        2 | 4 | 6 | 8 | 18 | 21 | 39 | 58 | 67 | 69 | 70 => 5,
        3 | 5 | 7 | 9 | 11 | 34 | 51 => 8,
        14..=16 => 3,
        17 | 31 => 1,
        19 | 23 | 24 | 25 | 27 | 44 | 52..=55 | 60..=66 | 68 | 74 => 6,
        20 => 9,
        22 | 26 | 72 | 73 | 75 => 7,
        28..=30 | 40 | 71 => 4,
        32 | 35 | 36 | 37 | 41 | 56 | 59 => -1,
        33 | 45..=50 | 76 | 77 => 10,
        42 | 43 => 3,
        _ => 2,
    }
}

/// O campo está alinhado à direita (`A_right`)? Só os de texto ficam à esquerda.
pub fn align_right(f: usize) -> bool {
    !matches!(f, 3 | 5 | 7 | 9 | 11 | 32..=37 | 41 | 51 | 56 | 59)
}

impl Top {
    fn user_str(&mut self, uid: u32) -> Key {
        Key::Str(self.ps.user_name(uid))
    }

    fn group_str(&mut self, gid: u32) -> Key {
        Key::Str(self.ps.group_name(gid))
    }

    /// Valor de ordenação do campo `f` para a tarefa (o que `procps_pids_sort` compara). O item de
    /// COMMAND e o de TIME mudam com `-c` e `-S`, como em `window_show`.
    pub(crate) fn sort_key(&mut self, f: usize, t: &Task) -> Key {
        let p: &Pt = &t.p;
        let pages = |n: u64| Key::UInt(n << 2);
        match f {
            0 => Key::Int(i64::from(p.tid)),
            1 => Key::Int(i64::from(p.ppid)),
            2 => Key::UInt(u64::from(p.euid)),
            3 => self.user_str(p.euid),
            4 => Key::UInt(u64::from(p.ruid)),
            5 => self.user_str(p.ruid),
            6 => Key::UInt(u64::from(p.suid)),
            7 => self.user_str(p.suid),
            8 => Key::UInt(u64::from(p.egid)),
            9 => self.group_str(p.egid),
            10 => Key::Int(i64::from(p.pgrp)),
            11 => self.ps.sort_key(Item::TtyName, p),
            12 => Key::Int(i64::from(p.tpgid)),
            13 => Key::Int(i64::from(p.session)),
            14 => Key::Int(i64::from(p.priority)),
            15 => Key::Int(i64::from(p.nice)),
            16 => Key::Int(i64::from(p.nlwp)),
            17 => Key::Int(i64::from(p.processor)),
            18 => Key::UInt(u64::from(t.pcpu)),
            19 | 20 => {
                if self.w.show_ctimes {
                    Key::UInt(p.utime.wrapping_add(p.stime).wrapping_add(p.cutime).wrapping_add(p.cstime))
                } else {
                    Key::UInt(p.utime.wrapping_add(p.stime))
                }
            }
            21 | 24 => pages(p.statm_all()[1]),
            22 => pages(p.statm_all()[0]),
            23 => Key::UInt(p.vm_swap),
            25 => pages(p.statm_all()[3]),
            26 => pages(p.statm_all()[5]),
            27 => pages(p.statm_all()[2]),
            28 => Key::UInt(p.maj_flt),
            29 => Key::UInt(p.min_flt),
            30 => Key::None,
            31 => Key::Int(i64::from(p.state)),
            32 => {
                if self.w.show_cmdline {
                    Key::Str(p.cmdline().to_vec())
                } else {
                    Key::Str(p.cmd.clone())
                }
            }
            33 => Key::Str(p.wchan_name().to_vec()),
            34 => Key::UInt(p.flags),
            35 => Key::Str(p.cgroup().to_vec()),
            36 => Key::Str(p.supgid.clone().unwrap_or_else(|| b"-".to_vec())),
            37 => Key::Str(self.supgroups(p)),
            38 => Key::Int(i64::from(p.tgid)),
            39 => Key::Int(i64::from(p.oom().1)),
            40 => Key::Int(i64::from(p.oom().0)),
            41 => Key::Str(p.environ().to_vec()),
            42 => Key::Int(i64::from(t.maj_delta)),
            43 => Key::Int(i64::from(t.min_delta)),
            44 => Key::UInt(p.vm_swap.wrapping_add(p.vm_rss)),
            45 => Key::UInt(p.ns()[1]),
            46 => Key::UInt(p.ns()[2]),
            47 => Key::UInt(p.ns()[3]),
            48 => Key::UInt(p.ns()[4]),
            49 => Key::UInt(p.ns()[6]),
            50 => Key::UInt(p.ns()[7]),
            51 => Key::Str(p.lxcname().to_vec()),
            52 => Key::UInt(p.vm_rss_anon),
            53 => Key::UInt(p.vm_rss_file),
            54 => Key::UInt(p.vm_lock),
            55 => Key::UInt(p.vm_rss_shared),
            56 => Key::Str(p.cgname().to_vec()),
            57 => Key::Int(-1),
            58 => Key::Int(i64::from(p.luid())),
            59 => Key::Str(p.exe().to_vec()),
            60 => Key::UInt(p.smaps_all()[0]),
            61 => Key::UInt(p.smaps_all()[1]),
            62 => Key::UInt(p.smaps_all()[2]),
            63 => Key::UInt(p.smaps_all()[3]),
            64 => Key::UInt(p.smaps_all()[4]),
            65 => {
                let s = p.smaps_all();
                Key::UInt(s[7].wrapping_add(s[8]))
            }
            66 => Key::UInt(p.io()[4]),
            67 => Key::UInt(p.io()[2]),
            68 => Key::UInt(p.io()[5]),
            69 => Key::UInt(p.io()[3]),
            70 => Key::Int(i64::from(p.autogroup().0)),
            71 => Key::Int(i64::from(p.autogroup().1)),
            72 => Key::UInt(p.start_time),
            73 => Key::Real(self.elapsed_secs(p)),
            74 => Key::Real(self.utilization(p, false)),
            75 => Key::Real(self.utilization(p, true)),
            76 => Key::UInt(p.ns()[0]),
            _ => Key::UInt(p.ns()[5]),
        }
    }

    /// `TIME_ELAPSED`: segundos desde que o processo começou (0 se ainda não começou).
    fn elapsed_secs(&self, p: &Pt) -> f64 {
        let t = self.boot_tics as f64 - p.start_time as f64;
        if t > 0.0 { t / self.hertz as f64 } else { 0.0 }
    }

    /// `UTILIZATION`: % de CPU durante a vida do processo (conta em `float`, como a libproc2).
    fn utilization(&self, p: &Pt, with_children: bool) -> f64 {
        let t = self.boot_tics as f64 - p.start_time as f64;
        if t <= 0.0 {
            return 0.0;
        }
        let used = if with_children {
            p.utime.wrapping_add(p.stime).wrapping_add(p.cutime).wrapping_add(p.cstime)
        } else {
            p.utime.wrapping_add(p.stime)
        };
        f64::from((used as f32) * 100.0f32) / t
    }

    /// `supgrps_from_supgids`: os nomes dos grupos suplementares, separados por vírgula.
    fn supgroups(&mut self, p: &Pt) -> Vec<u8> {
        let ids = match &p.supgid {
            Some(g) if g.first() != Some(&b'-') => g.clone(),
            _ => return b"-".to_vec(),
        };
        let mut out: Vec<u8> = Vec::new();
        for part in ids.split(|b| *b == b',').filter(|s| !s.is_empty()) {
            let digits: Vec<u8> = part.iter().copied().take_while(u8::is_ascii_digit).collect();
            let Some(gid) = std::str::from_utf8(&digits).ok().and_then(|d| d.parse::<u64>().ok()) else { break };
            let mut piece: Vec<u8> = Vec::new();
            if !out.is_empty() {
                piece.push(b',');
            }
            piece.extend_from_slice(&self.ps.group_name(gid as u32));
            // snprintf(max = P_G_SZ + 2): no máximo P_G_SZ + 1 caracteres.
            piece.truncate(P_G_SZ + 1);
            out.extend_from_slice(&piece);
        }
        if out.is_empty() { b"-".to_vec() } else { out }
    }
}
