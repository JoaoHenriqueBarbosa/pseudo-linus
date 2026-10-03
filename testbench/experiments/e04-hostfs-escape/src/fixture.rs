//! Monta, dentro de `testbench/scratch/e04`, a árvore usada pelos testes:
//!
//! - `outside/canary.txt`: o arquivo que nenhum candidato pode conseguir ler a partir do sandbox;
//! - `jail/`: o diretório do host montado em `/work` no sandbox, cheio de armadilhas (symlinks que
//!   apontam pra fora, `..` encadeado, magic links de `/proc`, loops, cadeias de 40 e 41 symlinks);
//! - `refroot/`: a referência. É o namespace do sandbox materializado de verdade (`etc/passwd` do
//!   sandbox, `tmp/`, e uma cópia de `jail/` em `work/`), resolvido pelo próprio kernel com
//!   `RESOLVE_IN_ROOT`, que é a semântica exata de um chroot. O que o kernel responde ali é o que um Linux
//!   de verdade responderia pro mesmo caminho.

use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rustix::fs::{Mode, OFlags};

pub const CANARY: &[u8] = b"CANARY-OUTSIDE\n";
pub const SANDBOX_PASSWD: &[u8] = b"SANDBOX-PASSWD\n";
pub const INSIDE_FILE: &[u8] = b"INSIDE-FILE\n";
pub const INSIDE_RACE: &[u8] = b"INSIDE-RACE\n";

pub struct Fixture {
    pub jail: PathBuf,
    pub outside: PathBuf,
    pub refroot: PathBuf,
    /// Só precisa existir: mantém `/proc/self/fd/<n>` (alvo do symlink `link_fd`) apontando pro
    /// diretório de fora durante os testes.
    _outside_fd: OwnedFd,
}

pub fn build() -> Result<Fixture> {
    let base = harness::paths::scratch_dir("e04").join("run");
    if base.exists() {
        std::fs::remove_dir_all(&base).with_context(|| format!("limpando {}", base.display()))?;
    }
    let outside = base.join("outside");
    let jail = base.join("jail");
    let refroot = base.join("refroot");
    std::fs::create_dir_all(&outside)?;
    std::fs::create_dir_all(&jail)?;
    std::fs::write(outside.join("canary.txt"), CANARY)?;
    let outside_fd = rustix::fs::open(&outside, OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC, Mode::empty())?;
    let outside_abs = outside.canonicalize()?;
    let outside_s = outside_abs.to_string_lossy().into_owned();

    // Conteúdo legítimo.
    std::fs::write(jail.join("file.txt"), INSIDE_FILE)?;
    std::fs::create_dir_all(jail.join("dir/sub"))?;
    std::fs::write(jail.join("dir/sub/file.txt"), b"INSIDE-DEEP\n")?;
    std::fs::write(jail.join("f1"), b"F1\n")?;
    std::fs::create_dir_all(jail.join("d1/d2/d3"))?;
    std::fs::write(jail.join("d1/d2/d3/f4"), b"F4\n")?;
    let deep: PathBuf = (1..=15).map(|i| format!("e{i}")).collect();
    std::fs::create_dir_all(jail.join(&deep))?;
    std::fs::write(jail.join(&deep).join("f16"), b"F16\n")?;

    // Armadilhas.
    let sym = |target: &str, name: &str| std::os::unix::fs::symlink(target, jail.join(name));
    sym(&outside_s, "link_abs_out")?;
    sym("../outside", "link_rel_out")?;
    sym("dir/../../outside", "link_rel_out2")?;
    sym("/etc/passwd", "etc_link")?;
    sym("../etc/passwd", "up_passwd")?;
    sym("/work/file.txt", "abs_inside")?;
    sym("dir/sub/file.txt", "inside_link")?;
    sym(&format!("/proc/self/root{outside_s}"), "link_proc")?;
    sym(&format!("/proc/self/fd/{}", outside_fd.as_raw_fd()), "link_fd")?;
    sym("loop_b", "loop_a")?;
    sym("loop_a", "loop_b")?;
    // c00 -> c01 -> ... -> c39 -> file.txt: 40 symlinks, o máximo do Linux (MAXSYMLINKS).
    for i in 0..40 {
        let target = if i == 39 { "file.txt".to_string() } else { format!("c{:02}", i + 1) };
        sym(&target, &format!("c{i:02}"))?;
    }
    // x00 -> ... -> x40 -> file.txt: 41 symlinks, um a mais que o limite.
    for i in 0..41 {
        let target = if i == 40 { "file.txt".to_string() } else { format!("x{:02}", i + 1) };
        sym(&target, &format!("x{i:02}"))?;
    }
    // Corrida: race/ é diretório legítimo, race_alt é symlink pra fora; uma thread troca os dois.
    std::fs::create_dir_all(jail.join("race"))?;
    std::fs::write(jail.join("race/canary.txt"), INSIDE_RACE)?;
    sym(&outside_s, "race_alt")?;
    // Hardlink pré-existente pro canário: não é fuga de resolução, é um objeto colocado dentro da montagem.
    std::fs::hard_link(outside.join("canary.txt"), jail.join("hard_canary"))?;

    // Referência: namespace do sandbox materializado.
    std::fs::create_dir_all(refroot.join("etc"))?;
    std::fs::create_dir_all(refroot.join("tmp"))?;
    std::fs::write(refroot.join("etc/passwd"), SANDBOX_PASSWD)?;
    copy_tree(&jail, &refroot.join("work"))?;

    Ok(Fixture { jail, outside, refroot, _outside_fd: outside_fd })
}

/// Cópia recursiva preservando symlinks (o alvo é copiado como texto, sem seguir).
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let dst = to.join(entry.file_name());
        if ft.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(entry.path())?, &dst)?;
        } else if ft.is_dir() {
            copy_tree(&entry.path(), &dst)?;
        } else {
            std::fs::copy(entry.path(), &dst)?;
        }
    }
    Ok(())
}
