# API pública do kernel

Tudo que o host (daemon) e a conformidade (`pl-testing`) usam. Todas as chamadas são de threads do host,
nunca de dentro de um pseudo-processo. Erros são `sysabi::Errno` com o número e a mensagem do Linux.

## Kernel

```rust
let k = kernel::Kernel::new(KernelConfig {
    cpus: 2,                    // CPUs virtuais (tokens) do escalonador; sched_getaffinity devolve 0..cpus
    stack_size: 2 << 20,        // pilha da thread de cada pseudo-processo (2 MiB, só virtual até tocar)
    spawner_hook: None,         // Option<Arc<dyn Fn(&SpawnerInfo) -> Result<(), String> + Send + Sync>>
});
let sb = k.create_sandbox(SandboxConfig { programs, ..SandboxConfig::default() })?;   // Result<Sandbox, CreateError>
let sb2 = k.create_sandbox_from(&snapshot, cfg)?;                                       // FS derivado, O(1)
```

`SandboxConfig` (todos com padrão):

| Campo | Padrão | Uso |
|---|---|---|
| `programs: Vec<sysabi::Program>` | vazio | um executável por programa em `dir/name` |
| `hostname: String` | `"sandbox"` | `uname -n`, `/etc/hostname`, `/etc/hosts` |
| `env: Vec<Vec<u8>>` | `kernel::BASE_ENV` (o da bancada) | ambiente dos processos que o host cria |
| `cwd: Vec<u8>` | `/root` | cwd dos processos que o host cria |
| `clock: ClockMode` | `Host` | `Fixed(TimeSpec)` = `faketime -f`; `StartAt(TimeSpec)` = começa ali e anda |
| `limits: SandboxLimits` | sem limites; `nofile` 1073741816; `fsize` infinito | `max_procs` (EAGAIN no fork), `fs_bytes` e `fs_inodes` (ENOSPC no tmpfs da raiz), `nofile` e `fsize` (rlimits iniciais), `mem_bytes` (marco 3) |
| `uid`, `gid` | 0, 0 | credenciais dos processos que o host cria |

`CreateError::Spawner(String)` quando o hook da thread spawner falha; `CreateError::Errno(e)` no resto.

### Thread spawner (E06)

Cada sandbox tem uma thread spawner criada no `create_sandbox`. O hook do `KernelConfig` roda nela uma
vez, antes de qualquer processo, com `SpawnerInfo { sandbox_id, host_mounts }`. As threads dos processos
que o host cria nascem da spawner (por canal). Threads criadas de dentro do sandbox (fork, spawn,
`spawn_thread`) nascem direto da thread do processo que pediu, que descende da spawner e por isso já herda
o Landlock e o seccomp: o efeito é o mesmo, sem a ida e volta pelo canal. Nenhum trabalho de
pseudo-processo vai pra pool global. Threads internas do kernel (timer, watchdog, rede) nunca nascem da
spawner.

## Processos a partir do host

```rust
// Atalho síncrono: argv[0] sem barra é procurado no PATH do ambiente (como execvp).
let out: RunOutput = sb.run(RunRequest {
    argv: vec![b"bash".to_vec(), b"-c".to_vec(), b"ls | sort".to_vec()],
    env: None,                  // None = SandboxConfig::env
    cwd: Some(b"/work/case".to_vec()),
    stdin: b"...".to_vec(),
    timeout: Some(Duration::from_secs(10)),
})?;
// out.stdout, out.stderr, out.status: WaitStatus, out.timed_out, out.rusage
```

`run` volta quando o processo principal termina (saída de processo em segundo plano que ainda segure o
pipe depois disso não é esperada). No timeout, SIGKILL em todos os processos da sessão do processo.
Programa inexistente: `Err(ENOENT)`; sem permissão de execução: `Err(EACCES)`; formato desconhecido:
`Err(ENOEXEC)` (o `#!` é tratado pelo kernel).

Streaming:

```rust
let sp: Spawned = sb.spawn(SpawnSpec { path, argv, attrs: ProcAttrs { env, cwd, fd_actions, .. } },
                           HostStdio { stdin: StdioSpec::Pipe, stdout: StdioSpec::Pipe, stderr: StdioSpec::Null })?;
sp.pid;                                   // pgid = sid = pid: todo processo criado pelo host abre sessão nova
sp.stdin: Option<HostWriter>              // write_all(&data, deadline) / try_write(&data); drop = EOF
sp.stdout: Option<HostReader>             // read(&mut buf, deadline) -> Data(n) | Eof | Timeout; try_read; read_to_end
sb.wait(pid, Some(deadline))?             // Option<(WaitStatus, Rusage)>; None = prazo vencido; colhe o zumbi
sb.kill(KillTarget::Group(pgid), Signal::SIGKILL)?   // Pid(p), Group(g), All (todos menos o init)
sb.kill_all();
sb.processes() -> Vec<ProcInfo>           // pid, ppid, pgid, sid, estado (R, S, T, Z), comm; o init (pid 1) incluso
sb.process(pid) -> Option<HostProcInfo>   // + argv, CPU das threads terminadas, número de threads
sb.usage() -> Usage                       // procs vivos, fs_bytes, fs_inodes, cpu_ns
sb.set_clock(ClockMode::Fixed(ts));
```

`SpawnSpec.path` já resolvido (o kernel não procura no PATH, como o `execve`). `ProcAttrs.fd_actions`
são aplicadas depois do stdio, na ordem: `Dup2`, `Close`, `Open { fd, path, flags, mode }` (relativo ao
cwd do filho). Processos criados pelo host são filhos do init (pid 1), que não colhe os zumbis deles: só o
`sb.wait` colhe. Órfãos de dentro do sandbox são adotados pelo init e colhidos na hora.

## Sistema de arquivos direto (como root, cwd em `/`)

```rust
let fs = sb.fs();
fs.mkdir_all(b"/work/case", 0o755)?;            // modo exato, sem umask
fs.write_file(b"/work/case/a.txt", b"..", 0o644)?;   // cria ou substitui, modo exato
fs.write(path, data, WriteMode::Truncate | Append | CreateNew | At(off), mode)?;
fs.symlink(b"target", b"/work/case/link")?;
fs.set_mtime(path, TimeSpec { sec: FIXTURE_MTIME, nsec: 0 })?;   // atime e mtime, sem seguir symlink
fs.utimens(path, SetTime::At(t), SetTime::Omit, nofollow)?;
fs.read(path, offset, len)?; fs.read_file(path)?;
fs.readdir(path)? -> Vec<FsEntry { name, ino, kind }>     // ordem do tmpfs: mais novo primeiro
fs.stat(path)?; fs.lstat(path)?; fs.readlink(path)?;
fs.mkdir(path, mode)?; fs.unlink(path)?; fs.rmdir(path)?; fs.remove_all(path)?; fs.rename(a, b)?;
fs.chmod(path, mode)?; fs.chown(path, Some(uid), None, nofollow)?;
fs.tree(b"/work/case")? -> Vec<(Vec<u8> /* relativo */, TreeEntry)>   // ordenado por caminho
//   TreeEntry::File { data, mode } | Dir { mode } | Symlink { target } | Other { mode }  (mode sem o tipo)
```

Fixture de um caso da bancada: `mkdir_all(CASE_DIR)`, depois `write_file`/`mkdir`/`symlink` de cada
entrada, depois `set_mtime(FIXTURE_MTIME)` em cada uma (e no diretório, se o caso compara o mtime dele).

## Snapshot

```rust
let snap: Snapshot = sb.snapshot();   // O(1): raiz e /dev (imbl)
sb.restore(&snap);                    // custa o que mudou; processos vivos continuam (fd de inode que
                                      // não existe no retrato dá ESTALE)
let sb2 = k.create_sandbox_from(&snap, cfg)?;   // sandbox nova com o FS do retrato
```

## Destruir

`drop(sb)` (ou `sb.destroy()`): SIGKILL em tudo, espera as threads saírem (até 2 s; uma thread presa num
laço sem checkpoint fica pra trás, como medido no H07), solta a tabela e para a spawner.

## O que muda nos próximos marcos (assinaturas já fixadas)

- Marco 2, grupos de CPU por usuário (usuário > sandbox > processo):
  `k.create_user_group(UserGroupSpec { cpu_weight: u64, cpu_max: Option<(u64 /*quota_ns*/, u64 /*period_ns*/)> }) -> UserGroup`,
  `k.set_user_group_limits(&UserGroup, cpu_weight, cpu_max) -> Result<(), Errno>`,
  `SandboxConfig::user_group: Option<UserGroup>` e `SandboxConfig::cpu_weight`.
- Marco 3, memória: `kernel::mem::install_tracker() -> Result<(), String>` (o host declara
  `#[global_allocator] static A: tracking_allocator::Allocator<mimalloc::MiMalloc>` e chama no início),
  `SandboxLimits::mem_bytes` passa a valer (SIGKILL no checkpoint seguinte ao estouro), `Usage::mem_bytes`.
- Marco 3: `net_connect` com allowlist (`SandboxConfig::net: NetPolicy`), hostfs (`SandboxConfig::mounts`),
  `/proc` completo.
