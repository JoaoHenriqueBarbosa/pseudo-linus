//! `slabtop` do procps-ng 4.0.4, no modo `-o` (uma passada, sem curses).
//!
//! Lê `/proc/slabinfo` (versão 2.1), soma os totais e lista os caches ordenados pelo critério de `-s`
//! (o padrão é o número de objetos). Sem `-o` o original abre uma tela do curses e atualiza a cada
//! `-d` segundos; o sandbox não tem terminal de curses, então o modo interativo imprime a mesma
//! passada uma vez.

use std::ffi::OsString;

use sysabi::{Ctx, Errno};
use ul_misc::util::getopt::{Getopt, HasArg, LongOpt};
use ul_misc::util::io;

use crate::common::{self, Strtol, out};
use crate::procfs;

const USAGE: &str = "\nUsage:\n slabtop [options]\n\nOptions:\n -d, --delay <secs>  delay updates\n -o, --once          only display once, then exit\n -s, --sort <char>   specify sort criteria by character (see below)\n\n -h, --help     display this help and exit\n -V, --version  output version information and exit\n\nThe following are valid sort criteria:\n a: sort by number of active objects\n b: sort by objects per slab\n c: sort by cache size\n l: sort by number of slabs\n v: sort by (non display) number of active slabs\n n: sort by name\n o: sort by number of objects (the default)\n p: sort by (non display) pages per slab\n s: sort by object size\n u: sort by cache utilization\n\nFor more details see slabtop(1).\n";

const LONGS: &[LongOpt] = &[
    LongOpt::new("delay", HasArg::Required, 'd' as i32),
    LongOpt::new("sort", HasArg::Required, 's' as i32),
    LongOpt::new("once", HasArg::No, 'o' as i32),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

/// Página de 4 KiB, em bytes.
const PAGE: u64 = 4096;

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// Um cache de `/proc/slabinfo`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cache {
    pub name: String,
    pub active_objs: u64,
    pub num_objs: u64,
    pub obj_size: u64,
    pub obj_per_slab: u64,
    pub pages_per_slab: u64,
    pub active_slabs: u64,
    pub num_slabs: u64,
}

impl Cache {
    /// Tamanho do cache em KiB.
    fn size_kb(&self) -> u64 {
        self.num_slabs * self.pages_per_slab * PAGE / 1024
    }

    /// Utilização em porcento (inteira, como o `%3u` da linha).
    fn use_pct(&self) -> u64 {
        if self.num_objs == 0 { 0 } else { self.active_objs * 100 / self.num_objs }
    }
}

/// Interpreta o `slabinfo`; linhas de comentário, a versão e linhas malformadas são puladas.
pub fn parse_slabinfo(text: &str) -> Vec<Cache> {
    let mut v = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.starts_with("slabinfo") {
            continue;
        }
        let t: Vec<&str> = line.split_ascii_whitespace().collect();
        if t.len() < 6 {
            continue;
        }
        let n = |i: usize| t.get(i).and_then(|x| x.parse::<u64>().ok()).unwrap_or(0);
        let (active_slabs, num_slabs) = match t.iter().position(|x| *x == "slabdata") {
            Some(i) => (n(i + 1), n(i + 2)),
            None => (0, 0),
        };
        v.push(Cache {
            name: t[0].to_string(),
            active_objs: n(1),
            num_objs: n(2),
            obj_size: n(3),
            obj_per_slab: n(4),
            pages_per_slab: n(5),
            active_slabs,
            num_slabs,
        });
    }
    v
}

/// Ordena pelo critério `key`; `None` se a letra não é um critério. Tudo em ordem decrescente, fora
/// o nome.
fn sort(caches: &mut [Cache], key: char) -> bool {
    match key {
        'a' => caches.sort_by(|a, b| b.active_objs.cmp(&a.active_objs)),
        'b' => caches.sort_by(|a, b| b.obj_per_slab.cmp(&a.obj_per_slab)),
        'c' => caches.sort_by(|a, b| b.size_kb().cmp(&a.size_kb())),
        'l' => caches.sort_by(|a, b| b.num_slabs.cmp(&a.num_slabs)),
        'v' => caches.sort_by(|a, b| b.active_slabs.cmp(&a.active_slabs)),
        'n' => caches.sort_by(|a, b| a.name.cmp(&b.name)),
        'o' => caches.sort_by(|a, b| b.num_objs.cmp(&a.num_objs)),
        'p' => caches.sort_by(|a, b| b.pages_per_slab.cmp(&a.pages_per_slab)),
        's' => caches.sort_by(|a, b| b.obj_size.cmp(&a.obj_size)),
        'u' => caches.sort_by(|a, b| b.use_pct().cmp(&a.use_pct())),
        _ => return false,
    }
    true
}

fn pct(part: u64, whole: u64) -> f64 {
    if whole == 0 { 0.0 } else { 100.0 * part as f64 / whole as f64 }
}

/// A tela inteira de uma passada: cinco linhas de totais, linha em branco, cabeçalho e os caches.
pub fn report(caches: &[Cache]) -> String {
    let objs: u64 = caches.iter().map(|c| c.num_objs).sum();
    let active_objs: u64 = caches.iter().map(|c| c.active_objs).sum();
    let slabs: u64 = caches.iter().map(|c| c.num_slabs).sum();
    let active_slabs: u64 = caches.iter().map(|c| c.active_slabs).sum();
    let active_caches = caches.iter().filter(|c| c.num_objs > 0).count() as u64;
    let total_caches = caches.len() as u64;
    let size: u64 = caches.iter().map(|c| c.num_slabs * c.pages_per_slab * PAGE).sum();
    let active_size: u64 = caches.iter().map(|c| c.active_slabs * c.pages_per_slab * PAGE).sum();
    let k = |b: u64| b as f64 / 1024.0;
    let (min, max) = if caches.is_empty() {
        (0, 0)
    } else {
        (caches.iter().map(|c| c.obj_size).min().unwrap_or(0), caches.iter().map(|c| c.obj_size).max().unwrap_or(0))
    };
    let avg = if objs == 0 { 0.0 } else { caches.iter().map(|c| c.obj_size * c.num_objs).sum::<u64>() as f64 / objs as f64 };
    let mut s = String::new();
    s.push_str(&format!(" {:<35}: {} / {} ({:.1}%)\n", "Active / Total Objects (% used)", active_objs, objs, pct(active_objs, objs)));
    s.push_str(&format!(" {:<35}: {} / {} ({:.1}%)\n", "Active / Total Slabs (% used)", active_slabs, slabs, pct(active_slabs, slabs)));
    s.push_str(&format!(" {:<35}: {} / {} ({:.1}%)\n", "Active / Total Caches (% used)", active_caches, total_caches, pct(active_caches, total_caches)));
    s.push_str(&format!(
        " {:<35}: {:.2}K / {:.2}K ({:.1}%)\n",
        "Active / Total Size (% used)",
        k(active_size),
        k(size),
        pct(active_size, size)
    ));
    s.push_str(&format!(
        " {:<35}: {:.2}K / {:.2}K / {:.2}K\n\n",
        "Minimum / Average / Maximum Object",
        k(min),
        avg / 1024.0,
        k(max)
    ));
    s.push_str("  OBJS ACTIVE  USE OBJ SIZE  SLABS OBJ/SLAB CACHE SIZE NAME                   \n");
    for c in caches {
        s.push_str(&format!(
            "{:6} {:6} {:3}% {:7.2}K {:6} {:8} {:9}K {:<23}\n",
            c.num_objs,
            c.active_objs,
            c.use_pct(),
            k(c.obj_size),
            c.num_slabs,
            c.obj_per_slab,
            c.size_kb(),
            c.name
        ));
    }
    s
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut sort_key = 'o';
    let mut g = Getopt::from_env(&argv[1..], "d:ohs:V", LONGS);
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n{USAGE}", e.message(&argv0)));
                return 1;
            }
        };
        match opt.short() {
            Some('d') => {
                let a = opt.arg_str();
                match common::strtol(&a) {
                    Strtol::Ok(v) if v >= 1 => {}
                    Strtol::Ok(_) => {
                        common::warn("slabtop", "delay must be positive integer");
                        return 1;
                    }
                    Strtol::Invalid => {
                        common::warn("slabtop", &format!("illegal delay: '{a}'"));
                        return 1;
                    }
                    Strtol::Range(_) => {
                        common::warn("slabtop", &format!("illegal delay: '{a}': {}", Errno::ERANGE.message()));
                        return 1;
                    }
                }
            }
            Some('o') => {}
            Some('s') => {
                let a = opt.arg_str();
                let mut it = a.chars();
                match (it.next(), it.next()) {
                    (Some(c), None) if "abclvnopsu".contains(c) => sort_key = c,
                    _ => {
                        io::eprint(USAGE);
                        return 1;
                    }
                }
            }
            Some('h') => {
                out(USAGE);
                return 0;
            }
            Some('V') => {
                out("slabtop from procps-ng 4.0.4\n");
                return 0;
            }
            _ => unreachable!("tabela de opções do slabtop"),
        }
    }
    let Some(data) = procfs::read("/proc/slabinfo") else {
        common::warn("slabtop", &format!("Unable to create slabinfo structure: {}", Errno::EACCES.message()));
        return 1;
    };
    let mut caches = parse_slabinfo(&String::from_utf8_lossy(&data));
    sort(&mut caches, sort_key);
    out(report(&caches));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "slabinfo - version: 2.1\n# name <active_objs> <num_objs> <objsize> <objperslab> <pagesperslab> : tunables <limit> <batchcount> <sharedfactor> : slabdata <active_slabs> <num_slabs> <sharedavail>\ndentry 100 200 192 21 1 : tunables 0 0 0 : slabdata 10 10 0\nkmalloc-8 50 50 8 512 1 : tunables 0 0 0 : slabdata 1 1 0\n";

    #[test]
    fn parses_and_sorts_by_objects() {
        let mut v = parse_slabinfo(SAMPLE);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].num_slabs, 10);
        assert_eq!(v[0].size_kb(), 40);
        sort(&mut v, 'n');
        assert_eq!(v[0].name, "dentry");
        sort(&mut v, 's');
        assert_eq!(v[1].name, "kmalloc-8");
    }

    #[test]
    fn report_has_the_totals_and_the_header() {
        let r = report(&parse_slabinfo(SAMPLE));
        assert!(r.starts_with(" Active / Total Objects (% used)    : 150 / 250 (60.0%)\n"), "{r}");
        assert!(r.contains("\n\n  OBJS ACTIVE  USE OBJ SIZE  SLABS OBJ/SLAB CACHE SIZE NAME"));
        let empty = report(&[]);
        assert!(empty.contains("0 / 0 (0.0%)"));
    }
}
