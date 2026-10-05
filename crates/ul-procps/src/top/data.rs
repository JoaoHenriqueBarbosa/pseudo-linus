//! Os dados do top: `/proc/stat` (a libproc2 `stat.c`), `/proc/meminfo` (`meminfo.c`), a lista de
//! processos com o histórico dos ticks (`pids.c`) e a linha de uptime (`uptime.c`).

use std::collections::HashMap;

use sysabi::{Clock, sys};

use super::{R, Task, Top};
use crate::procfs;
use crate::ps::proc::{Pt, reap, select_pids};

/// `struct stat_jifs`: os ticks de uma linha `cpu` mais os derivados.
#[derive(Clone, Copy, Default)]
pub struct Jifs {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    sirq: u64,
    stolen: u64,
    guest: u64,
    gnice: u64,
    xusr: u64,
    xsys: u64,
    xidl: u64,
    xbsy: u64,
    xtot: u64,
}

impl Jifs {
    /// `stat_derive_unique`, a parte que calcula os totais.
    fn derive(&mut self) {
        self.xusr = self.user.wrapping_add(self.nice);
        self.xsys = self.system.wrapping_add(self.irq).wrapping_add(self.sirq);
        self.xidl = self.idle.wrapping_add(self.iowait);
        self.xtot = self
            .xusr
            .wrapping_add(self.xsys)
            .wrapping_add(self.xidl)
            .wrapping_add(self.stolen)
            .wrapping_add(self.guest)
            .wrapping_add(self.gnice);
        self.xbsy = self.xtot.wrapping_sub(self.xidl);
    }
}

/// `struct hist_tic`: os ticks novos e os da leitura anterior.
#[derive(Clone, Copy, Default)]
pub struct HistTic {
    pub id: i32,
    old: Jifs,
    new: Jifs,
}

/// `TICsetH`: a diferença entre as duas leituras, nunca negativa.
fn delta(new: u64, old: u64) -> i64 {
    let d = new.wrapping_sub(old) as i64;
    if d < 0 { 0 } else { d }
}

/// Os resultados de uma linha de CPU que o top consulta (`Stat_items`).
#[derive(Clone, Copy, Default)]
pub struct CpuDelta {
    pub us: i64,
    pub sy: i64,
    pub ni: i64,
    pub il: i64,
    pub io: i64,
    pub ir: i64,
    pub si: i64,
    pub st: i64,
    pub gu: i64,
    pub gn: i64,
    pub sum_tot: i64,
}

impl HistTic {
    pub fn delta(&self) -> CpuDelta {
        let (n, o) = (&self.new, &self.old);
        CpuDelta {
            us: delta(n.user, o.user),
            sy: delta(n.system, o.system),
            ni: delta(n.nice, o.nice),
            il: delta(n.idle, o.idle),
            io: delta(n.iowait, o.iowait),
            ir: delta(n.irq, o.irq),
            si: delta(n.sirq, o.sirq),
            st: delta(n.stolen, o.stolen),
            gu: delta(n.guest, o.guest),
            gn: delta(n.gnice, o.gnice),
            sum_tot: delta(n.xtot, o.xtot),
        }
    }
}

/// O estado da leitura de `/proc/stat`.
#[derive(Default)]
pub struct StatState {
    pub summary: HistTic,
    pub cpus: Vec<HistTic>,
}

/// Lê no máximo `max` números decimais a partir de `s` (o `%llu %llu ...` do `sscanf`, que pula
/// qualquer espaço, inclusive quebras de linha, e para no primeiro texto que não é número).
fn scan_nums(s: &[u8], max: usize) -> Vec<u64> {
    let mut out = Vec::new();
    let mut i = 0;
    while out.len() < max {
        while i < s.len() && s[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        let mut v: u64 = 0;
        while i < s.len() && s[i].is_ascii_digit() {
            v = v.wrapping_mul(10).wrapping_add(u64::from(s[i] - b'0'));
            i += 1;
        }
        if i == start {
            break;
        }
        out.push(v);
    }
    out
}

/// Grava em `j` os números lidos, na ordem de `stat_jifs`; o que faltou fica como estava.
fn assign(j: &mut Jifs, nums: &[u64]) {
    let slots: [&mut u64; 10] = [
        &mut j.user,
        &mut j.nice,
        &mut j.system,
        &mut j.idle,
        &mut j.iowait,
        &mut j.irq,
        &mut j.sirq,
        &mut j.stolen,
        &mut j.guest,
        &mut j.gnice,
    ];
    for (slot, v) in slots.into_iter().zip(nums) {
        *slot = *v;
    }
}

/// `stat_derive_unique`: não distorce os deltas quando uma CPU sai ou volta.
fn derive_unique(h: &mut HistTic) {
    h.new.derive();
    let (n, o) = (&h.new, &h.old);
    if n.xusr < o.xusr || n.xsys < o.xsys || n.xidl < o.xidl || n.xbsy < o.xbsy || n.xtot < o.xtot {
        h.old = h.new;
    }
}

impl StatState {
    /// `stat_read_failed`: uma leitura de `/proc/stat`. `Err` leva a mensagem do `strerror`.
    pub fn read(&mut self) -> Result<(), String> {
        let data = sys::read_file(b"/proc/stat").map_err(|e| e.message().to_string())?;
        let mut bp = 0usize;
        self.summary.old = self.summary.new;
        self.summary.id = -1;
        if !data.starts_with(b"cpu") {
            return Err("Numerical result out of range".to_string());
        }
        let nums = scan_nums(&data[3..], 10);
        if nums.len() < 8 {
            return Err("Numerical result out of range".to_string());
        }
        assign(&mut self.summary.new, &nums);
        derive_unique(&mut self.summary);
        let mut total = 0usize;
        while let Some(nl) = data[bp..].iter().position(|b| *b == b'\n') {
            bp += nl + 1;
            if self.cpus.len() <= total {
                self.cpus.push(HistTic::default());
            }
            let cpu = &mut self.cpus[total];
            cpu.old = cpu.new;
            let rest = &data[bp..];
            if !rest.starts_with(b"cpu") {
                break;
            }
            let digits = rest[3..].iter().take_while(|b| b.is_ascii_digit()).count();
            if digits == 0 {
                break;
            }
            let id: u64 = std::str::from_utf8(&rest[3..3 + digits]).ok().and_then(|d| d.parse().ok()).unwrap_or(0);
            let nums = scan_nums(&rest[3 + digits..], 10);
            // O `sscanf` devolve o id mais os números lidos: menos de 8 interrompe a leitura.
            if nums.len() + 1 < 8 {
                break;
            }
            cpu.id = id as i32;
            assign(&mut cpu.new, &nums);
            derive_unique(cpu);
            total += 1;
        }
        self.cpus.truncate(total);
        Ok(())
    }
}

/// Os valores de `/proc/meminfo` que o top mostra (em kB).
#[derive(Clone, Copy, Default)]
pub struct MemVals {
    pub free: u64,
    pub used: u64,
    pub total: u64,
    pub cached: u64,
    pub buffers: u64,
    pub avail: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub swap_used: u64,
}

/// `meminfo_read_failed` e as contas derivadas (`MemAvailable` ausente, usado, cache, swap).
pub fn read_meminfo() -> Result<MemVals, String> {
    let mut data = sys::read_file(b"/proc/meminfo").map_err(|e| e.message().to_string())?;
    if data.is_empty() {
        return Err("Input/output error".to_string());
    }
    // A libproc2 lê no máximo MEMINFO_BUFF - 1 bytes.
    data.truncate(8191);
    let m = procfs::MemInfo::parse(&data);
    let total = m.get("MemTotal");
    let free = m.get("MemFree");
    let mut avail = m.get("MemAvailable");
    if avail == 0 {
        avail = free;
    }
    let cached = m.get("Cached").wrapping_add(m.get("SReclaimable"));
    if avail > total {
        avail = free;
    }
    let mut used = total as i64 - avail as i64;
    if used < 0 {
        used = total as i64 - free as i64;
    }
    let swap_total = m.get("SwapTotal");
    let swap_free = m.get("SwapFree");
    Ok(MemVals {
        free,
        used: used as u64,
        total,
        cached,
        buffers: m.get("Buffers"),
        avail,
        swap_total,
        swap_free,
        swap_used: swap_total.saturating_sub(swap_free),
    })
}

/// O histórico de um processo entre duas leituras (`HST_t`).
#[derive(Clone, Copy)]
pub struct Hist {
    tics: u64,
    maj: u64,
    min: u64,
}

/// As contagens de `pids_counts`.
#[derive(Clone, Copy, Default)]
pub struct Counts {
    pub total: i32,
    pub running: i32,
    pub sleeping: i32,
    pub stopped: i32,
    pub zombied: i32,
    pub other: i32,
}

impl Top {
    /// A hora local, o tempo ligado, os usuários e a carga (`procps_uptime_sprint`).
    pub fn uptime_sprint(&self) -> String {
        let Some((up, _)) = procfs::uptime_file() else { return String::new() };
        let (now, _) = procfs::now_realtime();
        let tz = ul_misc::util::time::local_tz();
        let dt = ul_misc::util::time::civil(now, &tz);
        let load = procfs::loadavg().unwrap_or_default();
        format!(
            " {:02}:{:02}:{:02} {}",
            dt.hour(),
            dt.minute(),
            dt.second(),
            crate::uptime::up_and_load(up, crate::common::utmp_users(), &load)
        )
    }

    /// `cpus_refresh`.
    pub fn cpus_refresh(&mut self, line: u32) -> R<()> {
        if let Err(e) = self.stat.read() {
            return self.error_exit(&format!("library failed cpu statistics, at {line}: {e}"));
        }
        let total = self.stat.cpus.len() as i32;
        if total != 0 && total != self.cpu_cnt {
            self.cpu_cnt = total;
        }
        Ok(())
    }

    /// `memory_refresh`: relê só se passaram 3 s desde a última leitura.
    pub fn memory_refresh(&mut self) -> R<()> {
        let (cur, _) = procfs::now_realtime();
        if cur - self.mem_secs >= 3 {
            match read_meminfo() {
                Ok(v) => self.mem = v,
                Err(e) => return self.error_exit(&format!("library failed memory statistics, at 2786: {e}")),
            }
            self.mem_secs = cur;
        }
        Ok(())
    }

    /// `tasks_refresh`: o tempo decorrido, a escala do %CPU e a lista de processos com os deltas.
    pub fn tasks_refresh(&mut self) -> R<()> {
        let up = crate::uptime::uptime_secs();
        let mut et = (up - self.uptime_sav) as f32;
        if et < 0.01 {
            et = 0.005;
        }
        self.uptime_sav = up;
        // Modo Irix (o padrão): sem dividir pelo número de CPUs.
        self.frame_etscale = 100.0f32 / ((self.hertz as f32) * et * 1.0f32);

        let ts = sys::try_current().and_then(|s| s.clock_gettime(Clock::Boottime).ok());
        self.boot_tics = match ts {
            Some(t) => ((t.sec as f64 + f64::from(t.nsec) * 1.0e-9) * self.hertz as f64) as u64,
            None => 0,
        };
        let pts: Vec<Pt> = if self.monpids.is_empty() {
            reap(self.thread_mode)
        } else {
            let ids: Vec<u32> = self.monpids.iter().map(|p| *p as u32).collect();
            select_pids(&ids, self.thread_mode)
        };
        let need_hist = self.need_history();
        let mut counts = Counts::default();
        let mut next: HashMap<i32, Hist> = HashMap::new();
        let mut tasks: Vec<Task> = Vec::with_capacity(pts.len());
        for p in pts {
            match p.state {
                b'R' => counts.running += 1,
                b'D' | b'S' => counts.sleeping += 1,
                b't' | b'T' => counts.stopped += 1,
                b'Z' => counts.zombied += 1,
                _ => counts.other += 1,
            }
            counts.total += 1;
            let mut t = Task { pcpu: 0, maj_delta: 0, min_delta: 0, p };
            if need_hist {
                let tics = t.p.utime.wrapping_add(t.p.stime);
                next.insert(t.p.tid, Hist { tics, maj: t.p.maj_flt, min: t.p.min_flt });
                let mut used = tics;
                if let Some(h) = self.hist.get(&t.p.tid) {
                    used = tics.wrapping_sub(h.tics);
                    t.maj_delta = t.p.maj_flt.wrapping_sub(h.maj) as i32;
                    t.min_delta = t.p.min_flt.wrapping_sub(h.min) as i32;
                }
                t.pcpu = used as u32;
            }
            tasks.push(t);
        }
        self.hist = next;
        self.tasks = tasks;
        self.counts = counts;
        Ok(())
    }

    /// Algum item da pilha pede histórico? (%CPU entre os campos mostrados, a ordenação ou a
    /// ausência dos processos ociosos, e os deltas de falhas de página.)
    fn need_history(&self) -> bool {
        use super::fields::{EU_CPU, EU_FV1, EU_FV2};
        let shown = self.procflgs.contains(&EU_CPU);
        shown || !self.w.show_idleps || [EU_CPU, EU_FV1, EU_FV2].contains(&self.w.sortindx)
    }
}
