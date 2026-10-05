//! `/proc/<pid>/maps` e `/proc/<pid>/smaps`, e os endereços do espaço de endereçamento que o `stat`
//! mostra (`startcode`, `startstack`, `arg_start`...).
//!
//! Um pseudo-processo não tem memória virtual: o mapa é um modelo estável e plausível do que o Linux
//! 6.12 mostra de um programa dinâmico do Debian 13 (PIE): os cinco segmentos do binário, o `[heap]`,
//! a pilha de cada thread extra, a libc e o `ld-linux` no topo da região de `mmap`, o `[stack]`, o
//! `[vvar]` e o `[vdso]`. Os tamanhos saem de [`MemData`] (o perfil da imagem medido no oráculo), então
//! o mapa casa com o `status` e o `statm`; as bases saem de um gerador determinístico semeado pelo pid
//! e pelo instante de criação, como uma ASLR que não muda entre leituras do mesmo processo.
//!
//! Formatos de `fs/proc/task_mmu.c`: `show_vma_header_prefix` (endereços com no mínimo 8 dígitos hexa,
//! deslocamento com 8, `maj:min` com 2 cada, inode em decimal e um espaço) e o `seq_pad` até a coluna
//! 73 antes do nome; o `smaps` com as linhas do `show_smap` e do `__show_smap` na ordem do 6.12.

use std::io::Write as _;

use sysabi::Resource;

use super::data::*;

const PAGE: u64 = 4096;
/// Largura do cabeçalho antes do `seq_pad` (`25 + sizeof(void *) * 6 - 1`).
const NAME_COLUMN: usize = 72;

/// Caminho da libc e do carregador no Debian 13 (o `maps` mostra o caminho canônico, sem o `/lib`).
pub(super) const LIBC_PATH: &[u8] = b"/usr/lib/x86_64-linux-gnu/libc.so.6";
pub(super) const LD_PATH: &[u8] = b"/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2";

/// Segmentos do binário além do texto: cabeçalho, dados só de leitura, RELRO e dados graváveis.
const EXE_HDR: u64 = 0x2000;
const EXE_RODATA: u64 = 0x5000;
const EXE_RELRO: u64 = 0x1000;
const EXE_RW: u64 = 0x1000;
/// Segmentos da libc 2.41 sem o texto (que acompanha o `VmLib` do perfil) e o `.bss` anônimo.
const LIBC_HDR: u64 = 0x28000;
const LIBC_RODATA: u64 = 0x56000;
const LIBC_RELRO: u64 = 0x4000;
const LIBC_RW: u64 = 0x2000;
const LIBC_BSS: u64 = 0xd000;
/// Segmentos do `ld-linux-x86-64.so.2` e a área anônima logo abaixo dele (TLS e `dtv`).
const LD_SEGS: [u64; 5] = [0x1000, 0x26000, 0xa000, 0x2000, 0x2000];
const LD_ANON: u64 = 0x2000;
const VVAR: u64 = 0x4000;
const VDSO: u64 = 0x2000;
/// Bytes que o `create_elf_tables` põe abaixo das strings: o auxv (22 pares) e a folga de alinhamento.
const AUXV_BYTES: u64 = 22 * 16;

/// `VmFlags` (`show_smap_vma_flags`) de cada tipo de mapeamento.
const FLAGS_RO: &str = "rd mr mw me sd";
const FLAGS_RX: &str = "rd ex mr mw me sd";
const FLAGS_RELRO: &str = "rd mr mw me ac sd";
const FLAGS_RW: &str = "rd wr mr mw me ac sd";
const FLAGS_GUARD: &str = "mr mw me sd";
const FLAGS_STACK: &str = "rd wr mr mw me gd ac";
const FLAGS_VVAR: &str = "rd mr pf io de dd sd";
const FLAGS_VDSO: &str = "rd ex mr mw me de sd";

/// Identidade de um arquivo mapeado: o caminho que a linha mostra, o dispositivo e o inode.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MappedFile {
    pub path: Vec<u8>,
    pub dev: (u32, u32),
    pub ino: u64,
}

/// Os três arquivos que o mapa usa.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapFiles {
    pub exe: MappedFile,
    pub libc: MappedFile,
    pub ld: MappedFile,
}

/// Uma `vm_area_struct`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Vma {
    pub start: u64,
    pub end: u64,
    pub perms: &'static [u8; 4],
    /// Deslocamento no arquivo, em bytes.
    pub offset: u64,
    pub dev: (u32, u32),
    pub ino: u64,
    /// Caminho, `[heap]`, `[stack]`... ou vazio num mapeamento anônimo.
    pub name: Vec<u8>,
    pub flags: &'static str,
    /// Residente e sujo, em kB.
    pub rss_kb: u64,
    pub dirty_kb: u64,
    /// Páginas anônimas (o resto é cache de arquivo).
    pub anon: bool,
    /// Páginas que outros processos também mapeiam (bibliotecas e `vdso`).
    pub shared: bool,
}

impl Vma {
    fn size_kb(&self) -> u64 {
        (self.end - self.start) / 1024
    }
}

/// Os endereços do `mm_struct` que o `/proc/<pid>/stat` mostra.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct MmAddrs {
    pub start_code: u64,
    pub end_code: u64,
    pub start_stack: u64,
    pub start_data: u64,
    pub end_data: u64,
    pub start_brk: u64,
    pub arg_start: u64,
    pub arg_end: u64,
    pub env_start: u64,
    pub env_end: u64,
}

/// O espaço de endereçamento de um processo: as áreas em ordem crescente e os endereços do `mm`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Layout {
    pub vmas: Vec<Vma>,
    pub addrs: MmAddrs,
}

/// O splitmix64: a mesma semente dá sempre o mesmo mapa.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

/// kB arredondados pra cima em páginas, em bytes, no mínimo uma página.
fn pages(kb: u64) -> u64 {
    kb.div_ceil(4).max(1) * PAGE
}

/// kB arredondados pra baixo em múltiplos de uma página.
fn page_floor_kb(kb: u64) -> u64 {
    kb / 4 * 4
}

/// Pilha de cada thread que o glibc cria: `RLIMIT_STACK` (2 MiB se ilimitado), em kB.
fn thread_stack_kb(p: &ProcData) -> u64 {
    let cur = p.rlimits[Resource::Stack as usize].0;
    if cur == u64::MAX { 2048 } else { (cur / 1024).clamp(16, 1 << 20) }
}

/// Um inode estável pra um arquivo que o sandbox não tem (FNV-1a do caminho).
pub(super) fn synthetic_ino(path: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    (h & 0xff_ffff) | 0x10_0000
}

fn count_nul(v: &[u8]) -> u64 {
    v.iter().filter(|b| **b == 0).count() as u64
}

#[allow(clippy::too_many_arguments)]
fn file_vma(
    start: u64,
    len: u64,
    perms: &'static [u8; 4],
    offset: u64,
    f: &MappedFile,
    flags: &'static str,
    rss_kb: u64,
    dirty_kb: u64,
    shared: bool,
) -> Vma {
    Vma {
        start,
        end: start + len,
        perms,
        offset,
        dev: f.dev,
        ino: f.ino,
        name: f.path.clone(),
        flags,
        rss_kb: rss_kb.min(len / 1024),
        dirty_kb: dirty_kb.min(len / 1024),
        anon: false,
        shared,
    }
}

fn anon_vma(start: u64, len: u64, perms: &'static [u8; 4], name: &[u8], flags: &'static str, rss_kb: u64) -> Vma {
    let rss_kb = rss_kb.min(len / 1024);
    Vma {
        start,
        end: start + len,
        perms,
        offset: 0,
        dev: (0, 0),
        ino: 0,
        name: name.to_vec(),
        flags,
        rss_kb,
        dirty_kb: rss_kb,
        anon: true,
        shared: false,
    }
}

/// O mapa de um processo. `None` quando ele não tem espaço de endereçamento (zumbi).
pub(super) fn layout(p: &ProcData, files: &MapFiles) -> Option<Layout> {
    let m = p.mem?;
    let mut rng = Rng(((p.pid as u64) << 40) ^ p.start_ns);
    let extra = u64::from(p.num_threads.max(1)) - 1;
    let ts_kb = thread_stack_kb(p);
    let mut v = Vec::new();

    // Tamanhos: o texto do binário é o `VmExe`; o da libc completa o `VmLib` (que soma o texto das
    // bibliotecas e o vdso).
    let exe_text = pages(m.vm_exe);
    let ld_text = LD_SEGS[1];
    let libc_text = pages(m.vm_lib.saturating_sub((ld_text + VDSO) / 1024).max(4));

    // Residente de arquivo: os segmentos pequenos inteiros, o que sobra do `RssFile` vai pros textos.
    let exe_fixed = (EXE_HDR + EXE_RODATA + EXE_RELRO + EXE_RW) / 1024;
    let libc_fixed = (LIBC_HDR + LIBC_RODATA + LIBC_RELRO + LIBC_RW) / 1024;
    let ld_total = LD_SEGS.iter().sum::<u64>() / 1024;
    let rem = m.rss_file.saturating_sub(exe_fixed + libc_fixed + ld_total);
    let exe_text_rss = (exe_text / 1024).min(rem);
    let libc_text_rss = page_floor_kb(rem - exe_text_rss);

    // Residente anônimo: a pilha (as páginas das strings mais duas), a pilha das threads, o `.bss` da
    // libc e a área do carregador; o resto é o heap.
    let stack_kb = m.vm_stk.max(132);
    let stack_rss = 8 + (stack_kb - 128);
    let libc_bss_rss = 12;
    let ld_anon_rss = 4;
    let heap_rss = page_floor_kb(m.rss_anon.saturating_sub(stack_rss + 8 * extra + libc_bss_rss + ld_anon_rss));

    // O binário (PIE): base em `0x5555_5555_4000` mais até 1 TiB de deslocamento aleatório.
    let exe_base = 0x5555_5555_4000 + ((rng.next() & 0xfff_ffff) << 12);
    let exe = &files.exe;
    let mut a = exe_base;
    let mut off = 0;
    let exe_segs: [(&'static [u8; 4], u64, &'static str, u64, u64); 5] = [
        (b"r--p", EXE_HDR, FLAGS_RO, EXE_HDR / 1024, 0),
        (b"r-xp", exe_text, FLAGS_RX, exe_text_rss, 0),
        (b"r--p", EXE_RODATA, FLAGS_RO, EXE_RODATA / 1024, 0),
        (b"r--p", EXE_RELRO, FLAGS_RELRO, EXE_RELRO / 1024, EXE_RELRO / 1024),
        (b"rw-p", EXE_RW, FLAGS_RW, EXE_RW / 1024, EXE_RW / 1024),
    ];
    for (perms, len, flags, rss, dirty) in exe_segs {
        v.push(file_vma(a, len, perms, off, exe, flags, rss, dirty, false));
        a += len;
        off += len;
    }
    let text_start = exe_base + EXE_HDR;
    let relro_start = text_start + exe_text + EXE_RODATA;
    let exe_end = a;

    // O heap: o `brk` começa até 32 MiB depois do fim do binário.
    let heap_start = exe_end + ((rng.next() & 0x1fff) << 12);
    let data_kb = m.vm_data.saturating_sub(extra * ts_kb);
    let fixed_data_kb = (EXE_RW + LIBC_RW + LIBC_BSS + LD_SEGS[4] + LD_ANON) / 1024;
    let heap_len = pages(data_kb.saturating_sub(fixed_data_kb).max(132));
    v.push(anon_vma(heap_start, heap_len, b"rw-p", b"[heap]", FLAGS_RW, heap_rss));

    // A região de `mmap`, de cima pra baixo: o carregador, a área dele, a libc e as pilhas das threads.
    let mmap_top = 0x7f00_0000_0000 + ((rng.next() & 0x7ff_ffff) << 12);
    let ld_len: u64 = LD_SEGS.iter().sum();
    let ld_base = mmap_top - ld_len;
    let ld_anon_start = ld_base - LD_ANON;
    let libc_len = LIBC_HDR + libc_text + LIBC_RODATA + LIBC_RELRO + LIBC_RW + LIBC_BSS;
    let libc_base = ld_anon_start - libc_len;
    let mut a = libc_base;
    let mut off = 0;
    let libc_segs: [(&'static [u8; 4], u64, &'static str, u64, u64); 5] = [
        (b"r--p", LIBC_HDR, FLAGS_RO, LIBC_HDR / 1024, 0),
        (b"r-xp", libc_text, FLAGS_RX, libc_text_rss, 0),
        (b"r--p", LIBC_RODATA, FLAGS_RO, LIBC_RODATA / 1024, 0),
        (b"r--p", LIBC_RELRO, FLAGS_RELRO, LIBC_RELRO / 1024, LIBC_RELRO / 1024),
        (b"rw-p", LIBC_RW, FLAGS_RW, LIBC_RW / 1024, LIBC_RW / 1024),
    ];
    for (perms, len, flags, rss, dirty) in libc_segs {
        v.push(file_vma(a, len, perms, off, &files.libc, flags, rss, dirty, true));
        a += len;
        off += len;
    }
    v.push(anon_vma(a, LIBC_BSS, b"rw-p", b"", FLAGS_RW, libc_bss_rss));
    v.push(anon_vma(ld_anon_start, LD_ANON, b"rw-p", b"", FLAGS_RW, ld_anon_rss));
    let mut a = ld_base;
    let mut off = 0;
    let ld_kinds: [(&'static [u8; 4], &'static str, bool); 5] = [
        (b"r--p", FLAGS_RO, false),
        (b"r-xp", FLAGS_RX, false),
        (b"r--p", FLAGS_RO, false),
        (b"r--p", FLAGS_RELRO, true),
        (b"rw-p", FLAGS_RW, true),
    ];
    for (len, (perms, flags, dirty)) in LD_SEGS.iter().zip(ld_kinds) {
        let kb = len / 1024;
        v.push(file_vma(a, *len, perms, off, &files.ld, flags, kb, if dirty { kb } else { 0 }, true));
        a += len;
        off += len;
    }
    // Cada thread além da principal: a pilha e, abaixo dela, a página de guarda.
    let mut below = libc_base;
    for _ in 0..extra {
        let len = ts_kb * 1024;
        let start = below - len;
        v.push(anon_vma(start, len, b"rw-p", b"", FLAGS_RW, 8));
        v.push(Vma { rss_kb: 0, dirty_kb: 0, ..anon_vma(start - PAGE, PAGE, b"---p", b"", FLAGS_GUARD, 0) });
        below = start - PAGE;
    }

    // A pilha: o topo fica entre `0x7ffc_0000_0000` e `0x7ffe_0000_0000`, o `vvar` e o `vdso` logo
    // acima, separados por um intervalo aleatório.
    let r = rng.next();
    let stack_end = 0x7ffc_0000_0000 + ((r & 0x1f_ffff) << 12);
    let stack_start = stack_end - stack_kb * 1024;
    v.push(anon_vma(stack_start, stack_kb * 1024, b"rw-p", b"[stack]", FLAGS_STACK, stack_rss));
    let vvar = stack_end + ((((r >> 32) & 0xfff) + 2) << 12);
    v.push(Vma { flags: FLAGS_VVAR, anon: false, ..anon_vma(vvar, VVAR, b"r--p", b"[vvar]", FLAGS_VVAR, 0) });
    v.push(Vma { shared: true, anon: false, dirty_kb: 0, ..anon_vma(vvar + VVAR, VDSO, b"r-xp", b"[vdso]", FLAGS_VDSO, 8) });

    // As strings do `execve` no topo da pilha (`arch_align_stack` tira até 8 KiB): o nome do
    // programa, o ambiente e o argv; abaixo, o auxv e os vetores de ponteiros até o `argc`.
    let sp = stack_end - ((r >> 48) & 0x1ff0);
    let env_end = sp - 8 - (exe.path.len() as u64 + 1);
    let env_start = env_end - p.environ.len() as u64;
    let arg_end = env_start;
    let arg_start = arg_end - p.cmdline.len() as u64;
    let below_strings = (arg_start - 16 - 7) & !0xf;
    let vectors = (8 * (1 + count_nul(&p.cmdline) + 1 + count_nul(&p.environ) + 1) + AUXV_BYTES).next_multiple_of(16);
    let start_stack = below_strings - vectors;

    v.sort_by_key(|x| x.start);
    let fine = rng.next();
    let addrs = MmAddrs {
        start_code: text_start,
        end_code: text_start + exe_text - 1 - (fine & 0x7ff),
        start_stack,
        start_data: relro_start + ((fine >> 16) & 0xff0),
        end_data: exe_end - 0x100 - ((fine >> 32) & 0x6f0),
        start_brk: heap_start,
        arg_start,
        arg_end,
        env_start,
        env_end,
    };
    Some(Layout { vmas: v, addrs })
}

/// `show_vma_header_prefix` mais o nome depois do `seq_pad`.
fn header(o: &mut Vec<u8>, v: &Vma) {
    let begin = o.len();
    let _ = write!(o, "{:08x}-{:08x} ", v.start, v.end);
    o.extend_from_slice(v.perms);
    let _ = write!(o, " {:08x} {:02x}:{:02x} {} ", v.offset, v.dev.0, v.dev.1, v.ino);
    if !v.name.is_empty() {
        while o.len() - begin < NAME_COLUMN {
            o.push(b' ');
        }
        o.push(b' ');
        o.extend_from_slice(&v.name);
    }
    o.push(b'\n');
}

/// `/proc/<pid>/maps`.
pub(super) fn maps(l: Option<&Layout>) -> Vec<u8> {
    let mut o = Vec::new();
    for v in l.map_or(&[][..], |l| &l.vmas[..]) {
        header(&mut o, v);
    }
    o
}

/// Uma linha de tamanho do `smaps`: o nome com dois pontos em 16 colunas e o valor em 8.
fn kb(o: &mut Vec<u8>, name: &str, v: u64) {
    let _ = writeln!(o, "{:<16}{v:>8} kB", format!("{name}:"));
}

/// `/proc/<pid>/smaps`: o cabeçalho de cada área e as linhas do `show_smap` do 6.12.
pub(super) fn smaps(l: Option<&Layout>) -> Vec<u8> {
    let mut o = Vec::new();
    for v in l.map_or(&[][..], |l| &l.vmas[..]) {
        header(&mut o, v);
        let clean = v.rss_kb - v.dirty_kb;
        let pss = if v.shared { clean / 2 + v.dirty_kb } else { v.rss_kb };
        kb(&mut o, "Size", v.size_kb());
        kb(&mut o, "KernelPageSize", 4);
        kb(&mut o, "MMUPageSize", 4);
        kb(&mut o, "Rss", v.rss_kb);
        kb(&mut o, "Pss", pss);
        kb(&mut o, "Pss_Dirty", v.dirty_kb);
        kb(&mut o, "Shared_Clean", if v.shared { clean } else { 0 });
        kb(&mut o, "Shared_Dirty", 0);
        kb(&mut o, "Private_Clean", if v.shared { 0 } else { clean });
        kb(&mut o, "Private_Dirty", v.dirty_kb);
        kb(&mut o, "Referenced", v.rss_kb);
        kb(&mut o, "Anonymous", if v.anon { v.rss_kb } else { v.dirty_kb });
        kb(&mut o, "KSM", 0);
        kb(&mut o, "LazyFree", 0);
        kb(&mut o, "AnonHugePages", 0);
        kb(&mut o, "ShmemPmdMapped", 0);
        kb(&mut o, "FilePmdMapped", 0);
        kb(&mut o, "Shared_Hugetlb", 0);
        kb(&mut o, "Private_Hugetlb", 0);
        kb(&mut o, "Swap", 0);
        kb(&mut o, "SwapPss", 0);
        kb(&mut o, "Locked", 0);
        let _ = writeln!(o, "{:<16}{:>8}", "THPeligible:", 0);
        let _ = writeln!(o, "{:<16}{:>8}", "ProtectionKey:", 0);
        o.extend_from_slice(b"VmFlags: ");
        for f in v.flags.split(' ') {
            o.extend_from_slice(f.as_bytes());
            o.push(b' ');
        }
        o.push(b'\n');
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> MapFiles {
        MapFiles {
            exe: MappedFile { path: b"/usr/bin/cat".to_vec(), dev: (0, 0x2a), ino: 1234 },
            libc: MappedFile { path: LIBC_PATH.to_vec(), dev: (0, 0x2a), ino: 2000 },
            ld: MappedFile { path: LD_PATH.to_vec(), dev: (0, 0x2a), ino: 2001 },
        }
    }

    /// O `cat` do `status.txt` dourado.
    fn cat() -> ProcData {
        let mut rlimits = [(u64::MAX, u64::MAX); 16];
        rlimits[Resource::Stack as usize] = (8 << 20, u64::MAX);
        ProcData {
            pid: 59,
            tid: 59,
            comm: b"cat".to_vec(),
            cmdline: b"cat\0/proc/self/maps\0".to_vec(),
            environ: b"PATH=/usr/bin\0HOME=/root\0".to_vec(),
            num_threads: 1,
            start_ns: 4_000_000_000,
            rlimits,
            mem: Some(MemData {
                vm_size: 3280,
                vm_rss: 1768,
                rss_anon: 116,
                rss_file: 1652,
                vm_data: 488,
                vm_stk: 132,
                vm_exe: 24,
                vm_lib: 1588,
                ..MemData::default()
            }),
            ..ProcData::default()
        }
    }

    #[test]
    fn layout_is_stable_ordered_and_without_overlaps() {
        let a = layout(&cat(), &files()).unwrap();
        assert_eq!(a, layout(&cat(), &files()).unwrap());
        for w in a.vmas.windows(2) {
            assert!(w[0].end <= w[1].start, "{:x?}", w);
        }
        for v in &a.vmas {
            assert_eq!(v.start % PAGE, 0);
            assert_eq!(v.end % PAGE, 0);
        }
        let other = layout(&ProcData { pid: 60, ..cat() }, &files()).unwrap();
        assert_ne!(a.vmas[0].start, other.vmas[0].start);
    }

    #[test]
    fn layout_follows_the_memory_profile() {
        let l = layout(&cat(), &files()).unwrap();
        let named = |n: &[u8]| l.vmas.iter().find(|v| v.name == n).unwrap().clone();
        assert_eq!(named(b"[stack]").size_kb(), 132);
        let text = l.vmas.iter().find(|v| v.name == b"/usr/bin/cat" && v.perms == b"r-xp").unwrap();
        assert_eq!(text.size_kb(), 24);
        let libc_text = l.vmas.iter().find(|v| v.name == LIBC_PATH && v.perms == b"r-xp").unwrap();
        let ld_text = l.vmas.iter().find(|v| v.name == LD_PATH && v.perms == b"r-xp").unwrap();
        // `VmLib`: o texto das bibliotecas e o vdso.
        assert_eq!(libc_text.size_kb() + ld_text.size_kb() + 8, 1588);
        let rss: u64 = l.vmas.iter().map(|v| v.rss_kb).sum();
        assert!(rss <= 1768 + 8, "rss {rss}");
        let names: Vec<&[u8]> = l.vmas.iter().filter(|v| v.name.starts_with(b"[")).map(|v| &v.name[..]).collect();
        assert_eq!(names, [&b"[heap]"[..], b"[stack]", b"[vvar]", b"[vdso]"]);
    }

    #[test]
    fn start_stack_falls_inside_the_stack_below_the_strings() {
        let p = cat();
        let l = layout(&p, &files()).unwrap();
        let st = l.vmas.iter().find(|v| v.name == b"[stack]").unwrap();
        let a = l.addrs;
        assert!(st.start < a.start_stack && a.start_stack < a.arg_start);
        assert_eq!(a.start_stack % 16, 0);
        assert_eq!(a.arg_end - a.arg_start, p.cmdline.len() as u64);
        assert_eq!(a.env_end - a.env_start, p.environ.len() as u64);
        assert!(a.env_end < st.end);
        let text = l.vmas.iter().find(|v| v.perms == b"r-xp").unwrap();
        assert_eq!(a.start_code, text.start);
        assert!(a.end_code > a.start_code && a.end_code <= text.end);
        let heap = l.vmas.iter().find(|v| v.name == b"[heap]").unwrap();
        assert_eq!(a.start_brk, heap.start);
    }

    #[test]
    fn maps_lines_have_the_kernel_columns() {
        let l = layout(&cat(), &files()).unwrap();
        let t = String::from_utf8(maps(Some(&l))).unwrap();
        for line in t.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            assert!(f.len() == 5 || f.len() == 6, "{line}");
            if f.len() == 6 {
                assert_eq!(line.find(f[5]), Some(73), "{line}");
            } else {
                assert!(line.ends_with(" 0 "), "anônimo termina com espaço: {line:?}");
            }
        }
        let first = t.lines().next().unwrap();
        assert!(first.contains(" r--p 00000000 00:2a 1234 "), "{first}");
        let text = t.lines().nth(1).unwrap();
        assert!(text.contains(" r-xp 00002000 00:2a 1234 "), "{text}");
        let heap = t.lines().find(|l| l.ends_with("[heap]")).unwrap();
        assert!(heap.contains(" rw-p 00000000 00:00 0 "), "{heap}");
        assert!(t.ends_with("[vdso]\n"));
    }

    #[test]
    fn header_pads_to_column_73() {
        let v = Vma {
            start: 0x55d0_c0a0_0000,
            end: 0x55d0_c0a1_c000,
            perms: b"r--p",
            offset: 0,
            dev: (8, 1),
            ino: 1_835_023,
            name: b"/usr/bin/bash".to_vec(),
            flags: FLAGS_RO,
            rss_kb: 0,
            dirty_kb: 0,
            anon: false,
            shared: false,
        };
        let mut o = Vec::new();
        header(&mut o, &v);
        assert_eq!(
            String::from_utf8(o).unwrap(),
            format!("55d0c0a00000-55d0c0a1c000 r--p 00000000 08:01 1835023{}/usr/bin/bash\n", " ".repeat(20))
        );
        let mut o = Vec::new();
        header(&mut o, &Vma { start: 0x1000, end: 0x2000, name: Vec::new(), dev: (0, 0), ino: 0, ..v });
        assert_eq!(o, b"00001000-00002000 r--p 00000000 00:00 0 \n".to_vec());
    }

    #[test]
    fn smaps_has_the_fields_of_6_12_in_order() {
        let l = layout(&cat(), &files()).unwrap();
        let t = String::from_utf8(smaps(Some(&l))).unwrap();
        let block: Vec<&str> = t.split_inclusive('\n').take(26).collect();
        let keys: Vec<&str> = block[1..].iter().map(|s| s.split(':').next().unwrap()).collect();
        assert_eq!(
            keys,
            [
                "Size",
                "KernelPageSize",
                "MMUPageSize",
                "Rss",
                "Pss",
                "Pss_Dirty",
                "Shared_Clean",
                "Shared_Dirty",
                "Private_Clean",
                "Private_Dirty",
                "Referenced",
                "Anonymous",
                "KSM",
                "LazyFree",
                "AnonHugePages",
                "ShmemPmdMapped",
                "FilePmdMapped",
                "Shared_Hugetlb",
                "Private_Hugetlb",
                "Swap",
                "SwapPss",
                "Locked",
                "THPeligible",
                "ProtectionKey",
                "VmFlags"
            ]
        );
        assert_eq!(block[1], format!("Size:{}8 kB\n", " ".repeat(18)));
        assert_eq!(block[2], format!("KernelPageSize:{}4 kB\n", " ".repeat(8)));
        assert_eq!(block[23], format!("THPeligible:{}0\n", " ".repeat(11)));
        assert_eq!(block[25], "VmFlags: rd mr mw me sd \n");
        assert!(t.contains(&format!("[stack]\nSize:{}132 kB\n", " ".repeat(16))));
        assert!(t.contains("VmFlags: rd wr mr mw me gd ac \n"));
    }

    #[test]
    fn zombie_has_an_empty_map() {
        let z = ProcData { mem: None, ..cat() };
        assert!(layout(&z, &files()).is_none());
        assert!(maps(None).is_empty() && smaps(None).is_empty());
    }

    #[test]
    fn threads_get_a_stack_and_a_guard_page() {
        let p = ProcData { num_threads: 3, ..cat() };
        let l = layout(&p, &files()).unwrap();
        assert_eq!(l.vmas.iter().filter(|v| v.perms == b"---p").count(), 2);
        assert_eq!(l.vmas.iter().filter(|v| v.anon && v.size_kb() == 8192).count(), 2);
    }
}
