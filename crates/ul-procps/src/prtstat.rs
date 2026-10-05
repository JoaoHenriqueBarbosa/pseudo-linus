//! `prtstat` do psmisc 23.7: imprime os campos de `/proc/<pid>/stat` de forma legível (`-r` imprime
//! a linha crua).
//!
//! Diferença conhecida: o layout foi reproduzido de memória do `prtstat.c`, sem conferência de
//! oráculo; os campos de convidado e `blkio` saem zerados porque o procfs do sandbox não os expõe.

use std::ffi::OsString;

use sysabi::{Ctx, sys};
use ul_misc::util::io;

use crate::common::out;
use crate::procfs;

const USAGE: &str = "Usage: prtstat [options] PID ...\n       prtstat -V\nPrint information about a process\n    -r,--raw       Raw display of information\n    -V,--version   Display version information and exit\n";
const VERSION: &str = "prtstat (PSmisc) 23.7\nCopyright (C) 2009-2024 Craig Small\n\nPSmisc comes with ABSOLUTELY NO WARRANTY.\nThis is free software, and you are welcome to redistribute it under\nthe terms of the GNU General Public License.\nFor more information about these matters, see the files named COPYING.\n";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn state_name(c: char) -> &'static str {
    match c {
        'R' => "running",
        'S' => "sleeping",
        'D' => "disk sleep",
        'T' => "stopped",
        't' => "tracing stop",
        'Z' => "zombie",
        'X' | 'x' => "dead",
        'W' => "paging",
        _ => "unknown",
    }
}

fn policy_name(p: u32) -> &'static str {
    match p {
        0 => "SCHED_OTHER",
        1 => "SCHED_FIFO",
        2 => "SCHED_RR",
        3 => "SCHED_BATCH",
        5 => "SCHED_IDLE",
        6 => "SCHED_DEADLINE",
        _ => "unknown",
    }
}

fn size(v: u64) -> String {
    const U: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let mut x = v as f64;
    let mut i = 0;
    while x >= 1024.0 && i < U.len() - 1 {
        x /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{v} B") } else { format!("{:.0} {}", x, U[i]) }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let mut raw = false;
    let mut pids: Vec<Vec<u8>> = Vec::new();
    let mut only_operands = false;
    for a in &argv[1.min(argv.len())..] {
        if !only_operands && a.first() == Some(&b'-') && a.len() > 1 {
            match a.as_slice() {
                b"-r" | b"--raw" => raw = true,
                b"-V" | b"--version" => {
                    io::eprint(VERSION);
                    return 0;
                }
                b"--" => only_operands = true,
                _ => {
                    io::eprint(USAGE);
                    return 1;
                }
            }
        } else {
            pids.push(a.clone());
        }
    }
    if pids.is_empty() {
        io::eprint("You must provide at least one PID.\n");
        io::eprint(USAGE);
        return 1;
    }
    let mut rc = 0;
    for p in pids {
        let digits: Vec<u8> = p.iter().copied().take_while(u8::is_ascii_digit).collect();
        let pid: i64 = String::from_utf8_lossy(&digits).parse().unwrap_or(0);
        let path = format!("/proc/{pid}/stat");
        let data = if pid > 0 { procfs::read(&path) } else { None };
        let Some(data) = data else {
            io::eprint(format!("Process with pid {pid} does not exist.\n"));
            continue;
        };
        if raw {
            out(data);
            continue;
        }
        let Some(st) = procfs::parse_stat(&data) else {
            io::eprint(format!("prtstat: cannot parse {path}\n"));
            rc = 1;
            continue;
        };
        let hz = procfs::HZ as f64;
        let mut s = Vec::new();
        s.extend_from_slice(b"Process: ");
        let mut comm = st.comm.clone();
        comm.resize(comm.len().max(14), b' ');
        s.extend_from_slice(&comm);
        s.extend_from_slice(format!("\t\tState: {} ({})\n", st.state, state_name(st.state)).as_bytes());
        let t = format!(
            "  CPU#:  {:<3}\t\tTTY: {}\tThreads: {}\nProcess, Group and Session IDs\n  Process ID: {}\t\t  Parent ID: {}\n    Group ID: {}\t\t Session ID: {}\n  T Group ID: {}\n\nPage Faults\n  This Process    (minor major): {:>8}  {:>8}\n  Child Processes (minor major): {:>8}  {:>8}\nCPU Times\n  This Process    (user system guest blkio): {:6.2} {:6.2} {:6.2} {:6.2}\n  Child processes (user system guest):       {:6.2} {:6.2} {:6.2}\nMemory\n  Vsize:       {:<10}\n  RSS:         {:<10} \t\t\tRSS Limit: {}\n  Code Start:  {:<#10x}\t\tCode Stop:  {:<#10x}\n  Stack Start: {:<#10x}\n  Stack Pointer (ESP): {:>#10x}\t Inst Pointer (EIP): {:>#10x}\nScheduling\n  Policy: {}\n  Nice:   {} \t\t\t RT Priority: {} {}\nSignals\n  Pending:    {:016x}\t\t Blocked:    {:016x}\n  Ignored:    {:016x}\t\t Caught:     {:016x}\n  Wake Chan:  {:#x}\n",
            st.processor,
            st.tty_nr,
            st.num_threads,
            st.pid,
            st.ppid,
            st.pgrp,
            st.session,
            st.tpgid,
            st.minflt,
            st.majflt,
            st.cminflt,
            st.cmajflt,
            st.utime as f64 / hz,
            st.stime as f64 / hz,
            0.0,
            0.0,
            st.cutime as f64 / hz,
            st.cstime as f64 / hz,
            0.0,
            size(st.vsize),
            size((st.rss.max(0) as u64) * procfs::PAGE_KB * 1024),
            if st.rsslim == u64::MAX { "unlimited".to_string() } else { size(st.rsslim) },
            st.startcode,
            st.endcode,
            st.startstack,
            st.kstkesp,
            st.kstkeip,
            policy_name(st.policy),
            st.nice,
            st.rt_priority,
            "",
            st.signal,
            st.blocked,
            st.sigignore,
            st.sigcatch,
            st.wchan
        );
        s.extend_from_slice(t.as_bytes());
        out(s);
    }
    let _ = sys::current();
    rc
}
