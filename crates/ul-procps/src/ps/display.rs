//! Fluxo principal do ps (display.c) e a seleção de processos (select.c): `run` encadeia o parser,
//! a preparação da saída e a listagem simples, ordenada ou em floresta.

use std::cmp::Ordering;

use sysabi::{Clock, sys};
use ul_misc::util::io;

use super::output::pr_nop;
use super::proc::{Pt, reap, select_pids};
use super::util::strverscmp;
use super::*;

/// Valor de ordenação de um item (os tipos de resultado da libproc2).
#[derive(Clone, Debug)]
pub(crate) enum Key {
    None,
    Int(i64),
    UInt(u64),
    Real(f64),
    Str(Vec<u8>),
    Vers(Vec<u8>),
}

pub(crate) fn cmp_keys(a: &Key, b: &Key) -> Ordering {
    match (a, b) {
        (Key::Int(x), Key::Int(y)) => x.cmp(y),
        (Key::UInt(x), Key::UInt(y)) => x.cmp(y),
        (Key::Real(x), Key::Real(y)) => {
            if x > y {
                Ordering::Greater
            } else if x < y {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        }
        (Key::Str(x), Key::Str(y)) => x.cmp(y),
        (Key::Vers(x), Key::Vers(y)) => strverscmp(x, y),
        _ => Ordering::Equal,
    }
}

pub fn run(ps: &mut Ps) -> R<()> {
    ps.hertz = 100;
    ps.reset_global()?;
    ps.arg_parse()?;
    ps.arg_check_conflicts()?;
    ps.init_output();
    ps.lists_and_needs();
    if ps.forest_type != 0 || !ps.sort_list.is_empty() {
        ps.fancy_spew()?;
    } else {
        ps.simple_spew()?;
    }
    ps.show_end()
}

impl Ps {
    // -----------------------------------------------------------------------------------------
    // Seleção (select.c).

    /// `select_bits_setup`: máscara dos processos escolhidos pelas opções simples.
    pub(super) fn select_bits_setup(&mut self) -> Result<(), String> {
        if self.simple_select == 0 && !self.prefer_bsd_defaults {
            self.select_bits = 0xaa00;
            return Ok(());
        }
        let switch_val = if self.personality & PER_NO_DEFAULT_G == 0 && self.simple_select & (SS_U_A | SS_U_D) == 0 {
            self.simple_select | SS_B_G
        } else {
            self.simple_select
        };
        match switch_val {
            x if x == SS_U_A | SS_U_D => self.select_bits = 0x3f3f,
            x if x == SS_U_A => self.select_bits = 0x0303,
            x if x == SS_U_D => self.select_bits = 0x3333,
            0 => self.select_bits = 0x0202,
            x if x == SS_B_A => self.select_bits = 0x0303,
            x if x == SS_B_X => self.select_bits = 0x2222,
            x if x == SS_B_X | SS_B_A => self.select_bits = 0x3333,
            x if x == SS_B_G => self.select_bits = 0x0a0a,
            x if x == SS_B_G | SS_B_A => self.select_bits = 0x0f0f,
            x if x == SS_B_G | SS_B_X => self.select_bits = 0xaaaa,
            x if x == SS_B_G | SS_B_X | SS_B_A => {
                self.all_processes = true;
                self.simple_select = 0;
            }
            _ => return Err("process selection options conflict".into()),
        }
        Ok(())
    }

    fn table_accept(&self, p: &Pt) -> bool {
        let idx = u32::from(p.euid == self.cached_euid)
            | (u32::from(p.session == p.tgid) << 1)
            | (u32::from(p.tty == 0) << 2)
            | (u32::from(p.tty == self.cached_tty) << 3);
        self.select_bits & (1 << idx) != 0
    }

    fn proc_was_listed(&self, p: &Pt) -> bool {
        for sn in &self.selection_list {
            let hit = |v: u32| sn.nums.iter().any(|n| *n as u32 == v);
            let found = match sn.typecode {
                SEL_RUID => hit(p.ruid),
                SEL_EUID => hit(p.euid),
                SEL_RGID => hit(p.rgid),
                SEL_EGID => hit(p.egid),
                SEL_PGRP => hit(p.pgrp as u32),
                SEL_PID | SEL_PID_QUICK => hit(p.tgid as u32),
                SEL_PPID => hit(p.ppid as u32),
                SEL_TTY => hit(p.tty as u32),
                SEL_SESS => hit(p.session as u32),
                SEL_COMM => sn.cmds.iter().any(|c| {
                    let cmd = &p.cmd;
                    let cl = cmd.iter().position(|b| *b == 0).unwrap_or(cmd.len());
                    let cmd = &cmd[..cl];
                    let sl = c.iter().position(|b| *b == 0).unwrap_or(c.len());
                    let sel = &c[..sl];
                    if cmd.len() == 15 && sel.len() >= 15 && cmd[..15] == sel[..15] {
                        return true;
                    }
                    cmd == sel
                }),
                _ => false,
            };
            if found {
                return true;
            }
        }
        false
    }

    /// `want_this_proc`: o processo passa pela seleção?
    pub(super) fn want_this_proc(&self, p: &Pt) -> bool {
        let mut accepted = true;
        if !self.all_processes {
            let by_table = (self.simple_select != 0 || self.selection_list.is_empty()) && self.table_accept(p);
            if !by_table && !self.proc_was_listed(p) {
                accepted = false;
            }
        }
        if self.running_only && !(p.state == b'R' || p.state == b'D') {
            accepted = false;
        }
        if self.negate_selection { !accepted } else { accepted }
    }

    // -----------------------------------------------------------------------------------------
    // Preparação.

    fn check_headers(&mut self) {
        if self.header_type == HEAD_MULTI {
            self.header_gap = self.screen_rows - 1;
            return;
        }
        if self.header_type == HEAD_NONE {
            self.lines_to_next_header = -1;
            return;
        }
        let head_normal = self.format_list.iter().filter(|n| !n.name.is_empty() && n.pr.is_some()).count();
        if head_normal == 0 {
            self.lines_to_next_header = -1;
        }
    }

    pub(super) fn lists_and_needs(&mut self) {
        self.check_headers();
        if self.thread_flags & TF_SHOW_BOTH != 0 {
            let mut proc_list = Vec::new();
            let mut task_list = Vec::new();
            for node in &self.format_list {
                let mut pn = node.clone();
                let mut tn = node.clone();
                match node.flags & CF_PRINT_MASK {
                    CF_PRINT_THREAD_ONLY => pn.pr = Some(pr_nop),
                    CF_PRINT_PROCESS_ONLY => tn.pr = Some(pr_nop),
                    _ => {}
                }
                proc_list.push(pn);
                task_list.push(tn);
            }
            self.proc_format_list = proc_list;
            self.task_format_list = task_list;
        } else {
            self.proc_format_list = self.format_list.clone();
            self.task_format_list = self.format_list.clone();
        }
    }

    pub(super) fn init_output(&mut self) {
        self.seconds_since_1970 = ul_misc::util::time::now().sec;
        self.check_header_width();
    }

    pub(super) fn arg_check_conflicts(&mut self) -> R<()> {
        let quick = self.selection_list.iter().filter(|n| n.typecode == SEL_PID_QUICK).count();
        let len = self.selection_list.len();
        let fail = |m: &str| -> R<()> {
            io::eprint(format!("{m}\n"));
            Err(1)
        };
        if quick > 1 {
            return fail("q/-q/--quick-pid can only be used once.");
        }
        if quick > 0 && len > quick {
            return fail("q/-q/--quick-pid cannot be combined with other selection options.");
        }
        if quick > 0 && self.forest_type != 0 {
            return fail("q/-q/--quick-pid cannot be used together with forest type listings.");
        }
        if quick > 0 && !self.sort_list.is_empty() {
            return fail("q/-q,--quick-pid cannot be used together with sort options.");
        }
        if quick > 0 && self.negate_selection {
            return fail("q/-q/--quick-pid cannot be used together with negation switches.");
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Varredura.

    /// Lê os processos como `procps_pids_reap` (ou `select`, no modo `-q`) e fixa o `boot_tics`.
    fn fetch(&mut self, threads: bool, quick: Option<Vec<u32>>) -> Vec<Pt> {
        let ts = sys::try_current().and_then(|s| s.clock_gettime(Clock::Boottime).ok());
        self.boot_tics = match ts {
            Some(t) => ((t.sec as f64 + f64::from(t.nsec) * 1.0e-9) * self.hertz as f64) as u64,
            None => 0,
        };
        match quick {
            Some(pids) => select_pids(&pids, threads),
            None => reap(threads),
        }
    }

    /// `simple_spew`: lista sem ordenar, na ordem do /proc.
    fn simple_spew(&mut self) -> R<()> {
        let threads = self.thread_flags & (TF_LOOSE_TASKS | TF_SHOW_TASK) != 0;
        let quick = match self.selection_list.first() {
            Some(n) if n.typecode == SEL_PID_QUICK => Some(n.nums.iter().map(|v| *v as u32).collect::<Vec<u32>>()),
            _ => None,
        };
        let had_quick = quick.is_some();
        let mut all = self.fetch(threads, quick);
        if all.is_empty() && !had_quick {
            io::eprint("fatal library error, reap\n");
            return Err(1);
        }
        let proc_fmt = self.proc_format_list.clone();
        let task_fmt = self.task_format_list.clone();
        match self.thread_flags & (TF_SHOW_PROC | TF_LOOSE_TASKS | TF_SHOW_TASK) {
            x if x == TF_SHOW_PROC => {
                for p in &all {
                    if self.want_this_proc(p) {
                        self.show_one_proc(Some(p), &proc_fmt);
                    }
                }
            }
            x if x == TF_SHOW_TASK || x == TF_SHOW_PROC | TF_LOOSE_TASKS => {
                for p in &all {
                    if self.want_this_proc(p) {
                        self.show_one_proc(Some(p), &task_fmt);
                    }
                }
            }
            x if x == TF_SHOW_PROC | TF_SHOW_TASK => {
                // Primeiro por início, depois por tgid (a ordenação é estável).
                all.sort_by_key(|p| p.start_time);
                all.sort_by_key(|p| p.tgid);
                let total = all.len();
                let mut i = 0;
                while i < total {
                    // next_proc:
                    loop {
                        let buf = &all[i];
                        if self.want_this_proc(buf) {
                            let me = buf.tid;
                            self.show_one_proc(Some(buf), &proc_fmt);
                            let mut moved = false;
                            while i < total {
                                let t = &all[i];
                                if t.tgid != me {
                                    moved = true;
                                    break;
                                }
                                self.show_one_proc(Some(t), &task_fmt);
                                i += 1;
                            }
                            if moved {
                                continue;
                            }
                        }
                        break;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Valor de ordenação de `item` para `p` (o que `procps_pids_sort` compara).
    pub(crate) fn sort_key(&mut self, item: Item, p: &Pt) -> Key {
        let sig = |v: &Vec<u8>| Key::Str(v.clone());
        match item {
            Item::Noop => Key::None,
            Item::AddrCodeEnd => Key::UInt(p.end_code),
            Item::AddrCodeStart => Key::UInt(p.start_code),
            Item::AddrCurrEip => Key::UInt(p.kstk_eip),
            Item::AddrCurrEsp => Key::UInt(p.kstk_esp),
            Item::AddrStackStart => Key::UInt(p.start_stack),
            Item::AutogrpId => Key::Int(i64::from(p.autogroup().0)),
            Item::AutogrpNice => Key::Int(i64::from(p.autogroup().1)),
            Item::Cgname => Key::Str(p.cgname().to_vec()),
            Item::Cgroup => Key::Str(p.cgroup().to_vec()),
            Item::Cmd => Key::Str(p.cmd.clone()),
            Item::Cmdline => Key::Str(p.cmdline().to_vec()),
            Item::Exe => Key::Str(p.exe().to_vec()),
            Item::Flags => Key::UInt(p.flags),
            Item::FltMaj => Key::UInt(p.maj_flt),
            Item::FltMin => Key::UInt(p.min_flt),
            Item::IdEgid => Key::UInt(u64::from(p.egid)),
            Item::IdEgroup => Key::Str(self.group_name(p.egid)),
            Item::IdEuid => Key::UInt(u64::from(p.euid)),
            Item::IdEuser => Key::Str(self.user_name(p.euid)),
            Item::IdFgid => Key::UInt(u64::from(p.fgid)),
            Item::IdFgroup => Key::Str(self.group_name(p.fgid)),
            Item::IdFuid => Key::UInt(u64::from(p.fuid)),
            Item::IdFuser => Key::Str(self.user_name(p.fuid)),
            Item::IdLogin => Key::Int(i64::from(p.luid())),
            Item::IdPgrp => Key::Int(i64::from(p.pgrp)),
            Item::IdPid => Key::Int(i64::from(p.tid)),
            Item::IdPpid => Key::Int(i64::from(p.ppid)),
            Item::IdRgid => Key::UInt(u64::from(p.rgid)),
            Item::IdRgroup => Key::Str(self.group_name(p.rgid)),
            Item::IdRuid => Key::UInt(u64::from(p.ruid)),
            Item::IdRuser => Key::Str(self.user_name(p.ruid)),
            Item::IdSession => Key::Int(i64::from(p.session)),
            Item::IdSgid => Key::UInt(u64::from(p.sgid)),
            Item::IdSgroup => Key::Str(self.group_name(p.sgid)),
            Item::IdSuid => Key::UInt(u64::from(p.suid)),
            Item::IdSuser => Key::Str(self.user_name(p.suid)),
            Item::IdTgid => Key::Int(i64::from(p.tgid)),
            Item::IdTpgid => Key::Int(i64::from(p.tpgid)),
            Item::IoReadBytes => Key::UInt(p.io()[4]),
            Item::IoReadChars => Key::UInt(p.io()[0]),
            Item::IoReadOps => Key::UInt(p.io()[2]),
            Item::IoWriteBytes => Key::UInt(p.io()[5]),
            Item::IoWriteCbytes => Key::UInt(p.io()[6]),
            Item::IoWriteChars => Key::UInt(p.io()[1]),
            Item::IoWriteOps => Key::UInt(p.io()[3]),
            Item::Lxcname => Key::Str(p.lxcname().to_vec()),
            Item::MemResPgs => Key::UInt(p.statm().0),
            Item::MemShrPgs => Key::UInt(p.statm().1),
            Item::Nice => Key::Int(i64::from(p.nice)),
            Item::Nlwp => Key::Int(i64::from(p.nlwp)),
            Item::NsCgroup => Key::UInt(p.ns()[0]),
            Item::NsIpc => Key::UInt(p.ns()[1]),
            Item::NsMnt => Key::UInt(p.ns()[2]),
            Item::NsNet => Key::UInt(p.ns()[3]),
            Item::NsPid => Key::UInt(p.ns()[4]),
            Item::NsTime => Key::UInt(p.ns()[5]),
            Item::NsUser => Key::UInt(p.ns()[6]),
            Item::NsUts => Key::UInt(p.ns()[7]),
            Item::OomAdj => Key::Int(i64::from(p.oom().1)),
            Item::OomScore => Key::Int(i64::from(p.oom().0)),
            Item::Priority => Key::Int(i64::from(p.priority)),
            Item::PriorityRt => Key::Int(i64::from(p.rtprio)),
            Item::Processor => Key::Int(i64::from(p.processor)),
            Item::ProcessorNode => Key::Int(-1),
            Item::RssRlim => Key::UInt(p.rss_rlim),
            Item::SchedClass => Key::Int(i64::from(p.sched)),
            Item::SdMach | Item::SdOuid | Item::SdSeat | Item::SdSess | Item::SdSlice | Item::SdUnit | Item::SdUunit => {
                Key::Str(b"-".to_vec())
            }
            Item::Sigblocked => sig(&p.blocked),
            Item::Sigcatch => sig(&p.sigcatch),
            Item::Sigignore => sig(&p.sigignore),
            Item::Signals => sig(&p.signal),
            Item::Sigpending => sig(&p.sigpnd),
            Item::SmapPrvTotal => Key::UInt(p.smaps().1),
            Item::SmapPss => Key::UInt(p.smaps().0),
            Item::State => Key::Int(i64::from(p.state)),
            Item::Supgids => Key::Str(p.supgid.clone().unwrap_or_else(|| b"-".to_vec())),
            Item::Supgroups => Key::Str(p.supgid.clone().unwrap_or_else(|| b"-".to_vec())),
            Item::TicsAll => Key::UInt(p.utime.wrapping_add(p.stime)),
            Item::TicsBegan => Key::UInt(p.start_time),
            Item::TicsUser => Key::UInt(p.utime),
            Item::TicsUserC => Key::UInt(p.utime.wrapping_add(p.cutime)),
            Item::TimeAll => Key::Real((p.utime as f64 + p.stime as f64) / self.hertz as f64),
            Item::TimeElapsed => Key::Real(self.time_elapsed(p)),
            Item::TtyName => Key::Vers(proc::dev_to_tty(self, p.tty, p.tid, false)),
            Item::Utilization => Key::Real(self.utilization(p, false)),
            Item::UtilizationC => Key::Real(self.utilization(p, true)),
            Item::VmData => Key::UInt(p.vm_data),
            Item::VmExe => Key::UInt(p.vm_exe),
            Item::VmLib => Key::UInt(p.vm_lib),
            Item::VmRss => Key::UInt(p.vm_rss),
            Item::VmRssLocked => Key::UInt(p.vm_lock),
            Item::VmSize => Key::UInt(p.vm_size),
            Item::VmStack => Key::UInt(p.vm_stack),
            Item::VsizeBytes => Key::UInt(p.vsize),
            Item::WchanName => Key::Str(p.wchan_name().to_vec()),
        }
    }

    /// Ordena `idx` (índices de `all`) por `item`; ordem 0 não ordena (a biblioteca recusa).
    fn sort_by_item(&mut self, all: &[Pt], idx: &mut Vec<usize>, item: Item, order: i8) {
        if order == 0 || idx.len() < 2 {
            return;
        }
        let keys: Vec<(Key, usize)> = idx.iter().map(|&i| (self.sort_key(item, &all[i]), i)).collect();
        let mut keys = keys;
        keys.sort_by(|a, b| {
            let o = cmp_keys(&a.0, &b.0);
            if order < 0 { o.reverse() } else { o }
        });
        *idx = keys.into_iter().map(|(_, i)| i).collect();
    }

    /// `fancy_spew`: com ordenação ou floresta.
    fn fancy_spew(&mut self) -> R<()> {
        let threads = self.thread_flags & TF_LOOSE_TASKS != 0;
        let all = self.fetch(threads, None);
        if all.is_empty() {
            io::eprint("fatal library error, reap\n");
            return Err(1);
        }
        let mut idx: Vec<usize> = (0..all.len()).filter(|&i| self.want_this_proc(&all[i])).collect();
        if !idx.is_empty() {
            if self.forest_type != 0 {
                // prep_forest_sort: início primeiro, depois a ordenação do usuário (ou o ppid).
                let mut list = vec![SortNode { sr: Item::TicsBegan, order: 1 }];
                if self.sort_list.is_empty() {
                    list.push(SortNode { sr: Item::IdPpid, order: 1 });
                } else {
                    list.extend(self.sort_list.iter().cloned());
                }
                self.sort_list = list;
            }
            let sorts = self.sort_list.clone();
            for s in &sorts {
                self.sort_by_item(&all, &mut idx, s.sr, s.order);
            }
            let fmt = self.format_list.clone();
            if self.forest_type != 0 {
                self.show_forest(&all, &idx, &fmt);
            } else {
                for &i in &idx {
                    self.show_one_proc(Some(&all[i]), &fmt);
                }
            }
        }
        Ok(())
    }

    /// `show_tree`: o processo `me` (índice em `idx`) e, recursivamente, os filhos.
    fn show_tree(&mut self, all: &[Pt], idx: &[usize], fmt: &[FNode], me: usize, level: usize, have_sibling: bool) {
        let n = idx.len();
        if level > 0 {
            if self.forest_prefix.len() < level {
                self.forest_prefix.resize(level, b' ');
            }
            self.forest_prefix[level - 1] = if have_sibling { b'+' } else { b'L' };
        }
        self.forest_prefix.truncate(level);
        self.show_one_proc(Some(&all[idx[me]]), fmt);
        let self_pid = all[idx[me]].tid;
        let mut i = 0;
        loop {
            if i >= n {
                return;
            }
            if all[idx[i]].ppid == self_pid {
                break;
            }
            i += 1;
        }
        if level > 0 {
            if self.forest_prefix.len() < level {
                self.forest_prefix.resize(level, b' ');
            }
            self.forest_prefix[level - 1] = if have_sibling { b'|' } else { b' ' };
        }
        self.forest_prefix.truncate(level);
        loop {
            if i >= n {
                break;
            }
            let mut more_children = true;
            if i + 1 >= n || all[idx[i + 1]].ppid != self_pid {
                more_children = false;
            }
            let child = i;
            i += 1;
            if self_pid == 1 && self.forest_type != b'u' {
                self.show_tree(all, idx, fmt, child, level, more_children);
            } else {
                self.show_tree(all, idx, fmt, child, level + 1, more_children);
            }
            if !more_children {
                break;
            }
        }
        self.forest_prefix.truncate(level);
    }

    fn show_forest(&mut self, all: &[Pt], idx: &[usize], fmt: &[FNode]) {
        let n = idx.len();
        let mut i = n;
        while i > 0 {
            i -= 1;
            let root = !(0..n).any(|j| all[idx[j]].tid == all[idx[i]].ppid);
            if root {
                self.show_tree(all, idx, fmt, i, 0, false);
            }
        }
    }

    /// Fim da execução: se nada foi impresso, ainda pode sair o cabeçalho, e o status é 1.
    fn show_end(&mut self) -> R<()> {
        if self.did_stuff {
            return Ok(());
        }
        self.lines_to_next_header -= 1;
        if self.lines_to_next_header == 0 {
            self.lines_to_next_header = self.header_gap;
            let fmt = self.format_list.clone();
            self.show_one_proc(None, &fmt);
        }
        Err(1)
    }
}
