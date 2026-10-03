//! Cenários de memória do H17. Rodam no binário `e03-mem`, que instala um allocator contador
//! (`stats_alloc`) e passa a leitura dos contadores pra cá.
//!
//! "Vivo" é bytes pedidos e ainda não liberados (sem o overhead do malloc, ~16 bytes por bloco no
//! glibc, que é estimado à parte a partir do número de alocações vivas).

use serde::Serialize;
use serde_json::{Value, json};

use crate::maps::Flavor;
use crate::measure::{BIG_DIR, HUGE_FILE, SIZES};
use crate::vfs::Vfs;
use crate::workload::{Image, ImageSpec, Rng, apply_changes, build_big_dir, build_huge_file, build_image};

#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    pub bytes_allocated: u64,
    pub bytes_deallocated: u64,
    pub allocations: u64,
    pub deallocations: u64,
}

impl Counts {
    fn live_bytes(&self) -> i64 {
        self.bytes_allocated as i64 - self.bytes_deallocated as i64
    }
    fn live_allocs(&self) -> i64 {
        self.allocations as i64 - self.deallocations as i64
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Delta {
    /// Bytes vivos a mais depois do trecho.
    pub live_bytes: i64,
    /// Alocações vivas a mais depois do trecho.
    pub live_allocs: i64,
    /// Bytes alocados durante o trecho (inclui temporários).
    pub allocated_bytes: u64,
}

pub type Counter<'a> = &'a dyn Fn() -> Counts;

fn delta(before: Counts, after: Counts) -> Delta {
    Delta {
        live_bytes: after.live_bytes() - before.live_bytes(),
        live_allocs: after.live_allocs() - before.live_allocs(),
        allocated_bytes: after.bytes_allocated - before.bytes_allocated,
    }
}

fn measure<R>(c: Counter, f: impl FnOnce() -> R) -> (Delta, R) {
    let before = c();
    let r = f();
    (delta(before, c()), r)
}

fn mean(xs: &[i64]) -> f64 {
    xs.iter().sum::<i64>() as f64 / xs.len().max(1) as f64
}

/// Bytes retidos pela primeira escrita de 1 byte depois de um snapshot (o que a cópia de caminho
/// alocou), com o snapshot ainda vivo. Média sobre `paths`.
fn first_write_bytes<F: Flavor>(c: Counter, vfs: &mut Vfs<F>, paths: &[Vec<u8>]) -> Value {
    let mut bytes = Vec::new();
    let mut allocs = Vec::new();
    for p in paths {
        let snap = vfs.snapshot();
        let (d, r) = measure(c, || vfs.write(p, 0, b"x"));
        r.expect("escrita");
        bytes.push(d.live_bytes);
        allocs.push(d.live_allocs);
        drop(snap);
    }
    json!({"samples": paths.len(), "mean_live_bytes": mean(&bytes), "mean_live_allocs": mean(&allocs),
           "max_live_bytes": bytes.iter().max().copied().unwrap_or(0)})
}

pub fn structure_memory<F: Flavor>(c: Counter) -> Value {
    let mut sizes = Vec::new();
    let mut depth = Value::Null;
    let mut sandboxes = Value::Null;
    for &n in &SIZES {
        let (base, img): (Delta, Image<F>) = measure(c, || build_image(&ImageSpec::with_files(n)));
        let (snap_cost, snap) = measure(c, || img.fs.clone());
        drop(snap);
        let (sandbox_cost, sb) = measure(c, || Vfs::from_image(&img.fs));
        drop(sb);
        let mut rng = Rng::new(n as u64);
        let picks: Vec<Vec<u8>> = (0..64).map(|_| img.files[rng.below(img.files.len() as u64) as usize].clone()).collect();
        let mut vfs = Vfs::from_image(&img.fs);
        let first = first_write_bytes(c, &mut vfs, &picks);
        // Restore depois de 10 escritas: o que ele libera são as cópias de caminho dessas escritas.
        let base_snap = vfs.snapshot();
        for p in &picks[..10] {
            vfs.write(p, 0, b"z").expect("escrita");
        }
        let (restore, ()) = measure(c, || vfs.restore(&base_snap));
        drop(base_snap);
        // Restore depois de uma escrita só, em 10 arquivos diferentes: trabalho por escrita, sem a
        // sobreposição de caminhos que 10 escritas têm numa imagem pequena.
        let mut freed_one = Vec::new();
        for p in &picks[10..20] {
            let snap = vfs.snapshot();
            vfs.write(p, 0, b"w").expect("escrita");
            let (d, ()) = measure(c, || vfs.restore(&snap));
            freed_one.push(-d.live_bytes);
        }
        if n == *SIZES.last().expect("tamanhos") {
            let rows: Vec<Value> = [1usize, 8, 32]
                .iter()
                .map(|&d| {
                    let p = std::slice::from_ref(&img.deep_files[d - 1]);
                    json!({"depth": d, "first_write": first_write_bytes(c, &mut vfs, p)})
                })
                .collect();
            depth = Value::Array(rows);
            let (derived, boxes) = measure(c, || {
                (0..100)
                    .map(|i| {
                        let mut v = Vfs::from_image(&img.fs);
                        apply_changes(&mut v, &img.files, i, &mut rng);
                        v
                    })
                    .collect::<Vec<_>>()
            });
            let per = derived.live_bytes as f64 / 100.0;
            sandboxes = json!({
                "count": 100,
                "base_live_bytes": base.live_bytes,
                "derived_total_live_bytes": derived.live_bytes,
                "per_sandbox_live_bytes": per,
                "per_sandbox_live_allocs": derived.live_allocs as f64 / 100.0,
                "base_plus_100_bytes": base.live_bytes + derived.live_bytes,
                "deep_copy_equivalent_bytes": base.live_bytes * 101,
                "sharing_factor": (base.live_bytes * 101) as f64 / (base.live_bytes + derived.live_bytes) as f64,
            });
            drop(boxes);
        }
        drop(vfs);
        sizes.push(json!({
            "files": n,
            "inodes": img.inodes,
            "content_bytes": img.content_bytes,
            "image_live_bytes": base.live_bytes,
            "image_live_allocs": base.live_allocs,
            "overhead_bytes_per_inode": (base.live_bytes - img.content_bytes as i64) as f64 / img.inodes as f64,
            "snapshot": snap_cost,
            "sandbox_new": sandbox_cost,
            "first_write_after_snapshot": first,
            "restore_after_10_writes": {"freed_bytes": -restore.live_bytes, "freed_allocs": -restore.live_allocs},
            "restore_after_1_write": {"mean_freed_bytes": mean(&freed_one)},
        }));
        drop(img);
    }
    let (big_cost, big) = measure(c, || build_big_dir::<F>(BIG_DIR));
    let mut vfs = Vfs::from_image(&big);
    let snap = vfs.snapshot();
    let (create_first, r) = measure(c, || vfs.create(b"/big/new-a", 0o644));
    r.expect("create");
    let (create_second, r) = measure(c, || vfs.create(b"/big/new-b", 0o644));
    r.expect("create");
    drop(snap);
    json!({
        "sizes": sizes,
        "depth": depth,
        "sandboxes_100k": sandboxes,
        "big_dir": {
            "entries": BIG_DIR,
            "image_live_bytes": big_cost.live_bytes,
            "create_after_snapshot": create_first,
            "create_in_place": create_second,
        },
    })
}

pub fn content_memory<F: Flavor>(c: Counter) -> Value {
    let (base, img): (Delta, Image<F>) = measure(c, || build_image(&ImageSpec::with_files(100_000)));
    let image = json!({
        "files": 100_000,
        "content_bytes": img.content_bytes,
        "image_live_bytes": base.live_bytes,
        "image_live_allocs": base.live_allocs,
        "overhead_bytes_per_inode": (base.live_bytes - img.content_bytes as i64) as f64 / img.inodes as f64,
    });
    drop(img);
    let (huge_cost, huge) = measure(c, || build_huge_file::<F>(HUGE_FILE));
    let mut vfs = Vfs::from_image(&huge);
    let mut rng = Rng::new(3);
    let mut bytes = Vec::new();
    for _ in 0..8 {
        let off = rng.below(HUGE_FILE as u64);
        let snap = vfs.snapshot();
        let (d, r) = measure(c, || vfs.write(b"/huge", off, b"x"));
        r.expect("escrita");
        bytes.push(d.live_bytes);
        drop(snap);
    }
    json!({
        "image_100k": image,
        "huge_file": {
            "size_bytes": HUGE_FILE,
            "live_bytes": huge_cost.live_bytes,
            "overhead_bytes": huge_cost.live_bytes - HUGE_FILE as i64,
            "write_1_byte_after_snapshot_live_bytes": mean(&bytes),
        },
    })
}
