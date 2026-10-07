//! A fonte de dados do procfs: a tabela de processos do sandbox vista pelo `vfs::procfs`.

use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock, Weak};

use sysabi::{OFlags, Pid, WaitStatus};
use vfs::Caller;
use vfs::fs::{FileSystem, Link};
use vfs::procfs::{CpuTimes, FdInfo, FdLink, MemSystem, ProcData, ProcProvider, SigMasks, SysData};

use crate::fd::{FileObj, Ofd};
use crate::pipe::PipeObject;
use crate::proc::{INIT_PID, PID_MAX, Proc};
use crate::sandbox::SbInner;
use crate::sys::{UNAME_RELEASE, UNAME_VERSION};

/// `mnt_id` do pipefs, a montagem interna dos pipes (o valor que o oráculo mostra).
const PIPEFS_MNT_ID: u32 = 16;
/// `mnt_id` do sockfs (o valor que o oráculo mostra).
const SOCKFS_MNT_ID: u32 = 10;
/// `O_LARGEFILE` do x86_64: o kernel o põe em todo `f_flags` de arquivo aberto por 64 bits.
const O_LARGEFILE: u32 = 0o100000;

pub(crate) struct SbProcProvider {
    pub sb: OnceLock<Weak<SbInner>>,
}

impl SbProcProvider {
    fn sb(&self) -> Option<Arc<SbInner>> {
        self.sb.get().and_then(Weak::upgrade)
    }

    /// Os dados de uma thread (`whole == false`) ou do processo inteiro (`whole == true`, com `tid` igual
    /// ao pid). `None` se o processo não existe ou a thread não é dele.
    fn build(&self, pid: Pid, tid: Pid, whole: bool) -> Option<ProcData> {
        let sb = self.sb()?;
        let (proc, rel) = {
            let t = sb.table.lock();
            let e = t.map.get(&pid)?;
            (e.proc.clone(), e.rel.clone())
        };
        let zombie = rel.zombie.is_some();
        // A thread descrita: a pedida, ou (no init e num zumbi, que não têm thread viva) nenhuma.
        let (task, any_task) = {
            let th = proc.threads.lock();
            (th.live.get(&tid).cloned(), th.live.get(&pid).or_else(|| th.live.values().next()).cloned())
        };
        if !whole && task.is_none() && !(tid == pid && (zombie || pid == INIT_PID)) {
            return None;
        }
        let state = if zombie {
            'Z'
        } else if pid == INIT_PID {
            'S'
        } else if proc.stopped.load(Ordering::Relaxed) {
            'T'
        } else {
            match task.as_ref().or(any_task.as_ref()) {
                Some(t) if t.blocked.load(Ordering::Relaxed) => 'S',
                _ => 'R',
            }
        };
        // Tempo de CPU: do processo todo, ou só da thread. O sandbox não separa tempo de sistema.
        let cpu_ns = if whole { proc.cpu_ns() } else { task.as_ref().map_or(0, |t| t.cpu_ns()) };
        let shown = task.as_ref().or(any_task.as_ref());
        let (sched_runtime_ns, sched_switches, last_cpu) = match shown {
            Some(t) => sb.kernel.cpus.task_stats(&t.ct),
            None => (proc.cpu_ns(), 0, 0),
        };
        let (voluntary_ctxt, nonvoluntary_ctxt) = match shown {
            Some(t) => (t.ct.nvcsw.load(Ordering::Relaxed), t.ct.nivcsw.load(Ordering::Relaxed)),
            None => (0, 0),
        };
        let (pending, ignored, caught) = proc.sig.lock().masks();
        let fdsize = if zombie { 0 } else { proc.fds.lock().fdsize() };
        let mem = (!zombie).then(|| crate::procmem::snapshot(&proc));
        let num_threads = proc.nthreads().max(1) as u32;
        let (exit_code, signaled) = match rel.zombie.as_ref().map(|z| z.0) {
            Some(WaitStatus::Exited(c)) => ((c & 0xff) << 8, false),
            Some(WaitStatus::Signaled { signal, core_dumped }) => (signal.0 | if core_dumped { 0x80 } else { 0 }, true),
            _ => (0, false),
        };
        let st = proc.st.lock();
        let tune = proc.tune.lock().clone();
        let mut rlimits = [(0u64, 0u64); 16];
        for (slot, l) in rlimits.iter_mut().zip(st.rlimits.iter()) {
            *slot = (l.cur, l.max);
        }
        Some(ProcData {
            pid,
            tid,
            ppid: rel.ppid,
            pgid: rel.pgid,
            sid: rel.sid,
            state,
            comm: st.comm.clone(),
            // Zumbi não tem mais memória: cmdline e environ vazios, sem cwd nem exe.
            cmdline: if zombie { Vec::new() } else { nul_join(&st.argv) },
            environ: if zombie { Vec::new() } else { nul_join(&st.env) },
            uid: st.cred.uid,
            gid: st.cred.gid,
            ruid: st.cred.ruid,
            suid: st.cred.suid,
            rgid: st.cred.rgid,
            sgid: st.cred.sgid,
            groups: st.cred.groups.clone(),
            umask: st.umask,
            cwd: (!zombie).then(|| st.cwd.loc().clone()),
            root: (!zombie).then(|| st.root.loc().clone()),
            exe: if zombie { None } else { st.exe.as_ref().map(|e| e.loc().clone()) },
            nice: st.nice,
            num_threads,
            utime_ns: cpu_ns,
            stime_ns: 0,
            cutime_ns: u64::try_from(rel.children_rusage.utime.as_nanos()).unwrap_or(u64::MAX),
            cstime_ns: u64::try_from(rel.children_rusage.stime.as_nanos()).unwrap_or(u64::MAX),
            start_ns: proc.start_ns,
            mem,
            rlimits,
            // O sandbox só tem sinais dirigidos ao processo (`kill`), então tudo pendente é `ShdPnd`; e
            // não há `sigprocmask`, então nada é bloqueado.
            sig: SigMasks { pending: 0, shared_pending: pending, blocked: 0, ignored, caught },
            sigq: u64::from(pending.count_ones()),
            fdsize,
            voluntary_ctxt,
            nonvoluntary_ctxt,
            last_cpu: last_cpu as u32,
            ncpus: sb.kernel.cpus.ncpus() as u32,
            policy: tune.sched.policy as u32,
            rt_priority: tune.sched.rt_priority,
            cpus_allowed: tune.cpus.clone(),
            fork_noexec: proc.fork_noexec.load(Ordering::Relaxed),
            exit_code,
            signaled,
            secondary: tid != pid,
            sched_runtime_ns,
            sched_switches,
        })
    }

    fn proc_of(&self, pid: Pid) -> Option<(Arc<SbInner>, Arc<Proc>)> {
        let sb = self.sb()?;
        let proc = sb.table.lock().proc(pid)?;
        Some((sb, proc))
    }
}

fn nul_join(v: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    for a in v {
        out.extend_from_slice(a);
        out.push(0);
    }
    out
}

impl ProcProvider for SbProcProvider {
    fn pids(&self) -> Vec<Pid> {
        let Some(sb) = self.sb() else { return Vec::new() };
        sb.table.lock().map.keys().copied().collect()
    }

    fn process(&self, pid: Pid) -> Option<ProcData> {
        self.build(pid, pid, true)
    }

    fn owner(&self, pid: Pid) -> Option<(sysabi::Uid, sysabi::Gid)> {
        let (_, proc) = self.proc_of(pid)?;
        let st = proc.st.lock();
        Some((st.cred.uid, st.cred.gid))
    }

    fn tids(&self, pid: Pid) -> Option<Vec<Pid>> {
        let (_, proc) = self.proc_of(pid)?;
        let mut v: Vec<Pid> = proc.threads.lock().live.keys().copied().collect();
        // A thread principal primeiro, as outras em ordem crescente (a ordem de criação).
        if let Some(i) = v.iter().position(|t| *t == pid) {
            let main = v.remove(i);
            v.insert(0, main);
        }
        if v.is_empty() {
            // Init e zumbi não têm thread viva, mas o Linux ainda mostra `task/<pid>`.
            v.push(pid);
        }
        Some(v)
    }

    fn thread(&self, pid: Pid, tid: Pid) -> Option<ProcData> {
        self.build(pid, tid, false)
    }

    fn children(&self, pid: Pid, tid: Pid) -> Option<Vec<Pid>> {
        let sb = self.sb()?;
        let t = sb.table.lock();
        let rel = t.rel(pid)?;
        // Os filhos são do processo; ficam na thread principal.
        if tid == pid { Some(rel.children.iter().copied().collect()) } else { Some(Vec::new()) }
    }

    fn fds(&self, pid: Pid) -> Option<Vec<i32>> {
        let (_, proc) = self.proc_of(pid)?;
        let fds = proc.fds.lock().fds().into_iter().map(|f| f.0).collect();
        Some(fds)
    }

    fn fd(&self, cx: &Caller, pid: Pid, fd: i32) -> Option<FdLink> {
        let (sb, proc) = self.proc_of(pid)?;
        let ofd = proc.fds.lock().ofd(sysabi::Fd(fd)).ok()?;
        let perm = ofd.link_perm();
        let (text, target) = match &ofd.obj {
            FileObj::Vfs { loc, .. } | FileObj::Path { loc, .. } => (sb.ns.fd_path(cx, loc), Link::Jump(loc.clone())),
            FileObj::Pipe { fifo: Some(loc), .. } => (sb.ns.fd_path(cx, loc), Link::Jump(loc.clone())),
            FileObj::Pipe { end, fifo: None } => (
                format!("pipe:[{}]", end.pipe.ino).into_bytes(),
                Link::Object(Arc::new(PipeObject { pipe: end.pipe.clone(), fifo_stat: None })),
            ),
            FileObj::Dev { loc: Some(loc), .. } => (sb.ns.fd_path(cx, loc), Link::Jump(loc.clone())),
            FileObj::Dev { loc: None, .. } => (b"/dev/null".to_vec(), Link::Path(b"/dev/null".to_vec())),
            FileObj::Listener(l) => {
                let text = format!("socket:[{}]", l.ident.ino).into_bytes();
                (text.clone(), Link::Path(text))
            }
            FileObj::Stream(c) => {
                let text = format!("socket:[{}]", c.ident.ino).into_bytes();
                (text.clone(), Link::Path(text))
            }
            FileObj::Unix(u) => {
                let text = format!("socket:[{}]", u.ident.ino).into_bytes();
                (text.clone(), Link::Path(text))
            }
        };
        Some(FdLink { text, target, perm })
    }

    fn fdinfo(&self, _cx: &Caller, pid: Pid, fd: i32) -> Option<FdInfo> {
        let (sb, proc) = self.proc_of(pid)?;
        let (ofd, cloexec) = {
            let fds = proc.fds.lock();
            let slot = fds.get(sysabi::Fd(fd)).ok()?;
            (slot.ofd.clone(), slot.cloexec)
        };
        Some(describe_fd(&sb, &ofd, cloexec))
    }

    fn system(&self) -> SysData {
        let Some(sb) = self.sb() else { return SysData::default() };
        let uptime_ns = sb.mono_ns();
        let btime = sb.now().sec - (uptime_ns / 1_000_000_000) as i64;
        let cpu = sb
            .cpu_acct
            .times()
            .into_iter()
            .map(|(user_ns, nice_ns)| CpuTimes { user_ns, nice_ns, system_ns: 0 })
            .collect();
        let (procs_running, nr_threads) = crate::loadavg::count_tasks(&sb);
        let (forks, last_pid) = {
            let t = sb.table.lock();
            (t.forks, t.last_pid())
        };
        SysData {
            ncpus: sb.kernel.cpus.ncpus() as u32,
            uptime_ns,
            btime,
            cpu,
            ctxt: sb.cpu_acct.switches(),
            forks,
            procs_running,
            procs_blocked: 0,
            nr_threads,
            last_pid,
            load: sb.load.get(),
        }
    }

    fn mem(&self) -> MemSystem {
        match self.sb() {
            Some(sb) => crate::procmem::system(&sb),
            None => MemSystem::default(),
        }
    }

    fn ncpus(&self) -> u32 {
        self.sb().map_or(1, |sb| sb.kernel.cpus.ncpus() as u32)
    }

    fn tcp_socks(&self) -> Vec<vfs::procfs::TcpSock> {
        self.sb().map_or_else(Vec::new, |sb| sb.ports.tcp_socks())
    }

    fn unix_socks(&self) -> Vec<vfs::procfs::UnixSockRow> {
        self.sb().map_or_else(Vec::new, |sb| sb.unix.rows())
    }

    fn version(&self) -> Vec<u8> {
        let mut v = b"Linux version ".to_vec();
        v.extend_from_slice(UNAME_RELEASE);
        v.extend_from_slice(
            b" (debian-kernel@lists.debian.org) (x86_64-linux-gnu-gcc-14 (Debian 14.2.0-19) 14.2.0, GNU ld (GNU Binutils for Debian) 2.44) ",
        );
        v.extend_from_slice(UNAME_VERSION);
        v.push(b'\n');
        v
    }

    fn pid_max(&self) -> u32 {
        PID_MAX as u32
    }

    fn os_release(&self) -> Vec<u8> {
        UNAME_RELEASE.to_vec()
    }

    fn kernel_version(&self) -> Vec<u8> {
        UNAME_VERSION.to_vec()
    }

    fn hostname(&self) -> Vec<u8> {
        self.sb().map_or_else(|| b"localhost".to_vec(), |sb| sb.hostname.lock().clone())
    }

    fn domainname(&self) -> Vec<u8> {
        self.sb().map_or_else(|| b"(none)".to_vec(), |sb| sb.domainname.lock().clone())
    }

    fn set_hostname(&self, name: &[u8]) -> Result<(), sysabi::Errno> {
        let sb = self.sb().ok_or(sysabi::Errno::ESRCH)?;
        *sb.hostname.lock() = name.to_vec();
        Ok(())
    }

    fn set_domainname(&self, name: &[u8]) -> Result<(), sysabi::Errno> {
        let sb = self.sb().ok_or(sysabi::Errno::ESRCH)?;
        *sb.domainname.lock() = name.to_vec();
        Ok(())
    }
}

/// `pos`, `flags`, `mnt_id` e `ino` de uma descrição de arquivo aberto (`proc_fdinfo` + `seq_show`).
fn describe_fd(sb: &SbInner, ofd: &Ofd, cloexec: bool) -> FdInfo {
    let (pos, status) = {
        let st = ofd.st.lock();
        (if matches!(&ofd.obj, FileObj::Vfs { kind: sysabi::FileType::Directory, .. }) { st.dir_cookie } else { st.pos }, st.status)
    };
    // `f_flags`: modo de acesso, flags de status, as que só o `open` guarda, e `O_LARGEFILE` nos arquivos
    // do VFS e dispositivos (pipes e `O_PATH` não têm).
    let mut flags = ofd.accmode | status.bits() | ofd.open_extra;
    let (mnt_id, ino) = match &ofd.obj {
        FileObj::Vfs { loc, .. } => {
            flags |= O_LARGEFILE;
            (loc.mnt.id, loc.ino)
        }
        FileObj::Path { loc, .. } => (loc.mnt.id, loc.ino),
        FileObj::Pipe { fifo: Some(loc), .. } => (loc.mnt.id, loc.ino),
        FileObj::Pipe { end, fifo: None } => (PIPEFS_MNT_ID, end.pipe.ino),
        FileObj::Listener(l) => (SOCKFS_MNT_ID, l.ident.ino),
        FileObj::Stream(c) => (SOCKFS_MNT_ID, c.ident.ino),
        FileObj::Unix(u) => (SOCKFS_MNT_ID, u.ident.ino),
        FileObj::Dev { loc: Some(loc), .. } => {
            flags |= O_LARGEFILE;
            (loc.mnt.id, loc.ino)
        }
        FileObj::Dev { loc: None, .. } => {
            flags |= O_LARGEFILE;
            (sb.ns.mounts().iter().find(|m| m.fs.dev() == sb.devfs.dev()).map_or(0, |m| m.id), 0)
        }
    };
    if cloexec {
        flags |= OFlags::CLOEXEC.bits();
    }
    FdInfo { pos, flags, mnt_id, ino }
}
