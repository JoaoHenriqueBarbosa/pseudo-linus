# vfs: estado

Testes: `cargo test -p vfs` (unitários, cenários escritos à mão em `tests/basic.rs` e o diferencial
`tests/differential.rs` contra o tmpfs real do host em /dev/shm: 512 sequências por padrão, ~26 mil
operações; rodadas de 4096 sequências e ~207 mil operações verdes; `VFS_DIFF_CASES`, `VFS_DIFF_SEED`).

## Pronto

- Resolução de caminho com o comportamento do Linux 6.12: permissão de busca em cada componente, ENOTDIR,
  ENOENT, ENAMETOOLONG (componente e caminho), 40 symlinks e ELOOP no 41º, `..` limitado à raiz do processo
  e atravessando montagens, barra no fim, magic links do procfs (pulo pra lugar e objeto do kernel).
- Syscalls de arquivo com a ordem de errnos do Linux: open (O_CREAT, O_EXCL, O_TRUNC, O_DIRECTORY,
  O_NOFOLLOW, O_PATH, symlink pendurado com O_CREAT cria o alvo), stat/lstat/fstatat com AT_EMPTY_PATH,
  access, mkdir, mknod, symlink, link (protected_hardlinks), unlink, rmdir, rename e renameat2 (NOREPLACE,
  EXCHANGE), readlink, chmod (EOPNOTSUPP em symlink), chown (limpa setuid/setgid), utimensat (OMIT, NOW),
  truncate, statfs, chdir, getcwd (ENOENT com cwd removido), exec_open.
- Permissões: tríade dono/grupo/outros, root com CAP_DAC_OVERRIDE e CAP_DAC_READ_SEARCH (executar exige
  um bit x), sticky, setgid de diretório, EROFS, perda de setuid/setgid em escrita e truncamento.
- tmpfs persistente do E03: imbl OrdMap pra inodes e diretórios, imbl Vector de blocos de 4 KiB com
  buracos, readdir do mais novo pro mais antigo (rename põe a entrada como a mais nova), tamanho de
  diretório 40 + 20 por entrada, nlink de diretório, st_blocks por página alocada, relatime, órfãos
  abertos, snapshot/restore O(1) sem voltar o contador de inodes, sandbox derivada, cota de páginas e de
  inodes (ENOSPC), SEEK_DATA/SEEK_HOLE.
- procfs mínimo: `self`, `thread-self`, `mounts`, `sys/kernel/pid_max`, e por pid `fd/`, `cwd`, `exe`,
  `root`, `cmdline`, `environ`, `comm`, `mounts`.

## Falta

- Marco 3: procfs completo nos formatos de `testbench/golden/linux-facts/proc/` (stat com 52 campos,
  status, statm, task/<tid>/, limits, fdinfo, meminfo, cpuinfo, stat global, uptime, loadavg, version,
  filesystems, `/sys/devices/system/cpu/online`); hostfs híbrido do E04; overlay.
- O_TMPFILE e RENAME_WHITEOUT (EOPNOTSUPP/EINVAL hoje).
