# `os` e `posix`: o que medir no oráculo

A família `sysconf`, `confstr`, `pathconf`, `times`, `sched_*`, `eventfd`, `timerfd`, `memfd_create`, `*xattr`,
`chroot` e as chamadas vetoriais entrou em `crates/ul-python/src/modules/py/os.py` (Python) e `osextra.rs` (as
chamadas de sistema). As tabelas `sysconf_names`, os valores fixos e os números fora da tabela foram conferidos com
o `python3` 3.13.5 de um Debian 13 (glibc 2.41); a lista de nomes de `os` e `posix` bate com a dele.

O que segue depende do contêiner do oráculo (limites, CPU, cgroup, capabilities) e precisa ser medido lá antes de
confiar. Cada item diz o que o sandbox devolve hoje.

## `sysconf`

| Chamada | Hoje | O que medir |
|---|---|---|
| `SC_ARG_MAX` | `max(131072, RLIMIT_STACK / 4)`; pilha ilimitada dá 4611686018427387903 | com `ulimit -s unlimited` e com `ulimit -s 1024` |
| `SC_CHILD_MAX` | `RLIMIT_NPROC` (ilimitado vira `-1`) | no contêiner padrão |
| `SC_SIGQUEUE_MAX` | `RLIMIT_SIGPENDING` (ilimitado vira `-1`) | idem |
| `SC_OPEN_MAX` | `RLIMIT_NOFILE` | idem (o docker costuma dar 1048576) |
| `SC_NPROCESSORS_CONF`, `_ONLN` | linhas `cpuN` do `/proc/stat` | com `--cpus` e com `--cpuset-cpus`: a glibc ignora o cgroup |
| `SC_PHYS_PAGES`, `SC_AVPHYS_PAGES` | `MemTotal` e `MemFree` do `/proc/meminfo` / 4096 | com `-m` (a glibc lê o `sysinfo`, que ignora o cgroup) |
| `SC_MINSIGSTKSZ` (249) e o número 250 (`_SC_SIGSTKSZ`) | 3376 e 13504, os da máquina de referência | variam com a CPU (`AT_MINSIGSTKSZ`) |
| números 185 a 197 (`_SC_LEVEL*_CACHE_*`) | os da máquina de referência | variam com a CPU |
| `SC_PASS_MAX` | 8192 | confirmar no contêiner |

`os.cpu_count()` é `SC_NPROCESSORS_ONLN`; `os.process_cpu_count()` é `len(os.sched_getaffinity(0))`.
`os.getloadavg()` lê o `/proc/loadavg`; `os.times()` usa `getrusage` (arredondado a 10 ms) e o `/proc/uptime` para o
`elapsed`: medir o ponto de partida do `elapsed` (o `times(2)` real devolve tiques desde um instante arbitrário).

## Erros e valores de uma chamada só

- `os.getlogin()` segue como estava (`$USER` ou `root`). Na máquina de referência, sem terminal, devolve
  `OSError: [Errno -25] Unknown error -25`; medir no contêiner com e sem terminal.
- `os.timerfd_create(clock, flags=0)`: o sandbox repassa `flags` sem somar `TFD_CLOEXEC`; medir
  `os.get_inheritable(fd)` do fd devolvido.
- `os.lseek` num eventfd, timerfd, epoll e pidfd: o sandbox devolve ESPIPE (como já fazia com epoll e pidfd); o
  `noop_llseek` do kernel pode devolver 0.
- `os.memfd_create`: `st_nlink` do arquivo (o sandbox devolve 0 por causa do `unlink` interno; o Linux deve dar 1),
  nome com `/` (o sandbox troca por `_`; o Linux mostra `/memfd:a/b (deleted)`), `MFD_HUGETLB` (EINVAL aqui).
- Atributos estendidos (`setxattr` e família): só o tmpfs guarda, e só `user.*`. Medir no overlay do docker:
  `user.*` em arquivo e diretório, em symlink (EPERM aqui), `trusted.*` e `security.*` (EPERM aqui, sem
  `CAP_SYS_ADMIN`), nome sem prefixo (EOPNOTSUPP), limite de 128 atributos (ENOSPC aqui), `listxattr` em fs sem
  suporte (vazio aqui). A mensagem de erro de um fd mostra o número do fd sem aspas no CPython; aqui sai com aspas.
- `os.chroot`: o sandbox exige uid efetivo 0 (EPERM senão) e troca a raiz do processo; medir como root e como
  usuário comum, e o efeito no `/proc/self/root`.
- `os.unshare` (EPERM aqui para qualquer flag) e `os.setns` (EBADF para fd inválido, EINVAL para fd comum): medir
  com flags inválidas e com `CLONE_NEWUSER`.
- `os.nice(-n)` e `os.setpriority` abaixo da prioridade atual: EPERM e EACCES conforme o kernel do sandbox já
  modelava; medir `os.nice(-1)` como root e como usuário comum.
- `os.lockf(fd, F_TEST, n)`: o sandbox usa `F_GETLK` do `fcntl`; medir com a trava de outro processo.
- `os.posix_fallocate` em tmpfs e em overlay (a glibc cai para escrita se o kernel devolve EOPNOTSUPP).
- `os.sendfile`, `os.copy_file_range` e `os.splice` são implementados com `read`/`write` (e `pread`/`pwrite`): a
  contagem devolvida em fd não bloqueante e entre sistemas de arquivos diferentes pode divergir do kernel.

## Ainda ausente

- `os.lchmod`, `os.chflags`, `os.add_dll_directory` não existem no Linux e ficam ausentes, como no CPython.
- `os.posix_spawn`, `os.dup` e `fcntl` têm dono próprio (outros agentes).
