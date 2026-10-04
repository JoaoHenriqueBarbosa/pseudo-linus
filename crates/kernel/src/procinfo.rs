//! A fonte de dados do procfs: a tabela de processos do sandbox vista pelo `vfs::procfs`.

use std::sync::{Arc, OnceLock, Weak};
use std::sync::atomic::Ordering;

use sysabi::Pid;
use vfs::Caller;
use vfs::fs::Link;
use vfs::procfs::{FdLink, ProcData, ProcProvider};

use crate::fd::FileObj;
use crate::pipe::PipeObject;
use crate::proc::INIT_PID;
use crate::sandbox::SbInner;

pub(crate) struct SbProcProvider {
    pub sb: OnceLock<Weak<SbInner>>,
}

impl SbProcProvider {
    fn sb(&self) -> Option<Arc<SbInner>> {
        self.sb.get().and_then(Weak::upgrade)
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
        let sb = self.sb()?;
        let (proc, rel) = {
            let t = sb.table.lock();
            let e = t.map.get(&pid)?;
            (e.proc.clone(), e.rel.clone())
        };
        let zombie = rel.zombie.is_some();
        let state = if zombie {
            'Z'
        } else if pid == INIT_PID {
            'S'
        } else if proc.stopped.load(Ordering::Relaxed) {
            'T'
        } else {
            let th = proc.threads.lock();
            match th.live.get(&pid).or_else(|| th.live.values().next()) {
                Some(task) if task.blocked.load(Ordering::Relaxed) => 'S',
                _ => 'R',
            }
        };
        let st = proc.st.lock();
        Some(ProcData {
            pid,
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
            umask: st.umask,
            cwd: (!zombie).then(|| st.cwd.loc().clone()),
            root: (!zombie).then(|| st.root.loc().clone()),
            exe: if zombie { None } else { st.exe.as_ref().map(|e| e.loc().clone()) },
        })
    }

    fn fds(&self, pid: Pid) -> Option<Vec<i32>> {
        let sb = self.sb()?;
        let proc = sb.table.lock().proc(pid)?;
        let fds = proc.fds.lock().fds().into_iter().map(|f| f.0).collect();
        Some(fds)
    }

    fn fd(&self, cx: &Caller, pid: Pid, fd: i32) -> Option<FdLink> {
        let sb = self.sb()?;
        let proc = sb.table.lock().proc(pid)?;
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
        };
        Some(FdLink { text, target, perm })
    }
}
