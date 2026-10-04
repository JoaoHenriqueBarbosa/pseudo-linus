# sysio

Fachada com a forma do std sobre as syscalls do `sysabi`. Serve pra portar programas Rust pro
pseudo-linus trocando o caminho dos imports (`std::fs` vira `sysio::fs`, `std::io::stdout` vira
`sysio::io::stdout`...). Nada aqui toca o host: toda operação vira chamada em `sysabi::sys`, no
processo corrente da thread. É o sucessor do shim do F06 (`testbench/experiments/f06-coreutils-find`)
com a mesma API, agora sobre o contrato do kernel em vez de um VFS próprio.

## Ponto de entrada

Todo programa entra por `sysio::run`:

```rust
fn cat_main(_ctx: &mut sysabi::Ctx, args: &[std::ffi::OsString]) -> i32 {
    sysio::run(|| uu_cat::uumain(args.iter().cloned()))
}
```

`run` abre o estado de userland do processo (o que num programa C mora na libc: buffer do stdout,
buffer do stdin, código de saída do uucore, `proc_local`) e, no fim:

- descarrega o stdout; se falhar, escreve `prog: write error: <strerror>` e sai com 1, como o
  `close_stdout` do gnulib;
- `process::exit` no meio também descarrega (é o `exit(3)`);
- morte por sinal e `execve` não descarregam (o buffer some com a imagem do processo, como no
  Linux).

Fora de um `run` (código que roda num processo sem passar por um `main`, como o corpo de um
`spawn_fn` do shell), o primeiro acesso cria um estado implícito com stdout sem buffer.

## Módulos

| std | sysio | observação |
|---|---|---|
| `std::fs` | `sysio::fs` | `File` é dono de um fd do pseudo-processo; `read_dir` usa `getdents` e o `DirEntry::metadata` faz `fstatat` no fd do diretório, como o std. `canonicalize` é o `realpath(3)` da glibc (ENOENT, ENOTDIR, ELOOP depois de 40 links). `remove_dir_all` usa as syscalls `*at` sem seguir link. Extras: `exists`, `is_dir`, `is_file`, `is_symlink`, `try_exists`, `set_times`, `utimensat`, `chown`, `lchown`, `fchown`, `chroot` (resolve o caminho e devolve EPERM), `mkfifo`, `mknod`, `statfs`, `fstatfs`, `access`, `eaccess`, `read_link_bytes`. |
| métodos de `Path` (`exists()`, `is_dir()`, `metadata()`, `canonicalize()`...) | `sysio::path::PathExt` | `p.sys_exists()`, `p.sys_is_dir()`... Método inerente ganha de trait, então o porte troca o nome; um grep por `\.exists()` acha o que faltou. |
| `std::io` | `sysio::io` | Reexporta tudo de `std::io` e sombreia `stdin`, `stdout`, `stderr`, `Stdin`, `StdinLock`, `Stdout`, `StdoutLock`, `Stderr`, `StderrLock`, `IsTerminal`. |
| `std::env` | `sysio::env` | `args_os`, `var`, `vars_os`, `set_var` (sem `unsafe`: o ambiente é do processo, no kernel), `current_dir`, `set_current_dir`, `temp_dir`, `home_dir`, `environ_raw`. |
| `std::process` | `sysio::process` | `exit`, `abort`, `id`, `Command` (com `spawn`, `status`, `output`, `env`, `env_clear`, `current_dir`, `stdin/stdout/stderr`), `Child` (`wait`, `try_wait`, `kill`, `wait_with_output`), `Stdio`, `ExitStatus`, `Output`, `CommandExt` (`exec`, `arg0`, `process_group`), `ExitStatusExt`, `resolve_in_path`, `get_umask`. |
| `std::time` | `sysio::time` | `now()` (`CLOCK_REALTIME` do pseudo-kernel), `Instant` (`CLOCK_MONOTONIC`), `sleep`, `sleep_interruptible`. Os tipos puros (`SystemTime`, `UNIX_EPOCH`, `Duration`) são os do std. |
| `std::thread` | `sysio::thread` | `spawn` e `JoinHandle` sobre `spawn_thread`/`join_thread` do kernel (a thread entra no escalonador, herda o estado de userland, `exit` nela termina o processo inteiro, panic comum volta no `join`), `sleep`, `yield_now`, `available_parallelism` (`sched_getaffinity`: CPUs virtuais do sandbox). |
| `std::os::unix::fs` | `sysio::os::unix::fs` | `MetadataExt`, `PermissionsExt`, `FileTypeExt`, `DirEntryExt`, `OpenOptionsExt`, `DirBuilderExt`, `FileExt`, `symlink`, `chown`, `lchown`, `fchown`, `chroot`, `mkfifo`. |
| `std::os::fd` | `sysio::os::fd` | `RawFd`, `AsRawFd`, `FromRawFd`, `IntoRawFd`, `AsFd`, `BorrowedFd`, `OwnedFd`. `from_raw_fd` e `borrow_raw` são seguros aqui (um fd errado só erra de fd do pseudo-processo). `AsFd` traz `fstat()`, `seek_position()` e `is_appending()`; `sysio::fs::Fstat` é o mesmo trait (nome do F06). |
| `std::os::unix::process` | `sysio::os::unix::process` | `CommandExt`, `ExitStatusExt`. |
| `print!`, `println!`, `eprint!`, `eprintln!` | `sysio::print!`... | Mesma sintaxe. |
| `libc`/`nix`/`rustix` avulsos | `sysio::unistd`, `sysio::users`, `sysio::random`, `sysio::errno` | `uname`, `gethostname`, `isatty`, `ttyname`, `winsize`, `getpriority`, `kill`, `signal`, `getrlimit`, `sync`, `dup2`, `checkpoint`; `/etc/passwd` e `/etc/group` do pseudo-linus; `getrandom`; constantes de errno e `strerror`. |
| `std::fs`/`io` crus | `sysio::sysabi` | O contrato inteiro, pra syscall que a fachada não embrulha. |

## Comportamentos que importam pra fidelidade

- **Bufferização do stdout igual à da glibc**, não à do std: por linha quando o fd 1 é terminal,
  em bloco de 4096 bytes quando é pipe ou arquivo (o std usa sempre por linha). É o que faz
  `head a falta b 2>&1` sair na mesma ordem do GNU (o erro antes do conteúdo). Programa que no GNU
  escreve direto com `write(2)` (cat, tee, yes, dd, tail -f) deve chamar `flush()` nos mesmos
  pontos.
- **Antes de criar filho e de `exec`**, o stdout é descarregado (`Command::spawn`, `status`,
  `output`, `CommandExt::exec`), pra saída do pai não ficar atrás da do filho.
- **stderr sem buffer, uma escrita por mensagem**: `eprintln!`/`writeln!(stderr())` montam a linha
  inteira antes do `write`, como o `fprintf` da glibc num fluxo sem buffer.
- **Erros**: `sysio::errno::strerror(&e)` dá a mensagem da glibc 2.41 (tabela do `sysabi`). Nunca use
  o `Display` do `io::Error` em mensagem pro usuário (ele acrescenta " (os error N)").
- **Busca no PATH do `Command`** = `execvp`: pula ENOENT/ENOTDIR, lembra de EACCES, roda com
  `/bin/sh` o executável sem `#!`. Sem `PATH`, usa `/bin:/usr/bin` (o `_CS_PATH` da glibc).
- **Ambiente do filho** com `env`/`env_remove`: mantém a ordem do pai, muda no lugar e acrescenta as
  novas no fim (o std ordena por nome).
- **`print!` não devolve erro**: se a escrita falhar, o fim do `run` avisa `write error`.

## O que não existe (ainda)

- `std::thread::Builder` não foi sombreado (use `sysio::thread::spawn`).
- `File::lock`, `copy_file_range`, `sendfile`, `splice` e xattr: os portes leem e escrevem.
- Seleção/poll de vários fds: o contrato não tem `poll`; `Child::wait_with_output` lê o stderr numa
  thread.

## Testes

`cargo test -p sysio` roda programas pequenos escritos contra a fachada no kernel de teste do
`sysabi` (feature `testkit`): FS, ordem stdout/stderr, `exit`, `Command` (herdado, capturado, pipe,
PATH, stdin de arquivo), ambiente e identidade, stdin grande, erro de escrita no fim e threads.
