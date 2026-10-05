//! Colunas do ps (output.c): as funções `pr_*` que formatam cada coluna e o `show_one_proc`, que
//! alinha, limita a largura e escreve a linha (ou o cabeçalho).
//!
//! Cada função recebe o processo e o buffer da coluna, e devolve a quantidade de células de tela
//! (que pode diferir dos bytes escritos em UTF-8). A largura disponível vem de `Ps::max_rightward`.

use sysabi::sys;
use ul_misc::util::io;

use super::proc::{Pt, dev_to_tty};
use super::util::{escape_str_out, strverscmp};
use super::*;

/// `COLWID` do original: `snprintf` corta em 239 caracteres.
const COLWID: usize = 240;
/// Espaço de preenchimento antes do dado (`SPACE_AMOUNT`).
const SPACE_AMOUNT: i32 = 144;
const SIGNAL_NAME_WIDTH: i32 = 27;

/// Escreve `s` como o `snprintf(outbuf, COLWID, "%s", s)`: corta em 239 bytes, devolve o tamanho
/// que teria.
fn snp(out: &mut Vec<u8>, s: &str) -> usize {
    snp_bytes(out, s.as_bytes())
}

fn snp_bytes(out: &mut Vec<u8>, s: &[u8]) -> usize {
    out.extend_from_slice(&s[..s.len().min(COLWID - 1)]);
    s.len()
}

/// Erro fatal no meio da saída (o `xerrx` do original): descarrega o que já saiu e termina.
fn fatal(ps: &Ps, msg: &str) -> ! {
    let _ = io::flush_stdout();
    io::eprint(format!("{}: {msg}\n", ps.myname));
    sys::exit(1)
}

// ---------------------------------------------------------------------------------------------
// Dados do sistema e do processo.

impl Ps {
    /// Hora do boot (`btime` de /proc/stat).
    fn boot_time(&mut self) -> i64 {
        if let Some(b) = self.boot_time_cache {
            return b;
        }
        let mut btime = None;
        if let Some(d) = proc::read_path("/proc/stat") {
            let text = String::from_utf8_lossy(&d).into_owned();
            for l in text.lines() {
                if let Some(rest) = l.strip_prefix("btime ") {
                    btime = rest.split_whitespace().next().and_then(|v| v.parse::<i64>().ok());
                    break;
                }
            }
        }
        match btime {
            Some(b) => {
                self.boot_time_cache = Some(b);
                b
            }
            None => fatal(self, "Unable to get system boot time"),
        }
    }

    /// `MemTotal` do /proc/meminfo, em kB.
    fn memory_total(&mut self) -> u64 {
        if let Some(m) = self.mem_total_cache {
            return m;
        }
        let mut total = None;
        if let Some(d) = proc::read_path("/proc/meminfo") {
            let text = String::from_utf8_lossy(&d).into_owned();
            for l in text.lines() {
                if let Some(rest) = l.strip_prefix("MemTotal:") {
                    total = rest.split_whitespace().next().and_then(|v| v.parse::<u64>().ok());
                    break;
                }
            }
        }
        match total {
            Some(t) => {
                self.mem_total_cache = Some(t);
                t
            }
            None => fatal(self, "Unable to get total memory"),
        }
    }

    /// `TIME_ELAPSED`: segundos desde que o processo começou (0 se ainda não começou).
    pub(super) fn time_elapsed(&self, p: &Pt) -> f64 {
        let t = self.boot_tics as f64 - p.start_time as f64;
        if t > 0.0 { t / self.hertz as f64 } else { 0.0 }
    }

    /// `UTILIZATION`: % de CPU durante a vida do processo (conta em `float`, como a libproc2).
    pub(super) fn utilization(&self, p: &Pt, with_children: bool) -> f64 {
        let t = self.boot_tics as f64 - p.start_time as f64;
        if t > 0.0 {
            let used = if with_children {
                p.utime.wrapping_add(p.stime).wrapping_add(p.cutime).wrapping_add(p.cstime)
            } else {
                p.utime.wrapping_add(p.stime)
            };
            f64::from((used as f32) * 100.0f32) / t
        } else {
            0.0
        }
    }

    /// Tempo de CPU em segundos (`TIME_ALL`).
    fn time_all(&self, p: &Pt) -> f64 {
        (p.utime as f64 + p.stime as f64) / self.hertz as f64
    }

    fn tics_all(&self, p: &Pt) -> u64 {
        if self.include_dead_children {
            p.utime.wrapping_add(p.stime).wrapping_add(p.cutime).wrapping_add(p.cstime)
        } else {
            p.utime.wrapping_add(p.stime)
        }
    }

    fn local_tz(&mut self) -> jiff::tz::TimeZone {
        if self.tz.is_none() {
            self.tz = Some(ul_misc::util::time::local_tz());
        }
        self.tz.clone().unwrap_or(jiff::tz::TimeZone::UTC)
    }

    /// Instante de início do processo, em segundos desde a época.
    fn start_epoch(&mut self, p: &Pt) -> i64 {
        self.boot_time() + (p.start_time / self.hertz) as i64
    }

    /// Prefixo de floresta (`forest_helper`): devolve os bytes escritos.
    fn forest_helper(&self, out: &mut Vec<u8>) -> i32 {
        let before = out.len();
        let mut rightward = if self.max_rightward < OUTBUF_SIZE { self.max_rightward } else { OUTBUF_SIZE - 1 };
        if self.forest_prefix.is_empty() {
            return 0;
        }
        let unixy = self.forest_type == b'u';
        for ch in &self.forest_prefix {
            if unixy {
                if rightward < 2 {
                    break;
                }
                out.extend_from_slice(b"  ");
                rightward -= 2;
            } else {
                if rightward < 4 {
                    break;
                }
                out.extend_from_slice(match ch {
                    b' ' => b"    ",
                    b'L' | b'+' => b" \\_ ",
                    _ => b" |  ",
                });
                rightward -= 4;
            }
        }
        (out.len() - before) as i32
    }
}

fn room(out: &[u8]) -> i32 {
    OUTBUF_SIZE - out.len() as i32
}

// ---------------------------------------------------------------------------------------------
// Comando.

fn env_tail(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>, rightward: &mut i32) {
    if ps.bsd_e_option && *rightward > 1 {
        let e = p.environ();
        if e != b"-" {
            out.push(b' ');
            *rightward -= 1;
            let r = room(out);
            escape_str_out(out, e, r, rightward);
        }
    }
}

pub fn pr_args(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let mut rightward = ps.max_rightward;
    let fh = ps.forest_helper(out);
    rightward -= fh;
    let r = room(out);
    if !ps.bsd_c_option {
        escape_str_out(out, p.cmdline(), r, &mut rightward);
    } else {
        escape_str_out(out, &p.cmd, r, &mut rightward);
    }
    env_tail(ps, p, out, &mut rightward);
    (ps.max_rightward - rightward) as usize
}

pub fn pr_comm(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let mut rightward = ps.max_rightward;
    let fh = ps.forest_helper(out);
    rightward -= fh;
    let r = room(out);
    if ps.unix_f_option {
        escape_str_out(out, p.cmdline(), r, &mut rightward);
    } else {
        escape_str_out(out, &p.cmd, r, &mut rightward);
    }
    env_tail(ps, p, out, &mut rightward);
    (ps.max_rightward - rightward) as usize
}

pub fn pr_cgname(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let mut rightward = ps.max_rightward;
    escape_str_out(out, p.cgname(), OUTBUF_SIZE, &mut rightward);
    (ps.max_rightward - rightward) as usize
}

pub fn pr_cgroup(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let mut rightward = ps.max_rightward;
    escape_str_out(out, p.cgroup(), OUTBUF_SIZE, &mut rightward);
    (ps.max_rightward - rightward) as usize
}

pub fn pr_fname(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let mut rightward = ps.max_rightward;
    let fh = ps.forest_helper(out);
    rightward -= fh;
    if rightward > 8 {
        rightward = 8;
    }
    let r = room(out);
    escape_str_out(out, &p.cmd, r, &mut rightward);
    (ps.max_rightward - rightward) as usize
}

pub fn pr_exe(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let mut rightward = ps.max_rightward;
    escape_str_out(out, p.exe(), OUTBUF_SIZE, &mut rightward);
    (ps.max_rightward - rightward) as usize
}

// ---------------------------------------------------------------------------------------------
// Tempos e uso de CPU.

fn dhms(t: u64) -> (u64, u32, u32, u32) {
    let ss = (t % 60) as u32;
    let t = t / 60;
    let mm = (t % 60) as u32;
    let t = t / 60;
    let hh = (t % 24) as u32;
    (t / 24, hh, mm, ss)
}

pub fn pr_etime(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let (dd, hh, mm, ss) = dhms(ps.time_elapsed(p) as u64);
    let mut s = String::new();
    if dd != 0 {
        s.push_str(&format!("{}-", dd as u32));
    }
    if dd != 0 || hh != 0 {
        s.push_str(&format!("{hh:02}:"));
    }
    s.push_str(&format!("{mm:02}:{ss:02}"));
    snp(out, &s)
}

pub fn pr_etimes(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(ps.time_elapsed(p) as u64 as u32).to_string())
}

/// `jiffies` de vida do processo para as contas de %CPU.
fn life_jiffies(ps: &Ps, p: &Pt) -> u64 {
    (ps.time_elapsed(p) * ps.hertz as f64) as u64
}

pub fn pr_c(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let total = ps.tics_all(p);
    let jiffies = life_jiffies(ps, p);
    let mut pcpu: u32 = 0;
    if jiffies != 0 {
        pcpu = (total.wrapping_mul(100) / jiffies) as u32;
    }
    if pcpu > 99 {
        pcpu = 99;
    }
    snp(out, &format!("{pcpu:2}"))
}

pub fn pr_pcpu(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let total = ps.tics_all(p);
    let jiffies = life_jiffies(ps, p);
    let mut pcpu: u32 = 0;
    if jiffies != 0 {
        pcpu = (total.wrapping_mul(1000) / jiffies) as u32;
    }
    if pcpu > 999 {
        return snp(out, &(pcpu / 10).to_string());
    }
    snp(out, &format!("{}.{}", pcpu / 10, pcpu % 10))
}

pub fn pr_cp(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let total = ps.tics_all(p);
    let jiffies = life_jiffies(ps, p);
    let mut pcpu: u32 = 0;
    if jiffies != 0 {
        pcpu = (total.wrapping_mul(1000) / jiffies) as u32;
    }
    if pcpu > 999 {
        pcpu = 999;
    }
    snp(out, &format!("{pcpu:3}"))
}

pub fn pr_time(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let (dd, hh, mm, ss) = dhms(ps.time_all(p) as u64);
    let mut s = String::new();
    if dd != 0 {
        s.push_str(&format!("{}-", dd as u32));
    }
    s.push_str(&format!("{hh:02}:{mm:02}:{ss:02}"));
    snp(out, &s)
}

pub fn pr_times(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(ps.time_all(p) as u64).to_string())
}

pub fn pr_bsdtime(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let u = (ps.tics_all(p) / ps.hertz) as u32;
    snp(out, &format!("{:3}:{:02}", u / 60, u % 60))
}

pub fn pr_utilization(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let mut cu = ps.utilization(p, false);
    if cu > 99.0 {
        cu = 99.999;
    }
    snp(out, &format!("{cu:.3}"))
}

pub fn pr_utilization_c(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let mut cu = ps.utilization(p, true);
    if cu > 99.0 {
        cu = 99.999;
    }
    snp(out, &format!("{cu:.3}"))
}

pub fn pr_bsdstart(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let start = ps.start_epoch(p);
    let mut ago = ps.seconds_since_1970 - start;
    if ago < 0 {
        ago = 0;
    }
    let tz = ps.local_tz();
    let c = ul_misc::util::time::ctime(start, &tz);
    // `ctime` termina em '\n' no original; aqui só interessam os 6 primeiros caracteres.
    let s = if ago > 3600 * 24 { c.get(4..).unwrap_or("") } else { c.get(10..).unwrap_or("") };
    let mut six: Vec<u8> = s.bytes().take(6).collect();
    while six.len() < 6 {
        six.push(0);
    }
    // outbuf[6] = '\0' e retorno 6.
    out.extend(six.iter().take_while(|b| **b != 0));
    6
}

pub fn pr_lstart(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let t = ps.start_epoch(p);
    let tz = ps.local_tz();
    let dt = ul_misc::util::time::civil(t, &tz);
    let abbr = ul_misc::util::time::abbreviation(t, &tz);
    let off = tz.to_offset(jiff::Timestamp::from_second(t).unwrap_or(jiff::Timestamp::UNIX_EPOCH)).seconds();
    let default_fmt: &[u8] = b"%a %b %e %H:%M:%S %Y";
    let fmt = ps.lstart_format.clone();
    let s = util::strftime(fmt.as_deref().unwrap_or(default_fmt), &dt, &abbr, off, t);
    if s.is_empty() || s.len() >= COLWID {
        return 0;
    }
    out.extend_from_slice(&s);
    s.len()
}

pub fn pr_stime(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let t = ps.start_epoch(p);
    let tz = ps.local_tz();
    let now = ul_misc::util::time::civil(ps.seconds_since_1970, &tz);
    let proc_time = ul_misc::util::time::civil(t, &tz);
    let mut fmt: &[u8] = b"%H:%M";
    if now.date().day_of_year() != proc_time.date().day_of_year() {
        fmt = b"%b%d";
    }
    if now.year() != proc_time.year() {
        fmt = b"%Y";
    }
    let abbr = ul_misc::util::time::abbreviation(t, &tz);
    let s = util::strftime(fmt, &proc_time, &abbr, 0, t);
    if s.is_empty() || s.len() >= COLWID {
        return 0;
    }
    out.extend_from_slice(&s);
    s.len()
}

pub fn pr_start(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let t = ps.start_epoch(p);
    let tz = ps.local_tz();
    let c = ul_misc::util::time::ctime(t, &tz);
    let mut cb = c.into_bytes();
    cb.push(b'\n');
    if cb.get(8) == Some(&b' ') {
        cb[8] = b'0';
    }
    if cb.get(11) == Some(&b' ') {
        cb[11] = b'0';
    }
    if (t as u64).wrapping_add(60 * 60 * 24) > ps.seconds_since_1970 as u64 {
        let s: Vec<u8> = cb[11..].iter().copied().take(8).collect();
        return snp_bytes(out, &format!("{:>8}", String::from_utf8_lossy(&s)).into_bytes());
    }
    let s: Vec<u8> = cb[4..].iter().copied().take(6).collect();
    snp_bytes(out, format!("  {:>6}", String::from_utf8_lossy(&s)).as_bytes())
}

// ---------------------------------------------------------------------------------------------
// Ids, prioridades e estado.

pub fn pr_pgid(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(p.pgrp as u32).to_string())
}

pub fn pr_ppid(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(p.ppid as u32).to_string())
}

pub fn pr_procs(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.tgid.to_string())
}

pub fn pr_tasks(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.tid.to_string())
}

pub fn pr_nlwp(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.nlwp.to_string())
}

pub fn pr_sess(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.session.to_string())
}

pub fn pr_tpgid(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.tpgid.to_string())
}

pub fn pr_vsz(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.vm_size.to_string())
}

pub fn pr_priority(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.priority.to_string())
}

pub fn pr_opri(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(60 + p.priority).to_string())
}

pub fn pr_pri_foo(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(p.priority - 20).to_string())
}

pub fn pr_pri_bar(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(p.priority + 1).to_string())
}

pub fn pr_pri_baz(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(p.priority + 100).to_string())
}

pub fn pr_pri(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(39 - p.priority).to_string())
}

pub fn pr_pri_api(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(-1 - p.priority).to_string())
}

pub fn pr_nice(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    if p.sched != 0 && p.sched != 3 && p.sched != -1 {
        return snp(out, "-");
    }
    snp(out, &p.nice.to_string())
}

pub fn pr_oom_adj(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.oom().1.to_string())
}

pub fn pr_oom(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.oom().0.to_string())
}

pub fn pr_class(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let s = match p.sched {
        -1 => "-",
        0 => "TS",
        1 => "FF",
        2 => "RR",
        3 => "B",
        4 => "ISO",
        5 => "IDL",
        6 => "DLN",
        7 => "#7",
        8 => "#8",
        9 => "#9",
        _ => "?",
    };
    snp(out, s)
}

pub fn pr_rtprio(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    if p.sched == 0 || p.sched == -1 {
        return snp(out, "-");
    }
    snp(out, &p.rtprio.to_string())
}

pub fn pr_sched(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    if p.sched == -1 {
        return snp(out, "-");
    }
    snp(out, &p.sched.to_string())
}

pub fn pr_wchan(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let w = p.wchan_name();
    let len = w.len().min(ps.max_rightward.max(0) as usize);
    out.extend_from_slice(&w[..len]);
    len
}

pub fn pr_tty4(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let n = dev_to_tty(ps, p.tty, p.tid, true);
    snp_bytes(out, &n)
}

pub fn pr_tty8(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let n = dev_to_tty(ps, p.tty, p.tid, false);
    snp_bytes(out, &n)
}

pub fn pr_stat(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let before = out.len();
    out.push(p.state);
    if p.nice < 0 {
        out.push(b'<');
    }
    if p.nice > 0 {
        out.push(b'N');
    }
    if p.vm_lock != 0 {
        out.push(b'L');
    }
    if p.session == p.tgid {
        out.push(b's');
    }
    if p.nlwp > 1 {
        out.push(b'l');
    }
    if p.pgrp == p.tpgid {
        out.push(b'+');
    }
    out.len() - before
}

pub fn pr_s(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    out.push(p.state);
    1
}

pub fn pr_flag(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &format!("{:o}", ((p.flags >> 6) & 0x7) as u32))
}

pub fn pr_stackp(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &format!("{:016x}", p.start_stack))
}

pub fn pr_esp(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &format!("{:016x}", p.kstk_esp))
}

pub fn pr_eip(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &format!("{:016x}", p.kstk_eip))
}

pub fn pr_nop(_ps: &mut Ps, _p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, "-")
}

pub fn pr_sz(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let div = (ps.page_size / 1024).max(1) as u64;
    snp(out, &(p.vm_size / div).to_string())
}

fn code_sizes(p: &Pt) -> (i64, i64) {
    // (dsiz, tsiz) como as contas de pr_dsiz e pr_tsiz.
    let mut dsiz: i64 = 0;
    let mut tsiz: i64 = 0;
    if p.vsize != 0 {
        dsiz = (p.vsize.wrapping_sub(p.end_code).wrapping_add(p.start_code) >> 10) as i64;
        tsiz = (p.end_code.wrapping_sub(p.start_code) >> 10) as i64;
    }
    (dsiz, tsiz)
}

pub fn pr_dsiz(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &code_sizes(p).0.to_string())
}

pub fn pr_tsiz(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &code_sizes(p).1.to_string())
}

pub fn pr_drs(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &code_sizes(p).0.to_string())
}

pub fn pr_trs(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &code_sizes(p).1.to_string())
}

pub fn pr_swapable(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.vm_data.wrapping_add(p.vm_stack).to_string())
}

pub fn pr_size(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.vsize.to_string())
}

pub fn pr_minflt(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let flt = if ps.include_dead_children { p.min_flt.wrapping_add(p.cmin_flt) } else { p.min_flt };
    snp(out, &flt.to_string())
}

pub fn pr_majflt(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let flt = if ps.include_dead_children { p.maj_flt.wrapping_add(p.cmaj_flt) } else { p.maj_flt };
    snp(out, &flt.to_string())
}

pub fn pr_lim(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    if p.rss_rlim == u64::MAX {
        out.extend_from_slice(b"xx");
        return 2;
    }
    snp(out, &format!("{:5}", p.rss_rlim >> 10))
}

pub fn pr_psr(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.processor.to_string())
}

pub fn pr_pss(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.smaps().0.to_string())
}

pub fn pr_uss(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.smaps().1.to_string())
}

pub fn pr_numa(_ps: &mut Ps, _p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, "-1")
}

pub fn pr_rss(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.vm_rss.to_string())
}

pub fn pr_pmem(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let total = ps.memory_total();
    let mut pmem = if total == 0 { 0 } else { p.vm_rss.wrapping_mul(1000) / total };
    if pmem > 999 {
        pmem = 999;
    }
    snp(out, &format!("{:2}.{}", (pmem / 10) as u32, (pmem % 10) as u32))
}

// ---------------------------------------------------------------------------------------------
// Sinais.

/// `sigstat_strsignal_abbrev` da signames.c.
fn signal_abbrev(sig: i32) -> String {
    const RTMIN: i32 = 34;
    const RTMAX: i32 = 64;
    const NSIG: i32 = 65;
    if sig == 0 || sig >= NSIG {
        return format!("BOGUS_{:02}", sig - 65);
    }
    if sig < RTMIN - 2
        && let Some(n) = crate::common::signal_name(sig) {
            return match n {
                "POLL" => "IO".to_string(),
                other => other.to_string(),
            };
        }
    if sig >= 34 {
        if sig == 34 {
            return "RTMIN".to_string();
        }
        if sig == RTMAX {
            return "RTMAX".to_string();
        }
        return format!("RTMIN+{:02}", sig - 34);
    }
    format!("LIBC+{:02}", sig - 32)
}

/// `print_signame`: máscara decimal para nomes (`HUP,INT`). `None` se não é número.
fn print_signame(sig: &[u8], len_in: i32) -> Option<Vec<u8>> {
    let digits: Vec<u8> = sig.iter().copied().take_while(u8::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let mask: u64 = String::from_utf8_lossy(&digits).parse().unwrap_or(u64::MAX);
    let mut out: Vec<u8> = Vec::new();
    let mut len = len_in;
    for i in 1..65 {
        if mask & (1u64 << (i - 1)) != 0 {
            let name = signal_abbrev(i);
            let n = name.len() as i32;
            if n + 1 >= len {
                out.push(b'+');
                break;
            }
            let piece = if out.is_empty() { name } else { format!(",{name}") };
            len -= piece.len() as i32;
            out.extend_from_slice(piece.as_bytes());
        }
    }
    if out.is_empty() {
        out.push(b'-');
    }
    Some(out)
}

fn help_pr_sig(ps: &Ps, out: &mut Vec<u8>, sig: &[u8]) -> usize {
    let len = sig.len();
    if ps.signal_names
        && let Some(v) = print_signame(sig, ps.max_rightward) {
            out.extend_from_slice(&v);
            return v.len();
        }
    let s = String::from_utf8_lossy(sig).into_owned();
    if ps.wide_signals {
        if len > 8 {
            return snp(out, &s);
        }
        return snp(out, &format!("00000000{s}"));
    }
    let zeros = sig.iter().take_while(|b| **b == b'0').count();
    if len - zeros > 8 {
        return snp(out, &format!("<{}", &s[len - 8..]));
    }
    if len < 8 {
        return snp(out, &format!("{}{}", &"00000000"[len..], s));
    }
    snp(out, &s[len - 8..])
}

pub fn pr_tsig(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    help_pr_sig(ps, out, &p.sigpnd)
}

pub fn pr_sig(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    help_pr_sig(ps, out, &p.signal)
}

pub fn pr_sigmask(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    help_pr_sig(ps, out, &p.blocked)
}

pub fn pr_sigignore(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    help_pr_sig(ps, out, &p.sigignore)
}

pub fn pr_sigcatch(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    help_pr_sig(ps, out, &p.sigcatch)
}

// ---------------------------------------------------------------------------------------------
// Usuários e grupos.

pub fn pr_egid(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(p.egid as i32).to_string())
}

pub fn pr_rgid(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(p.rgid as i32).to_string())
}

pub fn pr_sgid(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(p.sgid as i32).to_string())
}

pub fn pr_fgid(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(p.fgid as i32).to_string())
}

pub fn pr_euid(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(p.euid as i32).to_string())
}

pub fn pr_ruid(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(p.ruid as i32).to_string())
}

pub fn pr_suid(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(p.suid as i32).to_string())
}

pub fn pr_fuid(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &(p.fuid as i32).to_string())
}

pub fn pr_luid(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let l = p.luid();
    if l == -1 {
        return snp(out, "-");
    }
    snp(out, &l.to_string())
}

/// `do_pr_name`: nome de usuário ou grupo; cortado com `+` se não cabe, ou o número com `-n`.
fn do_pr_name(ps: &mut Ps, out: &mut Vec<u8>, name: &[u8], u: u32) -> usize {
    if !ps.user_is_number {
        let mut rightward = OUTBUF_SIZE;
        let start = out.len();
        escape_str_out(out, name, OUTBUF_SIZE, &mut rightward);
        let len = (OUTBUF_SIZE - rightward) as usize;
        let max = ps.max_rightward.max(0) as usize;
        if len <= max {
            return len;
        }
        // Só usa o '+' se não está no meio de um caractere multibyte.
        if max >= 1 && out.get(start + max - 1).is_some_and(|b| *b < 127) {
            out.truncate(start + max - 1);
            out.push(b'+');
            return max;
        }
        out.truncate(start);
    }
    snp(out, &u.to_string())
}

macro_rules! name_printer {
    ($fname:ident, $field:ident, $id:ident, $lookup:ident) => {
        pub fn $fname(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
            let n = ps.$lookup(p.$id);
            do_pr_name(ps, out, &n, p.$id)
        }
    };
}

name_printer!(pr_ruser, ruser, ruid, user_name);
name_printer!(pr_euser, euser, euid, user_name);
name_printer!(pr_fuser, fuser, fuid, user_name);
name_printer!(pr_suser, suser, suid, user_name);
name_printer!(pr_egroup, egroup, egid, group_name);
name_printer!(pr_rgroup, rgroup, rgid, group_name);
name_printer!(pr_fgroup, fgroup, fgid, group_name);
name_printer!(pr_sgroup, sgroup, sgid, group_name);

pub fn pr_supgid(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let s = p.supgid.clone().unwrap_or_else(|| b"-".to_vec());
    let n = s.len().min(ps.max_rightward.max(0) as usize);
    out.extend_from_slice(&s[..n]);
    n
}

pub fn pr_supgrp(ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    let supgid = p.supgid.clone().unwrap_or_else(|| b"-".to_vec());
    let mut names: Vec<u8> = Vec::new();
    if supgid.first() != Some(&b'-') {
        let mut rest: &[u8] = &supgid;
        let mut first = true;
        loop {
            while rest.first() == Some(&b',') {
                rest = &rest[1..];
            }
            let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
            if digits == 0 {
                break;
            }
            let gid: u32 = String::from_utf8_lossy(&rest[..digits]).parse().unwrap_or(0);
            rest = &rest[digits..];
            if !first {
                names.push(b',');
            }
            first = false;
            names.extend(ps.group_name(gid));
            if rest.is_empty() {
                break;
            }
        }
    }
    if names.is_empty() {
        names.push(b'-');
    }
    let mut rightward = ps.max_rightward;
    escape_str_out(out, &names, OUTBUF_SIZE, &mut rightward);
    (ps.max_rightward - rightward) as usize
}

pub fn pr_sgi_p(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    if p.state == b'R' {
        return snp(out, &(p.processor as u32).to_string());
    }
    snp(out, "*")
}

// ---------------------------------------------------------------------------------------------
// IO, namespaces, systemd, contexto e autogrupo.

macro_rules! io_printer {
    ($fname:ident, $idx:expr) => {
        pub fn $fname(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
            snp(out, &p.io()[$idx].to_string())
        }
    };
}

io_printer!(pr_rchars, 0);
io_printer!(pr_wchars, 1);
io_printer!(pr_rops, 2);
io_printer!(pr_wops, 3);
io_printer!(pr_rbytes, 4);
io_printer!(pr_wbytes, 5);
io_printer!(pr_wcbytes, 6);

macro_rules! ns_printer {
    ($fname:ident, $idx:expr) => {
        pub fn $fname(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
            let v = p.ns()[$idx];
            if v != 0 { snp(out, &v.to_string()) } else { snp(out, "-") }
        }
    };
}

ns_printer!(pr_cgroupns, 0);
ns_printer!(pr_ipcns, 1);
ns_printer!(pr_mntns, 2);
ns_printer!(pr_netns, 3);
ns_printer!(pr_pidns, 4);
ns_printer!(pr_timens, 5);
ns_printer!(pr_userns, 6);
ns_printer!(pr_utsns, 7);

/// Colunas do systemd: o ps do Debian consulta o logind, que num sandbox sem sessão responde
/// erro, e o procps imprime `-`.
macro_rules! sd_printer {
    ($fname:ident) => {
        pub fn $fname(_ps: &mut Ps, _p: &Pt, out: &mut Vec<u8>) -> usize {
            snp(out, "-")
        }
    };
}

sd_printer!(pr_sd_unit);
sd_printer!(pr_sd_session);
sd_printer!(pr_sd_ouid);
sd_printer!(pr_sd_machine);
sd_printer!(pr_sd_uunit);
sd_printer!(pr_sd_seat);
sd_printer!(pr_sd_slice);

pub fn pr_lxcname(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp_bytes(out, p.lxcname())
}

pub fn pr_context(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    if let Some(d) = proc::read_path(&format!("/proc/{}/attr/current", p.tgid))
        && !d.is_empty() {
            let len = d.iter().take_while(|b| (0x20..0x7f).contains(*b)).count();
            if len > 0 {
                out.extend_from_slice(&d[..len]);
                return len;
            }
        }
    out.push(b'-');
    1
}

pub fn pr_agid(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.autogroup().0.to_string())
}

pub fn pr_agnice(_ps: &mut Ps, p: &Pt, out: &mut Vec<u8>) -> usize {
    snp(out, &p.autogroup().1.to_string())
}

// ---------------------------------------------------------------------------------------------
// Colunas de teste do upstream (`_left`, `_right`, `_unlimited`...).

fn cycle(ps: &Ps, vals: &[&str]) -> usize {
    let n = vals.len() as u32;
    (ps.lines_to_next_header as u32 % n) as usize
}

pub fn pr_t_unlimited(ps: &mut Ps, _p: &Pt, out: &mut Vec<u8>) -> usize {
    const V: [&str; 3] = ["[123456789-12345] <defunct>", "ps", "123456789-123456"];
    let s = V[cycle(ps, &V)];
    let n = s.len().min((ps.max_rightward + 1).max(0) as usize).saturating_sub(0);
    let n = n.min(ps.max_rightward.max(0) as usize);
    out.extend_from_slice(&s.as_bytes()[..n]);
    n
}

pub fn pr_t_unlimited2(ps: &mut Ps, _p: &Pt, out: &mut Vec<u8>) -> usize {
    const V: [&str; 4] = ["unlimited", "[123456789-12345] <defunct>", "ps", "123456789-123456"];
    let s = V[cycle(ps, &V)];
    let n = s.len().min(ps.max_rightward.max(0) as usize);
    out.extend_from_slice(&s.as_bytes()[..n]);
    n
}

pub fn pr_t_right(ps: &mut Ps, _p: &Pt, out: &mut Vec<u8>) -> usize {
    const V: [&str; 4] = ["999-23:59:59", "99-23:59:59", "9-23:59:59", "59:59"];
    snp(out, V[cycle(ps, &V)])
}

pub fn pr_t_right2(ps: &mut Ps, _p: &Pt, out: &mut Vec<u8>) -> usize {
    const V: [&str; 3] = ["999-23:59:59", "99-23:59:59", "9-23:59:59"];
    snp(out, V[cycle(ps, &V)])
}

pub fn pr_t_left(ps: &mut Ps, _p: &Pt, out: &mut Vec<u8>) -> usize {
    const V: [&str; 5] = ["tty7", "pts/9999", "iseries/vtty42", "ttySMX0", "3270/tty4"];
    snp(out, V[cycle(ps, &V)])
}

pub fn pr_t_left2(ps: &mut Ps, _p: &Pt, out: &mut Vec<u8>) -> usize {
    const V: [&str; 4] = ["tty7", "pts/9999", "ttySMX0", "3270/tty4"];
    snp(out, V[cycle(ps, &V)])
}

// ---------------------------------------------------------------------------------------------
// Largura das colunas e saída das linhas.

impl Ps {
    /// `print_format_specifiers` (`ps L`).
    pub(super) fn print_format_specifiers(&self) {
        let mut s = String::new();
        for f in FORMAT_ARRAY {
            if f.spec == "~" {
                break;
            }
            if !f.nop {
                let spec: String = f.spec.chars().take(12).collect();
                let head: String = f.head.chars().take(8).collect();
                s.push_str(&format!("{spec:<12} {head:<8}\n"));
            }
        }
        crate::common::out(s);
    }

    /// `check_header_width`: quantas telas de largura a linha precisa e se cabem sinais largos.
    pub(super) fn check_header_width(&mut self) {
        let mut total: u32 = 0;
        let mut was_normal: u32 = 0;
        let mut sigs: u32 = 0;
        let n = self.format_list.len();
        for idx in 0..n {
            let has_next = idx + 1 < n;
            let width = self.format_list[idx].width as u32;
            match self.format_list[idx].flags & CF_JUST_MASK {
                CF_SIGNAL => {
                    sigs += 1;
                    if self.signal_names {
                        if self.format_list[idx].width < SIGNAL_NAME_WIDTH {
                            self.format_list[idx].width = SIGNAL_NAME_WIDTH;
                        }
                        self.format_list[idx].flags = CF_UNLIMITED;
                        if has_next {
                            total += self.format_list[idx].width as u32;
                        } else {
                            total += 3;
                        }
                    } else {
                        total += width;
                    }
                    total += was_normal;
                    was_normal = 1;
                }
                CF_UNLIMITED => {
                    if has_next {
                        total += width;
                    } else {
                        total += 3;
                    }
                    total += was_normal;
                    was_normal = 1;
                }
                0 => {
                    total += width;
                    was_normal = 0;
                }
                _ => {
                    total += width;
                    total += was_normal;
                    was_normal = 1;
                }
            }
        }
        let mut i: i64 = 0;
        loop {
            i += 1;
            self.active_cols = (i64::from(self.screen_cols) * i) as i32;
            if self.active_cols as i64 >= i64::from(total) {
                break;
            }
            if i64::from(self.screen_cols) * i >= i64::from(OUTBUF_SIZE / 2) {
                break;
            }
        }
        self.wide_signals = i64::from(total) + i64::from(sigs) * 7 <= i64::from(self.active_cols);
    }

    /// `show_one_proc`: uma linha de processo (`Some`) ou o cabeçalho (`None`).
    pub(super) fn show_one_proc(&mut self, p: Option<&Pt>, fmt: &[FNode]) {
        let mut correct: i32 = 0;
        let mut actual: i32 = 0;
        let mut dospace: i32 = 0;
        if p.is_some() {
            self.lines_to_next_header -= 1;
            if self.lines_to_next_header == 0 {
                self.lines_to_next_header = self.header_gap;
                self.show_one_proc(None, fmt);
            }
        }
        self.did_stuff = true;
        if self.active_cols > OUTBUF_SIZE {
            io::eprint("fix bigness error\n");
        }
        let mut row: Vec<u8> = Vec::new();
        let mut buf: Vec<u8> = Vec::new();
        for (k, node) in fmt.iter().enumerate() {
            let last = k + 1 == fmt.len();
            let mut legit = 0;
            let tmpspace;
            if !last {
                self.max_rightward = node.width;
                tmpspace = 0;
            } else {
                let t = correct - actual;
                if t < 1 {
                    tmpspace = dospace;
                    self.max_rightward = self.active_cols - actual - tmpspace;
                } else {
                    tmpspace = t;
                    self.max_rightward = self.active_cols - if correct > actual { correct } else { actual };
                }
            }
            if self.max_rightward <= 0 {
                self.max_rightward = 0;
            } else if self.max_rightward >= OUTBUF_SIZE {
                self.max_rightward = OUTBUF_SIZE - 1;
            }
            buf.clear();
            let mut amount: i32 = match (p, node.pr) {
                (Some(pt), Some(pr)) => pr(self, pt, &mut buf) as i32,
                _ => {
                    buf.extend_from_slice(&node.name);
                    node.name.len() as i32
                }
            };
            if amount < 0 {
                buf.clear();
                amount = 0;
            } else if amount >= OUTBUF_SIZE {
                buf.truncate((OUTBUF_SIZE - 1) as usize);
                amount = OUTBUF_SIZE - 1;
            }
            let mut leftpad: i32;
            match node.flags & CF_JUST_MASK {
                0 | CF_LEFT => leftpad = 0,
                CF_RIGHT => {
                    leftpad = node.width - amount;
                    if leftpad < 0 {
                        leftpad = 0;
                    }
                }
                CF_SIGNAL => {
                    if self.wide_signals {
                        leftpad = 16 - amount;
                        legit = 7;
                    } else {
                        leftpad = 9 - amount;
                    }
                    if leftpad < 0 {
                        leftpad = 0;
                    }
                }
                CF_USER => {
                    leftpad = node.width - amount;
                    if leftpad < 0 {
                        leftpad = 0;
                    }
                    if !self.user_is_number {
                        leftpad = 0;
                    }
                }
                CF_WCHAN => {
                    if self.wchan_is_number {
                        leftpad = node.width - amount;
                        if leftpad < 0 {
                            leftpad = 0;
                        }
                    } else {
                        if self.active_cols - actual - tmpspace < 1 {
                            buf.truncate(1);
                        }
                        leftpad = 0;
                    }
                }
                CF_UNLIMITED => {
                    if self.active_cols - actual - tmpspace < 1 {
                        buf.truncate(1);
                    }
                    leftpad = 0;
                }
                _ => {
                    io::eprint("bad alignment code\n");
                    leftpad = 0;
                }
            }
            let mut space = correct - actual + leftpad;
            if space < 1 {
                space = dospace;
            }
            if space > SPACE_AMOUNT {
                space = SPACE_AMOUNT;
            }
            // `strlen(outbuf)`: o dado termina no primeiro NUL, que nunca é escrito aqui.
            let sz = buf.len();
            row.extend(std::iter::repeat_n(b' ', space.max(0) as usize));
            row.extend_from_slice(&buf);
            if last {
                row.push(b'\n');
                break;
            }
            actual += space + amount;
            correct += node.width;
            correct += legit;
            if node.pr.is_some() && fmt[k + 1].pr.is_some() {
                correct += 1;
                dospace = 1;
            } else {
                dospace = 0;
            }
            let _ = sz;
        }
        crate::common::out(&row);
    }
}

/// Compara duas strings como `strverscmp` (a ordenação de `tty`).
pub(super) fn cmp_vers(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    strverscmp(a, b)
}
