//! Memória de um processo e da máquina vistas pelo `/proc`.
//!
//! Um pseudo-processo é uma thread do SO rodando código nativo: não tem mapa de memória, e o kernel não
//! tem como medir quanto um processo usa (o allocator rastreado do desenho, E07, ainda não está ligado).
//! Então a memória que o `/proc` mostra é um modelo, montado do que o kernel sabe de verdade: o perfil
//! da imagem em execução (medido no oráculo, no Debian 13, pra cada programa que o sandbox tem), o
//! tamanho do argv e do ambiente (a pilha cresce em páginas conforme eles, como no `execve` do Linux),
//! o número de threads vivas (cada thread acrescenta a pilha dela, de `RLIMIT_STACK`) e o pico
//! acumulado. O uso real de heap de um programa não entra.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use sysabi::{RLIM_INFINITY, Resource};
use vfs::procfs::{MemData, MemSystem};

use crate::proc::{INIT_PID, Proc};
use crate::sandbox::SbInner;

/// Memória total de um sandbox sem `mem_bytes`: a do oráculo (`MemTotal`), em kB.
const DEFAULT_MEM_TOTAL_KB: u64 = 32_735_068;
/// Bytes que o `execve` põe na pilha além das strings e dos ponteiros (auxv, plataforma, bytes
/// aleatórios).
const AUX_BYTES: usize = 512;
/// Pilha de kernel de cada thread, em kB (`THREAD_SIZE`).
const KERNEL_STACK_KB: u64 = 16;

/// Uma imagem de programa em kB, com a pilha inicial de 132 kB (128 de folga mais a página de argumentos).
struct Profile {
    names: &'static [&'static str],
    size: u64,
    rss_anon: u64,
    rss_file: u64,
    data: u64,
    exe: u64,
    lib: u64,
    pte: u64,
}

/// O que um processo parado numa leitura mostrava no oráculo. O primeiro é o `cat` do `status.txt`
/// dourado e vale pra tudo que não está na lista (os coreutils são parecidos entre si).
const PROFILES: &[Profile] = &[
    Profile { names: &[], size: 3280, rss_anon: 116, rss_file: 1652, data: 488, exe: 24, lib: 1588, pte: 48 },
    Profile { names: &["bash"], size: 5116, rss_anon: 852, rss_file: 3104, data: 928, exe: 804, lib: 1668, pte: 48 },
    Profile { names: &["sh", "dash"], size: 2672, rss_anon: 104, rss_file: 1576, data: 232, exe: 80, lib: 1588, pte: 48 },
    Profile { names: &["grep", "egrep", "fgrep"], size: 3932, rss_anon: 152, rss_file: 1944, data: 292, exe: 144, lib: 2080, pte: 44 },
    Profile { names: &["sed"], size: 4048, rss_anon: 148, rss_file: 1988, data: 248, exe: 84, lib: 2228, pte: 52 },
    Profile { names: &["awk", "gawk"], size: 6564, rss_anon: 344, rss_file: 3344, data: 352, exe: 532, lib: 3352, pte: 48 },
    Profile { names: &["jq"], size: 5632, rss_anon: 872, rss_file: 2648, data: 932, exe: 12, lib: 2508, pte: 48 },
    Profile { names: &["git"], size: 7896, rss_anon: 240, rss_file: 3440, data: 460, exe: 2876, lib: 2160, pte: 48 },
    Profile { names: &["sqlite3"], size: 6072, rss_anon: 244, rss_file: 3516, data: 292, exe: 204, lib: 3524, pte: 52 },
    Profile { names: &["perl"], size: 8160, rss_anon: 436, rss_file: 4452, data: 424, exe: 1720, lib: 2176, pte: 56 },
    Profile { names: &["python3"], size: 14428, rss_anon: 2752, rss_file: 5516, data: 4372, exe: 3200, lib: 2288, pte: 64 },
];

fn profile(comm: &[u8]) -> &'static Profile {
    PROFILES.iter().find(|p| p.names.iter().any(|n| n.as_bytes() == comm)).unwrap_or(&PROFILES[0])
}

/// A memória do processo agora. Atualiza o pico guardado no processo (`VmPeak`, `VmHWM`).
pub(crate) fn snapshot(proc: &Proc) -> MemData {
    let (comm, argc, envc, strings, stack_rlim) = {
        let st = proc.st.lock();
        let strings: usize = st.argv.iter().chain(st.env.iter()).map(|s| s.len() + 1).sum();
        (st.comm.clone(), st.argv.len(), st.env.len(), strings, st.rlimits[Resource::Stack as usize].cur)
    };
    let nthreads = proc.nthreads().max(1) as u64;
    let prof = profile(&comm);
    // `execve`: a pilha tem as strings, o argv, o envp e o auxv, em páginas, sobre os 128 kB de folga.
    let pages = (strings + 8 * (argc + envc + 2) + AUX_BYTES).div_ceil(4096).max(1) as u64;
    let vm_stk = 128 + 4 * pages;
    // Pilha de cada thread que o glibc cria: `RLIMIT_STACK` (2 MiB se ilimitado) mais a página de guarda.
    let thread_stack = if stack_rlim == RLIM_INFINITY { 2048 } else { (stack_rlim / 1024).clamp(16, 1 << 20) };
    let extra = nthreads - 1;
    let vm_size = prof.size - 132 + vm_stk + extra * (thread_stack + 4);
    let rss_anon = prof.rss_anon + 4 * (pages - 1) + 8 * extra;
    let rss_file = prof.rss_file;
    let vm_rss = rss_anon + rss_file;
    let vm_peak = proc.peak_size_kb.fetch_max(vm_size, Ordering::Relaxed).max(vm_size);
    let vm_hwm = proc.peak_rss_kb.fetch_max(vm_rss, Ordering::Relaxed).max(vm_rss);
    MemData {
        vm_peak,
        vm_size,
        vm_lck: 0,
        vm_pin: 0,
        vm_hwm,
        vm_rss,
        rss_anon,
        rss_file,
        rss_shmem: 0,
        vm_data: prof.data + extra * thread_stack,
        vm_stk,
        vm_exe: prof.exe,
        vm_lib: prof.lib,
        vm_pte: prof.pte + 4 * extra,
        vm_swap: 0,
    }
}

/// A memória do sandbox: a soma dos processos vivos, mais o conteúdo dos tmpfs.
pub(crate) fn system(sb: &SbInner) -> MemSystem {
    let procs: Vec<std::sync::Arc<Proc>> = {
        let t = sb.table.lock();
        t.map.values().filter(|e| e.rel.zombie.is_none() && e.proc.pid != INIT_PID).map(|e| e.proc.clone()).collect()
    };
    let mut m = MemSystem {
        total: sb.cfg.limits.mem_bytes.map_or(DEFAULT_MEM_TOTAL_KB, |b| b / 1024),
        ..MemSystem::default()
    };
    // Páginas de arquivo de uma mesma imagem são o mesmo cache: conta o maior de cada programa.
    let mut mapped: HashMap<Vec<u8>, u64> = HashMap::new();
    for p in procs {
        let d = snapshot(&p);
        m.anon += d.rss_anon;
        m.committed += d.vm_size;
        m.page_tables += d.vm_pte;
        m.kernel_stack += KERNEL_STACK_KB * p.nthreads().max(1) as u64;
        let comm = p.st.lock().comm.clone();
        let e = mapped.entry(comm).or_insert(0);
        *e = (*e).max(d.rss_file);
    }
    m.mapped = mapped.values().sum();
    let (rb, _) = sb.rootfs.usage();
    let (db, _) = sb.devfs.usage();
    m.shmem = (rb + db).div_ceil(1024);
    m
}
