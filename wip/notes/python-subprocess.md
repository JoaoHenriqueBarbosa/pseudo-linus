# subprocess do disco sobre `_posixsubprocess.fork_exec`

O `subprocess` do sandbox deixou de ser uma versão própria: `modules/pysrc.rs` registra o
`usr/lib/python3.13/subprocess.py` da imagem, e o `py/subprocess.py` próprio saiu (`git rm`).
Repr, argumentos, mensagens de erro, `communicate`, `wait` com prazo, `__del__` e o resto vêm do código do Debian.

## Peças

- `modules/py/_posixsubprocess.py`: `fork_exec` com os 23 argumentos posicionais do 3.13. Calcula o plano de fds
  do `child_exec` (stdio, `pass_fds`, `close_fds` pelo `/proc/self/fd`) e cria o filho com `_os.spawn`
  (`Sys::spawn`). Falha de `exec` ou de `chdir` sai pelo `errpipe` (`OSError:%x:noexec`, `OSError:%x:noexec:chdir`,
  `SubprocessError:0:Exception occurred in preexec_fn.`) e o pid devolvido é um que nunca existe, então o
  `waitpid` seguinte acha `ECHILD`, que o `_execute_child` ignora. Registrado como nativo em
  `vm.rs::native_in_cpython` (sem quadro no traceback).
- `_os.spawn` ganhou o oitavo argumento (`setsid`) e o nono (`pgid_to_set`: `-1` herda, `0` grupo novo, outro entra no grupo).
  `_os.dup2`, `_os.setsid` e `_os.setpgid` são novos (usados pelo filho do `preexec_fn`).
- `os.waitpid` bloqueante é cooperativo: com threads pendentes, serviços ou pollers, espera em fatias de `WNOHANG`
  por `threading._wait_for`, como o `subprocess.py` próprio fazia (`server-thread-subprocess-client`). O
  `communicate` usa `selectors.PollSelector` sobre o `select.poll`, que já cede às threads.

## Como cada argumento chega ao filho

| argumento | como |
|---|---|
| `cwd` | validado no pai (`stat`, `S_ISDIR`, `access`); caminho relativo do executável vira relativo ao cwd |
| `env`, `restore_signals`, `start_new_session`, `process_group` | atributos do `spawn` |
| `umask` | o pai troca a sua enquanto cria o filho e devolve |
| `user`, `group`, `extra_groups` | o pai assume as credenciais (`setresuid`, `setresgid`, `setgroups`) enquanto cria o filho e devolve; sem privilégio vira `EPERM` |
| `pass_fds`, `close_fds` | `Dup2(fd, fd)` tira o `FD_CLOEXEC`; fecha o resto acima de 2 |
| `preexec_fn` | `fork` (com `register_at_fork`, sem o aviso de threads) e `execve` no filho; depende das regras do `os.fork` (laço mais externo) |

## `os.posix_spawn` e o caminho do `subprocess.py`

`os.posix_spawn` e `os.posix_spawnp` existem (`py/os.py`, sobre `_os.posix_spawn` em `modules/osspawn.rs` e o `Sys::spawn`
do kernel), então o `_USE_POSIX_SPAWN` do `subprocess.py` do disco vale (`confstr('CS_GNU_LIBC_VERSION')` é `glibc 2.41`
e `os.POSIX_SPAWN_CLOSEFROM` existe). O `Popen` passa pelo `posix_spawn`, como no Debian, quando todas estas condições
valem (as de `_execute_child`):

- o `executable` tem diretório no nome (`/bin/sh`, `./x`; um nome sem barra, como `echo`, segue pelo `fork_exec`);
- sem `preexec_fn`, `pass_fds`, `cwd`, `start_new_session`, `process_group`, `user`, `group`, `extra_groups` e `umask`;
- `close_fds` falso ou, como a 3.13 tem `POSIX_SPAWN_CLOSEFROM`, também verdadeiro (o padrão);
- os fds de stdio do filho (`p2cread`, `c2pwrite`, `errwrite`) valem `-1` ou mais que 2.

`shell=True` entra (o executável é `/bin/sh`). O resultado é o mesmo do `fork_exec`: as ações de arquivo são `close` das
pontas do pai, `dup2` das do filho e `closefrom(3)`; `restore_signals` vira `setsigdef=[SIGPIPE, SIGXFSZ]`; uma falha de
`exec` volta como `OSError` com o nome do programa (`FileNotFoundError: [Errno 2] ...: '/x'`), igual à mensagem que o
`_execute_child` monta no outro caminho. Na própria API:

- argumentos e mensagens de erro seguem o `py_posix_spawn` (`argv must not be empty`, `Unknown file_actions identifier`,
  `A dup2 file_action tuple must have 3 elements`, `signal number N out of range [1; 64]`, `must have a sched_param object`);
  fd negativo numa ação é `OSError(EBADF)` sem nome de arquivo, e o que falha no filho leva o nome do programa;
- `posix_spawnp` procura no `PATH` do próprio processo (`/bin:/usr/bin` sem `PATH`) como o `__execvpex`: segue em
  `ENOENT`, `ESTALE`, `ENOTDIR`, `ENODEV`, `ETIMEDOUT` e devolve `EACCES` se alguma entrada negou o acesso. Um
  executável sem `#!` dá `ENOEXEC` (o glibc novo não cai no `/bin/sh`);
- no kernel (`ProcAttrs`): `FdAction::CloseFrom`, `reset_ids` (uid e gid efetivos viram os reais) e `scheduler`
  (`sched_setscheduler` ou `sched_setparam` do filho, com os limites dele; a falha desfaz o processo). O fd da ação
  `Open` nunca herda `FD_CLOEXEC`, e `Close` de fd fora de `RLIMIT_NOFILE` é `EBADF`.

## `fcntl`, `ioctl`, travas e tamanho do pipe

`fcntl.py` é o `fcntlmodule.c` em Python sobre `_os.fcntl`, `_os.fcntl_buffer`, `_os.flock` e `_os.ioctl`
(`modules/osfcntl.rs`):

- `fcntl.fcntl` com inteiro: `F_DUPFD`, `F_DUPFD_CLOEXEC`, `F_GETFD`, `F_SETFD`, `F_GETFL` (agora com `O_LARGEFILE`
  nos arquivos e dispositivos, como o `f_flags`), `F_SETFL`, `F_GETPIPE_SZ`, `F_SETPIPE_SZ`; comando que o kernel não
  conhece é `EINVAL`. Com `bytes` (até 1024) devolve os bytes; `F_GETLK`, `F_SETLK`, `F_SETLKW` e `F_OFD_*` leem e
  escrevem o `struct flock` de 32 bytes. `ioctl` cobre `FIONREAD`, `FIONBIO`, `TIOCGWINSZ` e `TIOCSWINSZ` (os que
  levam ponteiro dão `EFAULT` com argumento inteiro; qualquer outro pedido é `ENOTTY`).
- Travas (`sandbox.rs::LockTable`): três famílias que não se enxergam, POSIX (dono = processo, solta no `close` de
  qualquer fd do arquivo, no `exec` e na saída), OFD (dono = descrição) e `flock` (dono = descrição; a conversão
  solta a trava antes de ver o conflito). POSIX e OFD conflitam entre si, até no mesmo processo. `F_SETLKW` detecta
  impasse entre processos (`EDEADLK`, até 11 elos, como `posix_locks_deadlock`). `whence`, `l_len` negativo e os
  `EINVAL`/`EOVERFLOW` seguem o `flock_to_posix_lock`; `LOCK_MAND` é ignorado (0) como no 6.12.
- Pipe (`pipe.rs`): capacidade de 65536 até um `F_SETPIPE_SZ`; arredonda à potência de 2 que cobre o pedido (mínimo
  4096), `EINVAL` acima de 2^31, `EPERM` além de `/proc/sys/fs/pipe-max-size` (1048576, gravável) sem ser root,
  `EBUSY` se as páginas ocupadas não cabem. `Popen(pipesize=...)` funciona por aí.
- `FIONREAD`: arquivo regular (`tamanho - posição`, negativo depois do fim), pipe/FIFO, TCP, Unix (fluxo, datagrama,
  seqpacket), UDP e pty; `ENOTTY` no resto, `EINVAL` em socket que escuta.
- Testes: `crates/kernel/tests/fcntl.rs` (kernel) e `fcntl_ioctl_locks_and_posix_spawn_over_the_kernel` em `stdlib_tests.rs`.

## Lacunas conhecidas

- `setsigmask` do `posix_spawn` é validado e ignorado: o kernel ainda não tem máscara de sinais por thread
  (`signal.pthread_sigmask` também é vazio), então o filho nunca herda sinais bloqueados.
- A cota de páginas de pipe por usuário (`pipe-user-pages-soft` e `-hard`) não é contada: só `pipe-max-size` limita.
- `posix_spawnp` refaz as ações de arquivo a cada entrada do `PATH` que falha no `exec`; só importa se uma ação
  `Open` tem efeito colateral (`O_CREAT`, `O_TRUNC`), e o resultado final é o mesmo.
- `FIONREAD` de arquivo especial que não é pty (`/dev/null`...) e de `epoll`/`pidfd` é `ENOTTY`, como no Linux; outros
  `ioctl` (`TCGETS`...) seguem pelo módulo `termios`.
- `user`/`group` por credencial temporária do pai é uma aproximação; o certo é `ProcAttrs` levar uid, gid e grupos.
