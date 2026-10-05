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
- procfs (marco 3), formatos do 6.12 conferidos no oráculo (`testbench/golden/linux-facts/proc/` e
  sondas): raiz com `self`, `thread-self`, `mounts`, `sys/kernel/pid_max`, `meminfo`, `cpuinfo`, `stat`,
  `uptime`, `loadavg`, `version`, `filesystems`; por pid `task/<tid>/` (com os mesmos arquivos mais
  `children`), `fd/`, `fdinfo/<fd>` (`pos`, `flags` em octal com `O_LARGEFILE` e `O_CLOEXEC`, `mnt_id`,
  `ino`), `cwd`, `exe`, `root`, `cmdline`, `environ`, `comm`, `mounts`, `stat` (52 campos), `statm`,
  `status` (todas as linhas do 6.12, zumbi sem `Umask` e sem bloco `Vm*`), `limits`, e as constantes
  `wchan`, `schedstat`, `cpuset`, `cgroup`, `sessionid`. `readdir` na ordem do kernel (raiz: fixos por
  tamanho de nome e bytes, `self`, `thread-self`, pids; processo: ordem de `tgid_base_stuff` e
  `tid_base_stuff`); modos, `nlink` e `st_size` do `fd/` (fds abertos) e dos links de `fd/N` (64).
  Arquivo fixo recusa abertura pra escrita com EACCES, arquivo de processo dá EINVAL na escrita.
  Dados reais do kernel: estado, tempos de CPU do escalonador, rlimits, sinais, trocas de contexto,
  tempos por CPU virtual, processos criados, média de carga (`calc_load`), uptime do sandbox.

## Aproximado no procfs

- Memória por processo (`Vm*`, `statm`, `stat` vsize e rss) e da máquina (`meminfo`): o kernel não mede
  uso real; é um modelo (`crates/kernel/src/procmem.rs`): perfil da imagem medido no oráculo, pilha
  conforme argv e ambiente, uma pilha por thread, pico acumulado. Sem swap, slab, buffers ou hugetlb (0).
- `utime`/`stime`: o escalonador não separa tempo de sistema; tudo é `utime`, `stime` é 0. O mesmo vale
  pro `/proc/stat` (coluna de sistema 0, iowait, irq, softirq, steal e guest 0, `intr` e `softirq` 0).
- `minflt`/`majflt` (e as de filhos) são 0 (sem contabilidade de page fault); endereços do `stat`
  (`startcode`, `startstack`, `arg_start`...) são 0, o que o Linux mostra de processo sem `mm`.
- `SigQ` conta só os pendentes do próprio processo (o Linux conta por usuário); `SigBlk` é 0 (sem
  `sigprocmask`); `starttime` de thread secundária é o do processo.
- `Seccomp`, `Speculation_*`, `Mems_allowed`, `x86_Thread_features` e `/proc/filesystems` copiam o
  oráculo (constantes do ambiente); `cpuinfo` usa o modelo do oráculo com topologia sem SMT.
- `comm` aceita só leitura (mode 0644 como no Linux, escrita dá EINVAL); `/proc/sys/kernel/pid_max` idem.

## Falta

- Marco 3: `/sys/devices/system/cpu/online` (e o resto do sysfs): o sandbox tem `/sys` como diretório
  vazio do tmpfs da raiz e nenhum sistema de arquivos montado nele; falta um `sysfs` e a montagem (e a
  linha em `/proc/mounts`), então nada foi inventado; outros arquivos de `/proc` que o oráculo tem
  (`maps`, `smaps`, `mountinfo`, `io`, `oom_*`, `net/`, `sys/` além de `kernel/pid_max`, `vmstat`...);
  escrita em `comm` e em `pid_max`; hostfs híbrido do E04; overlay.
- O_TMPFILE e RENAME_WHITEOUT (EOPNOTSUPP/EINVAL hoje).
