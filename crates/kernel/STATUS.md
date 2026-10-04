# kernel: estado

API pública: `API.md`. Testes: `cargo test -p kernel` (10 unitários + 19 de integração com programas de
teste próprios: echo, cat, yes, head, sleep, um pipeline mínimo e cenários).

## Marco 1 (pronto)

- Modelo A: thread do SO por thread de pseudo-processo, `Syscalls` instalado por thread (tid = pid na
  principal). Threads (`spawn_thread`, `join_thread`, `gettid`) com `exit_group`: `exit`, sinal fatal ou fim
  do `main` em qualquer thread desenrola as outras no próximo ponto de checagem; zumbi quando a última sai.
- Thread spawner por sandbox com hook do host; threads criadas de dentro do sandbox nascem da thread do
  processo (descendente da spawner, herda Landlock e seccomp).
- Processos: `spawn` (posix_spawn com erros de exec devolvidos ao pai), `spawn_fn` (fork: cópia de cwd,
  ambiente, umask, rlimits, nice, disposições, fds compartilhando descrições), `ProcAttrs` (cwd, ações de
  fd na ordem, sinais, grupo, sessão), `execve` (builtin por conteúdo, `#!` com 5 níveis e ELOOP, E2BIG,
  FD_CLOEXEC, capturados voltam ao padrão, `comm`), `wait4` (Any, Pid, Group, NOHANG, UNTRACED,
  CONTINUED, ECHILD, EINTR), zumbis até o wait, órfãos adotados pelo init (pid 1 virtual), SIGCHLD
  ignorado colhe na hora, pid_max 4194304 com volta pra 300.
- Sinais: geração e entrega como o Linux (ignorados descartados na geração, SIGCONT retoma, paradas com
  relatório pro pai, SIGKILL desenrola processo parado ou bloqueado), `Catch` com fila e EINTR na espera,
  `KillUnwind`/`ExitUnwind` com Drops; durante um unwind nada desenrola de novo (espera devolve EINTR).
- fds: descrições compartilhadas (offset e flags de status), CLOEXEC por fd, O_APPEND, O_TRUNC,
  O_CREAT|O_EXCL, O_DIRECTORY, O_NOFOLLOW, O_NONBLOCK, O_PATH, EMFILE pelo RLIMIT_NOFILE, F_DUPFD,
  F_GETFL/F_SETFL.
- Pipes: 65536 bytes, atômico até 4096, EOF, EPIPE + SIGPIPE, O_NONBLOCK/EAGAIN; FIFOs com encontro de
  leitor e escritor (contadores), ENXIO; reabrir pipe por `/proc/self/fd/N` (`/dev/stdin`).
- Dispositivos: null, zero, full (ENOSPC), random, urandom; `/dev/tty` sem terminal dá ENXIO.
- `poll` (pipes com registro de espera, arquivos e dispositivos sempre prontos), travas OFD
  (`F_OFD_SETLK`/`SETLKW`/`GETLK`, soltas no último close; sem detecção de impasse, como o Linux faz pra
  travas OFD), RLIMIT_FSIZE com SIGXFSZ.
- Imagem Debian 13 (`/etc` copiado do oráculo em `image/`, árvore do container, `/usr/bin` com um
  executável por programa), `/dev` em tmpfs próprio, `/proc` montado, `uname` do Debian 13.
- Host: `run`, `spawn` com `HostStdio`, `wait`, `kill`, `processes`, `usage`, FS direto, `tree`,
  snapshot/restore, sandbox derivada, relógio fixo (faketime), destruição.

Desvio pedido pelo dono: `/usr/lib/os-release` tem `ID_LIKE=debian` (o Debian real não tem essa linha).

## Falta (com marco)

- Marco 2: tokens de CPU com o EEVDF, grupos usuário > sandbox > processo (`create_user_group`), timer,
  watchdog nice 19, `sched_yield`/nice/`setpriority` no escalonador, utime/stime do escalonador (hoje: CPU
  da thread do host; `getrusage` não soma threads vivas que não sejam a que chama).
- Marco 3: `/proc` completo (stat, status, statm, task/, meminfo, cpuinfo, stat, uptime, loadavg, limits,
  fdinfo), statfs dos pseudo-fs, RLIMIT_NPROC já vale, RLIMIT_CPU, `kernel::mem` (tracker do E07,
  `mem_bytes`), `net_connect` com allowlist (hoje: política vazia, toda conexão dá EACCES), hostfs,
  setpgid de filho que já fez exec (EACCES).
- Marco 4: pty e line discipline, `isatty`/`tcgetwinsize` reais (hoje sempre falso/ENOTTY), terminal de
  controle, job control com grupos órfãos e SIGTTIN/SIGTTOU, `tcsetpgrp`.
- O_TMPFILE (EOPNOTSUPP), ETXTBSY.
